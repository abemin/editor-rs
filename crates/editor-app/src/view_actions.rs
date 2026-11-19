//! View and editing commands from the menus.

use crate::app::Tab;
use crate::menus::MenuAction;
use crate::text_view::update_frames;
use std::rc::Rc;

fn caret_line(tab: &Tab) -> usize {
    let doc = tab.doc.borrow();
    doc.doc.line_col(doc.doc.selections().primary().head).0
}

pub fn perform(tab: &Rc<Tab>, action: MenuAction) {
    match action {
        MenuAction::FoldBlock => {
            if let Some(block) = crate::folds::block_at(tab, caret_line(tab)) {
                tab.folds.borrow_mut().fold(block);
                let offset = tab.doc.borrow().doc.offset_at(block.start_line, 0);
                tab.doc
                    .borrow_mut()
                    .doc
                    .set_selections(editor_core::SelectionSet::single(offset));
            }
        }
        MenuAction::UnfoldBlock => {
            tab.folds.borrow_mut().unfold_at(caret_line(tab));
        }
        MenuAction::FoldAll => {
            if let Some(structure) = crate::highlight::structure(tab) {
                let mut folds = tab.folds.borrow_mut();
                for block in structure.folds.iter().filter(|f| f.end_line > f.start_line) {
                    folds.fold(*block);
                }
            }
            let line = caret_line(tab);
            if tab.folds.borrow().is_hidden(line) {
                let start = tab
                    .folds
                    .borrow()
                    .line_of_row(tab.folds.borrow().row_of_line(line));
                let offset = tab.doc.borrow().doc.offset_at(start, 0);
                tab.doc
                    .borrow_mut()
                    .doc
                    .set_selections(editor_core::SelectionSet::single(offset));
            }
        }
        MenuAction::UnfoldAll => tab.folds.borrow_mut().clear(),
        _ => return,
    }
    update_frames(tab);
    crate::input::after_change(tab, false);
}

/// Chooses the coloring language for the front document.
pub fn set_language(tab: &Rc<Tab>, name: &str) {
    if let Some(language) = editor_syntax::Language::from_name(name) {
        tab.doc.borrow_mut().language = language;
        tab.folds.borrow_mut().clear();
        crate::highlight::reset(tab);
        update_frames(tab);
        crate::status_bar::refresh(tab);
        crate::session_driver::note_structure();
    }
}

/// Encodings offered by Reopen with Encoding.
pub const REOPEN_ENCODINGS: &[&str] = &[
    "UTF-8",
    "Windows-1252",
    "ISO-8859-1",
    "ISO-8859-2",
    "ISO-8859-15",
    "Macintosh",
    "UTF-16 LE",
    "UTF-16 BE",
];

/// Reads the file again with a chosen encoding, refusing one that would change it.
pub fn reopen_with_encoding(mtm: objc2::MainThreadMarker, tab: &Rc<Tab>, name: &str) {
    let Some(path) = tab.doc.borrow().path.clone() else {
        return;
    };
    if tab.doc.borrow().container.is_some() {
        crate::prompts::tell(
            mtm,
            "FortiWeb backups always open as UTF-8.",
            "Their protected sections are kept exactly as they are.",
        );
        return;
    }
    if tab.doc.borrow().doc.is_dirty()
        && crate::prompts::ask(
            mtm,
            "Reopen with a different encoding?",
            "Your unsaved changes will be lost.",
            &["Reopen", "Cancel"],
            true,
        ) != 0
    {
        return;
    }
    let Ok(bytes) = std::fs::read(&path) else {
        return;
    };
    let label = match name {
        "UTF-16 LE" => "utf-16le",
        "UTF-16 BE" => "utf-16be",
        other => other,
    };
    let Some(encoding) = encoding_rs::Encoding::for_label(label.as_bytes()) else {
        return;
    };
    let text = match name {
        "UTF-8" => std::str::from_utf8(&bytes)
            .map(str::to_string)
            .map_err(|_| ()),
        "UTF-16 LE" | "UTF-16 BE" => encoding
            .decode_without_bom_handling_and_without_replacement(bytes.get(2..).unwrap_or_default())
            .map(|t| t.into_owned())
            .ok_or(()),
        _ => editor_formats::reopen_with(&bytes, encoding).map_err(|_| ()),
    };
    let Ok(text) = text else {
        crate::prompts::tell(
            mtm,
            &format!("The file can't be shown as {name} without changing it."),
            "Some bytes in the file have no meaning in that encoding, so it was left as it was.",
        );
        return;
    };
    let new_encoding = match name {
        "UTF-8" => editor_formats::TextEncoding::Utf8 { bom: false },
        "UTF-16 LE" => editor_formats::TextEncoding::Utf16Le,
        "UTF-16 BE" => editor_formats::TextEncoding::Utf16Be,
        _ => editor_formats::TextEncoding::SingleByte(encoding),
    };
    {
        let mut doc = tab.doc.borrow_mut();
        let eol = editor_formats::dominant_eol(&text);
        doc.doc = editor_core::Document::new(&text, eol);
        doc.encoding = new_encoding;
        doc.max_columns = crate::doc_model::longest_line(&text);
    }
    tab.folds.borrow_mut().clear();
    crate::highlight::reset(tab);
    update_frames(tab);
    crate::status_bar::refresh(tab);
}
