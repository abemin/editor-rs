//! Folded blocks: which lines are hidden, and mapping between screen rows and lines.

use crate::app::Tab;
use editor_syntax::FoldRange;
use std::ops::Range;

/// Folded blocks, sorted by start line. Lines after each start, up to its end, are hidden.
#[derive(Clone, Debug, Default)]
pub struct Folds {
    blocks: Vec<FoldRange>,
    hidden: Vec<Range<usize>>,
}

impl Folds {
    fn rebuild(&mut self) {
        self.blocks.sort_by_key(|f| (f.start_line, f.end_line));
        self.blocks.dedup();
        let mut hidden: Vec<Range<usize>> = Vec::new();
        for block in &self.blocks {
            let range = block.start_line + 1..block.end_line + 1;
            match hidden.last_mut() {
                Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
                _ => hidden.push(range),
            }
        }
        self.hidden = hidden;
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    pub fn starts(&self) -> impl Iterator<Item = usize> + '_ {
        self.blocks.iter().map(|b| b.start_line)
    }

    pub fn is_hidden(&self, line: usize) -> bool {
        let index = self.hidden.partition_point(|r| r.end <= line);
        self.hidden.get(index).is_some_and(|r| r.start <= line)
    }

    pub fn row_of_line(&self, line: usize) -> usize {
        let mut hidden_before = 0;
        for range in &self.hidden {
            if range.start > line {
                break;
            }
            if line < range.end {
                return range.start - 1 - hidden_before;
            }
            hidden_before += range.len();
        }
        line - hidden_before
    }

    pub fn line_of_row(&self, row: usize) -> usize {
        let mut line = row;
        for range in &self.hidden {
            if range.start <= line {
                line += range.len();
            } else {
                break;
            }
        }
        line
    }

    pub fn visible_rows(&self, total_lines: usize) -> usize {
        total_lines - self.hidden.iter().map(Range::len).sum::<usize>()
    }

    pub fn fold(&mut self, block: FoldRange) {
        self.blocks.push(block);
        self.rebuild();
    }

    /// Removes folds that hide `line` or start at it. Returns true if any were removed.
    pub fn unfold_at(&mut self, line: usize) -> bool {
        let before = self.blocks.len();
        self.blocks
            .retain(|b| !(b.start_line <= line && line <= b.end_line));
        self.rebuild();
        before != self.blocks.len()
    }

    pub fn clear(&mut self) {
        self.blocks.clear();
        self.hidden.clear();
    }
}

/// Keeps folds in place when lines are added or removed at `line`.
pub fn shift(tab: &Tab, line: usize, delta: isize) {
    let mut folds = tab.folds.borrow_mut();
    if folds.is_empty() {
        return;
    }
    folds.blocks = folds
        .blocks
        .iter()
        .filter(|b| b.end_line < line || b.start_line > line)
        .map(|b| {
            if b.start_line > line {
                FoldRange {
                    start_line: b.start_line.saturating_add_signed(delta),
                    end_line: b.end_line.saturating_add_signed(delta),
                }
            } else {
                *b
            }
        })
        .collect();
    folds.rebuild();
}

/// The innermost block containing `line`.
pub fn block_at(tab: &Tab, line: usize) -> Option<FoldRange> {
    let structure = crate::highlight::structure(tab)?;
    structure
        .folds
        .into_iter()
        .filter(|f| f.start_line <= line && line <= f.end_line && f.end_line > f.start_line)
        .min_by_key(|f| f.end_line - f.start_line)
}
