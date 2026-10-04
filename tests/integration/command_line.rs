//! The command-line surface: the options `--help` documents, the `bumpp` argument
//! that `cargo bumpp` forwards to the binary, and the lockfile that stays out of the
//! commit when git already ignores it.

use super::*;

#[test]
fn an_ignored_lockfile_is_not_committed() {
    let project = Project::package("ignored-lock", "0.0.2");
    project.write(".gitignore", "/target\nCargo.lock\n");
    project.commit_all("chore: initial");

    let run = project.run(&["patch", "-y", "--no-push", "--no-git-check"]);
    run.expect_success();
    let files = project.git(&["show", "--name-only", "--format=", "HEAD"]);
    assert!(files.contains("Cargo.toml"), "{files}");
    assert!(
        !files.contains("Cargo.lock"),
        "an ignored lockfile stays out: {files}"
    );
}

#[test]
fn a_help_request_prints_the_documented_options() {
    let project = Project::package("help", "0.0.2");
    let run = project.run(&["--help"]);
    run.expect_success();
    assert!(
        run.stdout.contains("cargo bumpp [level] [options]"),
        "{}",
        run.stdout
    );
    assert!(run.stdout.contains("--no-git-check"), "{}", run.stdout);
    assert!(run.stdout.contains("EXIT CODES"), "{}", run.stdout);
}

#[test]
fn the_cargo_subcommand_binary_accepts_the_extra_argument() {
    let project = Project::package("cargo-sub", "0.0.2");
    project.commit_all("chore: initial");

    // `cargo bumpp patch …` reaches the binary as `cargo-bumpp bumpp patch …`
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-bumpp"))
        .args(["bumpp", "patch", "-y", "--no-push"])
        .current_dir(&project.dir)
        .env("GIT_CONFIG_GLOBAL", &project.global_config)
        .env("GIT_CONFIG_SYSTEM", &project.system_config)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("CARGO_NET_OFFLINE", "true")
        .stdin(Stdio::null())
        .output()
        .expect("run cargo-bumpp");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(project.read("Cargo.toml").contains("0.0.3"));
}
