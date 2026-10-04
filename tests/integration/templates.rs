//! The commit and tag templates, and the `{date}` token they can carry. Rendering
//! happens once per run, so the date in the message is the date the tag gets.

use super::*;

#[test]
fn templates_are_rendered_for_the_commit_and_the_tag() {
    let project = Project::workspace("templates", "0.0.2");
    project.commit_all("chore: initial");

    project
        .run(&[
            "patch",
            "-y",
            "--no-push",
            "--tag",
            "release-{version}",
            "--commit",
            "chore: {tag} from {oldVersion}",
        ])
        .expect_success();
    assert_eq!(project.log_subjects()[0], "chore: release-0.0.3 from 0.0.2");
    assert!(project.git_ok(&["rev-parse", "--verify", "release-0.0.3"]));
}

#[test]
fn the_date_token_is_the_local_date() {
    let project = Project::package("date-token", "0.0.2");
    project.commit_all("chore: initial");

    project
        .run(&[
            "patch",
            "-y",
            "--no-push",
            "--commit",
            "release {version} ({date})",
        ])
        .expect_success();
    let subject = project.log_subjects()[0].clone();
    let (year, month, day) = cargo_bumpp::sys::local_date();
    assert_eq!(
        subject,
        format!("release 0.0.3 ({year:04}-{month:02}-{day:02})")
    );
}
