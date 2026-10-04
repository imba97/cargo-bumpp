//! Rendering for the messages the tool prints: a command line as the user would
//! type it, and a path relative to the repository.

use std::path::Path;

/// Render a command line for a message, quoting what needs it.
pub fn display_command(args: &[&str]) -> String {
    args.iter()
        .map(|arg| {
            if arg.is_empty() || arg.contains([' ', '\t', '\n', '"', '\'']) {
                format!("\"{}\"", arg.replace('"', "\\\""))
            } else {
                arg.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The path shown to the user: relative to the repository when possible.
pub fn display_path(repo_root: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(repo_root).unwrap_or(path);
    relative.to_string_lossy().replace('\\', "/")
}
