//! The names the parser accepts: release levels for the `level` argument and
//! `--release`, and the boolean/long-option names, each mapped onto the field
//! of [`RawOptions`] it sets.
//!
//! It is separate so that the parser only has to decide *which* name it saw,
//! while the list of legal names and their spelling lives in one place.

use crate::error::{Error, Result};
use crate::options::RawOptions;
use crate::semver::Level;

pub(super) const LEVELS: &str = "major | minor | patch | next | conventional | conventional-prerelease | premajor | preminor | prepatch | prerelease | as-is | <version>";

pub(super) fn parse_level(text: &str) -> Result<Level> {
    Level::parse(text).ok_or_else(|| {
        Error::usage(format!("`{text}` is not a release level or a version"))
            .with_hint(format!("expected one of: {LEVELS}"))
    })
}

pub(super) fn unknown_option(option: &str) -> Error {
    Error::usage(format!("unknown option `{option}`")).with_hint("run `bumpp --help` for the list")
}

pub(super) fn is_boolean(name: &str) -> bool {
    matches!(
        name,
        "all"
            | "git-check"
            | "commit"
            | "tag"
            | "sign"
            | "push"
            | "yes"
            | "recursive"
            | "verify"
            | "ignore-scripts"
            | "print-commits"
            | "lockfile"
            | "quiet"
    )
}

pub(super) fn set_boolean(raw: &mut RawOptions, name: &str, value: bool) -> Result<()> {
    match name {
        "all" => raw.all = Some(value),
        "git-check" => raw.git_check = Some(value),
        "commit" => raw.commit = Some(value),
        "tag" => raw.tag = Some(value),
        "sign" => raw.sign = Some(value),
        "push" => raw.push = Some(value),
        "yes" => raw.yes = Some(value),
        "recursive" => raw.recursive = Some(value),
        "verify" => raw.verify = Some(value),
        "ignore-scripts" => raw.ignore_scripts = Some(value),
        "print-commits" => raw.print_commits = Some(value),
        "lockfile" => raw.lockfile = Some(value),
        "quiet" => raw.quiet = Some(value),
        other => return Err(unknown_option(&format!("--{other}"))),
    }
    Ok(())
}
