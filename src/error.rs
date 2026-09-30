//! Error type and exit codes.
//!
//! The exit codes are part of the interface (see the design doc):
//!
//! | code | meaning |
//! | ---- | ------- |
//! | 0    | success |
//! | 1    | a check failed; the workspace was rolled back to a clean state |
//! | 2    | usage error |
//! | 130  | cancelled at a prompt |
//! | other| git's exit code, passed through when `push` failed (local commit and tag are kept) |

use std::fmt;

/// Exit code used when a check failed and the run was rolled back.
pub const EXIT_FAILURE: i32 = 1;
/// Exit code used for usage errors.
pub const EXIT_USAGE: i32 = 2;
/// Exit code used when the user cancelled at a prompt.
pub const EXIT_CANCELLED: i32 = 130;

/// What kind of failure this is. Determines the exit code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    /// Bad command line, or a combination of options that cannot work.
    Usage,
    /// A precondition or a step failed. Whatever was done so far was rolled back.
    Check,
    /// The user cancelled at a prompt.
    Cancelled,
    /// `git push` failed after the commit and tag were created: no rollback, and
    /// git's own exit code is passed through.
    Push(i32),
    /// I/O or environment failure.
    Io,
}

/// An error with a message, an optional hint, and an exit code.
#[derive(Debug, Clone)]
pub struct Error {
    kind: ErrorKind,
    message: String,
    hint: Option<String>,
}

impl Error {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Error {
            kind,
            message: message.into(),
            hint: None,
        }
    }

    /// Usage error: exit code 2.
    pub fn usage(message: impl Into<String>) -> Self {
        Error::new(ErrorKind::Usage, message)
    }

    /// Failed check: exit code 1, after rolling back.
    pub fn check(message: impl Into<String>) -> Self {
        Error::new(ErrorKind::Check, message)
    }

    /// I/O or environment failure: exit code 1.
    pub fn io(message: impl Into<String>) -> Self {
        Error::new(ErrorKind::Io, message)
    }

    /// Cancelled at a prompt: exit code 130.
    pub fn cancelled() -> Self {
        Error::new(ErrorKind::Cancelled, "cancelled")
    }

    /// `git push` failed: git's exit code is passed through.
    pub fn push(code: i32, message: impl Into<String>) -> Self {
        Error::new(ErrorKind::Push(code), message)
    }

    /// Attach an extra block of explanatory text, printed under the message.
    /// Several hints stack, one block per line group.
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        let hint = hint.into();
        match &mut self.hint {
            Some(existing) => {
                existing.push('\n');
                existing.push_str(&hint);
            }
            None => self.hint = Some(hint),
        }
        self
    }

    pub fn kind(&self) -> &ErrorKind {
        &self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn hint(&self) -> Option<&str> {
        self.hint.as_deref()
    }

    /// The process exit code this error maps to.
    pub fn exit_code(&self) -> i32 {
        match self.kind {
            ErrorKind::Usage => EXIT_USAGE,
            ErrorKind::Check => EXIT_FAILURE,
            ErrorKind::Cancelled => EXIT_CANCELLED,
            ErrorKind::Io => EXIT_FAILURE,
            // git's exit code, passed through; 0 would be meaningless here.
            ErrorKind::Push(code) => {
                if code == 0 {
                    EXIT_FAILURE
                } else {
                    code
                }
            }
        }
    }

    /// True when the failure happened after the local commit and tag were made,
    /// i.e. nothing was rolled back on purpose.
    pub fn is_push(&self) -> bool {
        matches!(self.kind, ErrorKind::Push(_))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(hint) = &self.hint {
            for line in hint.lines() {
                write!(f, "\n  {line}")?;
            }
        }
        Ok(())
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::io(err.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes() {
        assert_eq!(Error::usage("x").exit_code(), 2);
        assert_eq!(Error::check("x").exit_code(), 1);
        assert_eq!(Error::cancelled().exit_code(), 130);
        assert_eq!(Error::push(7, "x").exit_code(), 7);
        // a "push" error with a zero status would be meaningless, keep 1
        assert_eq!(Error::push(0, "x").exit_code(), 1);
    }

    #[test]
    fn hint_is_rendered_indented() {
        let err = Error::usage("a tag must point at a commit").with_hint("line one\nline two");
        assert_eq!(
            err.to_string(),
            "a tag must point at a commit\n  line one\n  line two"
        );
    }
}
