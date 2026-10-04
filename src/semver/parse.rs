//! The parse helpers `Version::parse` is built from: one numeric component, the
//! build metadata, and the pre-release string.
//!
//! They are their own file because the requirement checker splits identifiers
//! the same way, so the rules (and the error messages) have to exist once, next
//! to neither of their two callers in particular.

use super::version::PreId;

pub(super) fn parse_number(part: &str, input: &str) -> Result<u64, String> {
    let trimmed = part.trim();
    if trimmed.is_empty() || !trimmed.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!(
            "`{input}` is not a valid version (bad number `{part}`)"
        ));
    }
    trimmed
        .parse::<u64>()
        .map_err(|_| format!("`{input}` is not a valid version (number out of range: `{part}`)"))
}

pub(super) fn split_identifiers(part: &str, what: &str) -> Result<Vec<String>, String> {
    let ids: Vec<String> = part.split('.').map(|s| s.to_string()).collect();
    if ids.iter().any(|s| s.is_empty()) {
        return Err(format!("invalid {what} `{part}`"));
    }
    Ok(ids)
}

pub(super) fn split_pre(part: &str) -> Result<Vec<PreId>, String> {
    if part.is_empty() {
        return Err("empty pre-release identifier".to_string());
    }
    let mut out = Vec::new();
    for id in part.split('.') {
        if id.is_empty() {
            return Err(format!("invalid pre-release `{part}`"));
        }
        if id.bytes().all(|b| b.is_ascii_digit()) {
            match id.parse::<u64>() {
                Ok(n) => out.push(PreId::Num(n)),
                Err(_) => {
                    return Err(format!(
                        "numeric pre-release identifier out of range: `{id}`"
                    ))
                }
            }
        } else {
            out.push(PreId::Alpha(id.to_string()));
        }
    }
    Ok(out)
}
