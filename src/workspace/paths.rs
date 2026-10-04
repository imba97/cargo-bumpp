//! Looking members up and listing the manifests a run touches. It is its own
//! file because these are pure path questions, asked by the plan builder and
//! the detection of the current version alike.

use std::path::{Path, PathBuf};

use super::{Member, Workspace};
use crate::toml_line::same_dir;

impl Workspace {
    pub fn member(&self, name: &str) -> Option<&Member> {
        self.members.iter().find(|member| member.name == name)
    }

    /// The member whose manifest is `path`.
    pub fn member_by_manifest(&self, path: &Path) -> Option<&Member> {
        self.members
            .iter()
            .find(|member| same_dir(&member.manifest_path, path))
    }

    /// Distinct manifest paths, root manifest first.
    pub fn manifests(&self) -> Vec<PathBuf> {
        let mut paths = vec![self.root_manifest.clone()];
        for member in &self.members {
            if !paths
                .iter()
                .any(|path| same_dir(path, &member.manifest_path))
            {
                paths.push(member.manifest_path.clone());
            }
        }
        paths
    }
}
