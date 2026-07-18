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
    for (function_index, function) in program.functions.iter().enumerate() {
        if let Some(tail) = &function.body.tail {
            let replacement = default_expression(tail.ty);
            if !tail.structurally_matches(&replacement) {
                let mut candidate = program.clone();
                candidate.functions[function_index].body.tail = Some(Box::new(replacement));
                candidates.push(candidate);
            }
        }
        for statement_index in 0..function.body.statements.len() {
            match &function.body.statements[statement_index] {
                Statement::Print { value, .. } => {
                    let mut simplified = program.clone();
                    simplified.functions[function_index].body.statements[statement_index] =
                        Statement::Print {
                            value: default_expression(value.ty),
                            span: GENERATED_RANGE,
                        };
                    candidates.push(simplified);

                    let mut removed = program.clone();
                    removed.functions[function_index]
                        .body
                        .statements
                        .remove(statement_index);
                    candidates.push(removed);
                }
                Statement::Expression(expression) if expression.ty == Type::Unit => {
                    let mut candidate = program.clone();
                    candidate.functions[function_index]
                        .body
                        .statements
                        .remove(statement_index);
                    candidates.push(candidate);
                }
                Statement::While { body, .. } => {
                    for nested_index in 0..body.statements.len() {
                        if matches!(body.statements[nested_index], Statement::Print { .. }) {
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
            })
            .collect();
        let body = Block {
            statements: Vec::new(),
            tail: Some(Box::new(self.expression(GenerationContext {
                locals: &locals,
                functions: earlier,
                expected_type: return_type,
                expression_depth: 0,
                statement_budget: 0,
                loop_depth: 0,
            }))),
            span: GENERATED_RANGE,
        };
        Function {
            name: SmolStr::new(format!("f{id}")),
            parameters,
            return_type,
            explicit_return: true,
            local_count: parameter_count,
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
            },
            GeneratedBinding {
                id: 1,
                ty: Type::Bool,
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
                let op = match self.choose(4) {
                    0 => ArithOp::Add,
                    1 => ArithOp::Sub,
                    2 => ArithOp::Div,
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
            assert!(program.structurally_eq(&reparsed), "seed {seed}\n{source}");
            assert_eq!(source, format(&reparsed), "seed {seed}");
            let result = run(&reparsed, RuntimeLimits::default())
                .unwrap_or_else(|error| panic!("seed {seed} trapped: {error}\n{source}"));
            assert!(!result.stdout.is_empty(), "seed {seed}");
            assert!(result.stdout.iter().filter(|byte| **byte == b'\n').count() <= 10);
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(32))]

        #[test]
        fn arbitrary_decisions_round_trip_and_interpret_deterministically(
            decisions in prop::collection::vec(any::<u64>(), 0..96),
        ) {
            let generated = super::generate_checked_program(&decisions);
            let source = format(&generated);
            let checked = check_source(&source, Limits::default())?;
            prop_assert!(generated.structurally_eq(&checked));
            prop_assert_eq!(&source, &format(&checked));
            let first = run(&checked, RuntimeLimits::default())?;
            let second = run(&checked, RuntimeLimits::default())?;
            prop_assert_eq!(first, second);
        }
    }
}
