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

/// Opaque, type-safe executable representation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckedProgram {
    pub(crate) source: String,
}
