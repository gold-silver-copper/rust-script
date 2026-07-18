#![allow(clippy::struct_excessive_bools)]

/// Configurable frontend resource limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct Limits {
    /// Maximum input bytes accepted before UTF-8 validation.
    pub max_source_bytes: usize,
    /// Maximum `ra_ap_parser::LexedStr` tokens accepted before parsing.
    pub max_tokens: usize,
    /// Maximum delimiter nesting counted over lexer token kinds.
    pub max_delimiter_depth: usize,
    /// Maximum rust-analyzer syntax nodes plus tokens after parsing.
    pub max_syntax_elements: usize,
    /// Maximum rust-analyzer syntax tree depth and checked nesting.
    pub max_syntax_depth: usize,
    /// Maximum number of functions in a source file.
    pub max_functions: usize,
    /// Maximum parameters accepted per function.
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
