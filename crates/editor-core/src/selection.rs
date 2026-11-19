//! Cursors and selected ranges.

/// A caret when `anchor == head`, otherwise a selected range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
}

impl Selection {
    pub fn caret(at: usize) -> Self {
        Self {
            anchor: at,
            head: at,
        }
    }

    pub fn min(&self) -> usize {
        self.anchor.min(self.head)
    }

    pub fn max(&self) -> usize {
        self.anchor.max(self.head)
    }

    pub fn is_caret(&self) -> bool {
        self.anchor == self.head
    }
}

/// Sorted, merged selections. There is always at least one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionSet {
    ranges: Vec<Selection>,
    primary: usize,
}

impl SelectionSet {
    pub fn single(at: usize) -> Self {
        Self {
            ranges: vec![Selection::caret(at)],
            primary: 0,
        }
    }

    pub fn from_ranges(ranges: Vec<Selection>, primary: usize) -> Self {
        let primary_selection = ranges.get(primary).copied();
        let mut sorted = ranges;
        sorted.sort_by_key(|s| (s.min(), s.max()));
        let mut merged: Vec<Selection> = Vec::with_capacity(sorted.len());
        for s in sorted {
            match merged.last_mut() {
                Some(last)
                    if s.min() < last.max() || s.min() == last.min() && s.max() == last.max() =>
                {
                    let (low, high) = (last.min().min(s.min()), last.max().max(s.max()));
                    *last = if last.anchor <= last.head {
                        Selection {
                            anchor: low,
                            head: high,
                        }
                    } else {
                        Selection {
                            anchor: high,
                            head: low,
                        }
                    };
                }
                _ => merged.push(s),
            }
        }
        if merged.is_empty() {
            merged.push(Selection::caret(0));
        }
        let primary = primary_selection
            .and_then(|p| {
                merged
                    .iter()
                    .position(|m| m.min() <= p.head && p.head <= m.max())
            })
            .unwrap_or(merged.len() - 1);
        Self {
            ranges: merged,
            primary,
        }
    }

    pub fn ranges(&self) -> &[Selection] {
        &self.ranges
    }

    pub fn primary(&self) -> Selection {
        self.ranges[self.primary]
    }

    /// Adds a selection and makes it the primary one.
    pub fn add(&mut self, s: Selection) {
        let mut ranges = std::mem::take(&mut self.ranges);
        ranges.push(s);
        let primary = ranges.len() - 1;
        *self = Self::from_ranges(ranges, primary);
    }
}
