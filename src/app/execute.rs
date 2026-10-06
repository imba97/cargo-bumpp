//! Doing what the confirmed plan says: writing the files and the lockfile,
//! `--execute`, the commit, the tag, and the push.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::error::{Error, Result};
use crate::git::{display_path, Git};
use crate::options::Options;
use crate::plan::Plan;
use crate::report::{GitSummary, Ui};
use crate::workspace::Workspace;

use super::Transaction;

/// Everything that happens after the confirmation.
pub(super) fn apply(
    transaction: &mut Transaction,
    workspace: &Workspace,
    options: &Options,
    plan: &Plan,
    git: &GitSummary,
    ui: &Ui,
) -> Result<()> {
    if plan.files.is_empty() {
        ui.step("files", "no file changes");
    }
    for file in &plan.files {
        std::fs::write(&file.path, &file.new_text)
            .map_err(|err| Error::io(format!("cannot write `{}`: {err}", file.path.display())))?;
        ui.step("wrote", &file.display);
    }

    if let Some(lockfile) = &plan.lockfile {
        cargo_update_workspace(&workspace.root, ui)?;
        ui.step("updated", display_path(&workspace.root, lockfile));
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
                unsigned: options.explicit_no_sign,
                verify: options.verify,
                all: options.all,
            },
        )?;
        transaction.committed = true;
        ui.step("commit", &message);
    }

    if options.tag {
        if let Some(tag) = &git.tag {
            let message = git.commit_message.clone().unwrap_or_else(|| tag.clone());
            transaction
                .git
                .tag(tag, &message, options.sign, options.explicit_no_sign)?;
            transaction.tag = Some(tag.clone());
            ui.step("tag", tag);
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
        let push_text = format!("{remote} {}", what.join(", "));
        ui.step("push", &push_text);
    }

    Ok(())
}

/// Push, keeping git's exit code and error text when it fails.
fn push(git: &Git, remote: &str, refspec: &str) -> Result<()> {
    let output = git.push(remote, refspec, false)?;
    if output.ok() {
        return Ok(());
    }
    let detail = output.failure_text();
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
    ui.step("ran", "cargo update --workspace");
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
    ui.step("ran", command);
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
