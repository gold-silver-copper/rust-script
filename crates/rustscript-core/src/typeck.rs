use std::collections::{HashMap, HashSet};

use ra_ap_syntax::ast::{
    ArithOp, BinaryOp, CmpOp, HasArgList, HasGenericArgs, HasLoopBody, HasModuleItem, HasName,
    LiteralKind, LogicOp, UnaryOp,
};
use ra_ap_syntax::{AstNode, AstToken, SmolStr, ast};

use crate::checked_ir::{
    Block, CheckedProgram, Expression, ExpressionKind, Function, FunctionId, LocalId, Parameter,
    Statement,
};
use crate::{Diagnostic, ParseLimits, ParsedProgram, Phase, Type, Value};

struct Signature {
    name: SmolStr,
    parameters: Vec<Parameter>,
    return_type: Type,
    explicit_return: bool,
    syntax: ast::Fn,
}

pub(crate) fn check(parsed: &ParsedProgram) -> Result<CheckedProgram, Diagnostic> {
    let signatures = collect_signatures(parsed)?;
    let function_names: HashMap<_, _> = signatures
        .iter()
        .enumerate()
        .map(|(id, signature)| (signature.name.clone(), id))
        .collect();
    let mut functions = Vec::with_capacity(signatures.len());
    let mut main = None;

    for (id, signature) in signatures.iter().enumerate() {
        if signature.name == "main" {
            main = Some(id);
        }
        for parameter in &signature.parameters {
            if function_names.contains_key(&parameter.name) {
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
        let mut checker =
            BodyChecker::new(signature, &signatures, &function_names, parsed.limits());
        let checked = checker.check_block(&body)?;
        if checked.ty != signature.return_type && !checked.is_never {
            return Err(type_error(
                "function body does not produce its declared return type",
                body.syntax(),
            ));
        }
        functions.push(Function {
            name: signature.name.clone(),
            parameters: signature.parameters.clone(),
            return_type: signature.return_type,
            explicit_return: signature.explicit_return,
            local_count: checker.next_local,
            body: checked.block,
        });
    }
    let main = main.ok_or_else(|| {
        Diagnostic::new(Phase::Type, "exactly one main function is required", None)
    })?;
    Ok(CheckedProgram { functions, main })
}

#[derive(Clone, Copy)]
struct Binding {
    id: LocalId,
    ty: Type,
    mutable: bool,
}

struct CheckedBlock {
    block: Block,
    ty: Type,
    is_never: bool,
    exits: bool,
}
struct CheckedExpr {
    expression: Expression,
    is_never: bool,
    exits: bool,
}

struct BodyChecker<'a> {
    signature: &'a Signature,
    signatures: &'a [Signature],
    function_names: &'a HashMap<SmolStr, FunctionId>,
    limits: ParseLimits,
    scopes: Vec<HashMap<SmolStr, Binding>>,
    next_local: usize,
    loop_depth: usize,
}

impl<'a> BodyChecker<'a> {
    fn new(
        signature: &'a Signature,
        signatures: &'a [Signature],
        function_names: &'a HashMap<SmolStr, FunctionId>,
        limits: ParseLimits,
    ) -> Self {
        let scope = signature
            .parameters
            .iter()
            .map(|parameter| {
                (
                    parameter.name.clone(),
                    Binding {
                        id: parameter.id,
                        ty: parameter.ty,
                        mutable: false,
                    },
                )
            })
            .collect();
        Self {
            signature,
            signatures,
            function_names,
            limits,
            scopes: vec![scope],
            next_local: signature.parameters.len(),
            loop_depth: 0,
        }
    }

    fn check_block(&mut self, block: &ast::BlockExpr) -> Result<CheckedBlock, Diagnostic> {
        self.scopes.push(HashMap::new());
        let mut statements = Vec::new();
        let mut block_exits_early = false;
        for statement in block.statements() {
            let (checked, statement_diverges) = self.check_statement(statement)?;
            statements.push(checked);
            block_exits_early |= statement_diverges;
        }
        let span = block.syntax().text_range();
        let tail_expression = block.tail_expr();
        let (tail, ty, is_never, exits) = match tail_expression {
            Some(ast::Expr::WhileExpr(while_expression)) => {
                let (statement, while_exits) = self.check_while(while_expression)?;
                statements.push(statement);
                let tail = Box::new(Expression {
                    kind: ExpressionKind::Value(Value::Unit),
                    ty: Type::Unit,
                    span,
                });
                let exits = block_exits_early || while_exits;
                (Some(tail), Type::Unit, false, exits)
            }
            Some(expression) => {
                let checked = self.check_expression(expression)?;
                let ty = checked.expression.ty;
                (
                    Some(Box::new(checked.expression)),
                    ty,
                    checked.is_never,
                    block_exits_early || checked.exits,
                )
            }
            None => {
                let tail = Box::new(Expression {
                    kind: ExpressionKind::Value(Value::Unit),
                    ty: Type::Unit,
                    span,
                });
                (Some(tail), Type::Unit, block_exits_early, block_exits_early)
            }
        };
        self.scopes.pop();
        Ok(CheckedBlock {
            block: Block {
                statements,
                tail,
                span,
            },
            ty,
            is_never,
            exits,
        })
    }

    fn check_statement(&mut self, statement: ast::Stmt) -> Result<(Statement, bool), Diagnostic> {
        match statement {
            ast::Stmt::LetStmt(let_statement) => self.check_let(let_statement),
            ast::Stmt::ExprStmt(expression_statement) => {
                let expression = expression_statement.expr().ok_or_else(|| {
                    type_error(
                        "statement expression is required",
                        expression_statement.syntax(),
                    )
                })?;
                match expression {
                    ast::Expr::BinExpr(binary)
                        if matches!(binary.op_kind(), Some(BinaryOp::Assignment { op: None })) =>
                    {
                        self.check_assignment(binary)
                    }
                    ast::Expr::WhileExpr(while_expression) => {
                        if expression_statement.semicolon_token().is_some() {
                            return Err(type_error(
                                "while statements do not take a semicolon",
                                while_expression.syntax(),
                            ));
                        }
                        self.check_while(while_expression)
                    }
                    ast::Expr::ReturnExpr(return_expression) => {
                        self.check_return(return_expression)
                    }
                    ast::Expr::BreakExpr(break_expression) => {
                        if self.loop_depth == 0 {
                            return Err(type_error(
                                "break is only valid inside a loop",
                                break_expression.syntax(),
                            ));
                        }
                        if break_expression.expr().is_some()
                            || break_expression.lifetime().is_some()
                        {
                            return Err(type_error(
                                "break values and labels are unsupported",
                                break_expression.syntax(),
                            ));
                        }
                        Ok((
                            Statement::Break(break_expression.syntax().text_range()),
                            true,
                        ))
                    }
                    ast::Expr::ContinueExpr(continue_expression) => {
                        if self.loop_depth == 0 {
                            return Err(type_error(
                                "continue is only valid inside a loop",
                                continue_expression.syntax(),
                            ));
                        }
                        if continue_expression.lifetime().is_some() {
                            return Err(type_error(
                                "loop labels are unsupported",
                                continue_expression.syntax(),
                            ));
                        }
                        Ok((
                            Statement::Continue(continue_expression.syntax().text_range()),
                            true,
                        ))
                    }
                    ast::Expr::MacroExpr(macro_expression) => {
                        if expression_statement.semicolon_token().is_none() {
                            return Err(type_error(
                                "println must be terminated by a semicolon",
                                macro_expression.syntax(),
                            ));
                        }
                        self.check_print(macro_expression)
                    }
                    expression => {
                        if expression_statement.semicolon_token().is_none() {
                            return Err(type_error(
                                "statement expression requires a semicolon",
                                expression.syntax(),
                            ));
                        }
                        let checked = self.check_expression(expression)?;
                        Ok((Statement::Expression(checked.expression), checked.exits))
                    }
                }
            }
            ast::Stmt::Item(item) => Err(type_error("nested items are unsupported", item.syntax())),
        }
    }

    fn check_let(&mut self, statement: ast::LetStmt) -> Result<(Statement, bool), Diagnostic> {
        if statement.let_else().is_some() {
            return Err(type_error("let-else is unsupported", statement.syntax()));
        }
        let ast::Pat::IdentPat(pattern) = statement
            .pat()
            .ok_or_else(|| type_error("let pattern is required", statement.syntax()))?
        else {
            return Err(type_error(
                "let requires a bare identifier",
                statement.syntax(),
            ));
        };
        if pattern.ref_token().is_some() || pattern.at_token().is_some() {
            return Err(type_error(
                "let requires a bare identifier",
                pattern.syntax(),
            ));
        }
        let name_node = pattern
            .name()
            .ok_or_else(|| type_error("binding name is required", pattern.syntax()))?;
        let name = SmolStr::new(name_node.text());
        validate_binding_name(&name, name_node.syntax())?;
        if self.function_names.contains_key(&name) {
            return Err(type_error(
                "local name collides with a function",
                name_node.syntax(),
            ));
        }
        let initializer = statement
            .initializer()
            .ok_or_else(|| type_error("let initializer is required", statement.syntax()))?;
        let checked = self.check_expression(initializer)?;
        let annotation = statement
            .ty()
            .as_ref()
            .map(|ty| parse_type(Some(ty), statement.syntax()))
            .transpose()?;
        if annotation.is_some_and(|ty| ty != checked.expression.ty) {
            return Err(type_error(
                "initializer type does not match annotation",
                statement.syntax(),
            ));
        }
        let id = self.next_local;
        self.next_local += 1;
        let binding = Binding {
            id,
            ty: checked.expression.ty,
            mutable: pattern.mut_token().is_some(),
        };
        let scope = self
            .scopes
            .last_mut()
            .ok_or_else(|| type_error("internal scope invariant failed", statement.syntax()))?;
        scope.insert(name.clone(), binding);
        Ok((
            Statement::Let {
                id,
                name,
                mutable: binding.mutable,
                annotation,
                initializer: checked.expression,
            },
            checked.exits,
        ))
    }

    fn check_assignment(&mut self, binary: ast::BinExpr) -> Result<(Statement, bool), Diagnostic> {
        let lhs = binary
            .lhs()
            .ok_or_else(|| type_error("assignment target is required", binary.syntax()))?;
        let ast::Expr::PathExpr(path) = lhs else {
            return Err(type_error(
                "assignment target must be a bare local",
                binary.syntax(),
            ));
        };
        let name = bare_path_name(&path)
            .ok_or_else(|| type_error("assignment target must be a bare local", path.syntax()))?;
        let binding = self
            .lookup(&name)
            .ok_or_else(|| type_error("unknown assignment target", path.syntax()))?;
        if !binding.mutable {
            return Err(type_error(
                "cannot assign to an immutable binding",
                path.syntax(),
            ));
        }
        let rhs = binary
            .rhs()
            .ok_or_else(|| type_error("assignment value is required", binary.syntax()))?;
        let checked = self.check_expression(rhs)?;
        if checked.expression.ty != binding.ty {
            return Err(type_error("assignment type mismatch", binary.syntax()));
        }
        Ok((
            Statement::Assign {
                id: binding.id,
                value: checked.expression,
                span: binary.syntax().text_range(),
            },
            checked.exits,
        ))
    }

    fn check_while(&mut self, expression: ast::WhileExpr) -> Result<(Statement, bool), Diagnostic> {
        let condition = self.check_expression(
            expression
                .condition()
                .ok_or_else(|| type_error("while condition is required", expression.syntax()))?,
        )?;
        if condition.expression.ty != Type::Bool {
            return Err(type_error(
                "while condition must be bool",
                expression.syntax(),
            ));
        }
        self.loop_depth += 1;
        let body_result = expression
            .loop_body()
            .ok_or_else(|| type_error("while body is required", expression.syntax()))
            .and_then(|body| self.check_block(&body));
        self.loop_depth -= 1;
        let body = body_result?;
        if body.ty != Type::Unit && !body.is_never {
            return Err(type_error(
                "while body must complete as unit",
                expression.syntax(),
            ));
        }
        Ok((
            Statement::While {
                condition: condition.expression,
                body: body.block,
                span: expression.syntax().text_range(),
            },
            false,
        ))
    }

    fn check_return(
        &mut self,
        expression: ast::ReturnExpr,
    ) -> Result<(Statement, bool), Diagnostic> {
        let span = expression.syntax().text_range();
        let value = match expression.expr() {
            Some(value) => self.check_expression(value)?.expression,
            None => Expression {
                kind: ExpressionKind::Value(Value::Unit),
                ty: Type::Unit,
                span,
            },
        };
        if value.ty != self.signature.return_type {
            return Err(type_error("return type mismatch", expression.syntax()));
        }
        Ok((Statement::Return { value, span }, true))
    }

    fn check_print(&mut self, expression: ast::MacroExpr) -> Result<(Statement, bool), Diagnostic> {
        let macro_range = expression.syntax().text_range();
        let input = crate::frontend::intrinsic::print_expression(&expression, self.limits)
            .map_err(|mut error| {
                if error.span.is_none() {
                    error.span = Some(crate::diagnostic::span(macro_range));
                }
                error
            })?;
        let source_range = input.source_range;
        let range_map = input.range_map;
        let mut checked = self
            .check_expression(input.expression)
            .map_err(|mut error| {
                error.span = error
                    .span
                    .and_then(|span| range_map.span(span))
                    .or_else(|| Some(crate::diagnostic::span(source_range)));
                error
            })?;
        if !matches!(checked.expression.ty, Type::I64 | Type::Bool) {
            return Err(Diagnostic::new(
                Phase::Type,
                "println value must be i64 or bool",
                Some(crate::diagnostic::span(source_range)),
            ));
        }
        map_expression_ranges(&mut checked.expression, range_map, source_range);
        Ok((
            Statement::Print {
                value: checked.expression,
                span: macro_range,
            },
            checked.exits,
        ))
    }

    fn check_expression(&mut self, expression: ast::Expr) -> Result<CheckedExpr, Diagnostic> {
        let span = expression.syntax().text_range();
        let (kind, ty, is_never, exits) = match expression {
            ast::Expr::Literal(literal) => match literal.kind() {
                LiteralKind::Bool(value) => (
                    ExpressionKind::Value(Value::Bool(value)),
                    Type::Bool,
                    false,
                    false,
                ),
                LiteralKind::IntNumber(number) => {
                    let text = number.syntax().text().to_string();
                    let digits = text
                        .strip_suffix("_i64")
                        .ok_or_else(|| type_error("invalid i64 literal", literal.syntax()))?;
                    let value = digits
                        .parse::<i64>()
                        .map_err(|_| type_error("i64 literal is out of range", literal.syntax()))?;
                    (
                        ExpressionKind::Value(Value::I64(value)),
                        Type::I64,
                        false,
                        false,
                    )
                }
                _ => return Err(type_error("unsupported literal", literal.syntax())),
            },
            ast::Expr::TupleExpr(tuple) if tuple.fields().next().is_none() => {
                (ExpressionKind::Value(Value::Unit), Type::Unit, false, false)
            }
            ast::Expr::ParenExpr(paren) => {
                return self.check_expression(paren.expr().ok_or_else(|| {
                    type_error("parenthesized expression is required", paren.syntax())
                })?);
            }
            ast::Expr::PathExpr(path) => {
                let name = bare_path_name(&path).ok_or_else(|| {
                    type_error("only bare local paths are supported", path.syntax())
                })?;
                let binding = self
                    .lookup(&name)
                    .ok_or_else(|| type_error("unknown local name", path.syntax()))?;
                (ExpressionKind::Local(binding.id), binding.ty, false, false)
            }
            ast::Expr::PrefixExpr(prefix) => {
                let op = prefix
                    .op_kind()
                    .ok_or_else(|| type_error("unsupported unary operator", prefix.syntax()))?;
                let operand =
                    self.check_expression(prefix.expr().ok_or_else(|| {
                        type_error("unary operand is required", prefix.syntax())
                    })?)?;
                let ty = match (op, operand.expression.ty) {
                    (UnaryOp::Neg, Type::I64) => Type::I64,
                    (UnaryOp::Not, Type::Bool) => Type::Bool,
                    _ => return Err(type_error("invalid unary operand type", prefix.syntax())),
                };
                (
                    ExpressionKind::Unary {
                        op,
                        operand: Box::new(operand.expression),
                    },
                    ty,
                    false,
                    operand.exits,
                )
            }
            ast::Expr::BinExpr(binary) => return self.check_binary(binary),
            ast::Expr::CallExpr(call) => return self.check_call(call),
            ast::Expr::BlockExpr(block) => {
                let checked = self.check_block(&block)?;
                (
                    ExpressionKind::Block(checked.block),
                    checked.ty,
                    checked.is_never,
                    checked.exits,
                )
            }
            ast::Expr::IfExpr(if_expression) => return self.check_if(if_expression),
            _ => {
                return Err(type_error(
                    "unsupported expression in this position",
                    expression.syntax(),
                ));
            }
        };
        Ok(CheckedExpr {
            expression: Expression { kind, ty, span },
            is_never,
            exits,
        })
    }

    fn check_binary(&mut self, binary: ast::BinExpr) -> Result<CheckedExpr, Diagnostic> {
        let op = binary
            .op_kind()
            .ok_or_else(|| type_error("binary operator is required", binary.syntax()))?;
        if matches!(op, BinaryOp::Assignment { .. }) {
            return Err(type_error(
                "assignment is only a statement",
                binary.syntax(),
            ));
        }
        if matches!(op, BinaryOp::CmpOp(_)) && has_comparison_operand(&binary) {
            return Err(type_error(
                "comparison chains are unsupported",
                binary.syntax(),
            ));
        }
        let lhs = self.check_expression(
            binary
                .lhs()
                .ok_or_else(|| type_error("left operand is required", binary.syntax()))?,
        )?;
        let rhs = self.check_expression(
            binary
                .rhs()
                .ok_or_else(|| type_error("right operand is required", binary.syntax()))?,
        )?;
        let ty = match op {
            BinaryOp::ArithOp(
                ArithOp::Add | ArithOp::Sub | ArithOp::Mul | ArithOp::Div | ArithOp::Rem,
            ) if lhs.expression.ty == Type::I64 && rhs.expression.ty == Type::I64 => Type::I64,
            BinaryOp::LogicOp(LogicOp::And | LogicOp::Or)
                if lhs.expression.ty == Type::Bool && rhs.expression.ty == Type::Bool =>
            {
                Type::Bool
            }
            BinaryOp::CmpOp(CmpOp::Eq { .. })
                if lhs.expression.ty == rhs.expression.ty && lhs.expression.ty != Type::Unit =>
            {
                Type::Bool
            }
            BinaryOp::CmpOp(CmpOp::Ord { .. })
                if lhs.expression.ty == Type::I64 && rhs.expression.ty == Type::I64 =>
            {
                Type::Bool
            }
            _ => return Err(type_error("invalid binary operand types", binary.syntax())),
        };
        let exits = if matches!(op, BinaryOp::LogicOp(_)) {
            lhs.exits
        } else {
            lhs.exits || rhs.exits
        };
        Ok(CheckedExpr {
            expression: Expression {
                kind: ExpressionKind::Binary {
                    op,
                    lhs: Box::new(lhs.expression),
                    rhs: Box::new(rhs.expression),
                },
                ty,
                span: binary.syntax().text_range(),
            },
            is_never: false,
            exits,
        })
    }

    fn check_call(&mut self, call: ast::CallExpr) -> Result<CheckedExpr, Diagnostic> {
        let target = call
            .expr()
            .ok_or_else(|| type_error("call target is required", call.syntax()))?;
        let ast::Expr::PathExpr(path) = target else {
            return Err(type_error(
                "call target must be a bare function",
                call.syntax(),
            ));
        };
        let name = bare_path_name(&path)
            .ok_or_else(|| type_error("call target must be a bare function", path.syntax()))?;
        if name == "main" {
            return Err(type_error("calling main is forbidden", path.syntax()));
        }
        let id = *self
            .function_names
            .get(&name)
            .ok_or_else(|| type_error("unknown function", path.syntax()))?;
        let signature = &self.signatures[id];
        let arguments: Vec<_> = call
            .arg_list()
            .map(|list| list.args().collect())
            .unwrap_or_default();
        if arguments.len() != signature.parameters.len() {
            return Err(type_error("wrong function argument count", call.syntax()));
        }
        let mut checked_arguments = Vec::with_capacity(arguments.len());
        let mut exits = false;
        for (argument, parameter) in arguments.into_iter().zip(&signature.parameters) {
            let checked = self.check_expression(argument)?;
            if checked.expression.ty != parameter.ty {
                return Err(type_error("function argument type mismatch", call.syntax()));
            }
            exits |= checked.exits;
            checked_arguments.push(checked.expression);
        }
        Ok(CheckedExpr {
            expression: Expression {
                kind: ExpressionKind::Call {
                    function: id,
                    arguments: checked_arguments,
                },
                ty: signature.return_type,
                span: call.syntax().text_range(),
            },
            is_never: false,
            exits,
        })
    }

    fn check_if(&mut self, expression: ast::IfExpr) -> Result<CheckedExpr, Diagnostic> {
        let condition = self.check_expression(
            expression
                .condition()
                .ok_or_else(|| type_error("if condition is required", expression.syntax()))?,
        )?;
        if condition.expression.ty != Type::Bool {
            return Err(type_error("if condition must be bool", expression.syntax()));
        }
        let then_branch = self.check_block(
            &expression
                .then_branch()
                .ok_or_else(|| type_error("if branch is required", expression.syntax()))?,
        )?;
        let else_branch = match expression.else_branch() {
            Some(ast::ElseBranch::Block(block)) => self.check_block(&block)?,
            _ => {
                return Err(type_error(
                    "if requires an explicit else block",
                    expression.syntax(),
                ));
            }
        };
        let ty = if then_branch.is_never {
            else_branch.ty
        } else if else_branch.is_never || then_branch.ty == else_branch.ty {
            then_branch.ty
        } else {
            return Err(type_error("if branch type mismatch", expression.syntax()));
        };
        let is_never = then_branch.is_never && else_branch.is_never;
        let exits = condition.exits || (then_branch.exits && else_branch.exits);
        Ok(CheckedExpr {
            expression: Expression {
                kind: ExpressionKind::If {
                    condition: Box::new(condition.expression),
                    then_branch: then_branch.block,
                    else_branch: else_branch.block,
                },
                ty,
                span: expression.syntax().text_range(),
            },
            is_never,
            exits,
        })
    }

    fn lookup(&self, name: &str) -> Option<Binding> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
    }
}

fn map_expression_ranges(
    expression: &mut Expression,
    range_map: crate::frontend::intrinsic::ExpressionRangeMap,
    fallback: ra_ap_syntax::TextRange,
) {
    expression.span = range_map.text_range(expression.span).unwrap_or(fallback);
    match &mut expression.kind {
        ExpressionKind::Value(_) | ExpressionKind::Local(_) => {}
        ExpressionKind::Call { arguments, .. } => {
            for argument in arguments {
                map_expression_ranges(argument, range_map, fallback);
            }
        }
        ExpressionKind::Unary { operand, .. } => {
            map_expression_ranges(operand, range_map, fallback);
        }
        ExpressionKind::Binary { lhs, rhs, .. } => {
            map_expression_ranges(lhs, range_map, fallback);
            map_expression_ranges(rhs, range_map, fallback);
        }
        ExpressionKind::Block(block) => map_block_ranges(block, range_map, fallback),
        ExpressionKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            map_expression_ranges(condition, range_map, fallback);
            map_block_ranges(then_branch, range_map, fallback);
            map_block_ranges(else_branch, range_map, fallback);
        }
    }
}

fn map_block_ranges(
    block: &mut Block,
    range_map: crate::frontend::intrinsic::ExpressionRangeMap,
    fallback: ra_ap_syntax::TextRange,
) {
    block.span = range_map.text_range(block.span).unwrap_or(fallback);
    for statement in &mut block.statements {
        match statement {
            Statement::Let { initializer, .. } => {
                map_expression_ranges(initializer, range_map, fallback);
            }
            Statement::Assign { value, span, .. } => {
                *span = range_map.text_range(*span).unwrap_or(fallback);
                map_expression_ranges(value, range_map, fallback);
            }
            Statement::While {
                condition,
                body,
                span,
            } => {
                *span = range_map.text_range(*span).unwrap_or(fallback);
                map_expression_ranges(condition, range_map, fallback);
                map_block_ranges(body, range_map, fallback);
            }
            Statement::Return { value, span } | Statement::Print { value, span } => {
                *span = range_map.text_range(*span).unwrap_or(fallback);
                map_expression_ranges(value, range_map, fallback);
            }
            Statement::Break(span) | Statement::Continue(span) => {
                *span = range_map.text_range(*span).unwrap_or(fallback);
            }
            Statement::Expression(expression) => {
                map_expression_ranges(expression, range_map, fallback);
            }
        }
    }
    if let Some(tail) = &mut block.tail {
        map_expression_ranges(tail, range_map, fallback);
    }
}

fn has_comparison_operand(binary: &ast::BinExpr) -> bool {
    [binary.lhs(), binary.rhs()]
        .into_iter()
        .flatten()
        .any(|expr| {
            matches!(
                expr,
                ast::Expr::BinExpr(ref inner)
                    if matches!(inner.op_kind(), Some(BinaryOp::CmpOp(_)))
            )
        })
}

fn bare_path_name(path: &ast::PathExpr) -> Option<SmolStr> {
    let path = path.path()?;
    if path.qualifier().is_some() || path.coloncolon_token().is_some() {
        return None;
    }
    let segment = path.segment()?;
    if segment.generic_arg_list().is_some() || segment.parenthesized_arg_list().is_some() {
        return None;
    }
    Some(SmolStr::new(segment.name_ref()?.text()))
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
        ast::Type::PathType(path_type) => {
            let name = path_type.path().and_then(|path| {
                if path.qualifier().is_some() || path.coloncolon_token().is_some() {
                    return None;
                }
                let segment = path.segment()?;
                if segment.generic_arg_list().is_some()
                    || segment.parenthesized_arg_list().is_some()
                {
                    return None;
                }
                Some(SmolStr::new(segment.name_ref()?.text()))
            });
            match name.as_deref() {
                Some("i64") => Ok(Type::I64),
                Some("bool") => Ok(Type::Bool),
                _ => Err(type_error(
                    "only i64, bool, and () types are supported",
                    ty.syntax(),
                )),
            }
        }
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
    const RESERVED: &[&str] = &[
        "_",
        "as",
        "async",
        "await",
        "break",
        "const",
        "continue",
        "crate",
        "dyn",
        "else",
        "enum",
        "extern",
        "false",
        "fn",
        "for",
        "if",
        "impl",
        "in",
        "let",
        "loop",
        "match",
        "mod",
        "move",
        "mut",
        "pub",
        "ref",
        "return",
        "self",
        "Self",
        "static",
        "struct",
        "super",
        "trait",
        "true",
        "type",
        "unsafe",
        "use",
        "where",
        "while",
        "abstract",
        "become",
        "box",
        "do",
        "final",
        "gen",
        "macro",
        "override",
        "priv",
        "try",
        "typeof",
        "unsized",
        "virtual",
        "yield",
        "macro_rules",
        "raw",
        "safe",
        "union",
        "i64",
        "bool",
        "println",
        "main",
    ];
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
    use crate::{ParseLimits, check_source};

    #[test]
    fn checks_empty_main() {
        assert!(check_source("fn main() {}", ParseLimits::default()).is_ok());
    }

    #[test]
    fn collects_forward_and_recursive_signatures() {
        assert!(check_source("fn a(x: i64) -> i64 {} fn main() {}", ParseLimits::default()).is_err());
        assert!(check_source("fn a(x: i64) {} fn main() {}", ParseLimits::default()).is_ok());
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
            assert!(check_source(source, ParseLimits::default()).is_err(), "{source}");
        }
    }

    #[test]
    fn checks_scopes_calls_operators_and_control_flow() {
        let source = r#"
fn add(x: i64, y: i64) -> i64 { x + y }
fn positive(x: i64) -> bool { x > 0_i64 }
fn main() {
    let x = 1_i64;
    let x = add(x, 2_i64);
    let mut keep: bool = positive(x);
    while keep {
        keep = false;
        if x == 3_i64 { () } else { () };
    }
}
"#;
        check_source(source, ParseLimits::default()).unwrap();
    }

    #[test]
    fn rejects_body_type_and_control_errors() {
        for source in [
            "fn main() { let x: bool = 1_i64; }",
            "fn main() { let x = y; }",
            "fn main() { let x = 1_i64; x = 2_i64; }",
            "fn main() { while 1_i64 {} }",
            "fn main() { break; }",
            "fn f(x: bool) {} fn main() { f(1_i64); }",
            "fn main() { if true { 1_i64 } else { false }; }",
            "fn main() { let x = 1_i64 < 2_i64 < 3_i64; }",
        ] {
            assert!(check_source(source, ParseLimits::default()).is_err(), "{source}");
        }
    }
}
