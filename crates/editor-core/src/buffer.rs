//! Rope-backed text storage with atomic batches of edits.
//!
//! Offsets are char indices. Locked anchors mark placeholder characters that
//! stand in for protected file regions and can never be edited away.

use ropey::Rope;
use std::ops::Range;

/// Replace `range` with `text`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub range: Range<usize>,
    pub text: String,
}

/// Edits sorted by position and not overlapping.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EditBatch(pub Vec<Edit>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditError {
    Overlapping,
    OutOfBounds,
    TouchesLocked,
    ReadOnly,
}

#[derive(Clone, Debug)]
pub struct AppliedBatch {
    pub inverse: EditBatch,
    pub revision: u64,
}

#[derive(Clone, Debug)]
pub struct Buffer {
    rope: Rope,
    revision: u64,
    locked: Vec<usize>,
    zones: Vec<Range<usize>>,
    read_only: bool,
}

impl Buffer {
    pub fn new(text: &str) -> Self {
        Self::with_structure(text, Vec::new(), Vec::new())
    }

    pub fn with_structure(text: &str, locked: Vec<usize>, zones: Vec<Range<usize>>) -> Self {
        Self {
            rope: Rope::from_str(text),
            revision: 0,
            locked,
            zones,
            read_only: false,
        }
    }

    pub fn set_read_only(&mut self, value: bool) {
        self.read_only = value;
    }

    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    pub fn rope(&self) -> &Rope {
        &self.rope
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn locked(&self) -> &[usize] {
        &self.locked
    }

    pub fn zones(&self) -> &[Range<usize>] {
        &self.zones
    }

    fn validate(&self, batch: &EditBatch) -> Result<(), EditError> {
        if self.read_only {
            return Err(EditError::ReadOnly);
        }
        let len = self.rope.len_chars();
        let mut previous_end = 0;
        for edit in &batch.0 {
            if edit.range.start > edit.range.end || edit.range.end > len {
                return Err(EditError::OutOfBounds);
            }
            if edit.range.start < previous_end {
                return Err(EditError::Overlapping);
            }
            previous_end = edit.range.end;
            let first = self.locked.partition_point(|&a| a < edit.range.start);
            if self.locked.get(first).is_some_and(|&a| a < edit.range.end) {
                return Err(EditError::TouchesLocked);
            }
        }
        Ok(())
    }

    /// Applies every edit or none of them.
    pub fn apply(&mut self, batch: &EditBatch) -> Result<AppliedBatch, EditError> {
        self.validate(batch)?;
        let map = BatchMap::new(batch);
        let mut inverse = Vec::with_capacity(batch.0.len());
        let mut delta = 0isize;
        for edit in &batch.0 {
            let removed = self.rope.slice(edit.range.clone()).to_string();
            let inserted = edit.text.chars().count();
            let start = edit.range.start.wrapping_add_signed(delta);
            inverse.push(Edit {
                range: start..start + inserted,
                text: removed,
            });
            delta += inserted as isize - edit.range.len() as isize;
        }
        for edit in batch.0.iter().rev() {
            if !edit.range.is_empty() {
                self.rope.remove(edit.range.clone());
            }
            if !edit.text.is_empty() {
                self.rope.insert(edit.range.start, &edit.text);
            }
        }
        for anchor in &mut self.locked {
            *anchor = map.boundary(*anchor, true);
        }
        for zone in &mut self.zones {
            *zone = map.boundary(zone.start, false)..map.boundary(zone.end, true);
        }
        self.revision += 1;
        Ok(AppliedBatch {
            inverse: EditBatch(inverse),
            revision: self.revision,
        })
    }
}

/// Maps offsets from before a batch to after it.
pub struct BatchMap {
    spans: Vec<MapSpan>,
}

struct MapSpan {
    start: usize,
    end: usize,
    inserted: usize,
    delta_before: isize,
}

impl BatchMap {
    pub fn new(batch: &EditBatch) -> Self {
        let mut delta = 0isize;
        let spans = batch
            .0
            .iter()
            .map(|edit| {
                let inserted = edit.text.chars().count();
                let span = MapSpan {
                    start: edit.range.start,
                    end: edit.range.end,
                    inserted,
                    delta_before: delta,
                };
                delta += inserted as isize - edit.range.len() as isize;
                span
            })
            .collect();
        Self { spans }
    }

    fn resolve(&self, offset: usize, bias_right: bool) -> Result<usize, (usize, usize)> {
        let first = self.spans.partition_point(|s| s.end < offset);
        let mut delta = match first.checked_sub(1).map(|i| &self.spans[i]) {
            Some(s) => s.delta_before + s.inserted as isize - (s.end - s.start) as isize,
            None => 0,
        };
        for span in &self.spans[first..] {
            if span.start > offset {
                break;
            }
            if span.start == span.end {
                if bias_right {
                    delta += span.inserted as isize;
                } else {
                    break;
                }
            } else if offset < span.end {
                let start = span.start.wrapping_add_signed(span.delta_before);
                return Err((start, start + span.inserted));
            } else {
                delta += span.inserted as isize - (span.end - span.start) as isize;
            }
        }
        Ok(offset.wrapping_add_signed(delta))
    }

    /// `None` when the character at `offset` was removed.
    pub fn map(&self, offset: usize, bias_right: bool) -> Option<usize> {
        self.resolve(offset, bias_right).ok()
    }

    /// Like [`BatchMap::map`], but a removed offset snaps to the replacement's edge.
    pub fn boundary(&self, offset: usize, bias_right: bool) -> usize {
        match self.resolve(offset, bias_right) {
            Ok(mapped) => mapped,
            Err((start, end)) => {
                if bias_right {
                    end
                } else {
                    start
                }
            }
        }
    }
}

/// Maps a single offset through `batch`. Returns `None` if it was deleted.
pub fn map_offset(offset: usize, batch: &EditBatch, bias_right: bool) -> Option<usize> {
    BatchMap::new(batch).map(offset, bias_right)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn edit(range: std::ops::Range<usize>, text: &str) -> Edit {
        Edit {
            range,
            text: text.to_string(),
        }
    }

    #[test]
    fn apply_is_atomic_back_to_front() {
        let mut b = Buffer::new("abcdef");
        b.apply(&EditBatch(vec![edit(1..2, "XX"), edit(4..5, "")]))
            .unwrap();
        assert_eq!(b.rope().to_string(), "aXXcdf");
    }

    #[test]
    fn inverse_restores_exact_text() {
        let mut b = Buffer::new("a\r\nb\rc\n");
        let a = b.apply(&EditBatch(vec![edit(1..3, "\n")])).unwrap();
        b.apply(&a.inverse).unwrap();
        assert_eq!(b.rope().to_string(), "a\r\nb\rc\n");
    }

    #[test]
    fn cr_lf_and_crlf_all_break_lines() {
        assert_eq!(Buffer::new("a\rb\r\nc\nd").rope().len_lines(), 4);
    }

    #[test]
    fn locked_anchor_moves_with_edits() {
        let mut b = Buffer::with_structure("ab\u{FFFC}cd", vec![2], vec![]);
        b.apply(&EditBatch(vec![edit(0..0, "zz")])).unwrap();
        assert_eq!(b.locked(), &[4]);
    }

    #[test]
    fn insert_exactly_at_locked_index_is_allowed() {
        let mut b = Buffer::with_structure("ab\u{FFFC}cd", vec![2], vec![]);
        b.apply(&EditBatch(vec![edit(2..2, "Q")])).unwrap();
        assert_eq!(b.locked(), &[3]);
        assert_eq!(b.rope().to_string(), "abQ\u{FFFC}cd");
    }

    #[test]
    fn batch_touching_locked_anchor_is_rejected_whole() {
        let mut b = Buffer::with_structure("ab\u{FFFC}cd", vec![2], vec![]);
        let r = b.apply(&EditBatch(vec![edit(0..1, "Q"), edit(1..3, "")]));
        assert_eq!(r.unwrap_err(), EditError::TouchesLocked);
        assert_eq!(b.rope().to_string(), "ab\u{FFFC}cd");
        assert_eq!(b.revision(), 0);
    }

    #[test]
    fn read_only_rejects_edits() {
        let mut b = Buffer::new("abc");
        b.set_read_only(true);
        assert!(b.is_read_only());
        assert_eq!(
            b.apply(&EditBatch(vec![edit(0..1, "")])).unwrap_err(),
            EditError::ReadOnly
        );
        assert_eq!(b.rope().to_string(), "abc");
    }

    #[test]
    fn overlapping_and_out_of_bounds_are_rejected() {
        let mut b = Buffer::new("abc");
        assert_eq!(
            b.apply(&EditBatch(vec![edit(0..2, ""), edit(1..3, "")]))
                .unwrap_err(),
            EditError::Overlapping
        );
        assert_eq!(
            b.apply(&EditBatch(vec![edit(2..9, "")])).unwrap_err(),
            EditError::OutOfBounds
        );
    }

    #[test]
    fn zones_grow_on_insert_inside_and_at_end() {
        let mut b = Buffer::with_structure("hdr\nbody\nsep", vec![], vec![4..9]);
        b.apply(&EditBatch(vec![edit(5..5, "yy"), edit(9..9, "x")]))
            .unwrap();
        assert_eq!(b.zones(), &[4..12]);
    }

    #[test]
    fn zone_shifts_when_text_is_added_before_it() {
        let mut b = Buffer::with_structure("hdr\nbody\n", vec![], vec![4..9]);
        b.apply(&EditBatch(vec![edit(0..0, "zz")])).unwrap();
        assert_eq!(b.zones(), &[6..11]);
    }

    #[test]
    fn map_offset_follows_edits() {
        let batch = EditBatch(vec![edit(2..4, "x"), edit(6..6, "yyy")]);
        assert_eq!(map_offset(1, &batch, false), Some(1));
        assert_eq!(map_offset(3, &batch, false), None);
        assert_eq!(map_offset(4, &batch, false), Some(3));
        assert_eq!(map_offset(6, &batch, false), Some(5));
        assert_eq!(map_offset(6, &batch, true), Some(8));
        assert_eq!(map_offset(7, &batch, false), Some(9));
    }

    proptest! {
        #[test]
        fn inverse_always_restores(text in "[a-z\\r\\n€]{0,60}", cuts in proptest::collection::vec((0usize..60, 0usize..6, "[a-z\\n]{0,4}"), 0..6)) {
            let mut b = Buffer::new(&text);
            let len = b.rope().len_chars();
            let mut edits = Vec::new();
            let mut at = 0;
            let mut sorted = cuts;
            sorted.sort_by_key(|c| c.0);
            for (start, del, ins) in sorted {
                let s = start.max(at).min(len);
                let e = (s + del).min(len);
                edits.push(edit(s..e, &ins));
                at = e + 1;
            }
            let applied = b.apply(&EditBatch(edits)).unwrap();
            b.apply(&applied.inverse).unwrap();
            prop_assert_eq!(b.rope().to_string(), text);
        }
    }
}
