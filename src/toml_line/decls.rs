//! Turning the fields of a dependency table into a declaration. A dependency can
//! be spelled as a shorthand string, as an inline table or as a dotted
//! `[dependencies.foo]` section, and this is where all three become a `version`,
//! a `path` and `workspace = true`.

use super::types::{DepDecl, Found, KeyValue, Value};

/// Issue text used when a dependency table simply has no `version` key.
pub const ISSUE_NO_VERSION: &str = "no `version` key";

pub(super) fn collect_fields(decl: &mut DepDecl, fields: &[KeyValue], closed: bool) {
    for field in fields {
        match field.key().as_str() {
            "version" => match &field.value {
                Value::Str { text, inner } => {
                    decl.version = Some(Found {
                        text: text.clone(),
                        line: field.line,
                        inner: *inner,
                        section: decl.section.clone(),
                    });
                }
                Value::Unsupported(reason) => decl.issue = Some(reason.clone()),
                _ => decl.issue = Some("`version` is not a string".to_string()),
            },
            "path" => {
                let text = match &field.value {
                    Value::Str { text, .. } => Some(text.as_str()),
                    // Bare paths like `path = /usr/local/lib` are legal TOML;
                    // accepting them keeps an unquoted path from being dropped
                    // silently.
                    Value::Bare { text, .. } => Some(text.as_str()),
                    _ => None,
                };
                if let Some(text) = text {
                    decl.path = Some(text.to_string());
                }
            }
            "workspace" if field.value.as_bool() == Some(true) => decl.uses_workspace = true,
            _ => {}
        }
    }
    if decl.version.is_none() && decl.issue.is_none() {
        decl.issue = Some(if closed {
            ISSUE_NO_VERSION.to_string()
        } else {
            "the dependency table spans multiple lines".to_string()
        });
    }
}

/// Index of the dependency-section part in a section path, if any.
pub(super) fn dep_section_index(path: &[String]) -> Option<usize> {
    path.iter().rposition(|part| {
        matches!(
            part.as_str(),
            "dependencies" | "dev-dependencies" | "build-dependencies"
        )
    })
}

pub(super) fn found_string(entry: &KeyValue, expected: &[&str], header: &str) -> Option<Found> {
    if entry.parts.len() != expected.len() {
        return None;
    }
    if !entry.parts.iter().zip(expected).all(|(a, b)| a == b) {
        return None;
    }
    match &entry.value {
        Value::Str { text, inner } => Some(Found {
            text: text.clone(),
            line: entry.line,
            inner: *inner,
            section: header.to_string(),
        }),
        _ => None,
    }
}
