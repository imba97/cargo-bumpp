//! `bumpp.toml` and the environment variables.
//!
//! The config file is *optional*: without it every option takes its built-in
//! default and the tool is fully usable. It is a flat `key = value` file, and
//! only the subset of TOML that needs is parsed — a few dozen lines, not a TOML
//! implementation, which is why it does not contradict the no-dependencies rule.

mod env;
mod keys;
mod load;
mod parse;

pub use env::from_env;
pub use keys::CONFIG_FILE_NAME;
pub use load::load;
pub use parse::parse;
