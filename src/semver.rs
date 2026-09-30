//! A small semver implementation: parsing, precedence, `node-semver`-compatible
//! increments, and enough of Cargo's version-requirement syntax to *check*
//! whether a requirement still accepts the new version.
//!
//! It is written from scratch because the tool has no dependencies; the
//! `inc()` rules are a deliberate port of node-semver's `SemVer#inc`, since the
//! reference implementation (bumpp) delegates to it and the results must match.

use std::cmp::Ordering;
use std::fmt;

/// One dot-separated identifier of a pre-release version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreId {
    /// Numeric identifiers compare numerically.
    Num(u64),
    /// Alphanumeric identifiers compare by ASCII order.
    Alpha(String),
}

impl fmt::Display for PreId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PreId::Num(n) => write!(f, "{n}"),
            PreId::Alpha(s) => write!(f, "{s}"),
        }
    }
}

/// A parsed semantic version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Vec<PreId>,
    pub build: Vec<String>,
}

/// The release level, i.e. what `--release` accepts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Level {
    Major,
    Minor,
    Patch,
    /// Patch for a stable version, "next pre-release" for a pre-release one.
    Next,
    /// Same as [`Level::Next`], then landed on a pre-release.
    Conventional,
    /// Resolved from the commit history, landed on a stable version.
    ConventionalPrerelease,
    PreMajor,
    PreMinor,
    PrePatch,
    PreRelease,
    /// Keep the current version; only do the git steps.
    AsIs,
    /// An explicit version, e.g. `bumpp 1.2.3`.
    Explicit(Version),
    /// Ask with the interactive selector. Not a bump on its own.
    Prompt,
}

impl Level {
    /// The `{releaseType}` template token. Empty for an explicit version, which
    /// is what the reference implementation does too.
    pub fn as_token(&self) -> &str {
        match self {
            Level::Major => "major",
            Level::Minor => "minor",
            Level::Patch => "patch",
            Level::Next => "next",
            Level::Conventional => "conventional",
            Level::ConventionalPrerelease => "conventional-prerelease",
            Level::PreMajor => "premajor",
            Level::PreMinor => "preminor",
            Level::PrePatch => "prepatch",
            Level::PreRelease => "prerelease",
            Level::AsIs => "as-is",
            Level::Explicit(_) => "",
            Level::Prompt => "prompt",
        }
    }

    /// Accepts the level names `--release` understands. `as-is` is accepted as
    /// well, because the interactive selector offers it.
    pub fn parse(raw: &str) -> Option<Level> {
        let lower = raw.trim();
        let level = match lower {
            "major" => Level::Major,
            "minor" => Level::Minor,
            "patch" => Level::Patch,
            "next" => Level::Next,
            "conventional" => Level::Conventional,
            "conventional-prerelease" | "conventionalPre" => Level::ConventionalPrerelease,
            "premajor" | "pre-major" => Level::PreMajor,
            "preminor" | "pre-minor" => Level::PreMinor,
            "prepatch" | "pre-patch" => Level::PrePatch,
            "prerelease" | "pre" => Level::PreRelease,
            "as-is" | "asis" | "as_is" => Level::AsIs,
            "prompt" => Level::Prompt,
            _ => match Version::parse_lenient(lower) {
                Ok(v) => Level::Explicit(v),
                Err(_) => return None,
            },
        };
        Some(level)
    }

    /// True for the two levels whose target version depends on the commit log.
    pub fn is_conventional(&self) -> bool {
        matches!(self, Level::Conventional | Level::ConventionalPrerelease)
    }

    /// True when the level needs a `--preid`.
    pub fn is_prerelease(&self) -> bool {
        matches!(
            self,
            Level::ConventionalPrerelease
                | Level::PreMajor
                | Level::PreMinor
                | Level::PrePatch
                | Level::PreRelease
        )
    }
}

impl Version {
    pub fn new(major: u64, minor: u64, patch: u64) -> Self {
        Version {
            major,
            minor,
            patch,
            pre: Vec::new(),
            build: Vec::new(),
        }
    }

    /// Strict parse: `1.2.3`, `1.2.3-beta.1`, `1.2.3-rc.1+build.5`.
    pub fn parse(input: &str) -> Result<Version, String> {
        let text = input.trim();
        if text.is_empty() {
            return Err("empty version".to_string());
        }

        // Split off build metadata first: it may not contain '-' or '+'.
        let (rest, build) = match text.split_once('+') {
            Some((rest, build)) => (rest, split_identifiers(build, "build metadata")?),
            None => (text, Vec::new()),
        };

        // Split off the pre-release.
        let (core, pre) = match rest.split_once('-') {
            Some((core, pre)) => (core, split_pre(pre)?),
            None => (rest, Vec::new()),
        };

        let parts: Vec<&str> = core.split('.').collect();
        if parts.len() != 3 {
            return Err(format!(
                "`{input}` is not a valid version (expected major.minor.patch)"
            ));
        }
        let major = parse_number(parts[0], input)?;
        let minor = parse_number(parts[1], input)?;
        let patch = parse_number(parts[2], input)?;

        Ok(Version {
            major,
            minor,
            patch,
            pre,
            build,
        })
    }

    /// Like [`Version::parse`], but tolerates a leading `v` / `V` / `=` and
    /// surrounding whitespace. Used for values that come from a human
    /// (`bumpp v1.2.3`, `--current-version v1.2.3`).
    pub fn parse_lenient(input: &str) -> Result<Version, String> {
        let mut text = input.trim();
        while let Some(rest) = text.strip_prefix(['v', 'V', '=', ' ']) {
            text = rest.trim_start();
        }
        Version::parse(text)
    }

    pub fn is_prerelease(&self) -> bool {
        !self.pre.is_empty()
    }

    /// The leading alphanumeric pre-release identifier, if any: `rc` for
    /// `1.2.1-rc.3`. Used to keep a pre-release line from switching identifier
    /// (`rc` -> `beta`).
    pub fn pre_identifier(&self) -> Option<&str> {
        match self.pre.first() {
            Some(PreId::Alpha(s)) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Apply a release level, node-semver semantics.
    ///
    /// The pre-release counter starts at `1` for the pre-release levels
    /// (`premajor`, `preminor`, `prepatch`, `prerelease`, …) and at `0`
    /// otherwise, which is what the reference implementation passes as
    /// `identifierBase`.
    pub fn increment(&self, level: &Level, preid: &str) -> Result<Version, String> {
        let mut v = self.clone();
        v.build.clear();
        let base = if level.is_prerelease() { 1 } else { 0 };
        match level {
            Level::Explicit(explicit) => return Ok(explicit.clone()),
            Level::AsIs => return Ok(v),
            Level::Major => v.inc_major(),
            Level::Minor => v.inc_minor(),
            Level::Patch => v.inc_patch(),
            Level::Next => {
                if v.pre.is_empty() {
                    v.inc_patch();
                } else {
                    v.inc_pre(preid, base);
                }
            }
            Level::PreMajor => {
                v.pre.clear();
                v.patch = 0;
                v.minor = 0;
                v.major += 1;
                v.inc_pre(preid, base);
            }
            Level::PreMinor => {
                v.pre.clear();
                v.patch = 0;
                v.minor += 1;
                v.inc_pre(preid, base);
            }
            Level::PrePatch => {
                v.pre.clear();
                v.inc_patch();
                v.inc_pre(preid, base);
            }
            Level::PreRelease => {
                if v.pre.is_empty() {
                    v.inc_patch();
                }
                v.inc_pre(preid, base);
            }
            Level::Conventional | Level::ConventionalPrerelease => {
                return Err("conventional levels are resolved from the commit log first".to_string())
            }
            Level::Prompt => return Err("the release level has not been chosen yet".to_string()),
        }
        Ok(v)
    }

    fn inc_major(&mut self) {
        if self.minor != 0 || self.patch != 0 || self.pre.is_empty() {
            self.major += 1;
        }
        self.minor = 0;
        self.patch = 0;
        self.pre.clear();
    }

    fn inc_minor(&mut self) {
        if self.patch != 0 || self.pre.is_empty() {
            self.minor += 1;
        }
        self.patch = 0;
        self.pre.clear();
    }

    fn inc_patch(&mut self) {
        if self.pre.is_empty() {
            self.patch += 1;
        }
        self.pre.clear();
    }

    /// node-semver's `inc('pre', identifier, base)`: bump the last numeric
    /// identifier, then normalise the leading identifier to `preid`.
    fn inc_pre(&mut self, preid: &str, base: u64) {
        if self.pre.is_empty() {
            self.pre.push(PreId::Num(base));
        } else {
            let mut bumped = false;
            for i in (0..self.pre.len()).rev() {
                if let PreId::Num(n) = self.pre[i] {
                    self.pre[i] = PreId::Num(n + 1);
                    bumped = true;
                    break;
                }
            }
            if !bumped {
                self.pre.push(PreId::Num(base));
            }
        }

        if !preid.is_empty() {
            let target = vec![PreId::Alpha(preid.to_string()), PreId::Num(base)];
            let same_prefix = matches!(self.pre.first(), Some(PreId::Alpha(a)) if a == preid);
            if !same_prefix || !matches!(self.pre.get(1), Some(PreId::Num(_))) {
                self.pre = target;
            }
        }
    }

    /// Semver precedence: build metadata is ignored.
    pub fn cmp_precedence(&self, other: &Version) -> Ordering {
        self.major
            .cmp(&other.major)
            .then_with(|| self.minor.cmp(&other.minor))
            .then_with(|| self.patch.cmp(&other.patch))
            .then_with(|| compare_pre(&self.pre, &other.pre))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.cmp_precedence(other)
    }
}

impl std::str::FromStr for Version {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Version::parse(text)
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if !self.pre.is_empty() {
            let joined: Vec<String> = self.pre.iter().map(|p| p.to_string()).collect();
            write!(f, "-{}", joined.join("."))?;
        }
        if !self.build.is_empty() {
            write!(f, "+{}", self.build.join("."))?;
        }
        Ok(())
    }
}

fn compare_pre(a: &[PreId], b: &[PreId]) -> Ordering {
    match (a.is_empty(), b.is_empty()) {
        // A version without pre-release has higher precedence.
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => {
            for (left, right) in a.iter().zip(b.iter()) {
                let ord = match (left, right) {
                    (PreId::Num(x), PreId::Num(y)) => x.cmp(y),
                    (PreId::Num(_), PreId::Alpha(_)) => Ordering::Less,
                    (PreId::Alpha(_), PreId::Num(_)) => Ordering::Greater,
                    (PreId::Alpha(x), PreId::Alpha(y)) => x.cmp(y),
                };
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            a.len().cmp(&b.len())
        }
    }
}

fn parse_number(part: &str, input: &str) -> Result<u64, String> {
    let trimmed = part.trim();
    if trimmed.is_empty() || !trimmed.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!(
            "`{input}` is not a valid version (bad number `{part}`)"
        ));
    }
    trimmed
        .parse::<u64>()
        .map_err(|_| format!("`{input}` is not a valid version (number out of range: `{part}`)"))
}

fn split_identifiers(part: &str, what: &str) -> Result<Vec<String>, String> {
    let ids: Vec<String> = part.split('.').map(|s| s.to_string()).collect();
    if ids.iter().any(|s| s.is_empty()) {
        return Err(format!("invalid {what} `{part}`"));
    }
    Ok(ids)
}

fn split_pre(part: &str) -> Result<Vec<PreId>, String> {
    if part.is_empty() {
        return Err("empty pre-release identifier".to_string());
    }
    let mut out = Vec::new();
    for id in part.split('.') {
        if id.is_empty() {
            return Err(format!("invalid pre-release `{part}`"));
        }
        if id.bytes().all(|b| b.is_ascii_digit()) {
            match id.parse::<u64>() {
                Ok(n) => out.push(PreId::Num(n)),
                Err(_) => {
                    return Err(format!(
                        "numeric pre-release identifier out of range: `{id}`"
                    ))
                }
            }
        } else {
            out.push(PreId::Alpha(id.to_string()));
        }
    }
    Ok(out)
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn parses_and_displays() {
        assert_eq!(v("1.2.3").to_string(), "1.2.3");
        assert_eq!(v("1.2.3-beta.1").to_string(), "1.2.3-beta.1");
        assert_eq!(v("1.2.3-rc.1+build.5").to_string(), "1.2.3-rc.1+build.5");
        assert_eq!(v("0.0.0").to_string(), "0.0.0");
        assert!(Version::parse("1.2").is_err());
        assert!(Version::parse("1.2.3.4").is_err());
        assert!(Version::parse("a.b.c").is_err());
        assert!(Version::parse("").is_err());
    }

    #[test]
    fn lenient_parse() {
        assert_eq!(Version::parse_lenient("v1.2.3").unwrap(), v("1.2.3"));
        assert_eq!(Version::parse_lenient(" =v1.2.3 ").unwrap(), v("1.2.3"));
    }

    #[test]
    fn precedence() {
        assert!(v("1.2.3") < v("1.2.4"));
        assert!(v("1.2.3-beta.1") < v("1.2.3"));
        assert!(v("1.2.3-alpha") < v("1.2.3-beta"));
        assert!(v("1.2.3-beta.1") < v("1.2.3-beta.2"));
        assert!(v("1.2.3-beta.2") < v("1.2.3-beta.10"));
        assert!(v("1.2.3-1") < v("1.2.3-alpha"));
        assert_eq!(v("1.2.3+a").cmp_precedence(&v("1.2.3+b")), Ordering::Equal);
    }

    #[test]
    fn levels_parse() {
        assert_eq!(Level::parse("patch"), Some(Level::Patch));
        assert_eq!(
            Level::parse("conventional-prerelease"),
            Some(Level::ConventionalPrerelease)
        );
        assert_eq!(Level::parse("pre-patch"), Some(Level::PrePatch));
        assert_eq!(Level::parse("1.2.3"), Some(Level::Explicit(v("1.2.3"))));
        assert_eq!(Level::parse("nonsense"), None);
        assert_eq!(Level::Explicit(v("1.2.3")).as_token(), "");
        assert_eq!(Level::Patch.as_token(), "patch");
    }

    #[test]
    fn increments_match_node_semver() {
        let cases: &[(&str, Level, &str)] = &[
            ("1.2.0", Level::Major, "2.0.0"),
            ("1.2.0", Level::Minor, "1.3.0"),
            ("1.2.0", Level::Patch, "1.2.1"),
            ("1.2.0", Level::Next, "1.2.1"),
            ("1.2.1-beta.1", Level::Next, "1.2.1-beta.2"),
            ("1.2.1-rc.3", Level::Next, "1.2.1-beta.0"),
            ("1.2.0", Level::PrePatch, "1.2.1-beta.1"),
            ("1.2.0", Level::PreMinor, "1.3.0-beta.1"),
            ("1.2.0", Level::PreMajor, "2.0.0-beta.1"),
            ("1.2.0", Level::PreRelease, "1.2.1-beta.1"),
            ("1.2.1-beta.1", Level::PreRelease, "1.2.1-beta.2"),
            // node-semver's guards: a pre-release "absorbs" the bump
            ("1.2.0-beta.1", Level::Minor, "1.2.0"),
            ("2.0.0-beta.1", Level::Major, "2.0.0"),
            ("1.2.3-beta.1", Level::Patch, "1.2.3"),
            ("1.2.3-beta.1", Level::Minor, "1.3.0"),
            ("1.2.3-beta.1", Level::Major, "2.0.0"),
            ("1.2.3", Level::AsIs, "1.2.3"),
            // The pre-release counter starts at 1 for pre* levels and at 0 for
            // the others (`identifierBase` in the reference implementation).
            ("1.2.1-alpha.1", Level::Next, "1.2.1-beta.0"),
            ("1.2.1-alpha", Level::Next, "1.2.1-beta.0"),
        ];
        for (from, level, expected) in cases {
            let got = v(from).increment(level, "beta").unwrap();
            assert_eq!(got.to_string(), *expected, "{from} with {level:?}");
        }
    }

    #[test]
    fn preid_is_inherited_from_an_existing_prerelease() {
        // caller decides the preid; here we check the mechanism itself
        assert_eq!(
            v("1.2.1-rc.3")
                .increment(&Level::PrePatch, "rc")
                .unwrap()
                .to_string(),
            "1.2.2-rc.1"
        );
        // switching preid resets the identifier but keeps the base at 1
        assert_eq!(
            v("1.2.1-rc.3")
                .increment(&Level::PrePatch, "beta")
                .unwrap()
                .to_string(),
            "1.2.2-beta.1"
        );
        // `next` is not a pre* level, so its base is 0
        assert_eq!(
            v("1.2.1-rc.3")
                .increment(&Level::Next, "rc")
                .unwrap()
                .to_string(),
            "1.2.1-rc.4"
        );
    }

    #[test]
    fn conventional_prerelease_needs_no_resolution_when_already_prerelease() {
        // the caller picks PreRelease in that case; check both outcomes
        assert_eq!(
            v("1.2.1-beta.1")
                .increment(&Level::PreRelease, "beta")
                .unwrap()
                .to_string(),
            "1.2.1-beta.2"
        );
        assert_eq!(
            v("1.2.1-beta.1")
                .increment(&Level::PreMinor, "beta")
                .unwrap()
                .to_string(),
            "1.3.0-beta.1"
        );
    }

    #[test]
    fn pre_identifier() {
        assert_eq!(v("1.2.1-rc.3").pre_identifier(), Some("rc"));
        assert_eq!(v("1.2.1-1").pre_identifier(), None);
        assert_eq!(v("1.2.1").pre_identifier(), None);
    }

    #[test]
    fn requirement_checker() {
        let yes = |req: &str, ver: &str| {
            assert_eq!(req_matches(req, &v(ver)), Some(true), "{req} vs {ver}");
        };
        let no = |req: &str, ver: &str| {
            assert_eq!(req_matches(req, &v(ver)), Some(false), "{req} vs {ver}");
        };
        yes("^0.0.2", "0.0.2");
        no("^0.0.2", "0.0.3");
        yes("0.0.2", "0.0.2");
        no("0.0.2", "0.0.3");
        yes("^1.2.3", "1.5.0");
        no("^1.2.3", "2.0.0");
        yes("~1.2.3", "1.2.9");
        no("~1.2.3", "1.3.0");
        yes(">=1.0.0", "9.9.9");
        no(">=1.0.0", "0.9.9");
        yes("*", "0.0.1");
        yes("1.2.*", "1.2.7");
        no("1.2.*", "1.3.0");
        yes("1", "1.9.0");
        no("1", "2.0.0");
        yes(">=0.0.2", "0.0.2");
        no(">=0.0.2", "0.0.1");
        yes(">=0.0.2, <0.0.5", "0.0.3");
        no("^0.0.2, <0.0.5", "0.0.3");
        no("^0.0.2, <0.0.3", "0.0.3");
        // pre-releases only match a requirement that names one
        no("^1.2.3", "1.2.4-beta.1");
        yes("^1.2.4-beta.1", "1.2.4-beta.2");
        // unparseable syntax stays quiet
        assert_eq!(req_matches(">= 1.2.3 < 2", &v("1.2.3")), None);
    }
}
