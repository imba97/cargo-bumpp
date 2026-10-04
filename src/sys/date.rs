//! The local date, and the calendar arithmetic behind the UTC fallback.
//!
//! Separate because the date is the one piece of platform code here with an
//! algorithm worth testing: the tests below run on every host.

#[cfg(windows)]
use super::win;

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

#[cfg(windows)]
fn windows_local_date() -> Option<(i32, u32, u32)> {
    let mut time = win::SystemTime::default();
    unsafe { win::GetLocalTime(&mut time) };
    if time.year == 0 || time.month == 0 || time.day == 0 {
        return None;
    }
    Some((time.year as i32, time.month as u32, time.day as u32))
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
