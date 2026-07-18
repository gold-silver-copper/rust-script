#![forbid(unsafe_code)]

use std::io::{self, Write};
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
    let source = std::fs::read(path).map_err(CliError::Io)?;
    let display_path = path.display().to_string();
    let parsed = rustscript_core::parse_bytes(&source, Limits::default()).map_err(|error| {
        CliError::Diagnostic(Box::new(DiagnosticReport {
            error,
            source: source.clone(),
            path: display_path.clone(),
        }))
    })?;
    let checked = rustscript_core::check(&parsed).map_err(|error| {
        CliError::Diagnostic(Box::new(DiagnosticReport {
            error,
            source: source.clone(),
            path: display_path.clone(),
        }))
    })?;
    let mut stdout = io::stdout().lock();
    match operation {
        "check" => {}
        "run" => {
            let execution =
                rustscript_core::run(&checked, RuntimeLimits::default()).map_err(|error| {
                    CliError::Diagnostic(Box::new(DiagnosticReport {
                        error,
                        source: source.clone(),
                        path: display_path,
                    }))
                })?;
            stdout.write_all(&execution.output).map_err(CliError::Io)?;
        }
        "ast" => {
            stdout
                .write_all(rustscript_core::debug_ir(&checked).as_bytes())
                .map_err(CliError::Io)?;
            stdout.write_all(b"\n").map_err(CliError::Io)?;
        }
        "fmt" => stdout
            .write_all(rustscript_core::format(&checked).as_bytes())
            .map_err(CliError::Io)?,
        _ => return Err(CliError::Usage("unknown operation")),
    }
    stdout.flush().map_err(CliError::Io)
}

fn render_diagnostic(error: &Diagnostic, source: &[u8], path: &str) {
    let source = String::from_utf8_lossy(source);
    let end = source.len();
    let span = error
        .span
        .map(|span| span.start.min(end)..span.end.min(end).max(span.start.min(end)));
    let mut snippet = Snippet::source(source.as_ref()).path(path);
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
