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

use std::path::{Path, PathBuf};

/// A manifest file, split into lines with its original line endings.
#[derive(Debug, Clone)]
pub struct Manifest {
    pub path: PathBuf,
    lines: Vec<String>,
    eol: String,
    trailing_newline: bool,
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
    fn apply(&self, line: &mut String) -> Result<(), String> {
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

impl Manifest {
    pub fn read(path: impl Into<PathBuf>) -> std::io::Result<Manifest> {
        let path = path.into();
        let original = std::fs::read_to_string(&path)?;
        Ok(Manifest::from_text(path, original))
    }

    pub fn from_text(path: PathBuf, original: String) -> Manifest {
        let (lines, eol, trailing_newline) = split_lines(&original);
        Manifest {
            path,
            lines,
            eol,
            trailing_newline,
        }
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn line(&self, index: usize) -> Option<&str> {
        self.lines.get(index).map(|s| s.as_str())
    }

    /// Apply edits and return the new file contents. Every edit is verified
    /// against the current text first, so a stale plan cannot corrupt a file.
    pub fn render_with(&self, edits: &[Edit]) -> Result<String, String> {
        let mut lines = self.lines.clone();
        let mut ordered: Vec<&Edit> = edits.iter().collect();
        ordered.sort_by(|a, b| b.line.cmp(&a.line).then_with(|| b.inner.0.cmp(&a.inner.0)));
        for edit in ordered {
            let line = lines
                .get_mut(edit.line)
                .ok_or_else(|| format!("line {} does not exist", edit.line + 1))?;
            edit.apply(line)?;
        }
        Ok(self.join(&lines))
    }

    fn join(&self, lines: &[String]) -> String {
        let mut out = lines.join(&self.eol);
        if self.trailing_newline {
            out.push_str(&self.eol);
        }
        out
    }

    /// `[workspace.package] version = "..."`.
    pub fn workspace_package_version(&self) -> Option<Found> {
        self.entries().into_iter().find_map(|entry| {
            if entry.section_path == ["workspace", "package"] {
                found_string(&entry.item, &["version"], &entry.section)
            } else {
                None
            }
        })
    }

    /// `[package] version = "..."`, unless it is `version.workspace = true`.
    pub fn package_version(&self) -> Option<Found> {
        self.entries().into_iter().find_map(|entry| {
            if entry.section_path == ["package"] {
                found_string(&entry.item, &["version"], &entry.section)
            } else {
                None
            }
        })
    }

    /// Every dependency declaration in the file, across `[dependencies]`,
    /// `[dev-dependencies]`, `[build-dependencies]`, `[workspace.dependencies]`
    /// and their `[target.'cfg(..)'.dependencies]` / `[dependencies.foo]` forms.
    pub fn dependency_decls(&self) -> Vec<DepDecl> {
        let mut out = Vec::new();
        let entries = self.entries();
        let mut position = 0usize;

        while position < entries.len() {
            // All entries of one section belong together: the dotted form
            // `[dependencies.foo]` spreads one dependency over several lines.
            let section = entries[position].section.clone();
            let start = position;
            while position < entries.len() && entries[position].section == section {
                position += 1;
            }
            let group = &entries[start..position];

            let Some(index) = dep_section_index(&group[0].section_path) else {
                continue;
            };
            match group[0].section_path.get(index + 1).cloned() {
                // `[dependencies.foo]` with `version = "..."` / `path = "..."`
                Some(name) => {
                    let mut decl = new_decl(name, section, group[0].item.line);
                    let fields: Vec<KeyValue> =
                        group.iter().map(|entry| entry.item.clone()).collect();
                    collect_fields(&mut decl, &fields, true);
                    out.push(decl);
                }
                // `foo = { version = "..." }` or `foo = "1.2.3"`
                None => {
                    for entry in group {
                        let name = entry.item.key();
                        let mut decl = new_decl(name, section.clone(), entry.item.line);
                        match &entry.item.value {
                            Value::Table { entries, closed } => {
                                collect_fields(&mut decl, entries, *closed)
                            }
                            Value::Str { text, inner } => {
                                decl.version = Some(Found {
                                    text: text.clone(),
                                    line: entry.item.line,
                                    inner: *inner,
                                    section: section.clone(),
                                });
                            }
                            Value::Unsupported(reason) => decl.issue = Some(reason.clone()),
                            _ => {
                                decl.issue =
                                    Some("not a version string or an inline table".to_string())
                            }
                        }
                        out.push(decl);
                    }
                }
            }
        }
        out
    }

    /// Every `key = value` in the file, in order, with its section.
    fn entries(&self) -> Vec<ScopedEntry> {
        let mut out = Vec::new();
        let mut section = String::new();
        let mut section_path: Vec<String> = Vec::new();
        // A value that continues on later lines: (opening byte, depth).
        let mut container: Option<(u8, usize)> = None;

        for (index, line) in self.lines.iter().enumerate() {
            let trimmed = line.trim_start();
            let blank_or_comment = trimmed.is_empty() || trimmed.starts_with('#');

            if let Some((open, depth)) = container {
                if blank_or_comment {
                    continue;
                }
                let close = if open == b'{' { b'}' } else { b']' };
                match container_depth(line, open, close, depth) {
                    Some(0) => container = None,
                    Some(remaining) => container = Some((open, remaining)),
                    // An unreadable line leaves us inside the container: skipping
                    // too much is safer than inventing entries.
                    None => {}
                }
                continue;
            }

            if blank_or_comment {
                continue;
            }

            if trimmed.starts_with('[') {
                match parse_header(trimmed) {
                    Ok((header, path)) => {
                        section = header;
                        section_path = path;
                    }
                    Err(reason) => {
                        section = format!("<unparsed header: {reason}>");
                        section_path = Vec::new();
                    }
                }
                continue;
            }

            let indent = line.len() - trimmed.len();
            let mut scanner = Scanner::new(line, indent, index);
            let Some(parts) = scanner.key() else { continue };
            scanner.skip_space();
            if scanner.peek() != Some(b'=') {
                continue;
            }
            scanner.advance(1);
            let value = scanner.value();
            if let Some(open) = unclosed_container(&value) {
                // The scan counts the opening bracket itself, so start at 0.
                let depth = container_depth(line, open, if open == b'{' { b'}' } else { b']' }, 0);
                container = depth.map(|remaining| (open, remaining));
            }
            out.push(ScopedEntry {
                section: section.clone(),
                section_path: section_path.clone(),
                item: KeyValue {
                    parts,
                    line: index,
                    value,
                },
            });
        }
        out
    }
}

fn new_decl(name: String, section: String, line: usize) -> DepDecl {
    DepDecl {
        name,
        section,
        line,
        version: None,
        path: None,
        uses_workspace: false,
        issue: None,
    }
}

struct ScopedEntry {
    section: String,
    section_path: Vec<String>,
    item: KeyValue,
}

/// Issue text used when a dependency table simply has no `version` key.
pub const ISSUE_NO_VERSION: &str = "no `version` key";

fn collect_fields(decl: &mut DepDecl, fields: &[KeyValue], closed: bool) {
    for field in fields {
        match field.key().as_str() {
            "version" => match &field.value {
                Value::Str { text, inner } => {
                    decl.version = Some(Found {
                        text: text.clone(),
                        line: field.line,
                        inner: *inner,
                        section: decl.section.clone(),
                    });
                }
                Value::Unsupported(reason) => decl.issue = Some(reason.clone()),
                _ => decl.issue = Some("`version` is not a string".to_string()),
            },
            "path" => {
                let text = match &field.value {
                    Value::Str { text, .. } => Some(text.as_str()),
                    // Bare paths like `path = /usr/local/lib` are legal TOML;
                    // accepting them keeps an unquoted path from being dropped
                    // silently.
                    Value::Bare { text, .. } => Some(text.as_str()),
                    _ => None,
                };
                if let Some(text) = text {
                    decl.path = Some(text.to_string());
                }
            }
            "workspace" if field.value.as_bool() == Some(true) => decl.uses_workspace = true,
            _ => {}
        }
    }
    if decl.version.is_none() && decl.issue.is_none() {
        decl.issue = Some(if closed {
            ISSUE_NO_VERSION.to_string()
        } else {
            "the dependency table spans multiple lines".to_string()
        });
    }
}

/// Index of the dependency-section part in a section path, if any.
fn dep_section_index(path: &[String]) -> Option<usize> {
    path.iter().rposition(|part| {
        matches!(
            part.as_str(),
            "dependencies" | "dev-dependencies" | "build-dependencies"
        )
    })
}

fn found_string(entry: &KeyValue, expected: &[&str], header: &str) -> Option<Found> {
    if entry.parts.len() != expected.len() {
        return None;
    }
    if !entry.parts.iter().zip(expected).all(|(a, b)| a == b) {
        return None;
    }
    match &entry.value {
        Value::Str { text, inner } => Some(Found {
            text: text.clone(),
            line: entry.line,
            inner: *inner,
            section: header.to_string(),
        }),
        _ => None,
    }
}

fn unclosed_container(value: &Value) -> Option<u8> {
    match value {
        Value::Table { closed: false, .. } => Some(b'{'),
        Value::Unsupported(reason) if reason.starts_with(UNCLOSED_ARRAY) => Some(b'['),
        _ => None,
    }
}

const UNCLOSED_ARRAY: &str = "the value spans multiple lines";

fn split_lines(text: &str) -> (Vec<String>, String, bool) {
    if text.is_empty() {
        return (Vec::new(), "\n".to_string(), false);
    }
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let trailing_newline = text.ends_with('\n');
    let mut lines: Vec<String> = text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
        .collect();
    if trailing_newline {
        lines.pop();
    }
    (lines, eol.to_string(), trailing_newline)
}

/// Parse `[a.b.'c d']` into its header text and unquoted parts.
fn parse_header(line: &str) -> Result<(String, Vec<String>), String> {
    let end = line
        .find(']')
        .ok_or_else(|| "unterminated table header".to_string())?;
    let header = line[..=end].to_string();
    let inner = &line[1..end];
    let mut parts = Vec::new();
    let mut scanner = Scanner::new(inner, 0, 0);
    loop {
        scanner.skip_space();
        let Some(key) = scanner.key() else { break };
        parts.extend(key);
        scanner.skip_space();
        if scanner.peek() == Some(b'.') {
            scanner.advance(1);
            continue;
        }
        break;
    }
    if parts.is_empty() {
        return Err("empty table header".to_string());
    }
    Ok((header, parts))
}

/// Remaining container depth after scanning one line, or `None` when the line
/// cannot be read (an unterminated string, for instance).
fn container_depth(line: &str, open: u8, close: u8, mut depth: usize) -> Option<usize> {
    let mut scanner = Scanner::new(line, 0, 0);
    while let Some(byte) = scanner.peek() {
        match byte {
            b'"' | b'\'' => {
                scanner.string()?;
            }
            // A comment runs to the end of the line and holds no brackets.
            b'#' => return Some(depth),
            b if b == open => {
                depth += 1;
                scanner.advance(1);
            }
            b if b == close => {
                depth -= 1;
                scanner.advance(1);
                if depth == 0 {
                    return Some(0);
                }
            }
            _ => scanner.advance(1),
        }
    }
    Some(depth)
}

/// A tiny scanner over one line: strings, keys, values.
struct Scanner<'a> {
    text: &'a str,
    pos: usize,
    line: usize,
}

impl<'a> Scanner<'a> {
    fn new(text: &'a str, pos: usize, line: usize) -> Self {
        Scanner { text, pos, line }
    }

    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.pos).copied()
    }

    fn advance(&mut self, n: usize) {
        self.pos = (self.pos + n).min(self.text.len());
    }

    fn skip_space(&mut self) {
        while matches!(self.peek(), Some(b' ') | Some(b'\t')) {
            self.advance(1);
        }
    }

    /// A (possibly dotted, possibly quoted) key. Returns its unquoted parts.
    fn key(&mut self) -> Option<Vec<String>> {
        let mut parts = Vec::new();
        loop {
            self.skip_space();
            let start = self.pos;
            match self.peek()? {
                b'"' | b'\'' => {
                    let (text, _) = self.string()?;
                    parts.push(text);
                }
                b if is_bare_key_byte(b) => {
                    while matches!(self.peek(), Some(b) if is_bare_key_byte(b)) {
                        self.advance(1);
                    }
                    parts.push(self.text[start..self.pos].to_string());
                }
                _ => return None,
            }
            self.skip_space();
            if self.peek() == Some(b'.') {
                self.advance(1);
                continue;
            }
            return Some(parts);
        }
    }

    /// A value. Containers are only complete when they close on this line.
    fn value(&mut self) -> Value {
        self.skip_space();
        match self.peek() {
            Some(b'"') | Some(b'\'') => match self.string() {
                Some((text, inner)) => Value::Str { text, inner },
                None => Value::Unsupported("unterminated string".to_string()),
            },
            Some(b'{') => self.table(),
            Some(b'[') => self.array(),
            Some(b'#') | None => Value::Unsupported("no value".to_string()),
            Some(_) => {
                let start = self.pos;
                while let Some(byte) = self.peek() {
                    // A bare value ends at the end of the line, at a comment, or
                    // at whatever closes the inline table it sits in.
                    if matches!(byte, b'#' | b',' | b'}' | b']') {
                        break;
                    }
                    self.advance(1);
                }
                let raw = self.text[start..self.pos].trim_end();
                let bare_end = start + raw.len();
                match raw {
                    "true" => Value::Bool(true),
                    "false" => Value::Bool(false),
                    "" => Value::Unsupported("no value".to_string()),
                    other => match other.parse::<i64>() {
                        Ok(number) => Value::Integer(number),
                        Err(_) => Value::Bare {
                            text: other.to_string(),
                            inner: (start, bare_end),
                        },
                    },
                }
            }
        }
    }

    /// Read a quoted string, returning its decoded text and the byte range of
    /// its contents (quotes excluded).
    fn string(&mut self) -> Option<(String, (usize, usize))> {
        let quote = self.peek()?;
        if quote != b'"' && quote != b'\'' {
            return None;
        }
        self.advance(1);
        let start = self.pos;
        let bytes = self.text.as_bytes();
        let mut index = start;
        let mut escaped = false;
        while index < bytes.len() {
            let byte = bytes[index];
            if quote == b'"' && escaped {
                escaped = false;
                index += 1;
                continue;
            }
            if quote == b'"' && byte == b'\\' {
                escaped = true;
                index += 1;
                continue;
            }
            if byte == quote {
                let raw = &self.text[start..index];
                let text = if quote == b'"' {
                    decode_basic(raw)
                } else {
                    raw.to_string()
                };
                self.pos = index + 1;
                return Some((text, (start, index)));
            }
            index += 1;
        }
        None
    }

    fn table(&mut self) -> Value {
        self.advance(1);
        let mut entries = Vec::new();
        loop {
            self.skip_space();
            match self.peek() {
                None => {
                    return Value::Table {
                        entries,
                        closed: false,
                    }
                }
                Some(b'}') => {
                    self.advance(1);
                    return Value::Table {
                        entries,
                        closed: true,
                    };
                }
                Some(b'#') => {
                    return Value::Table {
                        entries,
                        closed: false,
                    }
                }
                Some(_) => {}
            }
            let Some(parts) = self.key() else {
                return Value::Unsupported(format!("unreadable key at byte {}", self.pos));
            };
            self.skip_space();
            if self.peek() != Some(b'=') {
                return Value::Unsupported(format!("expected `=` at byte {}", self.pos));
            }
            self.advance(1);
            let value = self.value();
            entries.push(KeyValue {
                parts,
                line: self.line,
                value,
            });
            self.skip_space();
            if self.peek() == Some(b',') {
                self.advance(1);
            }
        }
    }

    fn array(&mut self) -> Value {
        self.advance(1);
        let mut depth = 1usize;
        while let Some(byte) = self.peek() {
            match byte {
                b'"' | b'\'' => {
                    if self.string().is_none() {
                        break;
                    }
                }
                b'[' => {
                    depth += 1;
                    self.advance(1);
                }
                b']' => {
                    depth -= 1;
                    self.advance(1);
                    if depth == 0 {
                        return Value::Array;
                    }
                }
                b'#' => break,
                _ => self.advance(1),
            }
        }
        Value::Unsupported(format!("{UNCLOSED_ARRAY} (array)"))
    }
}

fn is_bare_key_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
}

/// Decode TOML basic-string escapes; unknown escapes are kept verbatim rather
/// than failing, because this text is only ever compared.
fn decode_basic(raw: &str) -> String {
    if !raw.contains('\\') {
        return raw.to_string();
    }
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('b') => out.push('\u{8}'),
            Some('f') => out.push('\u{c}'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Resolve a `path = "..."` value against the manifest's directory.
pub fn resolve_path(manifest: &Path, relative: &str) -> PathBuf {
    let base = manifest.parent().unwrap_or_else(|| Path::new("."));
    normalize(&base.join(relative))
}

/// Lexical path normalisation (no filesystem access, so it works for paths that
/// do not exist).
///
/// The result is rebuilt from components, which makes `.`/`..` disappear and
/// gives every caller the same spelling of the same path — comparisons here are
/// string comparisons, so that matters.
pub fn normalize(path: &Path) -> PathBuf {
    use std::path::Component;

    // The prefix and the root are kept verbatim: on Windows `C:` must not be
    // dropped, and pushing a separator onto it would replace it.
    let mut head = std::ffi::OsString::new();
    let mut parts: Vec<std::ffi::OsString> = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => head.push(prefix.as_os_str()),
            Component::RootDir => head.push(std::path::MAIN_SEPARATOR_STR),
            Component::CurDir => {}
            Component::ParentDir => {
                if parts.pop().is_none() && head.is_empty() {
                    parts.push("..".into());
                }
            }
            Component::Normal(part) => parts.push(part.to_os_string()),
        }
    }
    let mut out = head;
    for part in parts {
        let text = out.to_string_lossy();
        if !text.is_empty() && !text.ends_with(['/', '\\']) {
            out.push(std::path::MAIN_SEPARATOR_STR);
        }
        out.push(part);
    }
    if out.is_empty() {
        out.push(".");
    }
    PathBuf::from(out)
}

/// Canonical form used to compare two paths for "same directory".
pub fn same_dir(a: &Path, b: &Path) -> bool {
    let a = a.canonicalize().unwrap_or_else(|_| normalize(a));
    let b = b.canonicalize().unwrap_or_else(|_| normalize(b));
    if cfg!(windows) {
        let a = a.to_string_lossy().replace('/', "\\");
        let b = b.to_string_lossy().replace('/', "\\");
        a.eq_ignore_ascii_case(&b)
    } else {
        a == b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(text: &str) -> Manifest {
        Manifest::from_text(PathBuf::from("Cargo.toml"), text.to_string())
    }

    const ROOT: &str = r#"# a workspace
[workspace]
members = ["crates/a", "crates/b"]
resolver = "2"

[workspace.package]
version = "0.0.2"     # the single source of truth
edition = "2021"

[workspace.dependencies]
war3-core = { path = "crates/war3-core", version = "0.0.2" }
war3-map = { version = "0.0.2", path = "crates/war3-map" }
serde = { version = "1", features = ["derive"] }

[profile.release]
lto = true
"#;

    fn edit_from(found: &Found, new: &str) -> Edit {
        Edit {
            line: found.line,
            inner: found.inner,
            old: found.text.clone(),
            new: new.to_string(),
            label: found.section.clone(),
        }
    }

    #[test]
    fn finds_the_workspace_version() {
        let found = manifest(ROOT).workspace_package_version().unwrap();
        assert_eq!(found.text, "0.0.2");
        assert_eq!(found.line, 6);
        assert_eq!(found.section, "[workspace.package]");
        let line = ROOT.lines().nth(6).unwrap();
        assert_eq!(&line[found.inner.0..found.inner.1], "0.0.2");
    }

    #[test]
    fn finds_dependency_declarations() {
        let decls = manifest(ROOT).dependency_decls();
        let names: Vec<&str> = decls.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["war3-core", "war3-map", "serde"]);
        assert_eq!(decls[0].path.as_deref(), Some("crates/war3-core"));
        assert_eq!(decls[0].version.as_ref().unwrap().text, "0.0.2");
        assert_eq!(decls[0].line, 10);
        assert_eq!(decls[1].path.as_deref(), Some("crates/war3-map"));
        assert_eq!(decls[2].path, None);
        assert!(decls.iter().all(|d| d.issue.is_none()), "{decls:?}");
    }

    #[test]
    fn applies_an_edit_and_keeps_everything_else() {
        let doc = manifest(ROOT);
        let found = doc.workspace_package_version().unwrap();
        let updated = doc.render_with(&[edit_from(&found, "0.0.3")]).unwrap();
        assert!(updated.contains(r#"version = "0.0.3"     # the single source of truth"#));
        assert!(updated.contains(r#"war3-core = { path = "crates/war3-core", version = "0.0.2" }"#));
        assert_eq!(updated.lines().count(), ROOT.lines().count());
    }

    #[test]
    fn several_edits_are_applied_consistently() {
        let doc = manifest(ROOT);
        let edits: Vec<Edit> = doc
            .dependency_decls()
            .iter()
            .filter_map(|d| d.version.as_ref())
            .map(|v| edit_from(v, "9.9.9"))
            .collect();
        // every declaration with a version is rewritten here, including `serde`:
        // the plan is what decides to leave non-member dependencies alone.
        assert_eq!(edits.len(), 3);
        let updated = doc.render_with(&edits).unwrap();
        assert_eq!(updated.matches("\"9.9.9\"").count(), 3);
        assert!(updated.contains("serde = { version = \"9.9.9\""));
    }

    #[test]
    fn a_stale_edit_is_refused() {
        let doc = manifest(ROOT);
        let edit = Edit {
            line: 6,
            inner: (11, 16),
            old: "1.1.1".to_string(),
            new: "0.0.3".to_string(),
            label: "[workspace.package]".to_string(),
        };
        assert!(doc
            .render_with(&[edit])
            .unwrap_err()
            .contains("expected `1.1.1`"));
    }

    #[test]
    fn handles_version_workspace_and_rust_version() {
        let text = r#"[package]
name = "a"
version.workspace = true
rust-version = "1.74"
edition = "2021"

[dependencies]
b = { path = "../b", version = "0.0.2" }
"#;
        let doc = manifest(text);
        assert!(doc.package_version().is_none());
        let decls = doc.dependency_decls();
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].path.as_deref(), Some("../b"));

        let edits: Vec<Edit> = decls
            .iter()
            .filter_map(|d| d.version.as_ref())
            .map(|v| edit_from(v, "0.0.3"))
            .collect();
        let updated = doc.render_with(&edits).unwrap();
        assert!(updated.contains(r#"b = { path = "../b", version = "0.0.3" }"#));
        // `rust-version` must survive untouched
        assert!(updated.contains(r#"rust-version = "1.74""#));
        assert!(updated.contains("version.workspace = true"));
    }

    #[test]
    fn reads_the_dotted_table_form() {
        let text = r#"[package]
name = "a"
version = "0.0.2"

[dependencies.foo]
path = "../foo"
version = "0.0.2"
features = ["x"]

[dev-dependencies.bar]
version = "0.0.1"
"#;
        let decls = manifest(text).dependency_decls();
        assert_eq!(decls.len(), 2);
        assert_eq!(decls[0].name, "foo");
        assert_eq!(decls[0].path.as_deref(), Some("../foo"));
        assert_eq!(decls[0].version.as_ref().unwrap().line, 6);
        assert_eq!(decls[1].name, "bar");
        assert_eq!(decls[1].section, "[dev-dependencies.bar]");
    }

    #[test]
    fn reads_target_dependencies_and_workspace_table_form() {
        let text = r#"[target.'cfg(unix)'.dependencies]
libc = { version = "0.2", path = "../libc" }

[workspace.dependencies.war3-core]
path = "crates/war3-core"
version = "0.0.2"
"#;
        let decls = manifest(text).dependency_decls();
        assert_eq!(decls.len(), 2);
        assert_eq!(decls[0].name, "libc");
        assert_eq!(decls[0].version.as_ref().unwrap().text, "0.2");
        assert_eq!(decls[1].name, "war3-core");
        assert_eq!(decls[1].version.as_ref().unwrap().text, "0.0.2");
    }

    #[test]
    fn reports_multi_line_values_instead_of_guessing() {
        let text = r#"[package]
name = "a"
version = "0.0.2"

[dependencies]
foo = {
    path = "../foo",
    version = "0.0.2",
}
bar = { path = "../bar" }
"#;
        let decls = manifest(text).dependency_decls();
        assert_eq!(decls.len(), 2, "{decls:?}");
        assert_eq!(decls[0].name, "foo");
        assert!(decls[0].version.is_none());
        assert!(
            decls[0]
                .issue
                .as_deref()
                .unwrap()
                .contains("multiple lines"),
            "{:?}",
            decls[0]
        );
        assert_eq!(
            decls[1].name, "bar",
            "the continuation lines must not become entries"
        );
        assert_eq!(decls[1].path.as_deref(), Some("../bar"));
        assert_eq!(decls[1].issue.as_deref(), Some("no `version` key"));
    }

    #[test]
    fn reads_a_version_that_the_inline_table_keeps_on_the_first_line() {
        let text = "[dependencies]\nfoo = { path = \"../foo\",\n    features = [\"x\"] }\n";
        let decls = manifest(text).dependency_decls();
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].path.as_deref(), Some("../foo"));
        assert!(decls[0].version.is_none());
        assert!(decls[0]
            .issue
            .as_deref()
            .unwrap()
            .contains("multiple lines"));
    }

    #[test]
    fn reports_a_version_on_a_continuation_line() {
        let text = "[dependencies]\nfoo = {\n    path = \"../foo\",\n    version = \"0.0.2\",\n}\n";
        let decls = manifest(text).dependency_decls();
        assert_eq!(decls.len(), 1);
        assert!(decls[0].version.is_none());
        // both the path and the version are on later lines, so neither is
        // claimed; the declaration is reported instead
        assert_eq!(decls[0].path, None);
        assert!(
            decls[0]
                .issue
                .as_deref()
                .unwrap()
                .contains("multiple lines"),
            "{:?}",
            decls[0]
        );
    }

    #[test]
    fn reads_workspace_inheritance_inline() {
        let text = "[package]\nname = \"a\"\nversion.workspace = true\n\n[dependencies]\nb = { workspace = true }\nc = { workspace = true, features = [\"x\"] }\n";
        let decls = manifest(text).dependency_decls();
        assert_eq!(decls.len(), 2, "{decls:?}");
        assert!(decls[0].uses_workspace, "{decls:?}");
        assert!(decls[1].uses_workspace, "{decls:?}");
        assert!(decls.iter().all(|decl| decl.version.is_none()));
    }

    #[test]
    fn bare_values_stop_at_the_inline_table_brace() {
        let text = "[package]\nname = \"a\"\nversion = \"0.0.2\"\n\n[dependencies]\nb = { optional = true, path = \"../b\" }\n";
        let decls = manifest(text).dependency_decls();
        assert_eq!(decls[0].path.as_deref(), Some("../b"), "{decls:?}");
        assert_eq!(decls[0].issue.as_deref(), Some(ISSUE_NO_VERSION));
    }

    #[test]
    fn shorthand_string_dependencies_are_understood() {
        let text = "[dependencies]\nfoo = \"1.2.3\"  # plain\n";
        let decls = manifest(text).dependency_decls();
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].version.as_ref().unwrap().text, "1.2.3");
        assert_eq!(decls[0].path, None);
    }

    #[test]
    fn unquoted_paths_are_recognised_as_paths() {
        // Bare paths are legal TOML; without this, `path = /usr/local/lib`
        // would be silently dropped from path-dependency discovery.
        let text = "[dependencies]\nfoo = { path = ../foo, version = \"0.0.2\" }\n";
        let decls = manifest(text).dependency_decls();
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].path.as_deref(), Some("../foo"), "{decls:?}");
        assert_eq!(decls[0].version.as_ref().unwrap().text, "0.0.2");
    }

    #[test]
    fn multi_line_arrays_are_skipped_not_parsed_as_entries() {
        let text = "[workspace]\nmembers = [\n    \"crates/a\",\n    \"crates/b\",\n]\n\n[workspace.package]\nversion = \"0.0.2\"\n";
        let doc = manifest(text);
        assert_eq!(doc.workspace_package_version().unwrap().text, "0.0.2");
        assert!(doc.dependency_decls().is_empty());
    }

    #[test]
    fn ignores_commented_out_and_other_tables() {
        let text = r#"[package]
name = "a"
version = "0.0.2"

[package.metadata.docs.rs]
all-features = true
# version = "9.9.9"
"#;
        let doc = manifest(text);
        assert_eq!(doc.package_version().unwrap().text, "0.0.2");
        assert!(doc.dependency_decls().is_empty());
    }

    #[test]
    fn preserves_crlf_and_missing_final_newline() {
        let text = "[package]\r\nname = \"a\"\r\nversion = \"0.0.2\"";
        let doc = manifest(text);
        let found = doc.package_version().unwrap();
        assert_eq!(found.line, 2);
        let updated = doc.render_with(&[edit_from(&found, "0.0.3")]).unwrap();
        assert_eq!(updated, "[package]\r\nname = \"a\"\r\nversion = \"0.0.3\"");
    }

    #[test]
    fn quoted_values_with_escapes() {
        let text = "[package]\nname = \"a\"\nversion = \"0.0.2\"\ndescription = \"say \\\"hi\\\" #notacomment\"\n";
        let doc = manifest(text);
        assert_eq!(doc.package_version().unwrap().text, "0.0.2");
        assert_eq!(doc.line_count(), 4);
    }

    #[test]
    fn literal_strings_are_read() {
        let text = "[workspace.package]\nversion = '0.0.2'\n";
        let found = manifest(text).workspace_package_version().unwrap();
        assert_eq!(found.text, "0.0.2");
        assert_eq!(
            &text.lines().nth(1).unwrap()[found.inner.0..found.inner.1],
            "0.0.2"
        );
    }

    #[test]
    fn path_helpers() {
        assert_eq!(
            resolve_path(Path::new("/w/crates/a/Cargo.toml"), "../b"),
            PathBuf::from("/w/crates/b")
        );
        assert_eq!(
            normalize(Path::new("/w/crates/./a/../b")),
            PathBuf::from("/w/crates/b")
        );
        assert!(same_dir(
            Path::new("/w/crates/a"),
            Path::new("/w/crates/./a/")
        ));
    }
}
