//! `bumpp.toml` and the environment variables.
//!
//! The config file is *optional*: without it every option takes its built-in
//! default and the tool is fully usable. It is a flat `key = value` file, and
//! only the subset of TOML that needs is parsed — a few dozen lines, not a TOML
//! implementation, which is why it does not contradict the no-dependencies rule.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::options::RawOptions;

/// The file searched for at the workspace root.
pub const CONFIG_FILE_NAME: &str = "bumpp.toml";

/// Config keys, with the type expected and whether the key is part of the
/// documented surface.
struct KeySpec {
    name: &'static str,
    kind: Kind,
    help: &'static str,
}

#[derive(PartialEq, Clone, Copy)]
enum Kind {
    Bool,
    String,
    Integer,
}

const KEYS: &[KeySpec] = &[
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
const CLI_ONLY: &[&str] = &[
    "release",
    "current-version",
    "execute",
    "yes",
    "config-path",
    "config-file-path",
];

/// Load the config file for a workspace.
///
/// `explicit` is `--configFilePath`: when given, the file must exist. Otherwise
/// `<workspace_root>/bumpp.toml` is used when present, and silence is fine.
pub fn load(explicit: Option<&str>, workspace_root: &Path) -> Result<RawOptions> {
    let path: PathBuf = match explicit {
        Some(given) => PathBuf::from(given),
        None => workspace_root.join(CONFIG_FILE_NAME),
    };
    if !path.exists() {
        if explicit.is_some() {
            return Err(Error::usage(format!(
                "config file `{}` does not exist",
                path.display()
            )));
        }
        return Ok(RawOptions::default());
    }
    let text = std::fs::read_to_string(&path).map_err(|err| {
        Error::usage(format!(
            "cannot read config file `{}`: {err}",
            path.display()
        ))
    })?;
    parse(&text).map_err(|err| err.with_hint(format!("in {}", path.display())))
}

/// Parse the flat `key = value` subset.
pub fn parse(text: &str) -> Result<RawOptions> {
    let mut raw = RawOptions::default();
    for (index, line) in text.lines().enumerate() {
        let line_number = index + 1;
        let content = strip_comment(line);
        let content = content.trim();
        if content.is_empty() {
            continue;
        }
        if content.starts_with('[') {
            return Err(config_error(
                line_number,
                "bumpp.toml is flat: sections are not supported",
            ));
        }
        let Some((key, value)) = content.split_once('=') else {
            return Err(config_error(line_number, "expected `key = value`"));
        };
        let key = key
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .replace('_', "-");
        let value = value.trim();
        if value.is_empty() {
            return Err(config_error(line_number, "missing value"));
        }
        let spec = KEYS.iter().find(|spec| spec.name == key).ok_or_else(|| {
            if CLI_ONLY.contains(&key.as_str()) {
                config_error(
                    line_number,
                    format!("`{key}` can only be given on the command line, not in a config file"),
                )
            } else {
                config_error(line_number, format!("unknown key `{key}`")).with_hint(keys_help())
            }
        })?;

        match spec.kind {
            Kind::Bool => {
                let parsed = parse_bool(value).ok_or_else(|| {
                    config_error(
                        line_number,
                        format!("`{key}` expects true or false, found `{value}`"),
                    )
                })?;
                set_bool(&mut raw, spec.name, parsed);
            }
            Kind::String => {
                let parsed = parse_string(value).ok_or_else(|| {
                    config_error(
                        line_number,
                        format!("`{key}` expects a quoted string, found `{value}`"),
                    )
                })?;
                set_string(&mut raw, spec.name, parsed);
            }
            Kind::Integer => {
                let parsed = value.parse::<usize>().map_err(|_| {
                    config_error(
                        line_number,
                        format!("`{key}` expects an integer, found `{value}`"),
                    )
                })?;
                raw.commit_window = Some(parsed);
            }
        }
    }
    Ok(raw)
}

/// The known keys, with what they do — shown when a key is misspelled, because
/// a silently ignored key looks like "the option does not work".
fn keys_help() -> String {
    let width = KEYS.iter().map(|spec| spec.name.len()).max().unwrap_or(0);
    let mut text = String::from("known keys:");
    for spec in KEYS {
        text.push_str(&format!(
            "\n  {:<width$}  {}",
            spec.name,
            spec.help,
            width = width
        ));
    }
    text
}

fn config_error(line: usize, message: impl Into<String>) -> Error {
    Error::usage(format!("bumpp.toml line {line}: {}", message.into()))
}

fn set_bool(raw: &mut RawOptions, key: &str, value: bool) {
    match key {
        "commit" => raw.commit = Some(value),
        "tag" => raw.tag = Some(value),
        "push" => raw.push = Some(value),
        "sign" => raw.sign = Some(value),
        "all" => raw.all = Some(value),
        "git-check" => raw.git_check = Some(value),
        "verify" => raw.verify = Some(value),
        "lockfile" => raw.lockfile = Some(value),
        "recursive" => raw.recursive = Some(value),
        "quiet" => raw.quiet = Some(value),
        "print-commits" => raw.print_commits = Some(value),
        other => unreachable!("unhandled boolean key `{other}`"),
    }
}

fn set_string(raw: &mut RawOptions, key: &str, value: String) {
    match key {
        "commit-message" => raw.commit_message = Some(value),
        "tag-name" => raw.tag_name = Some(value),
        "preid" => raw.preid = Some(value),
        other => unreachable!("unhandled string key `{other}`"),
    }
}

/// Remove a `#` comment, ignoring `#` inside a quoted string.
fn strip_comment(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut quote: Option<u8> = None;
    let mut escaped = false;
    for (index, byte) in bytes.iter().enumerate() {
        match quote {
            Some(q) => {
                if escaped {
                    escaped = false;
                } else if *byte == b'\\' && q == b'"' {
                    escaped = true;
                } else if *byte == q {
                    quote = None;
                }
            }
            None => match byte {
                b'"' | b'\'' => quote = Some(*byte),
                b'#' => return line[..index].to_string(),
                _ => {}
            },
        }
    }
    line.to_string()
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Some(true),
        "false" | "no" | "off" | "0" => Some(false),
        _ => None,
    }
}

fn parse_string(value: &str) -> Option<String> {
    let value = value.trim();
    if value.len() >= 2 {
        let first = value.as_bytes()[0];
        let last = value.as_bytes()[value.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return Some(value[1..value.len() - 1].to_string());
        }
    }
    None
}

/// Options from the environment. These win over the config file, so CI can
/// override a repository's defaults without touching the file.
pub fn from_env() -> Result<RawOptions> {
    from_env_with(|name| std::env::var(name).ok())
}

fn from_env_with(get: impl Fn(&str) -> Option<String>) -> Result<RawOptions> {
    let mut raw = RawOptions::default();

    if let Some(value) = get("BUMPP_COMMIT") {
        raw.commit = Some(env_bool("BUMPP_COMMIT", &value)?);
    }
    if let Some(value) = get("BUMPP_TAG") {
        raw.tag = Some(env_bool("BUMPP_TAG", &value)?);
    }
    if let Some(value) = get("BUMPP_PUSH") {
        raw.push = Some(env_bool("BUMPP_PUSH", &value)?);
    }
    if let Some(value) = get("BUMPP_PREID") {
        raw.preid = Some(value);
    }
    if let Some(value) = get("BUMPP_COMMIT_MESSAGE") {
        raw.commit_message = Some(value);
    }
    Ok(raw)
}

fn env_bool(name: &str, value: &str) -> Result<bool> {
    parse_bool(value)
        .ok_or_else(|| Error::usage(format!("{name} expects true or false, found `{value}`")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_documented_file() {
        let raw = parse(
            r#"
# bumpp.toml
commit = true
tag = true
push = false
commit-message = "chore: release v{version}"   # comment
tag-name = 'v{version}'
preid = "beta"
sign = false
"#,
        )
        .unwrap();
        assert_eq!(raw.commit, Some(true));
        assert_eq!(raw.push, Some(false));
        assert_eq!(
            raw.commit_message.as_deref(),
            Some("chore: release v{version}")
        );
        assert_eq!(raw.tag_name.as_deref(), Some("v{version}"));
        assert_eq!(raw.preid.as_deref(), Some("beta"));
        assert_eq!(raw.sign, Some(false));
    }

    #[test]
    fn an_empty_file_changes_nothing() {
        assert_eq!(parse("\n# nothing here\n").unwrap(), RawOptions::default());
    }

    #[test]
    fn rejects_unknown_keys() {
        let err = parse("commti-message = \"x\"\n").unwrap_err();
        assert!(
            err.message().contains("unknown key `commti-message`"),
            "{err}"
        );
        assert!(
            err.hint().unwrap().contains("commit-message"),
            "the hint lists the known keys: {err}"
        );
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn rejects_sections_and_cli_only_keys() {
        assert!(parse("[options]\ncommit = true\n")
            .unwrap_err()
            .message()
            .contains("flat"));
        assert!(parse("execute = \"rm -rf /\"\n")
            .unwrap_err()
            .message()
            .contains("command line"));
    }

    #[test]
    fn rejects_wrong_types() {
        assert!(parse("commit = \"yes\"\n").is_err());
        assert!(parse("preid = beta\n").is_err());
        assert!(parse("commit-window = \"10\"\n").is_err());
        assert!(parse("commit = 1\n").unwrap().commit.unwrap());
        assert_eq!(
            parse("commit-window = 20\n").unwrap().commit_window,
            Some(20)
        );
    }

    #[test]
    fn accepts_snake_case_aliases() {
        assert_eq!(
            parse("commit_message = \"x\"\n")
                .unwrap()
                .commit_message
                .as_deref(),
            Some("x")
        );
    }

    #[test]
    fn a_comment_marker_inside_a_string_is_kept() {
        assert_eq!(
            parse("commit-message = \"release #1\"\n")
                .unwrap()
                .commit_message
                .as_deref(),
            Some("release #1")
        );
    }

    #[test]
    fn environment_overrides() {
        let env = from_env_with(|name| match name {
            "BUMPP_PUSH" => Some("false".to_string()),
            "BUMPP_PREID" => Some("rc".to_string()),
            "BUMPP_COMMIT_MESSAGE" => Some("release {version}".to_string()),
            _ => None,
        })
        .unwrap();
        assert_eq!(env.push, Some(false));
        assert_eq!(env.preid.as_deref(), Some("rc"));
        assert_eq!(env.commit_message.as_deref(), Some("release {version}"));
    }

    #[test]
    fn environment_rejects_nonsense_booleans() {
        let err = from_env_with(|name| (name == "BUMPP_COMMIT").then(|| "maybe".to_string()))
            .unwrap_err();
        assert!(err.message().contains("BUMPP_COMMIT"));
    }

    #[test]
    fn every_documented_key_is_understood() {
        for spec in KEYS {
            let value = match spec.kind {
                Kind::Bool => "true",
                Kind::String => "\"x\"",
                Kind::Integer => "3",
            };
            let text = format!("{} = {}\n", spec.name, value);
            parse(&text).unwrap_or_else(|err| panic!("`{}` is not handled: {err}", spec.name));
        }
    }

    #[test]
    fn missing_explicit_config_file_is_an_error_but_a_missing_default_is_not() {
        let missing = std::env::temp_dir().join("bumpp-does-not-exist.toml");
        assert!(load(Some(missing.to_str().unwrap()), Path::new(".")).is_err());
        assert_eq!(
            load(None, Path::new(&missing)).unwrap(),
            RawOptions::default()
        );
    }
}
