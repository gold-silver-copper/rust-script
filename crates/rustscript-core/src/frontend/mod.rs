mod admit;
mod bytes;
pub(crate) mod intrinsic;
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

    let file = parse_syntax(&source, limits)?;
    Ok(ParsedProgram {
        line_index: LineIndex::new(&source),
        source,
        file,
        limits,
    })
}

fn parse_syntax_uncontained(source: &str, limits: Limits) -> Result<ast::SourceFile, Diagnostic> {
    let parsed = SourceFile::parse(source, Edition::Edition2024);
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
    drop(parsed);
    Ok(file)
}

#[cfg(not(target_arch = "wasm32"))]
fn parse_syntax(source: &str, limits: Limits) -> Result<ast::SourceFile, Diagnostic> {
    std::panic::catch_unwind(|| parse_syntax_uncontained(source, limits))
        .map_err(|_| Diagnostic::new(Phase::Parse, "Rust parser aborted", None))
        .and_then(std::convert::identity)
}

#[cfg(target_arch = "wasm32")]
fn parse_syntax(source: &str, limits: Limits) -> Result<ast::SourceFile, Diagnostic> {
    parse_syntax_uncontained(source, limits)
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

#[cfg(test)]
mod property_tests {
    use proptest::prelude::*;
    use ra_ap_syntax::ast::{ArithOp, BinaryOp, LogicOp};
    use ra_ap_syntax::{AstNode, ast};

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn byte_frontend_terminates(data in prop::collection::vec(any::<u8>(), 0..4096)) {
            let _ = crate::parse_bytes(&data, crate::Limits::default());
        }

        #[test]
        fn ascii_parse_admission_terminates(data in prop::collection::vec(0_u8..=127, 0..4096)) {
            let _ = crate::parse_bytes(&data, crate::Limits::default());
        }
    }

    #[test]
    fn rust_analyzer_operator_tree_defines_precedence() {
        let parsed = crate::parse(
            "fn main() { let x = 1_i64 + 2_i64 * 3_i64; let y = false || true && false; }",
            crate::Limits::default(),
        )
        .expect("operator sample must parse");
        let binaries: Vec<_> = parsed
            .file()
            .syntax()
            .descendants()
            .filter_map(ast::BinExpr::cast)
            .collect();
        let add = binaries
            .iter()
            .find(|binary| binary.op_kind() == Some(BinaryOp::ArithOp(ArithOp::Add)))
            .expect("outer addition");
        assert!(matches!(
            add.rhs(),
            Some(ast::Expr::BinExpr(ref rhs))
                if rhs.op_kind() == Some(BinaryOp::ArithOp(ArithOp::Mul))
        ));
        let or = binaries
            .iter()
            .find(|binary| binary.op_kind() == Some(BinaryOp::LogicOp(LogicOp::Or)))
            .expect("outer logical or");
        assert!(matches!(
            or.rhs(),
            Some(ast::Expr::BinExpr(ref rhs))
                if rhs.op_kind() == Some(BinaryOp::LogicOp(LogicOp::And))
        ));
    }
}
