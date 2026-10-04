//! The JSON value type and the accessors callers use to walk `cargo metadata`
//! output; the parsing entry point lives here too because it is an inherent
//! method.

use std::collections::HashMap;

use super::error::JsonError;
use super::parser::Parser;

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
