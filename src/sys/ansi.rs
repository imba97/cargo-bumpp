//! ANSI escape processing for the console, so colours work in `cmd.exe`.
//!
//! Split off from the other terminal work because it is the one switch a caller
//! flips for the whole run, and on Unix there is nothing to flip at all.

use super::tty::stdout_is_tty;

#[cfg(windows)]
use super::win;

/// Turn on ANSI escape processing for the console, so colours work in `cmd.exe`.
///
/// Returns whether escape sequences may be written to stdout.
pub fn enable_ansi_output() -> bool {
    if !stdout_is_tty() {
        return false;
    }
    #[cfg(windows)]
    {
        windows_enable_virtual_terminal()
    }
    #[cfg(not(windows))]
    {
        true
    }
}

#[cfg(windows)]
fn windows_enable_virtual_terminal() -> bool {
    unsafe {
        let handle = win::stdout_handle();
        let mut mode = 0u32;
        if win::GetConsoleMode(handle, &mut mode) == 0 {
            return false;
        }
        if mode & win::ENABLE_VIRTUAL_TERMINAL_PROCESSING != 0 {
            return true;
        }
        win::SetConsoleMode(handle, mode | win::ENABLE_VIRTUAL_TERMINAL_PROCESSING) != 0
    }
}
