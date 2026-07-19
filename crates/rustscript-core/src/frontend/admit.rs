use ra_ap_syntax::ast::{
    ArithOp, BinaryOp, HasArgList, HasAttrs, HasGenericArgs, HasGenericParams, HasLoopBody,
    HasModuleItem, HasName, HasVisibility, LiteralKind, LogicOp, UnaryOp,
};
use ra_ap_syntax::{AstNode, SyntaxKind, ast};

use crate::{Diagnostic, ParseLimits, Phase};

pub(super) fn validate(file: &ast::SourceFile, limits: ParseLimits) -> Result<(), Diagnostic> {
    let mut functions = 0usize;
    for item in file.items() {
        let ast::Item::Fn(function) = item else {
            return Err(unsupported("top-level item", item.syntax()));
        };
        functions += 1;
        if functions > limits.max_functions {
            return Err(unsupported("function limit exceeded", function.syntax()));
        }
        validate_function(&function, limits)?;
    }

    validate_descendants(file.syntax(), limits)
}

fn validate_descendants(root: &ra_ap_syntax::SyntaxNode, limits: ParseLimits) -> Result<(), Diagnostic> {
    // rust-analyzer does not expose empty statements as an `ast::Stmt` variant.
    // The subset has no empty statement, so reject semicolons not owned by one
    // of the two statement nodes that can legally contain them.
    for token in root
        .descendants_with_tokens()
        .filter_map(|element| element.into_token())
    {
        if token.kind() == SyntaxKind::SEMICOLON
            && !token.parent().is_some_and(|parent| {
                matches!(parent.kind(), SyntaxKind::LET_STMT | SyntaxKind::EXPR_STMT)
            })
        {
            return Err(unsupported_range("empty statement", token.text_range()));
        }
    }

    for node in root.descendants() {
        if let Some(expr) = ast::Expr::cast(node.clone()) {
            match &expr {
                ast::Expr::BinExpr(_)
                | ast::Expr::BlockExpr(_)
                | ast::Expr::BreakExpr(_)
                | ast::Expr::CallExpr(_)
                | ast::Expr::ContinueExpr(_)
                | ast::Expr::IfExpr(_)
                | ast::Expr::Literal(_)
                | ast::Expr::MacroExpr(_)
                | ast::Expr::ParenExpr(_)
                | ast::Expr::PathExpr(_)
                | ast::Expr::PrefixExpr(_)
                | ast::Expr::ReturnExpr(_)
                | ast::Expr::TupleExpr(_)
                | ast::Expr::WhileExpr(_) => {}
                _ => return Err(unsupported("expression", expr.syntax())),
            }
            validate_expression_policy(&expr)?;
            if let ast::Expr::CallExpr(call) = &expr
                && call
                    .arg_list()
                    .is_some_and(|list| has_trailing_comma(list.syntax()))
            {
                return Err(unsupported("trailing comma", call.syntax()));
            }
            if let ast::Expr::MacroExpr(macro_expression) = &expr {
                let is_statement = macro_expression
                    .syntax()
                    .parent()
                    .and_then(ast::ExprStmt::cast)
                    .is_some_and(|statement| statement.semicolon_token().is_some());
                if !is_statement {
                    return Err(unsupported(
                        "println outside a terminated statement",
                        macro_expression.syntax(),
                    ));
                }
                let input = super::intrinsic::print_expression(macro_expression, limits).map_err(
                    |mut error| {
                        if error.span.is_none() {
                            error.span = Some(crate::diagnostic::span(
                                macro_expression.syntax().text_range(),
                            ));
                        }
                        error
                    },
                )?;
                validate_descendants(input.expression.syntax(), limits).map_err(|mut error| {
                    error.span = error
                        .span
                        .and_then(|span| input.range_map.span(span))
                        .or_else(|| Some(crate::diagnostic::span(input.source_range)));
                    error
                })?;
            }
        }
        if let Some(stmt) = ast::Stmt::cast(node.clone()) {
            match &stmt {
                ast::Stmt::ExprStmt(_) => {}
                ast::Stmt::LetStmt(statement) => validate_let_statement(statement)?,
                ast::Stmt::Item(_)
                    if node
                        .parent()
                        .is_some_and(|parent| ast::StmtList::can_cast(parent.kind())) =>
                {
                    return Err(unsupported("nested item", stmt.syntax()));
                }
                ast::Stmt::Item(_) => {}
            }
        }
        if let Some(ty) = ast::Type::cast(node.clone()) {
            validate_type(&ty)?;
        }
        if let Some(pat) = ast::Pat::cast(node.clone())
            && !matches!(pat, ast::Pat::IdentPat(_))
        {
            return Err(unsupported("pattern", pat.syntax()));
        }
        if ast::Attr::can_cast(node.kind()) {
            return Err(unsupported("attribute", &node));
        }
    }
    Ok(())
}

fn validate_expression_policy(expression: &ast::Expr) -> Result<(), Diagnostic> {
    validate_statement_position(expression)?;
    match expression {
        ast::Expr::BinExpr(binary) => validate_binary(binary),
        ast::Expr::BlockExpr(_) => Ok(()),
        ast::Expr::BreakExpr(expression)
            if expression.expr().is_none() && expression.lifetime().is_none() =>
        {
            Ok(())
        }
        ast::Expr::BreakExpr(expression) => {
            Err(unsupported("break value or label", expression.syntax()))
        }
        ast::Expr::CallExpr(call)
            if call.expr().is_some_and(
                |target| matches!(target, ast::Expr::PathExpr(ref path) if is_bare_path(path)),
            ) && call.arg_list().is_some() =>
        {
            Ok(())
        }
        ast::Expr::CallExpr(call) => Err(unsupported("call target", call.syntax())),
        ast::Expr::ContinueExpr(expression) if expression.lifetime().is_none() => Ok(()),
        ast::Expr::ContinueExpr(expression) => {
            Err(unsupported("continue label", expression.syntax()))
        }
        ast::Expr::IfExpr(expression)
            if expression.condition().is_some()
                && expression.then_branch().is_some()
                && matches!(expression.else_branch(), Some(ast::ElseBranch::Block(_))) =>
        {
            Ok(())
        }
        ast::Expr::IfExpr(expression) => Err(unsupported("if shape", expression.syntax())),
        ast::Expr::Literal(literal)
            if matches!(
                literal.kind(),
                LiteralKind::Bool(_) | LiteralKind::IntNumber(_)
            ) =>
        {
            Ok(())
        }
        ast::Expr::Literal(literal) => Err(unsupported("literal", literal.syntax())),
        ast::Expr::MacroExpr(_) => Ok(()),
        ast::Expr::ParenExpr(expression) if expression.expr().is_some() => Ok(()),
        ast::Expr::ParenExpr(expression) => {
            Err(unsupported("parenthesized expression", expression.syntax()))
        }
        ast::Expr::PathExpr(path) if is_bare_path(path) => Ok(()),
        ast::Expr::PathExpr(path) => Err(unsupported("path", path.syntax())),
        ast::Expr::PrefixExpr(prefix)
            if matches!(prefix.op_kind(), Some(UnaryOp::Neg | UnaryOp::Not))
                && prefix.expr().is_some() =>
        {
            Ok(())
        }
        ast::Expr::PrefixExpr(prefix) => Err(unsupported("unary operator", prefix.syntax())),
        ast::Expr::ReturnExpr(_) => Ok(()),
        ast::Expr::TupleExpr(tuple) if tuple.fields().next().is_none() => Ok(()),
        ast::Expr::TupleExpr(tuple) => Err(unsupported("tuple expression", tuple.syntax())),
        ast::Expr::WhileExpr(expression)
            if expression.condition().is_some() && expression.loop_body().is_some() =>
        {
            Ok(())
        }
        ast::Expr::WhileExpr(expression) => Err(unsupported("while shape", expression.syntax())),
        _ => Err(unsupported("expression", expression.syntax())),
    }
}

fn validate_binary(binary: &ast::BinExpr) -> Result<(), Diagnostic> {
    let supported = match binary.op_kind() {
        Some(BinaryOp::Assignment { op: None }) => binary
            .lhs()
            .is_some_and(|lhs| matches!(lhs, ast::Expr::PathExpr(ref path) if is_bare_path(path))),
        Some(
            BinaryOp::ArithOp(
                ArithOp::Add | ArithOp::Sub | ArithOp::Mul | ArithOp::Div | ArithOp::Rem,
            )
            | BinaryOp::LogicOp(LogicOp::And | LogicOp::Or)
            | BinaryOp::CmpOp(_),
        ) => binary.lhs().is_some(),
        _ => false,
    } && binary.rhs().is_some();
    if supported {
        Ok(())
    } else {
        Err(unsupported("binary operator", binary.syntax()))
    }
}

fn validate_statement_position(expression: &ast::Expr) -> Result<(), Diagnostic> {
    if matches!(expression, ast::Expr::WhileExpr(_))
        && expression
            .syntax()
            .parent()
            .is_some_and(|parent| ast::StmtList::can_cast(parent.kind()))
    {
        return Ok(());
    }
    let Some(statement) = expression.syntax().parent().and_then(ast::ExprStmt::cast) else {
        return match expression {
            ast::Expr::ReturnExpr(_) | ast::Expr::BreakExpr(_) | ast::Expr::ContinueExpr(_) => Err(
                unsupported("control expression position", expression.syntax()),
            ),
            ast::Expr::WhileExpr(_) => Err(unsupported(
                "while expression position",
                expression.syntax(),
            )),
            ast::Expr::BinExpr(binary)
                if matches!(binary.op_kind(), Some(BinaryOp::Assignment { .. })) =>
            {
                Err(unsupported(
                    "assignment expression position",
                    expression.syntax(),
                ))
            }
            _ => Ok(()),
        };
    };

    let terminated = statement.semicolon_token().is_some();
    let valid = match expression {
        ast::Expr::WhileExpr(_) => !terminated,
        ast::Expr::ReturnExpr(_)
        | ast::Expr::BreakExpr(_)
        | ast::Expr::ContinueExpr(_)
        | ast::Expr::MacroExpr(_) => terminated,
        ast::Expr::BinExpr(binary)
            if matches!(binary.op_kind(), Some(BinaryOp::Assignment { .. })) =>
        {
            terminated
        }
        _ => terminated || is_block_tail(&statement, expression),
    };
    if valid {
        Ok(())
    } else {
        Err(unsupported(
            "expression statement position",
            expression.syntax(),
        ))
    }
}

fn is_block_tail(statement: &ast::ExprStmt, expression: &ast::Expr) -> bool {
    statement
        .syntax()
        .parent()
        .and_then(ast::StmtList::cast)
        .and_then(|list| list.syntax().parent())
        .and_then(ast::BlockExpr::cast)
        .and_then(|block| block.tail_expr())
        .is_some_and(|tail| tail.syntax() == expression.syntax())
}

fn validate_let_statement(statement: &ast::LetStmt) -> Result<(), Diagnostic> {
    let valid_pattern = statement.pat().is_some_and(|pattern| {
        matches!(
            pattern,
            ast::Pat::IdentPat(ref pattern)
                if pattern.name().is_some()
                    && pattern.ref_token().is_none()
                    && pattern.at_token().is_none()
        )
    });
    if statement.let_else().is_none() && statement.initializer().is_some() && valid_pattern {
        Ok(())
    } else {
        Err(unsupported("let statement", statement.syntax()))
    }
}

fn validate_type(ty: &ast::Type) -> Result<(), Diagnostic> {
    let supported = match ty {
        ast::Type::PathType(path_type) => path_type
            .path()
            .and_then(|path| bare_path_name(&path))
            .is_some_and(|name| matches!(name.as_str(), "i64" | "bool")),
        ast::Type::TupleType(tuple) => tuple.fields().next().is_none(),
        _ => false,
    };
    if supported {
        Ok(())
    } else {
        Err(unsupported("type", ty.syntax()))
    }
}

fn is_bare_path(path_expression: &ast::PathExpr) -> bool {
    path_expression
        .path()
        .and_then(|path| bare_path_name(&path))
        .is_some()
}

fn bare_path_name(path: &ast::Path) -> Option<String> {
    if path.qualifier().is_some() || path.coloncolon_token().is_some() {
        return None;
    }
    let segment = path.segment()?;
    if segment.generic_arg_list().is_some() || segment.parenthesized_arg_list().is_some() {
        return None;
    }
    Some(segment.name_ref()?.text().to_string())
}

fn validate_function(function: &ast::Fn, limits: ParseLimits) -> Result<(), Diagnostic> {
    if function.attrs().next().is_some()
        || function.generic_param_list().is_some()
        || function.where_clause().is_some()
        || function.visibility().is_some()
        || function.abi().is_some()
        || function.async_token().is_some()
        || function.const_token().is_some()
        || function.unsafe_token().is_some()
        || function.safe_token().is_some()
    {
        return Err(unsupported("function modifier", function.syntax()));
    }
    if function.name().is_none() || function.param_list().is_none() || function.body().is_none() {
        return Err(unsupported("function shape", function.syntax()));
    }
    if let Some(parameters) = function.param_list() {
        if parameters.self_param().is_some() {
            return Err(unsupported("self parameter", parameters.syntax()));
        }
        for parameter in parameters.params() {
            let valid_pattern = parameter.pat().is_some_and(|pattern| {
                matches!(
                    pattern,
                    ast::Pat::IdentPat(ref pattern)
                        if pattern.name().is_some()
                            && pattern.mut_token().is_none()
                            && pattern.ref_token().is_none()
                            && pattern.at_token().is_none()
                )
            });
            if !valid_pattern || parameter.ty().is_none() {
                return Err(unsupported("parameter", parameter.syntax()));
            }
        }
    }
    let parameters = function
        .param_list()
        .map_or(0, |list| list.params().count());
    if parameters > limits.max_parameters {
        return Err(unsupported("parameter limit exceeded", function.syntax()));
    }
    if function
        .param_list()
        .is_some_and(|list| has_trailing_comma(list.syntax()))
    {
        return Err(unsupported("trailing comma", function.syntax()));
    }
    Ok(())
}

fn has_trailing_comma(node: &ra_ap_syntax::SyntaxNode) -> bool {
    let mut token = node.last_token().and_then(|token| token.prev_token());
    while let Some(current) = token {
        if !matches!(current.kind(), SyntaxKind::WHITESPACE | SyntaxKind::COMMENT) {
            return current.kind() == SyntaxKind::COMMA;
        }
        token = current.prev_token();
    }
    false
}

fn unsupported(what: &str, node: &ra_ap_syntax::SyntaxNode) -> Diagnostic {
    unsupported_range(what, node.text_range())
}

fn unsupported_range(what: &str, range: ra_ap_syntax::TextRange) -> Diagnostic {
    Diagnostic::new(
        Phase::Parse,
        format!("unsupported {what}"),
        Some(crate::diagnostic::span(range)),
    )
}

#[cfg(test)]
mod tests {
    use crate::{ParseLimits, parse};

    #[test]
    fn accepts_supported_typed_shapes() {
        parse(
            "fn unit() -> () { return; } fn main() { let mut x: i64 = 1_i64; while x < 2_i64 { x = x + 1_i64; } let y = if true { x } else { 0_i64 }; { y; }; unit(); }",
            ParseLimits::default(),
        )
        .unwrap();
    }

    #[test]
    fn rejects_representative_unsupported_shapes() {
        for source in [
            "struct X {} fn main() {}",
            "fn main() { for x in y {} }",
            "fn main() { let [x] = y; }",
            "fn main() { let x: [i64; 1] = y; }",
            "fn main() { let x = 1_i64; *x; }",
            "fn main() { let x = (1_i64, 2_i64); }",
            "fn main() { let x: (i64, i64) = (); }",
            "fn f() -> i64 { return 1_i64 } fn main() {}",
            "fn main() { let x = return; }",
            "fn main() { while true { break } }",
            "fn main() { let mut x = 1_i64; x += 1_i64; }",
            "fn main() { let mut x = 1_i64; (x) = 2_i64; }",
            "fn main() { let x = if true { 1_i64 }; }",
            "fn main() { let x = (f)(1_i64); }",
            "fn main() { let x = while false {}; }",
            "fn main() { if true { () } else { () } let x = 1_i64; }",
            "fn main() { ; }",
        ] {
            assert!(parse(source, ParseLimits::default()).is_err(), "{source}");
        }
    }
}
