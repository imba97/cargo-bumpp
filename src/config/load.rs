//! Locating and reading the config file: an explicit `--configFilePath`, or
//! `bumpp.toml` at the workspace root when it happens to be there.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::options::RawOptions;

use super::keys::CONFIG_FILE_NAME;
use super::parse::parse;

/// Load the config file for a workspace.
///
/// `explicit` is `--configFilePath`: when given, the file must exist. Otherwise
/// `<workspace_root>/bumpp.toml` is used when present, and silence is fine.
pub fn load(explicit: Option<&str>, workspace_root: &Path) -> Result<RawOptions> {
    let path: PathBuf = match explicit {
        Some(given) => PathBuf::from(given),
        None => workspace_root.join(CONFIG_FILE_NAME),
    };
    if !path.exists() {
        if explicit.is_some() {
            return Err(Error::usage(format!(
                "config file `{}` does not exist",
                path.display()
            )));
        }
        return Ok(RawOptions::default());
    }
    let text = std::fs::read_to_string(&path).map_err(|err| {
        Error::usage(format!(
            "cannot read config file `{}`: {err}",
            path.display()
        ))
    })?;
    parse(&text).map_err(|err| err.with_hint(format!("in {}", path.display())))
}
