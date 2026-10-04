//! The bump flow end to end: what a plain `patch` run writes, commits, tags and
//! pushes, and the points at which it refuses to start. The rewriting itself is
//! covered elsewhere; the order of the steps and the refusals are what these check.

use super::*;

#[test]
fn patch_bumps_every_spot_and_tags_the_commit() {
    let project = Project::workspace("shared", "0.0.2");
    project.commit_all("chore: initial");

    let run = project.run(&["patch", "-y", "--no-push"]);
    run.expect_success();

    let root = project.read("Cargo.toml");
    assert_eq!(root.matches("0.0.3").count(), 3, "{root}");
    assert!(!root.contains("0.0.2"), "{root}");
    assert_eq!(
        project.read("crates/a/Cargo.toml").matches("0.0.3").count(),
        1
    );
    // the lockfile is refreshed too
    assert!(
        project.read("Cargo.lock").contains("0.0.3"),
        "{}",
        project.read("Cargo.lock")
    );

    assert_eq!(project.log_subjects()[0], "chore: release v0.0.3");
    assert!(
        project.git_ok(&["rev-parse", "--verify", "v0.0.3"]),
        "the tag exists"
    );
    assert_eq!(
        project.tag_message("v0.0.3"),
        "chore: release v0.0.3",
        "annotated, reusing the message"
    );
    assert_eq!(
        project.git(&["status", "--porcelain"]),
        "",
        "the tree is clean afterwards"
    );
    // the plan output names every spot with its line number
    let out = run.all();
    assert!(out.contains("[workspace.package]"), "{out}");
    assert!(out.contains("[workspace.dependencies]"), "{out}");
    assert!(out.contains("line "), "{out}");
}

#[test]
fn pushes_the_branch_and_only_the_tag_it_created() {
    let project = Project::workspace("push", "0.0.2");
    project.commit_all("chore: initial");
    let remote = project.bare_remote();
    // an unrelated tag that must not travel along
    project.git(&["tag", "old-tag"]);

    project.run(&["patch", "-y"]).expect_success();

    let tags = project.git(&["--git-dir", remote.to_str().unwrap(), "tag", "--list"]);
    assert!(tags.contains("v0.0.3"), "the new tag was pushed: {tags}");
    assert!(!tags.contains("old-tag"), "`--tags` was not used: {tags}");
    let branch = project.git(&[
        "--git-dir",
        remote.to_str().unwrap(),
        "log",
        "--format=%s",
        "main",
    ]);
    assert!(branch.contains("chore: release v0.0.3"), "{branch}");
}

#[test]
fn a_dirty_working_tree_is_refused_before_anything_happens() {
    let project = Project::workspace("dirty", "0.0.2");
    project.commit_all("chore: initial");
    project.write("scratch.txt", "not mine\n");

    let run = project.run(&["patch", "-y", "--no-push"]);
    run.expect_code(1);
    assert!(
        run.all().contains("Git working tree is not clean"),
        "{}",
        run.all()
    );
    assert!(
        project.read("Cargo.toml").contains("0.0.2"),
        "nothing was written"
    );
    assert!(!project.git_ok(&["rev-parse", "--verify", "v0.0.3"]));
}

#[test]
fn no_git_check_lets_a_dirty_tree_through_but_only_commits_the_bump() {
    let project = Project::workspace("dirty-ok", "0.0.2");
    project.commit_all("chore: initial");
    project.write("scratch.txt", "not mine\n");

    project
        .run(&["patch", "-y", "--no-push", "--no-git-check"])
        .expect_success();

    let files = project.git(&["show", "--name-only", "--format=", "HEAD"]);
    assert!(files.contains("Cargo.toml"), "{files}");
    assert!(
        !files.contains("scratch.txt"),
        "somebody else's file is not committed: {files}"
    );
    assert!(project.exists("scratch.txt"));
}

#[test]
fn an_existing_tag_stops_the_run_before_it_writes() {
    let project = Project::workspace("tag-exists", "0.0.2");
    project.commit_all("chore: initial");
    project.git(&["tag", "--annotate", "--message", "taken", "v0.0.3"]);

    let run = project.run(&["patch", "-y", "--no-push"]);
    run.expect_code(1);
    assert!(run.all().contains("already exists"), "{}", run.all());
    assert!(
        project.read("Cargo.toml").contains("0.0.2"),
        "nothing was written"
    );
    assert_eq!(project.log_subjects().len(), 1, "no commit was created");
}

#[test]
fn no_commit_and_no_tag_only_rewrites_files() {
    let project = Project::workspace("files-only", "0.0.2");
    project.commit_all("chore: initial");

    let run = project.run(&["patch", "-y", "--no-commit"]);
    run.expect_success();
    assert!(project.read("Cargo.toml").contains("0.0.3"));
    assert_eq!(project.log_subjects().len(), 1, "no new commit");
    assert!(
        !project.git_ok(&["rev-parse", "--verify", "v0.0.3"]),
        "no tag"
    );
    // the implied rules are explained rather than silent
    assert!(
        run.all().contains("--no-commit also disables tagging"),
        "{}",
        run.all()
    );
}
