//! Rust-analyzer-backed validation for the one supported macro invocation.
//!
//! Macro token trees are intentionally opaque to the normal Rust expression
//! grammar.  We validate the fixed `println!("{}", expression)` envelope using
//! typed path/token-tree APIs, then parse the bounded expression through the
//! same `SourceFile::parse` entry point used by the main frontend.  No project
//! lexer, expression parser, or precedence implementation is introduced.

use ra_ap_syntax::ast::HasModuleItem;
use ra_ap_syntax::{
    AstNode, Edition, NodeOrToken, SourceFile, SyntaxKind, TextRange, TextSize, ast,
};

use crate::{Diagnostic, Limits, Phase};

const WRAPPER_PREFIX: &str = "fn __rustscript_intrinsic(){let __rustscript_value=(";
const WRAPPER_SUFFIX: &str = ");}";

pub(crate) struct PrintExpression {
    pub(crate) expression: ast::Expr,
    pub(crate) source_range: TextRange,
    pub(crate) range_map: ExpressionRangeMap,
}

#[derive(Clone, Copy)]
pub(crate) struct ExpressionRangeMap {
    wrapper_start: TextSize,
    source_start: TextSize,
    source_range: TextRange,
}

impl ExpressionRangeMap {
    pub(crate) fn text_range(self, range: TextRange) -> Option<TextRange> {
        let wrapper_start = u32::from(self.wrapper_start);
        let source_start = u32::from(self.source_start);
        let start = u32::from(range.start())
            .checked_sub(wrapper_start)?
            .checked_add(source_start)?;
        let end = u32::from(range.end())
            .checked_sub(wrapper_start)?
            .checked_add(source_start)?;
        let mapped = TextRange::new(TextSize::new(start), TextSize::new(end));
        (self.source_range.contains_range(mapped)).then_some(mapped)
    }

    pub(crate) fn span(self, span: crate::Span) -> Option<crate::Span> {
        let range = TextRange::new(
            TextSize::new(u32::try_from(span.start).ok()?),
            TextSize::new(u32::try_from(span.end).ok()?),
        );
        self.text_range(range).map(crate::diagnostic::span)
    }
}

pub(crate) fn print_expression(
    macro_expression: &ast::MacroExpr,
    limits: Limits,
) -> Result<PrintExpression, Diagnostic> {
    let call = macro_expression.macro_call().ok_or_else(|| {
        error(
            "println macro call is incomplete",
            macro_expression.syntax().text_range(),
        )
    })?;
    let name = call
        .path()
        .and_then(|path| path.as_single_name_ref())
        .ok_or_else(|| {
            error(
                "macro target must be the bare name `println`",
                call.syntax().text_range(),
            )
        })?;
    if name.text() != "println" {
        return Err(error(
            "only the `println` intrinsic is supported",
            name.syntax().text_range(),
        ));
    }
    let tree = call
        .token_tree()
        .ok_or_else(|| error("println arguments are required", call.syntax().text_range()))?;
    if tree
        .left_delimiter_token()
        .is_none_or(|token| token.kind() != SyntaxKind::L_PAREN)
        || tree
            .right_delimiter_token()
            .is_none_or(|token| token.kind() != SyntaxKind::R_PAREN)
    {
        return Err(error(
            "println must use parentheses",
            tree.syntax().text_range(),
        ));
    }

    let elements: Vec<_> = tree.token_trees_and_tokens().collect();
    if elements.len() < 4 {
        return Err(error(
            "println requires a placeholder and a value",
            tree.syntax().text_range(),
        ));
    }
    let interior = &elements[1..elements.len() - 1];
    let significant: Vec<_> = interior
        .iter()
        .enumerate()
        .filter(|(_, element)| !is_trivia(element))
        .collect();
    let Some((_, placeholder)) = significant.first() else {
        return Err(error(
            "println requires a placeholder and a value",
            tree.syntax().text_range(),
        ));
    };
    let Some(placeholder_token) = placeholder.as_token() else {
        return Err(error(
            "println placeholder must be exactly `\"{}\"`",
            element_range(placeholder),
        ));
    };
    if placeholder_token.kind() != SyntaxKind::STRING || placeholder_token.text() != "\"{}\"" {
        return Err(error(
            "println placeholder must be exactly `\"{}\"`",
            placeholder_token.text_range(),
        ));
    }
    let Some((comma_index, comma)) = significant.get(1) else {
        return Err(error(
            "println requires a comma and one value",
            tree.syntax().text_range(),
        ));
    };
    if comma
        .as_token()
        .is_none_or(|token| token.kind() != SyntaxKind::COMMA)
    {
        return Err(error(
            "println requires a comma after the placeholder",
            element_range(comma),
        ));
    }
    let Some((_, first_expression_element)) = significant.get(2) else {
        return Err(error(
            "println value expression is required",
            tree.syntax().text_range(),
        ));
    };
    if significant
        .last()
        .and_then(|(_, element)| element.as_token())
        .is_some_and(|token| token.kind() == SyntaxKind::COMMA)
    {
        return Err(error(
            "trailing commas are unsupported",
            significant
                .last()
                .map_or(tree.syntax().text_range(), |(_, element)| {
                    element_range(element)
                }),
        ));
    }

    let expression_elements = &interior[comma_index + 1..];
    let expression_text: String = expression_elements
        .iter()
        .map(ToString::to_string)
        .collect();
    let source_range = TextRange::new(
        element_range(first_expression_element).start(),
        significant.last().map_or(
            element_range(first_expression_element).end(),
            |(_, element)| element_range(element).end(),
        ),
    );
    let source_expression_start = expression_elements
        .first()
        .map_or(source_range.start(), |element| {
            element_range(element).start()
        });
    let wrapper = format!("{WRAPPER_PREFIX}{expression_text}{WRAPPER_SUFFIX}");
    let file = parse_wrapper(&wrapper, limits).map_err(|mut error| {
        error.span = Some(crate::diagnostic::span(source_range));
        error
    })?;
    let mut items = file.items();
    let Some(ast::Item::Fn(function)) = items.next() else {
        return Err(error("println value expression is invalid", source_range));
    };
    if items.next().is_some() {
        return Err(error("println value expression is invalid", source_range));
    }
    let body = function
        .body()
        .ok_or_else(|| error("println value expression is invalid", source_range))?;
    let mut statements = body.statements();
    let Some(ast::Stmt::LetStmt(statement)) = statements.next() else {
        return Err(error("println value expression is invalid", source_range));
    };
    if statements.next().is_some() || body.tail_expr().is_some() {
        return Err(error("println value expression is invalid", source_range));
    }
    let expression = statement
        .initializer()
        .ok_or_else(|| error("println value expression is invalid", source_range))?;
    Ok(PrintExpression {
        expression,
        source_range,
        range_map: ExpressionRangeMap {
            wrapper_start: TextSize::new(WRAPPER_PREFIX.len() as u32),
            source_start: source_expression_start,
            source_range,
        },
    })
}

fn is_trivia(element: &&NodeOrToken<ast::TokenTree, ra_ap_syntax::SyntaxToken>) -> bool {
    element
        .as_token()
        .is_some_and(|token| matches!(token.kind(), SyntaxKind::WHITESPACE | SyntaxKind::COMMENT))
}

fn element_range(element: &NodeOrToken<ast::TokenTree, ra_ap_syntax::SyntaxToken>) -> TextRange {
    match element {
        NodeOrToken::Node(tree) => tree.syntax().text_range(),
        NodeOrToken::Token(token) => token.text_range(),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn parse_wrapper(source: &str, limits: Limits) -> Result<ast::SourceFile, Diagnostic> {
    std::panic::catch_unwind(|| parse_wrapper_uncontained(source, limits))
        .map_err(|_| Diagnostic::new(Phase::Parse, "Rust parser aborted", None))
        .and_then(std::convert::identity)
}

#[cfg(target_arch = "wasm32")]
fn parse_wrapper(source: &str, limits: Limits) -> Result<ast::SourceFile, Diagnostic> {
    parse_wrapper_uncontained(source, limits)
}

fn parse_wrapper_uncontained(source: &str, limits: Limits) -> Result<ast::SourceFile, Diagnostic> {
    let parsed = SourceFile::parse(source, Edition::Edition2024);
    if !parsed.errors().is_empty() {
        return Err(Diagnostic::new(
            Phase::Parse,
            "println value expression is invalid",
            None,
        ));
    }
    let file = parsed.tree();
    super::validate_tree(&file, limits)?;
    drop(parsed);
    Ok(file)
}

fn error(message: &str, range: TextRange) -> Diagnostic {
    Diagnostic::new(Phase::Parse, message, Some(crate::diagnostic::span(range)))
}
