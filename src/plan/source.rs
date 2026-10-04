//! Where the version being bumped comes from: one shared workspace version, the
//! version of the only package, or one version per package under `--recursive`.
//! It is separate because the plan stores the source and the builder picks it.

/// Where the version that is being bumped comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionSource {
    /// `[workspace.package] version` — one version for the whole workspace.
    WorkspacePackage,
    /// The single package's own version.
    SinglePackage(String),
    /// Every package carries its own version (`--recursive`).
    PerPackage,
}

impl VersionSource {
    pub fn describe(&self) -> String {
        match self {
            VersionSource::WorkspacePackage => "shared version".to_string(),
            VersionSource::SinglePackage(name) => format!("package {name}"),
            VersionSource::PerPackage => "each package on its own version".to_string(),
        }
    }
}
