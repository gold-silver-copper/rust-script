#![forbid(unsafe_code)]
#![doc = "A resource-bounded interpreter for a strict, Rust-compatible subset."]

mod checked_ir;
mod diagnostic;
mod emit;
mod eval;
mod frontend;
mod limits;
mod typeck;

pub use checked_ir::{CheckedProgram, Type, Value};
pub use diagnostic::{Diagnostic, Location, Phase, Span};
pub use eval::{Execution, RuntimeLimits};
pub use frontend::ParsedProgram;
pub use limits::Limits;

/// Validate and parse UTF-8 source using the strict rustscript profile.
pub fn parse(source: &str, limits: Limits) -> Result<ParsedProgram, Diagnostic> {
    frontend::parse(source.as_bytes(), limits)
}

/// Validate and parse source bytes, reporting invalid UTF-8 as a diagnostic.
pub fn parse_bytes(source: &[u8], limits: Limits) -> Result<ParsedProgram, Diagnostic> {
    frontend::parse(source, limits)
}

/// Type-check a previously parsed program and lower it to executable IR.
pub fn check(parsed: &ParsedProgram) -> Result<CheckedProgram, Diagnostic> {
    typeck::check(parsed)
}

/// Parse and type-check source in one operation.
pub fn check_source(source: &str, limits: Limits) -> Result<CheckedProgram, Diagnostic> {
    check(&parse(source, limits)?)
}

/// Execute a checked program with deterministic safeguards.
pub fn run(program: &CheckedProgram, limits: RuntimeLimits) -> Result<Execution, Diagnostic> {
    eval::run(program, limits)
}

/// Emit the canonical Rust representation of a checked program.
pub fn format(program: &CheckedProgram) -> String {
    emit::format(program)
}

/// Return a stable-for-this-build debug representation of checked executable IR.
pub fn debug_ir(program: &CheckedProgram) -> String {
    format!("{program:#?}")
}

/// Lazily resolve a diagnostic's source location without reparsing the program.
pub fn locate(source: &str, diagnostic: &Diagnostic) -> Option<Location> {
    diagnostic.location(&line_index::LineIndex::new(source))
}
