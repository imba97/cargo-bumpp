//! Running git as a subprocess: the only place that spawns a process, and the
//! helpers that expect it to succeed.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::error::{Error, Result};

use super::display::display_command;
use super::types::{Git, Output};

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
    pub(super) fn checked(&self, args: &[&str]) -> Result<String> {
        let output = self.run(args)?;
        if !output.ok() {
            return Err(self.failure(args, &output));
        }
        Ok(output.stdout.trim().to_string())
    }

    /// Run git and expect success, discarding stdout.
    pub(super) fn checked_quiet(&self, args: &[&str]) -> Result<()> {
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
}
