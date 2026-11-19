//! Main-thread application state.
//!
//! AppKit calls back into the app while it is running our code, so state is
//! only borrowed briefly: take what you need, release, then call AppKit.

use crate::Args;
use crate::doc_model::EditorDoc;
use crate::text_view::{DEFAULT_FONT_SIZE, TextView};
use objc2::rc::Retained;
use objc2_app_kit::{NSScrollView, NSWindow};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Instant;

pub struct Tab {
    pub id: String,
    pub window: Retained<NSWindow>,
    pub doc: RefCell<EditorDoc>,
    pub view: Retained<TextView>,
    pub scroll: Retained<NSScrollView>,
    pub opened_at: Cell<Option<Instant>>,
    pub first_paint_ms: Cell<Option<f64>>,
}

pub struct App {
    pub args: Args,
    pub tabs: Vec<Rc<Tab>>,
    pub font_size: f64,
    next_id: u64,
}

impl Default for App {
    fn default() -> Self {
        Self {
            args: Args::default(),
            tabs: Vec::new(),
            font_size: DEFAULT_FONT_SIZE,
            next_id: 0,
        }
    }
}

thread_local! {
    static APP: RefCell<App> = RefCell::new(App::default());
}

pub fn tab_by_id(id: &str) -> Option<Rc<Tab>> {
    with_app(|a| a.tabs.iter().find(|t| t.id == id).cloned())
}

/// Runs `f` with the app state. Never call AppKit inside `f`.
pub fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> R {
    APP.with(|app| f(&mut app.borrow_mut()))
}

impl App {
    pub fn new_tab_id(&mut self) -> String {
        self.next_id += 1;
        format!("t{}-{}", std::process::id(), self.next_id)
    }

    pub fn tab_for_window(&self, window: &NSWindow) -> Option<Rc<Tab>> {
        self.tabs
            .iter()
            .find(|t| std::ptr::eq(&*t.window, window))
            .cloned()
    }
}

pub fn tabs() -> Vec<Rc<Tab>> {
    with_app(|a| a.tabs.clone())
}

pub fn args() -> Args {
    with_app(|a| a.args.clone())
}
