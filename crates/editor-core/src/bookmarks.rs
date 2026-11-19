//! Line bookmarks and the commands that act on bookmarked lines.

use crate::buffer::{BatchMap, Edit, EditBatch, EditError};
use crate::document::Document;
use std::collections::BTreeSet;
use std::ops::Range;

/// Where bookmarks land after `batch`. Must be called before the batch is applied.
pub(crate) fn remapped(doc: &Document, batch: &EditBatch) -> Vec<usize> {
    let map = BatchMap::new(batch);
    let mut result = Vec::with_capacity(doc.bookmarks.len());
    for &offset in &doc.bookmarks {
        match map.map(offset, true) {
            Some(mapped) => result.push(mapped),
            None => {
                let index = batch.0.partition_point(|e| e.range.end <= offset);
                if batch.0[index].range.end < doc.line_end(doc.line_col(offset).0) {
                    result.push(map.boundary(offset, false));
                }
            }
        }
    }
    result.sort_unstable();
    result.dedup();
    result
}

impl Document {
    /// Offset just after the line's ending.
    fn line_end(&self, line: usize) -> usize {
        let rope = self.buffer.rope();
        if line + 1 < rope.len_lines() {
            rope.line_to_char(line + 1)
        } else {
            rope.len_chars()
        }
    }

    fn line_count(&self) -> usize {
        self.buffer.rope().len_lines()
    }

    /// In a backup, only lines inside editable sections can be changed by bookmark commands.
    fn line_in_scope(&self, line: usize) -> bool {
        let buffer = &self.buffer;
        if buffer.zones().is_empty() && buffer.locked().is_empty() {
            return true;
        }
        let start = buffer.rope().line_to_char(line);
        let end = self.line_end(line);
        start < end
            && buffer
                .zones()
                .iter()
                .any(|z| z.start <= start && end <= z.end)
    }

    pub fn bookmarked_lines(&self) -> Vec<usize> {
        let lines: BTreeSet<usize> = self.bookmarks.iter().map(|&o| self.line_col(o).0).collect();
        lines.into_iter().collect()
    }

    pub fn set_bookmarked_lines(&mut self, lines: &[usize]) {
        let rope = self.buffer.rope();
        let mut offsets: Vec<usize> = lines
            .iter()
            .filter(|&&l| l < rope.len_lines())
            .map(|&l| rope.line_to_char(l))
            .collect();
        offsets.sort_unstable();
        offsets.dedup();
        self.bookmarks = offsets;
    }

    pub fn toggle_bookmark(&mut self, line: usize) {
        let mut lines = self.bookmarked_lines();
        match lines.binary_search(&line) {
            Ok(index) => {
                lines.remove(index);
            }
            Err(index) => lines.insert(index, line),
        }
        self.set_bookmarked_lines(&lines);
    }

    /// The next bookmarked line after `from_line`, wrapping to the top.
    pub fn next_bookmark(&self, from_line: usize) -> Option<usize> {
        let lines = self.bookmarked_lines();
        lines
            .iter()
            .copied()
            .find(|&l| l > from_line)
            .or(lines.first().copied())
    }

    /// The previous bookmarked line before `from_line`, wrapping to the bottom.
    pub fn previous_bookmark(&self, from_line: usize) -> Option<usize> {
        let lines = self.bookmarked_lines();
        lines
            .iter()
            .rev()
            .copied()
            .find(|&l| l < from_line)
            .or(lines.last().copied())
    }

    /// Bookmarks every line touched by `ranges`. Returns how many lines matched.
    pub fn bookmark_lines_of(&mut self, ranges: &[Range<usize>]) -> usize {
        let mut matched = BTreeSet::new();
        for range in ranges {
            let (first, _) = self.line_col(range.start);
            let (mut last, column) = self.line_col(range.end);
            if last > first && column == 0 {
                last -= 1;
            }
            matched.extend((first..=last).filter(|&l| self.line_in_scope(l)));
        }
        let mut lines: BTreeSet<usize> = self.bookmarked_lines().into_iter().collect();
        lines.extend(matched.iter().copied());
        self.set_bookmarked_lines(&lines.into_iter().collect::<Vec<_>>());
        matched.len()
    }

    pub fn clear_bookmarks(&mut self) {
        self.bookmarks.clear();
    }

    pub fn inverse_bookmarks(&mut self) {
        let current: BTreeSet<usize> = self.bookmarked_lines().into_iter().collect();
        let inverted: Vec<usize> = (0..self.line_count())
            .filter(|&l| {
                if self.line_in_scope(l) {
                    !current.contains(&l)
                } else {
                    current.contains(&l)
                }
            })
            .collect();
        self.set_bookmarked_lines(&inverted);
    }

    fn scoped_bookmarks(&self) -> Vec<usize> {
        self.bookmarked_lines()
            .into_iter()
            .filter(|&l| self.line_in_scope(l))
            .collect()
    }

    pub fn copy_bookmarked_lines(&self) -> String {
        let rope = self.buffer.rope();
        let eol = self.eol().as_str();
        self.scoped_bookmarks()
            .into_iter()
            .map(|l| {
                rope.slice(rope.line_to_char(l)..self.line_text_end(l))
                    .to_string()
                    + eol
            })
            .collect()
    }

    fn delete_lines(&mut self, lines: &[usize]) -> Result<(), EditError> {
        let rope = self.buffer.rope();
        let last_line = self.line_count() - 1;
        let selected: BTreeSet<usize> = lines.iter().copied().collect();
        let mut ranges: Vec<Range<usize>> = Vec::new();
        for &line in lines {
            let mut start = rope.line_to_char(line);
            let end = self.line_end(line);
            if start == end {
                continue;
            }
            if line == last_line && line > 0 && !selected.contains(&(line - 1)) {
                start = self.line_text_end(line - 1);
            }
            match ranges.last_mut() {
                Some(previous) if previous.end >= start => previous.end = end,
                _ => ranges.push(start..end),
            }
        }
        self.commit_lines(EditBatch(
            ranges
                .into_iter()
                .map(|range| Edit {
                    range,
                    text: String::new(),
                })
                .collect(),
        ))
    }

    pub fn cut_bookmarked_lines(&mut self) -> Result<String, EditError> {
        let text = self.copy_bookmarked_lines();
        self.remove_bookmarked_lines()?;
        Ok(text)
    }

    /// Replaces the text of every bookmarked line with `text`.
    pub fn paste_to_bookmarked_lines(&mut self, text: &str) -> Result<(), EditError> {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        let replacement = normalized
            .trim_end_matches('\n')
            .replace('\n', self.eol().as_str());
        let rope = self.buffer.rope();
        let edits = self
            .scoped_bookmarks()
            .into_iter()
            .map(|l| Edit {
                range: rope.line_to_char(l)..self.line_text_end(l),
                text: replacement.clone(),
            })
            .collect();
        self.commit_lines(EditBatch(edits))
    }

    pub fn remove_bookmarked_lines(&mut self) -> Result<(), EditError> {
        let lines = self.scoped_bookmarks();
        self.delete_lines(&lines)
    }

    pub fn remove_unbookmarked_lines(&mut self) -> Result<(), EditError> {
        let keep: BTreeSet<usize> = self.bookmarked_lines().into_iter().collect();
        let lines: Vec<usize> = (0..self.line_count())
            .filter(|l| !keep.contains(l) && self.line_in_scope(*l))
            .collect();
        self.delete_lines(&lines)
    }
}

#[cfg(test)]
mod tests {
    use crate::buffer::Buffer;
    use crate::document::{Document, Eol};
    use crate::selection::SelectionSet;

    const T: &str = "config a\n  set ip 1\nend\n  set ip 2\nfoo\n";

    fn marked(lines: &[usize]) -> Document {
        let mut d = Document::new(T, Eol::Lf);
        d.set_bookmarked_lines(lines);
        d
    }

    #[test]
    fn toggle_adds_and_removes() {
        let mut d = Document::new(T, Eol::Lf);
        d.toggle_bookmark(2);
        assert_eq!(d.bookmarked_lines(), vec![2]);
        d.toggle_bookmark(2);
        assert!(d.bookmarked_lines().is_empty());
    }

    #[test]
    fn bookmarks_follow_lines_through_edits() {
        let mut d = marked(&[2]);
        d.set_selections(SelectionSet::single(0));
        d.insert_text("x\n").unwrap();
        assert_eq!(d.bookmarked_lines(), vec![3]);
    }

    #[test]
    fn editing_the_start_of_a_line_keeps_its_bookmark() {
        let mut d = marked(&[1]);
        d.set_selections(SelectionSet::from_ranges(
            vec![crate::Selection {
                anchor: 9,
                head: 11,
            }],
            0,
        ));
        d.backspace().unwrap();
        assert_eq!(d.bookmarked_lines(), vec![1]);
    }

    #[test]
    fn bookmark_on_deleted_line_disappears() {
        let mut d = marked(&[1]);
        d.set_selections(SelectionSet::from_ranges(
            vec![crate::Selection {
                anchor: 9,
                head: 20,
            }],
            0,
        ));
        d.backspace().unwrap();
        assert!(d.bookmarked_lines().is_empty());
    }

    #[test]
    fn next_and_previous_wrap() {
        let d = marked(&[1, 3]);
        assert_eq!(d.next_bookmark(1), Some(3));
        assert_eq!(d.next_bookmark(3), Some(1));
        assert_eq!(d.previous_bookmark(3), Some(1));
        assert_eq!(d.previous_bookmark(1), Some(3));
        assert_eq!(Document::new(T, Eol::Lf).next_bookmark(0), None);
    }

    #[test]
    fn bookmark_lines_of_marks_each_line_once() {
        let mut d = Document::new(T, Eol::Lf);
        let n = d.bookmark_lines_of(&[11..14, 12..15, 26..29]);
        assert_eq!(n, 2);
        assert_eq!(d.bookmarked_lines(), vec![1, 3]);
    }

    #[test]
    fn match_ending_with_the_newline_does_not_mark_the_next_line() {
        let mut d = Document::new(T, Eol::Lf);
        d.bookmark_lines_of(&[0..9]);
        assert_eq!(d.bookmarked_lines(), vec![0]);
    }

    #[test]
    fn clear_removes_all() {
        let mut d = marked(&[1, 3]);
        d.clear_bookmarks();
        assert!(d.bookmarked_lines().is_empty());
    }

    #[test]
    fn inverse_covers_every_other_line() {
        let mut d = marked(&[1, 3]);
        d.inverse_bookmarks();
        assert_eq!(d.bookmarked_lines(), vec![0, 2, 4, 5]);
    }

    #[test]
    fn remove_bookmarked() {
        let mut d = marked(&[1, 3]);
        d.remove_bookmarked_lines().unwrap();
        assert_eq!(d.text(), "config a\nend\nfoo\n");
        assert!(d.bookmarked_lines().is_empty());
    }

    #[test]
    fn remove_unbookmarked() {
        let mut d = marked(&[1, 3]);
        d.remove_unbookmarked_lines().unwrap();
        assert_eq!(d.text(), "  set ip 1\n  set ip 2\n");
        assert_eq!(d.bookmarked_lines(), vec![0, 1]);
    }

    #[test]
    fn consecutive_lines_remove_cleanly() {
        let mut d = Document::new("a\nb\nc\nd\n", Eol::Lf);
        d.set_bookmarked_lines(&[1, 2]);
        d.remove_bookmarked_lines().unwrap();
        assert_eq!(d.text(), "a\nd\n");
    }

    #[test]
    fn last_line_without_newline() {
        let mut d = Document::new("a\nb", Eol::Lf);
        d.set_bookmarked_lines(&[1]);
        d.remove_bookmarked_lines().unwrap();
        assert_eq!(d.text(), "a");
    }

    #[test]
    fn removing_every_line_leaves_an_empty_document() {
        let mut d = Document::new("a\nb", Eol::Lf);
        d.set_bookmarked_lines(&[0, 1]);
        d.remove_bookmarked_lines().unwrap();
        assert_eq!(d.text(), "");
    }

    #[test]
    fn copy_preserves_order_and_uses_document_eol() {
        assert_eq!(
            marked(&[3, 1]).copy_bookmarked_lines(),
            "  set ip 1\n  set ip 2\n"
        );
        let mut d = Document::new("a\r\nb\r\n", Eol::CrLf);
        d.set_bookmarked_lines(&[0]);
        assert_eq!(d.copy_bookmarked_lines(), "a\r\n");
    }

    #[test]
    fn cut_returns_lines_and_removes_them() {
        let mut d = marked(&[1, 3]);
        assert_eq!(
            d.cut_bookmarked_lines().unwrap(),
            "  set ip 1\n  set ip 2\n"
        );
        assert_eq!(d.text(), "config a\nend\nfoo\n");
    }

    #[test]
    fn paste_replaces_each_bookmarked_line() {
        let mut d = marked(&[1, 3]);
        d.paste_to_bookmarked_lines("X\n").unwrap();
        assert_eq!(d.text(), "config a\nX\nend\nX\nfoo\n");
        assert_eq!(d.bookmarked_lines(), vec![1, 3]);
    }

    #[test]
    fn each_command_is_one_undo_and_restores_bookmarks() {
        type Cmd = fn(&mut Document);
        let commands: [Cmd; 4] = [
            |d| d.remove_bookmarked_lines().unwrap(),
            |d| d.remove_unbookmarked_lines().unwrap(),
            |d| {
                d.cut_bookmarked_lines().unwrap();
            },
            |d| d.paste_to_bookmarked_lines("X").unwrap(),
        ];
        for command in commands {
            let mut d = marked(&[1, 3]);
            command(&mut d);
            assert_ne!(d.text(), T);
            assert!(d.undo());
            assert_eq!(d.text(), T);
            assert_eq!(d.bookmarked_lines(), vec![1, 3]);
            assert!(!d.undo());
        }
    }

    fn backup() -> Document {
        let text = "[header]\nSEP\nline a\nline b\nline c\nSEP\n[/file]\n[file]\nSEP\n\u{FFFC}SEP\n[/file]\n";
        let start = "[header]\nSEP\n".chars().count();
        let end = start + "line a\nline b\nline c\n".chars().count();
        let locked = text.chars().position(|c| c == '\u{FFFC}').unwrap();
        Document::from_buffer(
            Buffer::with_structure(text, vec![locked], vec![start..end]),
            Eol::Lf,
        )
    }

    #[test]
    fn in_a_backup_commands_only_touch_zones() {
        let original = backup().text();
        let mut d = backup();
        d.set_bookmarked_lines(&[3]);
        d.remove_unbookmarked_lines().unwrap();
        assert_eq!(
            d.text(),
            original.replace("line a\n", "").replace("line c\n", "")
        );

        let mut d = backup();
        d.set_bookmarked_lines(&[0, 3]);
        d.remove_bookmarked_lines().unwrap();
        assert_eq!(d.text(), original.replace("line b\n", ""));

        let mut d = backup();
        d.set_bookmarked_lines(&[3]);
        d.inverse_bookmarks();
        assert_eq!(d.bookmarked_lines(), vec![2, 4]);

        let mut d = backup();
        d.bookmark_lines_of(&[0..8]);
        assert!(d.bookmarked_lines().is_empty());
    }
}
