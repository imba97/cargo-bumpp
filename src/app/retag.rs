//! `--retag`: re-releasing a tag that is already there.
//!
//! A pipeline that failed after the tag was pushed needs the same tag pushed
//! again, and `git push <remote> refs/tags/<tag>` cannot do that: git skips a
//! ref that has not moved, so nothing reaches the pipeline. The tag therefore has
//! to be *re-created* — for an annotated tag that is a new object, and a ref the
//! force-push really updates.
//!
//! Nothing is bumped, written or committed here, so this path starts after the
//! repository check and never builds a plan: it is a git action with a
//! confirmation in front of it. Like a failed push in the bump flow, a failed
//! force-push is not rolled back — the re-created tag is local work that the
//! printed retry command can still send.

use crate::error::{Error, Result};
use crate::git::Git;
use crate::options::Options;
use crate::prompt::Prompt;
use crate::report::{self, RetagSummary, Ui};

use super::summary::validate_tag;

/// Re-release a tag: show it, ask, re-create it at HEAD, force-push it.
pub(super) fn run(
    git: &Git,
    root: &std::path::Path,
    options: &Options,
    ui: &Ui,
    prompt: &mut dyn Prompt,
) -> Result<()> {
    let name = match &options.retag_name {
        Some(name) => {
            validate_tag(name)?;
            name.clone()
        }
        None => git.last_tag()?.ok_or_else(|| {
            Error::check("there is no tag to re-release")
                .with_hint("name one with --retag <tag>; `git tag` lists them")
        })?,
    };

    if !git.tag_exists(&name) {
        return Err(Error::check(format!("tag `{name}` does not exist here"))
            .with_hint("fetch it with `git fetch --tags`, or name another with --retag <tag>"));
    }

    // What the tag is now: annotated (with a message to keep) or a plain ref.
    let annotated = git.tag_is_annotated(&name)?;
    let message = if annotated {
        Some(git.tag_message(&name)?)
    } else {
        None
    };
    let from_commit = git.tag_target(&name)?;
    let head = git.head()?;

    let remote = if options.push {
        Some(git.default_remote()?)
    } else {
        None
    };

    let summary = RetagSummary {
        tag: name.clone(),
        from: git.commit_label(&from_commit)?,
        to: git.commit_label("HEAD")?,
        annotated,
        push: remote.clone(),
    };

    report::print_retag(ui, root, &summary);
    if from_commit != head {
        ui.warn(format!(
            "`{name}` points at {}; re-releasing moves it to HEAD ({})",
            &from_commit[..from_commit.len().min(8)],
            &head[..head.len().min(8)]
        ));
    }
    if options.sign && !annotated {
        ui.note(format!(
            "`{name}` is a lightweight tag, so --sign has nothing to sign"
        ));
    }

    let confirmation = report::retag_confirmation(ui, &summary);
    if !options.yes {
        if !prompt.confirm(&confirmation, "Re-release?")? {
            return Err(Error::cancelled());
        }
    } else {
        ui.info(confirmation.trim_end());
    }

    let before = git.tag_object(&name)?;
    git.force_tag(
        &name,
        message.as_deref(),
        options.sign,
        options.explicit_no_sign,
    )?;
    ui.step("retag", &name);
    if git.tag_object(&name)? == before {
        // Git skips a ref that has not moved, so the force-push would report
        // nothing to do and the pipeline would never run. Two ways to get here,
        // and they need different advice.
        if annotated {
            ui.note(format!(
                "`{name}` was re-created in the same second, so its object did not change; \
                 run it again in a moment if the pipeline did not start"
            ));
        } else {
            let remote = remote.as_deref().unwrap_or("origin");
            ui.note(format!(
                "`{name}` is lightweight and points straight at the commit, so there is nothing \
                 to re-create; to push it again, delete the remote one first:\n  \
                 git push {remote} :refs/tags/{name}"
            ));
        }
    }

    if let Some(remote) = &remote {
        push_tag(git, remote, &name)?;
        ui.step("push", format!("{remote} refs/tags/{name} (forced)"));
    }

    Ok(())
}

/// `git push --force <remote> refs/tags/<tag>`, reporting a rejection the way the
/// bump flow reports a failed push: git's own text, its exit code, and the retry.
fn push_tag(git: &Git, remote: &str, tag: &str) -> Result<()> {
    let refspec = format!("refs/tags/{tag}");
    let output = git.push(remote, &refspec, true)?;
    if output.ok() {
        return Ok(());
    }
    let detail = output.failure_text();
    let hint = format!(
        "The tag was re-created locally and nothing was rolled back.\nRetry the push with:\n  git push --force {remote} {refspec}"
    );
    Err(Error::push(
        output.code,
        format!("`git push --force {remote} {refspec}` failed:\n{detail}"),
    )
    .with_hint(hint))
}
