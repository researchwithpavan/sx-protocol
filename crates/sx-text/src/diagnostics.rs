use sx_core::SxPath;

/// Parser diagnostic with source location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseDiagnostic {
    pub message: String,
    pub line: usize,
    pub column: usize,
    pub path: Option<SxPath>,
}

impl ParseDiagnostic {
    pub fn new(message: impl Into<String>, line: usize, column: usize) -> Self {
        Self {
            message: message.into(),
            line,
            column,
            path: None,
        }
    }
}
