//! `Options`: the resolved form, after defaults and the cross-option rules. It is
//! its own file because this is the type every other module reads: it is built
//! once, from a `RawOptions`, by `Options::resolve`.

use crate::error::{Error, Result};
use crate::semver::{Level, Version};

use super::defaults::{
    DEFAULT_COMMIT_MESSAGE, DEFAULT_COMMIT_WINDOW, DEFAULT_PREID, DEFAULT_TAG_NAME,
};
use super::raw::RawOptions;
use super::validate::validate_preid;

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
        let commit = raw
            .commit
            .unwrap_or_else(|| raw.tag.unwrap_or(true) || raw.push.unwrap_or(true));

        // 3. tag: explicit wins, otherwise on — except after a deliberate
        //    `--no-commit`, where there is nothing to tag.
        let tag = raw.tag.unwrap_or_else(|| raw.commit != Some(false));

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
            Some(Level::Prompt) | None => Level::Patch,
            Some(level) => level,
        };

        let preid = raw.preid.unwrap_or_else(|| DEFAULT_PREID.to_string());
        validate_preid(&preid)?;

        let current_version = raw
            .current_version
            .as_deref()
            .map(Version::parse_lenient)
            .transpose()
            .map_err(|err| Error::usage(format!("--current-version: {err}")))?;

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
