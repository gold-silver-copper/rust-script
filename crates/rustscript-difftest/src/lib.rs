#![forbid(unsafe_code)]
#![doc = "Native rustc differential-testing support for rustscript."]

use std::ffi::OsString;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use rand_chacha::ChaCha8Rng;
use rand_core::{Rng, SeedableRng};

const DEFAULT_STREAM_CAP: usize = 1024 * 1024;
const READER_DRAIN_TIMEOUT: Duration = Duration::from_secs(1);
const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(5);
// Keep subprocess waits one-at-a-time for reproducible oracle behavior under
// cargo's concurrent test runner and the standalone runner's one-worker default.
static PROCESS_WAIT_LOCK: Mutex<()> = Mutex::new(());

/// Safe wrapper around a rustc subprocess oracle.
#[derive(Clone, Debug)]
pub struct RustcOracle {
    rustc: PathBuf,
    compile_timeout: Duration,
    run_timeout: Duration,
    stream_cap: usize,
    version_cache: Arc<OnceLock<String>>,
}

/// Captured subprocess status and bounded stdout/stderr.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessCapture {
    /// True when the process exited successfully and did not time out.
    pub success: bool,
    /// Platform exit code when available.
    pub code: Option<i32>,
    /// True when the configured wall-clock timeout elapsed.
    pub timed_out: bool,
    /// Captured stdout bytes, capped by the oracle stream limit.
    pub stdout: Vec<u8>,
    /// Captured stderr bytes, capped by the oracle stream limit.
    pub stderr: Vec<u8>,
    /// True when stdout exceeded the stream cap.
    pub stdout_truncated: bool,
    /// True when stderr exceeded the stream cap.
    pub stderr_truncated: bool,
}

/// Full compiler and native-run result for one source file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OracleResult {
    /// Output from `rustc -Vv`.
    pub rustc_version: String,
    /// Exact argument vector used for the rustc compile invocation, UTF-8 lossy.
    pub rustc_argv: Vec<String>,
    /// Compiler subprocess capture.
    pub compiler: ProcessCapture,
    /// Native executable capture, present only when compilation succeeded.
    pub native: Option<ProcessCapture>,
}

/// Structured differential failure category.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffFailureKind {
    /// Canonical formatting, reparsing, or checked-IR equivalence failed.
    PrettyPrintRoundTrip,
    /// rustc rejected a program accepted by rustscript.
    RustcRejectedAcceptedProgram,
    /// rustc compilation exceeded the compile timeout.
    CompilerTimeout,
    /// Native execution exceeded the run timeout.
    NativeTimeout,
    /// Native execution failed unexpectedly.
    NativeFailure,
    /// Compiler or native output exceeded the configured capture cap.
    OutputCaptureTruncated,
    /// Native stdout bytes differed from interpreter stdout bytes.
    StdoutMismatch,
    /// Native execution wrote stderr.
    UnexpectedNativeStderr,
    /// Repeated interpreter runs differed.
    NondeterministicInterpreter,
}

/// Complete differential failure and artifact-writing context.
#[derive(Clone, Debug)]
pub struct DiffFailure {
    /// Failure category.
    pub kind: DiffFailureKind,
    /// Generator seed or zero for replay input.
    pub seed: u64,
    /// Case index or zero for replay input.
    pub case_index: usize,
    /// Original source under comparison.
    pub source: String,
    /// Canonical formatted source.
    pub canonical: String,
    /// Best minimized source found so far.
    pub minimized: String,
    /// Human-readable failure reason.
    pub reason: String,
    /// Interpreter stdout bytes for the failing run.
    pub interpreter_stdout: Vec<u8>,
    /// rustc/native result when the failure reached the oracle.
    pub oracle: Option<OracleResult>,
}

/// Successful differential case summary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaseSuccess {
    /// Generator seed or zero for replay input.
    pub seed: u64,
    /// Case index or zero for replay input.
    pub case_index: usize,
    /// Original source under comparison.
    pub source: String,
    /// Canonical formatted source.
    pub canonical: String,
    /// Interpreter stdout bytes.
    pub stdout: Vec<u8>,
    /// Interpreter step count.
    pub steps: u64,
}

/// Errors produced while invoking or capturing the rustc oracle.
#[derive(Debug)]
pub enum OracleError {
    /// Filesystem or process I/O failed.
    Io(io::Error),
    /// Expected child pipe was not available.
    MissingPipe(&'static str),
    /// Output reader thread panicked before returning capture data.
    ReaderPanicked(&'static str),
    /// `rustc -Vv` failed, timed out, or exceeded capture limits.
    VersionFailed {
        /// True when the version probe timed out.
        timed_out: bool,
        /// Version-process exit code when available.
        code: Option<i32>,
        /// Captured version stderr.
        stderr: Vec<u8>,
        /// True when version stdout exceeded the stream cap.
        stdout_truncated: bool,
        /// True when version stderr exceeded the stream cap.
        stderr_truncated: bool,
    },
    /// Output reader did not finish promptly after process exit.
    ReaderTimedOut(&'static str),
}

impl std::fmt::Display for OracleError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::MissingPipe(stream) => write!(formatter, "child {stream} pipe was unavailable"),
            Self::ReaderPanicked(stream) => write!(formatter, "child {stream} reader panicked"),
            Self::ReaderTimedOut(stream) => {
                write!(
                    formatter,
                    "child {stream} reader did not finish after process exit"
                )
            }
            Self::VersionFailed {
                timed_out,
                code,
                stderr,
                stdout_truncated,
                stderr_truncated,
            } => write!(
                formatter,
                "rustc -Vv failed (timed_out={timed_out}, code={code:?}, stdout_truncated={stdout_truncated}, stderr_truncated={stderr_truncated}): {}",
                String::from_utf8_lossy(stderr)
            ),
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
    /// Resolve a rustc executable from an explicit path, `RUSTC`, or `PATH`.
    pub fn discover(explicit: Option<PathBuf>) -> Self {
        let rustc = explicit
            .or_else(|| std::env::var_os("RUSTC").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("rustc"));
        Self {
            rustc,
            compile_timeout: Duration::from_secs(10),
            run_timeout: Duration::from_secs(2),
            stream_cap: DEFAULT_STREAM_CAP,
            version_cache: Arc::new(OnceLock::new()),
        }
    }

    #[must_use]
    /// Return a copy of this oracle with custom compile and run timeouts.
    pub fn with_timeouts(mut self, compile_timeout: Duration, run_timeout: Duration) -> Self {
        self.compile_timeout = compile_timeout;
        self.run_timeout = run_timeout;
        self
    }

    #[must_use]
    /// Return a copy of this oracle with a custom stdout/stderr capture cap.
    pub fn with_stream_cap(mut self, bytes: usize) -> Self {
        self.stream_cap = bytes;
        self
    }

    #[must_use]
    /// Return the rustc executable path used by this oracle.
    pub fn rustc_path(&self) -> &Path {
        &self.rustc
    }

    /// Return cached `rustc -Vv` output, probing the compiler if needed.
    pub fn version(&self) -> Result<String, OracleError> {
        if let Some(version) = self.version_cache.get() {
            return Ok(version.clone());
        }
        let capture = run_command(
            &self.rustc,
            &[OsString::from("-Vv")],
            self.compile_timeout,
            self.stream_cap,
            false,
        )?;
        if !capture.success || capture.stdout_truncated || capture.stderr_truncated {
            return Err(OracleError::VersionFailed {
                timed_out: capture.timed_out,
                code: capture.code,
                stderr: capture.stderr,
                stdout_truncated: capture.stdout_truncated,
                stderr_truncated: capture.stderr_truncated,
            });
        }
        let version = String::from_utf8_lossy(&capture.stdout).into_owned();
        let _ = self.version_cache.set(version.clone());
        Ok(version)
    }

    /// Compile and execute one source file with overflow checks enabled.
    pub fn run_source(&self, source: &str) -> Result<OracleResult, OracleError> {
        let directory = tempfile::tempdir()?;
        let source_path = directory.path().join("program.rs");
        let executable_path = directory
            .path()
            .join(format!("program{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&source_path, source.as_bytes())?;
        let rustc_version = self.version()?;

        let arguments = compile_arguments(&source_path, &executable_path);
        let rustc_argv = argv_from_arguments(&self.rustc, &arguments);
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
            rustc_version,
            rustc_argv,
            compiler,
            native,
        })
    }
}

/// Generate, interpret, compile, and compare one deterministic case.
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
    let checked = rustscript_core::check_source(source, rustscript_core::ParseLimits::default())
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
    let reparsed = rustscript_core::check_source(&canonical, rustscript_core::ParseLimits::default())
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
    if !checked.structurally_eq(&reparsed)
        || rustscript_core::format(&reparsed) != canonical
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
    let limits = rustscript_core::Limits::default();
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
    let round_trip = rustscript_core::run(&reparsed, limits).map_err(|error| {
        failure(
            DiffFailureKind::PrettyPrintRoundTrip,
            seed,
            case_index,
            source,
            &canonical,
            format!("reparsed canonical program trapped: {error}"),
            first.stdout.clone(),
            None,
        )
    })?;
    if round_trip != first {
        return Err(failure(
            DiffFailureKind::PrettyPrintRoundTrip,
            seed,
            case_index,
            source,
            &canonical,
            "canonical round trip changed interpreter behavior".into(),
            first.stdout,
            None,
        ));
    }
    let second = rustscript_core::run(&checked, limits).map_err(|error| {
        failure(
            DiffFailureKind::NondeterministicInterpreter,
            seed,
            case_index,
            source,
            &canonical,
            format!("second interpreter run failed: {error}"),
            first.stdout.clone(),
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
            first.stdout,
            None,
        ));
    }
    compare_native(
        seed,
        case_index,
        source,
        &canonical,
        source,
        "accepted source",
        &first.stdout,
        oracle,
    )?;
    if canonical != source {
        compare_native(
            seed,
            case_index,
            source,
            &canonical,
            &canonical,
            "canonical source",
            &round_trip.stdout,
            oracle,
        )?;
    }
    Ok(CaseSuccess {
        seed,
        case_index,
        source: source.into(),
        canonical,
        stdout: first.stdout,
        steps: first.steps,
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "differential failures require original and compiled-source context"
)]
fn compare_native(
    seed: u64,
    case_index: usize,
    source: &str,
    canonical: &str,
    compiled_source: &str,
    source_label: &str,
    expected_stdout: &[u8],
    oracle: &RustcOracle,
) -> Result<(), Box<DiffFailure>> {
    let result = oracle.run_source(compiled_source).map_err(|error| {
        failure(
            DiffFailureKind::NativeFailure,
            seed,
            case_index,
            source,
            canonical,
            format!("oracle failed for {source_label}: {error}"),
            expected_stdout.to_vec(),
            None,
        )
    })?;
    if result.compiler.timed_out {
        return Err(failure(
            DiffFailureKind::CompilerTimeout,
            seed,
            case_index,
            source,
            canonical,
            format!("rustc timed out compiling {source_label}"),
            expected_stdout.to_vec(),
            Some(result),
        ));
    }
    if result.compiler.stdout_truncated || result.compiler.stderr_truncated {
        return Err(failure(
            DiffFailureKind::OutputCaptureTruncated,
            seed,
            case_index,
            source,
            canonical,
            format!("rustc output was truncated compiling {source_label}"),
            expected_stdout.to_vec(),
            Some(result),
        ));
    }
    if !result.compiler.success {
        return Err(failure(
            DiffFailureKind::RustcRejectedAcceptedProgram,
            seed,
            case_index,
            source,
            canonical,
            format!("rustc rejected {source_label}"),
            expected_stdout.to_vec(),
            Some(result),
        ));
    }
    let Some(native) = result.native.as_ref() else {
        return Err(failure(
            DiffFailureKind::NativeFailure,
            seed,
            case_index,
            source,
            canonical,
            format!("native result was missing for {source_label}"),
            expected_stdout.to_vec(),
            Some(result),
        ));
    };
    if native.timed_out {
        return Err(failure(
            DiffFailureKind::NativeTimeout,
            seed,
            case_index,
            source,
            canonical,
            format!("native {source_label} timed out"),
            expected_stdout.to_vec(),
            Some(result),
        ));
    }
    if native.stdout_truncated || native.stderr_truncated {
        return Err(failure(
            DiffFailureKind::OutputCaptureTruncated,
            seed,
            case_index,
            source,
            canonical,
            format!("native output was truncated for {source_label}"),
            expected_stdout.to_vec(),
            Some(result),
        ));
    }
    if !native.success {
        return Err(failure(
            DiffFailureKind::NativeFailure,
            seed,
            case_index,
            source,
            canonical,
            format!("native {source_label} failed"),
            expected_stdout.to_vec(),
            Some(result),
        ));
    }
    if !native.stderr.is_empty() {
        return Err(failure(
            DiffFailureKind::UnexpectedNativeStderr,
            seed,
            case_index,
            source,
            canonical,
            format!("native {source_label} wrote stderr"),
            expected_stdout.to_vec(),
            Some(result),
        ));
    }
    if native.stdout != expected_stdout {
        return Err(failure(
            DiffFailureKind::StdoutMismatch,
            seed,
            case_index,
            source,
            canonical,
            format!("native {source_label} and interpreter stdout differed"),
            expected_stdout.to_vec(),
            Some(result),
        ));
    }
    Ok(())
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
        minimized: source.into(),
        reason,
        interpreter_stdout,
        oracle,
    })
}

/// Greedily retain smaller checked-IR candidates that reproduce the same
/// differential failure category.
pub fn minimize_failure(failure: &mut DiffFailure, oracle: &RustcOracle) {
    let mut current = failure.source.clone();
    while let Ok(program) =
        rustscript_core::check_source(&current, rustscript_core::ParseLimits::default())
    {
        let mut candidates: Vec<_> = rustscript_core::reduction_candidates(&program)
            .into_iter()
            .map(|candidate| rustscript_core::format(&candidate))
            .filter(|candidate| candidate.len() < current.len())
            .collect();
        candidates.sort_by_key(String::len);
        candidates.dedup();
        let mut accepted = None;
        for candidate in candidates {
            if let Err(candidate_failure) = replay_source(&candidate, oracle)
                && candidate_failure.kind == failure.kind
            {
                accepted = Some(candidate);
                break;
            }
        }
        let Some(candidate) = accepted else {
            break;
        };
        current = candidate;
    }
    failure.minimized = current;
}

/// Persist a complete, non-overwriting differential failure artifact.
pub fn write_failure_artifact(failure: &DiffFailure, root: &Path) -> io::Result<PathBuf> {
    write_failure_artifact_with_oracle(failure, root, None)
}

/// Persist a complete failure artifact with current oracle provenance.
pub fn write_failure_artifact_with_oracle(
    failure: &DiffFailure,
    root: &Path,
    oracle: Option<&RustcOracle>,
) -> io::Result<PathBuf> {
    let directory = unique_artifact_directory(root, failure.seed, failure.case_index)?;
    write_file(&directory, "failure.rs", failure.source.as_bytes())?;
    write_file(&directory, "canonical.rs", failure.canonical.as_bytes())?;
    write_file(&directory, "minimized.rs", failure.minimized.as_bytes())?;
    let ast = rustscript_core::parse(&failure.source, rustscript_core::ParseLimits::default())
        .map(|program| rustscript_core::debug_syntax(&program))
        .unwrap_or_else(|error| format!("syntax tree unavailable: {error}"));
    write_file(&directory, "ast.txt", ast.as_bytes())?;
    write_file(
        &directory,
        "interpreter.stdout",
        &failure.interpreter_stdout,
    )?;

    let frontend = rustscript_core::ParseLimits::default();
    let runtime = rustscript_core::Limits::default();
    let mut rustc_version = failure
        .oracle
        .as_ref()
        .map(|result| result.rustc_version.clone())
        .or_else(|| {
            oracle.map(|oracle| {
                oracle
                    .version()
                    .unwrap_or_else(|error| format!("unavailable: {error}"))
            })
        })
        .unwrap_or_else(|| "unavailable: no oracle provided".into());
    if !rustc_version.ends_with('\n') {
        rustc_version.push('\n');
    }
    let reproduce_argv = runner_argv(
        failure.seed,
        failure.case_index,
        None,
        oracle.map(RustcOracle::rustc_path),
    );
    let replay_path = directory.join("failure.rs");
    let replay_argv = runner_argv(
        failure.seed,
        failure.case_index,
        Some(&replay_path),
        oracle.map(RustcOracle::rustc_path),
    );
    let artifact_executable = directory.join(format!("failure{}", std::env::consts::EXE_SUFFIX));
    let artifact_rustc_arguments = compile_arguments(&replay_path, &artifact_executable);
    let artifact_rustc_argv =
        oracle.map(|oracle| argv_from_arguments(oracle.rustc_path(), &artifact_rustc_arguments));
    let mut metadata = format!(
        "seed={}\ncase={}\ncategory={:?}\nreason={}\nhost_os={}\nhost_arch={}\nrustc_version=\n{}frontend_max_source_bytes={}\nfrontend_max_tokens={}\nfrontend_max_delimiter_depth={}\nfrontend_max_syntax_elements={}\nfrontend_max_syntax_depth={}\nfrontend_max_functions={}\nfrontend_max_parameters={}\ngenerator_helpers=0..=4\ngenerator_parameters=0..=3\ngenerator_statements=0..=8\ngenerator_expression_depth=5\ngenerator_loop_depth=2\ngenerator_loop_bound=8\ngenerator_output_lines=64\ninterpreter_fuel={}\ninterpreter_call_depth={}\ninterpreter_output_bytes={}\nreproduce_argv_json={}\nreplay_argv_json={}\n",
        failure.seed,
        failure.case_index,
        failure.kind,
        failure.reason,
        std::env::consts::OS,
        std::env::consts::ARCH,
        rustc_version,
        frontend.max_source_bytes,
        frontend.max_tokens,
        frontend.max_delimiter_depth,
        frontend.max_syntax_elements,
        frontend.max_syntax_depth,
        frontend.max_functions,
        frontend.max_parameters,
        runtime.fuel,
        runtime.maximum_call_depth,
        runtime.maximum_output_bytes,
        serde_json::to_string(&reproduce_argv).expect("runner argv must serialize"),
        serde_json::to_string(&replay_argv).expect("runner argv must serialize"),
    );
    if let Some(oracle) = oracle {
        metadata.push_str(&format!(
            "rustc_path={}\ncompile_timeout_ms={}\nrun_timeout_ms={}\nstream_cap_bytes={}\n",
            oracle.rustc.display(),
            oracle.compile_timeout.as_millis(),
            oracle.run_timeout.as_millis(),
            oracle.stream_cap,
        ));
    }
    if let Some(argv) = &artifact_rustc_argv {
        metadata.push_str("artifact_rustc_argv_json=");
        metadata.push_str(&serde_json::to_string(argv).expect("rustc argv must serialize"));
        metadata.push('\n');
    }
    if let Some(oracle) = &failure.oracle {
        metadata.push_str("rustc_argv_json=");
        metadata.push_str(
            &serde_json::to_string(&oracle.rustc_argv).expect("rustc argv must serialize"),
        );
        metadata.push('\n');
        append_capture_metadata(&mut metadata, "compiler", &oracle.compiler);
        write_file(&directory, "compiler.stdout", &oracle.compiler.stdout)?;
        write_file(&directory, "compiler.stderr", &oracle.compiler.stderr)?;
        if let Some(native) = &oracle.native {
            append_capture_metadata(&mut metadata, "native", native);
            write_file(&directory, "native.stdout", &native.stdout)?;
            write_file(&directory, "native.stderr", &native.stderr)?;
        } else {
            metadata.push_str("native_present=false\n");
            write_file(&directory, "native.stdout", &[])?;
            write_file(&directory, "native.stderr", &[])?;
        }
    } else {
        metadata.push_str("rustc_argv_json=");
        if let Some(argv) = &artifact_rustc_argv {
            metadata.push_str(&serde_json::to_string(argv).expect("rustc argv must serialize"));
        } else {
            metadata.push_str("null");
        }
        metadata.push('\n');
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
    write_file(&directory, "source.rs", success.source.as_bytes())?;
    write_file(&directory, "canonical.rs", success.canonical.as_bytes())?;
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

fn append_capture_metadata(metadata: &mut String, prefix: &str, capture: &ProcessCapture) {
    metadata.push_str(&format!(
        "{prefix}_success={}\n{prefix}_code={:?}\n{prefix}_timed_out={}\n{prefix}_stdout_truncated={}\n{prefix}_stderr_truncated={}\n",
        capture.success,
        capture.code,
        capture.timed_out,
        capture.stdout_truncated,
        capture.stderr_truncated,
    ));
}

fn runner_argv(
    seed: u64,
    case_index: usize,
    replay: Option<&Path>,
    rustc: Option<&Path>,
) -> Vec<String> {
    let mut argv = vec![
        "cargo".into(),
        "run".into(),
        "-p".into(),
        "rustscript-difftest".into(),
        "--".into(),
    ];
    if let Some(path) = replay {
        argv.push("--replay".into());
        argv.push(path.to_string_lossy().into_owned());
    } else {
        argv.push("--seed".into());
        argv.push(seed.to_string());
        argv.push("--case".into());
        argv.push(case_index.to_string());
    }
    if let Some(rustc) = rustc {
        argv.push("--rustc".into());
        argv.push(rustc.to_string_lossy().into_owned());
    }
    argv
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

fn argv_from_arguments(program: &Path, arguments: &[OsString]) -> Vec<String> {
    std::iter::once(program.to_string_lossy().into_owned())
        .chain(
            arguments
                .iter()
                .map(|argument| argument.to_string_lossy().into_owned()),
        )
        .collect()
}

fn run_command(
    program: &Path,
    arguments: &[OsString],
    timeout: Duration,
    cap: usize,
    native: bool,
) -> Result<ProcessCapture, OracleError> {
    let _wait_guard = PROCESS_WAIT_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut command = Command::new(program);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
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
    let stdout_reader = spawn_reader(stdout, cap);
    let stderr_reader = spawn_reader(stderr, cap);
    let wait_result = wait_with_timeout(&mut child, timeout);
    let (status, timed_out) = match wait_result {
        Ok(Some(status)) => (status, false),
        Ok(None) => {
            terminate_process_tree(&mut child)?;
            (child.wait()?, true)
        }
        Err(wait_error) => {
            let cleanup = terminate_process_tree(&mut child).and_then(|()| child.wait().map(drop));
            return match cleanup {
                Ok(()) => Err(OracleError::Io(wait_error)),
                Err(cleanup_error) => Err(OracleError::Io(io::Error::other(format!(
                    "subprocess wait failed: {wait_error}; cleanup failed: {cleanup_error}"
                )))),
            };
        }
    };
    let (stdout, stdout_truncated) = receive_reader(stdout_reader, "stdout")?;
    let (stderr, stderr_truncated) = receive_reader(stderr_reader, "stderr")?;
    Ok(capture(
        status,
        timed_out,
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
    ))
}

fn wait_with_timeout(child: &mut Child, timeout: Duration) -> io::Result<Option<ExitStatus>> {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        let elapsed = start.elapsed();
        if elapsed >= timeout {
            return Ok(None);
        }
        thread::sleep(WAIT_POLL_INTERVAL.min(timeout - elapsed));
    }
}

fn spawn_reader(
    reader: impl Read + Send + 'static,
    cap: usize,
) -> mpsc::Receiver<io::Result<(Vec<u8>, bool)>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(read_capped(reader, cap));
    });
    receiver
}

fn receive_reader(
    receiver: mpsc::Receiver<io::Result<(Vec<u8>, bool)>>,
    name: &'static str,
) -> Result<(Vec<u8>, bool), OracleError> {
    match receiver.recv_timeout(READER_DRAIN_TIMEOUT) {
        Ok(result) => result.map_err(OracleError::Io),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(OracleError::ReaderTimedOut(name)),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(OracleError::ReaderPanicked(name)),
    }
}

#[cfg(unix)]
fn terminate_process_tree(child: &mut Child) -> io::Result<()> {
    let process_group = format!("-{}", child.id());
    let group_kill_succeeded = Command::new("kill")
        .args(["-KILL", "--", &process_group])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    match child.try_wait()? {
        Some(_) => Ok(()),
        None => match child.kill() {
            Ok(()) => Ok(()),
            Err(error)
                if group_kill_succeeded
                    && matches!(
                        error.kind(),
                        io::ErrorKind::InvalidInput | io::ErrorKind::NotFound
                    ) =>
            {
                Ok(())
            }
            Err(error) => Err(error),
        },
    }
}

#[cfg(not(unix))]
fn terminate_process_tree(child: &mut Child) -> io::Result<()> {
    match child.try_wait()? {
        Some(_) => Ok(()),
        None => child.kill(),
    }
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
    use proptest::prelude::*;

    const EXAMPLES: &[(&str, &str, &[u8])] = &[
        (
            "arithmetic",
            include_str!("../../../examples/arithmetic.rs"),
            b"42\n",
        ),
        (
            "evaluation_order",
            include_str!("../../../examples/evaluation_order.rs"),
            b"1\n2\n30\n",
        ),
        (
            "short_circuit",
            include_str!("../../../examples/short_circuit.rs"),
            b"false\ntrue\n",
        ),
        (
            "sum_loop",
            include_str!("../../../examples/sum_loop.rs"),
            b"55\n",
        ),
        ("gcd", include_str!("../../../examples/gcd.rs"), b"6\n"),
        (
            "shadowing",
            include_str!("../../../examples/shadowing.rs"),
            b"2\n",
        ),
    ];

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
    fn normative_examples_match_native_stdout() {
        let oracle = RustcOracle::discover(None);
        for (case_index, (name, source, expected)) in EXAMPLES.iter().enumerate() {
            let success = compare_source(1, case_index, source, &oracle, false)
                .unwrap_or_else(|failure| panic!("{name}: {:?}: {}", failure.kind, failure.reason));
            assert_eq!(&success.stdout, expected, "{name}");
        }
    }

    #[test]
    fn trap_suite_matches_native_failure_categories() {
        let oracle = RustcOracle::discover(None);
        for (name, source, expected_message) in [
            (
                "addition overflow",
                "fn max() -> i64 { 9223372036854775807_i64 } fn one() -> i64 { 1_i64 } fn main() { max() + one(); }",
                "integer arithmetic overflow",
            ),
            (
                "multiplication overflow",
                "fn big() -> i64 { 3037000500_i64 } fn main() { big() * big(); }",
                "integer arithmetic overflow",
            ),
            (
                "division by zero",
                "fn one() -> i64 { 1_i64 } fn zero() -> i64 { 0_i64 } fn main() { one() / zero(); }",
                "division by zero",
            ),
            (
                "remainder by zero",
                "fn one() -> i64 { 1_i64 } fn zero() -> i64 { 0_i64 } fn main() { one() % zero(); }",
                "remainder by zero",
            ),
            (
                "negation overflow",
                "fn min(x: i64) -> i64 { x - 1_i64 } fn main() { let x: i64 = min(-9223372036854775807_i64); -x; }",
                "integer negation overflow",
            ),
            (
                "division overflow",
                "fn min(x: i64) -> i64 { x - 1_i64 } fn neg_one() -> i64 { -1_i64 } fn main() { let x: i64 = min(-9223372036854775807_i64); x / neg_one(); }",
                "integer arithmetic overflow",
            ),
            (
                "remainder overflow",
                "fn min(x: i64) -> i64 { x - 1_i64 } fn neg_one() -> i64 { -1_i64 } fn main() { let x: i64 = min(-9223372036854775807_i64); x % neg_one(); }",
                "integer arithmetic overflow",
            ),
        ] {
            let checked = rustscript_core::check_source(source, rustscript_core::ParseLimits::default())
                .unwrap_or_else(|error| panic!("{name} did not check: {error}"));
            let error = rustscript_core::run(&checked, rustscript_core::Limits::default())
                .unwrap_err();
            assert_eq!(error.diagnostic.message, expected_message, "{name}");

            let native = oracle
                .run_source(source)
                .unwrap_or_else(|error| panic!("{name} oracle failed: {error}"));
            assert!(
                native.compiler.success,
                "{name}: {}",
                String::from_utf8_lossy(&native.compiler.stderr)
            );
            let native = native.native.expect("native run");
            assert!(!native.success, "{name}");
            assert!(native.stdout.is_empty(), "{name}");
        }
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

    #[cfg(unix)]
    #[test]
    fn bounds_rustc_version_probe() {
        use std::os::unix::fs::PermissionsExt;
        use std::time::Instant;

        let directory = tempfile::tempdir().unwrap();
        let fake_rustc = directory.path().join("rustc");
        std::fs::write(&fake_rustc, "#!/bin/sh\nsleep 5\n").unwrap();
        let mut permissions = std::fs::metadata(&fake_rustc).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fake_rustc, permissions).unwrap();

        let oracle = RustcOracle::discover(Some(fake_rustc))
            .with_timeouts(Duration::from_millis(50), Duration::from_secs(1));
        let started = Instant::now();
        let error = oracle.version().unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(matches!(
            error,
            OracleError::VersionFailed {
                timed_out: true,
                ..
            }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn serializes_concurrent_timeout_waits() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let fake_rustc = directory.path().join("rustc");
        std::fs::write(&fake_rustc, "#!/bin/sh\nsleep 5\n").unwrap();
        let mut permissions = std::fs::metadata(&fake_rustc).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fake_rustc, permissions).unwrap();

        let oracle = RustcOracle::discover(Some(fake_rustc))
            .with_timeouts(Duration::from_millis(50), Duration::from_secs(1));
        let workers: Vec<_> = (0..4)
            .map(|_| {
                let oracle = oracle.clone();
                std::thread::spawn(move || oracle.version())
            })
            .collect();
        for worker in workers {
            let error = worker
                .join()
                .expect("version worker must not panic")
                .unwrap_err();
            assert!(matches!(
                error,
                OracleError::VersionFailed {
                    timed_out: true,
                    ..
                }
            ));
        }
    }

    #[test]
    fn rejects_truncated_version_probe() {
        let oracle = RustcOracle::discover(None).with_stream_cap(0);
        let error = oracle.version().unwrap_err();
        assert!(matches!(
            error,
            OracleError::VersionFailed {
                stdout_truncated: true,
                ..
            }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn classifies_compiler_timeout_separately() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let fake_rustc = directory.path().join("rustc");
        std::fs::write(
            &fake_rustc,
            "#!/bin/sh\nif [ \"$1\" = \"-Vv\" ]; then printf 'rustc 1.95.0\\nhost: fake\\n'; exit 0; fi\nsleep 5\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&fake_rustc).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fake_rustc, permissions).unwrap();

        let oracle = RustcOracle::discover(Some(fake_rustc))
            .with_timeouts(Duration::from_millis(50), Duration::from_secs(1));
        let failure = replay_source("fn main() {}", &oracle).unwrap_err();
        assert_eq!(failure.kind, DiffFailureKind::CompilerTimeout);
    }

    #[test]
    fn classifies_native_capture_truncation_separately() {
        let oracle = RustcOracle::discover(None);
        let _ = oracle.version().unwrap();
        let oracle = oracle.with_stream_cap(1);
        let failure =
            replay_source("fn main() { println!(\"{}\", 123_i64); }", &oracle).unwrap_err();
        assert_eq!(failure.kind, DiffFailureKind::OutputCaptureTruncated);
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

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(8))]

        #[test]
        fn generated_programs_compile_and_match_native(seed in any::<u64>(), case_index in 0usize..8) {
            let oracle = RustcOracle::discover(None);
            let success = run_case(seed, case_index, &oracle)
                .map_err(|failure| TestCaseError::fail(format!("{:?}: {}", failure.kind, failure.reason)))?;
            prop_assert!(success.steps > 0);
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
            minimized: "fn main() {}\n".into(),
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

        let oracle = RustcOracle::discover(None);
        let with_oracle =
            write_failure_artifact_with_oracle(&failure, root.path(), Some(&oracle)).unwrap();
        let metadata = std::fs::read_to_string(with_oracle.join("metadata.txt")).unwrap();
        assert!(metadata.contains("artifact_rustc_argv_json=["));
        assert!(metadata.contains("rustc_argv_json=["));
        assert!(metadata.contains("failure.rs"));

        let missing_oracle = RustcOracle::discover(Some(root.path().join("missing-rustc")));
        let unavailable =
            write_failure_artifact_with_oracle(&failure, root.path(), Some(&missing_oracle))
                .unwrap();
        let metadata = std::fs::read_to_string(unavailable.join("metadata.txt")).unwrap();
        assert!(metadata.contains("unavailable:"));
        assert!(metadata.contains("\nfrontend_max_source_bytes="));
    }
}
