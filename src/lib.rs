//! `cargo-bumpp` — interactive version bumping for Cargo projects.
//!
//! Version maintenance plus the git commit and tag that go with it: pick a
//! level, rewrite the manifests (and the lockfile), commit, tag, push. It
//! deliberately does **not** publish; that is CI's job, after the tag exists.
//!
//! ```no_run
//! // as a library: options from the command line, the terminal prompt as usual
//! let code = cargo_bumpp::main(vec!["patch".to_string(), "--no-push".to_string()]);
//! std::process::exit(code);
//! ```
//!
//! Nothing here depends on another crate. That is a design constraint, not an
//! accident: `cargo install cargo-bumpp` should take seconds, and the dependency
//! tree should be small enough to read.

pub mod app;
pub mod cli;
pub mod config;
pub mod error;
pub mod git;
pub mod json;
pub mod options;
pub mod plan;
pub mod prompt;
pub mod report;
pub mod semver;
pub mod sys;
pub mod tokens;
pub mod toml_line;
pub mod workspace;

pub use error::{Error, ErrorKind, Result};
pub use options::{Options, RawOptions};
pub use plan::{Plan, VersionSource};
pub use prompt::{Choice, Prompt, Selection};
pub use semver::{Level, Version};

use std::path::Path;

use report::Ui;

/// Run with the given arguments (without the program name) and the real
/// terminal, returning the process exit code.
pub fn main(args: Vec<String>) -> i32 {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(err) => {
            eprintln!("error: cannot determine the current directory: {err}");
            return error::EXIT_FAILURE;
        }
    };
    exit_code(run_in(&cwd, &args, None))
}

/// The library entry point: run in `cwd`, optionally with your own prompt.
pub fn run_in(cwd: &Path, args: &[String], prompt: Option<&mut dyn Prompt>) -> Result<()> {
    app::run(args, cwd, prompt)
}

/// Turn a result into an exit code, printing the error the way the CLI does.
pub fn exit_code(result: Result<()>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(err) => {
            let ui = Ui::new(false, sys::enable_ansi_output());
            // The hint block is indented by `Error`'s `Display`.
            eprintln!("{} {err}", ui.red("error:"));
            err.exit_code()
        }
    }
}

/// Remove a directory tree, clearing read-only flags on the way.
///
/// git writes its object files read-only, so a plain `remove_dir_all` fails on
/// Windows. Used by tests, which must not leave anything behind.
#[cfg(test)]
pub(crate) fn remove_dir_forced(path: &Path) {
    let _ = make_writable(path);
    let _ = std::fs::remove_dir_all(path);
}

#[cfg(test)]
fn make_writable(path: &Path) -> std::io::Result<()> {
    // Only Windows blocks removal on a read-only file, and the flag is cleared
    // on a throwaway directory that is about to be deleted.
    #[allow(clippy::permissions_set_readonly_false)]
    {
        let metadata = std::fs::symlink_metadata(path)?;
        let mut permissions = metadata.permissions();
        if permissions.readonly() {
            permissions.set_readonly(false);
            let _ = std::fs::set_permissions(path, permissions);
        }
        if metadata.is_dir() {
            for entry in std::fs::read_dir(path)? {
                make_writable(&entry?.path())?;
            }
        }
    }
    Ok(())
}
