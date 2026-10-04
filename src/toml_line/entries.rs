//! Scanning manifest lines into section-scoped `key = value` entries. Each entry
//! remembers the header it appeared under, so later passes can ask about
//! `[package]` or `[dependencies.foo]` without re-reading the text.

use super::syntax::{container_depth, parse_header, unclosed_container, Scanner};
use super::types::{DepDecl, KeyValue};

/// Build the per-line entries list. Pure function so the cache wrapper above
/// can hand the result to `OnceCell::get_or_init` directly.
pub(super) fn scan_entries(lines: &[String]) -> Vec<ScopedEntry> {
    let mut out = Vec::new();
    let mut section = String::new();
    let mut section_path: Vec<String> = Vec::new();
    // A value that continues on later lines: (opening byte, depth).
    let mut container: Option<(u8, usize)> = None;

    for (index, line) in lines.iter().enumerate() {
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

pub(super) fn new_decl(name: String, section: String, line: usize) -> DepDecl {
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

#[derive(Debug, Clone)]
pub(super) struct ScopedEntry {
    pub(super) section: String,
    pub(super) section_path: Vec<String>,
    pub(super) item: KeyValue,
}
