//! The single-key ("raw-ish") terminal mode, and how each platform enters it.
//!
//! The mode is restored on drop, so the guard struct and the platform calls that
//! build it belong in one file: the saved console mode is private state that
//! only they know how to put back.

use super::tty::stdin_is_tty;

#[cfg(windows)]
use super::win;

/// A terminal in single-key ("raw-ish") mode, restored on drop.
///
/// Returns `None` when the mode could not be changed; the caller then falls back
/// to line-based prompting.
#[must_use = "the terminal returns to its normal mode as soon as this is dropped"]
pub struct RawMode {
    #[cfg(windows)]
    stdin_mode: u32,
    #[cfg(unix)]
    saved_state: Option<String>,
    #[cfg(not(any(windows, unix)))]
    _nothing: (),
}

impl RawMode {
    /// Put the terminal into single-key mode.
    pub fn enable() -> Option<RawMode> {
        if !stdin_is_tty() {
            return None;
        }
        #[cfg(windows)]
        {
            windows_raw_mode()
        }
        #[cfg(unix)]
        {
            unix_raw_mode()
        }
        #[cfg(not(any(windows, unix)))]
        {
            None
        }
    }
}

#[cfg(windows)]
impl Drop for RawMode {
    fn drop(&mut self) {
        unsafe {
            win::SetConsoleMode(win::stdin_handle(), self.stdin_mode);
        }
    }
}

#[cfg(unix)]
impl Drop for RawMode {
    fn drop(&mut self) {
        if let Some(state) = self.saved_state.take() {
            let _ = std::process::Command::new("stty")
                .arg(state)
                .stdin(std::process::Stdio::inherit())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    }
}

#[cfg(not(any(windows, unix)))]
impl Drop for RawMode {
    fn drop(&mut self) {}
}

#[cfg(windows)]
fn windows_raw_mode() -> Option<RawMode> {
    unsafe {
        let handle = win::stdin_handle();
        let mut mode = 0u32;
        if win::GetConsoleMode(handle, &mut mode) == 0 {
            return None;
        }
        let raw =
            mode & !(win::ENABLE_LINE_INPUT | win::ENABLE_ECHO_INPUT | win::ENABLE_PROCESSED_INPUT);
        // Prefer VT input so arrow keys arrive as `ESC [ A`; fall back to the
        // legacy `0xE0 0x48` encoding, which the reader understands as well.
        if win::SetConsoleMode(handle, raw | win::ENABLE_VIRTUAL_TERMINAL_INPUT) == 0
            && win::SetConsoleMode(handle, raw) == 0
        {
            return None;
        }
        Some(RawMode { stdin_mode: mode })
    }
}

#[cfg(unix)]
fn unix_raw_mode() -> Option<RawMode> {
    fn stty(args: &[&str]) -> Option<String> {
        let output = std::process::Command::new("stty")
            .args(args)
            .stdin(std::process::Stdio::inherit())
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    let saved = stty(&["-g"])?;
    if saved.is_empty() {
        return None;
    }
    // `min 1 time 0` keeps reads blocking until at least one byte is available.
    if stty(&["-icanon", "-echo", "min", "1", "time", "0"]).is_none() {
        let _ = stty(&[saved.as_str()]);
        return None;
    }
    Some(RawMode {
        saved_state: Some(saved),
    })
}
