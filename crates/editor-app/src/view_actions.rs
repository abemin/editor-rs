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
    let mtm = objc2::MainThreadMarker::new().expect("main thread");
    match action {
        MenuAction::Indent => {
            crate::input::edit(tab, editor_core::Document::indent);
            return;
        }
        MenuAction::Outdent => {
            crate::input::edit(tab, editor_core::Document::outdent);
            return;
        }
        MenuAction::GoToLine => {
            go_to_line(mtm, tab);
            return;
        }
        MenuAction::BiggerText => return crate::settings::change_font_size(mtm, 1.0),
        MenuAction::SmallerText => return crate::settings::change_font_size(mtm, -1.0),
        MenuAction::ActualSize => {
            return crate::settings::set_font_size(mtm, crate::text_view::DEFAULT_FONT_SIZE);
        }
        MenuAction::Preferences => return crate::settings::show(mtm),
        MenuAction::About => {
            objc2_app_kit::NSApplication::sharedApplication(mtm).orderFrontStandardAboutPanel(None);
            return;
        }
        MenuAction::WordWrap => {
            toggle_wrap(tab);
            return;
        }
        MenuAction::FoldBlock if tab.doc.borrow().word_wrap => return,
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
    let Ok((text, encoding)) = editor_formats::reopen_as(&bytes, name) else {
        crate::prompts::tell(
            mtm,
            &format!("The file can't be shown as {name} without changing it."),
            "Some bytes would not be saved back exactly as they are, so the file was left as it was.",
        );
        return;
    };
    let mut fresh = crate::doc_model::EditorDoc::untitled();
    fresh.doc = editor_core::Document::new(&text, editor_formats::dominant_eol(&text));
    fresh.encoding = encoding;
    fresh.max_columns = crate::doc_model::longest_line(&text);
    {
        let old = tab.doc.borrow();
        fresh.path = old.path.clone();
        fresh.source = old.source.clone();
        fresh.language = old.language;
        fresh.word_wrap = old.word_wrap;
    }
    crate::document_controller::replace_document(tab, fresh);
    update_frames(tab);
    crate::status_bar::refresh(tab);
}

pub fn toggle_wrap(tab: &Rc<Tab>) {
    let lines = tab.doc.borrow().doc.buffer().rope().len_lines();
    let enabling = !tab.doc.borrow().word_wrap;
    if enabling && lines > crate::rows::WRAP_LINE_LIMIT {
        crate::input::notice(
            tab,
            "Word wrap is not available for documents with more than 500,000 lines.",
        );
        return;
    }
    tab.doc.borrow_mut().word_wrap = enabling;
    tab.folds.borrow_mut().clear();
    crate::rows::invalidate(tab);
    update_frames(tab);
    crate::input::after_change(tab, false);
    crate::session_driver::note_structure();
}

fn go_to_line(mtm: objc2::MainThreadMarker, tab: &Rc<Tab>) {
    let lines = tab.doc.borrow().doc.buffer().rope().len_lines();
    let Some(answer) = crate::prompts::ask_text(
        mtm,
        "Go to Line",
        &format!("Enter a line number from 1 to {lines}."),
    ) else {
        return;
    };
    match answer.trim().parse::<usize>() {
        Ok(number) if (1..=lines).contains(&number) => {
            let offset = tab.doc.borrow().doc.offset_at(number - 1, 0);
            tab.doc
                .borrow_mut()
                .doc
                .set_selections(editor_core::SelectionSet::single(offset));
            crate::input::after_change(tab, false);
            crate::text_view::scroll_to(tab, number - 1, 0);
        }
        _ => crate::input::notice(
            tab,
            &format!(
                "“{}” is not a line number between 1 and {lines}.",
                answer.trim()
            ),
        ),
    }
}
