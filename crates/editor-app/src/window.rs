//! Document windows. Each tab is a window in a native tab group.

use crate::app::{Tab, with_app};
use objc2::{MainThreadMarker, MainThreadOnly};

use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSWindow, NSWindowStyleMask, NSWindowTabbingMode,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use std::rc::Rc;

const TABBING_ID: &str = "document";

fn testing() -> bool {
    crate::app::args().test_script.is_some()
}

/// Creates a tab. It joins the frontmost window's tab group when there is one.
pub fn open_tab(mtm: MainThreadMarker, title: &str) -> Rc<Tab> {
    let style = NSWindowStyleMask::Titled
        | NSWindowStyleMask::Closable
        | NSWindowStyleMask::Miniaturizable
        | NSWindowStyleMask::Resizable;
    let frame = NSRect::new(NSPoint::new(200.0, 200.0), NSSize::new(900.0, 650.0));
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            frame,
            style,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&NSString::from_str(title));
    window.setTabbingMode(NSWindowTabbingMode::Preferred);
    window.setTabbingIdentifier(&NSString::from_str(TABBING_ID));
    window.setMinSize(NSSize::new(320.0, 200.0));

    let tab = with_app(|a| {
        let tab = Rc::new(Tab {
            id: a.new_tab_id(),
            window: window.clone(),
        });
        a.tabs.push(tab.clone());
        tab
    });
    let front = NSApplication::sharedApplication(mtm)
        .keyWindow()
        .or_else(|| {
            crate::app::tabs()
                .iter()
                .rev()
                .nth(1)
                .map(|t| t.window.clone())
        });
    match front {
        Some(front) if !std::ptr::eq(&*front, &*window) => {
            front.addTabbedWindow_ordered(&window, objc2_app_kit::NSWindowOrderingMode::Above)
        }
        _ => {
            if testing() {
                window.setFrameOrigin(NSPoint::new(-20000.0, -20000.0));
            } else {
                window.center();
            }
        }
    }
    window.makeKeyAndOrderFront(None);
    tab
}

/// Visible tab groups: windows that are not hidden behind another tab.
pub fn window_count() -> usize {
    crate::app::tabs()
        .iter()
        .filter(|t| t.window.isVisible())
        .filter(|t| {
            t.window
                .tabGroup()
                .and_then(|g| g.selectedWindow())
                .is_none_or(|w| std::ptr::eq(&*w, &*t.window))
        })
        .count()
}

/// The tab the user is looking at.
pub fn current_tab(mtm: MainThreadMarker) -> Option<Rc<Tab>> {
    let app = NSApplication::sharedApplication(mtm);
    let window = app.keyWindow().or_else(|| app.mainWindow());
    let tabs = crate::app::tabs();
    if let Some(window) = window
        && let Some(tab) = with_app(|a| a.tab_for_window(&window))
    {
        return Some(tab);
    }
    tabs.into_iter().rev().find(|t| t.window.isVisible())
}
