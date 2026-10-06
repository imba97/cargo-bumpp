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

mod conventional;
mod execute;
mod retag;
mod rollback;
mod summary;

#[cfg(test)]
mod tests;

use std::path::Path;

use crate::cli::{self, Parsed};
use crate::config;
use crate::error::{Error, Result};
use crate::git::Git;
use crate::options::{self, Options};
use crate::plan;
use crate::prompt::{self, Prompt, Selection, TerminalPrompt};
use crate::report::{self, Ui};
use crate::semver::Level;
use crate::sys;
use crate::workspace::Workspace;

use self::conventional::analyze_commits;
use self::execute::apply;
use self::rollback::Transaction;
use self::summary::git_summary;

pub use self::conventional::Analysis;

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

    // Contradictions only the command line can express are checked here, before
    // the environment and `bumpp.toml` are merged into these options: a
    // repository's defaults must not turn a valid run into a usage error.
    options::validate_cli(&raw)?;

    // The config file lives at the workspace root, so the workspace comes first.
    let workspace = Workspace::load(cwd)?;
    let from_file = config::load(raw.config_path.as_deref(), &workspace.root)?;
    let from_env = config::from_env()?;
    let options = Options::resolve(raw.overlay(from_env.overlay(from_file)))?;

    let color = !options.quiet && sys::enable_ansi_output();
    let ui = Ui::new(options.quiet, color);
    let mut terminal = TerminalPrompt::new(color, options.yes, options.quiet);
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
        if options.git_check && !options.retag {
            let entries = git.status_porcelain()?;
            if !entries.is_empty() {
                let mut hint = entries.join("\n");
                hint.push_str("\ncommit or stash these first, or pass --no-git-check");
                return Err(Error::check("Git working tree is not clean:").with_hint(hint));
            }
        }
    }

    // --------------------------------------------------------------- re-release
    // `--retag` re-creates a tag that is already there and force-pushes it: a git
    // action with a confirmation in front of it, so none of the version machinery
    // below runs — no manifest is read beyond the workspace root, nothing is
    // written, and there is no plan to roll back.
    if options.retag {
        return retag::run(&git, &workspace.root, &options, &ui, prompt);
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
    let release_type = level.as_token();
    let asked_conventional = level.is_conventional();
    let level = prompt::resolve_level(&level, &current, conventional_level);

    if let Some(analysis) = &analysis {
        if options.print_commits && asked_conventional {
            report::print_commits(
                &ui,
                &analysis.range,
                &analysis.commits,
                release_type,
                analysis.truncated,
            );
        }
    }

    let plan = plan::build(&workspace, &options, &level)?;
    let git_summary = git_summary(&git, &options, &plan, release_type)?;

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
        if !prompt.confirm(&confirmation, "Bump?")? {
            return Err(Error::cancelled());
        }
    } else {
        ui.info(confirmation.trim_end());
    }

    // ------------------------------------------------------------- the run
    let mut transaction = Transaction::new(&git);
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
