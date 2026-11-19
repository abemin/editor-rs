//! Auto-save: journals unsaved work to the session folder on a background thread.
//!
//! The main thread only sends messages, so typing never waits for the disk.
//! Edits are written about a second after the last change, and immediately
//! when the app quits, loses focus, or the last window closes.

use crate::app::{Tab, tabs};
use editor_core::EditBatch;
use editor_formats::session::{
    MANIFEST_VERSION, Manifest, RestoredTab, SessionError, SessionStore, TabRecord, clone_protected,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const IDLE_FLUSH: Duration = Duration::from_millis(1000);
const COMPACT_AFTER: usize = 400;
const FLUSH_WAIT: Duration = Duration::from_secs(3);

enum Msg {
    Append(String, Vec<EditBatch>),
    Snapshot(String, String),
    Manifest(Manifest),
    Discard(String),
    Clone(String, PathBuf),
    ClearAll,
    Flush(Sender<()>),
}

struct Driver {
    tx: Sender<Msg>,
    root: PathBuf,
}

thread_local! {
    static DRIVER: RefCell<Option<Driver>> = const { RefCell::new(None) };
}

static WARNING: Mutex<Option<String>> = Mutex::new(None);

fn set_warning(message: Option<String>) {
    *WARNING.lock().expect("warning lock") = message;
}

/// A persistent problem with auto-save, shown in the status bar.
pub fn warning() -> Option<String> {
    WARNING.lock().expect("warning lock").clone()
}

/// Auto-save is running and its last write succeeded.
pub fn is_reliable() -> bool {
    root().is_some() && warning().is_none()
}

pub fn default_root() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join("Library/Application Support")
        .join(crate::config::APP.display_name)
        .join("Session")
}

pub fn root() -> Option<PathBuf> {
    DRIVER.with(|d| d.borrow().as_ref().map(|d| d.root.clone()))
}

/// Opens the session folder and starts the writer. Returns what can be restored.
pub fn start(root: &Path) -> Result<Vec<RestoredTab>, SessionError> {
    let store = SessionStore::open(root)?;
    let restored = store.restore();
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("session-writer".into())
        .spawn(move || writer(store, rx))
        .map_err(SessionError::Io)?;
    DRIVER.with(|d| {
        *d.borrow_mut() = Some(Driver {
            tx,
            root: root.to_path_buf(),
        })
    });
    Ok(restored)
}

pub fn disable(reason: &str) {
    set_warning(Some(format!(
        "Auto-save is not working ({reason}). Changes are kept only when you save."
    )));
}

fn writer(store: SessionStore, rx: Receiver<Msg>) {
    let mut pending: HashMap<String, Vec<EditBatch>> = HashMap::new();
    let mut manifest: Option<Manifest> = None;
    let mut last_message = Instant::now();
    let report = |result: std::io::Result<()>| match result {
        Ok(()) => set_warning(None),
        Err(error) => set_warning(Some(format!(
            "Auto-save is not working ({error}). Changes are kept only when you save."
        ))),
    };
    let write_pending = |pending: &mut HashMap<String, Vec<EditBatch>>,
                         manifest: &mut Option<Manifest>| {
        for (id, batches) in pending.drain() {
            report(store.append(&id, &batches));
        }
        if let Some(m) = manifest.take() {
            report(store.write_manifest(&m));
        }
    };
    loop {
        let waiting = !pending.is_empty() || manifest.is_some();
        let timeout = if waiting {
            IDLE_FLUSH.saturating_sub(last_message.elapsed())
        } else {
            Duration::from_secs(3600)
        };
        match rx.recv_timeout(timeout) {
            Ok(message) => {
                last_message = Instant::now();
                match message {
                    Msg::Append(id, batches) => pending.entry(id).or_default().extend(batches),
                    Msg::Snapshot(id, text) => {
                        pending.remove(&id);
                        report(store.snapshot(&id, &text));
                    }
                    Msg::Manifest(m) => manifest = Some(m),
                    Msg::Discard(id) => {
                        pending.remove(&id);
                        report(store.discard(&id));
                    }
                    Msg::Clone(id, source) => {
                        report(clone_protected(&source, &store.protected_path(&id)).map(|_| ()))
                    }
                    Msg::ClearAll => {
                        pending.clear();
                        manifest = None;
                        report(store.clear_all());
                    }
                    Msg::Flush(ack) => {
                        write_pending(&mut pending, &mut manifest);
                        let _ = ack.send(());
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => write_pending(&mut pending, &mut manifest),
            Err(RecvTimeoutError::Disconnected) => {
                write_pending(&mut pending, &mut manifest);
                return;
            }
        }
    }
}

fn send(message: Msg) {
    DRIVER.with(|d| {
        if let Some(driver) = d.borrow().as_ref() {
            let _ = driver.tx.send(message);
        }
    });
}

fn millis(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

fn record(tab: &Tab) -> TabRecord {
    let doc = tab.doc.borrow();
    let line_height = crate::text_view::metrics().line_height;
    TabRecord {
        id: tab.id.clone(),
        path: doc.path.clone(),
        title: doc.title(),
        dirty: doc.doc.is_dirty(),
        caret: doc.doc.selections().primary().head,
        scroll_line: (tab.view.visibleRect().origin.y / line_height)
            .floor()
            .max(0.0) as usize,
        bookmarks: doc.doc.bookmarked_lines(),
        word_wrap: doc.word_wrap,
        language: Some(doc.language.name().to_string()),
        encoding: crate::status_bar::encoding_name(&doc.encoding),
        source_len: doc.source.as_ref().map(|s| s.len),
        source_modified_ms: doc.source.as_ref().map(|s| millis(s.modified)),
        has_protected_clone: tab.cloned.get(),
    }
}

/// Records which tabs are open, in order.
pub fn note_structure() {
    if root().is_none() {
        return;
    }
    let active = crate::window::current_tab_without_marker().map(|t| t.id.clone());
    let tabs = tabs();
    let manifest = Manifest {
        version: MANIFEST_VERSION,
        active,
        tabs: tabs.iter().map(|t| record(t)).collect(),
    };
    send(Msg::Manifest(manifest));
}

/// Called after every edit.
pub fn note_change(tab: &Tab) {
    let batches = tab.doc.borrow_mut().doc.take_change_log();
    if root().is_none() {
        return;
    }
    if !batches.is_empty() {
        let compact = tab.appended.get() + batches.len() > COMPACT_AFTER;
        if !tab.journaled.get() || compact || warning().is_some() {
            let (is_backup, path) = {
                let doc = tab.doc.borrow();
                (doc.container.is_some(), doc.path.clone())
            };
            if is_backup
                && !tab.cloned.get()
                && let Some(path) = path
            {
                send(Msg::Clone(tab.id.clone(), path));
                tab.cloned.set(true);
            }
            send(Msg::Snapshot(tab.id.clone(), tab.doc.borrow().doc.text()));
            tab.journaled.set(true);
            tab.appended.set(0);
        } else {
            tab.appended.set(tab.appended.get() + batches.len());
            send(Msg::Append(tab.id.clone(), batches));
        }
    }
    note_structure();
}

/// After a save the file on disk is the truth again.
pub fn saved(tab: &Tab) {
    tab.doc.borrow_mut().doc.take_change_log();
    tab.journaled.set(false);
    tab.cloned.set(false);
    send(Msg::Discard(tab.id.clone()));
    note_structure();
}

/// The tab was closed on purpose: forget its unsaved work.
pub fn discard(tab: &Tab) {
    send(Msg::Discard(tab.id.clone()));
}

pub fn clear_all() {
    for tab in tabs() {
        tab.journaled.set(false);
        tab.cloned.set(false);
    }
    send(Msg::ClearAll);
}

/// Writes everything now and waits briefly for it to finish.
pub fn flush_now() {
    if root().is_none() {
        return;
    }
    for tab in tabs() {
        note_change(&tab);
    }
    note_structure();
    let (ack_tx, ack_rx) = mpsc::channel();
    send(Msg::Flush(ack_tx));
    let _ = ack_rx.recv_timeout(FLUSH_WAIT);
}
