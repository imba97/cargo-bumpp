//! The `--help` text: the whole user-facing description of the command line in
//! one literal, kept apart from the parser that reads those same options.

use super::flags::LEVELS;

/// The `--help` text.
pub fn help() -> String {
    format!(
        "\
cargo-bumpp {version}
Interactive version bumping for Cargo projects: bump, commit, tag, push.

USAGE:
  cargo bumpp [level] [options]
  bumpp [level] [options]

ARGS:
  [level]  {levels}
           Omit it to pick from a list.

OPTIONS:
      --release <level>       the level to bump by, instead of the positional argument
      --retag [tag]           re-release an existing tag: re-create it at HEAD, force-push it
                              [default tag: the most recent one, when it is already at HEAD]
      --preid <preid>         pre-release identifier [default: beta]
  -a, --all                   stage and commit every change, not just the bumped files
      --git-check             require a clean working tree [default: on]
      --no-git-check          skip that check
  -c, --commit [msg]          commit [default: on], optionally with a custom message
      --no-commit             do not commit
  -t, --tag [name]            create an annotated tag [default: on]
      --no-tag                do not tag
      --sign                  GPG-sign the commit and the tag
  -p, --push                  push the branch and the tag [default: on]
      --no-push               do not push
  -y, --yes                   skip the `Bump?` confirmation
  -r, --recursive             bump every package in the workspace, not only the shared version
      --verify                let git run its verification hooks [default: on]
      --no-verify             pass --no-verify to git commit
      --ignore-scripts        accepted for parity with bumpp; Cargo has no lifecycle scripts
  -x, --execute <command>     run a command after writing the versions, before committing
      --current-version <v>   the version to bump from [default: detected]
      --print-commits         print the commits used by `conventional` [default: on]
      --lockfile              refresh Cargo.lock [default: on]
      --no-lockfile           leave Cargo.lock alone
      --commit-window <n>     how many recent commits `conventional` reads [default: 100]
      --configFilePath <path> config file to read [default: bumpp.toml at the workspace root]
  -q, --quiet                 only print warnings and errors
  -h, --help                  print this help
  -V, --version               print the version

TEMPLATES (commit message and tag name):
  {{version}} {{oldVersion}} {{tag}} {{releaseType}} {{major}} {{minor}} {{patch}} {{date}}
  A template with no token and no `%s` gets the new version appended.

RE-RELEASE (--retag):
  Re-creates the tag at HEAD, then pushes it with --force. Re-creating is the
  point: git skips a tag whose ref has not moved, so pushing it again would do
  nothing; a new (annotated) tag object is a ref the push really updates. The tag
  keeps its shape and message, and nothing is bumped, written or committed.
  A tag name is optional: without one the most recent tag is re-released, but only
  when it already points at HEAD. HEAD having moved past a release means the tag
  would land on unreleased commits, so that move has to be asked for by name.

EXIT CODES:
  0 success, 1 check failed (rolled back), 2 usage error, 130 cancelled at a prompt.
  When push fails, git's exit code is passed through and the local commit and tag are kept.
",
        version = env!("CARGO_PKG_VERSION"),
        levels = LEVELS,
    )
}
