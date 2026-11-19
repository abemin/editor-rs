//! Turning a document line into what is drawn: tabs expanded, locked
//! sections replaced by labels, and a map between characters and columns.

use crate::doc_model::EditorDoc;
use editor_formats::fortiweb::PLACEHOLDER;
use editor_syntax::Role;
use std::ops::Range;

pub const TAB_WIDTH: usize = 4;

pub struct DisplayLine {
    /// Characters as drawn.
    pub chars: Vec<char>,
    /// Display column of each source character, plus one entry for the line end.
    /// `None` when every character is one column wide.
    pub columns: Option<Vec<u32>>,
    /// Colored ranges in display columns.
    pub roles: Vec<(Range<usize>, Role)>,
    /// Columns that belong to locked labels.
    pub locked: Vec<Range<usize>>,
}

impl DisplayLine {
    /// Display column of a character index within the line.
    pub fn column_of(&self, char_index: usize) -> usize {
        match &self.columns {
            Some(columns) => columns[char_index.min(columns.len() - 1)] as usize,
            None => char_index.min(self.chars.len()),
        }
    }

    /// Nearest character index for a display column.
    pub fn char_at_column(&self, column: usize) -> usize {
        match &self.columns {
            Some(columns) => {
                let index = columns.partition_point(|&c| (c as usize) < column);
                if index > 0 && index < columns.len() {
                    let (before, after) = (columns[index - 1] as usize, columns[index] as usize);
                    if column - before < after - column {
                        index - 1
                    } else {
                        index
                    }
                } else {
                    index.min(columns.len() - 1)
                }
            }
            None => column.min(self.chars.len()),
        }
    }

    pub fn width(&self) -> usize {
        self.chars.len()
    }
}

/// Source text of a line without its line ending.
pub fn line_text(doc: &EditorDoc, line: usize) -> String {
    let rope = doc.doc.buffer().rope();
    if line >= rope.len_lines() {
        return String::new();
    }
    let start = rope.line_to_char(line);
    let end = doc.doc.line_text_end(line);
    rope.slice(start..end).to_string()
}

pub fn display_line(doc: &EditorDoc, line: usize) -> DisplayLine {
    let text = line_text(doc, line);
    let simple = !text.contains(['\t', PLACEHOLDER]);
    if simple {
        return DisplayLine {
            chars: text.chars().collect(),
            columns: None,
            roles: Vec::new(),
            locked: Vec::new(),
        };
    }
    let rope = doc.doc.buffer().rope();
    let line_start = rope.line_to_char(line);
    let mut chars = Vec::with_capacity(text.len());
    let mut columns = Vec::with_capacity(text.len() + 1);
    let mut locked = Vec::new();
    for (index, c) in text.chars().enumerate() {
        columns.push(chars.len() as u32);
        match c {
            '\t' => {
                let spaces = TAB_WIDTH - chars.len() % TAB_WIDTH;
                chars.extend(std::iter::repeat_n(' ', spaces));
            }
            PLACEHOLDER => {
                let offset = line_start + index;
                let region = doc
                    .doc
                    .buffer()
                    .locked()
                    .iter()
                    .position(|&a| a == offset)
                    .unwrap_or(0);
                let start = chars.len();
                chars.extend(doc.locked_label(region).chars());
                locked.push(start..chars.len());
            }
            other => chars.push(other),
        }
    }
    columns.push(chars.len() as u32);
    DisplayLine {
        chars,
        columns: Some(columns),
        roles: Vec::new(),
        locked,
    }
}
