//! The escape hatches for a git setup that fights the run: a pre-commit hook that
//! refuses, signing forced on for every commit, and the `--all` staging that goes
//! with turning the dirty-tree check off.

use super::*;

#[test]
fn no_verify_skips_a_refusing_pre_commit_hook() {
    let project = Project::package("no-verify", "0.0.2");
    project.commit_all("chore: initial");
    let hook = project.path(".git/hooks/pre-commit");
    std::fs::write(&hook, "#!/bin/sh\nexit 1\n").expect("write the hook");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&hook).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&hook, permissions).unwrap();
    }

    // the hook refuses, so the plain run fails and rolls back ...
    project.run(&["patch", "-y", "--no-push"]).expect_code(1);
    assert!(project.read("Cargo.toml").contains("0.0.2"));

    // ... and `--no-verify` is the way past it
    project
        .run(&["patch", "-y", "--no-push", "--no-verify"])
        .expect_success();
    assert_eq!(project.log_subjects()[0], "chore: release v0.0.3");
}

#[test]
fn all_commits_everything_when_the_check_is_off() {
    let project = Project::workspace("all", "0.0.2");
    project.commit_all("chore: initial");
    project.write("notes.txt", "mine too\n");

    project
        .run(&["patch", "-y", "--no-push", "--no-git-check", "--all"])
        .expect_success();

    let files = project.git(&["show", "--name-only", "--format=", "HEAD"]);
    assert!(files.contains("Cargo.toml"), "{files}");
    assert!(
        files.contains("notes.txt"),
        "--all takes everything: {files}"
    );
    assert_eq!(project.git(&["status", "--porcelain"]), "");
}

#[test]
fn no_sign_silences_a_git_config_that_signs_everything() {
    let project = Project::package("unsigned", "0.0.2");
    project.commit_all("chore: initial");
    // A developer whose git signs everything, with a signing program that does
    // not exist: a signature would fail immediately instead of opening a prompt.
    project.configure(
        "[commit]\n\tgpgsign = true\n[tag]\n\tgpgsign = true\n[gpg]\n\tprogram = definitely-not-gpg\n",
    );

    // Without `--no-sign`, git is welcome to sign — and here that fails at the
    // tag, i.e. after the commit, so the commit has to be undone as well.
    let run = project.run(&["patch", "-y", "--no-push"]);
    run.expect_code(1);
    assert!(
        project.read("Cargo.toml").contains("0.0.2"),
        "the version was rolled back"
    );
    assert_eq!(project.log_subjects().len(), 1, "so was the commit");
    assert!(
        !project.git_ok(&["rev-parse", "--verify", "v0.0.3"]),
        "no tag"
    );
    assert_eq!(project.git(&["status", "--porcelain"]), "");

    // `--no-sign` means "do not sign", even with that config in place
    project
        .run(&["patch", "-y", "--no-push", "--no-sign"])
        .expect_success();
    assert_eq!(project.log_subjects()[0], "chore: release v0.0.3");
    assert!(project.git_ok(&["rev-parse", "--verify", "v0.0.3"]));
}
