//! The Cargo version-requirement checker: does `^0.0.2` still accept the new
//! version?
//!
//! It is its own file because it is the one place that reads the requirement
//! syntax of a manifest; it only ever *checks* a requirement, and it answers
//! `None` rather than guessing when the syntax is beyond it.

use std::cmp::Ordering;

use super::parse::split_pre;
use super::version::Version;

// ---------------------------------------------------------------------------
// Version requirements
// ---------------------------------------------------------------------------

/// Does `req` (Cargo syntax, e.g. `^0.0.2`, `>=1.2`, `*`) accept `version`?
///
/// Returns `None` when the requirement uses syntax this checker does not
/// understand, so callers can stay quiet instead of reporting nonsense.
pub fn req_matches(req: &str, version: &Version) -> Option<bool> {
    let req = req.trim();
    if req.is_empty() {
        return Some(true);
    }
    let mut matched_any = false;
    for comparator in req.split(',') {
        let comparator = comparator.trim();
        if comparator.is_empty() {
            continue;
        }
        matched_any = true;
        if !comparator_matches(comparator, version)? {
            return Some(false);
        }
    }
    Some(matched_any)
}

/// One comparator: an operator plus a (possibly partial) version.
fn comparator_matches(comparator: &str, version: &Version) -> Option<bool> {
    let (op, rest) = split_operator(comparator);
    let rest = rest.trim();
    if rest.is_empty() {
        return None;
    }

    // Separate the numeric core from a pre-release / build suffix.
    let cut = rest.find(['-', '+']).unwrap_or(rest.len());
    let (core, suffix) = rest.split_at(cut);
    if suffix.starts_with('+') {
        // `>=1.2.3+build` carries no extra meaning for a range check.
        return None;
    }
    let pre = match suffix.strip_prefix('-') {
        Some(pre) if !pre.is_empty() => Some(split_pre(pre).ok()?),
        Some(_) => return None,
        None => None,
    };

    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() > 3 {
        return None;
    }
    let mut nums = [0u64; 3];
    let mut missing = [true; 3];
    let mut explicit_wild = [false; 3];
    for (i, part) in parts.iter().enumerate() {
        let part = part.trim();
        if part == "*" || part.eq_ignore_ascii_case("x") {
            explicit_wild[i] = true;
            missing[i] = true;
        } else if !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()) {
            nums[i] = part.parse::<u64>().ok()?;
            missing[i] = false;
        } else {
            return None;
        }
    }

    // A pre-release target only satisfies a requirement that names the same
    // `major.minor.patch` and a pre-release of its own.
    if version.is_prerelease() {
        let names_prerelease = pre.is_some()
            && nums[0] == version.major
            && nums[1] == version.minor
            && nums[2] == version.patch;
        if !names_prerelease {
            return Some(false);
        }
    }

    let lower = Version {
        major: nums[0],
        minor: nums[1],
        patch: nums[2],
        pre: pre.clone().unwrap_or_default(),
        build: Vec::new(),
    };
    let at_least_lower = version.cmp_precedence(&lower) != Ordering::Less;
    let below = |upper: Version| version.cmp_precedence(&upper) == Ordering::Less;

    // An explicit `*` / `x` makes it a wildcard requirement, whatever the
    // operator: `1.2.*` is >=1.2.0, <1.3.0.
    if explicit_wild.iter().any(|w| *w) {
        if matches!(op, ">" | ">=" | "<" | "<=") {
            // `>=1.*` only really constrains the known numbers.
            return Some(match op {
                ">" => version.cmp_precedence(&lower) == Ordering::Greater,
                ">=" => at_least_lower,
                "<" => below(lower),
                _ => at_least_lower,
            });
        }
        let upper = if explicit_wild[0] {
            None
        } else if explicit_wild[1] {
            Some(Version::new(nums[0] + 1, 0, 0))
        } else {
            Some(Version::new(nums[0], nums[1] + 1, 0))
        };
        return Some(at_least_lower && upper.map_or(true, below));
    }

    match op {
        // Bare requirements are caret requirements in Cargo.
        "" | "^" => {
            let upper = if nums[0] > 0 {
                Some(Version::new(nums[0] + 1, 0, 0))
            } else if missing[1] {
                Some(Version::new(1, 0, 0))
            } else if nums[1] > 0 {
                Some(Version::new(0, nums[1] + 1, 0))
            } else if missing[2] {
                Some(Version::new(0, 1, 0))
            } else {
                Some(Version::new(0, 0, nums[2] + 1))
            };
            Some(at_least_lower && upper.map_or(true, below))
        }
        "~" => {
            let upper = if missing[1] {
                Some(Version::new(nums[0] + 1, 0, 0))
            } else {
                Some(Version::new(nums[0], nums[1] + 1, 0))
            };
            Some(at_least_lower && upper.map_or(true, below))
        }
        "=" => {
            if missing[0] {
                // `=*` accepts everything
                Some(true)
            } else if missing[1] {
                Some(at_least_lower && below(Version::new(nums[0] + 1, 0, 0)))
            } else if missing[2] {
                Some(at_least_lower && below(Version::new(nums[0], nums[1] + 1, 0)))
            } else {
                Some(version.cmp_precedence(&lower) == Ordering::Equal)
            }
        }
        ">" => Some(version.cmp_precedence(&lower) == Ordering::Greater),
        ">=" => Some(at_least_lower),
        "<" => Some(below(lower)),
        "<=" => Some(version.cmp_precedence(&lower) != Ordering::Greater),
        _ => None,
    }
}

fn split_operator(comparator: &str) -> (&str, &str) {
    for op in [">=", "<=", "^", "~", ">", "<", "="] {
        if let Some(rest) = comparator.strip_prefix(op) {
            return (op, rest);
        }
    }
    ("", comparator)
}
