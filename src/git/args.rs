//! The `Vec<String>` argument lists that `commit` and `tag` build, and the
//! options those two share.

use crate::error::Result;

use super::types::Git;

impl Git {
    /// Hand a built `Vec<String>` to `checked_quiet`. The two main callers
    /// (`commit`, `tag`) build the list in the same shape, so the conversion
    /// from `Vec<String>` to the `&[&str]` view that `checked_quiet` wants
    /// lives here once.
    pub(super) fn run_args(&self, args: Vec<String>) -> Result<()> {
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        self.checked_quiet(&refs)
    }
}

/// `-c <subcommand>.gpgsign=false` — same idea in `commit` and `tag`, just
/// with different subcommand names.
pub(super) fn push_gpgsign_override(
    args: &mut Vec<String>,
    subcommand: &str,
    sign: bool,
    unsigned: bool,
) {
    if !sign && unsigned {
        args.push("-c".into());
        args.push(format!("{subcommand}.gpgsign=false"));
    }
}

/// Knobs for [`Git::commit`].
#[derive(Debug, Clone, Copy)]
pub struct CommitOptions {
    /// `--sign`: sign this commit.
    pub sign: bool,
    /// `--no-sign` was given: do not sign, even when git is configured to sign
    /// everything.
    pub unsigned: bool,
    pub verify: bool,
    pub all: bool,
}
