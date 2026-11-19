//! Syntax colors and block structure for visible lines.
//!
//! Fortinet CLI keeps the scanner state at the start of each line and only
//! rescans from the first changed line. Other languages use tree-sitter and
//! re-parse after a change; very large files are shown without colors.

use crate::app::Tab;
use crate::doc_model::EditorDoc;
use crate::layout::{DisplayLine, line_text};
use editor_syntax::{
    CliStructure, Language, LineState, Role, Span, TreeHighlighter, cli_structure,
    highlight_cli_line, highlight_diff_line,
};
use std::ops::Range;

const TREE_LIMIT_BYTES: usize = 5_000_000;

pub struct Highlighting {
    language: Language,
    revision: u64,
    lines: usize,
    cli_states: Vec<LineState>,
    structure: Option<(u64, CliStructure)>,
    tree: Option<TreeHighlighter>,
    tree_revision: Option<u64>,
}

impl Default for Highlighting {
    fn default() -> Self {
        Self {
            language: Language::Plain,
            revision: 0,
            lines: 0,
            cli_states: vec![LineState::default()],
            structure: None,
            tree: None,
            tree_revision: None,
        }
    }
}

/// Starts over, for a new document or language.
pub fn reset(tab: &Tab) {
    let doc = tab.doc.borrow();
    let mut h = tab.highlight.borrow_mut();
    *h = Highlighting {
        language: doc.language,
        revision: doc.doc.buffer().revision(),
        lines: doc.doc.buffer().rope().len_lines(),
        ..Default::default()
    };
    if doc.doc.buffer().rope().len_bytes() <= TREE_LIMIT_BYTES {
        h.tree = TreeHighlighter::new(doc.language);
    }
}

/// Called after the text changes, before the change log is consumed.
pub fn note_change(tab: &Tab) {
    let (language, revision, lines, first_changed) = {
        let doc = tab.doc.borrow();
        let first = doc
            .doc
            .pending_changes()
            .iter()
            .filter_map(|b| b.0.first())
            .map(|e| e.range.start)
            .min();
        let first_line = first.map(|offset| {
            doc.doc
                .line_col(offset.min(doc.doc.buffer().rope().len_chars()))
                .0
        });
        (
            doc.language,
            doc.doc.buffer().revision(),
            doc.doc.buffer().rope().len_lines(),
            first_line,
        )
    };
    let needs_reset = tab.highlight.borrow().language != language;
    if needs_reset {
        reset(tab);
        return;
    }
    let mut h = tab.highlight.borrow_mut();
    if h.revision == revision {
        return;
    }
    let line = first_changed.unwrap_or(0);
    h.cli_states.truncate(line + 1);
    h.structure = None;
    let delta = lines as isize - h.lines as isize;
    h.lines = lines;
    h.revision = revision;
    drop(h);
    crate::folds::shift(tab, line, delta);
}

fn states_up_to(tab: &Tab, doc: &EditorDoc, line: usize) -> LineState {
    let mut h = tab.highlight.borrow_mut();
    while h.cli_states.len() <= line {
        let index = h.cli_states.len() - 1;
        let state = h.cli_states[index];
        let (_, next) = highlight_cli_line(&line_text(doc, index), &state);
        h.cli_states.push(next);
    }
    h.cli_states[line]
}

/// Converts byte offsets within a line to display columns.
fn to_columns(
    text: &str,
    display: &DisplayLine,
    spans: impl IntoIterator<Item = (Range<usize>, Role)>,
) -> Vec<(Range<usize>, Role)> {
    let mut char_at_byte = vec![0usize; text.len() + 1];
    let mut count = 0;
    for (byte, c) in text.char_indices() {
        for slot in &mut char_at_byte[byte..byte + c.len_utf8()] {
            *slot = count;
        }
        count += 1;
    }
    char_at_byte[text.len()] = count;
    spans
        .into_iter()
        .filter(|(_, role)| *role != Role::Plain)
        .map(|(range, role)| {
            let start = display.column_of(char_at_byte[range.start.min(text.len())]);
            let end = display.column_of(char_at_byte[range.end.min(text.len())]);
            (start..end, role)
        })
        .collect()
}

/// Colored display-column ranges for one line.
pub fn spans(
    tab: &Tab,
    doc: &EditorDoc,
    line: usize,
    display: &DisplayLine,
) -> Vec<(Range<usize>, Role)> {
    let text = line_text(doc, line);
    match doc.language {
        Language::Plain => Vec::new(),
        Language::FortiWebCli => {
            let state = states_up_to(tab, doc, line);
            let (spans, _) = highlight_cli_line(&text, &state);
            to_columns(
                &text,
                display,
                spans.into_iter().map(|Span { range, role }| (range, role)),
            )
        }
        Language::Diff => to_columns(
            &text,
            display,
            highlight_diff_line(&text)
                .into_iter()
                .map(|s| (s.range, s.role)),
        ),
        _ => {
            let rope = doc.doc.buffer().rope();
            let revision = doc.doc.buffer().revision();
            let mut h = tab.highlight.borrow_mut();
            if h.tree_revision != Some(revision)
                && let Some(tree) = h.tree.as_mut()
            {
                tree.parse(rope);
                h.tree_revision = Some(revision);
            }
            let Some(tree) = h.tree.as_ref() else {
                return Vec::new();
            };
            let start = rope.line_to_byte(line);
            let end = start + text.len();
            let spans = tree
                .spans(rope, start..end.max(start + 1))
                .into_iter()
                .filter_map(|(range, role)| {
                    let s = range.start.max(start);
                    let e = range.end.min(end);
                    (s < e).then(|| (s - start..e - start, role))
                });
            to_columns(&text, display, spans.collect::<Vec<_>>())
        }
    }
}

/// The role covering a display column, for tests.
pub fn role_at(tab: &Tab, line: usize, column: usize) -> Role {
    let doc = tab.doc.borrow();
    let display = crate::layout::display_line(&doc, line);
    spans(tab, &doc, line, &display)
        .into_iter()
        .rev()
        .find(|(r, _)| r.start <= column && column < r.end)
        .map_or(Role::Plain, |(_, role)| role)
}

/// Block structure for CLI documents, cached per revision.
pub fn structure(tab: &Tab) -> Option<CliStructure> {
    let doc = tab.doc.borrow();
    if doc.language != Language::FortiWebCli {
        return None;
    }
    let revision = doc.doc.buffer().revision();
    if let Some((cached, s)) = tab.highlight.borrow().structure.as_ref()
        && *cached == revision
    {
        return Some(s.clone());
    }
    let rope = doc.doc.buffer().rope();
    let lines: Vec<String> = (0..rope.len_lines()).map(|l| line_text(&doc, l)).collect();
    let s = cli_structure(lines.iter().map(String::as_str));
    tab.highlight.borrow_mut().structure = Some((revision, s.clone()));
    Some(s)
}
