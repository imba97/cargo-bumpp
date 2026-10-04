//! Line-oriented reading and rewriting of `Cargo.toml`.
//!
//! A real TOML parser (`toml_edit`) would be more robust, and most tools use
//! one. It is also the single biggest dependency this crate could take on, and
//! "installs in seconds, audits in a minute" is the whole point. So this module
//! edits line by line and **keeps every byte of the original formatting** except
//! the version values it replaces.
//!
//! The trade-off is explicit: rare spellings (a version value that spans lines,
//! for instance) are not handled. When that happens the scanner says so instead
//! of silently skipping the spot — see [`DepDecl::issue`].

mod decls;
mod entries;
mod manifest;
mod paths;
mod syntax;
mod types;

pub use decls::ISSUE_NO_VERSION;
pub use paths::{normalize, resolve_path, same_dir};
pub use types::{DepDecl, Edit, Found, KeyValue, Manifest, Value};
