//! Filling a plan in: read every manifest, collect the edits, render the new
//! text. This is the decision-making half of the module, kept separate from the
//! values it hands back.

use std::collections::HashSet;
use std::path::Path;

use crate::error::{Error, Result};
use crate::git::display_path;
use crate::options::Options;
use crate::semver::{req_matches, Level, Version};
use crate::toml_line::{resolve_path, same_dir, Edit, Manifest, ISSUE_NO_VERSION};
use crate::workspace::{MemberDep, Workspace};

use super::detect::independent_versions_error;
use super::helpers::{add_edit, find_bump, key_of, parse_version};
use super::levels::increment;
use super::source::VersionSource;
use super::types::{Bump, FilePlan, Plan};

/// Build a plan for `level`.
pub fn build(workspace: &Workspace, options: &Options, level: &Level) -> Result<Plan> {
    let paths = workspace.manifests();
    let mut docs: Vec<Manifest> = Vec::with_capacity(paths.len());
    // `paths[0]` is the workspace root; the workspace already parses and caches
    // it (see `Workspace::root_parsed`), so reusing the cached copy saves a
    // second disk read after `plan::detect_current` has looked at the same
    // file. Every other manifest is read here.
    docs.push(workspace.root_parsed()?.clone());
    for path in &paths[1..] {
        docs.push(
            Manifest::read(path)
                .map_err(|err| Error::io(format!("cannot read `{}`: {err}", path.display())))?,
        );
    }
    let mut edits: Vec<Vec<Edit>> = vec![Vec::new(); paths.len()];

    let mut warnings: Vec<String> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut bumps: Vec<Bump> = Vec::new();

    let index_of = |path: &Path| paths.iter().position(|candidate| same_dir(candidate, path));

    // ---------------------------------------------------------------- source
    let old_version;
    let new_version;
    let source: VersionSource;

    let shared = docs[0].workspace_package_version();
    match shared {
        Some(found) => {
            let manifest_old = parse_version(&found.text, &paths[0])?;
            old_version = options
                .current_version
                .clone()
                .unwrap_or_else(|| manifest_old.clone());
            if old_version != manifest_old {
                notes.push(format!(
                    "--current-version {old_version} overrides the value in the manifest ({manifest_old})"
                ));
            }
            new_version = increment(&old_version, level, &options.preid)?;
            add_edit(&mut edits[0], &mut warnings, &found, &new_version);
            source = VersionSource::WorkspacePackage;

            for member in &workspace.members {
                let Some(index) = index_of(&member.manifest_path) else {
                    continue;
                };
                match docs[index].package_version() {
                    Some(literal) => {
                        let value = parse_version(&literal.text, &member.manifest_path)?;
                        if value == manifest_old {
                            add_edit(&mut edits[index], &mut warnings, &literal, &new_version);
                            bumps.push(Bump {
                                name: member.name.clone(),
                                manifest_path: member.manifest_path.clone(),
                                old: value,
                                new: new_version.clone(),
                            });
                        } else if options.recursive {
                            let bumped = increment(&value, level, &options.preid)?;
                            add_edit(&mut edits[index], &mut warnings, &literal, &bumped);
                            notes.push(format!(
                                "{} keeps its own version: {value} -> {bumped}",
                                member.name
                            ));
                            bumps.push(Bump {
                                name: member.name.clone(),
                                manifest_path: member.manifest_path.clone(),
                                old: value,
                                new: bumped,
                            });
                        } else {
                            warnings.push(format!(
                                "{} declares its own version ({value}), not the workspace version ({manifest_old}); it was left alone — pass --recursive to bump it too",
                                member.name
                            ));
                        }
                    }
                    None => {
                        // `version.workspace = true`: it follows the shared value.
                        bumps.push(Bump {
                            name: member.name.clone(),
                            manifest_path: member.manifest_path.clone(),
                            old: manifest_old.clone(),
                            new: new_version.clone(),
                        });
                    }
                }
            }
        }
        None if workspace.members.len() == 1 => {
            let member = &workspace.members[0];
            let index = index_of(&member.manifest_path)
                .ok_or_else(|| Error::check("cannot find the manifest of the only package"))?;
            let literal = docs[index].package_version().ok_or_else(|| {
                Error::check(format!(
                    "`{}` has no `[package] version` to bump",
                    display_path(&workspace.root, &member.manifest_path)
                ))
            })?;
            let manifest_old = parse_version(&literal.text, &member.manifest_path)?;
            old_version = options
                .current_version
                .clone()
                .unwrap_or_else(|| manifest_old.clone());
            new_version = increment(&old_version, level, &options.preid)?;
            add_edit(&mut edits[index], &mut warnings, &literal, &new_version);
            source = VersionSource::SinglePackage(member.name.clone());
            bumps.push(Bump {
                name: member.name.clone(),
                manifest_path: member.manifest_path.clone(),
                old: manifest_old,
                new: new_version.clone(),
            });
        }
        None if options.recursive => {
            for member in &workspace.members {
                let Some(index) = index_of(&member.manifest_path) else {
                    continue;
                };
                let Some(literal) = docs[index].package_version() else {
                    warnings.push(format!(
                        "{} declares no `[package] version` and the workspace has no shared one",
                        member.name
                    ));
                    continue;
                };
                let value = parse_version(&literal.text, &member.manifest_path)?;
                let bumped = increment(&value, level, &options.preid)?;
                add_edit(&mut edits[index], &mut warnings, &literal, &bumped);
                bumps.push(Bump {
                    name: member.name.clone(),
                    manifest_path: member.manifest_path.clone(),
                    old: value,
                    new: bumped,
                });
            }
            source = VersionSource::PerPackage;
            old_version = bumps
                .first()
                .map(|bump| bump.old.clone())
                .unwrap_or_else(zero);
            new_version = bumps
                .first()
                .map(|bump| bump.new.clone())
                .unwrap_or_else(zero);

            let first = bumps.first().map(|bump| bump.new.clone());
            if !bumps.iter().all(|bump| Some(&bump.new) == first.as_ref()) {
                let list = bumps
                    .iter()
                    .map(|bump| format!("{} {}", bump.name, bump.new))
                    .collect::<Vec<_>>()
                    .join(", ");
                if options.tag || options.commit {
                    return Err(Error::check(format!(
                        "the packages do not land on a single version ({list})"
                    ))
                    .with_hint(
                        "one commit message and one tag need one version; pass an explicit version, or run with --no-tag --no-commit",
                    ));
                }
                notes.push(format!("packages land on different versions: {list}"));
            }
        }
        None => {
            return Err(independent_versions_error(workspace));
        }
    }

    // ------------------------------------------- dependency version requirements
    let mut rewritten: HashSet<(String, String)> = HashSet::new();
    // Members whose requirement comes from the root's `[workspace.dependencies]`
    // entry rather than from their own manifest.
    let mut inherits_workspace: HashSet<(String, String)> = HashSet::new();
    for index in 0..paths.len() {
        let manifest_path = paths[index].clone();
        for decl in docs[index].dependency_decls() {
            if decl.uses_workspace {
                inherits_workspace.insert((key_of(&manifest_path), decl.name.clone()));
            }
            let Some(path_text) = decl.path.clone() else {
                continue;
            };
            let to_dir = resolve_path(&manifest_path, &path_text);
            let Some(bump) = find_bump(&bumps, &to_dir).cloned() else {
                continue;
            };
            let label = decl.section.clone();
            match &decl.version {
                Some(found) => match Version::parse(&found.text) {
                    Ok(current) if current == bump.old => {
                        edits[index].push(Edit {
                            line: found.line,
                            inner: found.inner,
                            old: found.text.clone(),
                            new: bump.new.to_string(),
                            label,
                        });
                        rewritten.insert((key_of(&manifest_path), decl.name.clone()));
                    }
                    _ => warnings.push(format!(
                        "{}: `{} = \"{}\"` does not read as the current version ({}), so it was not rewritten",
                        display_path(&workspace.root, &manifest_path),
                        decl.name,
                        found.text,
                        bump.old
                    )),
                },
                None => {
                    let issue = decl.issue.clone().unwrap_or_default();
                    if issue == ISSUE_NO_VERSION {
                        warnings.push(format!(
                            "{}: `{}` depends on {} by path but declares no version; `cargo publish` cannot rewrite it",
                            display_path(&workspace.root, &manifest_path),
                            decl.name,
                            bump.name
                        ));
                    } else {
                        warnings.push(format!(
                            "{}: cannot locate the version of `{}` ({issue})",
                            display_path(&workspace.root, &manifest_path),
                            decl.name
                        ));
                    }
                }
            }
        }
    }

    // Requirements that will not accept the new version any more.
    for MemberDep {
        to,
        rename,
        to_dir,
        req,
        manifest_path,
        ..
    } in &workspace.deps
    {
        let Some(bump) = find_bump(&bumps, to_dir) else {
            continue;
        };
        let key = rename.clone().unwrap_or_else(|| to.clone());
        let manifest_key = key_of(manifest_path);
        // `foo = { workspace = true }` takes its requirement from the root
        // manifest, so that is the entry that decides whether it still matches.
        let supplier = if inherits_workspace.contains(&(manifest_key.clone(), key.clone())) {
            key_of(&workspace.root_manifest)
        } else {
            manifest_key.clone()
        };
        if rewritten.contains(&(supplier, key.clone())) {
            continue;
        }
        if req_matches(req, &bump.new) == Some(false) {
            warnings.push(format!(
                "{}: `{key} = \"{req}\"` does not accept {}",
                display_path(&workspace.root, manifest_path),
                bump.new
            ));
        }
    }

    // ------------------------------------------------------------- rendering
    let mut files = Vec::new();
    for (index, manifest) in docs.iter().enumerate() {
        let real: Vec<Edit> = edits[index]
            .iter()
            .filter(|edit| edit.old != edit.new)
            .cloned()
            .collect();
        if real.is_empty() {
            continue;
        }
        let new_text = manifest.render_with(&real).map_err(|err| {
            Error::check(format!(
                "cannot rewrite `{}`: {err}",
                display_path(&workspace.root, &paths[index])
            ))
        })?;
        files.push(FilePlan {
            path: paths[index].clone(),
            display: display_path(&workspace.root, &paths[index]),
            edits: real,
            new_text,
        });
    }

    bumps.sort_by(|a, b| a.name.cmp(&b.name));

    let lockfile = if options.lockfile && bumps.iter().any(Bump::changed) {
        workspace.lockfile.clone()
    } else {
        None
    };

    Ok(Plan {
        root: workspace.root.clone(),
        old_version,
        new_version,
        source,
        files,
        lockfile,
        warnings,
        notes,
        bumps,
    })
}

fn zero() -> Version {
    Version::new(0, 0, 0)
}
