use line_index::LineIndex;

/// Inclusive-exclusive byte range in source text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct Span {
    /// Start byte offset, inclusive.
    pub start: usize,
    /// End byte offset, exclusive.
    pub end: usize,
}

/// Pipeline stage that produced a diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum Phase {
    /// Byte validation or lexical policy.
    Lex,
    /// Rust parser or syntax admission.
    Parse,
    /// Subset type checking and lowering.
    Type,
    /// Deterministic interpreter execution.
    Runtime,
}

/// Lazily computed one-based source location.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct Location {
    /// One-based line number.
    pub line: u32,
    /// One-based UTF-8 column number.
    pub column: u32,
}

/// Stable structured error returned by every public operation.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct Diagnostic {
    /// Pipeline phase that produced the diagnostic.
    pub phase: Phase,
    /// Stable category-style explanation.
    pub message: String,
    /// Source byte span when the failing construct is known.
    pub span: Option<Span>,
    /// Optional display-only file name supplied by the embedding layer.
    pub file_name: Option<String>,
}

impl Diagnostic {
    pub(crate) fn new(phase: Phase, message: impl Into<String>, span: Option<Span>) -> Self {
        Self {
            phase,
            message: message.into(),
            span,
            file_name: None,
        }
    }

    /// Attach a display-only source file name.
    #[must_use]
    pub fn with_file_name(mut self, file_name: impl Into<String>) -> Self {
        self.file_name = Some(file_name.into());
        self
    }

    pub(crate) fn location(&self, index: &LineIndex) -> Option<Location> {
        let offset = u32::try_from(self.span?.start).ok()?.into();
        let line_col = index.try_line_col(offset)?;
        Some(Location {
            line: line_col.line + 1,
            column: line_col.col + 1,
        })
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.phase, self.message)
    }
}

impl std::error::Error for Diagnostic {}

pub(crate) fn span(range: ra_ap_syntax::TextRange) -> Span {
    Span {
        start: u32::from(range.start()) as usize,
        end: u32::from(range.end()) as usize,
    }
}
