//! Template rendering for commit messages and tag names.
//!
//! Three tiers, matching the reference implementation exactly:
//!
//! | template contains | result |
//! | --- | --- |
//! | any known named token | named tokens are replaced, `%s` is left alone |
//! | no named token, but `%s` | `%s` becomes the new version |
//! | neither | the new version is appended at the end |

/// Values available to a template.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tokens {
    pub version: String,
    pub old_version: String,
    pub tag: String,
    pub release_type: String,
    pub major: String,
    pub minor: String,
    pub patch: String,
    pub date: String,
}

/// The named tokens, in the order they are documented.
pub const NAMED_TOKENS: &[&str] = &[
    "version",
    "oldVersion",
    "tag",
    "releaseType",
    "major",
    "minor",
    "patch",
    "date",
];

impl Tokens {
    fn lookup(&self, name: &str) -> Option<&str> {
        let value = match name {
            "version" => &self.version,
            "oldVersion" => &self.old_version,
            "tag" => &self.tag,
            "releaseType" => &self.release_type,
            "major" => &self.major,
            "minor" => &self.minor,
            "patch" => &self.patch,
            "date" => &self.date,
            _ => return None,
        };
        Some(value.as_str())
    }
}

/// True when the template mentions at least one known named token.
pub fn has_named_token(template: &str) -> bool {
    NAMED_TOKENS
        .iter()
        .any(|name| template.contains(&format!("{{{name}}}")))
}

/// Render `template` with `tokens`, following the three-tier rule.
pub fn render(template: &str, tokens: &Tokens) -> String {
    if has_named_token(template) {
        let mut out = String::with_capacity(template.len());
        let mut rest = template;
        while let Some(open) = rest.find('{') {
            out.push_str(&rest[..open]);
            let after = &rest[open + 1..];
            match after.find('}') {
                Some(close) => {
                    let name = &after[..close];
                    match tokens.lookup(name) {
                        Some(value) => out.push_str(value),
                        None => {
                            out.push('{');
                            out.push_str(name);
                            out.push('}');
                        }
                    }
                    rest = &after[close + 1..];
                }
                None => {
                    out.push_str(&rest[open..]);
                    rest = "";
                }
            }
        }
        out.push_str(rest);
        return out;
    }

    if template.contains("%s") {
        return template.replace("%s", &tokens.version);
    }

    // Neither: append. `--commit "chore: release v"` is a legal spelling.
    let mut out = String::with_capacity(template.len() + tokens.version.len());
    out.push_str(template);
    out.push_str(&tokens.version);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens() -> Tokens {
        Tokens {
            version: "1.2.3".to_string(),
            old_version: "1.2.2".to_string(),
            tag: "v1.2.3".to_string(),
            release_type: "patch".to_string(),
            major: "1".to_string(),
            minor: "2".to_string(),
            patch: "3".to_string(),
            date: "2026-07-28".to_string(),
        }
    }

    #[test]
    fn named_tokens() {
        assert_eq!(
            render("chore: release v{version}", &tokens()),
            "chore: release v1.2.3"
        );
        assert_eq!(render("{tag} / {oldVersion}", &tokens()), "v1.2.3 / 1.2.2");
        assert_eq!(render("{major}.{minor}.{patch}", &tokens()), "1.2.3");
        assert_eq!(
            render("release {version} ({date})", &tokens()),
            "release 1.2.3 (2026-07-28)"
        );
    }

    #[test]
    fn percent_s_is_shadowed_by_any_named_token() {
        assert_eq!(render("release %s", &tokens()), "release 1.2.3");
        assert_eq!(
            render("release %s v{version}", &tokens()),
            "release %s v1.2.3"
        );
    }

    #[test]
    fn appends_when_neither_is_present() {
        assert_eq!(
            render("chore: release v", &tokens()),
            "chore: release v1.2.3"
        );
        assert_eq!(render("", &tokens()), "1.2.3");
        assert_eq!(render("release-", &tokens()), "release-1.2.3");
    }

    #[test]
    fn unknown_tokens_are_left_alone() {
        assert_eq!(render("{nope} {version}", &tokens()), "{nope} 1.2.3");
        assert_eq!(render("a { b", &tokens()), "a { b1.2.3");
    }

    #[test]
    fn release_type_is_empty_for_an_explicit_version() {
        let mut t = tokens();
        t.release_type = String::new();
        assert_eq!(
            render("chore({releaseType}): release", &t),
            "chore(): release"
        );
    }
}
