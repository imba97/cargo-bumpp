//! The plain data of the git side: the repository handle and the raw result of
//! a command, with no process handling in between.

use std::path::PathBuf;

/// A git repository (or, before the first check, a directory that might be one).
#[derive(Debug, Clone)]
pub struct Git {
    pub cwd: PathBuf,
}

/// One recent commit, as `conventional` needs to see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub id: String,
    pub subject: String,
    pub body: String,
}

/// Raw result of running git.
#[derive(Debug, Clone)]
pub struct Output {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn ok(&self) -> bool {
        self.code == 0
    }

    /// What to report when the command failed: git's stderr, or its stdout when
    /// stderr is empty — a rejected push says so on either.
    pub fn failure_text(&self) -> String {
        if self.stderr.trim().is_empty() {
            self.stdout.trim().to_string()
        } else {
            self.stderr.trim().to_string()
        }
    }
}
