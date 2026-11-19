//! Bookmark commands from the menu and the gutter.

use crate::app::Tab;
use crate::input::{edit, notice, set_clipboard_text};
use crate::menus::MenuAction;
use objc2::MainThreadMarker;
use std::rc::Rc;

pub fn toggle_line(tab: &Rc<Tab>, line: usize) {
    tab.doc.borrow_mut().doc.toggle_bookmark(line);
    bookmarks_changed(tab);
}

fn bookmarks_changed(tab: &Rc<Tab>) {
    tab.view.setNeedsDisplay(true);
    crate::session_driver::note_change(tab);
    crate::status_bar::refresh(tab);
}

fn jump(tab: &Rc<Tab>, line: Option<usize>) {
    let Some(line) = line else {
        notice(tab, "There are no bookmarks.");
        return;
    };
    let offset = tab.doc.borrow().doc.offset_at(line, 0);
    tab.doc
        .borrow_mut()
        .doc
        .set_selections(editor_core::SelectionSet::single(offset));
    crate::input::after_change(tab, false);
}

pub fn perform(_mtm: MainThreadMarker, tab: &Rc<Tab>, action: MenuAction) {
    let current_line = {
        let doc = tab.doc.borrow();
        doc.doc.line_col(doc.doc.selections().primary().head).0
    };
    match action {
        MenuAction::ToggleBookmark => toggle_line(tab, current_line),
        MenuAction::NextBookmark => {
            let line = tab.doc.borrow().doc.next_bookmark(current_line);
            jump(tab, line);
        }
        MenuAction::PreviousBookmark => {
            let line = tab.doc.borrow().doc.previous_bookmark(current_line);
            jump(tab, line);
        }
        MenuAction::ClearBookmarks => {
            tab.doc.borrow_mut().doc.clear_bookmarks();
            bookmarks_changed(tab);
        }
        MenuAction::InverseBookmarks => {
            tab.doc.borrow_mut().doc.inverse_bookmarks();
            bookmarks_changed(tab);
        }
        MenuAction::CopyBookmarkedLines => {
            let text = tab.doc.borrow().doc.copy_bookmarked_lines();
            set_clipboard_text(&text);
        }
        MenuAction::CutBookmarkedLines => {
            let text = tab.doc.borrow().doc.copy_bookmarked_lines();
            if edit(tab, |d| d.remove_bookmarked_lines()) {
                set_clipboard_text(&text);
            }
        }
        MenuAction::PasteToBookmarkedLines => {
            if let Some(text) = crate::input::clipboard_text() {
                edit(tab, |d| d.paste_to_bookmarked_lines(&text));
            }
        }
        MenuAction::RemoveBookmarkedLines => {
            edit(tab, |d| d.remove_bookmarked_lines());
        }
        MenuAction::RemoveUnbookmarkedLines => {
            edit(tab, |d| d.remove_unbookmarked_lines());
        }
        _ => crate::view_actions::perform(tab, action),
    }
}
