use crate::{CheckedProgram, Diagnostic};

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
            max_call_depth: 256,
            max_output_bytes: 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct Execution {
    pub output: Vec<u8>,
    pub steps: u64,
}

pub(crate) fn run(
    _program: &CheckedProgram,
    _limits: RuntimeLimits,
) -> Result<Execution, Diagnostic> {
    Ok(Execution {
        output: Vec::new(),
        steps: 0,
    })
}
