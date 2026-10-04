//! Platform bits: the local date, ANSI enablement, and single-key terminal mode.
//!
//! Everything here is implemented against the OS directly (no crates): Win32
//! through hand-written `extern "system"` declarations, Unix through `isatty`
//! and `stty`. Every entry point is allowed to fail, and callers are expected to
//! fall back to the portable path.

mod tty;

mod date;

mod ansi;

mod raw_mode;

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod win;

pub use ansi::enable_ansi_output;
pub use date::local_date;
pub use raw_mode::RawMode;
pub use tty::{stdin_is_tty, stdout_is_tty, terminal_rows};
