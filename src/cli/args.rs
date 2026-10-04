//! The cursor the parser walks the argument list with.
//!
//! It owns the rules that are easy to get wrong and are therefore worth reading
//! on their own: when a flag may take the next argument as its value, and when a
//! value that starts with `-` is refused.

use crate::error::{Error, Result};

pub(super) struct Args<'a> {
    pub(super) items: &'a [String],
    pub(super) index: usize,
}

impl<'a> Args<'a> {
    pub(super) fn next(&mut self) -> Option<String> {
        let item = self.items.get(self.index).cloned();
        if item.is_some() {
            self.index += 1;
        }
        item
    }

    /// The value of a flag that requires one: `--flag=value` or `--flag value`.
    pub(super) fn value(&mut self, flag: &str, inline: Option<String>) -> Result<Option<String>> {
        if let Some(value) = inline {
            return Ok(Some(value));
        }
        match self.items.get(self.index) {
            // A value that starts with `-` still counts (e.g. `--execute -x`),
            // as long as it is not a bare `-`... but then `--tag -y` would be
            // read as tag="-y", which is worse. Values use `--flag=value` for
            // anything that starts with a dash.
            Some(next) if next.starts_with('-') && next != "-" => {
                Err(Error::usage(format!("`--{flag}` requires a value")))
            }
            Some(next) => {
                let value = next.clone();
                self.index += 1;
                Ok(Some(value))
            }
            None => Err(Error::usage(format!("`--{flag}` requires a value"))),
        }
    }

    /// The value of a flag whose value is optional: `--tag`, `--tag v1.0.0`.
    pub(super) fn optional_value(
        &mut self,
        flag: &str,
        inline: Option<String>,
    ) -> Result<Option<String>> {
        if let Some(value) = inline {
            return Ok(Some(value));
        }
        match self.items.get(self.index) {
            Some(next) if !next.starts_with('-') || next == "-" => {
                let value = next.clone();
                self.index += 1;
                Ok(Some(value))
            }
            _ => {
                let _ = flag;
                Ok(None)
            }
        }
    }
}
