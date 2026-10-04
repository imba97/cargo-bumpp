//! Everything the tool prints.
//!
//! The plan output is a feature, not decoration: it lists every spot that will
//! change, with its line number, *before* anything is written. That is what the
//! design offers instead of a dry-run mode.

mod ui;

mod render;

#[cfg(test)]
mod tests;

pub use ui::{ansi, Ui};

pub use render::{confirmation_text, print_commits, print_plan, GitSummary};
