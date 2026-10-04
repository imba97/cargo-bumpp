//! The reports themselves: the plan, the git summary, and the commits list.
//!
//! Rendering is kept apart from the `Ui` sink so the wording and layout of a
//! report can change without touching colour or quiet handling.

use crate::git::display_path;
use crate::plan::Plan;

use super::Ui;

/// What the git steps will do, for the plan and the confirmation.
#[derive(Debug, Clone, Default)]
pub struct GitSummary {
    pub commit_message: Option<String>,
    pub tag: Option<String>,
    pub push: Option<String>,
    /// Branch being pushed, when known.
    pub branch: Option<String>,
}

impl GitSummary {
    /// True when a push step will run.
    pub fn will_push(&self) -> bool {
        self.push.is_some()
    }
}

/// Print the plan: where it runs, what it bumps, and every line it touches.
pub fn print_plan(ui: &Ui, plan: &Plan, command: &str, git: &GitSummary) {
    if ui.quiet {
        return;
    }
    println!("  {}  {}", ui.bold(command), plan.root.display());

    if plan.bumps.len() > 1 {
        let names: Vec<&str> = plan.bumps.iter().map(|bump| bump.name.as_str()).collect();
        println!(
            "  {}  {}  ({})",
            ui.dim("crates"),
            names.join(", "),
            plan.source.describe()
        );
    }

    println!();
    if plan.bumps.len() > 1 {
        for bump in &plan.bumps {
            if bump.changed() {
                println!(
                    "  {:<24} {} -> {}",
                    bump.name,
                    bump.old,
                    ui.bold_green(bump.new.to_string())
                );
            } else {
                println!("  {:<24} {} (unchanged)", bump.name, bump.old);
            }
        }
    } else {
        println!(
            "  {} -> {}",
            plan.old_version,
            ui.bold_green(plan.new_version.to_string())
        );
    }
    println!();

    if plan.files.is_empty() {
        println!(
            "  {}",
            ui.dim("no file changes (the version stays as it is)")
        );
    }
    for file in &plan.files {
        println!("  {}", ui.bold(&file.display));
        let label_width = file
            .edits
            .iter()
            .map(|edit| edit.label.chars().count())
            .max()
            .unwrap_or(0);
        for edit in &file.edits {
            println!(
                "    {:<width$}  line {}: {} -> {}",
                edit.label,
                edit.line + 1,
                edit.old,
                ui.bold_green(&edit.new),
                width = label_width
            );
        }
    }
    if let Some(lockfile) = &plan.lockfile {
        println!(
            "  {}  {}",
            ui.bold(display_path(&plan.root, lockfile)),
            ui.dim("(cargo update --workspace)")
        );
    }

    println!();
    match &git.commit_message {
        Some(message) => println!("  {}  {}", ui.dim("commit"), ui.bold(message)),
        None => println!("  {}  {}", ui.dim("commit"), ui.dim("no")),
    }
    match &git.tag {
        Some(tag) => println!("  {}  {}", ui.dim("   tag"), ui.bold(tag)),
        None => println!("  {}  {}", ui.dim("   tag"), ui.dim("no")),
    }
    if git.will_push() {
        // `git.push` is `Some(remote)` here; the `if` above guarantees it.
        let remote = git.push.as_deref().unwrap_or("");
        match &git.branch {
            Some(branch) => {
                let tag = git.tag.as_deref().unwrap_or("-");
                println!(
                    "  {}  {} {} and {}",
                    ui.dim("  push"),
                    remote,
                    branch,
                    ui.bold(tag)
                );
            }
            None => println!("  {}  {}", ui.dim("  push"), ui.bold(remote)),
        }
    } else {
        println!("  {}  {}", ui.dim("  push"), ui.dim("no"));
    }
    println!();
}

/// The `Bump?` block: a short restatement, not the whole plan again.
pub fn confirmation_text(ui: &Ui, plan: &Plan, git: &GitSummary) -> String {
    let files = if plan.files.is_empty() {
        "(none)".to_string()
    } else {
        plan.files
            .iter()
            .map(|file| ui.bold(&file.display))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut text = String::new();
    text.push_str(&format!("\n   files {files}\n"));
    match &git.commit_message {
        Some(message) => text.push_str(&format!("  commit {}\n", ui.bold(message))),
        None => text.push_str("  commit (none)\n"),
    }
    match &git.tag {
        Some(tag) => text.push_str(&format!("     tag {}\n", ui.bold(tag))),
        None => text.push_str("     tag (none)\n"),
    }
    match &git.push.as_deref() {
        Some(remote) => text.push_str(&format!("    push {} ({})\n", ui.bold_cyan("yes"), remote)),
        None => text.push_str("    push no\n"),
    }
    if plan.bumps.len() > 1 {
        text.push('\n');
        for bump in &plan.bumps {
            text.push_str(&format!(
                "    {} {} -> {}\n",
                ui.dim(&bump.name),
                bump.old,
                ui.bold_green(bump.new.to_string())
            ));
        }
    } else {
        text.push_str(&format!(
            "\n    from {}\n      to {}\n",
            ui.bold(plan.old_version.to_string()),
            ui.bold_green(plan.new_version.to_string())
        ));
    }
    text
}

/// Build one progress line so the `apply` stage's labels line up. The label is
/// right-padded with spaces, dim-styled, then the value follows.
pub(super) fn format_step_line(ui: &Ui, label: &str, value: &str) -> String {
    let padded = format!("{label:<width$}", width = Ui::APPLY_LABEL_WIDTH);
    format!("  {} {}", ui.dim(&padded), value)
}

/// The commits `conventional` looked at.
pub fn print_commits(
    ui: &Ui,
    range: &str,
    commits: &[crate::git::Commit],
    level: &str,
    truncated: usize,
) {
    if ui.quiet {
        return;
    }
    println!("  {} ({range}, {})", ui.dim("commits"), level);
    for commit in commits {
        let subject = commit.subject.trim();
        println!(
            "    {} {}",
            ui.dim(&commit.id[..commit.id.len().min(8)]),
            subject
        );
    }
    if truncated > 0 {
        println!(
            "    {}",
            ui.dim(format!("... {truncated} older commits were not inspected"))
        );
    }
    println!();
}
