use ra_ap_syntax::TextRange;
use ra_ap_syntax::ast::{ArithOp, BinaryOp, CmpOp, LogicOp, Ordering, UnaryOp};

use crate::checked_ir::{Block, Expression, ExpressionKind, FunctionId, Statement};
use crate::{CheckedProgram, Diagnostic, Phase, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct RuntimeLimits {
    pub fuel: u64,
    pub max_call_depth: usize,
    pub max_output_bytes: usize,
}

impl Default for RuntimeLimits {
    fn default() -> Self {
        Self {
            fuel: 1_000_000,
            max_call_depth: 1_024,
            max_output_bytes: 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct RunResult {
    pub stdout: Vec<u8>,
    pub steps: u64,
}

/// Compatibility alias for the interpreter result.
pub type Execution = RunResult;

enum Control {
    Value(Value),
    Return(Value),
    Break,
    Continue,
}

macro_rules! value_or_control {
    ($control:expr) => {
        match $control {
            Control::Value(value) => value,
            control => return Ok(control),
        }
    };
}

struct Evaluator<'a> {
    program: &'a CheckedProgram,
    limits: RuntimeLimits,
    fuel: u64,
    output: Vec<u8>,
    call_depth: usize,
}

pub(crate) fn run(
    program: &CheckedProgram,
    limits: RuntimeLimits,
) -> Result<RunResult, Diagnostic> {
    let mut evaluator = Evaluator {
        program,
        limits,
        fuel: limits.fuel,
        output: Vec::new(),
        call_depth: 0,
    };
    let value = evaluator.call(program.main, Vec::new(), None)?;
    if value != Value::Unit {
        return Err(runtime_error("main returned a non-unit value", None));
    }
    Ok(RunResult {
        stdout: evaluator.output,
        steps: limits.fuel - evaluator.fuel,
    })
}

impl Evaluator<'_> {
    fn call(
        &mut self,
        id: FunctionId,
        arguments: Vec<Value>,
        span: Option<TextRange>,
    ) -> Result<Value, Diagnostic> {
        if self.call_depth >= self.limits.max_call_depth {
            return Err(runtime_error("call depth limit exceeded", span));
        }
        let function = self
            .program
            .functions
            .get(id)
            .ok_or_else(|| runtime_error("invalid checked function id", span))?;
        if arguments.len() != function.parameters.len() {
            return Err(runtime_error("invalid checked argument count", span));
        }
        let mut locals = vec![None; function.local_count];
        for (parameter, value) in function.parameters.iter().zip(arguments) {
            let slot = locals
                .get_mut(parameter.id)
                .ok_or_else(|| runtime_error("invalid checked local id", span))?;
            *slot = Some(value);
        }
        self.call_depth += 1;
        let result = self.eval_block(&function.body, &mut locals);
        self.call_depth -= 1;
        match result? {
            Control::Value(value) | Control::Return(value) => Ok(value),
            Control::Break | Control::Continue => {
                Err(runtime_error("loop control escaped function", span))
            }
        }
    }

    fn eval_block(
        &mut self,
        block: &Block,
        locals: &mut [Option<Value>],
    ) -> Result<Control, Diagnostic> {
        for statement in &block.statements {
            match self.eval_statement(statement, locals)? {
                Control::Value(_) => {}
                control => return Ok(control),
            }
        }
        match &block.tail {
            Some(tail) => self.eval_expression(tail, locals),
            None => Ok(Control::Value(Value::Unit)),
        }
    }

    fn eval_statement(
        &mut self,
        statement: &Statement,
        locals: &mut [Option<Value>],
    ) -> Result<Control, Diagnostic> {
        self.step(statement_span(statement))?;
        match statement {
            Statement::Let {
                id, initializer, ..
            } => {
                let value = value_or_control!(self.eval_expression(initializer, locals)?);
                self.set_local(locals, *id, value, initializer.span)?;
                Ok(Control::Value(Value::Unit))
            }
            Statement::Assign { id, value, span } => {
                let value = value_or_control!(self.eval_expression(value, locals)?);
                self.set_local(locals, *id, value, *span)?;
                Ok(Control::Value(Value::Unit))
            }
            Statement::While {
                condition, body, ..
            } => {
                loop {
                    let value = value_or_control!(self.eval_expression(condition, locals)?);
                    let Value::Bool(keep_going) = value else {
                        return Err(runtime_error(
                            "invalid checked while condition",
                            Some(condition.span),
                        ));
                    };
                    if !keep_going {
                        break;
                    }
                    match self.eval_block(body, locals)? {
                        Control::Value(_) | Control::Continue => {}
                        Control::Break => break,
                        Control::Return(value) => return Ok(Control::Return(value)),
                    }
                }
                Ok(Control::Value(Value::Unit))
            }
            Statement::Return { value, .. } => match self.eval_expression(value, locals)? {
                Control::Value(value) => Ok(Control::Return(value)),
                control => Ok(control),
            },
            Statement::Print { value, span } => {
                let value = value_or_control!(self.eval_expression(value, locals)?);
                self.print(value, *span)?;
                Ok(Control::Value(Value::Unit))
            }
            Statement::Break(_) => Ok(Control::Break),
            Statement::Continue(_) => Ok(Control::Continue),
            Statement::Expression(expression) => match self.eval_expression(expression, locals)? {
                Control::Value(_) => Ok(Control::Value(Value::Unit)),
                control => Ok(control),
            },
        }
    }

    fn eval_expression(
        &mut self,
        expression: &Expression,
        locals: &mut [Option<Value>],
    ) -> Result<Control, Diagnostic> {
        self.step(expression.span)?;
        let value = match &expression.kind {
            ExpressionKind::Value(value) => *value,
            ExpressionKind::Local(id) => {
                locals.get(*id).and_then(|slot| *slot).ok_or_else(|| {
                    runtime_error(
                        "invalid or uninitialized checked local",
                        Some(expression.span),
                    )
                })?
            }
            ExpressionKind::Call {
                function,
                arguments,
            } => {
                let mut values = Vec::with_capacity(arguments.len());
                for argument in arguments {
                    values.push(value_or_control!(self.eval_expression(argument, locals)?));
                }
                self.call(*function, values, Some(expression.span))?
            }
            ExpressionKind::Unary { op, operand } => {
                let operand = value_or_control!(self.eval_expression(operand, locals)?);
                self.unary(*op, operand, expression.span)?
            }
            ExpressionKind::Binary { op, lhs, rhs } => {
                return self.binary(*op, lhs, rhs, locals, expression.span);
            }
            ExpressionKind::Block(block) => return self.eval_block(block, locals),
            ExpressionKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let condition = value_or_control!(self.eval_expression(condition, locals)?);
                match condition {
                    Value::Bool(true) => return self.eval_block(then_branch, locals),
                    Value::Bool(false) => return self.eval_block(else_branch, locals),
                    _ => {
                        return Err(runtime_error(
                            "invalid checked if condition",
                            Some(expression.span),
                        ));
                    }
                }
            }
        };
        Ok(Control::Value(value))
    }

    fn binary(
        &mut self,
        op: BinaryOp,
        lhs: &Expression,
        rhs: &Expression,
        locals: &mut [Option<Value>],
        span: TextRange,
    ) -> Result<Control, Diagnostic> {
        let left = value_or_control!(self.eval_expression(lhs, locals)?);
        if matches!(
            (op, left),
            (BinaryOp::LogicOp(LogicOp::And), Value::Bool(false))
        ) {
            return Ok(Control::Value(Value::Bool(false)));
        }
        if matches!(
            (op, left),
            (BinaryOp::LogicOp(LogicOp::Or), Value::Bool(true))
        ) {
            return Ok(Control::Value(Value::Bool(true)));
        }
        let right = value_or_control!(self.eval_expression(rhs, locals)?);
        let value = match (op, left, right) {
            (BinaryOp::ArithOp(op), Value::I64(lhs), Value::I64(rhs)) => Value::I64(
                arithmetic(op, lhs, rhs)
                    .ok_or_else(|| runtime_error(arithmetic_message(op, rhs), Some(span)))?,
            ),
            (BinaryOp::LogicOp(LogicOp::And), Value::Bool(lhs), Value::Bool(rhs)) => {
                Value::Bool(lhs && rhs)
            }
            (BinaryOp::LogicOp(LogicOp::Or), Value::Bool(lhs), Value::Bool(rhs)) => {
                Value::Bool(lhs || rhs)
            }
            (BinaryOp::CmpOp(CmpOp::Eq { negated }), lhs, rhs) => {
                Value::Bool((lhs == rhs) ^ negated)
            }
            (
                BinaryOp::CmpOp(CmpOp::Ord { ordering, strict }),
                Value::I64(lhs),
                Value::I64(rhs),
            ) => Value::Bool(match (ordering, strict) {
                (Ordering::Less, true) => lhs < rhs,
                (Ordering::Less, false) => lhs <= rhs,
                (Ordering::Greater, true) => lhs > rhs,
                (Ordering::Greater, false) => lhs >= rhs,
            }),
            _ => return Err(runtime_error("invalid checked binary operands", Some(span))),
        };
        Ok(Control::Value(value))
    }

    fn unary(&self, op: UnaryOp, operand: Value, span: TextRange) -> Result<Value, Diagnostic> {
        match (op, operand) {
            (UnaryOp::Neg, Value::I64(value)) => value
                .checked_neg()
                .map(Value::I64)
                .ok_or_else(|| runtime_error("integer negation overflow", Some(span))),
            (UnaryOp::Not, Value::Bool(value)) => Ok(Value::Bool(!value)),
            _ => Err(runtime_error("invalid checked unary operand", Some(span))),
        }
    }

    fn set_local(
        &self,
        locals: &mut [Option<Value>],
        id: usize,
        value: Value,
        span: TextRange,
    ) -> Result<(), Diagnostic> {
        let slot = locals
            .get_mut(id)
            .ok_or_else(|| runtime_error("invalid checked local id", Some(span)))?;
        *slot = Some(value);
        Ok(())
    }

    fn step(&mut self, span: TextRange) -> Result<(), Diagnostic> {
        if self.fuel == 0 {
            return Err(runtime_error("fuel exhausted", Some(span)));
        }
        self.fuel -= 1;
        Ok(())
    }

    fn print(&mut self, value: Value, span: TextRange) -> Result<(), Diagnostic> {
        let mut line = match value {
            Value::I64(value) => value.to_string().into_bytes(),
            Value::Bool(value) => {
                if value {
                    b"true".to_vec()
                } else {
                    b"false".to_vec()
                }
            }
            Value::Unit => {
                return Err(runtime_error("invalid checked print value", Some(span)));
            }
        };
        line.push(b'\n');
        let new_len = self
            .output
            .len()
            .checked_add(line.len())
            .ok_or_else(|| runtime_error("output byte limit exceeded", Some(span)))?;
        if new_len > self.limits.max_output_bytes {
            return Err(runtime_error("output byte limit exceeded", Some(span)));
        }
        self.output.extend_from_slice(&line);
        Ok(())
    }
}

fn arithmetic(op: ArithOp, lhs: i64, rhs: i64) -> Option<i64> {
    match op {
        ArithOp::Add => lhs.checked_add(rhs),
        ArithOp::Sub => lhs.checked_sub(rhs),
        ArithOp::Mul => lhs.checked_mul(rhs),
        ArithOp::Div => lhs.checked_div(rhs),
        ArithOp::Rem => lhs.checked_rem(rhs),
        _ => None,
    }
}

fn arithmetic_message(op: ArithOp, rhs: i64) -> &'static str {
    match (op, rhs) {
        (ArithOp::Div, 0) => "division by zero",
        (ArithOp::Rem, 0) => "remainder by zero",
        _ => "integer arithmetic overflow",
    }
}

fn statement_span(statement: &Statement) -> TextRange {
    match statement {
        Statement::Let { initializer, .. } => initializer.span,
        Statement::Assign { span, .. }
        | Statement::While { span, .. }
        | Statement::Return { span, .. }
        | Statement::Print { span, .. }
        | Statement::Break(span)
        | Statement::Continue(span) => *span,
        Statement::Expression(expression) => expression.span,
    }
}

fn runtime_error(message: &str, span: Option<TextRange>) -> Diagnostic {
    Diagnostic::new(Phase::Runtime, message, span.map(crate::diagnostic::span))
}

#[cfg(test)]
mod tests {
    use crate::{Limits, RuntimeLimits, check_source, run};

    fn execute(source: &str) -> Result<crate::Execution, crate::Diagnostic> {
        run(
            &check_source(source, Limits::default()).unwrap(),
            RuntimeLimits::default(),
        )
    }

    #[test]
    fn executes_calls_mutation_loops_and_short_circuiting() {
        let source = "fn add(x: i64, y: i64) -> i64 { x + y } fn main() { let mut x = add(1_i64, 2_i64); while x < 5_i64 { x = x + 1_i64; } let ok = false && (1_i64 / 0_i64 == 0_i64); println!(\"{}\", x); println!(\"{}\", ok); }";
        let first = execute(source).unwrap();
        let second = execute(source).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.stdout, b"5\nfalse\n");
        assert!(first.steps > 0);
    }

    #[test]
    fn traps_arithmetic_and_fuel_exhaustion() {
        assert!(execute("fn main() { 9223372036854775807_i64 + 1_i64; }").is_err());
        let program = check_source("fn main() { while true {} }", Limits::default()).unwrap();
        let error = run(
            &program,
            RuntimeLimits {
                fuel: 10,
                ..RuntimeLimits::default()
            },
        )
        .unwrap_err();
        assert_eq!(error.phase, crate::Phase::Runtime);

        let program = check_source(
            "fn main() { println!(\"{}\", 123_i64); }",
            Limits::default(),
        )
        .unwrap();
        let error = run(
            &program,
            RuntimeLimits {
                max_output_bytes: 3,
                ..RuntimeLimits::default()
            },
        )
        .unwrap_err();
        assert_eq!(error.message, "output byte limit exceeded");
    }
}
