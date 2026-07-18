#![forbid(unsafe_code)]
#![doc = "Native rustc differential-testing support for rustscript."]

use std::ffi::OsString;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::Duration;

use rand_chacha::ChaCha8Rng;
use rand_core::{Rng, SeedableRng};
use wait_timeout::ChildExt;

const DEFAULT_STREAM_CAP: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunConfig {
    pub seed: u64,
    pub cases: usize,
}

#[derive(Clone, Debug)]
pub struct RustcOracle {
    rustc: PathBuf,
    compile_timeout: Duration,
    run_timeout: Duration,
    stream_cap: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessCapture {
    pub success: bool,
    pub code: Option<i32>,
    pub timed_out: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OracleResult {
    pub rustc_version: String,
    pub rustc_argv: Vec<String>,
    pub compiler: ProcessCapture,
    pub native: Option<ProcessCapture>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffFailureKind {
    PrettyPrintRoundTrip,
    RustcRejectedAcceptedProgram,
    NativeTimeout,
    NativeFailure,
    StdoutMismatch,
    UnexpectedNativeStderr,
    NondeterministicInterpreter,
}

#[derive(Clone, Debug)]
pub struct DiffFailure {
    pub kind: DiffFailureKind,
    pub seed: u64,
    pub case_index: usize,
    pub source: String,
    pub canonical: String,
    pub reason: String,
    pub interpreter_stdout: Vec<u8>,
    pub oracle: Option<OracleResult>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaseSuccess {
    pub seed: u64,
    pub case_index: usize,
    pub source: String,
    pub stdout: Vec<u8>,
    pub steps: u64,
}

#[derive(Debug)]
pub enum OracleError {
    Io(io::Error),
    MissingPipe(&'static str),
    ReaderPanicked(&'static str),
}

impl std::fmt::Display for OracleError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::MissingPipe(stream) => write!(formatter, "child {stream} pipe was unavailable"),
            Self::ReaderPanicked(stream) => write!(formatter, "child {stream} reader panicked"),
        }
    }
}

impl std::error::Error for OracleError {}

impl From<io::Error> for OracleError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl RustcOracle {
    pub fn discover(explicit: Option<PathBuf>) -> Self {
        let rustc = explicit
            .or_else(|| std::env::var_os("RUSTC").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("rustc"));
        Self {
            rustc,
            compile_timeout: Duration::from_secs(10),
            run_timeout: Duration::from_secs(2),
            stream_cap: DEFAULT_STREAM_CAP,
        }
    }

    #[must_use]
    pub fn with_timeouts(mut self, compile_timeout: Duration, run_timeout: Duration) -> Self {
        self.compile_timeout = compile_timeout;
        self.run_timeout = run_timeout;
        self
    }

    #[must_use]
    pub fn with_stream_cap(mut self, bytes: usize) -> Self {
        self.stream_cap = bytes;
        self
    }

    pub fn version(&self) -> Result<String, OracleError> {
        let output = Command::new(&self.rustc).arg("-Vv").output()?;
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    pub fn run_source(&self, source: &str) -> Result<OracleResult, OracleError> {
        let directory = tempfile::tempdir()?;
        let source_path = directory.path().join("program.rs");
        let executable_path = directory
            .path()
            .join(format!("program{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&source_path, source.as_bytes())?;

        let arguments = compile_arguments(&source_path, &executable_path);
        let rustc_argv = std::iter::once(self.rustc.to_string_lossy().into_owned())
            .chain(
                arguments
                    .iter()
                    .map(|argument| argument.to_string_lossy().into_owned()),
            )
            .collect();
        let compiler = run_command(
            &self.rustc,
            &arguments,
            self.compile_timeout,
            self.stream_cap,
            false,
        )?;
        let native = if compiler.success {
            Some(run_command(
                &executable_path,
                &[],
                self.run_timeout,
                self.stream_cap,
                true,
            )?)
        } else {
            None
        };
        Ok(OracleResult {
            rustc_version: self.version()?,
            rustc_argv,
            compiler,
            native,
        })
    }
}

pub fn run_case(
    seed: u64,
    case_index: usize,
    oracle: &RustcOracle,
) -> Result<CaseSuccess, Box<DiffFailure>> {
    let mut rng = ChaCha8Rng::seed_from_u64(seed ^ (case_index as u64).rotate_left(29));
    let mut decisions = [0_u64; 32];
    for decision in &mut decisions {
        *decision = rng.next_u64();
    }
    let generated = rustscript_core::generate_checked_program(&decisions);
    let source = rustscript_core::format(&generated);
    compare_source(seed, case_index, &source, oracle, true)
}

/// Replay an arbitrary source file through the same comparison pipeline.
pub fn replay_source(source: &str, oracle: &RustcOracle) -> Result<CaseSuccess, Box<DiffFailure>> {
    compare_source(0, 0, source, oracle, false)
}

fn compare_source(
    seed: u64,
    case_index: usize,
    source: &str,
    oracle: &RustcOracle,
    require_canonical_input: bool,
) -> Result<CaseSuccess, Box<DiffFailure>> {
    let checked = rustscript_core::check_source(source, rustscript_core::Limits::default())
        .map_err(|error| {
            failure(
                DiffFailureKind::PrettyPrintRoundTrip,
                seed,
                case_index,
                source,
                source,
                format!("canonical source failed checking: {error}"),
                Vec::new(),
                None,
            )
        })?;
    let canonical = rustscript_core::format(&checked);
    let reparsed = rustscript_core::check_source(&canonical, rustscript_core::Limits::default())
        .map_err(|error| {
            failure(
                DiffFailureKind::PrettyPrintRoundTrip,
                seed,
                case_index,
                source,
                &canonical,
                format!("formatted source failed checking: {error}"),
                Vec::new(),
                None,
            )
        })?;
    if rustscript_core::format(&reparsed) != canonical
        || (require_canonical_input && canonical != source)
    {
        return Err(failure(
            DiffFailureKind::PrettyPrintRoundTrip,
            seed,
            case_index,
            source,
            &canonical,
            "canonical emitter was not idempotent".into(),
            Vec::new(),
            None,
        ));
    }
    let limits = rustscript_core::RuntimeLimits::default();
    let first = rustscript_core::run(&checked, limits).map_err(|error| {
        failure(
            DiffFailureKind::PrettyPrintRoundTrip,
            seed,
            case_index,
            source,
            &canonical,
            format!("generated program trapped: {error}"),
            Vec::new(),
            None,
        )
    })?;
    let second = rustscript_core::run(&checked, limits).map_err(|error| {
        failure(
            DiffFailureKind::NondeterministicInterpreter,
            seed,
            case_index,
            source,
            &canonical,
            format!("second interpreter run failed: {error}"),
            first.output.clone(),
            None,
        )
    })?;
    if first != second {
        return Err(failure(
            DiffFailureKind::NondeterministicInterpreter,
            seed,
            case_index,
            source,
            &canonical,
            "repeated interpreter runs differed".into(),
            first.output,
            None,
        ));
    }
    let result = oracle.run_source(&canonical).map_err(|error| {
        failure(
            DiffFailureKind::NativeFailure,
            seed,
            case_index,
            source,
            &canonical,
            format!("oracle failed: {error}"),
            first.output.clone(),
            None,
        )
    })?;
    if !result.compiler.success {
        return Err(failure(
            DiffFailureKind::RustcRejectedAcceptedProgram,
            seed,
            case_index,
            source,
            &canonical,
            "rustc rejected an accepted program".into(),
            first.output,
            Some(result),
        ));
    }
    let Some(native) = result.native.as_ref() else {
        return Err(failure(
            DiffFailureKind::NativeFailure,
            seed,
            case_index,
            source,
            &canonical,
            "native result was missing".into(),
            first.output,
            Some(result),
        ));
    };
    if native.timed_out {
        return Err(failure(
            DiffFailureKind::NativeTimeout,
            seed,
            case_index,
            source,
            &canonical,
            "native program timed out".into(),
            first.output,
            Some(result),
        ));
    }
    if !native.success {
        return Err(failure(
            DiffFailureKind::NativeFailure,
            seed,
            case_index,
            source,
            &canonical,
            "native program failed".into(),
            first.output,
            Some(result),
        ));
    }
    if !native.stderr.is_empty() {
        return Err(failure(
            DiffFailureKind::UnexpectedNativeStderr,
            seed,
            case_index,
            source,
            &canonical,
            "native program wrote stderr".into(),
            first.output,
            Some(result),
        ));
    }
    if native.stdout != first.output {
        return Err(failure(
            DiffFailureKind::StdoutMismatch,
            seed,
            case_index,
            source,
            &canonical,
            "native and interpreter stdout differed".into(),
            first.output,
            Some(result),
        ));
    }
    Ok(CaseSuccess {
        seed,
        case_index,
        source: source.into(),
        stdout: first.output,
        steps: first.steps,
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "failure artifacts require the complete case context"
)]
fn failure(
    kind: DiffFailureKind,
    seed: u64,
    case_index: usize,
    source: &str,
    canonical: &str,
    reason: String,
    interpreter_stdout: Vec<u8>,
    oracle: Option<OracleResult>,
) -> Box<DiffFailure> {
    Box::new(DiffFailure {
        kind,
        seed,
        case_index,
        source: source.into(),
        canonical: canonical.into(),
        reason,
        interpreter_stdout,
        oracle,
    })
}

/// Persist a complete, non-overwriting differential failure artifact.
pub fn write_failure_artifact(failure: &DiffFailure, root: &Path) -> io::Result<PathBuf> {
    let directory = unique_artifact_directory(root, failure.seed, failure.case_index)?;
    write_file(&directory, "failure.rs", failure.source.as_bytes())?;
    write_file(&directory, "canonical.rs", failure.canonical.as_bytes())?;
    // The initial reducer candidate is canonical and remains reproducible. Later
    // reductions replace this file only after preserving the failure category.
    write_file(&directory, "minimized.rs", failure.canonical.as_bytes())?;
    let ast = rustscript_core::check_source(&failure.canonical, rustscript_core::Limits::default())
        .map(|program| rustscript_core::debug_ir(&program))
        .unwrap_or_else(|error| format!("checked IR unavailable: {error}"));
    write_file(&directory, "ast.txt", ast.as_bytes())?;
    write_file(
        &directory,
        "interpreter.stdout",
        &failure.interpreter_stdout,
    )?;

    let mut metadata = format!(
        "seed={}\ncase={}\ncategory={:?}\nreason={}\nhost_os={}\nhost_arch={}\nreproduce=cargo run -p rustscript-difftest -- --seed {} --case {}\n",
        failure.seed,
        failure.case_index,
        failure.kind,
        failure.reason,
        std::env::consts::OS,
        std::env::consts::ARCH,
        failure.seed,
        failure.case_index,
    );
    if let Some(oracle) = &failure.oracle {
        metadata.push_str("rustc_version=\n");
        metadata.push_str(&oracle.rustc_version);
        metadata.push_str("rustc_argv=");
        metadata.push_str(&oracle.rustc_argv.join(" "));
        metadata.push('\n');
        write_file(&directory, "compiler.stdout", &oracle.compiler.stdout)?;
        write_file(&directory, "compiler.stderr", &oracle.compiler.stderr)?;
        if let Some(native) = &oracle.native {
            write_file(&directory, "native.stdout", &native.stdout)?;
            write_file(&directory, "native.stderr", &native.stderr)?;
        } else {
            write_file(&directory, "native.stdout", &[])?;
            write_file(&directory, "native.stderr", &[])?;
        }
    } else {
        for name in [
            "compiler.stdout",
            "compiler.stderr",
            "native.stdout",
            "native.stderr",
        ] {
            write_file(&directory, name, &[])?;
        }
    }
    write_file(&directory, "metadata.txt", metadata.as_bytes())?;
    Ok(directory)
}

/// Persist a successful case when the runner's `--keep-all` option is active.
pub fn write_success_artifact(success: &CaseSuccess, root: &Path) -> io::Result<PathBuf> {
    let directory = unique_artifact_directory(root, success.seed, success.case_index)?;
    write_file(&directory, "canonical.rs", success.source.as_bytes())?;
    write_file(&directory, "interpreter.stdout", &success.stdout)?;
    write_file(
        &directory,
        "metadata.txt",
        format!(
            "seed={}\ncase={}\nsteps={}\n",
            success.seed, success.case_index, success.steps
        )
        .as_bytes(),
    )?;
    Ok(directory)
}

fn unique_artifact_directory(root: &Path, seed: u64, case_index: usize) -> io::Result<PathBuf> {
    std::fs::create_dir_all(root)?;
    let stem = format!("seed-{seed}-case-{case_index}");
    let mut suffix = 0_u64;
    loop {
        let name = if suffix == 0 {
            stem.clone()
        } else {
            format!("{stem}-{suffix}")
        };
        let path = root.join(name);
        match std::fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        suffix = suffix
            .checked_add(1)
            .ok_or_else(|| io::Error::other("artifact suffix space exhausted"))?;
    }
}

fn write_file(directory: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
    std::fs::write(directory.join(name), bytes)
}

fn compile_arguments(source: &Path, executable: &Path) -> Vec<OsString> {
    [
        OsString::from("--edition=2024"),
        OsString::from("--crate-type=bin"),
        OsString::from("-C"),
        OsString::from("overflow-checks=yes"),
        OsString::from("-C"),
        OsString::from("panic=abort"),
        OsString::from("-C"),
        OsString::from("debuginfo=0"),
        OsString::from("-A"),
        OsString::from("warnings"),
        OsString::from("--color=never"),
        source.as_os_str().to_owned(),
        OsString::from("-o"),
        executable.as_os_str().to_owned(),
    ]
    .into()
}

fn run_command(
    program: &Path,
    arguments: &[OsString],
    timeout: Duration,
    cap: usize,
    native: bool,
) -> Result<ProcessCapture, OracleError> {
    let mut command = Command::new(program);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if native {
        command.env("RUST_BACKTRACE", "0");
    }
    let child = command.spawn()?;
    capture_child(child, timeout, cap)
}

fn capture_child(
    mut child: Child,
    timeout: Duration,
    cap: usize,
) -> Result<ProcessCapture, OracleError> {
    let stdout = child
        .stdout
        .take()
        .ok_or(OracleError::MissingPipe("stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or(OracleError::MissingPipe("stderr"))?;
    let stdout_reader = thread::spawn(move || read_capped(stdout, cap));
    let stderr_reader = thread::spawn(move || read_capped(stderr, cap));
    let (status, timed_out) = match child.wait_timeout(timeout)? {
        Some(status) => (status, false),
        None => {
            let _ = child.kill();
            (child.wait()?, true)
        }
    };
    let (stdout, stdout_truncated) = join_reader(stdout_reader, "stdout")?;
    let (stderr, stderr_truncated) = join_reader(stderr_reader, "stderr")?;
    Ok(capture(
        status,
        timed_out,
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
    ))
}

fn read_capped(mut reader: impl Read, cap: usize) -> io::Result<(Vec<u8>, bool)> {
    let mut stored = Vec::with_capacity(cap.min(8192));
    let mut truncated = false;
    let mut buffer = [0_u8; 8192];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let remaining = cap.saturating_sub(stored.len());
        let keep = remaining.min(read);
        stored.extend_from_slice(&buffer[..keep]);
        truncated |= keep != read;
    }
    Ok((stored, truncated))
}

fn join_reader(
    handle: thread::JoinHandle<io::Result<(Vec<u8>, bool)>>,
    name: &'static str,
) -> Result<(Vec<u8>, bool), OracleError> {
    handle
        .join()
        .map_err(|_| OracleError::ReaderPanicked(name))?
        .map_err(OracleError::Io)
}

fn capture(
    status: ExitStatus,
    timed_out: bool,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    stdout_truncated: bool,
    stderr_truncated: bool,
) -> ProcessCapture {
    ProcessCapture {
        success: status.success() && !timed_out,
        code: status.code(),
        timed_out,
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiles_and_runs_without_a_shell() {
        let result = RustcOracle::discover(None)
            .run_source("fn main() { println!(\"ok\"); }")
            .unwrap();
        assert!(
            result.compiler.success,
            "{}",
            String::from_utf8_lossy(&result.compiler.stderr)
        );
        let native = result.native.unwrap();
        assert!(native.success);
        assert_eq!(native.stdout, b"ok\n");
        assert!(native.stderr.is_empty());
    }

    #[test]
    fn captures_compiler_failure_separately() {
        let result = RustcOracle::discover(None).run_source("not rust").unwrap();
        assert!(!result.compiler.success);
        assert!(result.native.is_none());
    }

    #[test]
    fn kills_and_reaps_timed_out_native_process() {
        let oracle = RustcOracle::discover(None)
            .with_timeouts(Duration::from_secs(10), Duration::from_millis(50));
        let result = oracle.run_source("fn main() { loop {} }").unwrap();
        let native = result.native.unwrap();
        assert!(native.timed_out);
        assert!(!native.success);
    }

    #[test]
    fn deterministic_twenty_five_case_differential_smoke_corpus() {
        let oracle = RustcOracle::discover(None);
        for case_index in 0..25 {
            if let Err(failure) = run_case(1, case_index, &oracle) {
                panic!(
                    "seed 1 case {case_index} failed: {:?}: {}",
                    failure.kind, failure.reason
                );
            }
        }
    }

    #[test]
    fn failure_artifacts_are_complete_and_non_overwriting() {
        let root = tempfile::tempdir().unwrap();
        let failure = DiffFailure {
            kind: DiffFailureKind::StdoutMismatch,
            seed: 7,
            case_index: 3,
            source: "fn main() {}".into(),
            canonical: "fn main() {}\n".into(),
            reason: "test mismatch".into(),
            interpreter_stdout: b"interpreter".to_vec(),
            oracle: None,
        };
        let first = write_failure_artifact(&failure, root.path()).unwrap();
        let second = write_failure_artifact(&failure, root.path()).unwrap();
        assert_ne!(first, second);
        for name in [
            "failure.rs",
            "canonical.rs",
            "minimized.rs",
            "ast.txt",
            "metadata.txt",
            "interpreter.stdout",
            "native.stdout",
            "native.stderr",
            "compiler.stdout",
            "compiler.stderr",
        ] {
            assert!(first.join(name).is_file(), "missing {name}");
        }
    }
}
