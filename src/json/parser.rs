//! The recursive-descent parser: strict RFC 8259, depth-limited, and only
//! reachable through `Json::parse`.

use super::error::JsonError;
use super::value::Json;

const MAX_DEPTH: usize = 128;

pub(super) struct Parser<'a> {
    pub(super) bytes: &'a [u8],
    pub(super) pos: usize,
    pub(super) depth: usize,
}

impl<'a> Parser<'a> {
    pub(super) fn error(&self, message: impl Into<String>) -> JsonError {
        JsonError {
            message: message.into(),
            offset: self.pos,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    pub(super) fn skip_ws(&mut self) {
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

    pub(super) fn value(&mut self) -> Result<Json, JsonError> {
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
