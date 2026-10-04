//! A small JSON reader, only used to read `cargo metadata` output.
//!
//! Implementing it here keeps the crate dependency-free; the parser is strict
//! (RFC 8259), depth-limited, and has no opinion about the schema.

mod error;
mod parser;
mod value;

#[cfg(test)]
mod tests;

pub use error::JsonError;
pub use value::Json;
