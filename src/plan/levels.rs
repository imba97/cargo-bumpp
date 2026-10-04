//! Applying a level to a version, including the rule that keeps the current
//! pre-release identifier. The tests for that rule sit here, with the code they
//! cover.

use crate::error::{Error, Result};
use crate::semver::{Level, Version};

/// Apply a level, honouring the "keep the current pre-release identifier" rule:
/// a version already on `rc` stays on `rc` even when `--preid beta` is given, so
/// one pre-release line never mixes identifiers.
pub fn increment(old: &Version, level: &Level, preid: &str) -> Result<Version> {
    let effective = old.pre_identifier().unwrap_or(preid);
    old.increment(level, effective)
        .map_err(|err| Error::check(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    #[test]
    fn an_existing_prerelease_identifier_wins() {
        assert_eq!(
            increment(&v("1.2.1-rc.3"), &Level::Next, "beta")
                .unwrap()
                .to_string(),
            "1.2.1-rc.4"
        );
        assert_eq!(
            increment(&v("1.2.1-rc.3"), &Level::PrePatch, "beta")
                .unwrap()
                .to_string(),
            "1.2.2-rc.1"
        );
        // a stable version uses the configured preid, and the pre* levels start
        // their counter at 1
        assert_eq!(
            increment(&v("1.2.0"), &Level::PrePatch, "beta")
                .unwrap()
                .to_string(),
            "1.2.1-beta.1"
        );
        assert_eq!(
            increment(&v("1.2.0"), &Level::PrePatch, "rc")
                .unwrap()
                .to_string(),
            "1.2.1-rc.1"
        );
        assert_eq!(
            increment(&v("1.2.0"), &Level::Patch, "beta")
                .unwrap()
                .to_string(),
            "1.2.1"
        );
    }
}
