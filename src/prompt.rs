//! The interactive selector and the confirmation.
//!
//! Two renderings of the same list: an arrow-key selector when both stdin and
//! stdout are terminals and single-key mode could be enabled, and a numbered
//! list otherwise. The list itself is the reference implementation's, including
//! the fact that `conventional-prerelease` is *labelled* `conventional` — the
//! two rows differ only by the version they show.

use std::io::{BufRead, Read, Write};

use crate::error::{Error, Result};
use crate::plan::increment;
use crate::report::ansi;
use crate::semver::{Level, Version};
use crate::sys;

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

/// Append one line to a redraw buffer, clearing the old line first when the
/// buffer is rewriting a menu that is already on screen.
fn push_line(out: &mut String, clear: bool, text: &str) {
    if clear {
        out.push_str("\r\u{1b}[2K");
    }
    out.push_str(text);
    out.push('\n');
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
/// The row the cursor starts on.
const DEFAULT_ROW: usize = 3; // `next`

/// Tracks how many invalid answers a prompt has received.
///
/// Three prompt loops (`confirm`, `select_with_numbers`, `ask_version`) all
/// give the user three tries before erroring out — the previous code repeated
/// the `attempts += 1; if attempts >= 3 { ... }` pattern in each. This struct
/// keeps the loop body focused on what changes between sites (the hint text)
/// and what to do when the limit is reached (build the `Error`).
struct Attempts {
    count: usize,
    limit: usize,
}

impl Attempts {
    fn new(limit: usize) -> Self {
        Self { count: 0, limit }
    }
    /// Record one invalid answer. Returns `true` while the caller should keep
    /// trying, `false` once the limit is reached.
    fn invalid(&mut self) -> bool {
        self.count += 1;
        self.count < self.limit
    }
}

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

/// The real thing: keys on a terminal, lines when there is no terminal.
pub struct TerminalPrompt {
    /// Whether ANSI escape sequences may be written.
    pub color: bool,
    /// `-y`: never ask for confirmation.
    pub skip_confirm: bool,
    /// `--quiet`: the confirmation block is part of the run's normal output and
    /// is hidden by `--quiet`, the same way the plan and the per-step lines are.
    pub quiet: bool,
    /// Terminal height to lay the selector out for; `None` asks the OS.
    pub rows: Option<usize>,
}

/// Lines the selector spends on things other than the list itself: the
/// `Current version` header, the key hint, and one line of slack so the menu
/// does not end flush against the bottom of the window.
const MENU_CHROME: usize = 3;
/// Even on a very short terminal, show at least this many rows.
const MIN_VISIBLE_ROWS: usize = 3;

/// The slice of the list that is on screen, with a look-ahead of one row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Window {
    /// First visible index.
    start: usize,
    /// One past the last visible index.
    end: usize,
    /// There are rows above `start`: the first line says `...`.
    more_above: bool,
    /// There are rows below `end`: the last line says `...`.
    more_below: bool,
}

impl Window {
    /// Lay out `len` rows around `cursor` in `capacity` lines.
    ///
    /// One line is kept for the `...` that says "there is more below", and the
    /// cursor stops one row short of the bottom: that is where the window starts
    /// moving, so the next row is already visible by the time it is needed.
    fn around(cursor: usize, capacity: usize, len: usize) -> Window {
        if capacity == 0 || len == 0 {
            return Window {
                start: 0,
                end: 0,
                more_above: false,
                more_below: false,
            };
        }
        if len <= capacity {
            return Window {
                start: 0,
                end: len,
                more_above: false,
                more_below: false,
            };
        }

        let rows = capacity - 1; // one line goes to the bottom `...`
        let max_start = len - rows;
        let start = cursor.saturating_sub(rows.saturating_sub(2)).min(max_start);
        let end = (start + rows).min(len);
        Window {
            start,
            end,
            more_above: start > 0,
            more_below: end < len,
        }
    }
}

impl TerminalPrompt {
    pub fn new(color: bool, skip_confirm: bool, quiet: bool) -> Self {
        TerminalPrompt {
            color,
            skip_confirm,
            quiet,
            rows: None,
        }
    }

    /// Lay the menu out for a given terminal height, instead of asking the OS.
    pub fn with_rows(mut self, rows: usize) -> Self {
        self.rows = Some(rows);
        self
    }

    /// How many lines the list may use on this terminal.
    fn capacity(&self, len: usize) -> usize {
        match self.rows.or_else(sys::terminal_rows) {
            Some(rows) => rows
                .saturating_sub(MENU_CHROME)
                .max(MIN_VISIBLE_ROWS)
                .min(len),
            // No terminal to measure: show everything rather than guess.
            None => len,
        }
    }
}

impl Prompt for TerminalPrompt {
    fn select_release(&mut self, current: &Version, choices: &[Choice]) -> Result<Selection> {
        // The arrow-key selector needs escape sequences to work, so it is only
        // used when `color` is on (which is what enabling ANSI output reports).
        if self.color && sys::stdin_is_tty() && sys::stdout_is_tty() {
            if let Some(raw) = sys::RawMode::enable() {
                let result = self.select_with_keys(current, choices);
                drop(raw);
                println!();
                return result;
            }
        }
        self.select_with_numbers(current, choices)
    }

    fn confirm(&mut self, summary: &str) -> Result<bool> {
        // `--quiet` hides everything else; the confirmation block is output
        // too, so it is hidden here as well.
        if !self.quiet {
            println!("{summary}");
        }
        if self.skip_confirm {
            return Ok(true);
        }
        let mut attempts = Attempts::new(3);
        loop {
            print!("? Bump? (Y/n) ");
            std::io::stdout().flush().ok();
            let Some(line) = read_line()? else {
                return Err(Error::check(
                    "cannot ask for confirmation because input is not available",
                )
                .with_hint("pass -y/--yes to skip the confirmation"));
            };
            match line.trim().to_ascii_lowercase().as_str() {
                "" | "y" | "yes" => return Ok(true),
                "n" | "no" => return Ok(false),
                other => {
                    if !attempts.invalid() {
                        return Err(Error::usage(format!(
                            "`{other}` is not an answer to `Bump?` (expected y or n)"
                        )));
                    }
                    println!("  please answer `y` or `n`");
                }
            }
        }
    }
}

impl TerminalPrompt {
    fn select_with_keys(&mut self, current: &Version, choices: &[Choice]) -> Result<Selection> {
        if choices.is_empty() {
            return Err(Error::check("there is nothing to choose from"));
        }
        let mut cursor = DEFAULT_ROW.min(choices.len() - 1);
        let mut drawn = 0usize;
        loop {
            drawn = self.draw_menu(current, choices, cursor, drawn)?;
            match read_key()? {
                Key::Up | Key::Char('k') => cursor = cursor.saturating_sub(1),
                Key::Down | Key::Char('j') => {
                    if cursor + 1 < choices.len() {
                        cursor += 1;
                    }
                }
                Key::Home | Key::Char('g') => cursor = 0,
                Key::End | Key::Char('G') => cursor = choices.len() - 1,
                Key::Digit(digit) => {
                    // 1..=9 jump straight to a row, 0 is the tenth
                    let index = if digit == 0 { 9 } else { digit as usize - 1 };
                    if index < choices.len() {
                        cursor = index;
                    }
                    if let Some(selection) = self.finish(current, choices, cursor, drawn)? {
                        return Ok(selection);
                    }
                    drawn = 0;
                }
                Key::Enter => {
                    if let Some(selection) = self.finish(current, choices, cursor, drawn)? {
                        return Ok(selection);
                    }
                    drawn = 0;
                }
                Key::Escape | Key::Interrupt => return Err(Error::cancelled()),
                _ => {}
            }
        }
    }

    /// Enter was pressed: either a row is picked, or `custom …` asks for a
    /// version.
    fn finish(
        &mut self,
        current: &Version,
        choices: &[Choice],
        cursor: usize,
        drawn: usize,
    ) -> Result<Option<Selection>> {
        let choice = &choices[cursor];
        if let Some(version) = &choice.version {
            self.clear_menu(drawn);
            // The picked row keeps the highlight, so the answer is easy to spot.
            println!(
                "? Current version {} » {}",
                self.paint("32", &current.to_string()),
                self.paint("1;36", &format!("{} {version}", choice.label))
            );
            return Ok(Some(Selection::Level(choice.level.clone())));
        }

        self.clear_menu(drawn);
        println!(
            "? Current version {} » custom",
            self.paint("32", &current.to_string())
        );
        match ask_version(current)? {
            Some(version) => Ok(Some(Selection::Version(version))),
            None => Err(Error::cancelled()),
        }
    }

    fn draw_menu(
        &self,
        current: &Version,
        choices: &[Choice],
        cursor: usize,
        drawn: usize,
    ) -> Result<usize> {
        let redraw = drawn > 0;
        let mut out = String::new();
        if redraw {
            // Move back up over the previous rendering.
            out.push_str(&format!("\u{1b}[{drawn}A"));
        }
        let lines = self.menu_lines(current, choices, cursor);
        for line in &lines {
            push_line(&mut out, redraw, line);
        }
        print!("{out}");
        std::io::stdout().flush().ok();
        Ok(lines.len())
    }

    /// The whole menu, one string per line: header, hint, then as much of the
    /// list as the terminal has room for. Pure, so the layout can be tested
    /// without a terminal.
    fn menu_lines(&self, current: &Version, choices: &[Choice], cursor: usize) -> Vec<String> {
        let mut lines = Vec::with_capacity(choices.len() + 4);
        lines.push(format!(
            "? Current version {} »",
            self.paint("32", &current.to_string())
        ));
        lines.push(self.paint("2", "  up/down (or j/k), Enter to pick, Ctrl+C to cancel"));

        let window = Window::around(cursor, self.capacity(choices.len()), choices.len());
        if window.more_above {
            lines.push(self.render_more('↑'));
        }
        for (index, choice) in choices
            .iter()
            .enumerate()
            .take(window.end)
            .skip(window.start)
        {
            lines.push(self.render_row(choice, index == cursor));
        }
        if window.more_below {
            lines.push(self.render_more('↓'));
        }
        lines
    }

    /// The `...` line that says the list continues: an arrow for the direction,
    /// then the ellipsis the design asks for.
    fn render_more(&self, arrow: char) -> String {
        self.paint("2", &format!("  {arrow} ..."))
    }

    /// One row as it goes to the terminal: the row under the cursor is
    /// highlighted, the others are dimmed, which is what the reference's
    /// `prompts` does as well. With colour off (a pipe, `--quiet`, or a console
    /// that cannot do ANSI) this is exactly `Choice::render`.
    fn render_row(&self, choice: &Choice, selected: bool) -> String {
        let marker = if selected { ">" } else { " " };
        let row = format!("{marker} {}", choice.render());
        if !self.color {
            return row;
        }
        if selected {
            self.paint("1;36", &row)
        } else {
            self.paint("2", &row)
        }
    }

    /// Wrap `text` in an ANSI sequence, unless colour is off.
    fn paint(&self, code: &str, text: &str) -> String {
        ansi(code, self.color, text)
    }

    fn clear_menu(&self, drawn: usize) {
        if drawn == 0 {
            return;
        }
        let mut out = String::new();
        out.push_str(&format!("\u{1b}[{drawn}A"));
        for _ in 0..drawn {
            out.push_str("\r\u{1b}[2K\n");
        }
        out.push_str(&format!("\u{1b}[{drawn}A"));
        print!("{out}");
        std::io::stdout().flush().ok();
    }

    fn select_with_numbers(&mut self, current: &Version, choices: &[Choice]) -> Result<Selection> {
        println!("  Current version {current}");
        println!();
        for (index, choice) in choices.iter().enumerate() {
            println!("  {:>2}) {}", index + 1, choice.render());
        }
        println!();

        let default = DEFAULT_ROW.min(choices.len() - 1);
        let mut attempts = Attempts::new(3);
        loop {
            print!("  > ");
            std::io::stdout().flush().ok();
            let Some(line) = read_line()? else {
                return Err(prompt_unavailable());
            };
            let text = line.trim();
            if text.is_empty() {
                return Ok(Selection::Level(choices[default].level.clone()));
            }
            match match_choice(choices, text) {
                Some(index) => {
                    if choices[index].is_custom() {
                        return match ask_version(current)? {
                            Some(version) => Ok(Selection::Version(version)),
                            None => Err(Error::cancelled()),
                        };
                    }
                    return Ok(Selection::Level(choices[index].level.clone()));
                }
                None => {
                    if !attempts.invalid() {
                        return Err(Error::usage(format!(
                            "`{text}` is not one of the choices (give a number or a name)"
                        )));
                    }
                    println!(
                        "  please give a number between 1 and {}, or a name",
                        choices.len()
                    );
                }
            }
        }
    }
}

/// Find a row by number or by (unique prefix of a) label.
fn match_choice(choices: &[Choice], text: &str) -> Option<usize> {
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

/// Ask for a version by hand, validating it like the reference does.
fn ask_version(current: &Version) -> Result<Option<Version>> {
    let mut attempts = Attempts::new(3);
    loop {
        print!("  version (current {current}) > ");
        std::io::stdout().flush().ok();
        let Some(line) = read_line()? else {
            return Ok(None);
        };
        let text = line.trim();
        if text.is_empty() {
            return Err(Error::cancelled());
        }
        match Version::parse_lenient(text) {
            Ok(version) => return Ok(Some(version)),
            Err(err) => {
                if !attempts.invalid() {
                    return Err(Error::usage(format!("`{text}`: {err}")));
                }
                println!("  {err}");
            }
        }
    }
}

/// Read one line, or `None` at end of input (a closed or empty stdin).
fn read_line() -> Result<Option<String>> {
    let mut buffer = String::new();
    let read = std::io::stdin().lock().read_line(&mut buffer);
    match read {
        Ok(0) => Ok(None),
        Ok(_) => {
            // A terminal echoes what was typed; a pipe does not, so the prompt
            // would otherwise stay on the same line as the next output.
            if !sys::stdin_is_tty() {
                println!();
            }
            Ok(Some(buffer))
        }
        Err(err) => Err(Error::io(format!("cannot read input: {err}"))),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Key {
    Up,
    Down,
    Home,
    End,
    Enter,
    Escape,
    Interrupt,
    Digit(u8),
    Char(char),
    Other,
}

/// Read a single key. Arrow keys arrive as `ESC [ A` (VT input) or `0xE0 0x48`
/// (the legacy console encoding).
fn read_key() -> Result<Key> {
    let mut byte = [0u8; 1];
    let read = std::io::stdin().lock().read(&mut byte);
    match read {
        Ok(0) => return Ok(Key::Interrupt),
        Ok(_) => {}
        Err(err) => return Err(Error::io(format!("cannot read input: {err}"))),
    }
    match byte[0] {
        b'\r' | b'\n' => Ok(Key::Enter),
        0x03 => Ok(Key::Interrupt),
        0x1b => {
            let mut next = [0u8; 1];
            if std::io::stdin().lock().read(&mut next).unwrap_or(0) == 0 {
                return Ok(Key::Escape);
            }
            match next[0] {
                b'[' | b'O' => {
                    let mut third = [0u8; 1];
                    if std::io::stdin().lock().read(&mut third).unwrap_or(0) == 0 {
                        return Ok(Key::Escape);
                    }
                    Ok(match third[0] {
                        b'A' => Key::Up,
                        b'B' => Key::Down,
                        b'H' => Key::Home,
                        b'F' => Key::End,
                        _ => Key::Other,
                    })
                }
                b => Ok(legacy_arrow(b)),
            }
        }
        0xE0 => {
            let mut next = [0u8; 1];
            if std::io::stdin().lock().read(&mut next).unwrap_or(0) == 0 {
                return Ok(Key::Other);
            }
            Ok(legacy_arrow(next[0]))
        }
        b @ b'1'..=b'9' => Ok(Key::Digit(b - b'0')),
        b'0' => Ok(Key::Digit(0)),
        b => Ok(Key::Char(b as char)),
    }
}

fn legacy_arrow(byte: u8) -> Key {
    match byte {
        b'H' => Key::Up,
        b'P' => Key::Down,
        _ => Key::Other,
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
