use ra_ap_syntax::ast::{
    BinaryOp, CmpOp, HasArgList, HasGenericArgs, HasLoopBody, HasModuleItem, HasName, LiteralKind,
    LogicOp, Ordering, UnaryOp,
};
use ra_ap_syntax::{AstToken, ast};

use crate::checked_ir::{Block, Expression, ExpressionKind, Function, Statement};
use crate::{CheckedProgram, ParseLimits, ParsedProgram, Type, Value};

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

pub(crate) fn format_parsed(program: &ParsedProgram) -> String {
    let mut emitter = SyntaxEmitter {
        output: String::new(),
        limits: program.limits(),
    };
    for (index, item) in program.file().items().enumerate() {
        if index != 0 {
            emitter.output.push('\n');
        }
        if let ast::Item::Fn(function) = item {
            emitter.function(function);
        }
    }
    emitter.output
}

struct SyntaxEmitter {
    output: String,
    limits: ParseLimits,
}

impl SyntaxEmitter {
    fn function(&mut self, function: ast::Fn) {
        self.output.push_str("fn ");
        if let Some(name) = function.name() {
            self.output.push_str(&name.text());
        } else {
            self.output.push_str("__missing_name");
        }
        self.output.push('(');
        if let Some(parameters) = function.param_list() {
            for (index, parameter) in parameters.params().enumerate() {
                if index != 0 {
                    self.output.push_str(", ");
                }
                self.parameter(parameter);
            }
        }
        self.output.push(')');
        if let Some(ret_type) = function.ret_type() {
            self.output.push_str(" -> ");
            self.ty(ret_type.ty().as_ref());
        }
        self.output.push(' ');
        if let Some(body) = function.body() {
            self.block(body, 0);
        } else {
            self.output.push_str("{}");
        }
        self.output.push('\n');
    }

    fn parameter(&mut self, parameter: ast::Param) {
        self.pattern_name(parameter.pat().as_ref());
        self.output.push_str(": ");
        self.ty(parameter.ty().as_ref());
    }

    fn block(&mut self, block: ast::BlockExpr, indent: usize) {
        self.output.push('{');
        let statements: Vec<_> = block.statements().collect();
        let tail = block.tail_expr();
        if statements.is_empty() && (tail.is_none() || is_syntax_implicit_unit(tail.as_ref())) {
            self.output.push('}');
            return;
        }

        self.output.push('\n');
        for statement in statements {
            self.indent(indent + 1);
            self.statement(statement, indent + 1);
            self.output.push('\n');
        }
        if let Some(ast::Expr::WhileExpr(while_expression)) = tail.clone() {
            self.indent(indent + 1);
            self.while_statement(while_expression, indent + 1);
            self.output.push('\n');
        } else if let Some(tail) = tail
            && !is_syntax_implicit_unit(Some(&tail))
        {
            self.indent(indent + 1);
            self.expression(tail, indent + 1);
            self.output.push('\n');
        }
        self.indent(indent);
        self.output.push('}');
    }

    fn statement(&mut self, statement: ast::Stmt, indent: usize) {
        match statement {
            ast::Stmt::LetStmt(statement) => self.let_statement(statement, indent),
            ast::Stmt::ExprStmt(statement) => {
                if let Some(expression) = statement.expr() {
                    match expression {
                        ast::Expr::BinExpr(binary)
                            if matches!(
                                binary.op_kind(),
                                Some(BinaryOp::Assignment { op: None })
                            ) =>
                        {
                            self.assignment(binary, indent);
                        }
                        ast::Expr::WhileExpr(while_expression) => {
                            self.while_statement(while_expression, indent);
                        }
                        ast::Expr::ReturnExpr(return_expression) => {
                            self.return_statement(return_expression, indent);
                        }
                        ast::Expr::BreakExpr(_) => self.output.push_str("break;"),
                        ast::Expr::ContinueExpr(_) => self.output.push_str("continue;"),
                        ast::Expr::MacroExpr(macro_expression) => {
                            self.print_statement(macro_expression, indent);
                        }
                        expression => {
                            self.expression(expression, indent);
                            self.output.push(';');
                        }
                    }
                }
            }
            ast::Stmt::Item(_) => {}
        }
    }

    fn let_statement(&mut self, statement: ast::LetStmt, indent: usize) {
        self.output.push_str("let ");
        if let Some(ast::Pat::IdentPat(pattern)) = statement.pat() {
            if pattern.mut_token().is_some() {
                self.output.push_str("mut ");
            }
            if let Some(name) = pattern.name() {
                self.output.push_str(&name.text());
            } else {
                self.output.push_str("__missing_name");
            }
        } else {
            self.output.push_str("__missing_name");
        }
        if let Some(ty) = statement.ty() {
            self.output.push_str(": ");
            self.ty(Some(&ty));
        }
        self.output.push_str(" = ");
        if let Some(initializer) = statement.initializer() {
            self.expression(initializer, indent);
        } else {
            self.output.push_str("()");
        }
        self.output.push(';');
    }

    fn assignment(&mut self, binary: ast::BinExpr, indent: usize) {
        if let Some(ast::Expr::PathExpr(path)) = binary.lhs() {
            self.path_name(path);
        } else {
            self.output.push_str("__missing_name");
        }
        self.output.push_str(" = ");
        if let Some(rhs) = binary.rhs() {
            self.expression(rhs, indent);
        } else {
            self.output.push_str("()");
        }
        self.output.push(';');
    }

    fn while_statement(&mut self, expression: ast::WhileExpr, indent: usize) {
        self.output.push_str("while ");
        if let Some(condition) = expression.condition() {
            self.expression(condition, indent);
        } else {
            self.output.push_str("false");
        }
        self.output.push(' ');
        if let Some(body) = expression.loop_body() {
            self.block(body, indent);
        } else {
            self.output.push_str("{}");
        }
    }

    fn return_statement(&mut self, expression: ast::ReturnExpr, indent: usize) {
        self.output.push_str("return");
        if let Some(value) = expression.expr() {
            self.output.push(' ');
            self.expression(value, indent);
        }
        self.output.push(';');
    }

    fn print_statement(&mut self, expression: ast::MacroExpr, indent: usize) {
        self.output.push_str("println!(\"{}\", ");
        if let Ok(input) = crate::frontend::intrinsic::print_expression(&expression, self.limits) {
            self.expression(input.expression, indent);
        } else {
            self.output.push_str("()");
        }
        self.output.push_str(");");
    }

    fn expression(&mut self, expression: ast::Expr, indent: usize) {
        match expression {
            ast::Expr::Literal(literal) => match literal.kind() {
                LiteralKind::Bool(value) => {
                    self.output.push_str(if value { "true" } else { "false" })
                }
                LiteralKind::IntNumber(number) => self.output.push_str(number.syntax().text()),
                _ => self.output.push_str("()"),
            },
            ast::Expr::TupleExpr(tuple) if tuple.fields().next().is_none() => {
                self.output.push_str("()");
            }
            ast::Expr::ParenExpr(paren) => {
                if let Some(inner) = paren.expr() {
                    self.expression(inner, indent);
                } else {
                    self.output.push_str("()");
                }
            }
            ast::Expr::PathExpr(path) => self.path_name(path),
            ast::Expr::PrefixExpr(prefix) => {
                self.output.push('(');
                self.output
                    .push_str(prefix.op_kind().map_or("__unsupported_unary", unary_text));
                if let Some(operand) = prefix.expr() {
                    self.expression(operand, indent);
                } else {
                    self.output.push_str("()");
                }
                self.output.push(')');
            }
            ast::Expr::BinExpr(binary) => {
                self.output.push('(');
                if let Some(lhs) = binary.lhs() {
                    self.expression(lhs, indent);
                } else {
                    self.output.push_str("()");
                }
                self.output.push(' ');
                self.output.push_str(
                    binary
                        .op_kind()
                        .map_or("__unsupported_operator", binary_text),
                );
                self.output.push(' ');
                if let Some(rhs) = binary.rhs() {
                    self.expression(rhs, indent);
                } else {
                    self.output.push_str("()");
                }
                self.output.push(')');
            }
            ast::Expr::CallExpr(call) => self.call_expression(call, indent),
            ast::Expr::BlockExpr(block) => self.block(block, indent),
            ast::Expr::IfExpr(if_expression) => self.if_expression(if_expression, indent),
            _ => self.output.push_str("()"),
        }
    }

    fn call_expression(&mut self, call: ast::CallExpr, indent: usize) {
        if let Some(ast::Expr::PathExpr(path)) = call.expr() {
            self.path_name(path);
        } else {
            self.output.push_str("__missing_function");
        }
        self.output.push('(');
        if let Some(arguments) = call.arg_list() {
            for (index, argument) in arguments.args().enumerate() {
                if index != 0 {
                    self.output.push_str(", ");
                }
                self.expression(argument, indent);
            }
        }
        self.output.push(')');
    }

    fn if_expression(&mut self, expression: ast::IfExpr, indent: usize) {
        self.output.push_str("(if ");
        if let Some(condition) = expression.condition() {
            self.expression(condition, indent);
        } else {
            self.output.push_str("false");
        }
        self.output.push(' ');
        if let Some(then_branch) = expression.then_branch() {
            self.block(then_branch, indent);
        } else {
            self.output.push_str("{}");
        }
        self.output.push_str(" else ");
        match expression.else_branch() {
            Some(ast::ElseBranch::Block(block)) => self.block(block, indent),
            _ => self.output.push_str("{}"),
        }
        self.output.push(')');
    }

    fn ty(&mut self, ty: Option<&ast::Type>) {
        match ty {
            Some(ast::Type::PathType(path)) => {
                if let Some(name) = path.path().and_then(|path| {
                    if path.qualifier().is_some() || path.coloncolon_token().is_some() {
                        return None;
                    }
                    let segment = path.segment()?;
                    if segment.generic_arg_list().is_some()
                        || segment.parenthesized_arg_list().is_some()
                    {
                        return None;
                    }
                    segment.name_ref().map(|name| name.text().to_string())
                }) {
                    self.output.push_str(&name);
                } else {
                    self.output.push_str("__unsupported_type");
                }
            }
            Some(ast::Type::TupleType(tuple)) if tuple.fields().next().is_none() => {
                self.output.push_str("()");
            }
            _ => self.output.push_str("__unsupported_type"),
        }
    }

    fn pattern_name(&mut self, pattern: Option<&ast::Pat>) {
        if let Some(ast::Pat::IdentPat(pattern)) = pattern {
            if let Some(name) = pattern.name() {
                self.output.push_str(&name.text());
            } else {
                self.output.push_str("__missing_name");
            }
        } else {
            self.output.push_str("__missing_name");
        }
    }

    fn path_name(&mut self, path: ast::PathExpr) {
        let name = path.path().and_then(|path| {
            if path.qualifier().is_some() || path.coloncolon_token().is_some() {
                return None;
            }
            let segment = path.segment()?;
            if segment.generic_arg_list().is_some() || segment.parenthesized_arg_list().is_some() {
                return None;
            }
            segment.name_ref().map(|name| name.text().to_string())
        });
        self.output
            .push_str(name.as_deref().unwrap_or("__unsupported_path"));
    }

    fn indent(&mut self, depth: usize) {
        for _ in 0..depth {
            self.output.push_str("    ");
        }
    }
}

fn is_syntax_implicit_unit(expression: Option<&ast::Expr>) -> bool {
    matches!(
        expression,
        Some(ast::Expr::TupleExpr(tuple)) if tuple.fields().next().is_none()
    )
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
            Statement::Print { value, .. } => {
                self.output.push_str("println!(\"{}\", ");
                self.expression(value, indent);
                self.output.push_str(");");
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
                self.output.push_str(unary_text(*op));
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

fn unary_text(op: UnaryOp) -> &'static str {
    match op {
        UnaryOp::Neg => "-",
        UnaryOp::Not => "!",
        UnaryOp::Deref => "*",
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
    use crate::{ParseLimits, check_source, format};

    #[test]
    fn canonical_output_is_idempotent() {
        let source = "fn add(x:i64,y:i64)->i64{x+y} fn main(){let mut x=add(1_i64,2_i64);while x<4_i64{x=x+1_i64;} if x==4_i64 {()} else {()};}";
        let first = format(&check_source(source, ParseLimits::default()).unwrap());
        let second = format(&check_source(&first, ParseLimits::default()).unwrap());
        assert_eq!(first, second);
    }
}
