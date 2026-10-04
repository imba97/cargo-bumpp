//! Reading the `BUMPP_*` environment variables, which win over the config file.

use crate::error::{Error, Result};
use crate::options::RawOptions;

use super::parse::parse_bool;

/// Options from the environment. These win over the config file, so CI can
/// override a repository's defaults without touching the file.
pub fn from_env() -> Result<RawOptions> {
    from_env_with(|name| std::env::var(name).ok())
}

pub(super) fn from_env_with(get: impl Fn(&str) -> Option<String>) -> Result<RawOptions> {
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
