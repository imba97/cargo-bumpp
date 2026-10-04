//! Where the new version comes from: conventional commits read back to the last tag,
//! the fallback when that history is free-form, a version given verbatim, and the
//! workspaces this crate refuses to bump because the answer would be ambiguous.

use super::*;

#[test]
fn conventional_reads_the_commits_since_the_last_tag() {
    let project = Project::workspace("conventional", "0.0.2");
    project.commit_all("chore: initial");
    project.git(&["tag", "v0.0.2"]);
    project.write("crates/b/src/extra.rs", "pub fn extra() {}\n");
    project.git(&["add", "--all"]);
    project.git(&["commit", "--message", "feat: add an extra function"]);

    let run = project.run(&["conventional", "-y", "--no-push"]);
    run.expect_success();
    assert!(
        project.read("Cargo.toml").contains("0.1.0"),
        "{}",
        project.read("Cargo.toml")
    );
    assert!(
        run.all().contains("feat: add an extra function"),
        "the commits are printed"
    );
}

#[test]
fn conventional_falls_back_to_patch_on_free_form_history() {
    let project = Project::workspace("conventional-fallback", "0.0.2");
    project.commit_all("Hello w3wright");

    let run = project.run(&["conventional", "-y", "--no-push"]);
    run.expect_success();
    assert!(
        project.read("Cargo.toml").contains("0.0.3"),
        "{}",
        project.read("Cargo.toml")
    );
}

#[test]
fn an_explicit_version_is_used_as_given() {
    let project = Project::package("explicit", "0.0.2");
    project.commit_all("chore: initial");

    project.run(&["1.2.3", "-y", "--no-push"]).expect_success();
    assert!(project.read("Cargo.toml").contains("version = \"1.2.3\""));
    assert_eq!(project.log_subjects()[0], "chore: release v1.2.3");
}

#[test]
fn as_is_creates_an_empty_commit_because_the_tag_needs_somewhere_to_point() {
    let project = Project::package("as-is", "0.0.2");
    project.commit_all("chore: initial");

    let run = project.run(&["as-is", "-y", "--no-push"]);
    run.expect_success();
    assert!(
        project.read("Cargo.toml").contains("0.0.2"),
        "the version is untouched"
    );
    assert_eq!(
        project.log_subjects().len(),
        2,
        "an empty commit: {}",
        project.log_subjects().join("|")
    );
    assert!(project.git_ok(&["rev-parse", "--verify", "v0.0.2"]));
    assert!(run.all().contains("no file changes"), "{}", run.all());
}

#[test]
fn pre_release_levels_use_the_preid_and_start_at_one() {
    let project = Project::package("prerelease", "0.0.2");
    project.commit_all("chore: initial");

    project
        .run(&["--release", "prepatch", "--preid", "rc", "-y", "--no-push"])
        .expect_success();
    assert!(project
        .read("Cargo.toml")
        .contains("version = \"0.0.3-rc.1\""));

    project.run(&["next", "-y", "--no-push"]).expect_success();
    assert!(
        project
            .read("Cargo.toml")
            .contains("version = \"0.0.3-rc.2\""),
        "the identifier is kept and the counter moves: {}",
        project.read("Cargo.toml")
    );
}

#[test]
fn a_workspace_with_independent_versions_is_refused() {
    let project = Project::new("independent");
    project.write(
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/a\", \"crates/b\"]\nresolver = \"2\"\n",
    );
    project.write(
        "crates/a/Cargo.toml",
        "[package]\nname = \"a\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    project.write("crates/a/src/lib.rs", "");
    project.write(
        "crates/b/Cargo.toml",
        "[package]\nname = \"b\"\nversion = \"0.2.0\"\nedition = \"2021\"\n",
    );
    project.write("crates/b/src/lib.rs", "");
    project.commit_all("chore: initial");

    let run = project.run(&["patch", "-y", "--no-push"]);
    run.expect_code(1);
    let out = run.all();
    assert!(out.contains("no `[workspace.package] version`"), "{out}");
    assert!(
        out.contains("a 0.1.0") && out.contains("b 0.2.0"),
        "the members are listed: {out}"
    );

    // --recursive is the way out, and then every package moves on its own
    project
        .run(&[
            "patch",
            "-y",
            "--no-push",
            "--recursive",
            "--no-tag",
            "--no-commit",
        ])
        .expect_success();
    assert!(project.read("crates/a/Cargo.toml").contains("0.1.1"));
    assert!(project.read("crates/b/Cargo.toml").contains("0.2.1"));
}
