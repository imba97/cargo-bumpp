//! The git side: every command the tool runs, and nothing else.
//!
//! git is a subprocess, not a library: no `git2`, no `openssl`, no vendored C to
//! build. That is the difference between an install measured in seconds and one
//! measured in minutes.

mod types;

mod process;

mod query;

mod mutate;

mod history;

mod worktree;

mod args;

mod display;

#[cfg(test)]
mod tests;

pub use args::CommitOptions;
pub use display::{display_command, display_path};
pub use types::{Commit, Git, Output};
