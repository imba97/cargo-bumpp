//! Lexical path helpers: resolving a `path = "..."` dependency against the
//! manifest that declares it, and deciding whether two manifests live in the same
//! directory. Nothing here touches the filesystem unless the paths exist.

use std::path::{Path, PathBuf};

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
