//! Small shared helpers for building a plan: locating the bump for a directory,
//! parsing a version literal and recording an edit. They live here so the
//! builder reads as decisions rather than bookkeeping.

use std::path::Path;

use crate::error::{Error, Result};
use crate::semver::Version;
use crate::toml_line::{same_dir, Edit, Found};

use super::types::Bump;

pub(super) fn find_bump<'a>(bumps: &'a [Bump], dir: &Path) -> Option<&'a Bump> {
    bumps.iter().find(|bump| {
        bump.manifest_path
            .parent()
            .map(|parent| same_dir(parent, dir))
            .unwrap_or(false)
    })
}

pub(super) fn key_of(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/").to_lowercase()
}

pub(super) fn parse_version(text: &str, manifest: &Path) -> Result<Version> {
    Version::parse(text).map_err(|err| {
        Error::check(format!(
            "{} has an unreadable version: {err}",
            manifest.display()
        ))
    })
}

pub(super) fn add_edit(
    edits: &mut Vec<Edit>,
    warnings: &mut Vec<String>,
    found: &Found,
    new: &Version,
) {
    let value = new.to_string();
    if let Some(existing) = edits
        .iter()
        .find(|edit| edit.line == found.line && edit.inner == found.inner)
    {
        if existing.new != value {
            warnings.push(format!(
                "{}: two different versions were planned for the same spot",
                found.section
            ));
        }
        return;
    }
    edits.push(Edit {
        line: found.line,
        inner: found.inner,
        old: found.text.clone(),
        new: value,
        label: found.section.clone(),
    });
}
