use ra_ap_syntax::ast::{ArithOp, BinaryOp, CmpOp, LogicOp, Ordering, UnaryOp};
use ra_ap_syntax::{SmolStr, TextRange, TextSize};

use crate::checked_ir::{
    Block, CheckedProgram, Expression, ExpressionKind, Function, Parameter, Statement,
};
use crate::{Type, Value};

const GENERATED_RANGE: TextRange = TextRange::empty(TextSize::new(0));
const MAX_EXPRESSION_DEPTH: usize = 5;

/// Build a bounded, typed, terminating checked program from a decision stream.
///
/// This is deliberately a syntax-neutral checked-IR generator rather than a
/// source-string or parsed-AST generator. The canonical emitter is the only
/// path from generated semantics to Rust text.
pub fn generate_checked_program(decisions: &[u64]) -> CheckedProgram {
    Generator::new(decisions).program()
}

/// Produce small, type-preserving checked-IR reductions.
///
/// Candidates remain trusted checked programs and are materialized only by the
/// shared canonical emitter. The differential harness rechecks every emitted
/// candidate and retains it only when the original failure category survives.
pub fn reduction_candidates(program: &CheckedProgram) -> Vec<CheckedProgram> {
    let mut candidates = Vec::new();
    for function_index in uncalled_function_indices(program) {
        let mut candidate = program.clone();
        remove_function(&mut candidate, function_index);
        candidates.push(candidate);
    }
    for (function_index, function) in program.functions.iter().enumerate() {
        if let Some(tail) = &function.body.tail {
            for replacement in expression_replacements(tail) {
                let mut candidate = program.clone();
                candidate.functions[function_index].body.tail = Some(Box::new(replacement));
                candidates.push(candidate);
            }
        }
        for statement_index in 0..function.body.statements.len() {
            match &function.body.statements[statement_index] {
                Statement::Let {
                    id, initializer, ..
                } => {
                    for replacement in expression_replacements(initializer) {
                        let mut candidate = program.clone();
                        if let Statement::Let { initializer, .. } =
                            &mut candidate.functions[function_index].body.statements
                                [statement_index]
                        {
                            *initializer = replacement;
                        }
                        candidates.push(candidate);
                    }
                    let mut without = function.body.clone();
                    without.statements.remove(statement_index);
                    if !block_uses_local(&without, *id) && !block_assigns_local(&without, *id) {
                        let mut candidate = program.clone();
                        candidate.functions[function_index]
                            .body
                            .statements
                            .remove(statement_index);
                        candidates.push(candidate);
                    }
                    if matches!(initializer.kind, ExpressionKind::Value(_))
                        && !block_assigns_local(&without, *id)
                    {
                        let mut candidate = program.clone();
                        candidate.functions[function_index]
                            .body
                            .statements
                            .remove(statement_index);
                        replace_local_in_block(
                            &mut candidate.functions[function_index].body,
                            *id,
                            initializer,
                        );
                        candidates.push(candidate);
                    }
                }
                Statement::Assign { value, .. } => {
                    for replacement in expression_replacements(value) {
                        let mut candidate = program.clone();
                        if let Statement::Assign { value, .. } =
                            &mut candidate.functions[function_index].body.statements
                                [statement_index]
                        {
                            *value = replacement;
                        }
                        candidates.push(candidate);
                    }
                }
                Statement::Print { value, .. } => {
                    for replacement in expression_replacements(value) {
                        let mut simplified = program.clone();
                        if let Statement::Print { value, .. } =
                            &mut simplified.functions[function_index].body.statements
                                [statement_index]
                        {
                            *value = replacement;
                        }
                        candidates.push(simplified);
                    }

                    let mut removed = program.clone();
                    removed.functions[function_index]
                        .body
                        .statements
                        .remove(statement_index);
                    candidates.push(removed);
                }
                Statement::Return { value, .. } => {
                    for replacement in expression_replacements(value) {
                        let mut candidate = program.clone();
                        if let Statement::Return { value, .. } =
                            &mut candidate.functions[function_index].body.statements
                                [statement_index]
                        {
                            *value = replacement;
                        }
                        candidates.push(candidate);
                    }
                }
                Statement::Expression(expression) => {
                    for replacement in expression_replacements(expression) {
                        let mut candidate = program.clone();
                        candidate.functions[function_index].body.statements[statement_index] =
                            Statement::Expression(replacement);
                        candidates.push(candidate);
                    }
                    if expression.ty == Type::Unit {
                        let mut candidate = program.clone();
                        candidate.functions[function_index]
                            .body
                            .statements
                            .remove(statement_index);
                        candidates.push(candidate);
                    }
                }
                Statement::While {
                    condition, body, ..
                } => {
                    for replacement in expression_replacements(condition) {
                        let mut candidate = program.clone();
                        if let Statement::While { condition, .. } =
                            &mut candidate.functions[function_index].body.statements
                                [statement_index]
                        {
                            *condition = replacement;
                        }
                        candidates.push(candidate);
                    }
                    let mut candidate = program.clone();
                    if let Statement::While { condition, .. } =
                        &mut candidate.functions[function_index].body.statements[statement_index]
                    {
                        *condition = boolean(false);
                    }
                    candidates.push(candidate);
                    for nested_index in 0..body.statements.len() {
                        if matches!(
                            body.statements[nested_index],
                            Statement::Print { .. } | Statement::Expression(_)
                        ) {
                            let mut candidate = program.clone();
                            if let Statement::While { body, .. } =
                                &mut candidate.functions[function_index].body.statements
                                    [statement_index]
                            {
                                body.statements.remove(nested_index);
                            }
                            candidates.push(candidate);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    candidates
}

struct Generator<'a> {
    decisions: &'a [u64],
    cursor: usize,
}

#[derive(Clone, Copy)]
struct GeneratedBinding {
    id: usize,
    ty: Type,
    mutable: bool,
}

#[derive(Clone, Copy)]
struct GenerationContext<'a> {
    locals: &'a [GeneratedBinding],
    functions: &'a [Function],
    expected_type: Type,
    expression_depth: usize,
    statement_budget: usize,
    loop_depth: usize,
}

impl<'a> Generator<'a> {
    fn new(decisions: &'a [u64]) -> Self {
        Self {
            decisions,
            cursor: 0,
        }
    }

    fn program(mut self) -> CheckedProgram {
        let helper_count = self.choose(5);
        let mut functions = Vec::with_capacity(helper_count + 1);
        for id in 0..helper_count {
            functions.push(self.helper(id, &functions));
        }
        let main = functions.len();
        functions.push(self.main(&functions));
        CheckedProgram { functions, main }
    }

    fn helper(&mut self, id: usize, earlier: &[Function]) -> Function {
        let parameter_count = self.choose(4);
        let parameters: Vec<_> = (0..parameter_count)
            .map(|parameter| Parameter {
                id: parameter,
                name: SmolStr::new(format!("x{parameter}")),
                ty: if self.choose(2) == 0 {
                    Type::I64
                } else {
                    Type::Bool
                },
            })
            .collect();
        let return_type = match self.choose(3) {
            0 => Type::I64,
            1 => Type::Bool,
            _ => Type::Unit,
        };
        let locals: Vec<_> = parameters
            .iter()
            .map(|parameter| GeneratedBinding {
                id: parameter.id,
                ty: parameter.ty,
                mutable: false,
            })
            .collect();
        let mut locals = locals;
        let mut next_local = parameter_count;
        let statement_budget = self.choose(9);
        let statements = self.statements(
            &mut locals,
            earlier,
            &mut next_local,
            statement_budget,
            0,
            false,
        );
        let body = Block {
            statements,
            tail: Some(Box::new(self.expression(GenerationContext {
                locals: &locals,
                functions: earlier,
                expected_type: return_type,
                expression_depth: 0,
                statement_budget,
                loop_depth: 0,
            }))),
            span: GENERATED_RANGE,
        };
        Function {
            name: SmolStr::new(format!("f{id}")),
            parameters,
            return_type,
            explicit_return: true,
            local_count: next_local,
            body,
        }
    }

    fn main(&mut self, helpers: &[Function]) -> Function {
        let initial = self.small_i64();
        let iterations = self.choose(9) as i64;
        let mut statements = vec![Statement::Let {
            id: 0,
            name: SmolStr::new("x0"),
            mutable: true,
            annotation: Some(Type::I64),
            initializer: integer(initial),
        }];
        statements.push(Statement::Let {
            id: 1,
            name: SmolStr::new("x1"),
            mutable: true,
            annotation: Some(Type::Bool),
            initializer: boolean(self.choose(2) == 0),
        });

        let mut loop_statements = Vec::new();
        if self.choose(2) == 0 {
            loop_statements.push(Statement::Print {
                value: local(0, Type::I64),
                span: GENERATED_RANGE,
            });
        }
        loop_statements.push(Statement::Assign {
            id: 1,
            value: unary(UnaryOp::Not, local(1, Type::Bool), Type::Bool),
            span: GENERATED_RANGE,
        });
        loop_statements.push(Statement::Assign {
            id: 0,
            value: binary(
                BinaryOp::ArithOp(ArithOp::Add),
                local(0, Type::I64),
                integer(1),
                Type::I64,
            ),
            span: GENERATED_RANGE,
        });
        statements.push(Statement::While {
            condition: binary(
                BinaryOp::CmpOp(CmpOp::Ord {
                    ordering: Ordering::Less,
                    strict: true,
                }),
                local(0, Type::I64),
                integer(initial + iterations),
                Type::Bool,
            ),
            body: Block {
                statements: loop_statements,
                tail: Some(Box::new(unit())),
                span: GENERATED_RANGE,
            },
            span: GENERATED_RANGE,
        });

        let locals = [
            GeneratedBinding {
                id: 0,
                ty: Type::I64,
                mutable: true,
            },
            GeneratedBinding {
                id: 1,
                ty: Type::Bool,
                mutable: true,
            },
        ];
        statements.push(Statement::Print {
            value: self.expression(GenerationContext {
                locals: &locals,
                functions: helpers,
                expected_type: Type::I64,
                expression_depth: 0,
                statement_budget: 4,
                loop_depth: 0,
            }),
            span: GENERATED_RANGE,
        });
        statements.push(Statement::Print {
            value: self.expression(GenerationContext {
                locals: &locals,
                functions: helpers,
                expected_type: Type::Bool,
                expression_depth: 0,
                statement_budget: 4,
                loop_depth: 0,
            }),
            span: GENERATED_RANGE,
        });
        if let Some((id, _)) = helpers
            .iter()
            .enumerate()
            .find(|(_, function)| function.return_type == Type::Unit)
        {
            statements.push(Statement::Expression(self.call(id, &helpers[id])));
        }

        Function {
            name: SmolStr::new("main"),
            parameters: Vec::new(),
            return_type: Type::Unit,
            explicit_return: false,
            local_count: locals.len(),
            body: Block {
                statements,
                tail: Some(Box::new(unit())),
                span: GENERATED_RANGE,
            },
        }
    }

    fn statements(
        &mut self,
        locals: &mut Vec<GeneratedBinding>,
        functions: &[Function],
        next_local: &mut usize,
        statement_budget: usize,
        loop_depth: usize,
        allow_print: bool,
    ) -> Vec<Statement> {
        if statement_budget == 0 {
            return Vec::new();
        }
        let mut remaining_slots = self.choose(statement_budget + 1);
        let mut statements = Vec::new();
        while remaining_slots > 0 {
            let context = GenerationContext {
                locals,
                functions,
                expected_type: Type::Unit,
                expression_depth: 0,
                statement_budget: remaining_slots.saturating_sub(1),
                loop_depth,
            };
            let choices = if loop_depth < 2 && remaining_slots >= 2 {
                5
            } else {
                4
            };
            match self.choose(choices) {
                0 => {
                    let ty = if self.choose(2) == 0 {
                        Type::I64
                    } else {
                        Type::Bool
                    };
                    let id = *next_local;
                    *next_local = (*next_local).saturating_add(1);
                    let mutable = self.choose(2) == 0;
                    let initializer = self.expression(context.deeper(ty));
                    locals.push(GeneratedBinding { id, ty, mutable });
                    statements.push(Statement::Let {
                        id,
                        name: SmolStr::new(format!("x{id}")),
                        mutable,
                        annotation: (self.choose(2) == 0).then_some(ty),
                        initializer,
                    });
                }
                1 => {
                    if let Some(binding) = self.matching_mutable_local(locals) {
                        let value = self.expression(context.deeper(binding.ty));
                        statements.push(Statement::Assign {
                            id: binding.id,
                            value,
                            span: GENERATED_RANGE,
                        });
                    }
                }
                2 if allow_print => {
                    let ty = if self.choose(2) == 0 {
                        Type::I64
                    } else {
                        Type::Bool
                    };
                    statements.push(Statement::Print {
                        value: self.expression(context.deeper(ty)),
                        span: GENERATED_RANGE,
                    });
                }
                2 | 3 => statements.push(Statement::Expression(self.expression(context))),
                _ => {
                    let counter = *next_local;
                    *next_local = (*next_local).saturating_add(1);
                    locals.push(GeneratedBinding {
                        id: counter,
                        ty: Type::I64,
                        mutable: true,
                    });
                    let counter_binding = locals.len() - 1;
                    statements.push(Statement::Let {
                        id: counter,
                        name: SmolStr::new(format!("x{counter}")),
                        mutable: true,
                        annotation: Some(Type::I64),
                        initializer: integer(0),
                    });
                    let body_scope_len = locals.len();
                    locals[counter_binding].mutable = false;
                    let mut body = self.statements(
                        locals,
                        functions,
                        next_local,
                        remaining_slots.saturating_sub(2).min(2),
                        loop_depth + 1,
                        allow_print,
                    );
                    locals.truncate(body_scope_len);
                    locals[counter_binding].mutable = true;
                    body.push(Statement::Assign {
                        id: counter,
                        value: binary(
                            BinaryOp::ArithOp(ArithOp::Add),
                            local(counter, Type::I64),
                            integer(1),
                            Type::I64,
                        ),
                        span: GENERATED_RANGE,
                    });
                    statements.push(Statement::While {
                        condition: binary(
                            BinaryOp::CmpOp(CmpOp::Ord {
                                ordering: Ordering::Less,
                                strict: true,
                            }),
                            local(counter, Type::I64),
                            integer(self.choose(9) as i64),
                            Type::Bool,
                        ),
                        body: Block {
                            statements: body,
                            tail: Some(Box::new(unit())),
                            span: GENERATED_RANGE,
                        },
                        span: GENERATED_RANGE,
                    });
                    remaining_slots = remaining_slots.saturating_sub(2);
                    continue;
                }
            }
            remaining_slots = remaining_slots.saturating_sub(1);
        }
        debug_assert!(statements.len() <= statement_budget);
        statements
    }

    fn expression(&mut self, context: GenerationContext<'_>) -> Expression {
        let depth_limited = context.expression_depth >= MAX_EXPRESSION_DEPTH;
        match context.expected_type {
            Type::I64 => self.i64_expression(context, depth_limited),
            Type::Bool => self.bool_expression(context, depth_limited),
            Type::Unit => self.unit_expression(context, depth_limited),
        }
    }

    fn i64_expression(
        &mut self,
        context: GenerationContext<'_>,
        depth_limited: bool,
    ) -> Expression {
        let choices = if depth_limited { 3 } else { 7 };
        match self.choose(choices) {
            0 => integer(self.small_i64()),
            1 => self.matching_local(context.locals, Type::I64).map_or_else(
                || integer(self.small_i64()),
                |binding| local(binding.id, binding.ty),
            ),
            2 => match self.matching_function(context.functions, Type::I64) {
                Some((id, function)) => self.call(id, function),
                None => integer(self.small_i64()),
            },
            3 => {
                let next = context.deeper(Type::I64);
                let left = self.expression(next);
                let right = integer((self.choose(9) + 1) as i64);
                let op = match self.choose(5) {
                    0 => ArithOp::Add,
                    1 => ArithOp::Sub,
                    2 => ArithOp::Mul,
                    3 => ArithOp::Div,
                    _ => ArithOp::Rem,
                };
                binary(BinaryOp::ArithOp(op), left, right, Type::I64)
            }
            4 => unary(
                UnaryOp::Neg,
                integer((self.choose(20) + 1) as i64),
                Type::I64,
            ),
            5 => self.if_expression(context, Type::I64),
            _ => block_expression(self.expression(context.deeper(Type::I64)), Type::I64),
        }
    }

    fn bool_expression(
        &mut self,
        context: GenerationContext<'_>,
        depth_limited: bool,
    ) -> Expression {
        let choices = if depth_limited { 3 } else { 7 };
        match self.choose(choices) {
            0 => boolean(self.choose(2) == 0),
            1 => self.matching_local(context.locals, Type::Bool).map_or_else(
                || boolean(self.choose(2) == 0),
                |binding| local(binding.id, binding.ty),
            ),
            2 => match self.matching_function(context.functions, Type::Bool) {
                Some((id, function)) => self.call(id, function),
                None => boolean(self.choose(2) == 0),
            },
            3 => {
                let lhs = integer(self.small_i64());
                let rhs = integer(self.small_i64());
                let op = if self.choose(2) == 0 {
                    CmpOp::Ord {
                        ordering: Ordering::Less,
                        strict: self.choose(2) == 0,
                    }
                } else {
                    CmpOp::Eq {
                        negated: self.choose(2) == 0,
                    }
                };
                binary(BinaryOp::CmpOp(op), lhs, rhs, Type::Bool)
            }
            4 => {
                let next = context.deeper(Type::Bool);
                let left = self.expression(next);
                let right = self.expression(next);
                let op = if self.choose(2) == 0 {
                    LogicOp::And
                } else {
                    LogicOp::Or
                };
                binary(BinaryOp::LogicOp(op), left, right, Type::Bool)
            }
            5 => unary(
                UnaryOp::Not,
                self.expression(context.deeper(Type::Bool)),
                Type::Bool,
            ),
            _ => self.if_expression(context, Type::Bool),
        }
    }

    fn unit_expression(
        &mut self,
        context: GenerationContext<'_>,
        depth_limited: bool,
    ) -> Expression {
        if !depth_limited
            && self.choose(3) == 0
            && let Some((id, function)) = self.matching_function(context.functions, Type::Unit)
        {
            return self.call(id, function);
        }
        if !depth_limited && self.choose(3) == 0 {
            return block_expression(self.expression(context.deeper(Type::Unit)), Type::Unit);
        }
        unit()
    }

    fn if_expression(&mut self, context: GenerationContext<'_>, ty: Type) -> Expression {
        let condition = self.expression(context.deeper(Type::Bool));
        let then_value = self.expression(context.deeper(ty));
        let else_value = self.expression(context.deeper(ty));
        Expression {
            kind: ExpressionKind::If {
                condition: Box::new(condition),
                then_branch: tail_block(then_value),
                else_branch: tail_block(else_value),
            },
            ty,
            span: GENERATED_RANGE,
        }
    }

    fn matching_local<'b>(
        &mut self,
        locals: &'b [GeneratedBinding],
        ty: Type,
    ) -> Option<&'b GeneratedBinding> {
        let matches: Vec<_> = locals.iter().filter(|binding| binding.ty == ty).collect();
        (!matches.is_empty()).then(|| matches[self.choose(matches.len())])
    }

    fn matching_mutable_local(&mut self, locals: &[GeneratedBinding]) -> Option<GeneratedBinding> {
        let matches: Vec<_> = locals
            .iter()
            .copied()
            .filter(|binding| binding.mutable)
            .collect();
        (!matches.is_empty()).then(|| matches[self.choose(matches.len())])
    }

    fn matching_function<'b>(
        &mut self,
        functions: &'b [Function],
        ty: Type,
    ) -> Option<(usize, &'b Function)> {
        let matches: Vec<_> = functions
            .iter()
            .enumerate()
            .filter(|(_, function)| function.return_type == ty)
            .collect();
        (!matches.is_empty()).then(|| matches[self.choose(matches.len())])
    }

    fn call(&mut self, id: usize, function: &Function) -> Expression {
        let arguments = function
            .parameters
            .iter()
            .map(|parameter| self.leaf(parameter.ty))
            .collect();
        Expression {
            kind: ExpressionKind::Call {
                function: id,
                arguments,
            },
            ty: function.return_type,
            span: GENERATED_RANGE,
        }
    }

    fn leaf(&mut self, ty: Type) -> Expression {
        match ty {
            Type::I64 => integer(self.small_i64()),
            Type::Bool => boolean(self.choose(2) == 0),
            Type::Unit => unit(),
        }
    }

    fn small_i64(&mut self) -> i64 {
        self.choose(41) as i64 - 20
    }

    fn choose(&mut self, upper: usize) -> usize {
        debug_assert!(upper > 0);
        let decision = self
            .decisions
            .get(self.cursor)
            .copied()
            .unwrap_or(self.cursor as u64);
        self.cursor = self.cursor.saturating_add(1);
        (decision % upper as u64) as usize
    }
}

impl GenerationContext<'_> {
    fn deeper(&self, expected_type: Type) -> GenerationContext<'_> {
        GenerationContext {
            locals: self.locals,
            functions: self.functions,
            expected_type,
            expression_depth: self.expression_depth.saturating_add(1),
            statement_budget: self.statement_budget,
            loop_depth: self.loop_depth,
        }
    }
}

fn integer(value: i64) -> Expression {
    if value < 0 {
        unary(
            UnaryOp::Neg,
            value_expression(Value::I64(-value), Type::I64),
            Type::I64,
        )
    } else {
        value_expression(Value::I64(value), Type::I64)
    }
}

fn boolean(value: bool) -> Expression {
    value_expression(Value::Bool(value), Type::Bool)
}

fn unit() -> Expression {
    value_expression(Value::Unit, Type::Unit)
}

fn value_expression(value: Value, ty: Type) -> Expression {
    Expression {
        kind: ExpressionKind::Value(value),
        ty,
        span: GENERATED_RANGE,
    }
}

fn local(id: usize, ty: Type) -> Expression {
    Expression {
        kind: ExpressionKind::Local(id),
        ty,
        span: GENERATED_RANGE,
    }
}

fn unary(op: UnaryOp, operand: Expression, ty: Type) -> Expression {
    Expression {
        kind: ExpressionKind::Unary {
            op,
            operand: Box::new(operand),
        },
        ty,
        span: GENERATED_RANGE,
    }
}

fn binary(op: BinaryOp, lhs: Expression, rhs: Expression, ty: Type) -> Expression {
    Expression {
        kind: ExpressionKind::Binary {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        },
        ty,
        span: GENERATED_RANGE,
    }
}

fn block_expression(tail: Expression, ty: Type) -> Expression {
    Expression {
        kind: ExpressionKind::Block(tail_block(tail)),
        ty,
        span: GENERATED_RANGE,
    }
}

fn tail_block(tail: Expression) -> Block {
    Block {
        statements: Vec::new(),
        tail: Some(Box::new(tail)),
        span: GENERATED_RANGE,
    }
}

fn default_expression(ty: Type) -> Expression {
    match ty {
        Type::I64 => integer(0),
        Type::Bool => boolean(false),
        Type::Unit => unit(),
    }
}

fn expression_replacements(expression: &Expression) -> Vec<Expression> {
    let mut replacements = Vec::new();
    push_replacement(&mut replacements, default_expression(expression.ty));
    match &expression.kind {
        ExpressionKind::Value(Value::I64(value)) if *value != 0 => {
            push_replacement(&mut replacements, integer(value / 2));
        }
        ExpressionKind::Value(Value::Bool(true)) => {
            push_replacement(&mut replacements, boolean(false));
        }
        ExpressionKind::Value(_) | ExpressionKind::Local(_) => {}
        ExpressionKind::Call {
            function,
            arguments,
        } => {
            for (index, argument) in arguments.iter().enumerate() {
                for replacement in expression_replacements(argument).into_iter().take(4) {
                    if replacement.ty == argument.ty {
                        let mut arguments = arguments.clone();
                        arguments[index] = replacement;
                        push_replacement(
                            &mut replacements,
                            Expression {
                                kind: ExpressionKind::Call {
                                    function: *function,
                                    arguments,
                                },
                                ty: expression.ty,
                                span: expression.span,
                            },
                        );
                    }
                }
            }
        }
        ExpressionKind::Unary { op, operand } => {
            if operand.ty == expression.ty {
                push_replacement(&mut replacements, (**operand).clone());
            }
            for replacement in expression_replacements(operand).into_iter().take(4) {
                push_replacement(
                    &mut replacements,
                    Expression {
                        kind: ExpressionKind::Unary {
                            op: *op,
                            operand: Box::new(replacement),
                        },
                        ty: expression.ty,
                        span: expression.span,
                    },
                );
            }
        }
        ExpressionKind::Binary { op, lhs, rhs } => {
            if lhs.ty == expression.ty {
                push_replacement(&mut replacements, (**lhs).clone());
            }
            if rhs.ty == expression.ty {
                push_replacement(&mut replacements, (**rhs).clone());
            }
            for replacement in expression_replacements(lhs).into_iter().take(4) {
                if replacement.ty == lhs.ty {
                    push_replacement(
                        &mut replacements,
                        Expression {
                            kind: ExpressionKind::Binary {
                                op: *op,
                                lhs: Box::new(replacement),
                                rhs: rhs.clone(),
                            },
                            ty: expression.ty,
                            span: expression.span,
                        },
                    );
                }
            }
            for replacement in expression_replacements(rhs).into_iter().take(4) {
                if replacement.ty == rhs.ty {
                    push_replacement(
                        &mut replacements,
                        Expression {
                            kind: ExpressionKind::Binary {
                                op: *op,
                                lhs: lhs.clone(),
                                rhs: Box::new(replacement),
                            },
                            ty: expression.ty,
                            span: expression.span,
                        },
                    );
                }
            }
        }
        ExpressionKind::Block(block) => {
            if let Some(tail) = &block.tail
                && tail.ty == expression.ty
            {
                push_replacement(&mut replacements, (**tail).clone());
            }
        }
        ExpressionKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            for replacement in expression_replacements(condition).into_iter().take(4) {
                if replacement.ty == Type::Bool {
                    push_replacement(
                        &mut replacements,
                        Expression {
                            kind: ExpressionKind::If {
                                condition: Box::new(replacement),
                                then_branch: then_branch.clone(),
                                else_branch: else_branch.clone(),
                            },
                            ty: expression.ty,
                            span: expression.span,
                        },
                    );
                }
            }
            for branch in [then_branch, else_branch] {
                if let Some(tail) = &branch.tail
                    && tail.ty == expression.ty
                {
                    push_replacement(&mut replacements, (**tail).clone());
                }
            }
        }
    }
    replacements.retain(|replacement| !expression.structurally_matches(replacement));
    replacements
}

fn push_replacement(replacements: &mut Vec<Expression>, replacement: Expression) {
    if !replacements
        .iter()
        .any(|existing| existing.structurally_matches(&replacement))
    {
        replacements.push(replacement);
    }
}

fn uncalled_function_indices(program: &CheckedProgram) -> Vec<usize> {
    let mut called = vec![false; program.functions.len()];
    for function in &program.functions {
        collect_called_functions_in_block(&function.body, &mut called);
    }
    called
        .into_iter()
        .enumerate()
        .filter_map(|(index, called)| (index != program.main && !called).then_some(index))
        .collect()
}

fn remove_function(program: &mut CheckedProgram, removed: usize) {
    program.functions.remove(removed);
    if program.main > removed {
        program.main -= 1;
    }
    for function in &mut program.functions {
        remap_calls_in_block(&mut function.body, removed);
    }
}

fn collect_called_functions_in_block(block: &Block, called: &mut [bool]) {
    for statement in &block.statements {
        match statement {
            Statement::Let { initializer, .. } => {
                collect_called_functions_in_expression(initializer, called);
            }
            Statement::Assign { value, .. }
            | Statement::Return { value, .. }
            | Statement::Print { value, .. }
            | Statement::Expression(value) => {
                collect_called_functions_in_expression(value, called);
            }
            Statement::While {
                condition, body, ..
            } => {
                collect_called_functions_in_expression(condition, called);
                collect_called_functions_in_block(body, called);
            }
            Statement::Break(_) | Statement::Continue(_) => {}
        }
    }
    if let Some(tail) = &block.tail {
        collect_called_functions_in_expression(tail, called);
    }
}

fn collect_called_functions_in_expression(expression: &Expression, called: &mut [bool]) {
    match &expression.kind {
        ExpressionKind::Value(_) | ExpressionKind::Local(_) => {}
        ExpressionKind::Call {
            function,
            arguments,
        } => {
            if let Some(slot) = called.get_mut(*function) {
                *slot = true;
            }
            for argument in arguments {
                collect_called_functions_in_expression(argument, called);
            }
        }
        ExpressionKind::Unary { operand, .. } => {
            collect_called_functions_in_expression(operand, called)
        }
        ExpressionKind::Binary { lhs, rhs, .. } => {
            collect_called_functions_in_expression(lhs, called);
            collect_called_functions_in_expression(rhs, called);
        }
        ExpressionKind::Block(block) => collect_called_functions_in_block(block, called),
        ExpressionKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_called_functions_in_expression(condition, called);
            collect_called_functions_in_block(then_branch, called);
            collect_called_functions_in_block(else_branch, called);
        }
    }
}

fn remap_calls_in_block(block: &mut Block, removed: usize) {
    for statement in &mut block.statements {
        match statement {
            Statement::Let { initializer, .. } => remap_calls_in_expression(initializer, removed),
            Statement::Assign { value, .. }
            | Statement::Return { value, .. }
            | Statement::Print { value, .. }
            | Statement::Expression(value) => remap_calls_in_expression(value, removed),
            Statement::While {
                condition, body, ..
            } => {
                remap_calls_in_expression(condition, removed);
                remap_calls_in_block(body, removed);
            }
            Statement::Break(_) | Statement::Continue(_) => {}
        }
    }
    if let Some(tail) = &mut block.tail {
        remap_calls_in_expression(tail, removed);
    }
}

fn remap_calls_in_expression(expression: &mut Expression, removed: usize) {
    match &mut expression.kind {
        ExpressionKind::Value(_) | ExpressionKind::Local(_) => {}
        ExpressionKind::Call {
            function,
            arguments,
        } => {
            if *function > removed {
                *function -= 1;
            }
            for argument in arguments {
                remap_calls_in_expression(argument, removed);
            }
        }
        ExpressionKind::Unary { operand, .. } => remap_calls_in_expression(operand, removed),
        ExpressionKind::Binary { lhs, rhs, .. } => {
            remap_calls_in_expression(lhs, removed);
            remap_calls_in_expression(rhs, removed);
        }
        ExpressionKind::Block(block) => remap_calls_in_block(block, removed),
        ExpressionKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            remap_calls_in_expression(condition, removed);
            remap_calls_in_block(then_branch, removed);
            remap_calls_in_block(else_branch, removed);
        }
    }
}

fn block_uses_local(block: &Block, id: usize) -> bool {
    block.statements.iter().any(|statement| match statement {
        Statement::Let { initializer, .. } => expression_uses_local(initializer, id),
        Statement::Assign { value, .. }
        | Statement::Return { value, .. }
        | Statement::Print { value, .. }
        | Statement::Expression(value) => expression_uses_local(value, id),
        Statement::While {
            condition, body, ..
        } => expression_uses_local(condition, id) || block_uses_local(body, id),
        Statement::Break(_) | Statement::Continue(_) => false,
    }) || block
        .tail
        .as_deref()
        .is_some_and(|tail| expression_uses_local(tail, id))
}

fn expression_uses_local(expression: &Expression, id: usize) -> bool {
    match &expression.kind {
        ExpressionKind::Value(_) => false,
        ExpressionKind::Local(local) => *local == id,
        ExpressionKind::Call { arguments, .. } => arguments
            .iter()
            .any(|argument| expression_uses_local(argument, id)),
        ExpressionKind::Unary { operand, .. } => expression_uses_local(operand, id),
        ExpressionKind::Binary { lhs, rhs, .. } => {
            expression_uses_local(lhs, id) || expression_uses_local(rhs, id)
        }
        ExpressionKind::Block(block) => block_uses_local(block, id),
        ExpressionKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expression_uses_local(condition, id)
                || block_uses_local(then_branch, id)
                || block_uses_local(else_branch, id)
        }
    }
}

fn block_assigns_local(block: &Block, id: usize) -> bool {
    block.statements.iter().any(|statement| match statement {
        Statement::Assign { id: assigned, .. } => *assigned == id,
        Statement::Let { initializer, .. } => expression_assigns_local(initializer, id),
        Statement::Return { value, .. }
        | Statement::Print { value, .. }
        | Statement::Expression(value) => expression_assigns_local(value, id),
        Statement::While {
            condition, body, ..
        } => expression_assigns_local(condition, id) || block_assigns_local(body, id),
        Statement::Break(_) | Statement::Continue(_) => false,
    }) || block
        .tail
        .as_deref()
        .is_some_and(|tail| expression_assigns_local(tail, id))
}

fn expression_assigns_local(expression: &Expression, id: usize) -> bool {
    match &expression.kind {
        ExpressionKind::Value(_) | ExpressionKind::Local(_) => false,
        ExpressionKind::Call { arguments, .. } => arguments
            .iter()
            .any(|argument| expression_assigns_local(argument, id)),
        ExpressionKind::Unary { operand, .. } => expression_assigns_local(operand, id),
        ExpressionKind::Binary { lhs, rhs, .. } => {
            expression_assigns_local(lhs, id) || expression_assigns_local(rhs, id)
        }
        ExpressionKind::Block(block) => block_assigns_local(block, id),
        ExpressionKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expression_assigns_local(condition, id)
                || block_assigns_local(then_branch, id)
                || block_assigns_local(else_branch, id)
        }
    }
}

fn replace_local_in_block(block: &mut Block, id: usize, replacement: &Expression) {
    for statement in &mut block.statements {
        match statement {
            Statement::Let { initializer, .. } => {
                replace_local_in_expression(initializer, id, replacement);
            }
            Statement::Assign { value, .. }
            | Statement::Return { value, .. }
            | Statement::Print { value, .. }
            | Statement::Expression(value) => replace_local_in_expression(value, id, replacement),
            Statement::While {
                condition, body, ..
            } => {
                replace_local_in_expression(condition, id, replacement);
                replace_local_in_block(body, id, replacement);
            }
            Statement::Break(_) | Statement::Continue(_) => {}
        }
    }
    if let Some(tail) = &mut block.tail {
        replace_local_in_expression(tail, id, replacement);
    }
}

fn replace_local_in_expression(expression: &mut Expression, id: usize, replacement: &Expression) {
    match &mut expression.kind {
        ExpressionKind::Value(_) => {}
        ExpressionKind::Local(local) if *local == id => *expression = replacement.clone(),
        ExpressionKind::Local(_) => {}
        ExpressionKind::Call { arguments, .. } => {
            for argument in arguments {
                replace_local_in_expression(argument, id, replacement);
            }
        }
        ExpressionKind::Unary { operand, .. } => {
            replace_local_in_expression(operand, id, replacement);
        }
        ExpressionKind::Binary { lhs, rhs, .. } => {
            replace_local_in_expression(lhs, id, replacement);
            replace_local_in_expression(rhs, id, replacement);
        }
        ExpressionKind::Block(block) => replace_local_in_block(block, id, replacement),
        ExpressionKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            replace_local_in_expression(condition, id, replacement);
            replace_local_in_block(then_branch, id, replacement);
            replace_local_in_block(else_branch, id, replacement);
        }
    }
}

trait StructuralExpressionMatch {
    fn structurally_matches(&self, other: &Self) -> bool;
}

impl StructuralExpressionMatch for Expression {
    fn structurally_matches(&self, other: &Self) -> bool {
        self.kind == other.kind && self.ty == other.ty
    }
}

#[cfg(test)]
mod tests {
    use crate::checked_ir::{Block, CheckedProgram, Expression, ExpressionKind, Statement};
    use crate::{Limits, RuntimeLimits, check_source, format, run};
    use proptest::prelude::*;

    #[test]
    fn generated_ir_round_trips_terminates_and_exercises_output() {
        for seed in 0_u64..128 {
            let decisions: Vec<_> = (0..64)
                .map(|index| seed.wrapping_mul(31).wrapping_add(index * 17))
                .collect();
            let program = super::generate_checked_program(&decisions);
            let source = format(&program);
            let reparsed = check_source(&source, Limits::default())
                .unwrap_or_else(|error| panic!("seed {seed} did not check: {error}\n{source}"));
            assert_program_statement_bound(&program, 8);
            assert!(program.structurally_eq(&reparsed), "seed {seed}\n{source}");
            assert_eq!(source, format(&reparsed), "seed {seed}");
            let result = run(&reparsed, RuntimeLimits::default())
                .unwrap_or_else(|error| panic!("seed {seed} trapped: {error}\n{source}"));
            assert!(!result.stdout.is_empty(), "seed {seed}");
            assert!(result.stdout.iter().filter(|byte| **byte == b'\n').count() <= 10);
        }
    }

    #[test]
    fn generated_blocks_never_exceed_statement_budget() {
        for seed in 0_u64..1024 {
            let decisions: Vec<_> = (0..96)
                .map(|index| seed.wrapping_mul(1_315_423_911).wrapping_add(index * 97))
                .collect();
            let program = super::generate_checked_program(&decisions);
            assert_program_statement_bound(&program, 8);
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(32))]

        #[test]
        fn arbitrary_decisions_round_trip_and_interpret_deterministically(
            decisions in prop::collection::vec(any::<u64>(), 0..96),
        ) {
            let generated = super::generate_checked_program(&decisions);
            assert_program_statement_bound(&generated, 8);
            let source = format(&generated);
            let checked = check_source(&source, Limits::default())?;
            prop_assert!(generated.structurally_eq(&checked));
            prop_assert_eq!(&source, &format(&checked));
            let first = run(&checked, RuntimeLimits::default())?;
            let second = run(&checked, RuntimeLimits::default())?;
            prop_assert_eq!(first, second);
        }
    }

    fn assert_program_statement_bound(program: &CheckedProgram, limit: usize) {
        for function in &program.functions {
            assert_block_statement_bound(&function.body, limit);
        }
    }

    fn assert_block_statement_bound(block: &Block, limit: usize) {
        assert!(
            block.statements.len() <= limit,
            "block has {} statements, expected at most {limit}",
            block.statements.len()
        );
        for statement in &block.statements {
            match statement {
                Statement::Let { initializer, .. } => {
                    assert_expression_statement_bound(initializer, limit);
                }
                Statement::Assign { value, .. }
                | Statement::Return { value, .. }
                | Statement::Print { value, .. }
                | Statement::Expression(value) => assert_expression_statement_bound(value, limit),
                Statement::While {
                    condition, body, ..
                } => {
                    assert_expression_statement_bound(condition, limit);
                    assert_block_statement_bound(body, limit);
                }
                Statement::Break(_) | Statement::Continue(_) => {}
            }
        }
        if let Some(tail) = &block.tail {
            assert_expression_statement_bound(tail, limit);
        }
    }

    fn assert_expression_statement_bound(expression: &Expression, limit: usize) {
        match &expression.kind {
            ExpressionKind::Value(_) | ExpressionKind::Local(_) => {}
            ExpressionKind::Call { arguments, .. } => {
                for argument in arguments {
                    assert_expression_statement_bound(argument, limit);
                }
            }
            ExpressionKind::Unary { operand, .. } => {
                assert_expression_statement_bound(operand, limit);
            }
            ExpressionKind::Binary { lhs, rhs, .. } => {
                assert_expression_statement_bound(lhs, limit);
                assert_expression_statement_bound(rhs, limit);
            }
            ExpressionKind::Block(block) => assert_block_statement_bound(block, limit),
            ExpressionKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                assert_expression_statement_bound(condition, limit);
                assert_block_statement_bound(then_branch, limit);
                assert_block_statement_bound(else_branch, limit);
            }
        }
    }
}
