//! The history the version logic reads: the most recent reachable tag, and the
//! commits of a revision range.

use crate::error::Result;

use super::types::{Commit, Git};

impl Git {
    /// The most recent tag reachable from HEAD, if there is one.
    pub fn last_tag(&self) -> Result<Option<String>> {
        match self.run(&["describe", "--tags", "--abbrev=0"]) {
            Ok(output) if output.ok() => {
                let tag = output.stdout.trim().to_string();
                Ok((!tag.is_empty()).then_some(tag))
            }
            Ok(_) => Ok(None),
            Err(err) => Err(err),
        }
    }

    /// Commits in `range` (e.g. `v1.0.0..HEAD` or `HEAD`), newest first.
    ///
    /// `limit` caps how many are read; 0 means no cap.
    pub fn commits(&self, range: &str, limit: usize) -> Result<Vec<Commit>> {
        let mut args: Vec<String> = vec!["log".to_string()];
        if limit > 0 {
            args.push(format!("-{limit}"));
        }
        args.push("--format=%H%x1f%s%x1f%b%x1e".to_string());
        args.push(range.to_string());
        let refs: Vec<&str> = args.iter().map(|arg| arg.as_str()).collect();
        let text = self.checked(&refs)?;

        let mut commits = Vec::new();
        for record in text.split('\u{1e}') {
            let record = record.trim_matches('\n');
            if record.is_empty() {
                continue;
            }
            let mut fields = record.splitn(3, '\u{1f}');
            let id = fields.next().unwrap_or_default().trim().to_string();
            let subject = fields.next().unwrap_or_default().to_string();
            let body = fields.next().unwrap_or_default().to_string();
            commits.push(Commit { id, subject, body });
        }
        Ok(commits)
    }
}
