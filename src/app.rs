//! The run itself: checks, plan, confirmation, writes, commit, tag, push.
//!
//! ## Rollback
//!
//! Pushing is the first action that cannot be undone locally. Everything before
//! it is wrapped: if writing, `cargo update`, `--execute`, the commit or the tag
//! fails, the run puts the workspace back the way it found it.
//!
//! | reached | undo |
//! | --- | --- |
//! | tag created | `git tag --delete <tag>` |
//! | commit created | `git reset --hard <the commit HEAD pointed at>` |
//! | files written | restored from the snapshot taken before the first write |
//!
//! A failed **push** is deliberately not rolled back: the commit and the tag stay
//! local, git's own error is printed, and the remediation commands are offered.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::cli::{self, Parsed};
use crate::config;
use crate::error::{Error, Result};
use crate::git::{display_path, Commit, Git};
use crate::options::Options;
use crate::plan::{self, Plan};
use crate::prompt::{self, Prompt, Selection, TerminalPrompt};
use crate::report::{self, GitSummary, Ui};
use crate::semver::Level;
use crate::sys;
use crate::tokens::{self, Tokens};
use crate::workspace::Workspace;

/// Run the tool. `args` excludes the program name; `injected` replaces the
/// terminal prompt (library use and tests).
pub fn run(args: &[String], cwd: &Path, injected: Option<&mut dyn Prompt>) -> Result<()> {
    let raw = match cli::parse(args)? {
        Parsed::Help => {
            print!("{}", cli::help());
            return Ok(());
        }
        Parsed::Version => {
            println!("cargo-bumpp {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Parsed::Run(raw) => *raw,
    };

    // The config file lives at the workspace root, so the workspace comes first.
    let workspace = Workspace::load(cwd)?;
    let from_file = config::load(raw.config_path.as_deref(), &workspace.root)?;
    let from_env = config::from_env()?;
    let options = Options::resolve(raw.overlay(from_env.overlay(from_file)))?;

    let color = !options.quiet && sys::enable_ansi_output();
    let ui = Ui::new(options.quiet, color);
    let mut terminal = TerminalPrompt::new(color, options.yes);
    let prompt: &mut dyn Prompt = match injected {
        Some(prompt) => prompt,
        None => &mut terminal,
    };

    if options.ignore_scripts {
        ui.note("--ignore-scripts has no effect here: Cargo has no lifecycle scripts");
    }
    for note in &options.notes {
        ui.note(note);
    }

    let git = Git::new(&workspace.root);

    // ------------------------------------------------------------ preconditions
    // The clean-tree check runs before any prompt and before anything is written:
    // in a dirty tree there is no way to tell our changes from someone else's,
    // and no way to roll back safely.
    if options.touches_git() {
        if !git.is_repository() {
            return Err(Error::check(format!(
                "`{}` is not a git repository",
                workspace.root.display()
            ))
            .with_hint(
                "the version numbers can still be bumped with --no-commit --no-tag --no-push",
            ));
        }
        if options.git_check {
            let entries = git.status_porcelain()?;
            if !entries.is_empty() {
                let mut hint = entries.join("\n");
                hint.push_str("\ncommit or stash these first, or pass --no-git-check");
                return Err(Error::check("Git working tree is not clean:").with_hint(hint));
            }
        }
    }

    // ------------------------------------------------------------------ version
    let current = plan::detect_current(&workspace, &options)?;
    let wants_conventional = options.level.is_conventional() || options.release_from_prompt;
    let analysis = if wants_conventional {
        Some(analyze_commits(&git, &options))
    } else {
        None
    };
    let conventional_level = analysis
        .as_ref()
        .map(|analysis| analysis.level.clone())
        .unwrap_or(Level::Patch);
    if let Some(analysis) = &analysis {
        if !analysis.available && wants_conventional {
            ui.warn("cannot read the commit history; `conventional` falls back to `patch`");
        }
    }

    let level = if options.release_from_prompt {
        if !prompt.available() {
            return Err(prompt::prompt_unavailable());
        }
        let rows = prompt::choices(&current, &options.preid, conventional_level.clone());
        match prompt.select_release(&current, &rows)? {
            Selection::Level(level) => level,
            Selection::Version(version) => Level::Explicit(version),
        }
    } else {
        options.level.clone()
    };
    // The level as asked for, before `conventional` becomes concrete: that is
    // what `{releaseType}` reports, and what decides whether the commit log is
    // worth printing.
    let release_type = level.as_token().to_string();
    let asked_conventional = level.is_conventional();
    let level = prompt::resolve_level(&level, &current, conventional_level);

    if let Some(analysis) = &analysis {
        if options.print_commits && asked_conventional {
            report::print_commits(
                &ui,
                &analysis.range,
                &analysis.commits,
                release_type.as_str(),
                analysis.truncated,
            );
        }
    }

    let plan = plan::build(&workspace, &options, &level)?;
    let git_summary = git_summary(&git, &options, &plan, &release_type)?;

    for warning in &plan.warnings {
        ui.warn(warning);
    }
    for note in &plan.notes {
        ui.note(note);
    }
    report::print_plan(&ui, &plan, "bumpp", &git_summary);

    if let Some(tag) = &git_summary.tag {
        if git.tag_exists(tag) {
            return Err(Error::check(format!("tag `{tag}` already exists"))
                .with_hint("delete it, or pick another name with --tag <template>"));
        }
    }

    // -------------------------------------------------------------- confirmation
    let confirmation = report::confirmation_text(&ui, &plan, &git_summary);
    if options.execute.is_some() {
        ui.note(
            "`--execute` runs before the commit; what it does is not rolled back if the run fails",
        );
    }
    if !options.yes {
        if !prompt.confirm(&confirmation)? {
            return Err(Error::cancelled());
        }
    } else {
        ui.info(confirmation.trim_end());
    }

    // ------------------------------------------------------------- the run
    let mut transaction = Transaction::new(&git, workspace.root.clone());
    for file in &plan.files {
        transaction.snapshot(&file.path)?;
    }
    if let Some(lockfile) = &plan.lockfile {
        transaction.snapshot(lockfile)?;
    }
    if options.touches_git() {
        transaction.head = git.head().ok();
    }

    match apply(
        &mut transaction,
        &workspace,
        &options,
        &plan,
        &git_summary,
        &ui,
    ) {
        Ok(()) => Ok(()),
        Err(err) => {
            if err.is_push() {
                // Local work stays: the commit and the tag are salvageable.
                return Err(err);
            }
            let problems = transaction.rollback(&ui);
            for problem in &problems {
                ui.warn(problem);
            }
            Err(err)
        }
    }
}

/// Everything that happens after the confirmation.
fn apply(
    transaction: &mut Transaction,
    workspace: &Workspace,
    options: &Options,
    plan: &Plan,
    git: &GitSummary,
    ui: &Ui,
) -> Result<()> {
    if plan.files.is_empty() {
        ui.info(format!("  {} no file changes", ui.dim("files")));
    }
    for file in &plan.files {
        std::fs::write(&file.path, &file.new_text)
            .map_err(|err| Error::io(format!("cannot write `{}`: {err}", file.path.display())))?;
        ui.info(format!("  {} {}", ui.dim("wrote"), file.display));
    }

    if let Some(lockfile) = &plan.lockfile {
        cargo_update_workspace(&workspace.root, ui)?;
        ui.info(format!(
            "  {} {}",
            ui.dim("updated"),
            display_path(&workspace.root, lockfile)
        ));
    }

    if let Some(command) = &options.execute {
        run_execute(ui, command, &workspace.root)?;
    }

    if options.commit {
        let mut files: Vec<PathBuf> = transaction
            .changed_paths()
            .into_iter()
            .filter(|path| !transaction.git.changed_but_ignored(path))
            .collect();
        if options.all {
            transaction.git.add_all()?;
            files.clear();
        }
        let message = git.commit_message.clone().unwrap_or_default();
        transaction.git.commit(
            &files,
            &message,
            crate::git::CommitOptions {
                sign: options.sign,
                unsigned: options.unsigned,
                verify: options.verify,
                all: options.all,
            },
        )?;
        transaction.committed = true;
        ui.info(format!("  {} {}", ui.dim("commit"), message));
    }

    if options.tag {
        if let Some(tag) = &git.tag {
            let message = git.commit_message.clone().unwrap_or_else(|| tag.clone());
            transaction
                .git
                .tag(tag, &message, options.sign, options.unsigned)?;
            transaction.tag = Some(tag.clone());
            ui.info(format!("  {} {}", ui.dim("   tag"), tag));
        }
    }

    if options.push {
        let remote = git.push.clone().unwrap_or_else(|| "origin".to_string());
        let branch = git.branch.clone();
        if let Some(branch) = &branch {
            push(&transaction.git, &remote, branch)?;
        } else {
            ui.warn("detached HEAD: only the tag will be pushed");
        }
        if let Some(tag) = &git.tag {
            push(&transaction.git, &remote, &format!("refs/tags/{tag}"))?;
        }
        let mut what = Vec::new();
        if let Some(branch) = &branch {
            what.push(branch.clone());
        }
        if let Some(tag) = &git.tag {
            what.push(tag.clone());
        }
        ui.info(format!(
            "  {} {} {}",
            ui.dim("  push"),
            remote,
            what.join(", ")
        ));
    }

    Ok(())
}

/// Push, keeping git's exit code and error text when it fails.
fn push(git: &Git, remote: &str, refspec: &str) -> Result<()> {
    let output = git.push(remote, refspec)?;
    if output.ok() {
        return Ok(());
    }
    let detail = if output.stderr.trim().is_empty() {
        output.stdout.trim().to_string()
    } else {
        output.stderr.trim().to_string()
    };
    let hint = format!(
        "The commit and the tag are local; nothing was rolled back.\nRetry the push with:\n  git push {remote} {refspec}\nTo start over instead:\n  git reset --hard HEAD~1\n  git tag --delete {}",
        refspec.rsplit('/').next().unwrap_or(refspec)
    );
    Err(Error::push(
        output.code,
        format!("`git push {remote} {refspec}` failed:\n{detail}"),
    )
    .with_hint(hint))
}

/// `cargo update --workspace`: refresh the lockfile entries of the members.
fn cargo_update_workspace(root: &Path, ui: &Ui) -> Result<()> {
    ui.info(format!("  {} cargo update --workspace", ui.dim("   run")));
    let output = Command::new("cargo")
        .args(["update", "--workspace"])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|err| Error::io(format!("cannot run cargo: {err}")))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(Error::check(format!(
        "`cargo update --workspace` failed:\n{}",
        stderr.trim()
    ))
    .with_hint("pass --no-lockfile to leave Cargo.lock alone"))
}

/// `-x, --execute`: run the command with the shell, like the reference does.
fn run_execute(ui: &Ui, command: &str, cwd: &Path) -> Result<()> {
    ui.info(format!("  {} {command}", ui.dim("   run")));
    let mut shell = if cfg!(windows) {
        Command::new("cmd")
    } else {
        Command::new("sh")
    };
    shell
        .arg(if cfg!(windows) { "/C" } else { "-c" })
        .arg(command);
    let status = shell
        .current_dir(cwd)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|err| Error::io(format!("cannot run `{command}`: {err}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::check(format!(
            "`{command}` failed with {}",
            status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "a signal".to_string())
        )))
    }
}

/// Work out the commit message, the tag name and where the push goes, so the
/// plan can show them and the confirmation can be about something concrete.
fn git_summary(
    git: &Git,
    options: &Options,
    plan: &Plan,
    release_type: &str,
) -> Result<GitSummary> {
    let date = sys::local_date();
    let base = Tokens {
        version: plan.new_version.to_string(),
        old_version: plan.old_version.to_string(),
        tag: String::new(),
        release_type: release_type.to_string(),
        major: plan.new_version.major.to_string(),
        minor: plan.new_version.minor.to_string(),
        patch: plan.new_version.patch.to_string(),
        date: format!("{:04}-{:02}-{:02}", date.0, date.1, date.2),
    };

    // The tag is resolved first: its result is the `{tag}` token for everything
    // else, including the commit message.
    let tag = if options.tag {
        let rendered = tokens::render(&options.tag_name, &base);
        let name = rendered.trim().to_string();
        validate_tag(&name)?;
        Some(name)
    } else {
        Some(format!("v{}", plan.new_version))
    };

    let with_tag = Tokens {
        tag: tag.clone().unwrap_or_default(),
        ..base
    };
    let commit_message = options
        .commit
        .then(|| tokens::render(&options.commit_message, &with_tag));

    let (push, branch) = if options.push && options.touches_git() {
        let remote = git.default_remote()?;
        (Some(remote), git.current_branch().ok())
    } else {
        (None, None)
    };

    Ok(GitSummary {
        commit_message,
        tag: options.tag.then_some(tag).flatten(),
        push,
        branch,
    })
}

/// A tag name git will accept, checked early so the failure happens before
/// anything is written.
fn validate_tag(name: &str) -> Result<()> {
    let bad = name.is_empty()
        || name.starts_with('-')
        || name.contains("..")
        || name.ends_with(".lock")
        || name
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
        || name.contains(['~', '^', ':', '?', '*', '[', '\\']);
    if bad {
        return Err(
            Error::check(format!("`{name}` is not a usable git tag name"))
                .with_hint("change the tag template with --tag <template> or in bumpp.toml"),
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// conventional commits
// ---------------------------------------------------------------------------

/// What the recent commits say the next version should be.
#[derive(Debug, Clone)]
pub struct Analysis {
    pub level: Level,
    pub commits: Vec<Commit>,
    pub range: String,
    pub truncated: usize,
    pub available: bool,
}

/// Read the commits since the last tag and decide `major` / `minor` / `patch`.
///
/// `isBreaking` anywhere wins, otherwise a `feat` means minor, otherwise patch.
fn analyze_commits(git: &Git, options: &Options) -> Analysis {
    let fallback = Analysis {
        level: Level::Patch,
        commits: Vec::new(),
        range: String::new(),
        truncated: 0,
        available: false,
    };
    if !git.is_repository() {
        return fallback;
    }
    let from = git.last_tag().unwrap_or(None);
    let range = match &from {
        Some(tag) => format!("{tag}..HEAD"),
        None => "HEAD".to_string(),
    };
    let commits = match git.commits(&range, options.commit_window + 1) {
        Ok(commits) => commits,
        Err(_) => return fallback,
    };

    let mut commits = commits;
    let truncated = if commits.len() > options.commit_window {
        let extra = commits.len() - options.commit_window;
        commits.truncate(options.commit_window);
        extra
    } else {
        0
    };

    let level = classify(&commits);
    Analysis {
        level,
        commits,
        range,
        truncated,
        available: true,
    }
}

fn classify(commits: &[Commit]) -> Level {
    let mut major = false;
    let mut minor = false;
    for commit in commits {
        if is_breaking(commit) {
            major = true;
        } else if commit_type(&commit.subject).as_deref() == Some("feat") {
            minor = true;
        }
    }
    if major {
        Level::Major
    } else if minor {
        Level::Minor
    } else {
        Level::Patch
    }
}

/// `type(scope)!: description` — the type, when the subject follows the
/// convention at all.
fn commit_type(subject: &str) -> Option<String> {
    let colon = subject.find(':')?;
    let prefix = &subject[..colon];
    let end = prefix.find('(').unwrap_or(prefix.len());
    let kind = prefix[..end].trim();
    if kind.is_empty() || !kind.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return None;
    }
    Some(kind.to_ascii_lowercase())
}

/// Breaking when the header carries `!` before the colon, or when a footer says
/// `BREAKING CHANGE:` / `BREAKING-CHANGE:`.
fn is_breaking(commit: &Commit) -> bool {
    if let Some(colon) = commit.subject.find(':') {
        let prefix = &commit.subject[..colon];
        if prefix.trim_end().ends_with('!') || prefix.contains(")!") {
            return true;
        }
    }
    commit.body.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with("BREAKING CHANGE:") || line.starts_with("BREAKING-CHANGE:")
    })
}

// ---------------------------------------------------------------------------
// rollback
// ---------------------------------------------------------------------------

/// The state needed to undo a run that failed before the push.
struct Transaction {
    git: Git,
    root: PathBuf,
    snapshots: Vec<(PathBuf, Option<String>)>,
    /// The commit HEAD pointed at when the run started.
    head: Option<String>,
    committed: bool,
    tag: Option<String>,
}

impl Transaction {
    fn new(git: &Git, root: PathBuf) -> Transaction {
        Transaction {
            git: git.clone(),
            root,
            snapshots: Vec::new(),
            head: None,
            committed: false,
            tag: None,
        }
    }

    /// Remember a file's contents before it is touched. A file that does not
    /// exist yet is remembered as absent, so rollback can remove it again.
    fn snapshot(&mut self, path: &Path) -> Result<()> {
        if self
            .snapshots
            .iter()
            .any(|(candidate, _)| candidate == path)
        {
            return Ok(());
        }
        let content = match std::fs::read_to_string(path) {
            Ok(content) => Some(content),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(err) => {
                return Err(Error::io(format!(
                    "cannot read `{}`: {err}",
                    path.display()
                )))
            }
        };
        self.snapshots.push((path.to_path_buf(), content));
        Ok(())
    }

    /// Files whose contents differ from the snapshot: what the commit should
    /// include, no matter which step changed them.
    fn changed_paths(&self) -> Vec<PathBuf> {
        self.snapshots
            .iter()
            .filter(|(path, before)| {
                let now = std::fs::read_to_string(path).ok();
                &now != before
            })
            .map(|(path, _)| path.clone())
            .collect()
    }

    /// Put the workspace back. Returns the problems it could not fix.
    fn rollback(&self, ui: &Ui) -> Vec<String> {
        let mut problems = Vec::new();
        if let Some(tag) = &self.tag {
            if let Err(err) = self.git.delete_tag(tag) {
                problems.push(format!("could not delete tag `{tag}`: {err}"));
            } else {
                ui.info(format!("  {} tag {tag}", ui.dim("undid ")));
            }
        }

        let mut reset = false;
        if self.committed {
            if let Some(head) = &self.head {
                match self.git.reset_hard(head) {
                    Ok(()) => {
                        reset = true;
                        ui.info(format!(
                            "  {} commit (back to {})",
                            ui.dim("undid "),
                            &head[..head.len().min(8)]
                        ));
                    }
                    Err(err) => problems.push(format!("could not reset to {head}: {err}")),
                }
            }
        }

        for (path, before) in &self.snapshots {
            // After a successful reset git has already put every tracked file
            // back — and it put it back its own way (line endings included), so
            // writing the snapshot over it would only make the tree look dirty.
            if reset && self.git.is_tracked(path) {
                continue;
            }
            match before {
                Some(content) => {
                    if std::fs::read_to_string(path).ok().as_deref() == Some(content.as_str()) {
                        continue;
                    }
                    if let Err(err) = std::fs::write(path, content) {
                        problems.push(format!("could not restore `{}`: {err}", path.display()));
                    }
                }
                None => {
                    if path.exists() {
                        if let Err(err) = std::fs::remove_file(path) {
                            problems.push(format!("could not remove `{}`: {err}", path.display()));
                        }
                    }
                }
            }
        }
        let _ = &self.root;
        problems
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(subject: &str, body: &str) -> Commit {
        Commit {
            id: "0000000".to_string(),
            subject: subject.to_string(),
            body: body.to_string(),
        }
    }

    #[test]
    fn conventional_classification() {
        assert_eq!(classify(&[commit("chore: x", "")]), Level::Patch);
        assert_eq!(
            classify(&[commit("fix: x", ""), commit("feat: y", "")]),
            Level::Minor
        );
        assert_eq!(
            classify(&[commit("feat: x", ""), commit("feat!: y", "")]),
            Level::Major
        );
        assert_eq!(
            classify(&[commit("feat: x", "BREAKING CHANGE: gone")]),
            Level::Major
        );
        assert_eq!(
            classify(&[commit("chore: x", "BREAKING-CHANGE: gone")]),
            Level::Major
        );
        // free-form history falls back to patch, without complaint
        assert_eq!(classify(&[commit("Hello w3wright", "")]), Level::Patch);
        assert_eq!(classify(&[]), Level::Patch);
    }

    #[test]
    fn commit_types() {
        assert_eq!(commit_type("feat: x").as_deref(), Some("feat"));
        assert_eq!(commit_type("feat(api)!: x").as_deref(), Some("feat"));
        assert_eq!(commit_type("FIX: x").as_deref(), Some("fix"));
        assert_eq!(commit_type("no colon here"), None);
        assert_eq!(commit_type("中文提交: x"), None);
    }

    #[test]
    fn tag_names_are_checked_early() {
        assert!(validate_tag("v1.2.3").is_ok());
        assert!(validate_tag("release/1.2.3").is_ok());
        assert!(validate_tag("").is_err());
        assert!(validate_tag("v1.2.3 ").is_err());
        assert!(validate_tag("-v1").is_err());
        assert!(validate_tag("v1..2").is_err());
        assert!(validate_tag("v1^2").is_err());
        assert!(validate_tag("v1.2.3.lock").is_err());
    }

    #[test]
    fn transaction_reports_changed_paths() {
        let dir = std::env::temp_dir().join(format!("bumpp-tx-{}", std::process::id()));
        crate::remove_dir_forced(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Cargo.toml");
        std::fs::write(&file, "version = \"0.0.2\"\n").unwrap();

        let git = Git::new(&dir);
        let mut tx = Transaction::new(&git, dir.clone());
        tx.snapshot(&file).unwrap();
        tx.snapshot(&dir.join("Cargo.lock")).unwrap();
        assert!(tx.changed_paths().is_empty());

        std::fs::write(&file, "version = \"0.0.3\"\n").unwrap();
        std::fs::write(dir.join("Cargo.lock"), "new\n").unwrap();
        let changed = tx.changed_paths();
        assert_eq!(changed.len(), 2);

        let ui = Ui::new(true, false);
        assert!(tx.rollback(&ui).is_empty());
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "version = \"0.0.2\"\n"
        );
        assert!(
            !dir.join("Cargo.lock").exists(),
            "a file that did not exist is removed again"
        );
        crate::remove_dir_forced(&dir);
    }

    #[test]
    fn rollback_undoes_a_commit_and_a_tag_as_well_as_the_files() {
        // The nested case: when the tag step fails, undoing only the tag would
        // leave a quiet half-finished commit behind.
        let dir = std::env::temp_dir().join(format!("bumpp-tx-nested-{}", std::process::id()));
        crate::remove_dir_forced(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Cargo.toml");

        let git = Git::new(&dir);
        fn identity(args: &[&str]) -> Vec<String> {
            // `-c` rather than a global config: the developer's own git settings
            // (a signing key, for instance) must not decide whether this passes.
            let mut full: Vec<String> = [
                "-c",
                "user.name=bumpp test",
                "-c",
                "user.email=t@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "tag.gpgsign=false",
            ]
            .iter()
            .map(|arg| arg.to_string())
            .collect();
            full.extend(args.iter().map(|arg| arg.to_string()));
            full
        }
        let run = |args: &[&str]| {
            let full = identity(args);
            let refs: Vec<&str> = full.iter().map(|arg| arg.as_str()).collect();
            git.run(&refs).unwrap()
        };
        run(&["init", "--quiet"]);
        std::fs::write(&file, "version = \"0.0.2\"\n").unwrap();
        run(&["add", "--all"]);
        run(&["commit", "--quiet", "--message", "chore: initial"]);
        let head = git.head().unwrap();

        let mut tx = Transaction::new(&git, dir.clone());
        tx.snapshot(&file).unwrap();
        tx.head = Some(head.clone());

        // ... and then a bump that fails at the tag
        std::fs::write(&file, "version = \"0.0.3\"\n").unwrap();
        run(&["add", "--all"]);
        run(&["commit", "--quiet", "--message", "chore: release v0.0.3"]);
        tx.committed = true;
        git.tag("v0.0.3", "chore: release v0.0.3", false, true)
            .unwrap();
        tx.tag = Some("v0.0.3".to_string());
        assert_ne!(git.head().unwrap(), head);

        let ui = Ui::new(true, false);
        assert!(
            tx.rollback(&ui).is_empty(),
            "rollback has nothing to complain about"
        );
        assert_eq!(git.head().unwrap(), head, "HEAD is back where it started");
        assert!(!git.tag_exists("v0.0.3"), "the tag is gone");
        // After a reset git restored the tracked file itself; comparing through
        // git keeps this independent of line-ending normalisation.
        let restored = git
            .run(&["show", &format!("{head}:Cargo.toml")])
            .unwrap()
            .stdout;
        assert!(restored.contains("0.0.2"), "{restored}");
        assert_eq!(
            git.status_porcelain().unwrap(),
            Vec::<String>::new(),
            "the tree is clean again"
        );
        crate::remove_dir_forced(&dir);
    }

    #[test]
    fn increment_is_not_used_directly_for_prerelease_levels() {
        // guards the helper used by the interactive rows
        let current = crate::semver::Version::parse("1.2.0").unwrap();
        assert_eq!(
            crate::plan::increment(&current, &Level::Patch, "beta")
                .unwrap()
                .to_string(),
            "1.2.1"
        );
    }
}
