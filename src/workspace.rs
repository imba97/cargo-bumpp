//! Where the versions live: `cargo metadata` plus the manifests it points at.
//!
//! The manifest graph is read through `cargo metadata --no-deps --format-version
//! 1` — a subprocess, not a crate dependency. It reports the workspace root,
//! every member's version and manifest path, and every path dependency's
//! requirement, and its correctness is Cargo's own.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::error::{Error, Result};
use crate::json::Json;
use crate::semver::Version;
use crate::toml_line::{normalize, same_dir};

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

/// The workspace as Cargo sees it.
#[derive(Debug, Clone)]
pub struct Workspace {
    pub root: PathBuf,
    /// The root manifest, `<root>/Cargo.toml`.
    pub root_manifest: PathBuf,
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

    fn from_metadata(json: &Json) -> Result<Workspace> {
        let root_text = json
            .str_at("workspace_root")
            .ok_or_else(|| Error::io("`cargo metadata` did not report a workspace root"))?;
        let root = normalize(Path::new(root_text));

        let member_ids: Vec<&str> = json
            .get("workspace_members")
            .and_then(Json::as_array)
            .map(|ids| ids.iter().filter_map(Json::as_str).collect())
            .unwrap_or_default();

        let packages = json
            .get("packages")
            .and_then(Json::as_array)
            .ok_or_else(|| Error::io("`cargo metadata` did not report any packages"))?;

        let mut members = Vec::new();
        let mut all_packages: Vec<(String, PathBuf)> = Vec::new();

        for package in packages {
            let name = package.str_at("name").unwrap_or_default().to_string();
            let id = package.str_at("id").unwrap_or_default();
            let manifest_text = package.str_at("manifest_path").unwrap_or_default();
            let manifest_path = normalize(Path::new(manifest_text));
            let version_text = package.str_at("version").unwrap_or_default().to_string();

            // `--no-deps` should already limit this to members, but filter anyway.
            if !member_ids.is_empty() && !member_ids.contains(&id) {
                continue;
            }
            let version = Version::parse(&version_text).map_err(|err| {
                Error::check(format!("package `{name}` has an unreadable version: {err}"))
            })?;
            all_packages.push((name.clone(), manifest_path.clone()));
            members.push(Member {
                name,
                version,
                version_text,
                manifest_path,
            });
        }

        if members.is_empty() {
            return Err(Error::check("the workspace has no packages"));
        }
        members.sort_by(|a, b| a.name.cmp(&b.name));

        // Path dependencies that stay inside the workspace.
        let mut deps = Vec::new();
        for package in packages {
            let from = package.str_at("name").unwrap_or_default().to_string();
            let manifest_path = normalize(Path::new(
                package.str_at("manifest_path").unwrap_or_default(),
            ));
            let Some(list) = package.get("dependencies").and_then(Json::as_array) else {
                continue;
            };
            for dep in list {
                let Some(path_text) = dep.str_at("path") else {
                    continue;
                };
                let to_dir = normalize(Path::new(path_text));
                let Some((to, _)) = all_packages.iter().find(|(_, manifest)| {
                    same_dir(manifest.parent().unwrap_or(Path::new(".")), &to_dir)
                }) else {
                    // A path dependency outside the workspace: not ours to bump.
                    continue;
                };
                deps.push(MemberDep {
                    from: from.clone(),
                    to: to.clone(),
                    rename: dep.str_at("rename").map(|name| name.to_string()),
                    to_dir: to_dir.clone(),
                    req: dep.str_at("req").unwrap_or_default().to_string(),
                    manifest_path: manifest_path.clone(),
                    kind: dep.str_at("kind").map(|kind| kind.to_string()),
                });
            }
        }

        let lockfile = {
            let candidate = root.join("Cargo.lock");
            candidate.exists().then_some(candidate)
        };

        Ok(Workspace {
            root_manifest: root.join("Cargo.toml"),
            root,
            members,
            deps,
            lockfile,
        })
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata_json() -> Json {
        Json::parse(
            r#"{
              "packages": [
                {
                  "name": "a",
                  "version": "0.0.2",
                  "id": "path+file:///w/crates/a#0.0.2",
                  "manifest_path": "/w/crates/a/Cargo.toml",
                  "dependencies": [
                    {"name": "b", "req": "^0.0.2", "path": "/w/crates/b", "kind": null},
                    {"name": "serde", "req": "^1", "path": null, "kind": null},
                    {"name": "outside", "req": "^1", "path": "/elsewhere/outside", "kind": null}
                  ]
                },
                {
                  "name": "b",
                  "version": "0.0.2",
                  "id": "path+file:///w/crates/b#0.0.2",
                  "manifest_path": "/w/crates/b/Cargo.toml",
                  "dependencies": []
                }
              ],
              "workspace_members": ["path+file:///w/crates/a#0.0.2", "path+file:///w/crates/b#0.0.2"],
              "workspace_root": "/w"
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn reads_members_and_internal_path_dependencies() {
        let workspace = Workspace::from_metadata(&metadata_json()).unwrap();
        assert_eq!(workspace.root, PathBuf::from("/w"));
        assert_eq!(workspace.root_manifest, PathBuf::from("/w/Cargo.toml"));
        let names: Vec<&str> = workspace.members.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["a", "b"]);
        assert_eq!(workspace.members[0].version.to_string(), "0.0.2");
        // only the intra-workspace path dependency is kept
        assert_eq!(workspace.deps.len(), 1);
        assert_eq!(workspace.deps[0].from, "a");
        assert_eq!(workspace.deps[0].to, "b");
        assert_eq!(workspace.deps[0].req, "^0.0.2");
        assert_eq!(
            workspace.deps[0].manifest_path,
            PathBuf::from("/w/crates/a/Cargo.toml")
        );
    }

    #[test]
    fn ignores_packages_that_are_not_members() {
        let json = Json::parse(
            r#"{
              "packages": [
                {"name": "member", "version": "1.0.0", "id": "m", "manifest_path": "/w/Cargo.toml", "dependencies": []},
                {"name": "foreign", "version": "9.9.9", "id": "f", "manifest_path": "/other/Cargo.toml", "dependencies": []}
              ],
              "workspace_members": ["m"],
              "workspace_root": "/w"
            }"#,
        )
        .unwrap();
        let workspace = Workspace::from_metadata(&json).unwrap();
        assert_eq!(workspace.members.len(), 1);
        assert_eq!(workspace.members[0].name, "member");
    }

    #[test]
    fn rejects_an_empty_workspace() {
        let json =
            Json::parse(r#"{"packages": [], "workspace_members": [], "workspace_root": "/w"}"#)
                .unwrap();
        assert!(Workspace::from_metadata(&json).is_err());
    }
}
