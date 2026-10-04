//! Command line parsing and `--help`.
//!
//! Option names and short options follow the reference implementation (bumpp),
//! so muscle memory carries over — including the two that are easy to guess
//! wrong: `-r` is `--recursive`, not `--release`, and `-p` is `--push`, not
//! `--preid`.

mod args;
mod flags;
mod parser;
mod usage;

pub use parser::{parse, Parsed};
pub use usage::help;
