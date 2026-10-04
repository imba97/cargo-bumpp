//! Conventional-commit analysis: reading the commits since the last tag and
//! deciding whether the next version is a major, a minor or a patch.

use crate::git::{Commit, Git};
use crate::options::Options;
use crate::semver::Level;

// ---------------------------------------------------------------------------
// conventional commits
// ---------------------------------------------------------------------------

/// What the recent commits say the next version should be.
#[derive(Debug, Clone)]
pub struct Analysis {
    pub level: Level,
    pub commits: Vec<Commit>,
    pub range: String,
    pub truncated: usize,
    pub available: bool,
}

/// Read the commits since the last tag and decide `major` / `minor` / `patch`.
///
/// `isBreaking` anywhere wins, otherwise a `feat` means minor, otherwise patch.
pub(super) fn analyze_commits(git: &Git, options: &Options) -> Analysis {
    let fallback = Analysis {
        level: Level::Patch,
        commits: Vec::new(),
        range: String::new(),
        truncated: 0,
        available: false,
    };
    if !git.is_repository() {
        return fallback;
    }
    let from = git.last_tag().unwrap_or(None);
    let range = match &from {
        Some(tag) => format!("{tag}..HEAD"),
        None => "HEAD".to_string(),
    };
    let commits = match git.commits(&range, options.commit_window + 1) {
        Ok(commits) => commits,
        Err(_) => return fallback,
    };

    let mut commits = commits;
    let truncated = if commits.len() > options.commit_window {
        let extra = commits.len() - options.commit_window;
        commits.truncate(options.commit_window);
        extra
    } else {
        0
    };

    let level = classify(&commits);
    Analysis {
        level,
        commits,
        range,
        truncated,
        available: true,
    }
}

pub(super) fn classify(commits: &[Commit]) -> Level {
    let mut major = false;
    let mut minor = false;
    for commit in commits {
        if is_breaking(commit) {
            major = true;
        } else if commit_type(&commit.subject).as_deref() == Some("feat") {
            minor = true;
        }
    }
    if major {
        Level::Major
    } else if minor {
        Level::Minor
    } else {
        Level::Patch
    }
}

/// `type(scope)!: description` — the type, when the subject follows the
/// convention at all.
pub(super) fn commit_type(subject: &str) -> Option<String> {
    let colon = subject.find(':')?;
    let prefix = &subject[..colon];
    let end = prefix.find('(').unwrap_or(prefix.len());
    let kind = prefix[..end].trim();
    if kind.is_empty() || !kind.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return None;
    }
    Some(kind.to_ascii_lowercase())
}

/// Breaking when the header carries `!` before the colon, or when a footer says
/// `BREAKING CHANGE:` / `BREAKING-CHANGE:`.
fn is_breaking(commit: &Commit) -> bool {
    if let Some(colon) = commit.subject.find(':') {
        let prefix = &commit.subject[..colon];
        if prefix.trim_end().ends_with('!') || prefix.contains(")!") {
            return true;
        }
    }
    commit.body.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with("BREAKING CHANGE:") || line.starts_with("BREAKING-CHANGE:")
    })
}
