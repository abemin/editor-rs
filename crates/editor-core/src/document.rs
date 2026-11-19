//! An editable document: buffer, selections, undo and bookmarks.

use crate::buffer::{BatchMap, Buffer, Edit, EditBatch, EditError};
use crate::history::{History, Kind, Snapshot, Step};
use crate::selection::{Selection, SelectionSet};
use std::collections::BTreeSet;
use unicode_segmentation::UnicodeSegmentation;

const GRAPHEME_WINDOW: usize = 32;

/// A line ending.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Eol {
    Lf,
    CrLf,
    Cr,
}

impl Eol {
    pub fn as_str(self) -> &'static str {
        match self {
            Eol::Lf => "\n",
            Eol::CrLf => "\r\n",
            Eol::Cr => "\r",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Left,
    Right,
    Up,
    Down,
    WordLeft,
    WordRight,
    LineStart,
    LineEnd,
    DocStart,
    DocEnd,
    PageUp(usize),
    PageDown(usize),
}

pub struct Document {
    pub(crate) buffer: Buffer,
    history: History,
    selections: SelectionSet,
    pub(crate) bookmarks: Vec<usize>,
    eol: Eol,
    indent_unit: String,
    change_log: Vec<EditBatch>,
    goal_columns: Option<Vec<usize>>,
}

impl Document {
    pub fn new(text: &str, eol: Eol) -> Self {
        Self::from_buffer(Buffer::new(text), eol)
    }

    pub fn from_buffer(buffer: Buffer, eol: Eol) -> Self {
        Self {
            buffer,
            history: History::default(),
            selections: SelectionSet::single(0),
            bookmarks: Vec::new(),
            eol,
            indent_unit: "    ".to_string(),
            change_log: Vec::new(),
            goal_columns: None,
        }
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub fn eol(&self) -> Eol {
        self.eol
    }

    pub fn text(&self) -> String {
        self.buffer.rope().to_string()
    }

    pub fn selections(&self) -> &SelectionSet {
        &self.selections
    }

    pub fn set_selections(&mut self, selections: SelectionSet) {
        let len = self.len();
        let clamped = selections
            .ranges()
            .iter()
            .map(|s| Selection {
                anchor: s.anchor.min(len),
                head: s.head.min(len),
            })
            .collect();
        let primary = selections
            .ranges()
            .iter()
            .position(|s| *s == selections.primary())
            .unwrap_or(0);
        self.selections = SelectionSet::from_ranges(clamped, primary);
        self.goal_columns = None;
        self.history.close();
    }

    pub fn is_dirty(&self) -> bool {
        self.history.is_dirty()
    }

    pub fn mark_saved(&mut self) {
        self.history.mark_saved();
    }

    fn len(&self) -> usize {
        self.buffer.rope().len_chars()
    }

    /// Zero-based line and column of a char offset.
    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        let rope = self.buffer.rope();
        let offset = offset.min(rope.len_chars());
        let line = rope.char_to_line(offset);
        (line, offset - rope.line_to_char(line))
    }

    /// Char offset of a line and column, clamped to the line's text.
    pub fn offset_at(&self, line: usize, column: usize) -> usize {
        let rope = self.buffer.rope();
        let line = line.min(rope.len_lines().saturating_sub(1));
        (rope.line_to_char(line) + column).min(self.line_text_end(line))
    }

    /// Offset just before the line's ending.
    pub fn line_text_end(&self, line: usize) -> usize {
        let rope = self.buffer.rope();
        let start = rope.line_to_char(line);
        let slice = rope.line(line);
        let mut len = slice.len_chars();
        if len > 0 && matches!(slice.char(len - 1), '\n' | '\r') {
            len -= 1;
            if len > 0 && slice.char(len) == '\n' && slice.char(len - 1) == '\r' {
                len -= 1;
            }
        }
        start + len
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            selections: self.selections.clone(),
            bookmarks: self.bookmarks.clone(),
        }
    }

    fn commit(
        &mut self,
        batch: EditBatch,
        after: SelectionSet,
        kind: Kind,
    ) -> Result<(), EditError> {
        let before = self.snapshot();
        let bookmarks = crate::bookmarks::remapped(self, &batch);
        let applied = self.buffer.apply(&batch)?;
        self.bookmarks = bookmarks;
        self.selections = after;
        self.goal_columns = None;
        let after = self.snapshot();
        self.history.record(
            Step {
                forward: batch.clone(),
                inverse: applied.inverse,
            },
            before,
            after,
            kind,
        );
        self.change_log.push(batch);
        Ok(())
    }

    fn replace_selections(
        &mut self,
        kind: Kind,
        text_for: impl Fn(&Self, Selection) -> String,
    ) -> Result<(), EditError> {
        let mut edits = Vec::new();
        let mut carets = Vec::new();
        let mut delta = 0isize;
        for s in self.selections.ranges().to_vec() {
            let text = text_for(self, s);
            let inserted = text.chars().count();
            carets.push(Selection::caret(
                (s.min() + inserted).wrapping_add_signed(delta),
            ));
            delta += inserted as isize - (s.max() - s.min()) as isize;
            edits.push(Edit {
                range: s.min()..s.max(),
                text,
            });
        }
        self.commit(
            EditBatch(edits),
            SelectionSet::from_ranges(carets, usize::MAX),
            kind,
        )
    }

    pub fn insert_text(&mut self, text: &str) -> Result<(), EditError> {
        self.replace_selections(Kind::Typing, |_, _| text.to_string())
    }

    /// Inserts pasted text, converting its line endings to the document's.
    pub fn paste_text(&mut self, text: &str) -> Result<(), EditError> {
        let normalized = text
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .replace('\n', self.eol.as_str());
        self.replace_selections(Kind::Other, |_, _| normalized.clone())
    }

    pub fn insert_newline(&mut self) -> Result<(), EditError> {
        let eol = self.eol.as_str();
        self.replace_selections(Kind::Typing, |d, s| {
            let (line, _) = d.line_col(s.min());
            let rope = d.buffer.rope();
            let start = rope.line_to_char(line);
            let indent: String = rope
                .slice(start..s.min())
                .chars()
                .take_while(|c| matches!(c, ' ' | '\t'))
                .collect();
            format!("{eol}{indent}")
        })?;
        self.history.close();
        Ok(())
    }

    fn previous_grapheme(&self, offset: usize) -> usize {
        let rope = self.buffer.rope();
        let start = offset.saturating_sub(GRAPHEME_WINDOW);
        let text = rope.slice(start..offset).to_string();
        offset
            - text
                .graphemes(true)
                .next_back()
                .map_or(0, |g| g.chars().count())
    }

    fn next_grapheme(&self, offset: usize) -> usize {
        let rope = self.buffer.rope();
        let end = (offset + GRAPHEME_WINDOW).min(rope.len_chars());
        let text = rope.slice(offset..end).to_string();
        offset + text.graphemes(true).next().map_or(0, |g| g.chars().count())
    }

    fn delete_ranges(
        &mut self,
        range_for: impl Fn(&Self, Selection) -> (usize, usize),
    ) -> Result<(), EditError> {
        let mut merged: Vec<(usize, usize)> = Vec::new();
        for s in self.selections.ranges() {
            let (from, to) = if s.is_caret() {
                range_for(self, *s)
            } else {
                (s.min(), s.max())
            };
            match merged.last_mut() {
                Some(last) if from <= last.1 => last.1 = last.1.max(to),
                _ => merged.push((from, to)),
            }
        }
        merged.retain(|(from, to)| from < to);
        if merged.is_empty() {
            return Ok(());
        }
        let batch = EditBatch(
            merged
                .iter()
                .map(|&(from, to)| Edit {
                    range: from..to,
                    text: String::new(),
                })
                .collect(),
        );
        let map = BatchMap::new(&batch);
        let carets = self
            .selections
            .ranges()
            .iter()
            .map(|s| Selection::caret(map.boundary(s.min(), false)))
            .collect();
        self.commit(
            batch,
            SelectionSet::from_ranges(carets, usize::MAX),
            Kind::Deleting,
        )
    }

    pub fn backspace(&mut self) -> Result<(), EditError> {
        self.delete_ranges(|d, s| (d.previous_grapheme(s.head), s.head))
    }

    pub fn delete_forward(&mut self) -> Result<(), EditError> {
        self.delete_ranges(|d, s| (s.head, d.next_grapheme(s.head)))
    }

    fn selected_lines(&self) -> Vec<usize> {
        let mut lines = BTreeSet::new();
        for s in self.selections.ranges() {
            let (first, _) = self.line_col(s.min());
            let (mut last, column) = self.line_col(s.max());
            if last > first && column == 0 {
                last -= 1;
            }
            lines.extend(first..=last);
        }
        lines.into_iter().collect()
    }

    fn apply_line_edits(&mut self, edits: Vec<Edit>) -> Result<(), EditError> {
        if edits.is_empty() {
            return Ok(());
        }
        let batch = EditBatch(edits);
        let map = BatchMap::new(&batch);
        let primary = self
            .selections
            .ranges()
            .iter()
            .position(|s| *s == self.selections.primary())
            .unwrap_or(0);
        let mapped = self
            .selections
            .ranges()
            .iter()
            .map(|s| {
                let (low, high) = (map.boundary(s.min(), false), map.boundary(s.max(), true));
                if s.anchor <= s.head {
                    Selection {
                        anchor: low,
                        head: high,
                    }
                } else {
                    Selection {
                        anchor: high,
                        head: low,
                    }
                }
            })
            .collect();
        self.commit(
            batch,
            SelectionSet::from_ranges(mapped, primary),
            Kind::Other,
        )
    }

    pub fn indent(&mut self) -> Result<(), EditError> {
        let rope = self.buffer.rope();
        let edits = self
            .selected_lines()
            .into_iter()
            .map(|line| {
                let at = rope.line_to_char(line);
                Edit {
                    range: at..at,
                    text: self.indent_unit.clone(),
                }
            })
            .collect();
        self.apply_line_edits(edits)
    }

    pub fn outdent(&mut self) -> Result<(), EditError> {
        let rope = self.buffer.rope();
        let unit = self.indent_unit.chars().count();
        let mut edits = Vec::new();
        for line in self.selected_lines() {
            let start = rope.line_to_char(line);
            let mut count = 0;
            for c in rope.line(line).chars().take(unit) {
                match c {
                    '\t' if count == 0 => {
                        count = 1;
                        break;
                    }
                    ' ' => count += 1,
                    _ => break,
                }
            }
            if count > 0 {
                edits.push(Edit {
                    range: start..start + count,
                    text: String::new(),
                });
            }
        }
        self.apply_line_edits(edits)
    }

    fn char_class(c: char) -> u8 {
        if c.is_whitespace() {
            0
        } else if c.is_alphanumeric() || c == '_' {
            1
        } else {
            2
        }
    }

    fn word_left(&self, offset: usize) -> usize {
        let rope = self.buffer.rope();
        let mut at = offset;
        while at > 0 && Self::char_class(rope.char(at - 1)) == 0 {
            at -= 1;
        }
        if at > 0 {
            let class = Self::char_class(rope.char(at - 1));
            while at > 0 && Self::char_class(rope.char(at - 1)) == class {
                at -= 1;
            }
        }
        at
    }

    fn word_right(&self, offset: usize) -> usize {
        let rope = self.buffer.rope();
        let len = rope.len_chars();
        let mut at = offset;
        while at < len && Self::char_class(rope.char(at)) == 0 {
            at += 1;
        }
        if at < len {
            let class = Self::char_class(rope.char(at));
            while at < len && Self::char_class(rope.char(at)) == class {
                at += 1;
            }
        }
        at
    }

    fn line_start_target(&self, offset: usize) -> usize {
        let rope = self.buffer.rope();
        let (line, _) = self.line_col(offset);
        let start = rope.line_to_char(line);
        let first = start
            + rope
                .line(line)
                .chars()
                .take_while(|c| matches!(c, ' ' | '\t'))
                .count();
        if offset == first { start } else { first }
    }

    fn vertical(&self, offset: usize, lines: isize, goal: usize) -> usize {
        let (line, _) = self.line_col(offset);
        let last = self.buffer.rope().len_lines().saturating_sub(1);
        match line.checked_add_signed(lines) {
            None => 0,
            Some(target) if target > last => self.len(),
            Some(target) => self.offset_at(target, goal),
        }
    }

    pub fn move_cursors(&mut self, motion: Motion, extend: bool) {
        let vertical = matches!(
            motion,
            Motion::Up | Motion::Down | Motion::PageUp(_) | Motion::PageDown(_)
        );
        let current = self.selections.ranges().to_vec();
        let goals: Vec<usize> = match (&self.goal_columns, vertical) {
            (Some(g), true) if g.len() == current.len() => g.clone(),
            _ => current.iter().map(|s| self.line_col(s.head).1).collect(),
        };
        let moved = current
            .iter()
            .zip(&goals)
            .map(|(s, &goal)| {
                let head = match motion {
                    Motion::Left if !extend && !s.is_caret() => s.min(),
                    Motion::Right if !extend && !s.is_caret() => s.max(),
                    Motion::Left => self.previous_grapheme(s.head),
                    Motion::Right => self.next_grapheme(s.head),
                    Motion::Up => self.vertical(s.head, -1, goal),
                    Motion::Down => self.vertical(s.head, 1, goal),
                    Motion::PageUp(n) => self.vertical(s.head, -(n as isize), goal),
                    Motion::PageDown(n) => self.vertical(s.head, n as isize, goal),
                    Motion::WordLeft => self.word_left(s.head),
                    Motion::WordRight => self.word_right(s.head),
                    Motion::LineStart => self.line_start_target(s.head),
                    Motion::LineEnd => self.line_text_end(self.line_col(s.head).0),
                    Motion::DocStart => 0,
                    Motion::DocEnd => self.len(),
                };
                Selection {
                    anchor: if extend { s.anchor } else { head },
                    head,
                }
            })
            .collect();
        self.selections = SelectionSet::from_ranges(moved, usize::MAX);
        self.goal_columns = vertical
            .then_some(goals)
            .filter(|g| g.len() == self.selections.ranges().len());
        self.history.close();
    }

    /// Replaces the selections with one range per line between two (line, column) points.
    pub fn add_column_selection(&mut self, from: (usize, usize), to: (usize, usize)) {
        let (first, last) = (from.0.min(to.0), from.0.max(to.0));
        let ranges = (first..=last)
            .map(|line| Selection {
                anchor: self.offset_at(line, from.1),
                head: self.offset_at(line, to.1),
            })
            .collect();
        self.set_selections(SelectionSet::from_ranges(ranges, usize::MAX));
    }

    pub fn selected_text(&self) -> String {
        let rope = self.buffer.rope();
        let parts: Vec<String> = self
            .selections
            .ranges()
            .iter()
            .map(|s| rope.slice(s.min()..s.max()).to_string())
            .collect();
        parts.join(self.eol.as_str())
    }

    /// Applies a prepared batch as one undo step.
    pub fn apply_batch(&mut self, batch: EditBatch, _label: &'static str) -> Result<(), EditError> {
        let map = BatchMap::new(&batch);
        let mapped = self
            .selections
            .ranges()
            .iter()
            .map(|s| Selection {
                anchor: map.boundary(s.anchor, true),
                head: map.boundary(s.head, true),
            })
            .collect();
        self.commit(
            batch,
            SelectionSet::from_ranges(mapped, usize::MAX),
            Kind::Other,
        )
    }

    pub fn undo(&mut self) -> bool {
        if self.buffer.is_read_only() {
            return false;
        }
        let Some(group) = self.history.pop_undo() else {
            return false;
        };
        for step in group.steps.iter().rev() {
            self.buffer
                .apply(&step.inverse)
                .expect("undo applies to the state it was recorded from");
            self.change_log.push(step.inverse.clone());
        }
        self.selections = group.selections_before.clone();
        if group.is_whole_command() {
            self.bookmarks = group.bookmarks_before.clone();
        }
        self.goal_columns = None;
        self.history.push_redo(group);
        true
    }

    pub fn redo(&mut self) -> bool {
        if self.buffer.is_read_only() {
            return false;
        }
        let Some(group) = self.history.pop_redo() else {
            return false;
        };
        for step in &group.steps {
            self.buffer
                .apply(&step.forward)
                .expect("redo applies to the state it was recorded from");
            self.change_log.push(step.forward.clone());
        }
        self.selections = group.selections_after.clone();
        if group.is_whole_command() {
            self.bookmarks = group.bookmarks_after.clone();
        }
        self.goal_columns = None;
        self.history.push_undo(group);
        true
    }

    /// Edits not yet taken by `take_change_log`, oldest first.
    pub fn pending_changes(&self) -> &[EditBatch] {
        &self.change_log
    }

    /// Edits made since the last call, oldest first.
    pub fn take_change_log(&mut self) -> Vec<EditBatch> {
        std::mem::take(&mut self.change_log)
    }

    /// Runs a whole-line command as one undo step.
    pub(crate) fn commit_lines(&mut self, batch: EditBatch) -> Result<(), EditError> {
        let Some(first) = batch.0.first() else {
            return Ok(());
        };
        let caret = first.range.start;
        self.commit(batch, SelectionSet::single(caret), Kind::Other)?;
        let len = self.len();
        self.selections = SelectionSet::single(caret.min(len));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::{Buffer, Edit, EditBatch, EditError};

    fn doc(text: &str, eol: Eol) -> Document {
        Document::new(text, eol)
    }
    fn caret(d: &mut Document, at: usize) {
        d.set_selections(SelectionSet::single(at));
    }

    #[test]
    fn newline_uses_dominant_eol() {
        let mut d = doc("a\r\nb", Eol::CrLf);
        caret(&mut d, 1);
        d.insert_newline().unwrap();
        assert_eq!(d.text(), "a\r\n\r\nb");
    }

    #[test]
    fn newline_keeps_indent() {
        let mut d = doc("  set a", Eol::Lf);
        caret(&mut d, 7);
        d.insert_newline().unwrap();
        assert_eq!(d.text(), "  set a\n  ");
    }

    #[test]
    fn multi_cursor_typing_is_one_undo() {
        let mut d = doc("ab", Eol::Lf);
        let mut s = SelectionSet::single(0);
        s.add(Selection::caret(2));
        d.set_selections(s);
        d.insert_text("x").unwrap();
        assert_eq!(d.text(), "xabx");
        assert!(d.undo());
        assert_eq!(d.text(), "ab");
        assert!(d.redo());
        assert_eq!(d.text(), "xabx");
    }

    #[test]
    fn typing_within_a_run_coalesces_but_a_newline_splits() {
        let mut d = doc("", Eol::Lf);
        d.insert_text("a").unwrap();
        d.insert_text("b").unwrap();
        d.insert_newline().unwrap();
        d.insert_text("c").unwrap();
        assert!(d.undo());
        assert_eq!(d.text(), "ab\n");
        assert!(d.undo());
        assert_eq!(d.text(), "");
        assert!(!d.undo());
    }

    #[test]
    fn moving_the_caret_splits_typing() {
        let mut d = doc("", Eol::Lf);
        d.insert_text("a").unwrap();
        d.move_cursors(Motion::Left, false);
        d.insert_text("b").unwrap();
        d.undo();
        assert_eq!(d.text(), "a");
    }

    #[test]
    fn backspace_removes_whole_crlf() {
        let mut d = doc("a\r\nb", Eol::CrLf);
        caret(&mut d, 3);
        d.backspace().unwrap();
        assert_eq!(d.text(), "ab");
    }

    #[test]
    fn backspace_removes_grapheme_cluster() {
        let mut d = doc("e\u{301}x", Eol::Lf);
        caret(&mut d, 2);
        d.backspace().unwrap();
        assert_eq!(d.text(), "x");
    }

    #[test]
    fn delete_forward_removes_next_grapheme() {
        let mut d = doc("a\r\nb", Eol::CrLf);
        caret(&mut d, 1);
        d.delete_forward().unwrap();
        assert_eq!(d.text(), "ab");
    }

    #[test]
    fn column_selection_clamps_short_lines() {
        let mut d = doc("abcd\na\nabcd", Eol::Lf);
        d.add_column_selection((0, 1), (2, 3));
        let r = d.selections().ranges().to_vec();
        assert_eq!(r.len(), 3);
        assert_eq!((r[0].min(), r[0].max()), (1, 3));
        assert!(r[1].is_caret());
        assert_eq!(r[1].head, 6);
        assert_eq!((r[2].min(), r[2].max()), (8, 10));
        assert_eq!(d.selected_text(), "bc\n\nbc");
    }

    #[test]
    fn dirty_tracks_saved_revision_through_undo() {
        let mut d = doc("", Eol::Lf);
        assert!(!d.is_dirty());
        d.insert_text("a").unwrap();
        assert!(d.is_dirty());
        d.mark_saved();
        assert!(!d.is_dirty());
        d.insert_text("b").unwrap();
        assert!(d.is_dirty());
        d.undo();
        assert!(!d.is_dirty());
    }

    #[test]
    fn rejected_edit_changes_nothing() {
        let buffer = Buffer::with_structure("ab\u{FFFC}cd", vec![2], vec![]);
        let mut d = Document::from_buffer(buffer, Eol::Lf);
        d.set_selections(SelectionSet::from_ranges(
            vec![Selection { anchor: 1, head: 4 }],
            0,
        ));
        assert_eq!(d.backspace().unwrap_err(), EditError::TouchesLocked);
        assert_eq!(d.text(), "ab\u{FFFC}cd");
        assert_eq!(d.selections().primary(), Selection { anchor: 1, head: 4 });
        assert!(!d.undo());
    }

    #[test]
    fn apply_batch_is_one_undo_step() {
        let mut d = doc("aXbXc", Eol::Lf);
        let e = |r: std::ops::Range<usize>| Edit {
            range: r,
            text: "-".into(),
        };
        d.apply_batch(EditBatch(vec![e(1..2), e(3..4)]), "Replace All")
            .unwrap();
        assert_eq!(d.text(), "a-b-c");
        d.undo();
        assert_eq!(d.text(), "aXbXc");
    }

    #[test]
    fn indent_and_outdent_touch_every_selected_line() {
        let mut d = doc("a\nb\nc", Eol::Lf);
        d.set_selections(SelectionSet::from_ranges(
            vec![Selection { anchor: 0, head: 3 }],
            0,
        ));
        d.indent().unwrap();
        assert_eq!(d.text(), "    a\n    b\nc");
        d.outdent().unwrap();
        assert_eq!(d.text(), "a\nb\nc");
    }

    #[test]
    fn motions_work_across_lines() {
        let mut d = doc("one two\nthree", Eol::Lf);
        caret(&mut d, 5);
        d.move_cursors(Motion::Down, false);
        assert_eq!(d.selections().primary().head, 13);
        d.move_cursors(Motion::Up, false);
        assert_eq!(d.selections().primary().head, 5);
        d.move_cursors(Motion::LineEnd, true);
        assert_eq!(d.selections().primary(), Selection { anchor: 5, head: 7 });
        d.move_cursors(Motion::WordLeft, false);
        assert_eq!(d.selections().primary().head, 4);
        d.move_cursors(Motion::DocEnd, false);
        assert_eq!(d.selections().primary().head, 13);
    }

    #[test]
    fn pending_changes_show_unread_edits() {
        let mut d = doc("", Eol::Lf);
        d.insert_text("a").unwrap();
        assert_eq!(d.pending_changes().len(), 1);
        d.take_change_log();
        assert!(d.pending_changes().is_empty());
    }

    #[test]
    fn change_log_reports_edits_including_undo() {
        let mut d = doc("", Eol::Lf);
        d.insert_text("a").unwrap();
        d.undo();
        assert_eq!(d.take_change_log().len(), 2);
        assert!(d.take_change_log().is_empty());
    }
}
