#![forbid(unsafe_code)]
#![doc = "A resource-bounded interpreter for a strict, Rust-compatible subset."]

use ra_ap_syntax::AstNode;

mod checked_ir;
mod diagnostic;
mod emit;
mod eval;
mod frontend;
#[cfg(feature = "generator-support")]
mod generator_support;
mod limits;
mod typeck;

pub use checked_ir::{CheckedProgram, Type, Value};
pub use diagnostic::{Diagnostic, Location, Phase, Span};
pub use eval::{Execution, RunResult, RuntimeLimits};
pub use frontend::ParsedProgram;
#[cfg(feature = "generator-support")]
pub use generator_support::{generate_checked_program, reduction_candidates};
pub use limits::Limits;

/// Frontend resource limits used by [`parse`] and [`parse_bytes`].
pub type ParseLimits = Limits;

/// Structured runtime failure returned by [`run`].
pub type RuntimeDiagnostic = Diagnostic;

/// Validate and parse UTF-8 source using the strict rustscript profile.
pub fn parse(source: &str, limits: Limits) -> Result<ParsedProgram, Diagnostic> {
    frontend::parse(source.as_bytes(), limits)
}

/// Validate and parse source bytes, reporting invalid UTF-8 as a diagnostic.
pub fn parse_bytes(source: &[u8], limits: Limits) -> Result<ParsedProgram, Diagnostic> {
    frontend::parse(source, limits)
}

/// Type-check a previously parsed program and lower it to executable IR.
pub fn check(parsed: &ParsedProgram) -> Result<CheckedProgram, Vec<Diagnostic>> {
    typeck::check(parsed).map_err(|error| vec![error])
}

/// Parse and type-check source in one operation.
pub fn check_source(source: &str, limits: Limits) -> Result<CheckedProgram, Diagnostic> {
    typeck::check(&parse(source, limits)?)
}

/// Execute a checked program with deterministic safeguards.
pub fn run(program: &CheckedProgram, limits: RuntimeLimits) -> Result<Execution, Diagnostic> {
    eval::run(program, limits)
}

/// Emit the canonical Rust representation of a checked program.
pub fn format(program: &CheckedProgram) -> String {
    emit::format(program)
}

/// Canonically format a parsed program using the admitted rust-analyzer tree.
pub fn format_program(program: &ParsedProgram) -> String {
    emit::format_parsed(program)
}

/// Return rust-analyzer's pinned syntax debug representation.
pub fn debug_syntax(program: &ParsedProgram) -> String {
    format!("{:#?}", program.file().syntax())
}

/// Lazily resolve a diagnostic's source location without reparsing the program.
pub fn locate(source: &str, diagnostic: &Diagnostic) -> Option<Location> {
    diagnostic.location(&line_index::LineIndex::new(source))
}
