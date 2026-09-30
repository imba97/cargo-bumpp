//! End-to-end tests: a real git repository, a real Cargo workspace, the real
//! binary. `cargo metadata` and `git` are subprocesses, so these cover the parts
//! unit tests cannot.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A path next to `dir`, sharing its name: used for the throwaway git config and
/// the throwaway bare remote.
fn sibling(dir: &Path, suffix: &str) -> PathBuf {
    let name = dir
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    dir.with_file_name(format!("{name}-{suffix}"))
}

/// A throwaway Cargo + git project.
struct Project {
    dir: PathBuf,
    global_config: PathBuf,
    system_config: PathBuf,
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Run {
    fn ok(&self) -> bool {
        self.code == 0
    }

    fn all(&self) -> String {
        format!("{}\n{}", self.stdout, self.stderr)
    }

    #[track_caller]
    fn expect_success(&self) -> &Run {
        assert!(
            self.ok(),
            "expected success, got exit {}\n{}",
            self.code,
            self.all()
        );
        self
    }

    #[track_caller]
    fn expect_code(&self, code: i32) -> &Run {
        assert_eq!(self.code, code, "expected exit {code}\n{}", self.all());
        self
    }
}

impl Project {
    fn new(name: &str) -> Project {
        let unique = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "cargo-bumpp-it-{}-{unique}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the test project");

        // The git configuration lives *next to* the project, never inside it:
        // a file in the working tree would show up as a change of its own.
        let global_config = sibling(&dir, "gitconfig");
        let system_config = sibling(&dir, "gitconfig-system");
        std::fs::write(
            &global_config,
            "[user]\n\tname = bumpp test\n\temail = bumpp@example.invalid\n\
             [init]\n\tdefaultBranch = main\n\
             [commit]\n\tgpgsign = false\n\
             [tag]\n\tgpgsign = false\n\
             [core]\n\tautocrlf = false\n\
             [advice]\n\tdetachedHead = false\n",
        )
        .expect("write the git config");
        std::fs::write(&system_config, "").expect("write the empty system config");

        Project {
            dir,
            global_config,
            system_config,
        }
    }

    /// A workspace whose six-ish spots all carry one shared version.
    fn workspace(name: &str, version: &str) -> Project {
        let project = Project::new(name);
        project.write(
            "Cargo.toml",
            &format!(
                "[workspace]\nmembers = [\"crates/a\", \"crates/b\"]\nresolver = \"2\"\n\n\
                 [workspace.package]\nversion = \"{version}\"\nedition = \"2021\"\n\n\
                 [workspace.dependencies]\na = {{ path = \"crates/a\", version = \"{version}\" }}\n\
                 b = {{ path = \"crates/b\", version = \"{version}\" }}\n"
            ),
        );
        project.write(
            "crates/a/Cargo.toml",
            "[package]\nname = \"a\"\nversion.workspace = true\nedition.workspace = true\n\n\
             [dependencies]\nb = { path = \"../b\", version = \"0.0.2\" }\n",
        );
        project.write("crates/a/src/lib.rs", "pub fn a() {}\n");
        project.write(
            "crates/b/Cargo.toml",
            "[package]\nname = \"b\"\nversion.workspace = true\nedition.workspace = true\n",
        );
        project.write("crates/b/src/lib.rs", "pub fn b() {}\n");
        if version != "0.0.2" {
            // keep the intra-workspace requirement in step with the shared one
            let manifest = project
                .read("crates/a/Cargo.toml")
                .replace("0.0.2", version);
            project.write("crates/a/Cargo.toml", &manifest);
        }
        project.generate_lockfile();
        project
    }

    /// Extend the isolated git config (later sections win over earlier ones).
    fn configure(&self, extra: &str) -> &Project {
        let mut text = std::fs::read_to_string(&self.global_config).expect("read the git config");
        text.push_str(extra);
        std::fs::write(&self.global_config, text).expect("write the git config");
        self
    }

    /// A lockfile, so the `cargo update --workspace` step has something to do.
    fn generate_lockfile(&self) -> &Project {
        let output = Command::new("cargo")
            .args(["generate-lockfile", "--offline"])
            .current_dir(&self.dir)
            .output()
            .expect("run cargo generate-lockfile");
        assert!(
            output.status.success(),
            "cargo generate-lockfile failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        self
    }

    /// A single-package project.
    fn package(name: &str, version: &str) -> Project {
        let project = Project::new(name);
        project.write(
            "Cargo.toml",
            &format!(
                "[package]\nname = \"{name}\"\nversion = \"{version}\"\nedition = \"2021\"\n\n[dependencies]\n"
            ),
        );
        project.write("src/lib.rs", "pub fn it() {}\n");
        project.generate_lockfile();
        project
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.dir.join(relative)
    }

    fn write(&self, relative: &str, content: &str) -> &Project {
        let path = self.path(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create the parent directory");
        }
        std::fs::write(&path, content).expect("write the file");
        self
    }

    fn read(&self, relative: &str) -> String {
        std::fs::read_to_string(self.path(relative))
            .unwrap_or_else(|err| panic!("cannot read {relative}: {err}"))
    }

    fn exists(&self, relative: &str) -> bool {
        self.path(relative).exists()
    }

    /// `git init` plus one commit holding everything written so far.
    fn commit_all(&self, message: &str) -> &Project {
        self.git(&["init"]);
        self.git(&["add", "--all"]);
        self.git(&["commit", "--message", message]);
        self
    }

    fn git(&self, args: &[&str]) -> String {
        let output = self.git_raw(args);
        assert!(
            output.status.success(),
            "git {args:?} failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn git_raw(&self, args: &[&str]) -> std::process::Output {
        Command::new("git")
            .args(args)
            .current_dir(&self.dir)
            .env("GIT_CONFIG_GLOBAL", &self.global_config)
            .env("GIT_CONFIG_SYSTEM", &self.system_config)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("run git")
    }

    fn git_ok(&self, args: &[&str]) -> bool {
        self.git_raw(args).status.success()
    }

    fn tag_message(&self, tag: &str) -> String {
        self.git(&["tag", "--list", tag, "--format=%(contents:subject)"])
    }

    fn log_subjects(&self) -> Vec<String> {
        self.git(&["log", "--format=%s"])
            .lines()
            .map(|line| line.to_string())
            .collect()
    }

    fn run(&self, args: &[&str]) -> Run {
        self.run_with_stdin(args, None)
    }

    fn run_with_stdin(&self, args: &[&str], input: Option<&str>) -> Run {
        let mut command = Command::new(env!("CARGO_BIN_EXE_bumpp"));
        command
            .args(args)
            .current_dir(&self.dir)
            .env("GIT_CONFIG_GLOBAL", &self.global_config)
            .env("GIT_CONFIG_SYSTEM", &self.system_config)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("CARGO_NET_OFFLINE", "true")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        match input {
            Some(text) => {
                command.stdin(Stdio::piped());
                let mut child = command.spawn().expect("spawn bumpp");
                {
                    use std::io::Write;
                    let stdin = child.stdin.as_mut().expect("stdin");
                    stdin.write_all(text.as_bytes()).expect("write stdin");
                }
                let output = child.wait_with_output().expect("wait for bumpp");
                Run {
                    code: output.status.code().unwrap_or(-1),
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                }
            }
            None => {
                command.stdin(Stdio::null());
                let output = command.output().expect("run bumpp");
                Run {
                    code: output.status.code().unwrap_or(-1),
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                }
            }
        }
    }

    /// A bare repository to push to, next to the project. Call it after
    /// `git init`.
    fn bare_remote(&self) -> PathBuf {
        let remote = sibling(&self.dir, "remote.git");
        let _ = std::fs::remove_dir_all(&remote);
        let output = Command::new("git")
            .args(["init", "--bare", "--quiet"])
            .arg(&remote)
            .env("GIT_CONFIG_GLOBAL", &self.global_config)
            .env("GIT_CONFIG_SYSTEM", &self.system_config)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("git init --bare");
        assert!(output.status.success(), "cannot create the bare remote");
        self.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
        remote
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        remove_dir_forced(&self.dir);
        remove_dir_forced(&sibling(&self.dir, "remote.git"));
        let _ = std::fs::remove_file(&self.global_config);
        let _ = std::fs::remove_file(&self.system_config);
    }
}

/// Remove a directory tree, clearing read-only flags on the way: git writes its
/// object files read-only, which makes a plain `remove_dir_all` fail on Windows.
/// A test that leaves junk in `%TEMP%` is a test that will be noticed.
fn remove_dir_forced(path: &Path) {
    let _ = make_writable(path);
    let _ = std::fs::remove_dir_all(path);
}

fn make_writable(path: &Path) -> std::io::Result<()> {
    // Only Windows blocks removal on a read-only file, and the flag is cleared
    // on a throwaway directory that is about to be deleted.
    #[allow(clippy::permissions_set_readonly_false)]
    {
        let metadata = std::fs::symlink_metadata(path)?;
        let mut permissions = metadata.permissions();
        if permissions.readonly() {
            permissions.set_readonly(false);
            let _ = std::fs::set_permissions(path, permissions);
        }
        if metadata.is_dir() {
            for entry in std::fs::read_dir(path)? {
                make_writable(&entry?.path())?;
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------

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
