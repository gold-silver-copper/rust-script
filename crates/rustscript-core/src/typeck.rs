use crate::{CheckedProgram, Diagnostic, ParsedProgram, Phase};

pub(crate) fn check(parsed: &ParsedProgram) -> Result<CheckedProgram, Diagnostic> {
    let _ = parsed.file();
    let _ = parsed.limits();
    Err(Diagnostic::new(
        Phase::Type,
        "type checker implementation in progress",
        None,
    ))
}
