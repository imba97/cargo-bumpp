//! The built-in defaults every option falls back to when no source mentions it,
//! kept apart because they are the bottom of the precedence chain and the values
//! the help text and the tests quote.

/// The default commit message template.
pub const DEFAULT_COMMIT_MESSAGE: &str = "chore: release v{version}";
/// The default tag name template.
pub const DEFAULT_TAG_NAME: &str = "v{version}";
/// The default pre-release identifier.
pub const DEFAULT_PREID: &str = "beta";
/// How many recent commits `conventional` looks at by default.
pub const DEFAULT_COMMIT_WINDOW: usize = 100;
