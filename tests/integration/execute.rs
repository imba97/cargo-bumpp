//! The `--execute` hook and its place in the order: after the files are written,
//! before anything is committed. A command that fails there has to leave the
//! workspace exactly as it found it.

use super::*;

#[test]
fn execute_runs_after_the_write_and_before_the_commit() {
    let project = Project::workspace("execute", "0.0.2");
    project.commit_all("chore: initial");

    // the same command line works through `sh -c` and `cmd /C`
    project
        .run(&[
            "patch",
            "-y",
            "--no-push",
            "--all",
            "-x",
            "echo generated > generated.txt",
        ])
        .expect_success();

    assert!(project.exists("generated.txt"));
    let files = project.git(&["show", "--name-only", "--format=", "HEAD"]);
    assert!(
        files.contains("generated.txt"),
        "--all picks up what --execute produced: {files}"
    );
}

#[test]
fn a_failed_execute_rolls_the_files_back() {
    let project = Project::workspace("execute-fail", "0.0.2");
    project.commit_all("chore: initial");

    let run = project.run(&["patch", "-y", "--no-push", "-x", "exit 3"]);
    run.expect_code(1);
    assert!(
        project.read("Cargo.toml").contains("0.0.2"),
        "the version was put back"
    );
    assert_eq!(project.log_subjects().len(), 1, "no commit");
    assert!(
        !project.git_ok(&["rev-parse", "--verify", "v0.0.3"]),
        "no tag"
    );
    assert_eq!(
        project.git(&["status", "--porcelain"]),
        "",
        "the tree is clean again"
    );
}

#[test]
fn a_missing_remote_never_reaches_the_write_step() {
    let project = Project::workspace("no-remote", "0.0.2");
    project.commit_all("chore: initial");

    let run = project.run(&["patch", "-y"]);
    run.expect_code(1);
    assert!(run.all().contains("no git remote"), "{}", run.all());
    assert!(
        project.read("Cargo.toml").contains("0.0.2"),
        "nothing was written"
    );
}
