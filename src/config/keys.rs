//! The key table of `bumpp.toml`: every key, the type it expects, and the keys
//! that stay command-line only.

/// The file searched for at the workspace root.
pub const CONFIG_FILE_NAME: &str = "bumpp.toml";

/// Config keys, with the type expected and whether the key is part of the
/// documented surface.
pub(super) struct KeySpec {
    pub(super) name: &'static str,
    pub(super) kind: Kind,
    pub(super) help: &'static str,
}

#[derive(PartialEq, Clone, Copy)]
pub(super) enum Kind {
    Bool,
    String,
    Integer,
}

pub(super) const KEYS: &[KeySpec] = &[
    KeySpec {
        name: "commit",
        kind: Kind::Bool,
        help: "commit the bump (default: true)",
    },
    KeySpec {
        name: "tag",
        kind: Kind::Bool,
        help: "create an annotated tag (default: true)",
    },
    KeySpec {
        name: "push",
        kind: Kind::Bool,
        help: "push the branch and the tag (default: true)",
    },
    KeySpec {
        name: "commit-message",
        kind: Kind::String,
        help: "commit message template (default: \"chore: release v{version}\")",
    },
    KeySpec {
        name: "tag-name",
        kind: Kind::String,
        help: "tag name template (default: \"v{version}\")",
    },
    KeySpec {
        name: "preid",
        kind: Kind::String,
        help: "pre-release identifier (default: \"beta\")",
    },
    KeySpec {
        name: "sign",
        kind: Kind::Bool,
        help: "GPG-sign the commit and the tag (default: false)",
    },
    KeySpec {
        name: "all",
        kind: Kind::Bool,
        help: "stage and commit every change (default: false)",
    },
    KeySpec {
        name: "git-check",
        kind: Kind::Bool,
        help: "require a clean working tree (default: true)",
    },
    KeySpec {
        name: "verify",
        kind: Kind::Bool,
        help: "run git's verification hooks (default: true)",
    },
    KeySpec {
        name: "lockfile",
        kind: Kind::Bool,
        help: "refresh Cargo.lock with `cargo update --workspace` (default: true)",
    },
    KeySpec {
        name: "recursive",
        kind: Kind::Bool,
        help: "bump every package (default: false)",
    },
    KeySpec {
        name: "quiet",
        kind: Kind::Bool,
        help: "only print warnings and errors (default: false)",
    },
    KeySpec {
        name: "print-commits",
        kind: Kind::Bool,
        help: "print the commits `conventional` looked at (default: true)",
    },
    KeySpec {
        name: "commit-window",
        kind: Kind::Integer,
        help: "how many recent commits `conventional` reads (default: 100)",
    },
];

/// Keys that exist as command line options but are deliberately not readable
/// from a file that lives in the repository.
pub(super) const CLI_ONLY: &[&str] = &[
    "release",
    "retag",
    "retag-name",
    "current-version",
    "execute",
    "yes",
    "config-path",
    "config-file-path",
];
