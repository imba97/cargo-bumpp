//! The version a run starts from, read without building a plan. The interactive
//! selector needs a real number before any level exists, so this asks the
//! workspace directly.

use crate::error::{Error, Result};
use crate::git::display_path;
use crate::options::Options;
use crate::semver::Version;
use crate::toml_line::Manifest;
use crate::workspace::Workspace;

use super::helpers::parse_version;

/// The version the run starts from, without building a plan.
///
/// The interactive selector has to show real version numbers, so it needs the
/// current one before a level exists.
pub fn detect_current(workspace: &Workspace, options: &Options) -> Result<Version> {
    if let Some(version) = &options.current_version {
        return Ok(version.clone());
    }
    // Reuse the cached root manifest: `plan::build` reads the same file a
    // moment later, and the cache means it only hits disk once.
    let root = workspace.root_parsed()?;
    if let Some(found) = root.workspace_package_version() {
        return parse_version(&found.text, &workspace.root_manifest);
    }
    if workspace.members.len() == 1 {
        let member = &workspace.members[0];
        let doc = Manifest::read(&member.manifest_path).map_err(|err| {
            Error::io(format!(
                "cannot read `{}`: {err}",
                member.manifest_path.display()
            ))
        })?;
        let found = doc.package_version().ok_or_else(|| {
            Error::check(format!(
                "`{}` has no `[package] version` to bump",
                display_path(&workspace.root, &member.manifest_path)
            ))
        })?;
        return parse_version(&found.text, &member.manifest_path);
    }
    if options.recursive {
        // Every package is bumped from its own version; the first one is only
        // used to render the selector.
        return Ok(workspace.members[0].version.clone());
    }
    Err(independent_versions_error(workspace))
}

/// There is no single version to bump, so nothing is guessed.
pub(super) fn independent_versions_error(workspace: &Workspace) -> Error {
    let mut listing = String::new();
    for member in &workspace.members {
        listing.push_str(&format!("\n  {} {}", member.name, member.version));
    }
    Error::check(
        "the workspace has no `[workspace.package] version`, and its packages carry independent versions",
    )
    .with_hint(format!(
        "so there is no single version to bump; found:{listing}\nadd a `[workspace.package] version` and `version.workspace = true` in the members, or pass --recursive to bump every package on its own"
    ))
}
