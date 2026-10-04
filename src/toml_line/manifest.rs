//! The manifest itself: reading a file into lines, asking it for the versions it
//! declares, and rendering a list of edits back to text. Its per-line entries are
//! scanned once, when the manifest is built.

use std::path::PathBuf;

use super::decls::{collect_fields, dep_section_index, found_string};
use super::entries::{new_decl, scan_entries, ScopedEntry};
use super::syntax::split_lines;
use super::types::{DepDecl, Edit, Found, KeyValue, Manifest, Value};

#[cfg(test)]
use super::decls::ISSUE_NO_VERSION;
#[cfg(test)]
use super::paths::{normalize, resolve_path, same_dir};
#[cfg(test)]
use std::path::Path;

impl Manifest {
    pub fn read(path: impl Into<PathBuf>) -> std::io::Result<Manifest> {
        let path = path.into();
        let original = std::fs::read_to_string(&path)?;
        Ok(Manifest::from_text(path, original))
    }

    pub fn from_text(path: PathBuf, original: String) -> Manifest {
        let (lines, eol, trailing_newline) = split_lines(&original);
        let entries = scan_entries(&lines);
        Manifest {
            path,
            lines,
            eol,
            trailing_newline,
            entries,
        }
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn line(&self, index: usize) -> Option<&str> {
        self.lines.get(index).map(|s| s.as_str())
    }

    /// Apply edits and return the new file contents. Every edit is verified
    /// against the current text first, so a stale plan cannot corrupt a file.
    pub fn render_with(&self, edits: &[Edit]) -> Result<String, String> {
        let mut lines = self.lines.clone();
        let mut ordered: Vec<&Edit> = edits.iter().collect();
        ordered.sort_by(|a, b| b.line.cmp(&a.line).then_with(|| b.inner.0.cmp(&a.inner.0)));
        for edit in ordered {
            let line = lines
                .get_mut(edit.line)
                .ok_or_else(|| format!("line {} does not exist", edit.line + 1))?;
            edit.apply(line)?;
        }
        Ok(self.join(&lines))
    }

    fn join(&self, lines: &[String]) -> String {
        let mut out = lines.join(&self.eol);
        if self.trailing_newline {
            out.push_str(&self.eol);
        }
        out
    }

    /// `[workspace.package] version = "..."`.
    pub fn workspace_package_version(&self) -> Option<Found> {
        self.entries().iter().find_map(|entry| {
            if entry.section_path == ["workspace", "package"] {
                found_string(&entry.item, &["version"], &entry.section)
            } else {
                None
            }
        })
    }

    /// `[package] version = "..."`, unless it is `version.workspace = true`.
    pub fn package_version(&self) -> Option<Found> {
        self.entries().iter().find_map(|entry| {
            if entry.section_path == ["package"] {
                found_string(&entry.item, &["version"], &entry.section)
            } else {
                None
            }
        })
    }

    /// Every dependency declaration in the file, across `[dependencies]`,
    /// `[dev-dependencies]`, `[build-dependencies]`, `[workspace.dependencies]`
    /// and their `[target.'cfg(..)'.dependencies]` / `[dependencies.foo]` forms.
    pub fn dependency_decls(&self) -> Vec<DepDecl> {
        let mut out = Vec::new();
        let entries = self.entries();
        let mut position = 0usize;

        while position < entries.len() {
            // All entries of one section belong together: the dotted form
            // `[dependencies.foo]` spreads one dependency over several lines.
            let section = entries[position].section.clone();
            let start = position;
            while position < entries.len() && entries[position].section == section {
                position += 1;
            }
            let group = &entries[start..position];

            let Some(index) = dep_section_index(&group[0].section_path) else {
                continue;
            };
            match group[0].section_path.get(index + 1).cloned() {
                // `[dependencies.foo]` with `version = "..."` / `path = "..."`
                Some(name) => {
                    let mut decl = new_decl(name, section, group[0].item.line);
                    let fields: Vec<KeyValue> =
                        group.iter().map(|entry| entry.item.clone()).collect();
                    collect_fields(&mut decl, &fields, true);
                    out.push(decl);
                }
                // `foo = { version = "..." }` or `foo = "1.2.3"`
                None => {
                    for entry in group {
                        let name = entry.item.key();
                        let mut decl = new_decl(name, section.clone(), entry.item.line);
                        match &entry.item.value {
                            Value::Table { entries, closed } => {
                                collect_fields(&mut decl, entries, *closed)
                            }
                            Value::Str { text, inner } => {
                                decl.version = Some(Found {
                                    text: text.clone(),
                                    line: entry.item.line,
                                    inner: *inner,
                                    section: section.clone(),
                                });
                            }
                            Value::Unsupported(reason) => decl.issue = Some(reason.clone()),
                            _ => {
                                decl.issue =
                                    Some("not a version string or an inline table".to_string())
                            }
                        }
                        out.push(decl);
                    }
                }
            }
        }
        out
    }

    /// Every `key = value` in the file, in order, with its section. Computed
    /// once at construction (see [`Manifest::from_text`]); a borrow keeps the
    /// signature the same for the three callers.
    fn entries(&self) -> &[ScopedEntry] {
        &self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(text: &str) -> Manifest {
        Manifest::from_text(PathBuf::from("Cargo.toml"), text.to_string())
    }

    const ROOT: &str = r#"# a workspace
[workspace]
members = ["crates/a", "crates/b"]
resolver = "2"

[workspace.package]
version = "0.0.2"     # the single source of truth
edition = "2021"

[workspace.dependencies]
war3-core = { path = "crates/war3-core", version = "0.0.2" }
war3-map = { version = "0.0.2", path = "crates/war3-map" }
serde = { version = "1", features = ["derive"] }

[profile.release]
lto = true
"#;

    fn edit_from(found: &Found, new: &str) -> Edit {
        Edit {
            line: found.line,
            inner: found.inner,
            old: found.text.clone(),
            new: new.to_string(),
            label: found.section.clone(),
        }
    }

    #[test]
    fn finds_the_workspace_version() {
        let found = manifest(ROOT).workspace_package_version().unwrap();
        assert_eq!(found.text, "0.0.2");
        assert_eq!(found.line, 6);
        assert_eq!(found.section, "[workspace.package]");
        let line = ROOT.lines().nth(6).unwrap();
        assert_eq!(&line[found.inner.0..found.inner.1], "0.0.2");
    }

    #[test]
    fn finds_dependency_declarations() {
        let decls = manifest(ROOT).dependency_decls();
        let names: Vec<&str> = decls.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["war3-core", "war3-map", "serde"]);
        assert_eq!(decls[0].path.as_deref(), Some("crates/war3-core"));
        assert_eq!(decls[0].version.as_ref().unwrap().text, "0.0.2");
        assert_eq!(decls[0].line, 10);
        assert_eq!(decls[1].path.as_deref(), Some("crates/war3-map"));
        assert_eq!(decls[2].path, None);
        assert!(decls.iter().all(|d| d.issue.is_none()), "{decls:?}");
    }

    #[test]
    fn applies_an_edit_and_keeps_everything_else() {
        let doc = manifest(ROOT);
        let found = doc.workspace_package_version().unwrap();
        let updated = doc.render_with(&[edit_from(&found, "0.0.3")]).unwrap();
        assert!(updated.contains(r#"version = "0.0.3"     # the single source of truth"#));
        assert!(updated.contains(r#"war3-core = { path = "crates/war3-core", version = "0.0.2" }"#));
        assert_eq!(updated.lines().count(), ROOT.lines().count());
    }

    #[test]
    fn several_edits_are_applied_consistently() {
        let doc = manifest(ROOT);
        let edits: Vec<Edit> = doc
            .dependency_decls()
            .iter()
            .filter_map(|d| d.version.as_ref())
            .map(|v| edit_from(v, "9.9.9"))
            .collect();
        // every declaration with a version is rewritten here, including `serde`:
        // the plan is what decides to leave non-member dependencies alone.
        assert_eq!(edits.len(), 3);
        let updated = doc.render_with(&edits).unwrap();
        assert_eq!(updated.matches("\"9.9.9\"").count(), 3);
        assert!(updated.contains("serde = { version = \"9.9.9\""));
    }

    #[test]
    fn a_stale_edit_is_refused() {
        let doc = manifest(ROOT);
        let edit = Edit {
            line: 6,
            inner: (11, 16),
            old: "1.1.1".to_string(),
            new: "0.0.3".to_string(),
            label: "[workspace.package]".to_string(),
        };
        assert!(doc
            .render_with(&[edit])
            .unwrap_err()
            .contains("expected `1.1.1`"));
    }

    #[test]
    fn handles_version_workspace_and_rust_version() {
        let text = r#"[package]
name = "a"
version.workspace = true
rust-version = "1.74"
edition = "2021"

[dependencies]
b = { path = "../b", version = "0.0.2" }
"#;
        let doc = manifest(text);
        assert!(doc.package_version().is_none());
        let decls = doc.dependency_decls();
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].path.as_deref(), Some("../b"));

        let edits: Vec<Edit> = decls
            .iter()
            .filter_map(|d| d.version.as_ref())
            .map(|v| edit_from(v, "0.0.3"))
            .collect();
        let updated = doc.render_with(&edits).unwrap();
        assert!(updated.contains(r#"b = { path = "../b", version = "0.0.3" }"#));
        // `rust-version` must survive untouched
        assert!(updated.contains(r#"rust-version = "1.74""#));
        assert!(updated.contains("version.workspace = true"));
    }

    #[test]
    fn reads_the_dotted_table_form() {
        let text = r#"[package]
name = "a"
version = "0.0.2"

[dependencies.foo]
path = "../foo"
version = "0.0.2"
features = ["x"]

[dev-dependencies.bar]
version = "0.0.1"
"#;
        let decls = manifest(text).dependency_decls();
        assert_eq!(decls.len(), 2);
        assert_eq!(decls[0].name, "foo");
        assert_eq!(decls[0].path.as_deref(), Some("../foo"));
        assert_eq!(decls[0].version.as_ref().unwrap().line, 6);
        assert_eq!(decls[1].name, "bar");
        assert_eq!(decls[1].section, "[dev-dependencies.bar]");
    }

    #[test]
    fn reads_target_dependencies_and_workspace_table_form() {
        let text = r#"[target.'cfg(unix)'.dependencies]
libc = { version = "0.2", path = "../libc" }

[workspace.dependencies.war3-core]
path = "crates/war3-core"
version = "0.0.2"
"#;
        let decls = manifest(text).dependency_decls();
        assert_eq!(decls.len(), 2);
        assert_eq!(decls[0].name, "libc");
        assert_eq!(decls[0].version.as_ref().unwrap().text, "0.2");
        assert_eq!(decls[1].name, "war3-core");
        assert_eq!(decls[1].version.as_ref().unwrap().text, "0.0.2");
    }

    #[test]
    fn reports_multi_line_values_instead_of_guessing() {
        let text = r#"[package]
name = "a"
version = "0.0.2"

[dependencies]
foo = {
    path = "../foo",
    version = "0.0.2",
}
bar = { path = "../bar" }
"#;
        let decls = manifest(text).dependency_decls();
        assert_eq!(decls.len(), 2, "{decls:?}");
        assert_eq!(decls[0].name, "foo");
        assert!(decls[0].version.is_none());
        assert!(
            decls[0]
                .issue
                .as_deref()
                .unwrap()
                .contains("multiple lines"),
            "{:?}",
            decls[0]
        );
        assert_eq!(
            decls[1].name, "bar",
            "the continuation lines must not become entries"
        );
        assert_eq!(decls[1].path.as_deref(), Some("../bar"));
        assert_eq!(decls[1].issue.as_deref(), Some("no `version` key"));
    }

    #[test]
    fn reads_a_version_that_the_inline_table_keeps_on_the_first_line() {
        let text = "[dependencies]\nfoo = { path = \"../foo\",\n    features = [\"x\"] }\n";
        let decls = manifest(text).dependency_decls();
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].path.as_deref(), Some("../foo"));
        assert!(decls[0].version.is_none());
        assert!(decls[0]
            .issue
            .as_deref()
            .unwrap()
            .contains("multiple lines"));
    }

    #[test]
    fn reports_a_version_on_a_continuation_line() {
        let text = "[dependencies]\nfoo = {\n    path = \"../foo\",\n    version = \"0.0.2\",\n}\n";
        let decls = manifest(text).dependency_decls();
        assert_eq!(decls.len(), 1);
        assert!(decls[0].version.is_none());
        // both the path and the version are on later lines, so neither is
        // claimed; the declaration is reported instead
        assert_eq!(decls[0].path, None);
        assert!(
            decls[0]
                .issue
                .as_deref()
                .unwrap()
                .contains("multiple lines"),
            "{:?}",
            decls[0]
        );
    }

    #[test]
    fn reads_workspace_inheritance_inline() {
        let text = "[package]\nname = \"a\"\nversion.workspace = true\n\n[dependencies]\nb = { workspace = true }\nc = { workspace = true, features = [\"x\"] }\n";
        let decls = manifest(text).dependency_decls();
        assert_eq!(decls.len(), 2, "{decls:?}");
        assert!(decls[0].uses_workspace, "{decls:?}");
        assert!(decls[1].uses_workspace, "{decls:?}");
        assert!(decls.iter().all(|decl| decl.version.is_none()));
    }

    #[test]
    fn bare_values_stop_at_the_inline_table_brace() {
        let text = "[package]\nname = \"a\"\nversion = \"0.0.2\"\n\n[dependencies]\nb = { optional = true, path = \"../b\" }\n";
        let decls = manifest(text).dependency_decls();
        assert_eq!(decls[0].path.as_deref(), Some("../b"), "{decls:?}");
        assert_eq!(decls[0].issue.as_deref(), Some(ISSUE_NO_VERSION));
    }

    #[test]
    fn shorthand_string_dependencies_are_understood() {
        let text = "[dependencies]\nfoo = \"1.2.3\"  # plain\n";
        let decls = manifest(text).dependency_decls();
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].version.as_ref().unwrap().text, "1.2.3");
        assert_eq!(decls[0].path, None);
    }

    #[test]
    fn unquoted_paths_are_recognised_as_paths() {
        // Bare paths are legal TOML; without this, `path = /usr/local/lib`
        // would be silently dropped from path-dependency discovery.
        let text = "[dependencies]\nfoo = { path = ../foo, version = \"0.0.2\" }\n";
        let decls = manifest(text).dependency_decls();
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].path.as_deref(), Some("../foo"), "{decls:?}");
        assert_eq!(decls[0].version.as_ref().unwrap().text, "0.0.2");
    }

    #[test]
    fn multi_line_arrays_are_skipped_not_parsed_as_entries() {
        let text = "[workspace]\nmembers = [\n    \"crates/a\",\n    \"crates/b\",\n]\n\n[workspace.package]\nversion = \"0.0.2\"\n";
        let doc = manifest(text);
        assert_eq!(doc.workspace_package_version().unwrap().text, "0.0.2");
        assert!(doc.dependency_decls().is_empty());
    }

    #[test]
    fn ignores_commented_out_and_other_tables() {
        let text = r#"[package]
name = "a"
version = "0.0.2"

[package.metadata.docs.rs]
all-features = true
# version = "9.9.9"
"#;
        let doc = manifest(text);
        assert_eq!(doc.package_version().unwrap().text, "0.0.2");
        assert!(doc.dependency_decls().is_empty());
    }

    #[test]
    fn preserves_crlf_and_missing_final_newline() {
        let text = "[package]\r\nname = \"a\"\r\nversion = \"0.0.2\"";
        let doc = manifest(text);
        let found = doc.package_version().unwrap();
        assert_eq!(found.line, 2);
        let updated = doc.render_with(&[edit_from(&found, "0.0.3")]).unwrap();
        assert_eq!(updated, "[package]\r\nname = \"a\"\r\nversion = \"0.0.3\"");
    }

    #[test]
    fn quoted_values_with_escapes() {
        let text = "[package]\nname = \"a\"\nversion = \"0.0.2\"\ndescription = \"say \\\"hi\\\" #notacomment\"\n";
        let doc = manifest(text);
        assert_eq!(doc.package_version().unwrap().text, "0.0.2");
        assert_eq!(doc.line_count(), 4);
    }

    #[test]
    fn literal_strings_are_read() {
        let text = "[workspace.package]\nversion = '0.0.2'\n";
        let found = manifest(text).workspace_package_version().unwrap();
        assert_eq!(found.text, "0.0.2");
        assert_eq!(
            &text.lines().nth(1).unwrap()[found.inner.0..found.inner.1],
            "0.0.2"
        );
    }

    #[test]
    fn path_helpers() {
        assert_eq!(
            resolve_path(Path::new("/w/crates/a/Cargo.toml"), "../b"),
            PathBuf::from("/w/crates/b")
        );
        assert_eq!(
            normalize(Path::new("/w/crates/./a/../b")),
            PathBuf::from("/w/crates/b")
        );
        assert!(same_dir(
            Path::new("/w/crates/a"),
            Path::new("/w/crates/./a/")
        ));
    }
}
