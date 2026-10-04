//! Tests for reading `cargo metadata`: member selection, the path dependencies
//! that stay inside the workspace, and the empty workspace. They are their own
//! file because they drive the metadata interpreter with literal JSON.

use super::*;
use crate::json::Json;

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
    let json = Json::parse(r#"{"packages": [], "workspace_members": [], "workspace_root": "/w"}"#)
        .unwrap();
    assert!(Workspace::from_metadata(&json).is_err());
}
