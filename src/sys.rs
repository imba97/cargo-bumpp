//! Platform bits: the local date, ANSI enablement, and single-key terminal mode.
//!
//! Everything here is implemented against the OS directly (no crates): Win32
//! through hand-written `extern "system"` declarations, Unix through `isatty`
//! and `stty`. Every entry point is allowed to fail, and callers are expected to
//! fall back to the portable path.

use std::io::IsTerminal;

/// Is stdin a terminal we can read single keys from?
pub fn stdin_is_tty() -> bool {
    std::io::stdin().is_terminal()
}

/// Is stdout a terminal (as opposed to a pipe or a file)?
pub fn stdout_is_tty() -> bool {
    std::io::stdout().is_terminal()
}

/// Today's date in the local time zone, as `(year, month, day)`.
///
/// Falls back to UTC when the platform call is unavailable or fails.
pub fn local_date() -> (i32, u32, u32) {
    #[cfg(windows)]
    if let Some(date) = windows_local_date() {
        return date;
    }
    #[cfg(unix)]
    if let Some(date) = unix_local_date() {
        return date;
    }
    utc_date()
}

fn utc_date() -> (i32, u32, u32) {
    let secs = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => duration.as_secs() as i64,
        Err(err) => -(err.duration().as_secs() as i64),
    };
    civil_from_days(secs.div_euclid(86_400))
}

/// Howard Hinnant's `civil_from_days`.
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (
        (if month <= 2 { year + 1 } else { year }) as i32,
        month,
        day,
    )
}

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

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod win {
    use std::ffi::c_void;

    pub type Handle = *mut c_void;

    pub const STD_INPUT_HANDLE: u32 = -10i32 as u32;
    pub const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;

    pub const ENABLE_PROCESSED_INPUT: u32 = 0x0001;
    pub const ENABLE_LINE_INPUT: u32 = 0x0002;
    pub const ENABLE_ECHO_INPUT: u32 = 0x0004;
    pub const ENABLE_VIRTUAL_TERMINAL_INPUT: u32 = 0x0200;
    pub const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    pub struct SystemTime {
        pub year: u16,
        pub month: u16,
        pub day_of_week: u16,
        pub day: u16,
        pub hour: u16,
        pub minute: u16,
        pub second: u16,
        pub milliseconds: u16,
    }

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    pub struct Coord {
        pub x: i16,
        pub y: i16,
    }

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    pub struct SmallRect {
        pub left: i16,
        pub top: i16,
        pub right: i16,
        pub bottom: i16,
    }

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    pub struct ConsoleScreenBufferInfo {
        pub size: Coord,
        pub cursor_position: Coord,
        pub attributes: u16,
        pub window: SmallRect,
        pub maximum_window_size: Coord,
    }

    extern "system" {
        pub fn GetStdHandle(kind: u32) -> Handle;
        pub fn GetConsoleMode(handle: Handle, mode: *mut u32) -> i32;
        pub fn SetConsoleMode(handle: Handle, mode: u32) -> i32;
        pub fn GetLocalTime(out: *mut SystemTime);
        pub fn GetConsoleScreenBufferInfo(handle: Handle, out: *mut ConsoleScreenBufferInfo)
            -> i32;
    }

    pub fn stdin_handle() -> Handle {
        unsafe { GetStdHandle(STD_INPUT_HANDLE) }
    }

    pub fn stdout_handle() -> Handle {
        unsafe { GetStdHandle(STD_OUTPUT_HANDLE) }
    }
}

#[cfg(windows)]
fn windows_local_date() -> Option<(i32, u32, u32)> {
    let mut time = win::SystemTime::default();
    unsafe { win::GetLocalTime(&mut time) };
    if time.year == 0 || time.month == 0 || time.day == 0 {
        return None;
    }
    Some((time.year as i32, time.month as u32, time.day as u32))
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

// ---------------------------------------------------------------------------
// Unix
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn unix_local_date() -> Option<(i32, u32, u32)> {
    // `struct tm` is laid out identically on glibc, musl, macOS and the BSDs:
    // nine ints, then the offset and the zone abbreviation.
    #[repr(C)]
    struct Tm {
        sec: i32,
        min: i32,
        hour: i32,
        mday: i32,
        mon: i32,
        year: i32,
        wday: i32,
        yday: i32,
        isdst: i32,
        gmtoff: i64,
        zone: *const i8,
    }

    extern "C" {
        fn time(out: *mut i64) -> i64;
        fn localtime_r(clock: *const i64, out: *mut Tm) -> *mut Tm;
    }

    unsafe {
        let mut now: i64 = 0;
        if time(&mut now) == -1 {
            return None;
        }
        let mut tm: Tm = std::mem::zeroed();
        if localtime_r(&now, &mut tm).is_null() {
            return None;
        }
        let year = tm.year + 1900;
        if year < 1970 {
            return None;
        }
        Some((year, (tm.mon + 1) as u32, tm.mday as u32))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_from_days_matches_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(1), (1970, 1, 2));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        // 2000-03-01 and the leap day before it
        assert_eq!(civil_from_days(11016), (2000, 2, 29));
        assert_eq!(civil_from_days(11017), (2000, 3, 1));
        // 2026-07-28, the sample date from the design doc
        assert_eq!(civil_from_days(20662), (2026, 7, 28));
    }

    #[test]
    fn local_date_is_plausible() {
        let (year, month, day) = local_date();
        assert!((2020..2200).contains(&year), "year {year}");
        assert!((1..=12).contains(&month), "month {month}");
        assert!((1..=31).contains(&day), "day {day}");
    }
}
