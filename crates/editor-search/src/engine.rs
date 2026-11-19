//! Finding matches in a text snapshot.

use crate::query::Query;
use ropey::Rope;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};

const CHECKPOINT_EVERY: usize = 1024;
const CANCEL_CHECK_EVERY: usize = 256;

/// The search was stopped by the user.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cancelled;

/// A contiguous copy of the document, with fast byte and char conversion.
pub struct SearchText {
    pub(crate) text: String,
    ascii: bool,
    checkpoints: Vec<usize>,
    locked: Vec<usize>,
}

impl SearchText {
    pub fn new(rope: &Rope, locked: &[usize]) -> Self {
        let text = rope.to_string();
        let ascii = text.is_ascii();
        let checkpoints = if ascii {
            Vec::new()
        } else {
            text.char_indices()
                .step_by(CHECKPOINT_EVERY)
                .map(|(byte, _)| byte)
                .collect()
        };
        Self {
            text,
            ascii,
            checkpoints,
            locked: locked.to_vec(),
        }
    }

    pub(crate) fn byte_of(&self, char_offset: usize) -> usize {
        if self.ascii {
            return char_offset.min(self.text.len());
        }
        let index = char_offset / CHECKPOINT_EVERY;
        let Some(&start) = self.checkpoints.get(index) else {
            return self.text.len();
        };
        let remaining = char_offset - index * CHECKPOINT_EVERY;
        self.text[start..]
            .char_indices()
            .nth(remaining)
            .map_or(self.text.len(), |(byte, _)| start + byte)
    }

    pub(crate) fn char_of(&self, byte: usize) -> usize {
        if self.ascii {
            return byte;
        }
        let index = self
            .checkpoints
            .partition_point(|&b| b <= byte)
            .saturating_sub(1);
        index * CHECKPOINT_EVERY + self.text[self.checkpoints[index]..byte].chars().count()
    }

    fn touches_locked(&self, range: &Range<usize>) -> bool {
        let first = self.locked.partition_point(|&a| a < range.start);
        self.locked.get(first).is_some_and(|&a| a < range.end)
    }

    fn byte_scope(&self, scope: Option<&Range<usize>>) -> Range<usize> {
        scope.map_or(0..self.text.len(), |s| {
            self.byte_of(s.start)..self.byte_of(s.end)
        })
    }
}

/// Calls `visit` with each match in order until it returns false.
fn for_each_match(
    query: &Query,
    t: &SearchText,
    bytes: Range<usize>,
    mut visit: impl FnMut(Range<usize>) -> bool,
) {
    let haystack = &t.text[..bytes.end];
    let mut at = bytes.start;
    let mut last_end = None;
    while at <= haystack.len() {
        let Some(found) = query.find_at(haystack, at) else {
            return;
        };
        let empty = found.is_empty();
        let adjacent_empty = empty && last_end == Some(found.start);
        at = if empty {
            found.end
                + haystack[found.end..]
                    .chars()
                    .next()
                    .map_or(1, char::len_utf8)
        } else {
            found.end
        };
        if adjacent_empty {
            continue;
        }
        last_end = Some(found.end);
        let chars = t.char_of(found.start)..t.char_of(found.end);
        if !t.touches_locked(&chars) && !visit(chars) {
            return;
        }
    }
}

/// Finds the nearest match from `from`, optionally wrapping around the scope.
pub fn find_next(
    query: &Query,
    t: &SearchText,
    from: usize,
    backward: bool,
    wrap: bool,
    scope: Option<Range<usize>>,
) -> Option<Range<usize>> {
    let bytes = t.byte_scope(scope.as_ref());
    if backward {
        let (mut before, mut last) = (None, None);
        for_each_match(query, t, bytes, |m| {
            if m.end <= from {
                before = Some(m.clone());
            }
            last = Some(m);
            true
        });
        return before.or(if wrap { last } else { None });
    }
    let start = t.byte_of(from).clamp(bytes.start, bytes.end);
    let mut found = None;
    for_each_match(query, t, start..bytes.end, |m| {
        found = Some(m);
        false
    });
    if found.is_none() && wrap {
        for_each_match(query, t, bytes, |m| {
            found = Some(m);
            false
        });
    }
    found
}

/// Finds every match. Can be stopped through `cancel`.
pub fn find_all(
    query: &Query,
    t: &SearchText,
    scope: Option<Range<usize>>,
    cancel: &AtomicBool,
) -> Result<Vec<Range<usize>>, Cancelled> {
    if cancel.load(Ordering::Relaxed) {
        return Err(Cancelled);
    }
    let mut matches = Vec::new();
    let mut cancelled = false;
    for_each_match(query, t, t.byte_scope(scope.as_ref()), |m| {
        matches.push(m);
        cancelled = matches.len() % CANCEL_CHECK_EVERY == 0 && cancel.load(Ordering::Relaxed);
        !cancelled
    });
    if cancelled {
        Err(Cancelled)
    } else {
        Ok(matches)
    }
}
