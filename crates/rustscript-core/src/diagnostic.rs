use line_index::LineIndex;

/// Inclusive-exclusive byte range in source text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

/// Pipeline stage that produced a diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum Phase {
    Lex,
    Parse,
    Type,
    Runtime,
}

/// Lazily computed one-based source location.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct Location {
    pub line: u32,
    pub column: u32,
}

/// Stable structured error returned by every public operation.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct Diagnostic {
    pub phase: Phase,
    pub message: String,
    pub span: Option<Span>,
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

    /// Resolve this diagnostic's start offset through the shared line index.
    pub fn location(&self, index: &LineIndex) -> Option<Location> {
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
