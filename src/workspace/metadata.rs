//! Interpreting the JSON `cargo metadata` prints: the workspace root, the
//! members, and the path dependencies that stay inside the workspace. It is its
//! own file because this is where the reported graph is turned into our types.

use std::cell::OnceCell;
use std::path::{Path, PathBuf};

use super::{Member, MemberDep, Workspace};
use crate::error::{Error, Result};
use crate::json::Json;
use crate::semver::Version;
use crate::toml_line::{normalize, same_dir};

impl Workspace {
    pub(super) fn from_metadata(json: &Json) -> Result<Workspace> {
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
            let index = package.index();
            let lookup =
                |key: &str| -> Option<&str> { index.as_ref()?.get(key).and_then(|v| v.as_str()) };
            let name = lookup("name").unwrap_or_default().to_string();
            let id = lookup("id").unwrap_or_default();
            let manifest_text = lookup("manifest_path").unwrap_or_default();
            let manifest_path = normalize(Path::new(manifest_text));
            let version_text = lookup("version").unwrap_or_default().to_string();

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
            let index = package.index();
            let lookup =
                |key: &str| -> Option<&str> { index.as_ref()?.get(key).and_then(|v| v.as_str()) };
            let from = lookup("name").unwrap_or_default().to_string();
            let manifest_path = normalize(Path::new(lookup("manifest_path").unwrap_or_default()));
            let lookup_array = |key: &str| -> Option<&[Json]> {
                index.as_ref()?.get(key).and_then(|v| v.as_array())
            };
            let Some(list) = lookup_array("dependencies") else {
                continue;
            };
            for dep in list {
                let dep_index = dep.index();
                let lookup = |k: &str| -> Option<&str> {
                    dep_index.as_ref()?.get(k).and_then(|v| v.as_str())
                };
                let Some(path_text) = lookup("path") else {
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
                    rename: lookup("rename").map(str::to_string),
                    to_dir: to_dir.clone(),
                    req: lookup("req").unwrap_or_default().to_string(),
                    manifest_path: manifest_path.clone(),
                    kind: lookup("kind").map(str::to_string),
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
            root_parsed: OnceCell::new(),
            members,
            deps,
            lockfile,
        })
    }
}
