//! Everything the tool prints.
//!
//! The plan output is a feature, not decoration: it lists every spot that will
//! change, with its line number, *before* anything is written. That is what the
//! design offers instead of a dry-run mode.

use std::path::Path;

use crate::plan::Plan;

/// Output sink with colour and quiet handling.
#[derive(Debug, Clone)]
pub struct Ui {
    pub quiet: bool,
    pub color: bool,
}

impl Ui {
    pub fn new(quiet: bool, color: bool) -> Ui {
        Ui { quiet, color }
    }

    fn paint(&self, code: &str, text: &str) -> String {
        if self.color {
            format!("\u{1b}[{code}m{text}\u{1b}[0m")
        } else {
            text.to_string()
        }
    }

    pub fn bold(&self, text: impl AsRef<str>) -> String {
        self.paint("1", text.as_ref())
    }

    pub fn dim(&self, text: impl AsRef<str>) -> String {
        self.paint("2", text.as_ref())
    }

    pub fn green(&self, text: impl AsRef<str>) -> String {
        self.paint("32", text.as_ref())
    }

    pub fn yellow(&self, text: impl AsRef<str>) -> String {
        self.paint("33", text.as_ref())
    }

    pub fn red(&self, text: impl AsRef<str>) -> String {
        self.paint("31", text.as_ref())
    }

    pub fn cyan(&self, text: impl AsRef<str>) -> String {
        self.paint("36", text.as_ref())
    }

    /// Ordinary progress output — hidden by `--quiet`.
    pub fn info(&self, text: impl AsRef<str>) {
        if !self.quiet {
            println!("{}", text.as_ref());
        }
    }

    /// A blank line, hidden by `--quiet`.
    pub fn blank(&self) {
        if !self.quiet {
            println!();
        }
    }

    /// Warnings are never suppressed: something needs attention.
    pub fn warn(&self, text: impl AsRef<str>) {
        for line in text.as_ref().lines() {
            println!("{} {line}", self.yellow("warning:"));
        }
    }

    /// Notes are informational, but they explain surprising behaviour, so they
    /// survive `--quiet` as well.
    pub fn note(&self, text: impl AsRef<str>) {
        for line in text.as_ref().lines() {
            println!("{} {line}", self.dim("note:"));
        }
    }

    pub fn error(&self, text: impl AsRef<str>) {
        eprintln!("{} {}", self.red("error:"), text.as_ref());
    }
}

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
                    ui.bold(bump.new.to_string())
                );
            } else {
                println!("  {:<24} {} (unchanged)", bump.name, bump.old);
            }
        }
    } else {
        println!(
            "  {} -> {}",
            plan.old_version,
            ui.bold(plan.new_version.to_string())
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
                ui.bold(&edit.new),
                width = label_width
            );
        }
    }
    if let Some(lockfile) = &plan.lockfile {
        println!(
            "  {}  {}",
            ui.bold(display(&plan.root, lockfile)),
            ui.dim("(cargo update --workspace)")
        );
    }

    println!();
    match &git.commit_message {
        Some(message) => println!("  {}  {}", ui.dim("commit"), message),
        None => println!("  {}  {}", ui.dim("commit"), ui.dim("no")),
    }
    match &git.tag {
        Some(tag) => println!("  {}  {}", ui.dim("   tag"), tag),
        None => println!("  {}  {}", ui.dim("   tag"), ui.dim("no")),
    }
    if git.will_push() {
        // `git.push` is `Some(remote)` here; the `if` above guarantees it.
        let remote = git.push.as_deref().unwrap_or("");
        match &git.branch {
            Some(branch) => println!(
                "  {}  {} {} and {}",
                ui.dim("  push"),
                remote,
                branch,
                git.tag.as_deref().unwrap_or("-")
            ),
            None => println!("  {}  {}", ui.dim("  push"), remote),
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
            .map(|file| file.display.clone())
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut text = String::new();
    text.push_str(&format!("\n   files {files}\n"));
    match &git.commit_message {
        Some(message) => text.push_str(&format!("  commit {message}\n")),
        None => text.push_str("  commit (none)\n"),
    }
    match &git.tag {
        Some(tag) => text.push_str(&format!("     tag {tag}\n")),
        None => text.push_str("     tag (none)\n"),
    }
    match &git.push.as_deref() {
        Some(remote) => text.push_str(&format!("    push yes ({remote})\n")),
        None => text.push_str("    push no\n"),
    }
    if plan.bumps.len() > 1 {
        text.push('\n');
        for bump in &plan.bumps {
            text.push_str(&format!(
                "    {} {} -> {}\n",
                ui.dim(&bump.name),
                bump.old,
                bump.new
            ));
        }
    } else {
        text.push_str(&format!(
            "\n    from {}\n      to {}\n",
            plan.old_version,
            ui.bold(plan.new_version.to_string())
        ));
    }
    text
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

fn display(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{Bump, FilePlan, Plan, VersionSource};
    use crate::semver::Version;
    use crate::toml_line::Edit;

    fn plan() -> Plan {
        Plan {
            root: std::path::PathBuf::from("/w"),
            old_version: Version::parse("0.0.2").unwrap(),
            new_version: Version::parse("0.0.3").unwrap(),
            source: VersionSource::WorkspacePackage,
            files: vec![FilePlan {
                path: std::path::PathBuf::from("/w/Cargo.toml"),
                display: "Cargo.toml".to_string(),
                edits: vec![
                    Edit {
                        line: 5,
                        inner: (11, 16),
                        old: "0.0.2".to_string(),
                        new: "0.0.3".to_string(),
                        label: "[workspace.package]".to_string(),
                    },
                    Edit {
                        line: 20,
                        inner: (36, 41),
                        old: "0.0.2".to_string(),
                        new: "0.0.3".to_string(),
                        label: "[workspace.dependencies]".to_string(),
                    },
                ],
                new_text: String::new(),
            }],
            lockfile: Some(std::path::PathBuf::from("/w/Cargo.lock")),
            warnings: Vec::new(),
            notes: Vec::new(),
            bumps: vec![Bump {
                name: "a".to_string(),
                manifest_path: std::path::PathBuf::from("/w/Cargo.toml"),
                old: Version::parse("0.0.2").unwrap(),
                new: Version::parse("0.0.3").unwrap(),
            }],
        }
    }

    #[test]
    fn the_confirmation_block_looks_like_the_design() {
        let ui = Ui::new(false, false);
        let git = GitSummary {
            commit_message: Some("chore: release v0.0.3".to_string()),
            tag: Some("v0.0.3".to_string()),
            push: Some("origin".to_string()),
            branch: Some("main".to_string()),
        };
        let text = confirmation_text(&ui, &plan(), &git);
        assert!(text.contains("   files Cargo.toml\n"));
        assert!(text.contains("  commit chore: release v0.0.3\n"));
        assert!(text.contains("     tag v0.0.3\n"));
        assert!(text.contains("    push yes (origin)\n"));
        assert!(text.contains("\n    from 0.0.2\n      to 0.0.3\n"));
    }

    #[test]
    fn quiet_hides_the_plan() {
        let ui = Ui::new(true, false);
        let git = GitSummary::default();
        // Nothing to assert about stdout here beyond "it does not panic":
        print_plan(&ui, &plan(), "bumpp", &git);
    }
}
