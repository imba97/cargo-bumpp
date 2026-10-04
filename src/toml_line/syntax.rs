//! The text layer: splitting a file into lines, reading a table header, tracking a
//! container that continues on the next line, and scanning the keys and values of
//! one line.
//!
//! It is deliberately shallow — strings and inline tables only — and it reports
//! what it cannot read instead of guessing.

use super::types::{KeyValue, Value};

pub(super) fn unclosed_container(value: &Value) -> Option<u8> {
    match value {
        Value::Table { closed: false, .. } => Some(b'{'),
        Value::Unsupported(reason) if reason.starts_with(UNCLOSED_ARRAY) => Some(b'['),
        _ => None,
    }
}

const UNCLOSED_ARRAY: &str = "the value spans multiple lines";

pub(super) fn split_lines(text: &str) -> (Vec<String>, String, bool) {
    if text.is_empty() {
        return (Vec::new(), "\n".to_string(), false);
    }
    let crlf = text.contains("\r\n");
    let eol = if crlf { "\r\n" } else { "\n" };
    let trailing_newline = text.ends_with('\n');
    // LF files need no per-line work; CRLF files do, because each line still
    // carries a trailing `\r` that has to be stripped before the line is stored.
    let mut lines: Vec<String> = if crlf {
        text.split('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
            .collect()
    } else {
        text.split('\n').map(str::to_string).collect()
    };
    if trailing_newline {
        lines.pop();
    }
    (lines, eol.to_string(), trailing_newline)
}

/// Parse `[a.b.'c d']` into its header text and unquoted parts.
pub(super) fn parse_header(line: &str) -> Result<(String, Vec<String>), String> {
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
pub(super) fn container_depth(line: &str, open: u8, close: u8, mut depth: usize) -> Option<usize> {
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
pub(super) struct Scanner<'a> {
    text: &'a str,
    pos: usize,
    line: usize,
}

impl<'a> Scanner<'a> {
    pub(super) fn new(text: &'a str, pos: usize, line: usize) -> Self {
        Scanner { text, pos, line }
    }

    pub(super) fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.pos).copied()
    }

    pub(super) fn advance(&mut self, n: usize) {
        self.pos = (self.pos + n).min(self.text.len());
    }

    pub(super) fn skip_space(&mut self) {
        while matches!(self.peek(), Some(b' ') | Some(b'\t')) {
            self.advance(1);
        }
    }

    /// A (possibly dotted, possibly quoted) key. Returns its unquoted parts.
    pub(super) fn key(&mut self) -> Option<Vec<String>> {
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
    pub(super) fn value(&mut self) -> Value {
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
