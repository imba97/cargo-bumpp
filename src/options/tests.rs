//! The tests for the whole module: the defaults, the cross-option rules and the
//! merging of the sources. They are their own file because they exercise
//! `RawOptions` and `Options` together — the rules only show up at `resolve`,
//! where both are in play.

use super::*;

fn raw() -> RawOptions {
    RawOptions::default()
}

#[test]
fn defaults_are_the_documented_ones() {
    let options = Options::resolve(raw()).unwrap();
    assert!(options.commit && options.tag && options.push);
    assert!(options.git_check && options.verify && options.lockfile && options.print_commits);
    assert!(!options.all && !options.recursive && !options.sign && !options.yes);
    assert!(!options.ignore_scripts && !options.quiet);
    assert_eq!(options.preid, "beta");
    assert_eq!(options.commit_message, "chore: release v{version}");
    assert_eq!(options.tag_name, "v{version}");
    assert_eq!(options.commit_window, 100);
    assert!(options.release_from_prompt, "no level means the selector");
}

#[test]
fn no_commit_alone_also_drops_the_tag() {
    let options = Options::resolve(RawOptions {
        commit: Some(false),
        ..raw()
    })
    .unwrap();
    assert!(!options.commit);
    assert!(
        !options.tag,
        "a tag with no commit would point at the previous commit"
    );
    assert!(!options.push, "with nothing to push, pushing is pointless");
}

#[test]
fn no_commit_with_an_explicit_tag_is_a_usage_error() {
    let err = Options::resolve(RawOptions {
        commit: Some(false),
        tag: Some(true),
        ..raw()
    })
    .unwrap_err();
    assert_eq!(err.exit_code(), 2);
    assert!(err.message().contains("--no-commit and --tag"));
}

#[test]
fn tag_pulls_the_commit_in() {
    let options = Options::resolve(RawOptions {
        commit: Some(true),
        tag: Some(true),
        ..raw()
    })
    .unwrap();
    assert!(options.commit && options.tag);
}

#[test]
fn no_sign_is_remembered_so_it_can_override_git_config() {
    assert!(
        Options::resolve(RawOptions {
            sign: Some(false),
            ..raw()
        })
        .unwrap()
        .explicit_no_sign
    );
    // Not mentioned: git's own `commit.gpgsign` decides.
    assert!(!Options::resolve(raw()).unwrap().explicit_no_sign);
    assert!(
        !Options::resolve(RawOptions {
            sign: Some(true),
            ..raw()
        })
        .unwrap()
        .explicit_no_sign
    );
}

#[test]
fn no_tag_keeps_the_commit() {
    let options = Options::resolve(RawOptions {
        tag: Some(false),
        ..raw()
    })
    .unwrap();
    assert!(options.commit, "commit's default still comes from push");
    assert!(!options.tag);
    assert!(options.push);
}

#[test]
fn everything_can_be_switched_off() {
    let options = Options::resolve(RawOptions {
        commit: Some(false),
        tag: Some(false),
        push: Some(false),
        ..raw()
    })
    .unwrap();
    assert!(!options.commit && !options.tag && !options.push);
}

#[test]
fn push_can_be_switched_off_on_its_own() {
    let options = Options::resolve(RawOptions {
        push: Some(false),
        ..raw()
    })
    .unwrap();
    assert!(options.commit && options.tag && !options.push);
}

#[test]
fn overlay_prefers_the_higher_priority_source() {
    let file = RawOptions {
        push: Some(false),
        preid: Some("rc".into()),
        ..raw()
    };
    let env = RawOptions {
        preid: Some("alpha".into()),
        ..raw()
    };
    let cli = RawOptions {
        commit: Some(false),
        ..raw()
    };
    let merged = cli.overlay(env.overlay(file));
    assert_eq!(
        merged.push,
        Some(false),
        "a file value survives when nobody overrides it"
    );
    assert_eq!(merged.preid.as_deref(), Some("alpha"), "env beats the file");
    assert_eq!(merged.commit, Some(false));
}

#[test]
fn rejects_a_bad_preid() {
    assert!(Options::resolve(RawOptions {
        preid: Some("beta.1".into()),
        ..raw()
    })
    .is_err());
    assert!(Options::resolve(RawOptions {
        preid: Some(String::new()),
        ..raw()
    })
    .is_err());
    assert!(Options::resolve(RawOptions {
        preid: Some("1".into()),
        ..raw()
    })
    .is_err());
    assert!(Options::resolve(RawOptions {
        preid: Some("rc".into()),
        ..raw()
    })
    .is_ok());
}

#[test]
fn current_version_is_parsed_leniently() {
    let options = Options::resolve(RawOptions {
        current_version: Some("v1.2.3".into()),
        ..raw()
    })
    .unwrap();
    assert_eq!(options.current_version.unwrap().to_string(), "1.2.3");
    assert!(Options::resolve(RawOptions {
        current_version: Some("x.y".into()),
        ..raw()
    })
    .is_err());
}

#[test]
fn a_zero_commit_window_is_refused() {
    assert!(Options::resolve(RawOptions {
        commit_window: Some(0),
        ..raw()
    })
    .is_err());
}
