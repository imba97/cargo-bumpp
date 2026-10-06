# cargo-bumpp

[![Release](https://github.com/imba97/cargo-bumpp/actions/workflows/release.yaml/badge.svg)](https://github.com/imba97/cargo-bumpp/actions/workflows/release.yaml)
[![crates.io](https://img.shields.io/crates/v/cargo-bumpp.svg)](https://crates.io/crates/cargo-bumpp)
[![license](https://img.shields.io/badge/license-MIT-blue.svg)](#license)

> 中文版见 [README_CN.md](README_CN.md)

Interactive version bumping for Cargo projects: pick a level, rewrite the
manifests and the lockfile, commit, tag, push. **Zero dependencies** — `cargo
install` takes seconds, and there is no dependency tree to audit.

**It does not publish.** The tag is where this tool stops; CI takes over from
there. crates.io versions cannot be deleted or overwritten, so a registry publish
is not something to put behind one local keystroke.

```console
$ cargo bumpp patch
  bumpp  /path/to/project
  crates  war3-archive, war3-core, war3-map  (shared version)

  war3-archive             0.0.2 -> 0.0.3
  war3-core                0.0.2 -> 0.0.3
  war3-map                 0.0.2 -> 0.0.3

  Cargo.toml
    [workspace.package]       line 6: 0.0.2 -> 0.0.3
    [workspace.dependencies]  line 11: 0.0.2 -> 0.0.3
    [workspace.dependencies]  line 13: 0.0.2 -> 0.0.3

  commit  chore: release v0.0.3
     tag  v0.0.3
    push  origin main and v0.0.3

? Bump? (Y/n) y
  wrote Cargo.toml
  commit chore: release v0.0.3
     tag v0.0.3
    push origin main, v0.0.3
```

## Contents

- [Why this one](#why-this-one)
- [Install](#install)
- [Quick start](#quick-start)
- [What a default run does](#what-a-default-run-does)
- [Options](#options)
- [Signing (GPG)](#signing-gpg)
- [Exit codes](#exit-codes)
- [Where the version lives](#where-the-version-lives)
- [The plan output](#the-plan-output)
- [The interactive selector](#the-interactive-selector)
- [Commit message and tag name](#commit-message-and-tag-name)
- [Configuration](#configuration)
- [Differences from bumpp](#differences-from-bumpp)
- [Using it as a library](#using-it-as-a-library)
- [Troubleshooting](#troubleshooting)
- [Development](#development)
- [Acknowledgements](#acknowledgements)
- [License](#license)

## Why this one

The niche is already busy, so here is the honest comparison.

| | cargo-bumpp | [cargo-release](https://github.com/crate-ci/cargo-release) | [cargo-bump](https://crates.io/crates/cargo-bump) |
| --- | --- | --- | --- |
| Install cost | seconds, no dependencies | builds libgit2/OpenSSL unless configured otherwise | seconds |
| Interactive version selector | yes | no | no |
| `conventional` bump from the commit log | yes | no | no |
| Shared workspace version + path-dependency versions | yes | yes | partial |
| Rollback when a step fails | yes (before the push) | partial | no |
| Publishes to a registry | no, on purpose | yes | no |
| Changelog | no, on purpose | optional | no |
| PR-based release flow (`--pr`) | no | yes | no |

Choose `cargo-release` if you need changelog generation, per-crate releases, or a
tool that publishes for you. Choose `cargo-bumpp` if you want the common case —
one shared version, commit, tag, push — behind an interactive prompt that
installs in seconds.

## Install

```bash
cargo install cargo-bumpp
```

That gives you both `cargo bumpp` (as a Cargo subcommand) and `bumpp` (called
directly); the two are equivalent:

```bash
cargo bumpp patch     # as a Cargo subcommand
bumpp patch           # called directly
```

### Requirements

- **Rust 1.74+** to build and install (this is the MSRV).
- **`cargo` and `git`** on `PATH` at run time. Neither is linked in: the version
  graph comes from `cargo metadata`, and every git action is a subprocess.
- Linux, macOS and Windows. The arrow-key selector needs a terminal that accepts
  ANSI escapes; anywhere else the tool falls back to a numbered list.

## Quick start

```bash
cargo bumpp                        # selector → commit → tag → push
cargo bumpp patch                  # skip the selector, bump patch
cargo bumpp minor -y               # skip the confirmation too
cargo bumpp 1.0.0 --no-push        # explicit version, stop at the tag and look first
cargo bumpp conventional           # decide major/minor/patch from the commit log
cargo bumpp --preid rc --release prepatch
cargo bumpp --no-tag --no-push     # only rewrite the version numbers
```

## What a default run does

```
  1. show the version selector, the user picks   ← the only interaction
  2. print the plan (with line numbers) and ask "Bump?" once
  3. rewrite the versions (manifests + Cargo.lock)
  4. git commit   "chore: release v1.2.1"
  5. git tag      v1.2.1 (annotated; the message is the commit message)
  6. git push     the branch, then that one tag
```

**If any of steps 3–5 fails, the run is rolled back**: the tag is deleted, HEAD is
reset to the commit it pointed at before the run, and the files are restored from
snapshots taken before the first write.

**A failed push (step 6) is not rolled back.** Pushing is the first action that
cannot be undone locally, and a rejected push is usually the remote saying no
(protected branch, non-fast-forward, no permission) — retrying or fixing it beats
deleting local work. The commit and the tag stay, git's own error is printed
verbatim, and the commands to retry are offered ready to copy.

## Options

| Option | Default | What it does |
| --- | --- | --- |
| `--release <level>` | `prompt` | the level or version, instead of the positional argument |
| `--preid <preid>` | `beta` | pre-release identifier |
| `-a, --all` | `false` | `git add --all` and commit everything, not just the bumped files |
| `--git-check` / `--no-git-check` | on | require a clean working tree |
| `-c, --commit [msg]` / `--no-commit` | on | commit, optionally with a custom message |
| `-t, --tag [name]` / `--no-tag` | on | annotated tag, optionally with a custom name (a template) |
| `--sign` / `--no-sign` | neither | see [Signing](#signing-gpg) |
| `-p, --push` / `--no-push` | on | push the branch and that tag |
| `-y, --yes` | `false` | skip the `Bump?` confirmation |
| `-r, --recursive` | `false` | let every member bump its own version too |
| `--verify` / `--no-verify` | on | `--no-verify` passes `--no-verify` to `git commit` |
| `--ignore-scripts` | `false` | accepted for parity with bumpp; Cargo has no lifecycle scripts |
| `-x, --execute <command>` | — | run a command after writing, before committing |
| `--current-version <v>` | detected | state the current version explicitly |
| `--print-commits` / `--no-print-commits` | on | print the commits `conventional` looked at |
| `--lockfile` / `--no-lockfile` | on | whether to run `cargo update --workspace` |
| `--commit-window <n>` | `100` | how many commits `conventional` may read |
| `--configFilePath <path>` | `bumpp.toml` | a different config file |
| `-q, --quiet` | `false` | only warnings and errors |
| `-h, --help`, `-V, --version` | | |

**The short options are copied from bumpp**, including the two that are easy to
guess wrong: `-r` is `--recursive` (not `--release`) and `-p` is `--push` (not
`--preid`). `cargo bumpp --help` prints the same table.

`--release` accepts `major`, `minor`, `patch`, `next`, `conventional`,
`conventional-prerelease`, `premajor`, `preminor`, `prepatch`, `prerelease`,
`as-is`, `prompt`, or a version (`1.2.3`; `v1.2.3` is fine too).

### Signing (GPG)

| Case | Behaviour |
| --- | --- |
| `--sign` | commit gets `--gpg-sign`, tag gets `--sign` |
| neither | **git's own configuration decides**: with `commit.gpgsign` / `tag.gpgsign` set globally or in the repo, both are signed — so every run asks for your key |
| `--no-sign` | explicitly unsigned: passes `-c commit.gpgsign=false` / `-c tag.gpgsign=false`, overriding those settings |

`--sign` only *asks* for a signature; `--no-sign` is the one that overrides what
git would have done anyway. A failed signature (no usable key, wrong PIN, `gpg`
missing) counts as a failed tag step, so **the commit is undone too** rather than
leaving a half-signed release behind.

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | success |
| `1` | a check failed; the workspace was rolled back to a clean state |
| `2` | usage error (unknown option, conflicting combination, broken config file) |
| `130` | cancelled at a prompt (Ctrl+C in the selector, or `n` at `Bump?`) |
| other | git's exit code, passed through when the push failed; the local commit and tag are **kept** |

## Where the version lives

A bump touches more places than one expects. In a workspace whose single source
of truth is `[workspace.package] version`:

```toml
[workspace.package]
version = "0.0.2"                                              # 1. the source of truth

[workspace.dependencies]
war3-core = { path = "crates/war3-core", version = "0.0.2" }   # 2. repeated per member (for publishing)
```

A path dependency pointing back into the workspace is a third spelling of the
same thing:

```toml
[dependencies]
war3-core = { path = "../war3-core", version = "0.0.2" }        # 3. same value, another shape
```

The tool finds these through `cargo metadata --no-deps --format-version 1` — a
**subprocess, not a crate dependency** — so Cargo itself guarantees the parsing.
Rewriting is done **line by line, preserving the original formatting**: only the
version string is replaced; indentation, comments, line endings, `rust-version`
and `version.workspace = true` all survive untouched.

| Project shape | Behaviour |
| --- | --- |
| `[workspace.package] version` exists | bump it, plus every path dependency pointing at a member whose value equals it |
| one package, version in `[package]` | bump it |
| several packages with independent versions, no shared one | **error out**, listing the members and their versions; `--recursive` opts into bumping each on its own |

The third case is not guessed at and no single member is picked: bumping one
would leave the members that depend on it inconsistent, and that kind of mistake
only shows up at publish time.

### Reported, never silently skipped

- a member (outside `--recursive`) that carries its own different version → warning, mentioning `--recursive`
- a path dependency with a `path` but no `version` → warning (`cargo publish` cannot rewrite it)
- a version value that spans lines (a multi-line inline table) → warning naming **which** spot could not be located
- a requirement that will no longer accept the new version (`^0.0.2` versus `0.0.3`) → warning, listed

The last one is **detected and reported but not rewritten** in this version:
rewriting it changes dependency resolution, and a tool that quietly widens version
requirements is harder to trust than one that lists them for you to confirm.

## The plan output

Before anything is written, the tool prints what is about to happen — **every spot
with its line number**, so a missed one is visible while it is still cheap:

```
  Cargo.toml
    [workspace.package]       line 6: 0.0.2 -> 0.0.3
    [workspace.dependencies]  line 11: 0.0.2 -> 0.0.3
    [workspace.dependencies]  line 13: 0.0.2 -> 0.0.3
```

`cargo bumpp patch` on a workspace of three members, end to end, is the block at
the [top of this file](#cargo-bumpp).

This output is the reason there is **no `--dry-run`**: the only step that could
fail silently (missing a spot) is listed line by line beforehand, and the
remaining risk — writing something wrong — is what the rollback is for. The same
list is what could not be located, if anything, so "it says it cannot handle this
file" is answered before the confirmation rather than after.

## The interactive selector

Eleven rows, in bumpp's order. Note that the second `conventional` row is really
`conventional-prerelease`; the two are told apart by the version on the right:

```
? Current version 0.0.2                ← the version is green
  up/down (or j/k), Enter to pick, Ctrl+C to cancel
>          next 0.0.3                 ← the selected row: bold cyan
           major 1.0.0                ← the others: dimmed
           minor 0.1.0
           patch 0.0.3
    conventional 0.0.3
    conventional 0.0.3-beta.1
       pre-patch 0.0.3-beta.1
       pre-minor 0.1.0-beta.1
       pre-major 1.0.0-beta.1
           as-is 0.0.2
          custom ...
```

- **Keys**: `↑`/`↓` or `j`/`k` to move, `g`/`G` for the ends, a digit to jump to
  a row, `Enter` to pick, `Ctrl+C` to cancel.
- **The `custom …` row** asks for the version on the same single-key prompt the
  menu uses: the tool echoes what you type itself (a console in that mode neither
  echoes nor assembles lines), `Backspace` corrects it, `Enter` finishes it and
  `Ctrl+C` cancels the run — nothing has been written at that point. Versions are
  ASCII, so any other character is ignored rather than echoed.
- **Highlighting**: the row under the cursor is bold cyan, the rest is dimmed.
  Colour is only used when escape sequences can actually be written — redirected
  output, a `cmd.exe` without ANSI support, or `--quiet` all drop it, so a pipe
  always receives plain text.
- **Scrolling**: the menu takes at most "terminal rows − 3" (header, hint line,
  one row of slack) and never fewer than 3 rows. When the list does not fit, **the
  last line becomes `↓ ...`** and the cursor stops one row above it: the window
  only starts moving when the cursor reaches the second-to-last row, so the next
  row is already visible by the time you want it. Scrolling further puts `↑ ...`
  at the top; at the end of the list `↓ ...` disappears and the last row becomes a
  real choice again. If the terminal height cannot be determined, everything is
  shown rather than guessed.

  On an 8-row terminal, with the cursor on the fourth row:

  ```
  ? Current version 1.2.0 »
    up/down (or j/k), Enter to pick, Ctrl+C to cancel
    ↑ ...
            minor 1.3.0
            patch 1.2.1
  >          next 1.2.1
     conventional 1.2.1
    ↓ ...
  ```

- **Without a terminal**: a numbered list instead — type a number, a name (`pre-m`
  works as a prefix), or press Enter for the default row. **It never hangs without
  a TTY**: if input cannot be read it fails with the message below. This path has
  no colour and does not scroll, because its output is meant to be plain text for
  a human or a CI log.

```
Cannot prompt for the version number because input or output has been disabled.
```

### What each level means

- `next` on a **stable** version equals `patch` (0.0.2 → 0.0.3); on a
  **pre-release** it increments the pre-release counter (0.0.3-rc.1 → 0.0.3-rc.2).
  It **does not promote a pre-release to a release** — that is what the `custom`
  row is for.
- `as-is` leaves the version alone and only does the git steps. There are no file
  changes, so the commit would be empty — which is why the commit carries
  `--allow-empty`. `as-is` means "the version stays, but I want a release point",
  and that commit is what the tag points at.
- The first pre-release is `-beta.1`, not `-beta.0`; and when the current version
  is already a pre-release, **its identifier is kept** (`1.2.1-rc.3` stays on `rc`,
  `--preid beta` does not switch it).
- `conventional` reads the commits from **the last tag to HEAD**: a breaking change
  means `major`, otherwise a `feat` means `minor`, otherwise `patch`. The window is
  capped by `--commit-window` (100 by default) and, when it truncates, it says how
  many older commits were not inspected instead of quietly cutting them off.
- History that does not follow Conventional Commits **degrades to `patch` without
  complaining** — no `feat` means patch.

## Commit message and tag name

Defaults, overridable in the config file:

| | Default |
| --- | --- |
| commit message | `chore: release v{version}` |
| tag | `v{version}` |

Template tokens, the same set as bumpp:

| token | meaning | example |
| --- | --- | --- |
| `{version}` | the new version | `1.2.3` |
| `{oldVersion}` | the previous version | `1.2.2` |
| `{tag}` | the rendered tag name | `v1.2.3` |
| `{releaseType}` | the bump type (empty for an explicit version) | `patch` |
| `{major}` / `{minor}` / `{patch}` | the three parts of the new version | `1` / `2` / `3` |
| `{date}` | today's date (local time zone) | `2026-07-28` |

Rendering has three tiers: if the template contains any named token, only named
tokens are replaced (`%s` then does nothing); otherwise `%s` is replaced;
otherwise **the new version is appended** — which is why
`--commit "chore: release v"` is a legal spelling. The tag name is resolved
**first**, and its result is what `{tag}` means in the commit message and
elsewhere.

```bash
cargo bumpp --commit "chore: release {tag}" --tag "{version}"
```

## Configuration

**The config file is optional**: without a `bumpp.toml` every option takes its
built-in default and the tool is fully usable. It is looked up at the workspace
root, and the precedence is **command line > environment > `bumpp.toml` >
built-in defaults**.

```toml
# bumpp.toml (every option; copying this file changes nothing)
commit = true
tag = true
push = true
commit-message = "chore: release v{version}"
tag-name = "v{version}"
preid = "beta"
sign = false

# these are configurable too
all = false
git-check = true
verify = true
lockfile = true
recursive = false
quiet = false
print-commits = true
commit-window = 100
```

The two changes people actually make:

```toml
push = false                           # confirm locally before anything is pushed
commit-message = "release: {version}"  # a different message style
```

**An unknown key is an error, not something to ignore**: a mistyped
`commti-message` that is silently dropped looks like "the option does not work",
and the first suspect would be the tool. The error lists every known key with its
default. Options that execute something (`--execute`) **cannot** be put in a config
file and must be given on the command line.

Only the subset of TOML this flat schema needs is parsed (`key = "string"`,
`key = true/false`, `key = 123`, `#` comments, blank lines) — a few dozen lines,
not a general TOML implementation, in keeping with the line-by-line approach taken
for `Cargo.toml`.

### Environment variables

`BUMPP_COMMIT`, `BUMPP_TAG`, `BUMPP_PUSH`, `BUMPP_PREID`, `BUMPP_COMMIT_MESSAGE`,
all outranking the config file so CI can override a repository's defaults
(typically to disable pushing).

## Differences from bumpp

The differences the design doc already spells out (`--pr` is not implemented, the
config file is static TOML rather than an executable module, the working-tree
check is on by default, a specific tag is pushed instead of `--tags`) are
implemented as written. Beyond those:

| Subject | bumpp | This tool | Why |
| --- | --- | --- | --- |
| `--no-commit` | pulled back to true by tag/push via `\|\|`, so it barely works | an explicit `--no-commit` is honoured and **also drops the tag**; `--no-commit --tag` is a usage error | an explicit request should not be silently overridden; with no commit, what would the tag point at? |
| answering `n` at `Bump?` | exit code 1 | exit code 130 | cancelled at a prompt, per the exit code table |
| `conventional` window | last tag to HEAD, uncapped | the same, but capped by `--commit-window`, saying so when it truncates | a window you can configure |
| tag already exists | fails at the tag step, then rolls back | checked **before any file is written** | fail early rather than late |
| pushing the tag | `git push --tags` (every other local tag goes too) | `git push <remote> refs/tags/<tag>` | in a repo with a release workflow, pushing someone else's old tag can trigger an unexpected release |
| extra options | — | `--lockfile`, `--commit-window`, `--no-sign`, `--no-print-commits`, `--no-all` | every boolean has a negated form, so a config file can be overridden for one run |

Deliberately **not** in this version: the full `--pr` pull-request release flow;
rewriting `^x.y.z` requirements outside the workspace; changelog generation;
publishing to a registry.

## Using it as a library

```rust
use cargo_bumpp::{Prompt, Selection, Level};

// your own prompt: answer deterministically, or drive it from another UI
let mut prompt = cargo_bumpp::prompt::ScriptedPrompt::new(
    vec![Selection::Level(Level::Patch)],
    vec![true], // the confirmation
);
let args = vec!["--no-push".to_string()];
cargo_bumpp::run_in(std::path::Path::new("."), &args, Some(&mut prompt))?;
```

`Prompt` is a trait (`select_release` / `confirm`) whose default implementation is
the terminal. Pass `RefusePrompt` to make it fail with "input or output has been
disabled" instead of hanging. The pieces are public too, if you only want part of
the tool: `semver`, `tokens`, `toml_line`, `plan`, `workspace`.

## Troubleshooting

**It refuses to run: `Git working tree is not clean`**
That check is on by default, because a version tool that writes files, commits and
tags cannot tell its own changes from someone else's in a dirty tree, and neither
can a rollback. Commit or stash first, or pass `--no-git-check` if you accept
that.

**Every run asks for my GPG key**
Your git is configured to sign (`commit.gpgsign` / `tag.gpgsign`). Add `--no-sign`
for a run, or set `sign` in `bumpp.toml` — see [Signing](#signing-gpg).

**The commit failed with `user.email` not configured**
git needs an identity for `git commit`; this tool does not invent one. Set
`user.name` / `user.email` in your git config (or pass them to git) and retry —
the workspace was rolled back, so nothing is half-done.

**`Cannot prompt for the version number because input or output has been disabled.`**
There is no terminal to ask on, and the tool fails rather than hanging. Pass a
level (`cargo bumpp patch`) or `--release <level>`; add `-y` to skip the
confirmation too.

**The push failed, but the commit and tag are local**
That is on purpose — see [What a default run does](#what-a-default-run-does). The
error prints the exact `git push` commands to retry, and how to undo locally.

**`cargo build` says `Fresh` although a source file changed**
Not this tool's doing, but it bit us during development: if cargo's fingerprint is
stale, `cargo clean -p cargo-bumpp` forces the rebuild.

## Development

```bash
cargo test     # unit tests + end-to-end tests
cargo fmt      # no rustfmt.toml, so the default style
cargo clippy --all-targets -- -D warnings
```

Inside this repository, `cargo bumpp` runs the working tree rather than an
installed snapshot — a one-line `[alias]` in
[`.cargo/config.toml`](.cargo/config.toml) — so the crate can release itself
without being installed first. Anywhere else, `cargo bumpp` needs
`cargo install cargo-bumpp` (or `cargo install --path .` from a checkout).

The badge at the top of this file belongs to the pipeline that runs on release
tags; [`.github/workflows/ci.yaml`](.github/workflows/ci.yaml) holds the checks,
and these three commands are what they come down to.

The end-to-end tests build real temporary Cargo workspaces and git repositories
and run the real `cargo metadata` and `git commit`, covering rollback, a failed
push, a dirty tree, a missing TTY, a refusing pre-commit hook and a signing
failure. Their git configuration is fully isolated in temporary files
(`GIT_CONFIG_GLOBAL` / `GIT_CONFIG_SYSTEM`), so your own git settings are never
read or changed — and no GPG prompt can appear.

Where the code lives:

| File | Contents |
| --- | --- |
| `src/cli.rs` | command line parsing and `--help` |
| `src/options.rs` | default merging, and cross-option rules such as "tag/push decide commit's default" |
| `src/workspace.rs` | reading `cargo metadata` (the hand-written JSON parser is `src/json.rs`) |
| `src/toml_line.rs` | line-by-line `Cargo.toml` reading and rewriting, preserving formatting, reporting what it cannot locate |
| `src/plan.rs` | the plan: which file, which line, which value; warnings are produced here too |
| `src/app.rs` | orchestration, rollback, conventional-commit analysis, the git steps |
| `src/prompt.rs`, `src/sys.rs` | the selector and confirmation, plus the terminal/platform layer |
| `src/report.rs` | the plan output and the summary |
| `src/git.rs` | every git subprocess call |
| `src/config.rs` | `bumpp.toml` and `BUMPP_*` |
| `src/semver.rs` | semver parsing, comparison, `inc` (matching node-semver), and requirement checking |

Contributions are welcome — open an issue before a large change, and keep the
dependency count at zero.

## Acknowledgements

- [bumpp](https://github.com/antfu-collective/bumpp)
- [prompts](https://github.com/terkelg/prompts)

## License

MIT — see [LICENSE](LICENSE).
