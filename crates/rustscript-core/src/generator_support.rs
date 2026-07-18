use ra_ap_syntax::SmolStr;
use ra_ap_syntax::ast::{ArithOp, BinaryOp, CmpOp};

use crate::checked_ir::{
    Block, CheckedProgram, Expression, ExpressionKind, Function, Parameter, Statement,
};
use crate::{Span, Type, Value};

const GENERATED_SPAN: Span = Span { start: 0, end: 0 };

/// Build a bounded, successfully terminating checked program from decisions.
///
/// This is deliberately a checked-IR generator rather than a source-string or
/// syntax-AST generator. The canonical emitter is the only path to Rust text.
pub fn generate_checked_program(decisions: &[u64]) -> CheckedProgram {
    let decision = |index: usize| decisions.get(index).copied().unwrap_or(index as u64);
    let helper_count = (decision(0) % 5) as usize;
    let mut functions = Vec::with_capacity(helper_count + 1);
    for id in 0..helper_count {
        let amount = (decision(id + 1) % 9) as i64;
        let parameter = Parameter {
            id: 0,
            name: SmolStr::new("x0"),
            ty: Type::I64,
        };
        let body_expression = binary(
            BinaryOp::ArithOp(ArithOp::Add),
            local(0, Type::I64),
            value(Value::I64(amount), Type::I64),
            Type::I64,
        );
        functions.push(Function {
            name: SmolStr::new(format!("f{id}")),
            parameters: vec![parameter],
            return_type: Type::I64,
            explicit_return: true,
            local_count: 1,
            body: Block {
                statements: Vec::new(),
                tail: Some(Box::new(body_expression)),
                span: GENERATED_SPAN,
            },
        });
    }

    let initial = (decision(8) % 11) as i64;
    let iterations = (decision(9) % 9) as i64;
    let mut statements = vec![Statement::Let {
        id: 0,
        name: SmolStr::new("x0"),
        mutable: true,
        annotation: Some(Type::I64),
        initializer: value(Value::I64(initial), Type::I64),
    }];
    for function in 0..helper_count {
        statements.push(Statement::Assign {
            id: 0,
            value: Expression {
                kind: ExpressionKind::Call {
                    function,
                    arguments: vec![local(0, Type::I64)],
                },
                ty: Type::I64,
                span: GENERATED_SPAN,
            },
            span: GENERATED_SPAN,
        });
    }
    statements.push(Statement::Let {
        id: 1,
        name: SmolStr::new("x1"),
        mutable: true,
        annotation: None,
        initializer: value(Value::I64(0), Type::I64),
    });
    let condition = binary(
        BinaryOp::CmpOp(CmpOp::Ord {
            ordering: ra_ap_syntax::ast::Ordering::Less,
            strict: true,
        }),
        local(1, Type::I64),
        value(Value::I64(iterations), Type::I64),
        Type::Bool,
    );
    let increment = Statement::Assign {
        id: 1,
        value: binary(
            BinaryOp::ArithOp(ArithOp::Add),
            local(1, Type::I64),
            value(Value::I64(1), Type::I64),
            Type::I64,
        ),
        span: GENERATED_SPAN,
    };
    statements.push(Statement::While {
        condition,
        body: Block {
            statements: vec![increment],
            tail: Some(Box::new(value(Value::Unit, Type::Unit))),
            span: GENERATED_SPAN,
        },
        span: GENERATED_SPAN,
    });
    functions.push(Function {
        name: SmolStr::new("main"),
        parameters: Vec::new(),
        return_type: Type::Unit,
        explicit_return: false,
        local_count: 2,
        body: Block {
            statements,
            tail: Some(Box::new(value(Value::Unit, Type::Unit))),
            span: GENERATED_SPAN,
        },
    });
    CheckedProgram {
        main: helper_count,
        functions,
    }
}

fn value(value: Value, ty: Type) -> Expression {
    Expression {
        kind: ExpressionKind::Value(value),
        ty,
        span: GENERATED_SPAN,
    }
}

fn local(id: usize, ty: Type) -> Expression {
    Expression {
        kind: ExpressionKind::Local(id),
        ty,
        span: GENERATED_SPAN,
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
        span: GENERATED_SPAN,
    }
}

#[cfg(test)]
mod tests {
    use crate::{Limits, RuntimeLimits, check_source, format, run};

    #[test]
    fn generated_ir_round_trips_and_terminates() {
        for seed in 0..32 {
            let program = super::generate_checked_program(&[seed, seed * 17, seed * 31]);
            let source = format(&program);
            let reparsed = check_source(&source, Limits::default()).unwrap();
            assert_eq!(source, format(&reparsed));
            run(&reparsed, RuntimeLimits::default()).unwrap();
        }
    }
}
