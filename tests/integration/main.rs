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

mod command_line;
mod config;
mod execute;
mod flow;
mod hooks;
mod prompting;
mod retag;
mod templates;
mod version_source;
mod workspace_contract;
