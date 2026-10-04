//! The tests for the whole module: parsing and display, precedence, the
//! node-semver-compatible increments, and the requirement checker.
//!
//! They are their own file because they exercise every part of the module at
//! once — a test that increments a version and checks it against a requirement
//! would have to reach across the other files anyway.

use std::cmp::Ordering;

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
