//! Where the versions live: `cargo metadata` plus the manifests it points at.
//!
//! The manifest graph is read through `cargo metadata --no-deps --format-version
//! 1` — a subprocess, not a crate dependency. It reports the workspace root,
//! every member's version and manifest path, and every path dependency's
//! requirement, and its correctness is Cargo's own.

mod members;
mod metadata;
mod paths;
#[cfg(test)]
mod tests;

pub use members::{Member, MemberDep};

use std::cell::OnceCell;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::error::{Error, Result};
use crate::json::Json;
use crate::toml_line::Manifest;

/// The workspace as Cargo sees it.
#[derive(Debug)]
pub struct Workspace {
    pub root: PathBuf,
    /// The root manifest, `<root>/Cargo.toml`.
    pub root_manifest: PathBuf,
    /// The parsed root manifest, loaded on first request and shared by every
    /// caller (`plan::detect_current` and `plan::build` both need it, and the
    /// plan also loads every member manifest — sharing the parsed root avoids
    /// a redundant disk read).
    root_parsed: OnceCell<Manifest>,
    pub members: Vec<Member>,
    /// Path dependencies between members.
    pub deps: Vec<MemberDep>,
    /// `Cargo.lock`, when it exists.
    pub lockfile: Option<PathBuf>,
}

impl Workspace {
    /// Run `cargo metadata` in `dir` and interpret it.
    pub fn load(dir: &Path) -> Result<Workspace> {
        let output = Command::new("cargo")
            .args(["metadata", "--no-deps", "--format-version", "1"])
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|err| {
                Error::check(format!("cannot run cargo: {err}"))
                    .with_hint("cargo-bumpp must be run inside a Cargo project")
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::check(format!(
                "`cargo metadata` failed in {}:\n{}",
                dir.display(),
                stderr.trim()
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let json = Json::parse(&stdout)
            .map_err(|err| Error::io(format!("cannot read `cargo metadata` output: {err}")))?;
        Workspace::from_metadata(&json)
    }

    /// The parsed root manifest, loaded and cached on first call. Used by the
    /// plan builder so the workspace is only read from disk once.
    ///
    /// `OnceCell::get_or_try_init` is unstable on the project's MSRV (1.74),
    /// so the cache is filled manually: the first caller reads the file and
    /// inserts the result; any later caller reuses the cached value.
    pub fn root_parsed(&self) -> Result<&Manifest> {
        if let Some(existing) = self.root_parsed.get() {
            return Ok(existing);
        }
        let parsed = Manifest::read(&self.root_manifest).map_err(|err| {
            Error::io(format!(
                "cannot read `{}`: {err}",
                self.root_manifest.display()
            ))
        })?;
        let _ = self.root_parsed.set(parsed);
        // Either we just filled it, or another caller raced and did — either
        // way, `get()` now returns `Some`.
        Ok(self.root_parsed.get().expect("just initialized"))
    }
}
