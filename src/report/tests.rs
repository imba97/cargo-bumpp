//! Tests for the report rendering: the confirmation block, the quiet flag, and
//! the colour escapes.

use super::render::format_step_line;
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

#[test]
fn colors_are_kept_when_a_tty_supports_them() {
    // `confirmation_text` returns a `String`, so colour assertions don't
    // need a real terminal. `print_plan` writes via `println!` instead,
    // which is exercised by the integration suite (where the suite's own
    // `RUN` output is compared as text, ignoring ANSI).
    let ui = Ui::new(false, true);
    let git = GitSummary {
        commit_message: Some("chore: release v0.0.3".to_string()),
        tag: Some("v0.0.3".to_string()),
        push: Some("origin".to_string()),
        branch: Some("main".to_string()),
    };
    let confirm = confirmation_text(&ui, &plan(), &git);

    // The new version is rendered in bold + green: `[1;32m0.0.3[0m`.
    assert!(
        confirm.contains("      to \u{1b}[1;32m0.0.3\u{1b}[0m"),
        "the `to` line should be bold+green: {confirm:?}"
    );
    // `files Cargo.toml` → filename in bold.
    assert!(
        confirm.contains("\u{1b}[1mCargo.toml\u{1b}[0m"),
        "files should be bold: {confirm:?}"
    );
    // `push yes` → bold + cyan.
    assert!(
        confirm.contains("\u{1b}[1;36myes\u{1b}[0m"),
        "push 'yes' should be bold+cyan: {confirm:?}"
    );
    // `from 0.0.2` → bold (old version, no colour shift).
    assert!(
        confirm.contains("    from \u{1b}[1m0.0.2\u{1b}[0m"),
        "the `from` line should be bold: {confirm:?}"
    );
}

#[test]
fn step_lines_align_their_values() {
    // Every label in the `apply` stage fits within
    // `APPLY_LABEL_WIDTH`; once padded the values share a column.
    let ui = Ui::new(false, false);

    let short = format_step_line(&ui, "tag", "v1.0");
    let long_ = format_step_line(&ui, "updated", "Cargo.lock");

    // Without colour escapes (Ui::new(_, false)), the padded labels read
    // exactly as: `tag<two spaces>v1.0`, `updated<v1.0`.
    assert_eq!(
        short, "  tag     v1.0",
        "short label padded to width: {short:?}"
    );
    assert_eq!(
        long_, "  updated Cargo.lock",
        "long label needs no padding: {long_:?}"
    );

    // The value column starts at the same byte on both lines.
    let value_col_short = short.find("v1.0").unwrap();
    let value_col_long = long_.find("Cargo.lock").unwrap();
    assert_eq!(
        value_col_short, value_col_long,
        "values must align: {short:?} vs {long_:?}"
    );
}
