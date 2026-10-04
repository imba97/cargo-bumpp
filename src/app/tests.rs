//! Unit tests for the run: conventional classification, tag validation and the
//! rollback transaction, all through the helpers their own files expose.

use super::*;
use crate::git::Commit;

use super::conventional::{classify, commit_type};
use super::summary::validate_tag;

fn commit(subject: &str, body: &str) -> Commit {
    Commit {
        id: "0000000".to_string(),
        subject: subject.to_string(),
        body: body.to_string(),
    }
}

#[test]
fn conventional_classification() {
    assert_eq!(classify(&[commit("chore: x", "")]), Level::Patch);
    assert_eq!(
        classify(&[commit("fix: x", ""), commit("feat: y", "")]),
        Level::Minor
    );
    assert_eq!(
        classify(&[commit("feat: x", ""), commit("feat!: y", "")]),
        Level::Major
    );
    assert_eq!(
        classify(&[commit("feat: x", "BREAKING CHANGE: gone")]),
        Level::Major
    );
    assert_eq!(
        classify(&[commit("chore: x", "BREAKING-CHANGE: gone")]),
        Level::Major
    );
    // free-form history falls back to patch, without complaint
    assert_eq!(classify(&[commit("Hello w3wright", "")]), Level::Patch);
    assert_eq!(classify(&[]), Level::Patch);
}

#[test]
fn commit_types() {
    assert_eq!(commit_type("feat: x").as_deref(), Some("feat"));
    assert_eq!(commit_type("feat(api)!: x").as_deref(), Some("feat"));
    assert_eq!(commit_type("FIX: x").as_deref(), Some("fix"));
    assert_eq!(commit_type("no colon here"), None);
    assert_eq!(commit_type("中文提交: x"), None);
}

#[test]
fn tag_names_are_checked_early() {
    assert!(validate_tag("v1.2.3").is_ok());
    assert!(validate_tag("release/1.2.3").is_ok());
    assert!(validate_tag("").is_err());
    assert!(validate_tag("v1.2.3 ").is_err());
    assert!(validate_tag("-v1").is_err());
    assert!(validate_tag("v1..2").is_err());
    assert!(validate_tag("v1^2").is_err());
    assert!(validate_tag("v1.2.3.lock").is_err());
}

#[test]
fn transaction_reports_changed_paths() {
    let dir = std::env::temp_dir().join(format!("bumpp-tx-{}", std::process::id()));
    crate::remove_dir_forced(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("Cargo.toml");
    std::fs::write(&file, "version = \"0.0.2\"\n").unwrap();

    let git = Git::new(&dir);
    let mut tx = Transaction::new(&git);
    tx.snapshot(&file).unwrap();
    tx.snapshot(&dir.join("Cargo.lock")).unwrap();
    assert!(tx.changed_paths().is_empty());

    std::fs::write(&file, "version = \"0.0.3\"\n").unwrap();
    std::fs::write(dir.join("Cargo.lock"), "new\n").unwrap();
    let changed = tx.changed_paths();
    assert_eq!(changed.len(), 2);

    let ui = Ui::new(true, false);
    assert!(tx.rollback(&ui).is_empty());
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "version = \"0.0.2\"\n"
    );
    assert!(
        !dir.join("Cargo.lock").exists(),
        "a file that did not exist is removed again"
    );
    crate::remove_dir_forced(&dir);
}

#[test]
fn rollback_undoes_a_commit_and_a_tag_as_well_as_the_files() {
    // The nested case: when the tag step fails, undoing only the tag would
    // leave a quiet half-finished commit behind.
    let dir = std::env::temp_dir().join(format!("bumpp-tx-nested-{}", std::process::id()));
    crate::remove_dir_forced(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("Cargo.toml");

    let git = Git::new(&dir);
    let run = |args: &[&str]| {
        let output = git.run(args).unwrap();
        assert!(output.ok(), "git {args:?} failed: {}", output.stderr.trim());
        output
    };

    run(&["init", "--quiet"]);
    // Identity and signing belong in the repository's own config, not on the
    // command line: the tool runs `git tag` itself, without any `-c`, so a
    // machine (or a CI runner) with no global identity would otherwise fail
    // the tag step rather than the code under test.
    run(&["config", "user.name", "bumpp test"]);
    run(&["config", "user.email", "t@example.invalid"]);
    run(&["config", "commit.gpgsign", "false"]);
    run(&["config", "tag.gpgsign", "false"]);

    std::fs::write(&file, "version = \"0.0.2\"\n").unwrap();
    run(&["add", "--all"]);
    run(&["commit", "--quiet", "--message", "chore: initial"]);
    let head = git.head().unwrap();

    let mut tx = Transaction::new(&git);
    tx.snapshot(&file).unwrap();
    tx.head = Some(head.clone());

    // ... and then a bump that fails at the tag
    std::fs::write(&file, "version = \"0.0.3\"\n").unwrap();
    run(&["add", "--all"]);
    run(&["commit", "--quiet", "--message", "chore: release v0.0.3"]);
    tx.committed = true;
    git.tag("v0.0.3", "chore: release v0.0.3", false, true)
        .unwrap();
    tx.tag = Some("v0.0.3".to_string());
    assert_ne!(git.head().unwrap(), head);

    let ui = Ui::new(true, false);
    assert!(
        tx.rollback(&ui).is_empty(),
        "rollback has nothing to complain about"
    );
    assert_eq!(git.head().unwrap(), head, "HEAD is back where it started");
    assert!(!git.tag_exists("v0.0.3"), "the tag is gone");
    // After a reset git restored the tracked file itself; comparing through
    // git keeps this independent of line-ending normalisation.
    let restored = git
        .run(&["show", &format!("{head}:Cargo.toml")])
        .unwrap()
        .stdout;
    assert!(restored.contains("0.0.2"), "{restored}");
    assert_eq!(
        git.status_porcelain().unwrap(),
        Vec::<String>::new(),
        "the tree is clean again"
    );
    crate::remove_dir_forced(&dir);
}

#[test]
fn increment_is_not_used_directly_for_prerelease_levels() {
    // guards the helper used by the interactive rows
    let current = crate::semver::Version::parse("1.2.0").unwrap();
    assert_eq!(
        crate::plan::increment(&current, &Level::Patch, "beta")
            .unwrap()
            .to_string(),
        "1.2.1"
    );
}
