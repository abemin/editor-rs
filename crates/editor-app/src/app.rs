//! Main-thread application state.
//!
//! AppKit calls back into the app while it is running our code, so state is
//! only borrowed briefly: take what you need, release, then call AppKit.

use crate::Args;
use crate::doc_model::EditorDoc;
use crate::text_view::{DEFAULT_FONT_SIZE, TextView};
use objc2::rc::Retained;
use objc2_app_kit::{NSScrollView, NSTextField, NSWindow};
use objc2_foundation::NSURL;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Instant;

/// (document epoch, buffer revision) and the text a search ran over.
pub type CachedSearch = ((u64, u64), std::sync::Arc<editor_search::SearchText>);

pub struct Tab {
    pub id: String,
    pub window: Retained<NSWindow>,
    pub doc: RefCell<EditorDoc>,
    pub view: Retained<TextView>,
    pub scroll: Retained<NSScrollView>,
    pub opened_at: Cell<Option<Instant>>,
    pub first_paint_ms: Cell<Option<f64>>,
    pub notice: RefCell<Option<String>>,
    pub status: Retained<NSTextField>,
    /// Unsaved work is being journaled for this tab.
    pub journaled: Cell<bool>,
    pub appended: Cell<usize>,
    /// A private copy of a backup's protected sections exists.
    pub cloned: Cell<bool>,
    pub find: RefCell<Option<crate::find_panel::FindBar>>,
    pub search_cancel: RefCell<Option<std::sync::Arc<std::sync::atomic::AtomicBool>>>,
    pub highlight: RefCell<crate::highlight::Highlighting>,
    pub folds: RefCell<crate::folds::Folds>,
    pub wrap_cache: RefCell<Option<crate::rows::WrapCache>>,
    pub search_cache: RefCell<Option<CachedSearch>>,
    /// Changes whenever the whole document is replaced (reload, reopen).
    pub epoch: Cell<u64>,
}

pub struct App {
    pub args: Args,
    pub tabs: Vec<Rc<Tab>>,
    pub font_size: f64,
    pub window_delegate: Option<Retained<crate::window_delegate::WindowDelegate>>,
    pub launched: bool,
    pub pending_urls: Vec<Retained<NSURL>>,
    pub find: crate::find_panel::FindState,
    pub settings: Option<crate::settings::Settings>,
    next_id: u64,
}

impl Default for App {
    fn default() -> Self {
        Self {
            args: Args::default(),
            tabs: Vec::new(),
            font_size: DEFAULT_FONT_SIZE,
            window_delegate: None,
            launched: false,
            pending_urls: Vec::new(),
            find: Default::default(),
            settings: None,
            next_id: 0,
        }
    }
}

thread_local! {
    static APP: RefCell<App> = RefCell::new(App::default());
}

/// Runs `f` with the app state. Never call AppKit inside `f`.
pub fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> R {
    APP.with(|app| f(&mut app.borrow_mut()))
}

impl App {
    pub fn new_tab_id(&mut self) -> String {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let tabs = &self.tabs;
        unique_id(stamp, std::process::id(), &mut self.next_id, |id| {
            tabs.iter().any(|t| t.id == id)
        })
    }

    pub fn tab_for_window(&self, window: &NSWindow) -> Option<Rc<Tab>> {
        self.tabs
            .iter()
            .find(|t| std::ptr::eq(&*t.window, window))
            .cloned()
    }
}

/// A tab id that is unique across launches and not used by any open tab.
pub fn unique_id(stamp: u128, pid: u32, counter: &mut u64, taken: impl Fn(&str) -> bool) -> String {
    loop {
        *counter += 1;
        let id = format!("t{stamp:x}-{pid}-{counter}");
        if !taken(&id) {
            return id;
        }
    }
}

/// `None` also while the app state is being torn down at exit, when AppKit
/// can still send actions from views that are being removed.
pub fn tab_by_id(id: &str) -> Option<Rc<Tab>> {
    APP.try_with(|app| {
        app.try_borrow()
            .ok()?
            .tabs
            .iter()
            .find(|t| t.id == id)
            .cloned()
    })
    .ok()
    .flatten()
}

pub fn tabs() -> Vec<Rc<Tab>> {
    with_app(|a| a.tabs.clone())
}

pub fn args() -> Args {
    with_app(|a| a.args.clone())
}

#[cfg(test)]
mod tests {
    use super::unique_id;

    #[test]
    fn ids_skip_ones_in_use_and_include_a_time_stamp() {
        let mut counter = 0;
        let id = unique_id(0xabc, 7, &mut counter, |id| id == "tabc-7-1");
        assert_eq!(id, "tabc-7-2");
        let other_launch = unique_id(0xabd, 7, &mut 0, |_| false);
        assert_ne!(other_launch, "tabc-7-1");
    }
}
