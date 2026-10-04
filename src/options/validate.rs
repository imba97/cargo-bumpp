//! The check that a pre-release identifier is usable. It is its own file because
//! `--preid` has rules of its own and `resolve` is long enough without them.

use crate::error::{Error, Result};

pub(super) fn validate_preid(preid: &str) -> Result<()> {
    if preid.is_empty() {
        return Err(Error::usage("--preid cannot be empty"));
    }
    if !preid
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(Error::usage(format!(
            "--preid `{preid}` is not a valid pre-release identifier (letters, digits and `-` only)"
        )));
    }
    if preid.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Error::usage(format!(
            "--preid `{preid}` must not be numeric-only: it would be read as a pre-release counter"
        )));
    }
    Ok(())
}
