//! The output sink: the `Ui` colour and quiet switch, plus the `ansi` helper
//! every styled line goes through.
//!
//! It is separate from what the reports say: colour is decided once here, so a
//! change to an escape sequence or to the quiet rule happens in one place.

use super::render::format_step_line;

/// Wrap `text` in an ANSI escape sequence when `color` is on; otherwise pass
/// it through. The single place every colour call in the tool goes through —
/// `Ui::paint` and `TerminalPrompt::paint` both delegate here so a change to
/// the escape sequence (or to a future TTY-detection rule) only happens once.
pub fn ansi(code: &str, color: bool, text: &str) -> String {
    if color {
        format!("\u{1b}[{code}m{text}\u{1b}[0m")
    } else {
        text.to_string()
    }
}

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
        ansi(code, self.color, text)
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

    /// Bold + green: a version or value the run is about to land on.
    pub fn bold_green(&self, text: impl AsRef<str>) -> String {
        self.paint("1;32", text.as_ref())
    }

    /// Bold + cyan: a positive confirmation (`yes`, a URL).
    pub fn bold_cyan(&self, text: impl AsRef<str>) -> String {
        self.paint("1;36", text.as_ref())
    }

    /// Ordinary progress output — hidden by `--quiet`.
    pub fn info(&self, text: impl AsRef<str>) {
        if !self.quiet {
            println!("{}", text.as_ref());
        }
    }

    /// Width reserved for the label column in the `apply` stage's progress lines:
    /// every label is padded to this many characters so the values line up.
    pub const APPLY_LABEL_WIDTH: usize = 7;

    /// One progress line: `label  value`, with the label dimmed. Every line in
    /// the `apply` stage goes through here so the labels align.
    ///
    /// `label` is left-padded with spaces to
    /// [`APPLY_LABEL_WIDTH`](Self::APPLY_LABEL_WIDTH); the dim styling wraps
    /// the padded form so the value column starts at the same byte on every
    /// line.
    pub fn step(&self, label: &str, value: impl AsRef<str>) {
        if self.quiet {
            return;
        }
        let line = format_step_line(self, label, value.as_ref());
        println!("{line}");
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
