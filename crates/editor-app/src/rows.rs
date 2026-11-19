//! Screen rows. A row is one line, or one wrapped piece of a line, and folded
//! lines have no rows. Everything that turns positions into pixels goes through here.

use crate::app::Tab;
use crate::layout::display_line;
use crate::text_view::{metrics, text_left};
use std::ops::Range;

/// Wrapping is refused above this many lines, where measuring every line would be slow.
pub const WRAP_LINE_LIMIT: usize = 500_000;
const MIN_WRAP_COLUMNS: usize = 20;

pub struct WrapCache {
    revision: u64,
    columns: usize,
    /// First row of each line, plus the total at the end.
    first_row: Vec<usize>,
}

fn wrap_columns(tab: &Tab) -> usize {
    let m = metrics();
    let lines = tab.doc.borrow().doc.buffer().rope().len_lines();
    let width = tab.scroll.contentSize().width - text_left(lines, &m) - 2.0 * m.advance;
    ((width / m.advance).floor().max(0.0) as usize).max(MIN_WRAP_COLUMNS)
}

fn wrapping(tab: &Tab) -> bool {
    tab.doc.borrow().word_wrap
}

fn ensure(tab: &Tab) {
    let columns = wrap_columns(tab);
    let revision = tab.doc.borrow().doc.buffer().revision();
    let fresh = tab
        .wrap_cache
        .borrow()
        .as_ref()
        .is_some_and(|c| c.revision == revision && c.columns == columns);
    if fresh {
        return;
    }
    let doc = tab.doc.borrow();
    let lines = doc.doc.buffer().rope().len_lines();
    let mut first_row = Vec::with_capacity(lines + 1);
    let mut row = 0;
    for line in 0..lines {
        first_row.push(row);
        row += display_line(&doc, line).width().max(1).div_ceil(columns);
    }
    first_row.push(row);
    *tab.wrap_cache.borrow_mut() = Some(WrapCache {
        revision,
        columns,
        first_row,
    });
}

pub fn row_count(tab: &Tab) -> usize {
    if wrapping(tab) {
        ensure(tab);
        return tab
            .wrap_cache
            .borrow()
            .as_ref()
            .map_or(1, |c| *c.first_row.last().unwrap_or(&1));
    }
    let lines = tab.doc.borrow().doc.buffer().rope().len_lines();
    tab.folds.borrow().visible_rows(lines)
}

/// The line and wrapped piece shown on `row`.
pub fn locate(tab: &Tab, row: usize) -> (usize, usize) {
    if wrapping(tab) {
        ensure(tab);
        let cache = tab.wrap_cache.borrow();
        let cache = cache.as_ref().expect("wrap cache");
        let line = cache
            .first_row
            .partition_point(|&r| r <= row)
            .saturating_sub(1);
        return (line, row - cache.first_row[line]);
    }
    (tab.folds.borrow().line_of_row(row), 0)
}

/// Display columns shown by one piece of a line.
pub fn piece_columns(tab: &Tab, piece: usize, width: usize) -> Range<usize> {
    if !wrapping(tab) {
        return 0..width;
    }
    let columns = tab
        .wrap_cache
        .borrow()
        .as_ref()
        .map_or(width.max(1), |c| c.columns);
    let start = piece * columns;
    start.min(width)..(start + columns).min(width)
}

/// Row and column within the row for a display column on a line.
pub fn position(tab: &Tab, line: usize, column: usize) -> (usize, usize) {
    if wrapping(tab) {
        ensure(tab);
        let cache = tab.wrap_cache.borrow();
        let cache = cache.as_ref().expect("wrap cache");
        let pieces = cache.first_row[line + 1] - cache.first_row[line];
        let piece = (column / cache.columns).min(pieces.saturating_sub(1));
        return (
            cache.first_row[line] + piece,
            column - piece * cache.columns,
        );
    }
    (tab.folds.borrow().row_of_line(line), column)
}

pub fn invalidate(tab: &Tab) {
    *tab.wrap_cache.borrow_mut() = None;
}
