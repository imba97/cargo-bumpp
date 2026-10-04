//! The interactive selector and the confirmation.
//!
//! Two renderings of the same list: an arrow-key selector when both stdin and
//! stdout are terminals and single-key mode could be enabled, and a numbered
//! list otherwise. The list itself is the reference implementation's, including
//! the fact that `conventional-prerelease` is *labelled* `conventional` — the
//! two rows differ only by the version they show.
//!
//! This is the root of the module: the `Prompt` trait and its implementations,
//! the retry counter they share, and the re-exports that keep every item at the
//! path it had when this was a single file.

use crate::error::{Error, Result};
#[cfg(test)]
use crate::semver::Level;
use crate::semver::Version;

mod choice;
mod keys;
mod select;

#[cfg(test)]
pub(super) use choice::match_choice;
pub use choice::{choices, resolve_level};
pub use choice::{Choice, Selection};
pub use select::TerminalPrompt;
#[cfg(test)]
pub(super) use select::{Window, DEFAULT_ROW};

/// How the tool asks its questions. Implement it to drive the tool from a
/// library or a test.
pub trait Prompt {
    /// Can this prompt actually ask? When it cannot, the version selector fails
    /// instead of hanging on a terminal that is not there.
    fn available(&self) -> bool {
        true
    }

    /// Pick a release. `current` is shown in the header.
    fn select_release(&mut self, current: &Version, choices: &[Choice]) -> Result<Selection>;

    /// Ask `Bump?`. `false` means the user declined.
    fn confirm(&mut self, summary: &str) -> Result<bool>;
}

/// A prompt that refuses to ask: the honest answer when input or output is
/// unavailable.
#[derive(Debug, Default, Clone)]
pub struct RefusePrompt;

impl Prompt for RefusePrompt {
    fn available(&self) -> bool {
        false
    }

    fn select_release(&mut self, _current: &Version, _choices: &[Choice]) -> Result<Selection> {
        Err(prompt_unavailable())
    }

    fn confirm(&mut self, summary: &str) -> Result<bool> {
        // Print the summary anyway: it explains what the run would have done.
        println!("{summary}");
        Err(
            Error::check("cannot ask for confirmation because input is not available")
                .with_hint("pass -y/--yes to skip the confirmation"),
        )
    }
}

/// The message the reference implementation uses when prompting is impossible.
pub fn prompt_unavailable() -> Error {
    Error::check("Cannot prompt for the version number because input or output has been disabled.")
        .with_hint("pass a level such as `patch`, or `--release <level>`")
}

/// Answers fed in ahead of time. Handy for tests and for library callers.
#[derive(Debug, Default, Clone)]
pub struct ScriptedPrompt {
    pub releases: Vec<Selection>,
    pub confirmations: Vec<bool>,
    /// What the user would type at the `custom …` row.
    pub customs: Vec<String>,
}

impl ScriptedPrompt {
    pub fn new(releases: Vec<Selection>, confirmations: Vec<bool>) -> Self {
        ScriptedPrompt {
            releases,
            confirmations,
            customs: Vec::new(),
        }
    }
}

impl Prompt for ScriptedPrompt {
    fn select_release(&mut self, _current: &Version, _choices: &[Choice]) -> Result<Selection> {
        if self.releases.is_empty() {
            return Err(Error::cancelled());
        }
        Ok(self.releases.remove(0))
    }

    fn confirm(&mut self, _summary: &str) -> Result<bool> {
        if self.confirmations.is_empty() {
            return Ok(true);
        }
        Ok(self.confirmations.remove(0))
    }
}

/// Tracks how many invalid answers a prompt has received.
///
/// Three prompt loops (`confirm`, `select_with_numbers`, `ask_version`) all
/// give the user three tries before erroring out — the previous code repeated
/// the `attempts += 1; if attempts >= 3 { ... }` pattern in each. This struct
/// keeps the loop body focused on what changes between sites (the hint text)
/// and what to do when the limit is reached (build the `Error`).
pub(super) struct Attempts {
    count: usize,
    limit: usize,
}

impl Attempts {
    pub(super) fn new(limit: usize) -> Self {
        Self { count: 0, limit }
    }
    /// Record one invalid answer. Returns `true` while the caller should keep
    /// trying, `false` once the limit is reached.
    pub(super) fn invalid(&mut self) -> bool {
        self.count += 1;
        self.count < self.limit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    #[test]
    fn the_list_is_the_reference_list() {
        let rows = choices(&v("1.2.0"), "beta", Level::Patch);
        let rendered: Vec<String> = rows.iter().map(Choice::render).collect();
        assert_eq!(
            rendered,
            [
                "        major 2.0.0",
                "        minor 1.3.0",
                "        patch 1.2.1",
                "         next 1.2.1",
                " conventional 1.2.1",
                " conventional 1.2.1-beta.1",
                "    pre-patch 1.2.1-beta.1",
                "    pre-minor 1.3.0-beta.1",
                "    pre-major 2.0.0-beta.1",
                "        as-is 1.2.0",
                // `custom ...` is padded as a whole, not just the word
                "       custom ...",
            ]
        );
    }

    #[test]
    fn the_default_row_is_next() {
        let rows = choices(&v("1.2.0"), "beta", Level::Patch);
        assert_eq!(rows[DEFAULT_ROW].label, "next");
        assert_eq!(
            rows[DEFAULT_ROW].version.as_ref().unwrap().to_string(),
            "1.2.1"
        );
    }

    #[test]
    fn conventional_level_moves_the_conventional_rows() {
        let rows = choices(&v("1.2.0"), "beta", Level::Minor);
        assert_eq!(rows[4].version.as_ref().unwrap().to_string(), "1.3.0");
        assert_eq!(
            rows[5].version.as_ref().unwrap().to_string(),
            "1.3.0-beta.1"
        );
    }

    #[test]
    fn pre_release_lines_keep_counting() {
        // 1.2.1-beta.1: `next` and `conventional-prerelease` both count up
        let rows = choices(&v("1.2.1-beta.1"), "beta", Level::Minor);
        assert_eq!(
            rows[3].version.as_ref().unwrap().to_string(),
            "1.2.1-beta.2"
        );
        assert_eq!(
            rows[5].version.as_ref().unwrap().to_string(),
            "1.2.1-beta.2"
        );
        // ... and `pre-patch` still moves the base
        assert_eq!(
            rows[6].version.as_ref().unwrap().to_string(),
            "1.2.2-beta.1"
        );
    }

    #[test]
    fn a_foreign_preid_is_inherited_for_pre_releases() {
        let rows = choices(&v("1.2.1-rc.3"), "beta", Level::Patch);
        assert_eq!(rows[6].version.as_ref().unwrap().to_string(), "1.2.2-rc.1");
    }

    #[test]
    fn resolve_level_handles_the_dependent_levels() {
        assert_eq!(
            resolve_level(&Level::Conventional, &v("1.2.0"), Level::Minor),
            Level::Minor
        );
        assert_eq!(
            resolve_level(&Level::ConventionalPrerelease, &v("1.2.0"), Level::Minor),
            Level::PreMinor
        );
        assert_eq!(
            resolve_level(
                &Level::ConventionalPrerelease,
                &v("1.2.1-beta.1"),
                Level::Minor
            ),
            Level::PreRelease
        );
        assert_eq!(
            resolve_level(&Level::Patch, &v("1.2.0"), Level::Major),
            Level::Patch
        );
    }

    #[test]
    fn choices_match_by_number_and_by_name() {
        let rows = choices(&v("1.2.0"), "beta", Level::Patch);
        assert_eq!(match_choice(&rows, "1"), Some(0));
        assert_eq!(match_choice(&rows, "4"), Some(3));
        assert_eq!(match_choice(&rows, "11"), Some(10));
        assert_eq!(match_choice(&rows, "12"), None);
        assert_eq!(match_choice(&rows, "major"), Some(0));
        // a prefix matches the first row that has it: `pre-m` is `pre-minor`
        assert_eq!(match_choice(&rows, "pre-m"), Some(7));
        assert_eq!(match_choice(&rows, "pre-ma"), Some(8));
        assert_eq!(match_choice(&rows, "custom"), Some(10));
        assert_eq!(match_choice(&rows, "conventional"), Some(4));
        assert_eq!(match_choice(&rows, "nope"), None);
    }

    #[test]
    fn the_row_under_the_cursor_is_highlighted() {
        let rows = choices(&v("1.2.0"), "beta", Level::Patch);
        let plain = TerminalPrompt::new(false, false, false);
        let colored = TerminalPrompt::new(true, false, false);

        // Colour off: the row is exactly the marker plus the plain rendering,
        // which is what also goes into a pipe or a `--quiet` run.
        assert_eq!(
            plain.render_row(&rows[3], true),
            format!("> {}", rows[3].render())
        );
        assert_eq!(
            plain.render_row(&rows[0], false),
            format!("  {}", rows[0].render())
        );
        assert!(!plain.render_row(&rows[3], true).contains('\u{1b}'));

        // Colour on: the selected row stands out, the rest are dimmed.
        let selected = colored.render_row(&rows[3], true);
        assert!(selected.starts_with("\u{1b}[1;36m> "), "{selected:?}");
        assert!(selected.ends_with("\u{1b}[0m"), "{selected:?}");
        assert!(selected.contains("next 1.2.1"), "{selected:?}");
        let other = colored.render_row(&rows[0], false);
        assert!(other.starts_with("\u{1b}[2m  "), "{other:?}");
        assert!(other.ends_with("\u{1b}[0m"), "{other:?}");
        assert!(other.contains("major 2.0.0"), "{other:?}");
    }

    #[test]
    fn the_picked_row_is_reported_with_the_current_version() {
        let mut prompt = TerminalPrompt::new(true, false, false);
        let rows = choices(&v("1.2.0"), "beta", Level::Patch);
        // `finish` prints and returns; the level it hands back is the row's
        let selection = prompt.finish(&v("1.2.0"), &rows, 3, 0).unwrap();
        assert_eq!(selection, Some(Selection::Level(Level::Next)));
    }

    #[test]
    fn a_list_that_fits_shows_everything_and_says_nothing() {
        let rows = choices(&v("1.2.0"), "beta", Level::Patch);
        // 11 rows plus 3 lines of chrome need a 14-row terminal
        let prompt = TerminalPrompt::new(false, false, false).with_rows(20);
        let lines = prompt.menu_lines(&v("1.2.0"), &rows, 0);
        assert_eq!(lines.len(), 2 + rows.len());
        assert!(
            !lines
                .iter()
                .any(|line| line.contains("↑ ...") || line.contains("↓ ...")),
            "{lines:?}"
        );
    }

    #[test]
    fn a_list_that_does_not_fit_scrolls_with_an_ellipsis_at_the_bottom() {
        let rows = choices(&v("1.2.0"), "beta", Level::Patch); // 11 rows
                                                               // 8 rows of terminal - 3 lines of chrome = 5 lines: 4 items + the `...`
        let prompt = TerminalPrompt::new(false, false, false).with_rows(8);
        let cursor_row = |lines: &[String]| {
            lines
                .iter()
                .find(|line| line.starts_with("> "))
                .cloned()
                .unwrap_or_default()
        };

        let lines = prompt.menu_lines(&v("1.2.0"), &rows, 0);
        assert_eq!(lines.len(), 2 + 5, "{lines:?}");
        assert!(cursor_row(&lines).contains("major"), "{lines:?}");
        assert!(
            lines[5].contains("next"),
            "four rows are visible: {lines:?}"
        );
        assert_eq!(lines[6], "  ↓ ...", "{lines:?}");

        // The cursor sits on the second-to-last visible row without scrolling ...
        let at_second_to_last = prompt.menu_lines(&v("1.2.0"), &rows, 2);
        assert!(
            cursor_row(&at_second_to_last).contains("patch"),
            "{at_second_to_last:?}"
        );
        assert!(
            !at_second_to_last.iter().any(|line| line.contains('↑')),
            "the window has not moved yet: {at_second_to_last:?}"
        );

        // ... and one row further down the window moves instead of the cursor.
        let scrolled = prompt.menu_lines(&v("1.2.0"), &rows, 3);
        assert_eq!(scrolled[2], "  ↑ ...", "{scrolled:?}");
        assert!(
            cursor_row(&scrolled).contains("next"),
            "the cursor stayed on the second-to-last row: {scrolled:?}"
        );
        // Once the window has moved, the `↑` line is there as well: the menu is
        // one line taller in the middle of a long list, and shrinks again at the
        // very end. `draw_menu` tracks the line count, so the redraw stays put.
        assert_eq!(scrolled.len(), 2 + 6, "{scrolled:?}");
        assert_eq!(scrolled.last().unwrap(), "  ↓ ...", "{scrolled:?}");
    }

    #[test]
    fn the_end_of_the_list_is_reachable_without_a_marker() {
        let rows = choices(&v("1.2.0"), "beta", Level::Patch);
        let prompt = TerminalPrompt::new(false, false, false).with_rows(8);
        let lines = prompt.menu_lines(&v("1.2.0"), &rows, rows.len() - 1);

        assert_eq!(lines[2], "  ↑ ...", "{lines:?}");
        assert!(
            !lines.iter().any(|line| line.contains('↓')),
            "nothing below the last row: {lines:?}"
        );
        assert!(
            lines.last().unwrap().contains("custom"),
            "the last row is shown: {lines:?}"
        );
        // the cursor is on a real row, and it is the picked one
        assert!(lines.iter().any(|line| line.starts_with("> ")), "{lines:?}");
    }

    #[test]
    fn the_scrolled_menu_reads_like_this() {
        // A spec of the layout, spacing included, for an 8-line terminal with
        // the cursor three rows down. Colour off, so the text is the text.
        let rows = choices(&v("1.2.0"), "beta", Level::Patch);
        let prompt = TerminalPrompt::new(false, false, false).with_rows(8);
        let lines = prompt.menu_lines(&v("1.2.0"), &rows, 3);
        assert_eq!(
            lines,
            [
                "? Current version 1.2.0 »",
                "  up/down (or j/k), Enter to pick, Ctrl+C to cancel",
                "  ↑ ...",
                "          minor 1.3.0",
                "          patch 1.2.1",
                ">          next 1.2.1",
                "   conventional 1.2.1",
                "  ↓ ...",
            ]
        );
    }

    #[test]
    fn the_window_keeps_one_row_of_look_ahead() {
        // capacity 5 -> 4 item rows, the fifth line being the bottom marker
        let window = Window::around(0, 5, 11);
        assert_eq!((window.start, window.end), (0, 4));
        assert!(!window.more_above && window.more_below);

        // the cursor reaches the second-to-last visible row: still no scrolling
        let window = Window::around(2, 5, 11);
        assert_eq!((window.start, window.end), (0, 4));

        // one further down, and the window moves instead of the cursor
        let window = Window::around(3, 5, 11);
        assert_eq!((window.start, window.end), (1, 5));

        // at the end the bottom marker is gone, so the last row is usable
        let window = Window::around(10, 5, 11);
        assert_eq!((window.start, window.end), (7, 11));
        assert!(window.more_above && !window.more_below);

        // a short terminal still gets a usable window
        let window = Window::around(0, 3, 11);
        assert_eq!((window.start, window.end), (0, 2));
        assert!(window.more_below);

        // and a list that fits never scrolls
        let window = Window::around(4, 11, 11);
        assert_eq!((window.start, window.end), (0, 11));
        assert!(!window.more_above && !window.more_below);
    }

    #[test]
    fn scripted_prompt_hands_out_the_answers() {
        let mut prompt = ScriptedPrompt::new(vec![Selection::Level(Level::Minor)], vec![false]);
        let rows = choices(&v("1.2.0"), "beta", Level::Patch);
        assert_eq!(
            prompt.select_release(&v("1.2.0"), &rows).unwrap(),
            Selection::Level(Level::Minor)
        );
        assert!(!prompt.confirm("summary").unwrap());
    }
}
