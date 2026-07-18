#![allow(clippy::struct_excessive_bools)]

/// Configurable frontend resource limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct Limits {
    pub max_source_bytes: usize,
    pub max_tokens: usize,
    pub max_delimiter_depth: usize,
    pub max_syntax_elements: usize,
    pub max_syntax_depth: usize,
    pub max_functions: usize,
    pub max_parameters: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_source_bytes: 1024 * 1024,
            max_tokens: 100_000,
            max_delimiter_depth: 256,
            max_syntax_elements: 200_000,
            max_syntax_depth: 256,
            max_functions: 1_024,
            max_parameters: 256,
        }
    }
}
