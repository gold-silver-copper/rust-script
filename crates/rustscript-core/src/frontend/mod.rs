mod admit;
mod bytes;
pub(crate) mod intrinsic;
mod lex_policy;

use line_index::LineIndex;
use ra_ap_syntax::{AstNode, Edition, SourceFile, SyntaxKind, WalkEvent, ast};

use crate::{Diagnostic, ParseLimits, Phase};

/// Opaque owner of validated source and its rust-analyzer syntax tree.
///
/// The original source `String` is not retained separately: the lossless
/// rowan green tree reproduces the exact text, and positions resolve through
/// the stored [`LineIndex`].
pub struct ParsedProgram {
    file: ast::SourceFile,
    line_index: LineIndex,
    limits: ParseLimits,
}

impl ParsedProgram {
    /// Resolve a diagnostic location against this parsed source.
    pub fn location(&self, diagnostic: &Diagnostic) -> Option<crate::Location> {
        diagnostic.location(&self.line_index)
    }

    pub(crate) fn file(&self) -> &ast::SourceFile {
        &self.file
    }
    pub(crate) fn limits(&self) -> ParseLimits {
        self.limits
    }
}

pub(crate) fn parse(bytes: &[u8], limits: ParseLimits) -> Result<ParsedProgram, Diagnostic> {
    let source = bytes::validate(bytes, limits)?.to_owned();
    // `LexedStr::new` runs third-party lexer code (its Edition 2024
    // frontmatter probe on a leading `---` has panicked on the pinned
    // release), so it is contained alongside the parser rather than left to
    // unwind out of byte validation.
    contain_unwind(Phase::Lex, "Rust lexer aborted", || {
        lex_policy::validate(&source, limits)
    })?;

    let file = parse_syntax(&source, limits)?;
    Ok(ParsedProgram {
        line_index: LineIndex::new(&source),
        file,
        limits,
    })
}

fn parse_syntax_uncontained(
    source: &str,
    limits: ParseLimits,
) -> Result<ast::SourceFile, Diagnostic> {
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

fn parse_syntax(source: &str, limits: ParseLimits) -> Result<ast::SourceFile, Diagnostic> {
    contain_unwind(Phase::Parse, "Rust parser aborted", || {
        parse_syntax_uncontained(source, limits)
    })
}

// On unwind-capable native targets, run third-party frontend code behind a
// panic boundary and convert an unwind into a structured diagnostic. On
// `wasm32-unknown-unknown` panics abort rather than unwind, so the browser
// worker isolation boundary is the containment instead (see the README).
#[cfg(not(target_arch = "wasm32"))]
fn contain_unwind<T>(
    phase: Phase,
    message: &'static str,
    operation: impl FnOnce() -> Result<T, Diagnostic> + std::panic::UnwindSafe,
) -> Result<T, Diagnostic> {
    match std::panic::catch_unwind(operation) {
        Ok(result) => result,
        Err(_) => Err(Diagnostic::new(phase, message, None)),
    }
}

#[cfg(target_arch = "wasm32")]
fn contain_unwind<T>(
    _phase: Phase,
    _message: &'static str,
    operation: impl FnOnce() -> Result<T, Diagnostic>,
) -> Result<T, Diagnostic> {
    operation()
}

fn validate_tree(file: &ast::SourceFile, limits: ParseLimits) -> Result<(), Diagnostic> {
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
            let _ = crate::parse_bytes(&data, crate::ParseLimits::default());
        }

        #[test]
        fn ascii_parse_admission_terminates(data in prop::collection::vec(0_u8..=127, 0..4096)) {
            let _ = crate::parse_bytes(&data, crate::ParseLimits::default());
        }
    }

    #[test]
    fn rust_analyzer_operator_tree_defines_precedence() {
        let parsed = crate::parse(
            "fn main() { let x = 1_i64 + 2_i64 * 3_i64; let y = false || true && false; }",
            crate::ParseLimits::default(),
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

    #[test]
    fn enforces_syntax_function_and_parameter_limits() {
        let syntax_error = parse_error(crate::parse(
            "fn main() {}",
            crate::ParseLimits {
                max_syntax_elements: 1,
                ..crate::ParseLimits::default()
            },
        ));
        assert_eq!(syntax_error.message, "syntax element limit exceeded");

        let function_error = parse_error(crate::parse(
            "fn a() {} fn main() {}",
            crate::ParseLimits {
                max_functions: 1,
                ..crate::ParseLimits::default()
            },
        ));
        assert_eq!(
            function_error.message,
            "unsupported function limit exceeded"
        );

        let parameter_error = parse_error(crate::parse(
            "fn helper(a: i64, b: i64) {} fn main() {}",
            crate::ParseLimits {
                max_parameters: 1,
                ..crate::ParseLimits::default()
            },
        ));
        assert_eq!(
            parameter_error.message,
            "unsupported parameter limit exceeded"
        );
    }

    #[test]
    fn syntax_element_and_depth_boundaries_are_exact() {
        use ra_ap_syntax::WalkEvent;

        let source = "fn main() { 1_i64; }";
        let parsed = crate::parse(source, crate::ParseLimits::default()).unwrap();
        let mut elements = 0usize;
        let mut depth = 0usize;
        let mut deepest = 0usize;
        for event in parsed.file().syntax().preorder_with_tokens() {
            match event {
                WalkEvent::Enter(_) => {
                    elements += 1;
                    depth += 1;
                    deepest = deepest.max(depth);
                }
                WalkEvent::Leave(_) => depth -= 1,
            }
        }

        let at_limit = crate::ParseLimits {
            max_syntax_elements: elements,
            max_syntax_depth: deepest,
            ..crate::ParseLimits::default()
        };
        crate::parse(source, at_limit).expect("exact limits must accept");

        let element_error = parse_error(crate::parse(
            source,
            crate::ParseLimits {
                max_syntax_elements: elements - 1,
                ..crate::ParseLimits::default()
            },
        ));
        assert_eq!(element_error.message, "syntax element limit exceeded");

        let depth_error = parse_error(crate::parse(
            source,
            crate::ParseLimits {
                max_syntax_depth: deepest - 1,
                ..crate::ParseLimits::default()
            },
        ));
        assert_eq!(depth_error.message, "syntax nesting limit exceeded");
    }

    #[test]
    fn leading_frontmatter_lexer_panic_is_contained() {
        // Regression: `---...` triggers ra_ap_parser 0.0.342's Edition 2024
        // frontmatter probe, which panics on a char boundary inside
        // `LexedStr::new` — before the parser's own catch_unwind. Byte
        // validation must return a diagnostic, never unwind.
        for source in [
            "---{|.y(---|a\n-",
            "---\n",
            "----------",
            "---cargo\n---\nfn main() {}",
        ] {
            let result = crate::parse(source, crate::ParseLimits::default());
            assert!(result.is_err(), "{source:?} must be rejected, not panic");
        }
    }

    #[test]
    fn long_prefix_operator_runs_are_rejected_without_aborting() {
        // Regression: pre-lex bounds must stop these before `SourceFile::parse`
        // recurses itself into a fatal (non-unwinding) stack overflow.
        for operator in ["-", "!", "&", "*"] {
            let source = format!("fn main() {{ let x = {}1_i64; }}", operator.repeat(50_000));
            let error = parse_error(crate::parse(&source, crate::ParseLimits::default()));
            assert_eq!(error.message, "prefix operator nesting limit exceeded");
        }
        // Interleaved operands defeat a consecutive-run bound but still nest
        // one parser frame per `return`; the per-statement counter must stop
        // this before `SourceFile::parse` runs.
        let source = format!(
            "fn main() {{ let x = {}1_i64; }}",
            "return 1_i64 - ".repeat(9_000)
        );
        let error = parse_error(crate::parse(&source, crate::ParseLimits::default()));
        assert_eq!(error.message, "prefix operator nesting limit exceeded");
    }

    #[test]
    fn rust_analyzer_dependencies_are_lockstep_edition_2024() {
        let lock = include_str!("../../../../Cargo.lock");
        assert_eq!(locked_version(lock, "ra_ap_parser"), Some("0.0.342"));
        assert_eq!(locked_version(lock, "ra_ap_syntax"), Some("0.0.342"));

        let lexed = ra_ap_parser::LexedStr::new(
            ra_ap_parser::Edition::Edition2024,
            "fn main() { async {}; }",
        );
        assert!(lexed.errors().next().is_none());
        let parsed = ra_ap_syntax::SourceFile::parse(
            "fn main() { async {}; }",
            ra_ap_syntax::Edition::Edition2024,
        );
        assert!(parsed.errors().is_empty());
    }

    fn locked_version<'a>(lock: &'a str, package: &str) -> Option<&'a str> {
        let marker = format!("name = \"{package}\"");
        let start = lock.find(&marker)?;
        lock[start..]
            .lines()
            .find_map(|line| line.strip_prefix("version = \"")?.strip_suffix('"'))
    }

    fn parse_error(result: Result<crate::ParsedProgram, crate::Diagnostic>) -> crate::Diagnostic {
        match result {
            Ok(_) => panic!("expected parse error"),
            Err(error) => error,
        }
    }
}
