//! Byte-level input: one key at a time, and one line at a time.
//!
//! It is its own file because nothing here knows what a menu is: it only turns
//! the byte stream of a terminal — escape sequences included — into the small
//! `Key` alphabet the selector matches on, and reads whole lines. Two line
//! readers exist because two terminals do: [`read_line`] leaves the echoing and
//! the editing to a console that is in line mode, while [`read_typed_line`] does
//! both itself, for a terminal that is in single-key mode.

use std::io::{BufRead, Read, Write};

use crate::error::{Error, Result};
use crate::sys;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Key {
    Up,
    Down,
    Home,
    End,
    Enter,
    Escape,
    Interrupt,
    Backspace,
    Digit(u8),
    Char(char),
    Other,
}

impl Key {
    /// The character this key types, if any.
    ///
    /// Only [`read_typed_line`] asks: a version number is ASCII, so anything
    /// outside that — a stray arrow key, a byte of a multi-byte character — is
    /// not something to put in the line or echo back.
    pub(super) fn typed(&self) -> Option<char> {
        match self {
            Key::Char(c) if c.is_ascii_graphic() || *c == ' ' => Some(*c),
            Key::Digit(digit) => Some(char::from(b'0' + digit)),
            _ => None,
        }
    }
}

/// Read a single key. Arrow keys arrive as `ESC [ A` (VT input) or `0xE0 0x48`
/// (the legacy console encoding).
pub(super) fn read_key() -> Result<Key> {
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
        0x08 | 0x7f => Ok(Key::Backspace),
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

/// Read one line, or `None` at end of input (a closed or empty stdin).
pub(super) fn read_line() -> Result<Option<String>> {
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

/// Read one line with the terminal in single-key mode.
///
/// The console does neither echoing nor line editing in that mode, so this does
/// the little of both that a version number needs: it echoes what is typed,
/// `Backspace` corrects it, `Enter` finishes it and `Ctrl+C` cancels. `Ok(None)`
/// is a cancellation as well — Ctrl+C arrives as a byte here, and a closed input
/// reads as one too, so the two cannot be told apart (and do not need to be).
///
/// Asking the question *in* this mode, rather than restoring line mode and
/// reading there, is deliberate: a console that has been switched to single-key
/// mode is not guaranteed to go back to assembling lines (a pseudoconsole, which
/// is what Windows Terminal runs applications on, keeps handing over raw bytes),
/// so a prompt that depended on that would swallow Enter. See `Answer` in
/// [`super::select`].
pub(super) fn read_typed_line() -> Result<Option<String>> {
    let mut typed = String::new();
    loop {
        match read_key()? {
            Key::Enter => {
                println!();
                return Ok(Some(typed));
            }
            Key::Interrupt => {
                println!();
                return Ok(None);
            }
            Key::Backspace => {
                if typed.pop().is_some() {
                    // Back up over the character, overwrite it with a space, and
                    // come back: the erase a terminal would have done itself.
                    print!("\u{8} \u{8}");
                    std::io::stdout().flush().ok();
                }
            }
            key => {
                if let Some(character) = key.typed() {
                    typed.push(character);
                    print!("{character}");
                    std::io::stdout().flush().ok();
                }
            }
        }
    }
}
