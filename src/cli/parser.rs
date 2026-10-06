//! The argument parser and its result: one pass over the arguments, ending in
//! either a request for `--help`/`--version` or the [`RawOptions`] to run with.
//!
//! The tests at the bottom are the specification of the accepted spellings, so
//! they live with the parser rather than in a file of their own.

use super::args::Args;
use super::flags::{is_boolean, parse_level, set_boolean, unknown_option};
use crate::error::{Error, Result};
use crate::options::RawOptions;
use crate::semver::Level;

/// What the command line asked for.
#[derive(Debug, Clone, PartialEq)]
pub enum Parsed {
    /// `--help`
    Help,
    /// `--version`
    Version,
    /// Options to run with.
    Run(Box<RawOptions>),
}

/// Parse `args`, which must not include the program name.
pub fn parse(args: &[String]) -> Result<Parsed> {
    let mut raw = RawOptions::default();
    let mut positional: Option<String> = None;
    let mut release_flag: Option<(Level, String)> = None;
    let mut only_positional = false;
    let mut args = Args {
        items: args,
        index: 0,
    };

    while let Some(arg) = args.next() {
        if only_positional || !arg.starts_with('-') || arg == "-" {
            set_positional(&mut positional, &arg)?;
            continue;
        }
        if arg == "--" {
            only_positional = true;
            continue;
        }

        if let Some(long) = arg.strip_prefix("--") {
            let (name, inline) = match long.split_once('=') {
                Some((name, value)) => (name, Some(value.to_string())),
                None => (long, None),
            };

            // `--no-<flag>` switches any boolean off, which is how a config file
            // value is overridden for a single run.
            if let Some(base) = name.strip_prefix("no-") {
                if is_boolean(base) {
                    if inline.is_some() {
                        return Err(Error::usage(format!("`--{name}` does not take a value")));
                    }
                    set_boolean(&mut raw, base, false)?;
                    continue;
                }
            }

            match name {
                "help" => return Ok(Parsed::Help),
                "version" => return Ok(Parsed::Version),
                "release" => {
                    let value = args.value(name, inline)?.expect("required");
                    let level = parse_level(&value)?;
                    release_flag = Some((level, value));
                }
                "retag" => {
                    // `--retag [tag]`: the value is optional, like `--tag`'s.
                    let value = args.optional_value(name, inline)?;
                    raw.retag = Some(true);
                    if let Some(tag) = value {
                        raw.retag_name = Some(tag);
                    }
                }
                "preid" => raw.preid = args.value(name, inline)?,
                "all" => set_boolean(&mut raw, "all", true)?,
                "git-check" => set_boolean(&mut raw, "git-check", true)?,
                "commit" => {
                    // `-c, --commit [msg]`: the value is optional.
                    let value = args.optional_value(name, inline)?;
                    raw.commit = Some(true);
                    if let Some(message) = value {
                        raw.commit_message = Some(message);
                    }
                }
                "tag" => {
                    // `-t, --tag [tag]`: the value is optional.
                    let value = args.optional_value(name, inline)?;
                    raw.tag = Some(true);
                    if let Some(name) = value {
                        raw.tag_name = Some(name);
                    }
                }
                "sign" => set_boolean(&mut raw, "sign", true)?,
                "push" => set_boolean(&mut raw, "push", true)?,
                "yes" => set_boolean(&mut raw, "yes", true)?,
                "recursive" => set_boolean(&mut raw, "recursive", true)?,
                "verify" => set_boolean(&mut raw, "verify", true)?,
                "ignore-scripts" => set_boolean(&mut raw, "ignore-scripts", true)?,
                "print-commits" => set_boolean(&mut raw, "print-commits", true)?,
                "lockfile" => set_boolean(&mut raw, "lockfile", true)?,
                "quiet" => set_boolean(&mut raw, "quiet", true)?,
                "execute" => raw.execute = args.value(name, inline)?,
                "current-version" => raw.current_version = args.value(name, inline)?,
                "commit-window" => {
                    let value = args.value(name, inline)?.expect("required");
                    let window = value.parse::<usize>().map_err(|_| {
                        Error::usage(format!(
                            "`--commit-window` expects a number, found `{value}`"
                        ))
                    })?;
                    raw.commit_window = Some(window);
                }
                "configFilePath" | "config-file-path" => {
                    raw.config_path = args.value(name, inline)?
                }
                other => return Err(unknown_option(&format!("--{other}"))),
            }
            continue;
        }

        // A cluster of short options, e.g. `-ay`.
        let cluster: Vec<char> = arg.chars().skip(1).collect();
        let mut position = 0usize;
        while position < cluster.len() {
            let short = cluster[position];
            position += 1;
            let rest: String = cluster[position..].iter().collect();
            // A flag that takes a value swallows the rest of the cluster.
            let inline = (!rest.is_empty()).then(|| rest.clone());
            match short {
                'h' => return Ok(Parsed::Help),
                'V' => return Ok(Parsed::Version),
                'a' => set_boolean(&mut raw, "all", true)?,
                'p' => set_boolean(&mut raw, "push", true)?,
                'y' => set_boolean(&mut raw, "yes", true)?,
                'r' => set_boolean(&mut raw, "recursive", true)?,
                'q' => set_boolean(&mut raw, "quiet", true)?,
                'c' => {
                    let value = args.optional_value("-c", inline)?;
                    raw.commit = Some(true);
                    if let Some(message) = value {
                        raw.commit_message = Some(message);
                    }
                    break;
                }
                't' => {
                    let value = args.optional_value("-t", inline)?;
                    raw.tag = Some(true);
                    if let Some(name) = value {
                        raw.tag_name = Some(name);
                    }
                    break;
                }
                'x' => {
                    let value = args
                        .optional_value("-x", inline)?
                        .ok_or_else(|| Error::usage("`-x, --execute` requires a command"))?;
                    raw.execute = Some(value);
                    break;
                }
                other => return Err(unknown_option(&format!("-{other}"))),
            }
        }
    }

    raw.release = match (positional, release_flag) {
        (None, None) => None,
        (Some(text), None) => Some(parse_level(&text)?),
        (None, Some((level, _))) => Some(level),
        (Some(text), Some((level, flag_text))) => {
            let from_position = parse_level(&text)?;
            if from_position != level {
                return Err(Error::usage(format!(
                    "two different release levels were given: `{text}` and `--release {flag_text}`"
                )));
            }
            Some(level)
        }
    };

    Ok(Parsed::Run(Box::new(raw)))
}

fn set_positional(slot: &mut Option<String>, value: &str) -> Result<()> {
    if let Some(existing) = slot {
        return Err(Error::usage(format!(
            "unexpected extra argument `{value}` (the release level is already `{existing}`)"
        )));
    }
    *slot = Some(value.to_string());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::usage::help;

    fn parse_args(args: &[&str]) -> Result<Parsed> {
        let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        parse(&owned)
    }

    fn run(args: &[&str]) -> RawOptions {
        match parse_args(args).unwrap() {
            Parsed::Run(raw) => *raw,
            other => panic!("expected options, got {other:?}"),
        }
    }

    #[test]
    fn retag_takes_an_optional_tag_name() {
        assert_eq!(run(&["--retag"]).retag, Some(true));
        assert_eq!(run(&["--retag"]).retag_name, None);
        assert_eq!(
            run(&["--retag", "v1.2.3"]).retag_name.as_deref(),
            Some("v1.2.3")
        );
        assert_eq!(
            run(&["--retag=v1.2.3"]).retag_name.as_deref(),
            Some("v1.2.3")
        );
        // like `--tag`, the optional value never swallows the next flag
        let raw = run(&["--retag", "-y"]);
        assert_eq!(raw.retag_name, None);
        assert_eq!(raw.yes, Some(true));
        // and it is not a boolean, so `--no-retag` is not a thing
        assert!(parse_args(&["--no-retag"]).is_err());
    }

    #[test]
    fn no_arguments_means_prompt() {
        let raw = run(&[]);
        assert_eq!(raw.release, None);
    }

    #[test]
    fn positional_and_release_are_equivalent() {
        assert_eq!(run(&["patch"]).release, Some(Level::Patch));
        assert_eq!(run(&["--release", "patch"]).release, Some(Level::Patch));
        assert_eq!(
            run(&["1.2.3"]).release,
            Some(Level::Explicit("1.2.3".parse().unwrap()))
        );
        assert_eq!(run(&["--release=1.2.3"]).release, run(&["1.2.3"]).release);
        assert!(parse_args(&["patch", "--release", "minor"]).is_err());
        assert_eq!(
            run(&["patch", "--release", "patch"]).release,
            Some(Level::Patch)
        );
        assert!(parse_args(&["patch", "minor"]).is_err());
        assert!(parse_args(&["nonsense"]).is_err());
    }

    #[test]
    fn short_options_are_the_documented_ones() {
        let raw = run(&["patch", "-a", "-p", "-y", "-r", "-q"]);
        assert_eq!(raw.all, Some(true));
        assert_eq!(raw.push, Some(true));
        assert_eq!(raw.yes, Some(true));
        assert_eq!(raw.recursive, Some(true));
        assert_eq!(raw.quiet, Some(true));
        // `-r` is recursive, not release:
        assert_eq!(raw.release, Some(Level::Patch));
    }

    #[test]
    fn opt_in_flags_are_defaulted_to_unset() {
        let raw = run(&["patch"]);
        assert_eq!(raw.commit, None);
        assert_eq!(raw.tag, None);
        assert_eq!(raw.push, None);
        assert_eq!(raw.sign, None);
    }

    #[test]
    fn optional_values() {
        assert_eq!(run(&["patch", "-c"]).commit, Some(true));
        assert_eq!(run(&["patch", "-c"]).commit_message, None);
        assert_eq!(
            run(&["patch", "-c", "release {version}"])
                .commit_message
                .as_deref(),
            Some("release {version}")
        );
        assert_eq!(run(&["patch", "--tag"]).tag, Some(true));
        assert_eq!(
            run(&["patch", "--tag", "v{version}"]).tag_name.as_deref(),
            Some("v{version}")
        );
        assert_eq!(
            run(&["patch", "--tag=release-{version}"])
                .tag_name
                .as_deref(),
            Some("release-{version}")
        );
        // an optional value never swallows the next flag
        let raw = run(&["patch", "--tag", "-y"]);
        assert_eq!(raw.tag, Some(true));
        assert_eq!(raw.tag_name, None);
        assert_eq!(raw.yes, Some(true));
    }

    #[test]
    fn negative_forms() {
        let raw = run(&[
            "patch",
            "--no-push",
            "--no-tag",
            "--no-git-check",
            "--no-lockfile",
        ]);
        assert_eq!(raw.push, Some(false));
        assert_eq!(raw.tag, Some(false));
        assert_eq!(raw.git_check, Some(false));
        assert_eq!(raw.lockfile, Some(false));
        assert_eq!(run(&["--no-commit"]).commit, Some(false));
        assert!(parse_args(&["--no-sign=1"]).is_err());
    }

    #[test]
    fn no_sign_is_recorded_as_an_explicit_false() {
        assert_eq!(run(&["patch", "--no-sign"]).sign, Some(false));
        assert_eq!(run(&["patch"]).sign, None);
        assert_eq!(run(&["patch", "--sign"]).sign, Some(true));
    }

    #[test]
    fn value_options() {
        let raw = run(&[
            "--release",
            "prepatch",
            "--preid",
            "rc",
            "-x",
            "cargo test",
            "--current-version",
            "v0.0.2",
            "--commit-window",
            "5",
            "--configFilePath",
            "cfg.toml",
        ]);
        assert_eq!(raw.release, Some(Level::PrePatch));
        assert_eq!(raw.preid.as_deref(), Some("rc"));
        assert_eq!(raw.execute.as_deref(), Some("cargo test"));
        assert_eq!(raw.current_version.as_deref(), Some("v0.0.2"));
        assert_eq!(raw.commit_window, Some(5));
        assert_eq!(raw.config_path.as_deref(), Some("cfg.toml"));
        assert_eq!(
            run(&["--config-file-path", "cfg.toml"])
                .config_path
                .as_deref(),
            Some("cfg.toml")
        );
    }

    #[test]
    fn required_values_are_required() {
        assert!(parse_args(&["--release"]).is_err());
        assert!(parse_args(&["--preid", "--yes"]).is_err());
        assert!(parse_args(&["-x"]).is_err());
    }

    #[test]
    fn clustered_short_options() {
        let raw = run(&["patch", "-ay"]);
        assert_eq!(raw.all, Some(true));
        assert_eq!(raw.yes, Some(true));
        // `-t` takes an optional value, so the rest of the cluster is the value
        let raw = run(&["patch", "-tv1.0.0"]);
        assert_eq!(raw.tag, Some(true));
        assert_eq!(raw.tag_name.as_deref(), Some("v1.0.0"));
    }

    #[test]
    fn help_and_version_win() {
        assert_eq!(parse_args(&["--help"]).unwrap(), Parsed::Help);
        assert_eq!(parse_args(&["-h"]).unwrap(), Parsed::Help);
        assert_eq!(
            parse_args(&["patch", "--version"]).unwrap(),
            Parsed::Version
        );
        assert_eq!(parse_args(&["-V"]).unwrap(), Parsed::Version);
    }

    #[test]
    fn unknown_options_are_rejected() {
        assert!(parse_args(&["--nope"]).is_err());
        assert!(parse_args(&["-z"]).is_err());
        assert!(
            parse_args(&["--pr"]).is_err(),
            "there is no --pr in this tool"
        );
    }

    #[test]
    fn double_dash_ends_the_options() {
        assert_eq!(run(&["--", "patch"]).release, Some(Level::Patch));
        assert!(parse_args(&["--", "--help"]).is_err());
    }

    #[test]
    fn prompt_is_a_level() {
        assert_eq!(run(&["--release", "prompt"]).release, Some(Level::Prompt));
        assert_eq!(run(&["as-is"]).release, Some(Level::AsIs));
    }

    #[test]
    fn help_mentions_every_documented_option() {
        let text = help();
        for option in [
            "--release",
            "--retag",
            "--preid",
            "--all",
            "--git-check",
            "--commit",
            "--tag",
            "--sign",
            "--push",
            "--yes",
            "--recursive",
            "--verify",
            "--ignore-scripts",
            "--execute",
            "--current-version",
            "--print-commits",
            "--configFilePath",
            "--quiet",
        ] {
            assert!(text.contains(option), "`{option}` is missing from --help");
        }
    }
}
