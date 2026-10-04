//! The shapes the rest of the module works with: a manifest's lines, the string
//! values found in them, and the key/value, dependency and edit records built on
//! top. They are separate from the code that fills them in so the scanner and the
//! manifest API can name the same types.

use std::path::PathBuf;

use super::entries::ScopedEntry;

/// A manifest file, split into lines with its original line endings.
#[derive(Debug, Clone)]
pub struct Manifest {
    pub path: PathBuf,
    pub(super) lines: Vec<String>,
    pub(super) eol: String,
    pub(super) trailing_newline: bool,
    /// Eagerly scanned on construction: `workspace_package_version`,
    /// `package_version` and `dependency_decls` all need the same per-line
    /// entries, and the three callers were each rebuilding it before. Keeping
    /// it on the struct (rather than behind `OnceCell`) preserves `Clone`.
    pub(super) entries: Vec<ScopedEntry>,
}

/// A string value somewhere in a manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub text: String,
    /// 0-based line index.
    pub line: usize,
    /// Byte range of the *contents* of the string (quotes excluded) within that
    /// line, so an edit preserves the original quoting style.
    pub inner: (usize, usize),
    /// The section header the value lives in, e.g. `[workspace.dependencies]`.
    pub section: String,
}

/// One `key = value` pair inside a section (or inside an inline table).
#[derive(Debug, Clone, PartialEq)]
pub struct KeyValue {
    /// Dotted key parts, unquoted: `["version"]`, `["version", "workspace"]`.
    pub parts: Vec<String>,
    pub line: usize,
    pub value: Value,
}

impl KeyValue {
    pub fn key(&self) -> String {
        self.parts.join(".")
    }
}

/// A parsed value, kept shallow: only strings and inline tables are inspected.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str {
        text: String,
        inner: (usize, usize),
    },
    Bool(bool),
    Integer(i64),
    Bare {
        text: String,
        inner: (usize, usize),
    },
    Array,
    /// An inline table. `closed` is false when it continues on later lines, in
    /// which case the entries seen here are still usable.
    Table {
        entries: Vec<KeyValue>,
        closed: bool,
    },
    /// Something this scanner will not guess about; the string says why.
    Unsupported(String),
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str { text, .. } => Some(text),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

/// A dependency declaration found in a manifest.
#[derive(Debug, Clone, PartialEq)]
pub struct DepDecl {
    pub name: String,
    /// Section header it was declared in, for reporting.
    pub section: String,
    /// Line of the declaration.
    pub line: usize,
    /// The `version = "..."` value, when it could be located.
    pub version: Option<Found>,
    /// The `path = "..."` value, when present.
    pub path: Option<String>,
    /// `workspace = true`, i.e. the version comes from the root manifest.
    pub uses_workspace: bool,
    /// Set when the declaration exists but the scanner refused to guess.
    pub issue: Option<String>,
}

/// A pending rewrite: replace the string contents at `line` between `inner.0`
/// and `inner.1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub line: usize,
    pub inner: (usize, usize),
    pub old: String,
    pub new: String,
    /// What this edit is, for the plan output: `[workspace.package]`.
    pub label: String,
}

impl Edit {
    pub(super) fn apply(&self, line: &mut String) -> Result<(), String> {
        let (start, end) = self.inner;
        if end > line.len()
            || start > end
            || !line.is_char_boundary(start)
            || !line.is_char_boundary(end)
        {
            return Err(format!(
                "line {}: value range is out of bounds",
                self.line + 1
            ));
        }
        if line[start..end] != self.old {
            return Err(format!(
                "line {}: expected `{}` but found `{}`",
                self.line + 1,
                self.old,
                &line[start..end]
            ));
        }
        line.replace_range(start..end, &self.new);
        Ok(())
    }
}
