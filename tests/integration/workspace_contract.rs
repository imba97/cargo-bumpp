//! What the run owes the workspace around it: a push that fails keeps its backup and
//! a pre-commit hook that fails rolls the files back, the command works from a member
//! directory, and the paths it prints are relative to the workspace root.

use super::*;

#[test]
fn a_backup_is_kept_when_the_push_fails() {
    let project = Project::workspace("push-fails", "0.0.2");
    // a remote that points nowhere: the push fails, the commit and tag stay
    project.commit_all("chore: initial");
    project.git(&[
        "remote",
        "add",
        "origin",
        project.path("missing.git").to_str().unwrap(),
    ]);

    let run = project.run(&["patch", "-y"]);
    assert!(!run.ok());
    let out = run.all();
    assert!(out.contains("nothing was rolled back"), "{out}");
    assert!(
        out.contains("Retry the push with:"),
        "a retry command is offered: {out}"
    );
    assert!(out.contains("git push origin main"), "{out}");
    assert!(
        project.read("Cargo.toml").contains("0.0.3"),
        "the version stays bumped"
    );
    assert_eq!(
        project.log_subjects()[0],
        "chore: release v0.0.3",
        "the commit stays"
    );
    assert!(
        project.git_ok(&["rev-parse", "--verify", "v0.0.3"]),
        "the tag stays"
    );
}

#[test]
fn cargo_bumpp_runs_from_a_member_directory() {
    let project = Project::workspace("from-member", "0.0.2");
    project.commit_all("chore: initial");

    let output = Command::new(env!("CARGO_BIN_EXE_bumpp"))
        .args(["patch", "-y", "--no-push"])
        .current_dir(project.path("crates/a"))
        .env("GIT_CONFIG_GLOBAL", &project.global_config)
        .env("GIT_CONFIG_SYSTEM", &project.system_config)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("CARGO_NET_OFFLINE", "true")
        .stdin(Stdio::null())
        .output()
        .expect("run bumpp in a member directory");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        project.read("Cargo.toml").contains("0.0.3"),
        "the workspace root was bumped"
    );
}

#[test]
fn a_failure_is_reported_with_the_workspace_untouched() {
    let project = Project::workspace("rollback", "0.0.2");
    project.commit_all("chore: initial");
    // a pre-commit hook that refuses: the failure happens after the files change
    let hook = project.path(".git/hooks/pre-commit");
    std::fs::write(&hook, "#!/bin/sh\nexit 1\n").expect("write the hook");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&hook).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&hook, permissions).unwrap();
    }

    let run = project.run(&["patch", "-y", "--no-push"]);
    assert!(!run.ok(), "{}", run.all());
    assert!(
        project.read("Cargo.toml").contains("0.0.2"),
        "the version was rolled back"
    );
    assert_eq!(project.log_subjects().len(), 1, "no commit survived");
    assert_eq!(
        project.git(&["status", "--porcelain"]),
        "",
        "the tree is clean"
    );
    assert!(!project.exists("Cargo.lock.lock"), "no stray files");
}

#[test]
fn the_lockfile_can_be_left_alone() {
    let project = Project::package("no-lockfile", "0.0.2");
    project.git(&["init"]);
    project.git(&["add", "--all"]);
    project.git(&["commit", "--message", "chore: initial"]);
    let before = project.read("Cargo.lock");

    project
        .run(&["patch", "-y", "--no-push", "--no-lockfile"])
        .expect_success();
    assert_eq!(
        project.read("Cargo.lock"),
        before,
        "--no-lockfile leaves it as it is"
    );
}

#[test]
fn relative_paths_in_the_plan_are_shown_from_the_workspace_root() {
    let project = Project::workspace("display", "0.0.2");
    project.commit_all("chore: initial");

    let run = project.run(&["patch", "-y", "--no-push"]);
    run.expect_success();
    let out = run.all();
    assert!(out.contains("  crates/a/Cargo.toml\n"), "{out}");
    // the workspace root itself is printed once, in the header
    assert_eq!(
        out.matches(project.dir.to_str().unwrap()).count(),
        1,
        "{out}"
    );
}
