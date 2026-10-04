//! What git says about individual paths: whether it tracks them, and whether a
//! `.gitignore` rule covers them.

use std::path::Path;

use super::types::Git;

impl Git {
    /// True when the path exists but must not be committed: untracked *and*
    /// matched by a `.gitignore` rule. A tracked file is never skipped, even if
    /// a rule matches it.
    pub fn changed_but_ignored(&self, path: &Path) -> bool {
        if self.is_tracked(path) {
            return false;
        }
        let text = path.to_string_lossy().to_string();
        matches!(
            self.run(&["check-ignore", "--quiet", "--", &text]),
            Ok(output) if output.ok()
        )
    }

    /// True when git already tracks the path, i.e. a checkout can restore it.
    pub fn is_tracked(&self, path: &Path) -> bool {
        let text = path.to_string_lossy().to_string();
        matches!(
            self.run(&["ls-files", "--error-unmatch", "--", &text]),
            Ok(output) if output.ok()
        )
    }
}
