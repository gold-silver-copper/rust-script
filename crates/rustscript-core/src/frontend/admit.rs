use ra_ap_syntax::ast::{HasAttrs, HasGenericParams, HasModuleItem, HasVisibility};
use ra_ap_syntax::{AstNode, ast};

use crate::{Diagnostic, Limits, Phase};

pub(super) fn validate(file: &ast::SourceFile, limits: Limits) -> Result<(), Diagnostic> {
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

    for node in file.syntax().descendants() {
        if let Some(expr) = ast::Expr::cast(node.clone()) {
            match expr {
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
        }
        if let Some(stmt) = ast::Stmt::cast(node.clone()) {
            match stmt {
                ast::Stmt::ExprStmt(_) | ast::Stmt::LetStmt(_) => {}
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
            match ty {
                ast::Type::PathType(_) | ast::Type::TupleType(_) => {}
                _ => return Err(unsupported("type", ty.syntax())),
            }
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

fn validate_function(function: &ast::Fn, limits: Limits) -> Result<(), Diagnostic> {
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
    let parameters = function
        .param_list()
        .map_or(0, |list| list.params().count());
    if parameters > limits.max_parameters {
        return Err(unsupported("parameter limit exceeded", function.syntax()));
    }
    Ok(())
}

fn unsupported(what: &str, node: &ra_ap_syntax::SyntaxNode) -> Diagnostic {
    Diagnostic::new(
        Phase::Parse,
        format!("unsupported {what}"),
        Some(crate::diagnostic::span(node.text_range())),
    )
}

#[cfg(test)]
mod tests {
    use crate::{Limits, parse};

    #[test]
    fn accepts_supported_typed_shapes() {
        parse(
            "fn main() { let mut x: i64 = 1_i64; while x < 2_i64 { x = x + 1_i64; } }",
            Limits::default(),
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
        ] {
            assert!(parse(source, Limits::default()).is_err(), "{source}");
        }
    }
}
