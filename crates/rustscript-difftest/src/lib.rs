#![forbid(unsafe_code)]
#![doc = "Native rustc differential-testing support for rustscript."]

use std::ffi::OsString;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::Duration;

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
}
