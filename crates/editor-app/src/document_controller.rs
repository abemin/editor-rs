//! Opening, saving, reverting and closing documents.

use crate::app::{Tab, tabs, with_app};
use crate::doc_model::EditorDoc;
use crate::prompts::{ask, tell};
use editor_formats::save::Chunk;
use editor_formats::{
    SaveError, SourceIdentity, TextEncoding, atomic_write, encode_text, load_file,
};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSModalResponseOK, NSOpenPanel, NSSavePanel, NSWindow};
use objc2_foundation::{NSString, NSURL};
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;

const RECENT_KEY: &str = "RecentFiles";
const RECENT_LIMIT: usize = 15;

fn identity(path: &Path) -> Option<SourceIdentity> {
    let metadata = std::fs::metadata(path).ok()?;
    Some(SourceIdentity {
        path: path.to_path_buf(),
        len: metadata.len(),
        modified: metadata.modified().ok()?,
    })
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn is_pristine(tab: &Tab) -> bool {
    let doc = tab.doc.borrow();
    doc.path.is_none() && !doc.doc.is_dirty() && doc.doc.buffer().rope().len_chars() == 0
}

/// Opens a file, or focuses it when it is already open.
pub fn open_path(mtm: MainThreadMarker, path: &Path) -> Result<Rc<Tab>, String> {
    if let Some(existing) = tabs().into_iter().find(|t| {
        t.doc
            .borrow()
            .path
            .as_deref()
            .is_some_and(|p| same_file(p, path))
    }) {
        existing.window.makeKeyAndOrderFront(None);
        return Ok(existing);
    }
    let started = Instant::now();
    let file =
        load_file(path).map_err(|e| format!("“{}” could not be opened: {e}", path.display()))?;
    let pristine = crate::window::current_tab(mtm).filter(|t| is_pristine(t));
    let doc = EditorDoc::from_loaded(file, path);
    let tab = crate::window::open_tab(mtm, doc);
    tab.opened_at.set(Some(started));
    if let Some(old) = pristine {
        close_now(&old);
    }
    add_recent(path);
    crate::highlight::note_change(&tab);
    crate::session_driver::note_structure();
    Ok(tab)
}

pub fn open_dialog(mtm: MainThreadMarker) {
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setAllowsMultipleSelection(true);
    panel.setCanChooseDirectories(false);
    if panel.runModal() == NSModalResponseOK {
        for url in panel.URLs() {
            if let Some(path) = url.path() {
                open_or_tell(mtm, Path::new(&path.to_string()));
            }
        }
    }
}

pub fn open_or_tell(mtm: MainThreadMarker, path: &Path) {
    if let Err(message) = open_path(mtm, path) {
        tell(mtm, "The file could not be opened.", &message);
    }
}

/// Bytes to write for the current text, asking before converting to UTF-8.
fn encoded(mtm: MainThreadMarker, tab: &Tab) -> Result<Option<Vec<u8>>, String> {
    let (text, encoding, title) = {
        let doc = tab.doc.borrow();
        (doc.doc.text(), doc.encoding, doc.title())
    };
    match encode_text(&text, &encoding) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(problem) => {
            let name = crate::status_bar::encoding_name(&encoding);
            let choice = ask(
                mtm,
                &format!(
                    "“{title}” uses {name}, which cannot store “{}”.",
                    problem.ch
                ),
                "Convert the file to UTF-8, or cancel and remove the character.",
                &["Convert to UTF-8", "Cancel"],
                true,
            );
            if choice != 0 {
                return Ok(None);
            }
            tab.doc.borrow_mut().encoding = TextEncoding::Utf8 { bom: false };
            Ok(Some(text.into_bytes()))
        }
    }
}

/// Saves to `path`. Returns false if the user cancelled or saving failed.
pub fn save_to(mtm: MainThreadMarker, tab: &Rc<Tab>, path: &Path) -> bool {
    if let Some(reason) = tab.doc.borrow().read_only.clone() {
        tell(mtm, "This document is read-only.", &reason);
        return false;
    }
    let result: Result<(), String> = if tab.doc.borrow().container.is_some() {
        let doc = tab.doc.borrow();
        let text = doc.doc.text();
        let container = doc.container.as_ref().expect("container");
        match editor_formats::fortiweb::chunks(&text, doc.doc.buffer().locked(), container) {
            Ok(chunks) => atomic_write(path, &chunks).map_err(|e| e.to_string()),
            Err(problem) => Err(format!(
                "{problem} The backup was not saved, so the file on disk is unchanged. The [file] markers, file_number and the separator lines must stay consistent."
            )),
        }
    } else {
        match encoded(mtm, tab) {
            Ok(Some(bytes)) => {
                atomic_write(path, &[Chunk::Bytes(Cow::Owned(bytes))]).map_err(|e| match e {
                    SaveError::ReadOnly => {
                        "The file is read-only or its folder cannot be written to.".to_string()
                    }
                    other => other.to_string(),
                })
            }
            Ok(None) => return false,
            Err(message) => Err(message),
        }
    };
    if let Err(message) = result {
        tell(mtm, "The document could not be saved.", &message);
        return false;
    }
    {
        let mut doc = tab.doc.borrow_mut();
        doc.doc.mark_saved();
        doc.path = Some(path.to_path_buf());
        doc.source = identity(path);
    }
    let title = tab.doc.borrow().title();
    tab.window.setTitle(&NSString::from_str(&title));
    tab.window.setDocumentEdited(false);
    add_recent(path);
    crate::status_bar::refresh(tab);
    crate::session_driver::saved(tab);
    true
}

pub fn save(mtm: MainThreadMarker, tab: &Rc<Tab>) -> bool {
    let path = tab.doc.borrow().path.clone();
    match path {
        Some(path) => save_to(mtm, tab, &path),
        None => save_as_dialog(mtm, tab),
    }
}

pub fn save_as_dialog(mtm: MainThreadMarker, tab: &Rc<Tab>) -> bool {
    let panel = NSSavePanel::savePanel(mtm);
    panel.setNameFieldStringValue(&NSString::from_str(&tab.doc.borrow().title()));
    if panel.runModal() != NSModalResponseOK {
        return false;
    }
    let Some(path) = panel.URL().and_then(|u| u.path()) else {
        return false;
    };
    save_to(mtm, tab, Path::new(&path.to_string()))
}

/// Replaces the text with the file on disk.
pub fn reload(mtm: MainThreadMarker, tab: &Rc<Tab>) {
    let Some(path) = tab.doc.borrow().path.clone() else {
        return;
    };
    match load_file(&path) {
        Ok(file) => {
            let caret = tab.doc.borrow().doc.selections().primary().head;
            let mut fresh = EditorDoc::from_loaded(file, &path);
            let len = fresh.doc.buffer().rope().len_chars();
            fresh
                .doc
                .set_selections(editor_core::SelectionSet::single(caret.min(len)));
            *tab.doc.borrow_mut() = fresh;
            tab.window.setDocumentEdited(false);
            crate::input::after_change(tab, true);
            tab.window.setDocumentEdited(false);
            crate::session_driver::saved(tab);
        }
        Err(e) => tell(mtm, "The file could not be reloaded.", &e.to_string()),
    }
}

pub fn revert(mtm: MainThreadMarker, tab: &Rc<Tab>) {
    let dirty = tab.doc.borrow().doc.is_dirty();
    if dirty
        && ask(
            mtm,
            "Revert to the saved version?",
            "Your unsaved changes will be lost.",
            &["Revert", "Cancel"],
            true,
        ) != 0
    {
        return;
    }
    reload(mtm, tab);
}

/// Detects changes made by other apps.
pub fn check_external_change(mtm: MainThreadMarker, tab: &Rc<Tab>) {
    let (path, known, dirty, title) = {
        let doc = tab.doc.borrow();
        (
            doc.path.clone(),
            doc.source.clone(),
            doc.doc.is_dirty(),
            doc.title(),
        )
    };
    let (Some(path), Some(known)) = (path, known) else {
        return;
    };
    let Some(now) = identity(&path) else { return };
    if now.len == known.len && now.modified == known.modified {
        return;
    }
    tab.doc.borrow_mut().source = Some(now);
    let info = if dirty {
        "You have unsaved changes. Reloading will replace them with the version on disk."
    } else {
        "Reload it to see the latest version."
    };
    if ask(
        mtm,
        &format!("“{title}” changed on disk."),
        info,
        &["Reload", "Keep Mine"],
        dirty,
    ) == 0
    {
        reload(mtm, tab);
    }
}

pub fn check_all_external_changes(mtm: MainThreadMarker) {
    if let Some(tab) = crate::window::current_tab(mtm) {
        check_external_change(mtm, &tab);
    }
}

pub enum CloseOutcome {
    Closed,
    Cancelled,
}

/// Asks about unsaved changes. True when the tab may close.
fn confirm_close(mtm: MainThreadMarker, tab: &Rc<Tab>) -> bool {
    let (dirty, title) = {
        let doc = tab.doc.borrow();
        (doc.doc.is_dirty(), doc.title())
    };
    if !dirty {
        return true;
    }
    match ask(
        mtm,
        &format!("Do you want to save the changes to “{title}”?"),
        "Your changes will be lost if you don't save them.",
        &["Save", "Don't Save", "Cancel"],
        true,
    ) {
        0 => save(mtm, tab),
        1 => true,
        _ => false,
    }
}

/// Removes a tab without asking.
pub fn close_now(tab: &Rc<Tab>) {
    crate::session_driver::discard(tab);
    with_app(|a| a.tabs.retain(|t| !Rc::ptr_eq(t, tab)));
    tab.window.close();
    crate::session_driver::note_structure();
}

/// Closing a tab on purpose: asks about unsaved changes.
pub fn close_tab(mtm: MainThreadMarker, tab: &Rc<Tab>) -> CloseOutcome {
    if !confirm_close(mtm, tab) {
        return CloseOutcome::Cancelled;
    }
    close_now(tab);
    CloseOutcome::Closed
}

/// The window close button. The last window closes without asking: its
/// tabs stay in the session and come back next time.
pub fn should_close_window(mtm: MainThreadMarker, tab: &Rc<Tab>) -> bool {
    if tabs().len() <= 1 {
        crate::session_driver::flush_now();
        return true;
    }
    if !confirm_close(mtm, tab) {
        return false;
    }
    crate::session_driver::discard(tab);
    true
}

pub fn forget_window(window: &NSWindow) {
    let quitting = with_app(|a| a.tabs.len() <= 1);
    if !quitting {
        with_app(|a| a.tabs.retain(|t| !std::ptr::eq(&*t.window, window)));
        crate::session_driver::note_structure();
    }
}

/// Closes every tab in the front window's group.
pub fn close_window(mtm: MainThreadMarker) {
    let Some(tab) = crate::window::current_tab(mtm) else {
        return;
    };
    let group: Vec<Rc<Tab>> = match tab.window.tabbedWindows() {
        Some(windows) => tabs()
            .into_iter()
            .filter(|t| windows.iter().any(|w| std::ptr::eq(&*w, &*t.window)))
            .collect(),
        None => vec![tab],
    };
    if group.len() == tabs().len() {
        NSApplication::sharedApplication(mtm).terminate(None);
        return;
    }
    for tab in group {
        if let CloseOutcome::Cancelled = close_tab(mtm, &tab) {
            return;
        }
    }
}

fn testing() -> bool {
    crate::app::args().test_script.is_some()
}

thread_local! {
    static TEST_RECENT: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

pub fn recent_files() -> Vec<PathBuf> {
    let list: Vec<String> = if testing() {
        TEST_RECENT.with(|r| r.borrow().clone())
    } else {
        let defaults = objc2_foundation::NSUserDefaults::standardUserDefaults();
        defaults
            .stringArrayForKey(&NSString::from_str(RECENT_KEY))
            .map(|a| a.iter().map(|s| s.to_string()).collect())
            .unwrap_or_default()
    };
    list.into_iter().map(PathBuf::from).collect()
}

fn store_recent(list: &[String]) {
    if testing() {
        TEST_RECENT.with(|r| *r.borrow_mut() = list.to_vec());
        return;
    }
    let strings: Vec<_> = list.iter().map(|s| NSString::from_str(s)).collect();
    let array = objc2_foundation::NSArray::from_retained_slice(&strings);
    let defaults = objc2_foundation::NSUserDefaults::standardUserDefaults();
    unsafe { defaults.setObject_forKey(Some(&array), &NSString::from_str(RECENT_KEY)) };
}

pub fn add_recent(path: &Path) {
    let entry = path.to_string_lossy().into_owned();
    let mut list: Vec<String> = recent_files()
        .into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    list.retain(|p| *p != entry);
    list.insert(0, entry);
    list.truncate(RECENT_LIMIT);
    store_recent(&list);
}

pub fn clear_recent() {
    store_recent(&[]);
}

pub fn open_urls(mtm: MainThreadMarker, urls: &[objc2::rc::Retained<NSURL>]) {
    for url in urls {
        if let Some(path) = url.path() {
            open_or_tell(mtm, Path::new(&path.to_string()));
        }
    }
}
