#![forbid(unsafe_code)]
#![doc = "Command-line interface for rustscript."]

use std::io::{self, Read, Write};
use std::path::Path;
use std::process::ExitCode;

use annotate_snippets::{AnnotationKind, Level, Renderer, Snippet};
use rustscript_core::{Diagnostic, Limits, RuntimeLimits};

fn main() -> ExitCode {
    match run_cli(std::env::args_os().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(CliError::Usage(message)) => {
            eprintln!("{message}\nusage: rustscript <check|run|ast|fmt> <FILE>");
            ExitCode::from(2)
        }
        Err(CliError::Io(error)) => {
            eprintln!("rustscript: {error}");
            ExitCode::from(1)
        }
        Err(CliError::Diagnostic(report)) => {
            let DiagnosticReport {
                error,
                source,
                path,
            } = *report;
            render_diagnostic(&error, &source, &path);
            ExitCode::from(1)
        }
    }
}

enum CliError {
    Usage(&'static str),
    Io(io::Error),
    Diagnostic(Box<DiagnosticReport>),
}

struct DiagnosticReport {
    error: Diagnostic,
    source: Vec<u8>,
    path: String,
}

fn run_cli(arguments: Vec<std::ffi::OsString>) -> Result<(), CliError> {
    if arguments.len() != 2 {
        return Err(CliError::Usage("expected an operation and source file"));
    }
    let operation = arguments[0]
        .to_str()
        .ok_or(CliError::Usage("operation must be valid UTF-8"))?;
    if !matches!(operation, "check" | "run" | "ast" | "fmt") {
        return Err(CliError::Usage("unknown operation"));
    }
    let path = Path::new(&arguments[1]);
    let parse_limits = Limits::default();
    let source = read_bounded(path, parse_limits.max_source_bytes).map_err(CliError::Io)?;
    let display_path = path.display().to_string();
    let parsed = rustscript_core::parse_bytes(&source, parse_limits).map_err(|error| {
        CliError::Diagnostic(Box::new(DiagnosticReport {
            error: error.with_file_name(display_path.clone()),
            source: source.clone(),
            path: display_path.clone(),
        }))
    })?;
    let mut stdout = io::stdout().lock();
    if operation == "ast" {
        stdout
            .write_all(rustscript_core::debug_syntax(&parsed).as_bytes())
            .map_err(CliError::Io)?;
        stdout.write_all(b"\n").map_err(CliError::Io)?;
        return stdout.flush().map_err(CliError::Io);
    }
    if operation == "fmt" {
        stdout
            .write_all(rustscript_core::format_program(&parsed).as_bytes())
            .map_err(CliError::Io)?;
        return stdout.flush().map_err(CliError::Io);
    }
    let checked = rustscript_core::check(&parsed).map_err(|errors| {
        errors.into_iter().next().map_or_else(
            || CliError::Io(io::Error::other("checker returned no diagnostic")),
            |error| {
                CliError::Diagnostic(Box::new(DiagnosticReport {
                    error: error.with_file_name(display_path.clone()),
                    source: source.clone(),
                    path: display_path.clone(),
                }))
            },
        )
    })?;
    match operation {
        "check" => {}
        "run" => {
            let execution =
                rustscript_core::run(&checked, RuntimeLimits::default()).map_err(|error| {
                    CliError::Diagnostic(Box::new(DiagnosticReport {
                        error: error.with_file_name(display_path.clone()),
                        source: source.clone(),
                        path: display_path.clone(),
                    }))
                })?;
            stdout.write_all(&execution.stdout).map_err(CliError::Io)?;
        }
        "ast" => return Err(CliError::Usage("unknown operation")),
        "fmt" => return Err(CliError::Usage("unknown operation")),
        _ => return Err(CliError::Usage("unknown operation")),
    }
    stdout.flush().map_err(CliError::Io)
}

fn read_bounded(path: &Path, maximum_bytes: usize) -> io::Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    let limit = u64::try_from(maximum_bytes)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let mut source = Vec::with_capacity(maximum_bytes.saturating_add(1).min(64 * 1024));
    file.take(limit).read_to_end(&mut source)?;
    Ok(source)
}

fn render_diagnostic(error: &Diagnostic, source: &[u8], path: &str) {
    let Ok(source) = std::str::from_utf8(source) else {
        if let Some(span) = error.span {
            eprintln!(
                "{:?}: {} at {path}: bytes {}..{}",
                error.phase, error.message, span.start, span.end
            );
        } else {
            eprintln!("{:?}: {} at {path}", error.phase, error.message);
        }
        return;
    };
    let end = source.len();
    let span = error
        .span
        .map(|span| span.start.min(end)..span.end.min(end).max(span.start.min(end)));
    let mut snippet = Snippet::source(source).path(path);
    if let Some(span) = span {
        snippet = snippet.annotation(AnnotationKind::Primary.span(span).label(&error.message));
    }
    let title = format!("{:?}: {}", error.phase, error.message);
    let report = [Level::ERROR.primary_title(&title).element(snippet)];
    eprintln!("{}", Renderer::styled().render(&report));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_bad_arguments() {
        assert!(matches!(run_cli(Vec::new()), Err(CliError::Usage(_))));
    }
}
