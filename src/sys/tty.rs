//! Terminal detection: whether stdin and stdout are terminals, and how many rows
//! the window has.
//!
//! It is a file of its own because these are the questions the rest of the crate
//! asks before it decides to colour output or to lay a list out to fit; the
//! raw-mode and ANSI plumbing that follows from them lives next door.

use std::io::IsTerminal;

#[cfg(windows)]
use super::win;

/// Is stdin a terminal we can read single keys from?
pub fn stdin_is_tty() -> bool {
    std::io::stdin().is_terminal()
}

/// Is stdout a terminal (as opposed to a pipe or a file)?
pub fn stdout_is_tty() -> bool {
    std::io::stdout().is_terminal()
}

#[cfg(windows)]
fn windows_terminal_rows() -> Option<usize> {
    unsafe {
        let mut info = win::ConsoleScreenBufferInfo::default();
        if win::GetConsoleScreenBufferInfo(win::stdout_handle(), &mut info) == 0 {
            return None;
        }
        // The visible window, not the scrollback buffer.
        let rows = info.window.bottom - info.window.top + 1;
        (rows > 0).then_some(rows as usize)
    }
}

/// How many lines the terminal shows, so a list can be laid out to fit.
///
/// Returns `None` when there is no terminal to measure; callers then show
/// everything rather than guessing.
pub fn terminal_rows() -> Option<usize> {
    // An explicit `LINES` wins: it is the conventional way to say "lay out for
    // this height", and it makes the layout testable.
    if let Ok(text) = std::env::var("LINES") {
        if let Ok(rows) = text.trim().parse::<usize>() {
            if rows > 0 {
                return Some(rows);
            }
        }
    }
    #[cfg(windows)]
    {
        windows_terminal_rows()
    }
    #[cfg(unix)]
    {
        unix_terminal_rows()
    }
    #[cfg(not(any(windows, unix)))]
    {
        None
    }
}

#[cfg(unix)]
fn unix_terminal_rows() -> Option<usize> {
    // `stty size` prints "<rows> <columns>" for the terminal on stdin.
    let output = std::process::Command::new("stty")
        .arg("size")
        .stdin(std::process::Stdio::inherit())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let rows = text.split_whitespace().next()?.parse::<usize>().ok()?;
    (rows > 0).then_some(rows)
}
