//! The version model itself: `Version`, `PreId` and `Level`, their inherent
//! implementations, and the trait implementations that make a version
//! orderable, parseable and printable.
//!
//! It is its own file because this is the type the rest of the crate talks
//! about; the precedence rule it delegates to lives in `compare`, the parse
//! helpers in `parse`, and the requirement checker in `requirement`.

use std::cmp::Ordering;
use std::fmt;

use super::compare::compare_pre;
use super::parse::{parse_number, split_identifiers, split_pre};

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
