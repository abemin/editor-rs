//! Syntax colors for visible lines. Filled in by the highlighting task.

use crate::doc_model::EditorDoc;
use crate::layout::DisplayLine;
use editor_syntax::Role;
use std::ops::Range;

/// Colored display-column ranges for one line.
pub fn spans(_doc: &EditorDoc, _line: usize, _display: &DisplayLine) -> Vec<(Range<usize>, Role)> {
    Vec::new()
}

/// Called after the text changes.
pub fn note_change(_tab: &crate::app::Tab) {}
