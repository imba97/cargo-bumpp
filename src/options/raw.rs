//! `RawOptions`: one source's options, with "not mentioned" spelled `None`. It is
//! its own file because it is the merge surface — every source produces one of
//! these and they are stacked with `overlay` before anything is resolved.

use crate::semver::Level;

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
