//! A small semver implementation: parsing, precedence, `node-semver`-compatible
//! increments, and enough of Cargo's version-requirement syntax to *check*
//! whether a requirement still accepts the new version.
//!
//! It is written from scratch because the tool has no dependencies; the
//! `inc()` rules are a deliberate port of node-semver's `SemVer#inc`, since the
//! reference implementation (bumpp) delegates to it and the results must match.
//!
//! This is the root of the module and the only thing `src/lib.rs` names: the
//! version model lives in `version`, the pre-release precedence rule in
//! `compare`, the parse helpers in `parse`, and the requirement checker in
//! `requirement`. Everything that used to sit at `crate::semver::…` is
//! re-exported here.

mod compare;
mod parse;
mod requirement;
mod version;

#[cfg(test)]
mod tests;

pub use requirement::req_matches;
pub use version::{Level, PreId, Version};
