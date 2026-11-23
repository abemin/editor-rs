//! Editing commands shared by the keyboard, mouse, menus and tests.

use crate::app::Tab;
use crate::layout::display_line;
use crate::text_view::{metrics, scroll_to, text_left, update_frames};
use editor_core::{Document, EditError, Motion, Selection, SelectionSet};
use objc2_app_kit::{NSBeep, NSPasteboard, NSPasteboardTypeString};
use objc2_foundation::{NSPoint, NSString};
use std::rc::Rc;

pub const LOCKED_NOTICE: &str =
    "This change would touch a protected section of the FortiWeb backup, so it was not made.";

/// Shows a short message in the status bar and plays the alert sound.
pub fn notice(tab: &Tab, message: &str) {
    *tab.notice.borrow_mut() = Some(message.to_string());
    crate::status_bar::refresh(tab);
    NSBeep();
}

/// Runs an edit and updates the view. Errors become a notice; nothing changes.
pub fn edit(tab: &Rc<Tab>, f: impl FnOnce(&mut Document) -> Result<(), EditError>) -> bool {
    let result = {
        let mut doc = tab.doc.borrow_mut();
        match doc.read_only.clone() {
            Some(reason) => Err((EditError::ReadOnly, reason)),
            None if on_locked_line(&doc.doc) => Err((EditError::TouchesLocked, String::new())),
            None => f(&mut doc.doc).map_err(|e| (e, String::new())),
        }
    };
    match result {
        Ok(()) => {
            after_change(tab, true);
            true
        }
        Err((EditError::TouchesLocked, _)) => {
            notice(tab, LOCKED_NOTICE);
            false
        }
        Err((EditError::ReadOnly, reason)) => {
            notice(
                tab,
                if reason.is_empty() {
                    "This document is read-only."
                } else {
                    &reason
                },
            );
            false
        }
        Err(_) => {
            NSBeep();
            false
        }
    }
}

/// True when a cursor or selection is on a line that holds a protected section.
/// Those lines are framing around locked data, so they cannot be typed into.
fn on_locked_line(doc: &Document) -> bool {
    let locked = doc.buffer().locked();
    if locked.is_empty() {
        return false;
    }
    let locked_lines: Vec<usize> = locked.iter().map(|&a| doc.line_col(a).0).collect();
    doc.selections().ranges().iter().any(|s| {
        let (first, _) = doc.line_col(s.min());
        let (last, _) = doc.line_col(s.max());
        locked_lines.iter().any(|&l| first <= l && l <= last)
    })
}

/// Call after the text or selection changed.
pub fn after_change(tab: &Rc<Tab>, text_changed: bool) {
    let (line, column, dirty) = {
        let mut doc = tab.doc.borrow_mut();
        // Plain typing only changes the lines the cursors are on. Anything
        // else (pastes with line breaks, undo, Replace All, line commands)
        // can lengthen any line, so measure the whole document.
        let simple_typing = {
            let edits: Vec<_> = doc
                .doc
                .pending_changes()
                .iter()
                .flat_map(|b| &b.0)
                .collect();
            edits.len() <= doc.doc.selections().ranges().len()
                && edits.iter().all(|e| !e.text.contains(['\n', '\r']))
        };
        if text_changed && !simple_typing {
            doc.max_columns = crate::doc_model::longest_rope_line(doc.doc.buffer().rope());
        } else if text_changed {
            let carets: Vec<usize> = doc
                .doc
                .selections()
                .ranges()
                .iter()
                .map(|s| s.head)
                .collect();
            for head in carets {
                let (line, _) = doc.doc.line_col(head);
                let len = doc.doc.line_text_end(line) - doc.doc.buffer().rope().line_to_char(line);
                doc.max_columns = doc.max_columns.max(len);
            }
        }
        let head = doc.doc.selections().primary().head;
        let (line, char_column) = doc.doc.line_col(head);
        let column = display_line(&doc, line).column_of(char_column);
        (line, column, doc.doc.is_dirty())
    };
    if text_changed {
        update_frames(tab);
        tab.window.setDocumentEdited(dirty);
        *tab.notice.borrow_mut() = None;
        crate::highlight::note_change(tab);
        crate::session_driver::note_change(tab);
    }
    let unfolded = tab.folds.borrow().is_hidden(line) && tab.folds.borrow_mut().unfold_at(line);
    if unfolded {
        update_frames(tab);
    }
    scroll_to_caret(tab, line, column);
    tab.view.setNeedsDisplay(true);
    crate::status_bar::refresh(tab);
}

fn scroll_to_caret(tab: &Rc<Tab>, line: usize, column: usize) {
    let rect = crate::text_view::rect_for(tab, line, column);
    let visible = tab.view.visibleRect();
    let inside = rect.origin.y >= visible.origin.y
        && rect.origin.y + rect.size.height <= visible.origin.y + visible.size.height
        && rect.origin.x >= visible.origin.x
        && rect.origin.x <= visible.origin.x + visible.size.width;
    if !inside {
        let mut padded = rect;
        padded.origin.x = (padded.origin.x - 40.0).max(0.0);
        padded.size.width += 80.0;
        tab.view.scrollRectToVisible(padded);
    }
}

pub fn move_cursors(tab: &Rc<Tab>, motion: Motion, extend: bool) {
    tab.doc.borrow_mut().doc.move_cursors(motion, extend);
    after_change(tab, false);
}

pub fn page_lines(tab: &Tab) -> usize {
    (tab.scroll.contentSize().height / metrics().line_height)
        .floor()
        .max(1.0) as usize
}

/// Maps a key binding command to an editing action. Returns false if unknown.
pub fn command(tab: &Rc<Tab>, name: &str) -> bool {
    let page = page_lines(tab);
    let (motion, extend) = match name {
        "moveLeft:" => (Motion::Left, false),
        "moveRight:" => (Motion::Right, false),
        "moveUp:" => (Motion::Up, false),
        "moveDown:" => (Motion::Down, false),
        "moveWordLeft:" => (Motion::WordLeft, false),
        "moveWordRight:" => (Motion::WordRight, false),
        "moveToBeginningOfLine:" | "moveToLeftEndOfLine:" => (Motion::LineStart, false),
        "moveToEndOfLine:" | "moveToRightEndOfLine:" => (Motion::LineEnd, false),
        "moveToBeginningOfDocument:" => (Motion::DocStart, false),
        "moveToEndOfDocument:" => (Motion::DocEnd, false),
        "pageUp:" | "scrollPageUp:" => (Motion::PageUp(page), false),
        "pageDown:" | "scrollPageDown:" => (Motion::PageDown(page), false),
        "moveLeftAndModifySelection:" => (Motion::Left, true),
        "moveRightAndModifySelection:" => (Motion::Right, true),
        "moveUpAndModifySelection:" => (Motion::Up, true),
        "moveDownAndModifySelection:" => (Motion::Down, true),
        "moveWordLeftAndModifySelection:" => (Motion::WordLeft, true),
        "moveWordRightAndModifySelection:" => (Motion::WordRight, true),
        "moveToBeginningOfLineAndModifySelection:" | "moveToLeftEndOfLineAndModifySelection:" => {
            (Motion::LineStart, true)
        }
        "moveToEndOfLineAndModifySelection:" | "moveToRightEndOfLineAndModifySelection:" => {
            (Motion::LineEnd, true)
        }
        "moveToBeginningOfDocumentAndModifySelection:" => (Motion::DocStart, true),
        "moveToEndOfDocumentAndModifySelection:" => (Motion::DocEnd, true),
        "pageUpAndModifySelection:" => (Motion::PageUp(page), true),
        "pageDownAndModifySelection:" => (Motion::PageDown(page), true),
        "deleteBackward:" | "deleteBackwardByDecomposingPreviousCharacter:" => {
            edit(tab, Document::backspace);
            return true;
        }
        "deleteForward:" => {
            edit(tab, Document::delete_forward);
            return true;
        }
        "deleteWordBackward:" => {
            tab.doc
                .borrow_mut()
                .doc
                .move_cursors(Motion::WordLeft, true);
            edit(tab, Document::backspace);
            return true;
        }
        "deleteToBeginningOfLine:" => {
            tab.doc
                .borrow_mut()
                .doc
                .move_cursors(Motion::LineStart, true);
            edit(tab, Document::backspace);
            return true;
        }
        "insertNewline:" | "insertLineBreak:" | "insertNewlineIgnoringFieldEditor:" => {
            edit(tab, Document::insert_newline);
            return true;
        }
        "insertTab:" => {
            edit(tab, |d| d.insert_text("\t"));
            return true;
        }
        "insertBacktab:" => {
            edit(tab, Document::outdent);
            return true;
        }
        "selectAll:" => {
            select_all(tab);
            return true;
        }
        "cancelOperation:" => {
            let primary = tab.doc.borrow().doc.selections().primary().head;
            tab.doc
                .borrow_mut()
                .doc
                .set_selections(SelectionSet::single(primary));
            crate::find_panel::hide(tab);
            after_change(tab, false);
            return true;
        }
        "scrollToBeginningOfDocument:" => {
            scroll_to(tab, 0, 0);
            return true;
        }
        "scrollToEndOfDocument:" => {
            let last = tab.doc.borrow().doc.buffer().rope().len_lines();
            scroll_to(tab, last, 0);
            return true;
        }
        _ => return false,
    };
    move_cursors(tab, motion, extend);
    true
}

pub fn select_all(tab: &Rc<Tab>) {
    let len = tab.doc.borrow().doc.buffer().rope().len_chars();
    tab.doc
        .borrow_mut()
        .doc
        .set_selections(SelectionSet::from_ranges(
            vec![Selection {
                anchor: 0,
                head: len,
            }],
            0,
        ));
    after_change(tab, false);
}

/// Character offset nearest to a point in the text view.
pub fn offset_at_point(tab: &Tab, point: NSPoint) -> (usize, usize, usize) {
    let m = metrics();
    let row = (point.y / m.line_height).floor().max(0.0) as usize;
    let rows = crate::rows::row_count(tab).max(1);
    let (line, piece) = crate::rows::locate(tab, row.min(rows - 1));
    let doc = tab.doc.borrow();
    let rope = doc.doc.buffer().rope();
    let lines = rope.len_lines();
    let line = line.min(lines.saturating_sub(1));
    let display = display_line(&doc, line);
    let shown = crate::rows::piece_columns(tab, piece, display.width());
    let mut column = shown.start
        + ((point.x - text_left(lines, &m)) / m.advance)
            .round()
            .max(0.0) as usize;
    if doc.word_wrap {
        column = column.min(shown.end);
    }
    let char_index = display.char_at_column(column);
    (rope.line_to_char(line) + char_index, line, column)
}

fn word_bounds(doc: &Document, offset: usize) -> (usize, usize) {
    let rope = doc.buffer().rope();
    let is_word = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
    let len = rope.len_chars();
    let (mut start, mut end) = (offset.min(len), offset.min(len));
    while start > 0 && is_word(rope.char(start - 1)) {
        start -= 1;
    }
    while end < len && is_word(rope.char(end)) {
        end += 1;
    }
    (start, end)
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Modifiers {
    pub command: bool,
    pub option: bool,
    pub shift: bool,
}

#[derive(Clone, Copy, Debug)]
pub enum DragAnchor {
    Text(usize),
    Column(usize, usize),
}

/// Handles a click at a document position. Returns how a following drag behaves.
pub fn click(
    tab: &Rc<Tab>,
    offset: usize,
    line: usize,
    column: usize,
    mods: Modifiers,
    count: usize,
) -> DragAnchor {
    let anchor = {
        let mut doc = tab.doc.borrow_mut();
        let d = &mut doc.doc;
        if mods.option {
            d.add_column_selection((line, column), (line, column));
            DragAnchor::Column(line, column)
        } else if mods.command {
            let mut set = d.selections().clone();
            set.add(Selection::caret(offset));
            d.set_selections(set);
            DragAnchor::Text(offset)
        } else if mods.shift {
            let anchor = d.selections().primary().anchor;
            d.set_selections(SelectionSet::from_ranges(
                vec![Selection {
                    anchor,
                    head: offset,
                }],
                0,
            ));
            DragAnchor::Text(anchor)
        } else if count == 2 {
            let (start, end) = word_bounds(d, offset);
            d.set_selections(SelectionSet::from_ranges(
                vec![Selection {
                    anchor: start,
                    head: end,
                }],
                0,
            ));
            DragAnchor::Text(start)
        } else if count >= 3 {
            let rope = d.buffer().rope();
            let start = rope.line_to_char(line);
            let end = if line + 1 < rope.len_lines() {
                rope.line_to_char(line + 1)
            } else {
                rope.len_chars()
            };
            d.set_selections(SelectionSet::from_ranges(
                vec![Selection {
                    anchor: start,
                    head: end,
                }],
                0,
            ));
            DragAnchor::Text(start)
        } else {
            d.set_selections(SelectionSet::single(offset));
            DragAnchor::Text(offset)
        }
    };
    after_change(tab, false);
    anchor
}

pub fn drag(tab: &Rc<Tab>, anchor: DragAnchor, offset: usize, line: usize, column: usize) {
    {
        let mut doc = tab.doc.borrow_mut();
        match anchor {
            DragAnchor::Column(from_line, from_column) => doc
                .doc
                .add_column_selection((from_line, from_column), (line, column)),
            DragAnchor::Text(start) => {
                let mut set = doc.doc.selections().clone();
                let primary = Selection {
                    anchor: start,
                    head: offset,
                };
                let others: Vec<Selection> = set
                    .ranges()
                    .iter()
                    .copied()
                    .filter(|s| *s != set.primary())
                    .collect();
                let mut all = others;
                all.push(primary);
                let index = all.len() - 1;
                set = SelectionSet::from_ranges(all, index);
                doc.doc.set_selections(set);
            }
        }
    }
    after_change(tab, false);
}

/// The system clipboard, or a private one during automated tests so tests
/// running at the same time never touch each other's or the user's clipboard.
fn pasteboard() -> objc2::rc::Retained<NSPasteboard> {
    if crate::app::args().test_script.is_some() {
        let name = NSString::from_str(&format!(
            "{}.test.{}",
            crate::config::APP.bundle_id,
            std::process::id()
        ));
        NSPasteboard::pasteboardWithName(&name)
    } else {
        NSPasteboard::generalPasteboard()
    }
}

pub fn clipboard_text() -> Option<String> {
    let pasteboard = pasteboard();
    unsafe { pasteboard.stringForType(NSPasteboardTypeString) }.map(|s| s.to_string())
}

pub fn set_clipboard_text(text: &str) {
    let pasteboard = pasteboard();
    pasteboard.clearContents();
    unsafe { pasteboard.setString_forType(&NSString::from_str(text), NSPasteboardTypeString) };
}

pub fn copy(tab: &Rc<Tab>) {
    let text = tab.doc.borrow().doc.selected_text();
    if !text.is_empty() {
        set_clipboard_text(&text);
    }
}

pub fn cut(tab: &Rc<Tab>) {
    copy(tab);
    let has_selection = tab
        .doc
        .borrow()
        .doc
        .selections()
        .ranges()
        .iter()
        .any(|s| !s.is_caret());
    if has_selection {
        edit(tab, Document::backspace);
    }
}

pub fn paste(tab: &Rc<Tab>) {
    if let Some(text) = clipboard_text() {
        edit(tab, |d| d.paste_text(&text));
    }
}

pub fn undo(tab: &Rc<Tab>) {
    let changed = tab.doc.borrow_mut().doc.undo();
    if changed {
        after_change(tab, true)
    } else {
        NSBeep()
    }
}

pub fn redo(tab: &Rc<Tab>) {
    let changed = tab.doc.borrow_mut().doc.redo();
    if changed {
        after_change(tab, true)
    } else {
        NSBeep()
    }
}

/// A click in the gutter: the fold marker folds or unfolds, elsewhere toggles a bookmark.
pub fn gutter_click(tab: &Rc<Tab>, line: usize, on_marker: bool) {
    if on_marker
        && let Some(block) = crate::folds::block_at(tab, line).filter(|b| b.start_line == line)
    {
        let removed = tab.folds.borrow_mut().unfold_at(line);
        if !removed {
            tab.folds.borrow_mut().fold(block);
        }
        update_frames(tab);
        return;
    }
    crate::bookmark_actions::toggle_line(tab, line);
}
