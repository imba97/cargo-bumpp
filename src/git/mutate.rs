//! The operations that change the repository: staging, committing, tagging,
//! deleting a tag, resetting and pushing.

use std::path::PathBuf;

use crate::error::Result;

use super::args::{push_gpgsign_override, CommitOptions};
use super::types::{Git, Output};

impl Git {
    /// `git add --all`
    pub fn add_all(&self) -> Result<()> {
        self.checked_quiet(&["add", "--all"])
    }

    /// `git commit --allow-empty --message <msg> [files...]`
    ///
    /// `--allow-empty` is not a shortcut: `as-is` produces no file changes on
    /// purpose, and the commit is the anchor the tag needs.
    pub fn commit(&self, files: &[PathBuf], message: &str, options: CommitOptions) -> Result<()> {
        let mut args: Vec<String> = Vec::new();
        // `-c` is a git option, so it has to come before the subcommand.
        push_gpgsign_override(&mut args, "commit", options.sign, options.unsigned);
        args.extend([
            "commit".to_string(),
            "--allow-empty".to_string(),
            "--message".to_string(),
            message.to_string(),
        ]);
        if options.sign {
            args.push("--gpg-sign".into());
        }
        if !options.verify {
            args.push("--no-verify".into());
        }
        if options.all {
            args.push("--all".into());
        } else {
            for file in files {
                args.push(file.to_string_lossy().to_string());
            }
        }
        self.run_args(args)
    }

    /// `git tag --annotate --message <msg> [--sign] <name>`
    pub fn tag(&self, name: &str, message: &str, sign: bool, unsigned: bool) -> Result<()> {
        let mut args: Vec<String> = Vec::new();
        push_gpgsign_override(&mut args, "tag", sign, unsigned);
        args.extend([
            "tag".to_string(),
            "--annotate".to_string(),
            "--message".to_string(),
            message.to_string(),
        ]);
        if sign {
            args.push("--sign".into());
        }
        args.push(name.to_string());
        self.run_args(args)
    }

    pub fn delete_tag(&self, name: &str) -> Result<()> {
        self.checked_quiet(&["tag", "--delete", name])
    }

    pub fn reset_hard(&self, revision: &str) -> Result<()> {
        self.checked_quiet(&["reset", "--hard", revision])
    }

    /// `git push <remote> <refspec>`, returning the raw output so a failure can
    /// be reported verbatim with git's own exit code.
    pub fn push(&self, remote: &str, refspec: &str) -> Result<Output> {
        self.run(&["push", remote, refspec])
    }
}
