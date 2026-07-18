use std::collections::{HashMap, HashSet};

use ra_ap_syntax::ast::{HasModuleItem, HasName};
use ra_ap_syntax::{AstNode, SmolStr, ast};

use crate::checked_ir::{Block, CheckedProgram, Expression, ExpressionKind, Function, Parameter};
use crate::{Diagnostic, ParsedProgram, Phase, Type, Value};

struct Signature {
    name: SmolStr,
    parameters: Vec<Parameter>,
    return_type: Type,
    explicit_return: bool,
    syntax: ast::Fn,
}

pub(crate) fn check(parsed: &ParsedProgram) -> Result<CheckedProgram, Diagnostic> {
    let signatures = collect_signatures(parsed)?;
    let function_names: HashSet<_> = signatures
        .iter()
        .map(|signature| signature.name.clone())
        .collect();
    let mut functions = Vec::with_capacity(signatures.len());
    let mut main = None;

    for (id, signature) in signatures.into_iter().enumerate() {
        if signature.name == "main" {
            main = Some(id);
        }
        for parameter in &signature.parameters {
            if function_names.contains(&parameter.name) {
                return Err(type_error(
                    "parameter name collides with a function",
                    signature.syntax.syntax(),
                ));
            }
        }
        let body = signature
            .syntax
            .body()
            .ok_or_else(|| type_error("function body is required", signature.syntax.syntax()))?;
        if body.statements().next().is_some() || body.tail_expr().is_some() {
            return Err(type_error(
                "body checking is not yet available for this construct",
                body.syntax(),
            ));
        }
        if signature.return_type != Type::Unit {
            return Err(type_error(
                "function body does not produce its declared return type",
                body.syntax(),
            ));
        }
        functions.push(Function {
            name: signature.name,
            parameters: signature.parameters,
            return_type: signature.return_type,
            explicit_return: signature.explicit_return,
            local_count: 0,
            body: Block {
                statements: Vec::new(),
                tail: Some(Box::new(Expression {
                    kind: ExpressionKind::Value(Value::Unit),
                    ty: Type::Unit,
                    span: crate::diagnostic::span(body.syntax().text_range()),
                })),
                span: crate::diagnostic::span(body.syntax().text_range()),
            },
        });
    }
    let main = main.ok_or_else(|| {
        Diagnostic::new(Phase::Type, "exactly one main function is required", None)
    })?;
    Ok(CheckedProgram { functions, main })
}

fn collect_signatures(parsed: &ParsedProgram) -> Result<Vec<Signature>, Diagnostic> {
    let mut signatures = Vec::new();
    let mut names = HashMap::new();
    let mut main_count = 0usize;
    for item in parsed.file().items() {
        let ast::Item::Fn(function) = item else {
            continue;
        };
        let name_node = function
            .name()
            .ok_or_else(|| type_error("function name is required", function.syntax()))?;
        let name = SmolStr::new(name_node.text());
        validate_function_name(&name, &function)?;
        if names.insert(name.clone(), signatures.len()).is_some() {
            return Err(type_error("duplicate function name", name_node.syntax()));
        }

        let mut parameters = Vec::new();
        let mut parameter_names = HashSet::new();
        if let Some(list) = function.param_list() {
            if list.self_param().is_some() {
                return Err(type_error("self parameters are unsupported", list.syntax()));
            }
            for (id, parameter) in list.params().enumerate() {
                let ast::Pat::IdentPat(pattern) = parameter.pat().ok_or_else(|| {
                    type_error("parameter pattern is required", parameter.syntax())
                })?
                else {
                    return Err(type_error(
                        "parameter must be a bare identifier",
                        parameter.syntax(),
                    ));
                };
                if pattern.mut_token().is_some()
                    || pattern.ref_token().is_some()
                    || pattern.at_token().is_some()
                {
                    return Err(type_error(
                        "parameters are immutable bare identifiers",
                        pattern.syntax(),
                    ));
                }
                let parameter_name_node = pattern
                    .name()
                    .ok_or_else(|| type_error("parameter name is required", pattern.syntax()))?;
                let parameter_name = SmolStr::new(parameter_name_node.text());
                validate_binding_name(&parameter_name, parameter_name_node.syntax())?;
                if !parameter_names.insert(parameter_name.clone()) {
                    return Err(type_error(
                        "duplicate parameter name",
                        parameter_name_node.syntax(),
                    ));
                }
                let ty = parse_type(parameter.ty().as_ref(), parameter.syntax())?;
                parameters.push(Parameter {
                    id,
                    name: parameter_name,
                    ty,
                });
            }
        }
        let explicit_return = function.ret_type().is_some();
        let return_type = match function.ret_type() {
            Some(ret) => parse_type(ret.ty().as_ref(), ret.syntax())?,
            None => Type::Unit,
        };
        if name == "main" {
            main_count += 1;
            if !parameters.is_empty() || explicit_return {
                return Err(type_error(
                    "main must have signature `fn main()`",
                    function.syntax(),
                ));
            }
        }
        signatures.push(Signature {
            name,
            parameters,
            return_type,
            explicit_return,
            syntax: function,
        });
    }
    if main_count != 1 {
        return Err(Diagnostic::new(
            Phase::Type,
            "exactly one main function is required",
            None,
        ));
    }
    Ok(signatures)
}

fn parse_type(
    ty: Option<&ast::Type>,
    fallback: &ra_ap_syntax::SyntaxNode,
) -> Result<Type, Diagnostic> {
    let ty = ty.ok_or_else(|| type_error("type annotation is required", fallback))?;
    match ty {
        ast::Type::PathType(path) => match path
            .path()
            .and_then(|path| path.segment())
            .and_then(|segment| segment.name_ref())
            .map(|name| SmolStr::new(name.text()))
        {
            Some(name) if name == "i64" => Ok(Type::I64),
            Some(name) if name == "bool" => Ok(Type::Bool),
            _ => Err(type_error(
                "only i64, bool, and () types are supported",
                ty.syntax(),
            )),
        },
        ast::Type::TupleType(tuple) if tuple.fields().next().is_none() => Ok(Type::Unit),
        _ => Err(type_error(
            "only i64, bool, and () types are supported",
            ty.syntax(),
        )),
    }
}

fn validate_function_name(name: &str, function: &ast::Fn) -> Result<(), Diagnostic> {
    if name == "main" {
        return Ok(());
    }
    validate_binding_name(name, function.syntax())
}

fn validate_binding_name(name: &str, node: &ra_ap_syntax::SyntaxNode) -> Result<(), Diagnostic> {
    const RESERVED: &[&str] = &["_", "i64", "bool", "println", "main"];
    if RESERVED.contains(&name) {
        return Err(type_error("reserved identifier", node));
    }
    Ok(())
}

fn type_error(message: &str, node: &ra_ap_syntax::SyntaxNode) -> Diagnostic {
    Diagnostic::new(
        Phase::Type,
        message,
        Some(crate::diagnostic::span(node.text_range())),
    )
}

#[cfg(test)]
mod tests {
    use crate::{Limits, check_source};

    #[test]
    fn checks_empty_main() {
        assert!(check_source("fn main() {}", Limits::default()).is_ok());
    }

    #[test]
    fn collects_forward_and_recursive_signatures() {
        assert!(check_source("fn a(x: i64) -> i64 {} fn main() {}", Limits::default()).is_err());
        assert!(check_source("fn a(x: i64) {} fn main() {}", Limits::default()).is_ok());
    }

    #[test]
    fn rejects_main_and_signature_errors() {
        for source in [
            "fn helper() {}",
            "fn main(x: i64) {}",
            "fn main() -> () {}",
            "fn f(x: i64, x: i64) {} fn main() {}",
            "fn f() {} fn f() {} fn main() {}",
            "fn f(main: i64) {} fn main() {}",
            "fn f(x: u64) {} fn main() {}",
        ] {
            assert!(check_source(source, Limits::default()).is_err(), "{source}");
        }
    }
}
