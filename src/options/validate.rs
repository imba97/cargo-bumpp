//! The checks an option set has to pass: a usable `--preid`, and the options that
//! cannot be combined with `--retag`. It is its own file because these have rules
//! of their own and `resolve` is long enough without them.

use crate::error::{Error, Result};

use super::RawOptions;

/// Check the command line *as typed*, before the environment and `bumpp.toml` are
/// merged in.
///
/// What is checked here is what only the person typing can mean: `--retag` makes
/// the options that describe a bump meaningless, and silently ignoring an explicit
/// `-c <message>` would look like it had been applied. A repository's defaults are
/// not an error — a `bumpp.toml` that turns the lockfile off must not break a
/// re-release — which is why this runs before the merge.
pub fn validate_cli(raw: &RawOptions) -> Result<()> {
    if raw.retag != Some(true) {
        return Ok(());
    }
    let mut bump_only: Vec<&str> = Vec::new();
    let mut flag = |given: bool, name: &'static str| {
        if given {
            bump_only.push(name);
        }
    };
    flag(
        raw.commit.is_some() || raw.commit_message.is_some(),
        "--commit",
    );
    flag(raw.tag.is_some() || raw.tag_name.is_some(), "--tag");
    flag(raw.all.is_some(), "--all");
    flag(raw.recursive.is_some(), "--recursive");
    flag(raw.execute.is_some(), "--execute");
    flag(raw.preid.is_some(), "--preid");
    flag(raw.current_version.is_some(), "--current-version");
    flag(raw.commit_window.is_some(), "--commit-window");
    flag(raw.lockfile.is_some(), "--lockfile");
    flag(raw.print_commits.is_some(), "--print-commits");
    flag(raw.verify.is_some(), "--verify");
    flag(raw.ignore_scripts.is_some(), "--ignore-scripts");

    if bump_only.is_empty() {
        return Ok(());
    }
    Err(Error::usage(format!(
        "--retag cannot be combined with {}",
        bump_only.join(", ")
    ))
    .with_hint("a re-release re-creates the tag and pushes it; nothing is bumped or committed"))
}

pub(super) fn validate_preid(preid: &str) -> Result<()> {
    if preid.is_empty() {
        return Err(Error::usage("--preid cannot be empty"));
    }
    if !preid
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(Error::usage(format!(
            "--preid `{preid}` is not a valid pre-release identifier (letters, digits and `-` only)"
        )));
    }
    if preid.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Error::usage(format!(
            "--preid `{preid}` must not be numeric-only: it would be read as a pre-release counter"
        )));
    }
    Ok(())
}
