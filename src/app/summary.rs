//! The commit message, the tag name and the push target, worked out before the
//! plan is printed so the confirmation can be about something concrete.

use crate::error::{Error, Result};
use crate::git::Git;
use crate::options::Options;
use crate::plan::Plan;
use crate::report::GitSummary;
use crate::sys;
use crate::tokens::{self, Tokens};

/// Work out the commit message, the tag name and where the push goes, so the
/// plan can show them and the confirmation can be about something concrete.
pub(super) fn git_summary(
    git: &Git,
    options: &Options,
    plan: &Plan,
    release_type: &str,
) -> Result<GitSummary> {
    let date = sys::local_date();
    let base = Tokens {
        version: plan.new_version.to_string(),
        old_version: plan.old_version.to_string(),
        tag: String::new(),
        release_type: release_type.to_string(),
        major: plan.new_version.major.to_string(),
        minor: plan.new_version.minor.to_string(),
        patch: plan.new_version.patch.to_string(),
        date: format!("{:04}-{:02}-{:02}", date.0, date.1, date.2),
    };

    // The tag is resolved first: its result is the `{tag}` token for everything
    // else, including the commit message.
    let tag = if options.tag {
        let rendered = tokens::render(&options.tag_name, &base);
        let name = rendered.trim().to_string();
        validate_tag(&name)?;
        Some(name)
    } else {
        Some(format!("v{}", plan.new_version))
    };

    let with_tag = Tokens {
        tag: tag.clone().unwrap_or_default(),
        ..base
    };
    let commit_message = options
        .commit
        .then(|| tokens::render(&options.commit_message, &with_tag));

    let (push, branch) = if options.push && options.touches_git() {
        let remote = git.default_remote()?;
        (Some(remote), git.current_branch().ok())
    } else {
        (None, None)
    };

    Ok(GitSummary {
        commit_message,
        tag: options.tag.then_some(tag).flatten(),
        push,
        branch,
    })
}

/// A tag name git will accept, checked early so the failure happens before
/// anything is written.
pub(super) fn validate_tag(name: &str) -> Result<()> {
    let bad = name.is_empty()
        || name.starts_with('-')
        || name.contains("..")
        || name.ends_with(".lock")
        || name
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
        || name.contains(['~', '^', ':', '?', '*', '[', '\\']);
    if bad {
        return Err(
            Error::check(format!("`{name}` is not a usable git tag name"))
                .with_hint("change the tag template with --tag <template> or in bumpp.toml"),
        );
    }
    Ok(())
}
