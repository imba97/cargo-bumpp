//! The parse error type, kept on its own so the parser and the value type can
//! name it without depending on each other.

use std::fmt;

/// Parse error with the byte offset where it was detected.
#[derive(Debug, Clone)]
pub struct JsonError {
    pub message: String,
    pub offset: usize,
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (at byte {})", self.message, self.offset)
    }
}

impl std::error::Error for JsonError {}
