//! Assembly error type for the TESSA assembler.

use std::fmt;

use thiserror::Error;

/// Location of an error within the source program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub file: usize,
    pub line: usize,
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}", self.line)
    }
}

/// An assembly error. The message is kept human-readable; spans are attached to
/// help the CLI locate the faulty statement.
#[derive(Debug, Error)]
pub struct AsmError {
    pub span: Span,
    pub msg: String,
}

impl AsmError {
    pub fn new(span: Span, msg: impl Into<String>) -> Self {
        AsmError {
            span,
            msg: msg.into(),
        }
    }
}

impl fmt::Display for AsmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.span, self.msg)
    }
}

/// Convenience result alias.
pub type Result<T> = std::result::Result<T, AsmError>;

impl From<kr1878_core::error::Error> for AsmError {
    fn from(e: kr1878_core::error::Error) -> Self {
        AsmError::new(Span { file: 0, line: 0 }, format!("core: {e}"))
    }
}
