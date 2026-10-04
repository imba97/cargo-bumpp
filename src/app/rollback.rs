//! Rollback: the snapshot taken before the first write, and how to put the
//! workspace back when a run fails before the push.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::git::Git;
use crate::report::Ui;

// ---------------------------------------------------------------------------
// rollback
// ---------------------------------------------------------------------------

/// The state needed to undo a run that failed before the push.
pub(super) struct Transaction {
    pub(super) git: Git,
    snapshots: Vec<(PathBuf, Option<String>)>,
    /// The commit HEAD pointed at when the run started.
    pub(super) head: Option<String>,
    pub(super) committed: bool,
    pub(super) tag: Option<String>,
}

impl Transaction {
    pub(super) fn new(git: &Git) -> Transaction {
        Transaction {
            git: git.clone(),
            snapshots: Vec::new(),
            head: None,
            committed: false,
            tag: None,
        }
    }

    /// Remember a file's contents before it is touched. A file that does not
    /// exist yet is remembered as absent, so rollback can remove it again.
    pub(super) fn snapshot(&mut self, path: &Path) -> Result<()> {
        if self
            .snapshots
            .iter()
            .any(|(candidate, _)| candidate == path)
        {
            return Ok(());
        }
        let content = match std::fs::read_to_string(path) {
            Ok(content) => Some(content),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(err) => {
                return Err(Error::io(format!(
                    "cannot read `{}`: {err}",
                    path.display()
                )))
            }
        };
        self.snapshots.push((path.to_path_buf(), content));
        Ok(())
    }

    /// Files whose contents differ from the snapshot: what the commit should
    /// include, no matter which step changed them.
    pub(super) fn changed_paths(&self) -> Vec<PathBuf> {
        self.snapshots
            .iter()
            .filter(|(path, before)| {
                let now = std::fs::read_to_string(path).ok();
                &now != before
            })
            .map(|(path, _)| path.clone())
            .collect()
    }

    /// Put the workspace back. Returns the problems it could not fix.
    pub(super) fn rollback(&self, ui: &Ui) -> Vec<String> {
        let mut problems = Vec::new();
        if let Some(tag) = &self.tag {
            if let Err(err) = self.git.delete_tag(tag) {
                problems.push(format!("could not delete tag `{tag}`: {err}"));
            } else {
                ui.info(format!("  {} tag {tag}", ui.dim("undid ")));
            }
        }

        let mut reset = false;
        if self.committed {
            if let Some(head) = &self.head {
                match self.git.reset_hard(head) {
                    Ok(()) => {
                        reset = true;
                        ui.info(format!(
                            "  {} commit (back to {})",
                            ui.dim("undid "),
                            &head[..head.len().min(8)]
                        ));
                    }
                    Err(err) => problems.push(format!("could not reset to {head}: {err}")),
                }
            }
        }

        for (path, before) in &self.snapshots {
            // After a successful reset git has already put every tracked file
            // back — and it put it back its own way (line endings included), so
            // writing the snapshot over it would only make the tree look dirty.
            if reset && self.git.is_tracked(path) {
                continue;
            }
            match before {
                Some(content) => {
                    if std::fs::read_to_string(path).ok().as_deref() == Some(content.as_str()) {
                        continue;
                    }
                    if let Err(err) = std::fs::write(path, content) {
                        problems.push(format!("could not restore `{}`: {err}", path.display()));
                    }
                }
                None => {
                    if path.exists() {
                        if let Err(err) = std::fs::remove_file(path) {
                            problems.push(format!("could not remove `{}`: {err}", path.display()));
                        }
                    }
                }
            }
        }
        problems
    }
}
