//! The confirmation prompt and the output around it: what declining does, what
//! happens with no terminal to read from, and what `--quiet` is still allowed to
//! print.

use super::*;

#[test]
fn declining_the_confirmation_changes_nothing() {
    let project = Project::workspace("declined", "0.0.2");
    project.commit_all("chore: initial");

    // stdin is a pipe, so the tool uses the line-based confirmation
    let run = project.run_with_stdin(&["patch", "--no-push"], Some("n\n"));
    run.expect_code(130);
    assert!(project.read("Cargo.toml").contains("0.0.2"));
    assert_eq!(project.log_subjects().len(), 1);
}

#[test]
fn the_numbered_list_works_without_a_terminal() {
    let project = Project::workspace("numbered", "0.0.2");
    project.commit_all("chore: initial");

    // `1` is `major`, then the confirmation
    let run = project.run_with_stdin(&["--no-push"], Some("1\ny\n"));
    run.expect_success();
    let out = run.all();
    assert!(out.contains("Current version 0.0.2"), "{out}");
    assert!(out.contains("major 1.0.0"), "the rows are listed: {out}");
    assert!(
        project.read("Cargo.toml").contains("1.0.0"),
        "{}",
        project.read("Cargo.toml")
    );
}

#[test]
fn the_numbered_list_accepts_a_custom_version() {
    let project = Project::package("custom", "0.2.0");
    project.commit_all("chore: initial");

    // `11` is the `custom …` row, then the version to type, then the confirmation.
    // It is the same question the arrow-key selector asks, on the same line-based
    // prompt, so it is covered here: the key path cannot be driven from a test.
    let run = project.run_with_stdin(&["--no-push"], Some("11\n0.3.0\ny\n"));
    run.expect_success();
    let out = run.all();
    assert!(out.contains("custom"), "the row is listed: {out}");
    assert!(out.contains("version (current 0.2.0)"), "{out}");
    assert!(
        project.read("Cargo.toml").contains("0.3.0"),
        "{}",
        project.read("Cargo.toml")
    );
}

#[test]
fn a_closed_stdin_reports_that_it_cannot_prompt() {
    let project = Project::workspace("no-prompt", "0.0.2");
    project.commit_all("chore: initial");

    let run = project.run(&["--no-push"]);
    run.expect_code(1);
    assert!(
        run.all().contains(
            "Cannot prompt for the version number because input or output has been disabled."
        ),
        "{}",
        run.all()
    );
    assert!(project.read("Cargo.toml").contains("0.0.2"));
}

#[test]
fn quiet_prints_nothing_but_still_works() {
    let project = Project::package("quiet", "0.0.2");
    project.commit_all("chore: initial");

    let run = project.run(&["patch", "-y", "--no-push", "--quiet"]);
    run.expect_success();
    assert_eq!(run.stdout.trim(), "", "stdout: {}", run.stdout);
    assert!(project.read("Cargo.toml").contains("0.0.3"));
}
