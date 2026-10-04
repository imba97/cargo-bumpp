//! Building the plan: which files, which lines, which values.
//!
//! Everything the tool writes is decided here, before anything is written. The
//! plan is also what gets printed, line number by line number, so "did it miss a
//! spot?" is answerable *before* the files change — the design calls this the
//! replacement for a dry-run mode.

mod builder;
mod detect;
mod helpers;
mod levels;
mod source;
mod types;

pub use builder::build;
pub use detect::detect_current;
pub use levels::increment;
pub use source::VersionSource;
pub use types::{Bump, FilePlan, Plan};
