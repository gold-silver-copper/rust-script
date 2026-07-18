mod admit;
mod bytes;
mod lex_policy;

use line_index::LineIndex;
use ra_ap_syntax::{AstNode, Edition, SourceFile, SyntaxKind, WalkEvent, ast};

use crate::{Diagnostic, Limits, Phase};

/// Opaque owner of validated source and its rust-analyzer syntax tree.
pub struct ParsedProgram {
    source: String,
    file: ast::SourceFile,
    line_index: LineIndex,
    limits: Limits,
}

impl ParsedProgram {
    pub fn source(&self) -> &str {
        &self.source
    }
    pub fn line_index(&self) -> &LineIndex {
        &self.line_index
    }
    pub(crate) fn file(&self) -> &ast::SourceFile {
        &self.file
    }
    pub(crate) fn limits(&self) -> Limits {
        self.limits
    }
}

pub(crate) fn parse(bytes: &[u8], limits: Limits) -> Result<ParsedProgram, Diagnostic> {
    let source = bytes::validate(bytes, limits)?.to_owned();
    lex_policy::validate(&source, limits)?;

    let parsed = parse_ra(&source)?;
    if let Some(error) = parsed.errors().first() {
        return Err(Diagnostic::new(
            Phase::Parse,
            error.to_string(),
            Some(crate::diagnostic::span(error.range())),
        ));
    }
    let file = parsed.tree();
    validate_tree(&file, limits)?;
    admit::validate(&file, limits)?;
    Ok(ParsedProgram {
        line_index: LineIndex::new(&source),
        source,
        file,
        limits,
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn parse_ra(source: &str) -> Result<ra_ap_syntax::Parse<ast::SourceFile>, Diagnostic> {
    std::panic::catch_unwind(|| SourceFile::parse(source, Edition::Edition2024))
        .map_err(|_| Diagnostic::new(Phase::Parse, "Rust parser aborted", None))
}

#[cfg(target_arch = "wasm32")]
fn parse_ra(source: &str) -> Result<ra_ap_syntax::Parse<ast::SourceFile>, Diagnostic> {
    Ok(SourceFile::parse(source, Edition::Edition2024))
}

fn validate_tree(file: &ast::SourceFile, limits: Limits) -> Result<(), Diagnostic> {
    let root = file.syntax();
    let mut elements = 0usize;
    let mut depth = 0usize;
    for event in root.preorder_with_tokens() {
        match event {
            WalkEvent::Enter(element) => {
                elements += 1;
                depth += 1;
                if element.kind() == SyntaxKind::ERROR {
                    return Err(Diagnostic::new(
                        Phase::Parse,
                        "invalid recovered syntax",
                        Some(crate::diagnostic::span(element.text_range())),
                    ));
                }
                if elements > limits.max_syntax_elements {
                    return Err(Diagnostic::new(
                        Phase::Parse,
                        "syntax element limit exceeded",
                        Some(crate::diagnostic::span(element.text_range())),
                    ));
                }
                if depth > limits.max_syntax_depth {
                    return Err(Diagnostic::new(
                        Phase::Parse,
                        "syntax nesting limit exceeded",
                        Some(crate::diagnostic::span(element.text_range())),
                    ));
                }
            }
            WalkEvent::Leave(_) => depth = depth.saturating_sub(1),
        }
    }
    Ok(())
}
