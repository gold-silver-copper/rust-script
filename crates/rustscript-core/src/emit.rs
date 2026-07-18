use ra_ap_syntax::ast::{BinaryOp, CmpOp, LogicOp, Ordering, UnaryOp};

use crate::checked_ir::{Block, Expression, ExpressionKind, Function, Statement};
use crate::{CheckedProgram, Type, Value};

pub(crate) fn format(program: &CheckedProgram) -> String {
    let mut emitter = Emitter {
        program,
        output: String::new(),
        locals: Vec::new(),
    };
    for (index, function) in program.functions.iter().enumerate() {
        if index != 0 {
            emitter.output.push('\n');
        }
        emitter.function(function);
    }
    emitter.output
}

struct Emitter<'a> {
    program: &'a CheckedProgram,
    output: String,
    locals: Vec<String>,
}

impl Emitter<'_> {
    fn function(&mut self, function: &Function) {
        self.locals = vec![String::new(); function.local_count];
        self.output.push_str("fn ");
        self.output.push_str(&function.name);
        self.output.push('(');
        for (index, parameter) in function.parameters.iter().enumerate() {
            if index != 0 {
                self.output.push_str(", ");
            }
            if let Some(slot) = self.locals.get_mut(parameter.id) {
                *slot = parameter.name.to_string();
            }
            self.output.push_str(&parameter.name);
            self.output.push_str(": ");
            self.output.push_str(type_name(parameter.ty));
        }
        self.output.push(')');
        if function.explicit_return {
            self.output.push_str(" -> ");
            self.output.push_str(type_name(function.return_type));
        }
        self.output.push(' ');
        self.block(&function.body, 0);
        self.output.push('\n');
    }

    fn block(&mut self, block: &Block, indent: usize) {
        self.output.push('{');
        if block.statements.is_empty() && is_implicit_unit(block.tail.as_deref()) {
            self.output.push('}');
            return;
        }
        self.output.push('\n');
        for statement in &block.statements {
            self.indent(indent + 1);
            self.statement(statement, indent + 1);
            self.output.push('\n');
        }
        if let Some(tail) = block.tail.as_deref()
            && !is_implicit_unit(Some(tail))
        {
            self.indent(indent + 1);
            self.expression(tail, indent + 1);
            self.output.push('\n');
        }
        self.indent(indent);
        self.output.push('}');
    }

    fn statement(&mut self, statement: &Statement, indent: usize) {
        match statement {
            Statement::Let {
                id,
                name,
                mutable,
                annotation,
                initializer,
            } => {
                if let Some(slot) = self.locals.get_mut(*id) {
                    *slot = name.to_string();
                }
                self.output.push_str("let ");
                if *mutable {
                    self.output.push_str("mut ");
                }
                self.output.push_str(name);
                if let Some(ty) = annotation {
                    self.output.push_str(": ");
                    self.output.push_str(type_name(*ty));
                }
                self.output.push_str(" = ");
                self.expression(initializer, indent);
                self.output.push(';');
            }
            Statement::Assign { id, value, .. } => {
                self.local(*id);
                self.output.push_str(" = ");
                self.expression(value, indent);
                self.output.push(';');
            }
            Statement::While {
                condition, body, ..
            } => {
                self.output.push_str("while ");
                self.expression(condition, indent);
                self.output.push(' ');
                self.block(body, indent);
            }
            Statement::Return { value, .. } => {
                self.output.push_str("return");
                if value.ty != Type::Unit
                    || !matches!(value.kind, ExpressionKind::Value(Value::Unit))
                {
                    self.output.push(' ');
                    self.expression(value, indent);
                }
                self.output.push(';');
            }
            Statement::Break(_) => self.output.push_str("break;"),
            Statement::Continue(_) => self.output.push_str("continue;"),
            Statement::Expression(expression) => {
                self.expression(expression, indent);
                self.output.push(';');
            }
        }
    }

    fn expression(&mut self, expression: &Expression, indent: usize) {
        match &expression.kind {
            ExpressionKind::Value(Value::I64(value)) => {
                self.output.push_str(&value.to_string());
                self.output.push_str("_i64");
            }
            ExpressionKind::Value(Value::Bool(value)) => {
                self.output.push_str(if *value { "true" } else { "false" })
            }
            ExpressionKind::Value(Value::Unit) => self.output.push_str("()"),
            ExpressionKind::Local(id) => self.local(*id),
            ExpressionKind::Call {
                function,
                arguments,
            } => {
                if let Some(function) = self.program.functions.get(*function) {
                    self.output.push_str(&function.name);
                }
                for_invalid_id(
                    &mut self.output,
                    self.program.functions.get(*function).is_none(),
                );
                self.output.push('(');
                for (index, argument) in arguments.iter().enumerate() {
                    if index != 0 {
                        self.output.push_str(", ");
                    }
                    self.expression(argument, indent);
                }
                self.output.push(')');
            }
            ExpressionKind::Unary { op, operand } => {
                self.output.push('(');
                self.output.push_str(match op {
                    UnaryOp::Neg => "-",
                    UnaryOp::Not => "!",
                    UnaryOp::Deref => "*",
                });
                self.expression(operand, indent);
                self.output.push(')');
            }
            ExpressionKind::Binary { op, lhs, rhs } => {
                self.output.push('(');
                self.expression(lhs, indent);
                self.output.push(' ');
                self.output.push_str(binary_text(*op));
                self.output.push(' ');
                self.expression(rhs, indent);
                self.output.push(')');
            }
            ExpressionKind::Block(block) => self.block(block, indent),
            ExpressionKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.output.push_str("(if ");
                self.expression(condition, indent);
                self.output.push(' ');
                self.block(then_branch, indent);
                self.output.push_str(" else ");
                self.block(else_branch, indent);
                self.output.push(')');
            }
        }
    }

    fn local(&mut self, id: usize) {
        if let Some(name) = self.locals.get(id).filter(|name| !name.is_empty()) {
            self.output.push_str(name);
        } else {
            self.output.push_str("__invalid_local");
        }
    }

    fn indent(&mut self, depth: usize) {
        for _ in 0..depth {
            self.output.push_str("    ");
        }
    }
}

fn is_implicit_unit(expression: Option<&Expression>) -> bool {
    matches!(
        expression,
        Some(Expression {
            kind: ExpressionKind::Value(Value::Unit),
            ..
        })
    )
}

fn type_name(ty: Type) -> &'static str {
    match ty {
        Type::I64 => "i64",
        Type::Bool => "bool",
        Type::Unit => "()",
    }
}

fn binary_text(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::LogicOp(LogicOp::And) => "&&",
        BinaryOp::LogicOp(LogicOp::Or) => "||",
        BinaryOp::CmpOp(CmpOp::Eq { negated: false }) => "==",
        BinaryOp::CmpOp(CmpOp::Eq { negated: true }) => "!=",
        BinaryOp::CmpOp(CmpOp::Ord {
            ordering: Ordering::Less,
            strict: true,
        }) => "<",
        BinaryOp::CmpOp(CmpOp::Ord {
            ordering: Ordering::Less,
            strict: false,
        }) => "<=",
        BinaryOp::CmpOp(CmpOp::Ord {
            ordering: Ordering::Greater,
            strict: true,
        }) => ">",
        BinaryOp::CmpOp(CmpOp::Ord {
            ordering: Ordering::Greater,
            strict: false,
        }) => ">=",
        BinaryOp::ArithOp(op) => match op {
            ra_ap_syntax::ast::ArithOp::Add => "+",
            ra_ap_syntax::ast::ArithOp::Sub => "-",
            ra_ap_syntax::ast::ArithOp::Mul => "*",
            ra_ap_syntax::ast::ArithOp::Div => "/",
            ra_ap_syntax::ast::ArithOp::Rem => "%",
            _ => "__unsupported_operator",
        },
        BinaryOp::Assignment { .. } => "=",
    }
}

fn for_invalid_id(output: &mut String, invalid: bool) {
    if invalid {
        output.push_str("__invalid_function");
    }
}

#[cfg(test)]
mod tests {
    use crate::{Limits, check_source, format};

    #[test]
    fn canonical_output_is_idempotent() {
        let source = "fn add(x:i64,y:i64)->i64{x+y} fn main(){let mut x=add(1_i64,2_i64);while x<4_i64{x=x+1_i64;} if x==4_i64 {()} else {()};}";
        let first = format(&check_source(source, Limits::default()).unwrap());
        let second = format(&check_source(&first, Limits::default()).unwrap());
        assert_eq!(first, second);
    }
}
