//! The workspace's members and the path dependencies between them: the two
//! records `cargo metadata` is reduced to. They are their own file so the data
//! can be read on its own, away from the loading code that fills it in.

use std::path::PathBuf;

use crate::semver::Version;

/// A workspace member.
#[derive(Debug, Clone)]
pub struct Member {
    pub name: String,
    pub version: Version,
    /// Raw version text as Cargo reported it (kept for diagnostics).
    pub version_text: String,
    pub manifest_path: PathBuf,
}

/// A path dependency from one member to another.
#[derive(Debug, Clone)]
pub struct MemberDep {
    /// The package declaring the dependency.
    pub from: String,
    /// The package depended on.
    pub to: String,
    /// The key the dependency is declared under (`rename` when it is renamed).
    pub rename: Option<String>,
    /// Where the dependency points.
    pub to_dir: PathBuf,
    /// The requirement as Cargo normalised it, e.g. `^0.0.2`.
    pub req: String,
    /// The manifest that declares it.
    pub manifest_path: PathBuf,
    /// `dev` / `build` / normal.
    pub kind: Option<String>,
}
