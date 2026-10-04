//! The `bumpp.toml` file and the environment variables that stand in for flags, and
//! which of the three sources wins. A key that is not recognised has to be an error
//! rather than a silently ignored one.

use super::*;

#[test]
fn the_config_file_is_read_and_the_command_line_wins() {
    let project = Project::workspace("config", "0.0.2");
    project.write(
        "bumpp.toml",
        "commit-message = \"release: {version}\"\npush = false\n",
    );
    project.commit_all("chore: initial");

    project.run(&["patch", "-y"]).expect_success();
    assert_eq!(project.log_subjects()[0], "release: 0.0.3");

    // the command line overrides the file
    project.write("bumpp.toml", "commit-message = \"from file {version}\"\n");
    project.git(&["add", "--all"]);
    project.git(&["commit", "--message", "chore: another config"]);
    project
        .run(&["patch", "-y", "--commit", "from cli {version}", "--no-push"])
        .expect_success();
    assert_eq!(project.log_subjects()[0], "from cli 0.0.4");
}

#[test]
fn environment_variables_beat_the_config_file() {
    let project = Project::workspace("env", "0.0.2");
    project.write("bumpp.toml", "commit-message = \"from file {version}\"\n");
    project.commit_all("chore: initial");

    let mut command = Command::new(env!("CARGO_BIN_EXE_bumpp"));
    let output = command
        .args(["patch", "-y", "--no-push"])
        .current_dir(&project.dir)
        .env("GIT_CONFIG_GLOBAL", &project.global_config)
        .env("GIT_CONFIG_SYSTEM", &project.system_config)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("CARGO_NET_OFFLINE", "true")
        .env("BUMPP_COMMIT_MESSAGE", "from env {version}")
        .stdin(Stdio::null())
        .output()
        .expect("run bumpp");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(project.log_subjects()[0], "from env 0.0.3");
}

#[test]
fn an_unknown_config_key_is_a_usage_error() {
    let project = Project::package("bad-config", "0.0.2");
    project.write("bumpp.toml", "commti-message = \"typo\"\n");
    project.commit_all("chore: initial");

    let run = project.run(&["patch", "-y", "--no-push"]);
    run.expect_code(2);
    let out = run.all();
    assert!(out.contains("unknown key `commti-message`"), "{out}");
    assert!(
        out.contains("commit-message"),
        "the known keys are listed: {out}"
    );
}
