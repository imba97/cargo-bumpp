//! The real terminal selector: the arrow-key menu, the numbered fallback, and
//! the `Bump?` question.
//!
//! It is its own file because it is the only part of the prompt that talks to a
//! terminal, and because the layout it draws — the window that scrolls, and the
//! redraw bookkeeping — is large enough to deserve its own place next to the
//! pure list in [`super::choice`].

use std::io::Write;

use super::choice::{match_choice, Choice, Selection};
use super::keys::{read_key, read_line, read_typed_line, Key};
use super::{prompt_unavailable, Attempts, Prompt};
use crate::error::{Error, Result};
use crate::report::ansi;
use crate::semver::Version;
use crate::sys;

/// What picking a row decided.
///
/// The `custom …` row cannot answer itself where the key loop reads it: that loop
/// runs with the terminal in single-key mode, where the console neither echoes
/// what is typed nor assembles it into a line. Restoring line mode before asking
/// is not a way out either — a console switched to single-key mode is not
/// guaranteed to go back to line input (a pseudoconsole keeps handing over raw
/// bytes), and then Enter and Ctrl+C are swallowed. So the row hands the question
/// back as a value, and [`TerminalPrompt::select_release`] asks it with
/// `read_typed_line`, which does its own echoing — which is also why this is an
/// enum rather than an `Option` with a second meaning.
#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    /// A row was picked: that is the whole answer.
    Picked(Selection),
    /// The `custom …` row: a version still has to be typed in.
    Custom,
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
/// The row the cursor starts on.
pub const DEFAULT_ROW: usize = 3; // `next`

/// The slice of the list that is on screen, with a look-ahead of one row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    /// First visible index.
    pub start: usize,
    /// One past the last visible index.
    pub end: usize,
    /// There are rows above `start`: the first line says `...`.
    pub more_above: bool,
    /// There are rows below `end`: the last line says `...`.
    pub more_below: bool,
}

impl Window {
    /// Lay out `len` rows around `cursor` in `capacity` lines.
    ///
    /// One line is kept for the `...` that says "there is more below", and the
    /// cursor stops one row short of the bottom: that is where the window starts
    /// moving, so the next row is already visible by the time it is needed.
    pub(super) fn around(cursor: usize, capacity: usize, len: usize) -> Window {
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
                let answer = self.select_with_keys(current, choices);
                // The `custom …` row is answered with the terminal still in
                // single-key mode: the prompt echoes and edits the version itself
                // (`read_typed_line`), so nothing here depends on the console
                // going back to line input. The mode is only left behind once the
                // whole question has been answered.
                let result = match answer {
                    Ok(Answer::Picked(selection)) => Ok(selection),
                    Ok(Answer::Custom) => {
                        ask_version_in_key_mode(current).and_then(custom_selection)
                    }
                    Err(err) => Err(err),
                };
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
    pub(super) fn select_with_keys(
        &mut self,
        current: &Version,
        choices: &[Choice],
    ) -> Result<Answer> {
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
                    return self.finish(current, choices, cursor, drawn);
                }
                // Either a row is picked and the menu is over, or the custom row
                // asks for a version — which `select_release` asks on this same
                // single-key prompt. Both end the loop.
                Key::Enter => return self.finish(current, choices, cursor, drawn),
                Key::Escape | Key::Interrupt => return Err(Error::cancelled()),
                _ => {}
            }
        }
    }

    /// Enter was pressed: either a row is picked, or the `custom …` row asks for
    /// a version.
    ///
    /// The asking is *not* done here: [`Answer::Custom`] is handed back to
    /// [`Prompt::select_release`], which asks on the single-key prompt that can
    /// actually read it. Reading a whole line in this mode — what this used to do
    /// — is what made the prompt swallow what was typed, ignore Enter and never
    /// cancel.
    pub(super) fn finish(
        &mut self,
        current: &Version,
        choices: &[Choice],
        cursor: usize,
        drawn: usize,
    ) -> Result<Answer> {
        let choice = &choices[cursor];
        self.clear_menu(drawn);
        match &choice.version {
            Some(version) => {
                // The picked row keeps the highlight, so the answer is easy to spot.
                println!(
                    "? Current version {} » {}",
                    self.paint("32", &current.to_string()),
                    self.paint("1;36", &format!("{} {version}", choice.label))
                );
                Ok(Answer::Picked(Selection::Level(choice.level.clone())))
            }
            None => {
                println!(
                    "? Current version {} » custom",
                    self.paint("32", &current.to_string())
                );
                Ok(Answer::Custom)
            }
        }
    }

    pub(super) fn draw_menu(
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
    pub(super) fn menu_lines(
        &self,
        current: &Version,
        choices: &[Choice],
        cursor: usize,
    ) -> Vec<String> {
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
    pub(super) fn render_row(&self, choice: &Choice, selected: bool) -> String {
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
                        return ask_version(current).and_then(custom_selection);
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

/// Append one line to a redraw buffer, clearing the old line first when the
/// buffer is rewriting a menu that is already on screen.
fn push_line(out: &mut String, clear: bool, text: &str) {
    if clear {
        out.push_str("\r\u{1b}[2K");
    }
    out.push_str(text);
    out.push('\n');
}

/// The `custom …` row: the version it asks for, or a cancellation.
fn custom_selection(version: Option<Version>) -> Result<Selection> {
    match version {
        Some(version) => Ok(Selection::Version(version)),
        None => Err(Error::cancelled()),
    }
}

/// Ask for the version on the numbered list's line-based prompt.
fn ask_version(current: &Version) -> Result<Option<Version>> {
    ask_version_with(current, read_line)
}

/// Ask for the version while the terminal is in single-key mode: the reader does
/// the echoing and the editing, so both prompts only share the question.
fn ask_version_in_key_mode(current: &Version) -> Result<Option<Version>> {
    ask_version_with(current, read_typed_line)
}

/// Ask for a version by hand, validating it like the reference does.
///
/// Three tries, then a usage error — and an empty answer, a closed input or Ctrl+C
/// all mean the same thing: nothing was chosen.
fn ask_version_with(
    current: &Version,
    read: impl Fn() -> Result<Option<String>>,
) -> Result<Option<Version>> {
    let mut attempts = Attempts::new(3);
    loop {
        print!("  version (current {current}) > ");
        std::io::stdout().flush().ok();
        let Some(line) = read()? else {
            return Ok(None);
        };
        let text = line.trim();
        if text.is_empty() {
            return Ok(None);
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
