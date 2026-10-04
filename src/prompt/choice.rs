//! The list of choices and how a level is resolved into the one that is applied.
//!
//! It is its own file because this is the model the rest of the prompt works
//! from: the twelve rows, their labels and versions, and the two levels whose
//! meaning depends on the commits or on the current version.

use crate::plan::increment;
use crate::semver::{Level, Version};

/// One row of the selector.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    /// The label as shown, e.g. `pre-patch`.
    pub label: &'static str,
    /// The level picking this row means.
    pub level: Level,
    /// The version it would produce. `None` for `custom …`.
    pub version: Option<Version>,
}

impl Choice {
    /// The row as printed: label right-aligned, then the version.
    pub fn render(&self) -> String {
        match &self.version {
            Some(version) => format!("{:>width$} {}", self.label, version, width = LABEL_WIDTH),
            None => format!("{:>width$}", "custom ...", width = CUSTOM_WIDTH),
        }
    }
}

/// What the user picked.
#[derive(Debug, Clone, PartialEq)]
pub enum Selection {
    /// A row of the list.
    Level(Level),
    /// The `custom …` row, with the version typed in.
    Version(Version),
}

/// Width the labels are aligned to (the reference uses 13).
const LABEL_WIDTH: usize = 13;
/// Width of the `custom …` row.
const CUSTOM_WIDTH: usize = LABEL_WIDTH + 4;

/// The twelve rows, in the reference implementation's order.
pub fn choices(current: &Version, preid: &str, conventional: Level) -> Vec<Choice> {
    let conventional_level = match conventional {
        Level::Major | Level::Minor | Level::Patch => conventional,
        _ => Level::Patch,
    };
    let levels: [(&'static str, Level); 10] = [
        ("major", Level::Major),
        ("minor", Level::Minor),
        ("patch", Level::Patch),
        ("next", Level::Next),
        ("conventional", Level::Conventional),
        // shown as `conventional` as well: it is the same decision, landed on a
        // pre-release
        ("conventional", Level::ConventionalPrerelease),
        ("pre-patch", Level::PrePatch),
        ("pre-minor", Level::PreMinor),
        ("pre-major", Level::PreMajor),
        ("as-is", Level::AsIs),
    ];

    let mut rows = Vec::with_capacity(levels.len() + 1);
    for (label, level) in levels {
        let resolved = resolve_level(&level, current, conventional_level.clone());
        let version = increment(current, &resolved, preid).ok();
        rows.push(Choice {
            label,
            level,
            version,
        });
    }
    rows.push(Choice {
        label: "custom",
        level: Level::Prompt,
        version: None,
    });
    rows
}

/// Turn a level into the one that is actually applied.
///
/// Two levels depend on more than their own name:
///
/// * `conventional` becomes `major`/`minor`/`patch`, decided from the commits.
/// * `conventional-prerelease` becomes a plain pre-release bump when the current
///   version is already a pre-release, so a pre-release line keeps counting
///   instead of jumping to the next base version.
pub fn resolve_level(level: &Level, current: &Version, conventional: Level) -> Level {
    match level {
        Level::Conventional => conventional,
        Level::ConventionalPrerelease => {
            if current.is_prerelease() {
                Level::PreRelease
            } else {
                match conventional {
                    Level::Major => Level::PreMajor,
                    Level::Minor => Level::PreMinor,
                    _ => Level::PrePatch,
                }
            }
        }
        other => other.clone(),
    }
}

impl Choice {
    /// The `custom …` row is the only one without a version.
    pub fn is_custom(&self) -> bool {
        self.version.is_none()
    }
}

/// Find a row by number or by (unique prefix of a) label.
pub fn match_choice(choices: &[Choice], text: &str) -> Option<usize> {
    if let Ok(number) = text.parse::<usize>() {
        if (1..=choices.len()).contains(&number) {
            return Some(number - 1);
        }
        return None;
    }
    let needle = text.to_ascii_lowercase();
    choices
        .iter()
        .position(|choice| choice.label.eq_ignore_ascii_case(&needle))
        .or_else(|| {
            choices
                .iter()
                .position(|choice| choice.label.starts_with(&needle))
        })
}
