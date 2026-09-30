//! The git side: every command the tool runs, and nothing else.
//!
//! git is a subprocess, not a library: no `git2`, no `openssl`, no vendored C to
//! build. That is the difference between an install measured in seconds and one
//! measured in minutes.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::error::{Error, Result};

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
}

impl Git {
    pub fn new(cwd: impl Into<PathBuf>) -> Git {
        Git { cwd: cwd.into() }
    }

    /// Run git, capturing its output. stdin is inherited so that credential or
    /// passphrase prompts still reach the user.
    pub fn run(&self, args: &[&str]) -> Result<Output> {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.cwd)
            .stdin(Stdio::inherit())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|err| {
                Error::io(format!("cannot run git: {err}"))
                    .with_hint("is git installed and on PATH?")
            })?;
        Ok(Output {
            code: output.status.code().unwrap_or(1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }

    /// Run git and expect success, with stdout returned trimmed.
    fn checked(&self, args: &[&str]) -> Result<String> {
        let output = self.run(args)?;
        if !output.ok() {
            return Err(self.failure(args, &output));
        }
        Ok(output.stdout.trim().to_string())
    }

    /// Run git and expect success, discarding stdout.
    fn checked_quiet(&self, args: &[&str]) -> Result<()> {
        self.checked(args).map(|_| ())
    }

    fn failure(&self, args: &[&str], output: &Output) -> Error {
        let mut message = format!("`git {}` failed", display_command(args));
        let detail = output.stderr.trim();
        if !detail.is_empty() {
            message.push_str(":\n");
            message.push_str(detail);
        } else if output.code != 0 {
            message.push_str(&format!(" (exit code {})", output.code));
        }
        Error::check(message)
    }

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
        if !options.sign && options.unsigned {
            args.push("-c".into());
            args.push("commit.gpgsign=false".into());
        }
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
        let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        self.checked_quiet(&refs)
    }

    /// `git tag --annotate --message <msg> [--sign] <name>`
    pub fn tag(&self, name: &str, message: &str, sign: bool, unsigned: bool) -> Result<()> {
        let mut args: Vec<String> = Vec::new();
        if !sign && unsigned {
            args.push("-c".into());
            args.push("tag.gpgsign=false".into());
        }
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
        let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        self.checked_quiet(&refs)
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

    /// The most recent tag reachable from HEAD, if there is one.
    pub fn last_tag(&self) -> Result<Option<String>> {
        match self.run(&["describe", "--tags", "--abbrev=0"]) {
            Ok(output) if output.ok() => {
                let tag = output.stdout.trim().to_string();
                Ok((!tag.is_empty()).then_some(tag))
            }
            Ok(_) => Ok(None),
            Err(err) => Err(err),
        }
    }

    /// Commits in `range` (e.g. `v1.0.0..HEAD` or `HEAD`), newest first.
    ///
    /// `limit` caps how many are read; 0 means no cap.
    pub fn commits(&self, range: &str, limit: usize) -> Result<Vec<Commit>> {
        let mut args: Vec<String> = vec!["log".to_string()];
        if limit > 0 {
            args.push(format!("-{limit}"));
        }
        args.push("--format=%H%x1f%s%x1f%b%x1e".to_string());
        args.push(range.to_string());
        let refs: Vec<&str> = args.iter().map(|arg| arg.as_str()).collect();
        let text = self.checked(&refs)?;

        let mut commits = Vec::new();
        for record in text.split('\u{1e}') {
            let record = record.trim_matches('\n');
            if record.is_empty() {
                continue;
            }
            let mut fields = record.splitn(3, '\u{1f}');
            let id = fields.next().unwrap_or_default().trim().to_string();
            let subject = fields.next().unwrap_or_default().to_string();
            let body = fields.next().unwrap_or_default().to_string();
            commits.push(Commit { id, subject, body });
        }
        Ok(commits)
    }

    /// True when the path exists but must not be committed: untracked *and*
    /// matched by a `.gitignore` rule. A tracked file is never skipped, even if
    /// a rule matches it.
    pub fn changed_but_ignored(&self, path: &Path) -> bool {
        if self.is_tracked(path) {
            return false;
        }
        let text = path.to_string_lossy().to_string();
        matches!(
            self.run(&["check-ignore", "--quiet", "--", &text]),
            Ok(output) if output.ok()
        )
    }

    /// True when git already tracks the path, i.e. a checkout can restore it.
    pub fn is_tracked(&self, path: &Path) -> bool {
        let text = path.to_string_lossy().to_string();
        matches!(
            self.run(&["ls-files", "--error-unmatch", "--", &text]),
            Ok(output) if output.ok()
        )
    }

    /// The most recent commits, newest first.
    pub fn recent_commits(&self, window: usize) -> Result<Vec<Commit>> {
        self.commits("HEAD", window)
    }
}

/// Knobs for [`Git::commit`].
#[derive(Debug, Clone, Copy)]
pub struct CommitOptions {
    /// `--sign`: sign this commit.
    pub sign: bool,
    /// `--no-sign` was given: do not sign, even when git is configured to sign
    /// everything.
    pub unsigned: bool,
    pub verify: bool,
    pub all: bool,
}

/// Render a command line for a message, quoting what needs it.
pub fn display_command(args: &[&str]) -> String {
    args.iter()
        .map(|arg| {
            if arg.is_empty() || arg.contains([' ', '\t', '\n', '"', '\'']) {
                format!("\"{}\"", arg.replace('"', "\\\""))
            } else {
                arg.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The path shown to the user: relative to the repository when possible.
pub fn display_path(repo_root: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(repo_root).unwrap_or(path);
    relative.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_commands_readably() {
        assert_eq!(
            display_command(&["commit", "--message", "a b"]),
            "commit --message \"a b\""
        );
        assert_eq!(
            display_command(&["push", "origin", "v1.0.0"]),
            "push origin v1.0.0"
        );
    }

    #[test]
    fn relative_paths_for_display() {
        let root = Path::new("/w");
        assert_eq!(
            display_path(root, Path::new("/w/crates/a/Cargo.toml")),
            "crates/a/Cargo.toml"
        );
        assert_eq!(
            display_path(root, Path::new("/other/Cargo.toml")),
            "/other/Cargo.toml"
        );
    }

    #[test]
    fn outside_a_repository_everything_fails_softly() {
        let dir = std::env::temp_dir().join(format!("bumpp-git-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let git = Git::new(&dir);
        if !git.is_repository() {
            assert!(git.root().is_none());
            assert!(git.status_porcelain().is_err());
        }
        crate::remove_dir_forced(&dir);
    }
}
