//! Colors for unified diffs.

use crate::role::{Role, Span};

pub fn highlight_diff_line(line: &str) -> Vec<Span> {
    let end = line.trim_end_matches(['\r', '\n']).len();
    let role = if line.starts_with("+++")
        || line.starts_with("---")
        || line.starts_with("diff ")
        || line.starts_with("index ")
    {
        Role::Keyword
    } else if line.starts_with("@@") {
        Role::DiffHunk
    } else if line.starts_with('+') {
        Role::DiffAdded
    } else if line.starts_with('-') {
        Role::DiffRemoved
    } else {
        return Vec::new();
    };
    vec![Span {
        range: 0..end,
        role,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn role(line: &str) -> Role {
        highlight_diff_line(line)
            .first()
            .map_or(Role::Plain, |s| s.role)
    }

    #[test]
    fn diff_lines() {
        assert_eq!(role("+added"), Role::DiffAdded);
        assert_eq!(role("-removed"), Role::DiffRemoved);
        assert_eq!(role("@@ -1,2 +1,3 @@"), Role::DiffHunk);
        assert_eq!(role("+++ b/file"), Role::Keyword);
        assert_eq!(role("--- a/file"), Role::Keyword);
        assert_eq!(role(" context"), Role::Plain);
    }
}
