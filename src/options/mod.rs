//! Option merging and the rules that derive one option's default from another.
//!
//! Precedence, highest first: command line, environment, `bumpp.toml`, built-in
//! defaults. Merging happens on [`RawOptions`], where every field is optional;
//! [`Options::resolve`] then applies the defaults and the cross-option rules.

mod defaults;
mod raw;
mod resolved;
mod validate;

#[cfg(test)]
mod tests;

pub use defaults::{
    DEFAULT_COMMIT_MESSAGE, DEFAULT_COMMIT_WINDOW, DEFAULT_PREID, DEFAULT_TAG_NAME,
};
pub use raw::RawOptions;
pub use resolved::Options;
