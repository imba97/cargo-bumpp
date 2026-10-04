//! Tests for the git side: how a command line and a path are rendered, and that
//! every query fails softly outside a repository.

use std::path::Path;

use super::*;

#[test]
fn displays_commands_readably() {
    assert_eq!(
        display_command(&["commit", "--message", "a b"]),
        "commit --message \"a b\""
    );
    assert_eq!(
        display_command(&["push", "origin", "v1.0.0"]),
        "push origin v1.0.0"
    );
}

#[test]
fn relative_paths_for_display() {
    let root = Path::new("/w");
    assert_eq!(
        display_path(root, Path::new("/w/crates/a/Cargo.toml")),
        "crates/a/Cargo.toml"
    );
    assert_eq!(
        display_path(root, Path::new("/other/Cargo.toml")),
        "/other/Cargo.toml"
    );
}

#[test]
fn outside_a_repository_everything_fails_softly() {
    let dir = std::env::temp_dir().join(format!("bumpp-git-test-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let git = Git::new(&dir);
    if !git.is_repository() {
        assert!(git.root().is_none());
        assert!(git.status_porcelain().is_err());
    }
    crate::remove_dir_forced(&dir);
}
