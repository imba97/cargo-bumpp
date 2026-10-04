//! Read-only questions about the repository: whether it is one, where its root
//! is, and what the working tree, HEAD, the branch, the remotes and the tags
//! say.

use std::path::PathBuf;

use crate::error::{Error, Result};

use super::types::{Git, Output};

impl Git {
    /// Is this directory inside a git work tree?
    pub fn is_repository(&self) -> bool {
        matches!(self.run(&["rev-parse", "--is-inside-work-tree"]), Ok(output) if output.ok())
    }

    /// The repository root, or `None` outside a repository.
    pub fn root(&self) -> Option<PathBuf> {
        self.run(&["rev-parse", "--show-toplevel"])
            .ok()
            .filter(Output::ok)
            .map(|output| PathBuf::from(output.stdout.trim()))
    }

    /// `git status --porcelain`, one entry per line.
    pub fn status_porcelain(&self) -> Result<Vec<String>> {
        let text = self.checked(&["status", "--porcelain"])?;
        Ok(text.lines().map(|line| line.to_string()).collect())
    }

    /// The commit HEAD points at.
    pub fn head(&self) -> Result<String> {
        self.checked(&["rev-parse", "HEAD"])
            .map_err(|_| Error::check("this repository has no commits yet"))
    }

    /// The current branch name.
    pub fn current_branch(&self) -> Result<String> {
        self.checked(&["rev-parse", "--abbrev-ref", "HEAD"])
    }

    /// The remote to push to: `origin` when it exists, otherwise the only
    /// remote configured.
    pub fn default_remote(&self) -> Result<String> {
        let remotes: Vec<String> = self
            .checked(&["remote"])?
            .lines()
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .collect();
        match remotes.iter().find(|name| *name == "origin") {
            Some(name) => Ok(name.clone()),
            None => match remotes.len() {
                0 => Err(Error::check("no git remote is configured")
                    .with_hint("add one, or run with --no-push")),
                1 => Ok(remotes[0].clone()),
                _ => Err(Error::check(format!(
                    "several git remotes are configured ({}); none of them is `origin`",
                    remotes.join(", ")
                ))
                .with_hint("add an `origin` remote, or run with --no-push")),
            },
        }
    }

    pub fn tag_exists(&self, name: &str) -> bool {
        matches!(
            self.run(&["rev-parse", "--verify", "--quiet", &format!("refs/tags/{name}")]),
            Ok(output) if output.ok()
        )
    }
}
