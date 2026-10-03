//! A small JSON reader, only used to read `cargo metadata` output.
//!
//! Implementing it here keeps the crate dependency-free; the parser is strict
//! (RFC 8259), depth-limited, and has no opinion about the schema.

use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    pub fn parse(input: &str) -> Result<Json, JsonError> {
        let mut parser = Parser {
            bytes: input.as_bytes(),
            pos: 0,
            depth: 0,
        };
        parser.skip_ws();
        let value = parser.value()?;
        parser.skip_ws();
        if parser.pos != parser.bytes.len() {
            return Err(parser.error("trailing data after the JSON document"));
        }
        Ok(value)
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Convenience for `obj["key"].as_str()`.
    pub fn str_at(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(|v| v.as_str())
    }

    /// Build a one-shot lookup map for repeated `get` calls on the same object.
    ///
    /// Each [`get`](Self::get) call linearly scans the object's entries, which
    /// is fine for a single key but wasteful when callers do several lookups
    /// (the workspace loader does four per package). The returned map is owned
    /// by the caller; nothing is cached on the `Json` itself.
    pub fn index(&self) -> Option<HashMap<&str, &Json>> {
        match self {
            Json::Object(entries) => Some(entries.iter().map(|(k, v)| (k.as_str(), v)).collect()),
            _ => None,
        }
    }
}

/// Parse error with the byte offset where it was detected.
#[derive(Debug, Clone)]
pub struct JsonError {
    pub message: String,
    pub offset: usize,
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (at byte {})", self.message, self.offset)
    }
}

impl std::error::Error for JsonError {}

const MAX_DEPTH: usize = 128;

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    depth: usize,
}

impl<'a> Parser<'a> {
    fn error(&self, message: impl Into<String>) -> JsonError {
        JsonError {
            message: message.into(),
            offset: self.pos,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            match b {
                b' ' | b'\t' | b'\n' | b'\r' => self.pos += 1,
                _ => break,
            }
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), JsonError> {
        if self.peek() == Some(byte) {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.error(format!("expected `{}`", byte as char)))
        }
    }

    fn value(&mut self) -> Result<Json, JsonError> {
        if self.depth > MAX_DEPTH {
            return Err(self.error("JSON nesting is too deep"));
        }
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => Ok(Json::String(self.string()?)),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'n') => self.literal("null", Json::Null),
            Some(b) if b == b'-' || b.is_ascii_digit() => self.number(),
            Some(b) => Err(self.error(format!("unexpected byte `{}`", b as char))),
            None => Err(self.error("unexpected end of input")),
        }
    }

    fn literal(&mut self, text: &str, value: Json) -> Result<Json, JsonError> {
        if self.bytes[self.pos..].starts_with(text.as_bytes()) {
            self.pos += text.len();
            Ok(value)
        } else {
            Err(self.error(format!("expected `{text}`")))
        }
    }

    fn object(&mut self) -> Result<Json, JsonError> {
        self.expect(b'{')?;
        self.depth += 1;
        let mut entries = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            self.depth -= 1;
            return Ok(Json::Object(entries));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            self.expect(b':')?;
            self.skip_ws();
            let value = self.value()?;
            entries.push((key, value));
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                }
                Some(b'}') => {
                    self.pos += 1;
                    break;
                }
                Some(b) => {
                    return Err(self.error(format!("expected `,` or `}}`, found `{}`", b as char)))
                }
                None => return Err(self.error("unexpected end of input in object")),
            }
        }
        self.depth -= 1;
        Ok(Json::Object(entries))
    }

    fn array(&mut self) -> Result<Json, JsonError> {
        self.expect(b'[')?;
        self.depth += 1;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            self.depth -= 1;
            return Ok(Json::Array(items));
        }
        loop {
            self.skip_ws();
            items.push(self.value()?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                }
                Some(b']') => {
                    self.pos += 1;
                    break;
                }
                Some(b) => {
                    return Err(self.error(format!("expected `,` or `]`, found `{}`", b as char)))
                }
                None => return Err(self.error("unexpected end of input in array")),
            }
        }
        self.depth -= 1;
        Ok(Json::Array(items))
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.expect(b'"')?;
        let mut out = String::new();
        let mut chunk_start = self.pos;
        loop {
            let byte = match self.peek() {
                Some(b) => b,
                None => return Err(self.error("unterminated string")),
            };
            match byte {
                b'"' => {
                    out.push_str(self.utf8(chunk_start, self.pos)?);
                    self.pos += 1;
                    return Ok(out);
                }
                b'\\' => {
                    out.push_str(self.utf8(chunk_start, self.pos)?);
                    self.pos += 1;
                    let escape = self
                        .peek()
                        .ok_or_else(|| self.error("unterminated escape"))?;
                    self.pos += 1;
                    match escape {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => out.push(self.unicode_escape()?),
                        other => {
                            return Err(self.error(format!("invalid escape `\\{}`", other as char)))
                        }
                    }
                    chunk_start = self.pos;
                }
                b if b < 0x20 => return Err(self.error("control character in string")),
                _ => self.pos += 1,
            }
        }
    }

    fn unicode_escape(&mut self) -> Result<char, JsonError> {
        let first = self.hex4()?;
        if (0xD800..0xDC00).contains(&first) {
            // high surrogate: must be followed by a low surrogate
            if self.peek() == Some(b'\\') && self.bytes.get(self.pos + 1) == Some(&b'u') {
                self.pos += 2;
                let second = self.hex4()?;
                if (0xDC00..0xE000).contains(&second) {
                    let code = 0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00);
                    return char::from_u32(code)
                        .ok_or_else(|| self.error("invalid surrogate pair in string"));
                }
            }
            return Err(self.error("unpaired surrogate in string"));
        }
        char::from_u32(first).ok_or_else(|| self.error("invalid \\u escape"))
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        if self.pos + 4 > self.bytes.len() {
            return Err(self.error("truncated \\u escape"));
        }
        let text = std::str::from_utf8(&self.bytes[self.pos..self.pos + 4])
            .map_err(|_| self.error("invalid \\u escape"))?;
        let value = u32::from_str_radix(text, 16).map_err(|_| self.error("invalid \\u escape"))?;
        self.pos += 4;
        Ok(value)
    }

    fn utf8(&self, start: usize, end: usize) -> Result<&'a str, JsonError> {
        std::str::from_utf8(&self.bytes[start..end]).map_err(|_| JsonError {
            message: "invalid UTF-8 in string".to_string(),
            offset: start,
        })
    }

    fn number(&mut self) -> Result<Json, JsonError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        let digits_start = self.pos;
        while matches!(self.peek(), Some(b) if b.is_ascii_digit()) {
            self.pos += 1;
        }
        if self.pos == digits_start {
            return Err(self.error("expected a digit"));
        }
        // JSON forbids leading zeros: `01` is not a number.
        if self.pos - digits_start > 1 && self.bytes[digits_start] == b'0' {
            return Err(self.error("numbers must not have leading zeros"));
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            let frac_start = self.pos;
            while matches!(self.peek(), Some(b) if b.is_ascii_digit()) {
                self.pos += 1;
            }
            if self.pos == frac_start {
                return Err(self.error("expected a digit after `.`"));
            }
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.pos += 1;
            }
            let exp_start = self.pos;
            while matches!(self.peek(), Some(b) if b.is_ascii_digit()) {
                self.pos += 1;
            }
            if self.pos == exp_start {
                return Err(self.error("expected a digit in the exponent"));
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| self.error("invalid number"))?;
        text.parse::<f64>()
            .map(Json::Number)
            .map_err(|_| self.error("invalid number"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_metadata_like_document() {
        let text = r#"{"packages":[{"name":"a","version":"0.1.0","dependencies":[{"name":"b","req":"^0.1.0","path":"C:\\x"}]}],
            "workspace_members":["path+file:///x#0.1.0"],"workspace_root":"C:/x","resolve":null}"#;
        let json = Json::parse(text).unwrap();
        assert_eq!(json.str_at("workspace_root"), Some("C:/x"));
        let packages = json.get("packages").unwrap().as_array().unwrap();
        assert_eq!(packages[0].str_at("name"), Some("a"));
        let dep = &packages[0].get("dependencies").unwrap().as_array().unwrap()[0];
        assert_eq!(dep.str_at("req"), Some("^0.1.0"));
        assert_eq!(dep.str_at("path"), Some("C:\\x"));
        assert_eq!(json.get("resolve").unwrap(), &Json::Null);
        assert_eq!(
            json.get("workspace_members")
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn handles_escapes_and_unicode() {
        let json = Json::parse(r#"{"s":"a\u00e9b\u4e2d\ud83d\ude00\"\\\/\n\t"}"#).unwrap();
        assert_eq!(json.str_at("s"), Some("aéb中😀\"\\/\n\t"));
    }

    #[test]
    fn rejects_bad_input() {
        assert!(Json::parse("").is_err());
        assert!(Json::parse("{").is_err());
        assert!(Json::parse(r#"{"a":1,}"#).is_err());
        assert!(Json::parse(r#"{"a" 1}"#).is_err());
        assert!(Json::parse(r#"{"a":"b"}"#).is_ok());
        assert!(Json::parse(r#"{"a":"b"} trailing"#).is_err());
        assert!(Json::parse(r#"{"a":01}"#).is_err());
        assert!(Json::parse(r#"{"a":"\ud800"}"#).is_err());
    }

    #[test]
    fn parses_numbers() {
        let json = Json::parse("[0,-1,1.5,1e3,-2.5E-2]").unwrap();
        assert_eq!(
            json.as_array().unwrap(),
            &[
                Json::Number(0.0),
                Json::Number(-1.0),
                Json::Number(1.5),
                Json::Number(1000.0),
                Json::Number(-0.025)
            ]
        );
    }

    #[test]
    fn rejects_deep_nesting() {
        let text = "[".repeat(200) + &"]".repeat(200);
        assert!(Json::parse(&text).is_err());
    }
}
