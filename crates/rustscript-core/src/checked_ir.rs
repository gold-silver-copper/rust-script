use ra_ap_syntax::ast::{BinaryOp, UnaryOp};
use ra_ap_syntax::{SmolStr, TextRange};

/// The complete source-level type system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum Type {
    I64,
    Bool,
    Unit,
}

/// Runtime value produced by checked expressions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum Value {
    I64(i64),
    Bool(bool),
    Unit,
}

pub(crate) type FunctionId = usize;
pub(crate) type LocalId = usize;

/// Opaque, type-safe executable representation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckedProgram {
    pub(crate) functions: Vec<Function>,
    pub(crate) main: FunctionId,
}

impl CheckedProgram {
    /// Compare executable structure while ignoring source byte ranges.
    ///
    /// This is the normalization relation used by canonical round-trip,
    /// generation, reduction, and differential tests.
    pub fn structurally_eq(&self, other: &Self) -> bool {
        let mut left = self.clone();
        let mut right = other.clone();
        left.clear_ranges();
        right.clear_ranges();
        left == right
    }

    fn clear_ranges(&mut self) {
        let empty = TextRange::empty(0.into());
        for function in &mut self.functions {
            clear_block_ranges(&mut function.body, empty);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Function {
    pub(crate) name: SmolStr,
    pub(crate) parameters: Vec<Parameter>,
    pub(crate) return_type: Type,
    pub(crate) explicit_return: bool,
    pub(crate) local_count: usize,
    pub(crate) body: Block,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Parameter {
    pub(crate) id: LocalId,
    pub(crate) name: SmolStr,
    pub(crate) ty: Type,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Block {
    pub(crate) statements: Vec<Statement>,
    pub(crate) tail: Option<Box<Expression>>,
    pub(crate) span: TextRange,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Statement {
    Let {
        id: LocalId,
        name: SmolStr,
        mutable: bool,
        annotation: Option<Type>,
        initializer: Expression,
    },
    Assign {
        id: LocalId,
        value: Expression,
        span: TextRange,
    },
    While {
        condition: Expression,
        body: Block,
        span: TextRange,
    },
    Return {
        value: Expression,
        span: TextRange,
    },
    Print {
        value: Expression,
        span: TextRange,
    },
    Break(TextRange),
    Continue(TextRange),
    Expression(Expression),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Expression {
    pub(crate) kind: ExpressionKind,
    pub(crate) ty: Type,
    pub(crate) span: TextRange,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ExpressionKind {
    Value(Value),
    Local(LocalId),
    Call {
        function: FunctionId,
        arguments: Vec<Expression>,
    },
    Unary {
        op: UnaryOp,
        operand: Box<Expression>,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<Expression>,
        rhs: Box<Expression>,
    },
    Block(Block),
    If {
        condition: Box<Expression>,
        then_branch: Block,
        else_branch: Block,
    },
}

fn clear_block_ranges(block: &mut Block, empty: TextRange) {
    block.span = empty;
    for statement in &mut block.statements {
        match statement {
            Statement::Let { initializer, .. } => clear_expression_ranges(initializer, empty),
            Statement::Assign { value, span, .. } => {
                *span = empty;
                clear_expression_ranges(value, empty);
            }
            Statement::While {
                condition,
                body,
                span,
            } => {
                *span = empty;
                clear_expression_ranges(condition, empty);
                clear_block_ranges(body, empty);
            }
            Statement::Return { value, span } | Statement::Print { value, span } => {
                *span = empty;
                clear_expression_ranges(value, empty);
            }
            Statement::Break(span) | Statement::Continue(span) => *span = empty,
            Statement::Expression(expression) => clear_expression_ranges(expression, empty),
        }
    }
    if let Some(tail) = &mut block.tail {
        clear_expression_ranges(tail, empty);
    }
}

fn clear_expression_ranges(expression: &mut Expression, empty: TextRange) {
    expression.span = empty;
    match &mut expression.kind {
        ExpressionKind::Value(_) | ExpressionKind::Local(_) => {}
        ExpressionKind::Call { arguments, .. } => {
            for argument in arguments {
                clear_expression_ranges(argument, empty);
            }
        }
        ExpressionKind::Unary { operand, .. } => clear_expression_ranges(operand, empty),
        ExpressionKind::Binary { lhs, rhs, .. } => {
            clear_expression_ranges(lhs, empty);
            clear_expression_ranges(rhs, empty);
        }
        ExpressionKind::Block(block) => clear_block_ranges(block, empty),
        ExpressionKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            clear_expression_ranges(condition, empty);
            clear_block_ranges(then_branch, empty);
            clear_block_ranges(else_branch, empty);
        }
    }
}
