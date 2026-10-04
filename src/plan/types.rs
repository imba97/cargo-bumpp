//! The shape of a plan: the versions it moves between, the files it will write,
//! the packages it touches. The data is declared away from the code that fills
//! it in, so the fields can be read on their own.

use std::path::PathBuf;

use crate::semver::Version;
use crate::toml_line::Edit;

use super::source::VersionSource;

/// One file's pending edits.
#[derive(Debug, Clone)]
pub struct FilePlan {
    pub path: PathBuf,
    /// Path as shown to the user, relative to the workspace root.
    pub display: String,
    pub edits: Vec<Edit>,
    pub new_text: String,
}

/// A package that takes part in the bump.
#[derive(Debug, Clone, PartialEq)]
pub struct Bump {
    pub name: String,
    pub manifest_path: PathBuf,
    pub old: Version,
    pub new: Version,
}

impl Bump {
    pub fn changed(&self) -> bool {
        self.old != self.new
    }
}

/// The complete plan for one run.
#[derive(Debug, Clone)]
pub struct Plan {
    pub root: PathBuf,
    /// The version the run started from (the "primary" one when packages differ).
    pub old_version: Version,
    /// The version the run lands on.
    pub new_version: Version,
    pub source: VersionSource,
    pub files: Vec<FilePlan>,
    /// `Cargo.lock` to refresh with `cargo update --workspace`.
    pub lockfile: Option<PathBuf>,
    pub warnings: Vec<String>,
    pub notes: Vec<String>,
    /// Every package and its versions, for the summary.
    pub bumps: Vec<Bump>,
}

impl Plan {
    /// Every file the run will write.
    pub fn changed_files(&self) -> Vec<PathBuf> {
        self.files.iter().map(|file| file.path.clone()).collect()
    }

    /// True when nothing is written and the version does not move: `as-is` on a
    /// clean tree.
    pub fn is_noop(&self) -> bool {
        self.files.is_empty()
    }

    /// True when at least one package actually changes version.
    pub fn has_version_change(&self) -> bool {
        self.bumps.iter().any(Bump::changed)
    }
}
