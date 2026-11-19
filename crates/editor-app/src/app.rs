//! Main-thread application state.
//!
//! AppKit calls back into the app while it is running our code, so state is
//! only borrowed briefly: take what you need, release, then call AppKit.

use crate::Args;
use objc2::rc::Retained;
use objc2_app_kit::NSWindow;
use std::cell::RefCell;
use std::rc::Rc;

pub struct Tab {
    pub id: String,
    pub window: Retained<NSWindow>,
}

#[derive(Default)]
pub struct App {
    pub args: Args,
    pub tabs: Vec<Rc<Tab>>,
    next_id: u64,
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
