//! Option merging and the rules that derive one option's default from another.
//!
//! Precedence, highest first: command line, environment, `bumpp.toml`, built-in
//! defaults. Merging happens on [`RawOptions`], where every field is optional;
//! [`Options::resolve`] then applies the defaults and the cross-option rules.

use crate::error::{Error, Result};
use crate::semver::{Level, Version};

/// The default commit message template.
pub const DEFAULT_COMMIT_MESSAGE: &str = "chore: release v{version}";
/// The default tag name template.
pub const DEFAULT_TAG_NAME: &str = "v{version}";
/// The default pre-release identifier.
pub const DEFAULT_PREID: &str = "beta";
/// How many recent commits `conventional` looks at by default.
pub const DEFAULT_COMMIT_WINDOW: usize = 100;

/// Options as they come from one source (command line, environment, file), with
/// "not mentioned" represented as `None`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawOptions {
    pub release: Option<Level>,
    pub preid: Option<String>,
    pub all: Option<bool>,
    pub git_check: Option<bool>,
    pub commit: Option<bool>,
    pub commit_message: Option<String>,
    pub tag: Option<bool>,
    pub tag_name: Option<String>,
    pub sign: Option<bool>,
    pub push: Option<bool>,
    pub yes: Option<bool>,
    pub recursive: Option<bool>,
    pub verify: Option<bool>,
    pub ignore_scripts: Option<bool>,
    pub execute: Option<String>,
    pub current_version: Option<String>,
    pub print_commits: Option<bool>,
    pub config_path: Option<String>,
    pub quiet: Option<bool>,
    pub lockfile: Option<bool>,
    pub commit_window: Option<usize>,
}

impl RawOptions {
    /// `self` wins over `lower` for every field that `self` mentions.
    pub fn overlay(self, lower: RawOptions) -> RawOptions {
        macro_rules! pick {
            ($($field:ident),* $(,)?) => {
                RawOptions {
                    $($field: self.$field.or(lower.$field),)*
                }
            };
        }
        pick!(
            release,
            preid,
            all,
            git_check,
            commit,
            commit_message,
            tag,
            tag_name,
            sign,
            push,
            yes,
            recursive,
            verify,
            ignore_scripts,
            execute,
            current_version,
            print_commits,
            config_path,
            quiet,
            lockfile,
            commit_window,
        )
    }
}

/// Options after defaults have been applied.
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    /// The level to bump by. When [`Options::release_from_prompt`] is set the
    /// value is only a placeholder until the user picks.
    pub level: Level,
    /// True when the level must come from the interactive selector.
    pub release_from_prompt: bool,
    pub preid: String,
    pub all: bool,
    pub git_check: bool,
    pub commit: bool,
    pub commit_message: String,
    pub tag: bool,
    pub tag_name: String,
    pub sign: bool,
    /// `--no-sign` was given explicitly: skip signing even when git is
    /// configured to sign every commit and tag.
    pub explicit_no_sign: bool,
    pub push: bool,
    pub yes: bool,
    pub recursive: bool,
    pub verify: bool,
    pub ignore_scripts: bool,
    pub execute: Option<String>,
    pub current_version: Option<Version>,
    pub print_commits: bool,
    pub config_path: Option<String>,
    pub quiet: bool,
    pub lockfile: bool,
    pub commit_window: usize,
    /// Remarks about options that were derived rather than given, so the run can
    /// explain itself instead of surprising anyone.
    pub notes: Vec<String>,
}

impl Options {
    /// Apply built-in defaults and the cross-option rules.
    ///
    /// Two rules are easy to get wrong, and both come from the reference
    /// implementation:
    ///
    /// 1. **commit's default is pulled in by tag or push** — they are one
    ///    action, so the order matters: tag and push are read first.
    /// 2. **a tag needs a commit** — `--no-commit` therefore also turns tagging
    ///    off, unless `--tag` was asked for explicitly, which is a contradiction
    ///    and a usage error rather than a tag pointing at the *previous* commit.
    pub fn resolve(raw: RawOptions) -> Result<Options> {
        // 1. A deliberate "no commit" plus an explicit "tag" cannot both hold.
        if raw.commit == Some(false) && raw.tag == Some(true) {
            return Err(Error::usage("--no-commit and --tag cannot be combined").with_hint(
                "a tag must point at a commit; either let bumpp commit, or tag by hand afterwards",
            ));
        }

        // 2. commit: explicit wins, otherwise tag/push pull it in.
        let commit = match raw.commit {
            Some(value) => value,
            None => raw.tag.unwrap_or(true) || raw.push.unwrap_or(true),
        };

        // 3. tag: explicit wins, otherwise on — except after a deliberate
        //    `--no-commit`, where there is nothing to tag.
        let tag = match raw.tag {
            Some(value) => value,
            None => raw.commit != Some(false),
        };

        // 4. push: explicit wins, otherwise on.
        let push_requested = raw.push.unwrap_or(true);
        // 5. Nothing to push when neither a commit nor a tag will exist.
        let push = push_requested && (commit || tag);

        let mut notes = Vec::new();
        if raw.commit == Some(false) && raw.tag.is_none() {
            notes.push(
                "--no-commit also disables tagging: a tag must point at a commit".to_string(),
            );
        }
        if push_requested && !push {
            notes.push("nothing to push: no commit and no tag will be created".to_string());
        }

        let release_from_prompt = matches!(raw.release, None | Some(Level::Prompt));
        let level = match raw.release {
            Some(Level::Prompt) => Level::Patch,
            Some(level) => level,
            None => Level::Patch,
        };

        let preid = raw.preid.unwrap_or_else(|| DEFAULT_PREID.to_string());
        validate_preid(&preid)?;

        let current_version = match raw.current_version {
            Some(text) => Some(
                Version::parse_lenient(&text)
                    .map_err(|err| Error::usage(format!("--current-version: {err}")))?,
            ),
            None => None,
        };

        let commit_window = raw.commit_window.unwrap_or(DEFAULT_COMMIT_WINDOW);
        if commit_window == 0 {
            return Err(Error::usage("--commit-window must be at least 1"));
        }

        Ok(Options {
            level,
            release_from_prompt,
            preid,
            all: raw.all.unwrap_or(false),
            git_check: raw.git_check.unwrap_or(true),
            commit,
            commit_message: raw
                .commit_message
                .unwrap_or_else(|| DEFAULT_COMMIT_MESSAGE.to_string()),
            tag,
            tag_name: raw.tag_name.unwrap_or_else(|| DEFAULT_TAG_NAME.to_string()),
            sign: raw.sign.unwrap_or(false),
            explicit_no_sign: raw.sign == Some(false),
            push,
            yes: raw.yes.unwrap_or(false),
            recursive: raw.recursive.unwrap_or(false),
            verify: raw.verify.unwrap_or(true),
            ignore_scripts: raw.ignore_scripts.unwrap_or(false),
            execute: raw.execute,
            current_version,
            print_commits: raw.print_commits.unwrap_or(true),
            config_path: raw.config_path,
            quiet: raw.quiet.unwrap_or(false),
            lockfile: raw.lockfile.unwrap_or(true),
            commit_window,
            notes,
        })
    }

    /// True when any git step will run.
    pub fn touches_git(&self) -> bool {
        self.commit || self.tag || self.push
    }
}

fn validate_preid(preid: &str) -> Result<()> {
    if preid.is_empty() {
        return Err(Error::usage("--preid cannot be empty"));
    }
    if !preid
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(Error::usage(format!(
            "--preid `{preid}` is not a valid pre-release identifier (letters, digits and `-` only)"
        )));
    }
    if preid.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Error::usage(format!(
            "--preid `{preid}` must not be numeric-only: it would be read as a pre-release counter"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
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
}
