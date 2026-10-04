//! Byte-level input: one key at a time, and one line at a time.
//!
//! It is its own file because nothing here knows what a menu is: it only turns
//! the byte stream of a terminal — escape sequences included — into the small
//! `Key` alphabet the selector matches on, and reads whole lines when there is
//! no single-key mode to read from.

use std::io::{BufRead, Read};

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
    Digit(u8),
    Char(char),
    Other,
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
