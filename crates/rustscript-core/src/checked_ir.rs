use ra_ap_syntax::SmolStr;
use ra_ap_syntax::ast::{BinaryOp, UnaryOp};

use crate::Span;

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
    pub(crate) span: Span,
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
        span: Span,
    },
    While {
        condition: Expression,
        body: Block,
        span: Span,
    },
    Return {
        value: Expression,
        span: Span,
    },
    Break(Span),
    Continue(Span),
    Expression(Expression),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Expression {
    pub(crate) kind: ExpressionKind,
    pub(crate) ty: Type,
    pub(crate) span: Span,
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
