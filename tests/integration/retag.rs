//! `--retag`: re-releasing a tag that is already there — the named one, and the
//! most recent one when no name is given.

use super::*;

/// A release that already happened: an annotated tag at HEAD, pushed to a bare
/// remote, exactly what a failed pipeline leaves behind.
fn released(name: &str, tag: &str) -> Project {
    let project = Project::package(name, "0.3.0");
    project.write("src/main.rs", "fn main() {}\n");
    project.commit_all("chore: initial");
    project.git(&["tag", "--annotate", "--message", "chore: release", tag]);
    project
}

#[test]
fn a_named_tag_is_re_created_at_head_and_force_pushed() {
    let project = released("retag-named", "v0.3.0");
    let remote = project.bare_remote();
    project.git(&["push", "--quiet", "origin", "refs/tags/v0.3.0"]);
    let before = project.git(&[
        "--git-dir",
        remote.to_str().unwrap(),
        "rev-parse",
        "refs/tags/v0.3.0",
    ]);

    // The release commit is no longer HEAD: the tag has somewhere to move to.
    project.write("src/main.rs", "fn main() { println!(\"hi\"); }\n");
    project.git(&["commit", "--all", "--message", "fix: greet"]);
    let head = project.git(&["rev-parse", "HEAD"]);

    let run = project.run(&["--retag", "v0.3.0", "-y"]);
    run.expect_success();

    assert_eq!(
        project.git(&["rev-parse", "v0.3.0^{commit}"]),
        head,
        "the tag was moved to HEAD"
    );
    assert_eq!(
        project.tag_message("v0.3.0"),
        "chore: release",
        "an annotated tag keeps its message"
    );
    let after = project.git(&[
        "--git-dir",
        remote.to_str().unwrap(),
        "rev-parse",
        "refs/tags/v0.3.0",
    ]);
    assert_ne!(before, after, "the remote ref really moved");
    assert_eq!(
        project.git(&[
            "--git-dir",
            remote.to_str().unwrap(),
            "rev-parse",
            "refs/tags/v0.3.0^{commit}",
        ]),
        head,
        "and it points at the commit it was moved to"
    );
    // nothing was bumped or committed
    assert_eq!(project.log_subjects()[0], "fix: greet");
    assert!(project.read("Cargo.toml").contains("0.3.0"));
    let out = run.all();
    assert!(out.contains("re-releasing moves it to HEAD"), "{out}");
    assert!(
        out.contains("push yes force refs/tags/v0.3.0"),
        "the action is restated before it runs: {out}"
    );
}

#[test]
fn without_a_name_the_most_recent_tag_is_used() {
    let project = released("retag-latest", "v0.3.0");
    project.write("src/main.rs", "fn main() { println!(\"two\"); }\n");
    project.git(&["commit", "--all", "--message", "chore: release v0.3.1"]);
    project.git(&[
        "tag",
        "--annotate",
        "--message",
        "chore: release v0.3.1",
        "v0.3.1",
    ]);
    let older = project.git(&["rev-parse", "v0.3.0"]);

    let run = project.run(&["--retag", "-y", "--no-push"]);
    run.expect_success();

    assert_eq!(
        project.git(&["rev-parse", "v0.3.1^{commit}"]),
        project.git(&["rev-parse", "HEAD"]),
        "the most recent tag was the one re-released"
    );
    assert_eq!(
        project.git(&["rev-parse", "v0.3.0"]),
        older,
        "the older one was left alone"
    );
    assert!(run.all().contains("retag  v0.3.1"), "{}", run.all());
}

#[test]
fn a_tag_already_at_head_is_re_pushed_without_moving() {
    let project = released("retag-in-place", "v0.3.0");
    let remote = project.bare_remote();
    project.git(&["push", "--quiet", "origin", "refs/tags/v0.3.0"]);

    let run = project.run(&["--retag", "v0.3.0", "-y"]);
    run.expect_success();

    let out = run.all();
    assert!(out.contains("HEAD, unchanged"), "{out}");
    // The tag is re-created, so the push has something to send. Within one second
    // the object can come out identical; then git reports up-to-date and the run
    // says so as a note instead.
    let after = project.git(&[
        "--git-dir",
        remote.to_str().unwrap(),
        "rev-parse",
        "refs/tags/v0.3.0",
    ]);
    let local = project.git(&["rev-parse", "refs/tags/v0.3.0"]);
    assert!(
        after == local || out.contains("did not change"),
        "the remote tag is the one just made, or the run explained why not: {out}"
    );
}

#[test]
fn declining_changes_nothing() {
    let project = released("retag-declined", "v0.3.0");
    let remote = project.bare_remote();
    project.git(&["push", "--quiet", "origin", "refs/tags/v0.3.0"]);
    let tag_object = project.git(&["rev-parse", "v0.3.0"]);
    let remote_before = project.git(&[
        "--git-dir",
        remote.to_str().unwrap(),
        "rev-parse",
        "refs/tags/v0.3.0",
    ]);

    // stdin is a pipe, so the confirmation is the line-based one
    let run = project.run_with_stdin(&["--retag", "v0.3.0"], Some("n\n"));
    run.expect_code(130);
    assert!(run.all().contains("Re-release?"), "{}", run.all());
    assert_eq!(
        project.git(&["rev-parse", "v0.3.0"]),
        tag_object,
        "the tag was not touched"
    );
    assert_eq!(
        project.git(&[
            "--git-dir",
            remote.to_str().unwrap(),
            "rev-parse",
            "refs/tags/v0.3.0",
        ]),
        remote_before,
        "and nothing reached the remote"
    );
}

#[test]
fn a_re_release_needs_a_tag() {
    let project = Project::package("retag-none", "0.3.0");
    project.write("src/main.rs", "fn main() {}\n");
    project.commit_all("chore: initial");

    let run = project.run(&["--retag", "-y"]);
    run.expect_code(1);
    let out = run.all();
    assert!(out.contains("there is no tag to re-release"), "{out}");
    assert!(out.contains("--retag <tag>"), "{out}");
}

#[test]
fn a_tag_that_is_not_here_is_refused() {
    let project = released("retag-missing", "v0.3.0");

    let run = project.run(&["--retag", "v9.9.9", "-y"]);
    run.expect_code(1);
    assert!(run.all().contains("does not exist here"), "{}", run.all());
}

#[test]
fn a_dirty_tree_does_not_stop_a_re_release() {
    let project = released("retag-dirty", "v0.3.0");
    project.write("scratch.txt", "not mine\n");

    // Nothing is written, committed or rolled back here, so the clean-tree rule
    // that guards a bump does not apply.
    let run = project.run(&["--retag", "v0.3.0", "-y", "--no-push"]);
    run.expect_success();
    assert_eq!(
        project.git(&["rev-parse", "v0.3.0^{commit}"]),
        project.git(&["rev-parse", "HEAD"])
    );
}

#[test]
fn a_bump_option_next_to_a_re_release_is_a_usage_error() {
    let project = released("retag-clash", "v0.3.0");

    for args in [
        vec!["--retag", "v0.3.0", "-y", "-c", "release"],
        vec!["--retag", "v0.3.0", "-y", "--recursive"],
        vec!["--retag", "--release", "minor", "-y"],
    ] {
        let run = project.run(&args);
        run.expect_code(2);
        assert!(
            run.all().contains("--retag"),
            "the error names --retag: {}",
            run.all()
        );
    }
}

#[test]
fn a_config_file_does_not_turn_a_re_release_into_an_error() {
    let project = released("retag-config", "v0.3.0");
    // The repository's own defaults mention bump options; they are not the
    // command line, so they must not make `--retag` unusable.
    project.write(
        "bumpp.toml",
        "commit = false\nlockfile = false\npreid = \"rc\"\n",
    );

    project
        .run(&["--retag", "v0.3.0", "-y", "--no-push"])
        .expect_success();
}
