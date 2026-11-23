//! What the app shows when it starts: the restored session, or a new tab.

use crate::app::Tab;
use crate::doc_model::EditorDoc;
use crate::prompts::ask;
use editor_core::{Edit, EditBatch, SelectionSet};
use editor_formats::fortiweb::PLACEHOLDER;
use editor_formats::session::{RestoredTab, SessionError};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSRunningApplication, NSWorkspace, NSWorkspaceOpenConfiguration};
use objc2_foundation::{NSArray, NSString, NSURL};
use std::path::Path;
use std::rc::Rc;
use std::time::UNIX_EPOCH;

pub fn restore_or_start(mtm: MainThreadMarker) {
    let root = crate::app::args()
        .session_dir
        .unwrap_or_else(crate::session_driver::default_root);
    let restored = match crate::session_driver::start(&root) {
        Ok(restored) => restored,
        Err(SessionError::Busy) => {
            forward_to_running_instance(mtm);
            Vec::new()
        }
        Err(SessionError::Io(error)) => {
            crate::session_driver::disable(&error.to_string());
            Vec::new()
        }
    };
    let mut active = None;
    for entry in restored {
        let wanted = entry.record.id.clone();
        if let Some(tab) = restore_tab(mtm, entry) {
            active = Some((wanted, tab));
        }
    }
    match active {
        Some((_, tab)) => tab.window.makeKeyAndOrderFront(None),
        None => {
            crate::window::open_tab(mtm, EditorDoc::untitled());
        }
    }
    crate::session_driver::note_structure();
}

/// Another copy of the app owns the session: hand it the files and quit.
fn forward_to_running_instance(mtm: MainThreadMarker) {
    let me = std::process::id() as i32;
    let bundle = NSString::from_str(crate::config::APP.bundle_id);
    let others = NSRunningApplication::runningApplicationsWithBundleIdentifier(&bundle);
    let other = others.iter().find(|app| app.processIdentifier() != me);
    if let Some(other) = other {
        let urls: Vec<_> = crate::app::args()
            .files
            .iter()
            .map(|p| NSURL::fileURLWithPath(&NSString::from_str(&p.to_string_lossy())))
            .collect();
        if let (false, Some(app_url)) = (urls.is_empty(), other.bundleURL()) {
            let config = NSWorkspaceOpenConfiguration::configuration();
            NSWorkspace::sharedWorkspace()
                .openURLs_withApplicationAtURL_configuration_completionHandler(
                    &NSArray::from_retained_slice(&urls),
                    &app_url,
                    &config,
                    None,
                );
        }
        #[allow(deprecated)]
        other.activateWithOptions(objc2_app_kit::NSApplicationActivationOptions::empty());
        let _ = mtm;
        std::process::exit(0);
    }
    crate::session_driver::disable("another copy of the app is using the session");
}

fn millis(path: &Path) -> Option<(u64, i64)> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis() as i64;
    Some((metadata.len(), modified))
}

/// Turns `base` into `text` with the smallest edits that keep protected sections.
fn apply_restored_text(doc: &mut EditorDoc, text: &str) -> bool {
    let current = doc.doc.text();
    if current == text {
        return true;
    }
    // Only backups have protected sections; elsewhere U+FFFC is ordinary text.
    let split = |t: &str| -> Vec<String> {
        if doc.container.is_some() {
            t.split(PLACEHOLDER).map(str::to_string).collect()
        } else {
            vec![t.to_string()]
        }
    };
    let old_parts = split(&current);
    let new_parts = split(text);
    if old_parts.len() != new_parts.len() {
        return false;
    }
    let mut edits = Vec::new();
    let mut offset = 0usize;
    for (old, new) in old_parts.iter().zip(&new_parts) {
        let old_chars: Vec<char> = old.chars().collect();
        let new_chars: Vec<char> = new.chars().collect();
        let prefix = old_chars
            .iter()
            .zip(&new_chars)
            .take_while(|(a, b)| a == b)
            .count();
        let max_suffix = old_chars.len().min(new_chars.len()) - prefix;
        let suffix = old_chars
            .iter()
            .rev()
            .zip(new_chars.iter().rev())
            .take(max_suffix)
            .take_while(|(a, b)| a == b)
            .count();
        if prefix + suffix < old_chars.len() || prefix + suffix < new_chars.len() {
            let replacement: String = new_chars[prefix..new_chars.len() - suffix].iter().collect();
            edits.push(Edit {
                range: offset + prefix..offset + old_chars.len() - suffix,
                text: replacement,
            });
        }
        offset += old_chars.len() + 1;
    }
    doc.doc.apply_batch(EditBatch(edits), "Restore").is_ok()
}

fn load_doc(path: &Path, display: &Path) -> Option<EditorDoc> {
    let file = editor_formats::load_file(path).ok()?;
    let mut doc = EditorDoc::from_loaded(file, display);
    if path != display {
        doc.source = None;
    }
    Some(doc)
}

fn restore_tab(mtm: MainThreadMarker, entry: RestoredTab) -> Option<Rc<Tab>> {
    let record = entry.record;
    let clone = crate::session_driver::root()
        .map(|r| r.join(format!("{}.protected", record.id)))
        .filter(|p| p.exists());
    let Some(text) = entry.text else {
        let path = record.path.as_ref()?;
        let tab = crate::document_controller::open_path(mtm, path).ok()?;
        if record.dirty {
            *tab.notice.borrow_mut() = Some(
                "Unsaved changes for this file could not be recovered from auto-save. The file on disk is unchanged."
                    .to_string(),
            );
            crate::status_bar::refresh(&tab);
        }
        return Some(tab);
    };
    let mut notice = None;
    let mut doc = match &record.path {
        None => EditorDoc::untitled(),
        Some(path) => match millis(path) {
            None => {
                notice = Some("The original file is missing. Your unsaved text was kept; Save asks for a new location.".to_string());
                let mut doc = match &clone {
                    Some(clone) => load_doc(clone, path)?,
                    None => EditorDoc::untitled(),
                };
                doc.path = None;
                doc.name_hint = Some(record.title.clone());
                doc
            }
            Some((len, modified))
                if Some(len) == record.source_len
                    && Some(modified) == record.source_modified_ms =>
            {
                load_doc(path, path)?
            }
            Some(_) => {
                let choice = ask(
                    mtm,
                    &format!(
                        "“{}” changed on disk after your unsaved edits were kept.",
                        record.title
                    ),
                    "Keep your version, load the version on disk, or open both to compare.",
                    &["Keep Mine", "Load Disk Version", "Compare"],
                    true,
                );
                if choice == 1 {
                    return crate::document_controller::open_path(mtm, path).ok();
                }
                if choice == 2 {
                    let _ = crate::document_controller::open_path(mtm, path);
                }
                let mut doc = match &clone {
                    Some(clone) => load_doc(clone, path)?,
                    None => load_doc(path, path)?,
                };
                doc.source = editor_formats::load_file(path).ok().and_then(|f| f.source);
                doc
            }
        },
    };
    if !apply_restored_text(&mut doc, &text) {
        notice = Some("Some unsaved changes could not be restored.".to_string());
    }
    if entry.recovered_partial {
        notice = Some(
            "The last moments of typing before the app stopped could not be recovered.".to_string(),
        );
    }
    if let Some(language) = record
        .language
        .as_deref()
        .and_then(editor_syntax::Language::from_name)
    {
        doc.language = language;
    }
    doc.word_wrap = record.word_wrap;
    let len = doc.doc.buffer().rope().len_chars();
    doc.doc
        .set_selections(SelectionSet::single(record.caret.min(len)));
    doc.doc.set_bookmarked_lines(&record.bookmarks);
    doc.doc.take_change_log();
    doc.max_columns = crate::doc_model::longest_rope_line(doc.doc.buffer().rope());
    let tab = crate::window::open_tab_with_id(mtm, doc, Some(record.id.clone()));
    tab.journaled.set(false);
    tab.cloned
        .set(record.has_protected_clone && clone.is_some());
    tab.window
        .setDocumentEdited(tab.doc.borrow().doc.is_dirty());
    *tab.notice.borrow_mut() = notice;
    crate::status_bar::refresh(&tab);
    crate::text_view::scroll_to(&tab, record.scroll_line, 0);
    crate::highlight::reset(&tab);
    Some(tab)
}
