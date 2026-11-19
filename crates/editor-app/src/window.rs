//! Document windows. Each tab is a window in a native tab group.

use crate::app::{Tab, with_app};
use crate::doc_model::EditorDoc;
use crate::text_view::{TextView, update_frames};
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSAutoresizingMaskOptions, NSBackingStoreType, NSScrollView, NSView, NSWindow,
    NSWindowOrderingMode, NSWindowStyleMask, NSWindowTabbingMode,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const TABBING_ID: &str = "document";

fn testing() -> bool {
    crate::app::args().test_script.is_some()
}

/// Creates a tab. It joins the frontmost window's tab group when there is one.
pub fn open_tab(mtm: MainThreadMarker, doc: EditorDoc) -> Rc<Tab> {
    let title = doc.title();
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
    window.setTitle(&NSString::from_str(&title));
    window.setTabbingMode(NSWindowTabbingMode::Preferred);
    window.setTabbingIdentifier(&NSString::from_str(TABBING_ID));
    window.setMinSize(NSSize::new(320.0, 200.0));

    let id = with_app(|a| a.new_tab_id());
    let container = NSView::initWithFrame(NSView::alloc(mtm), window.contentLayoutRect());
    let scroll = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), container.bounds());
    scroll.setHasVerticalScroller(true);
    scroll.setHasHorizontalScroller(true);
    scroll.setAutohidesScrollers(true);
    scroll.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    let view = TextView::new(mtm, &id);
    scroll.setDocumentView(Some(&view));
    #[allow(deprecated)]
    scroll.contentView().setCopiesOnScroll(false);
    container.addSubview(&scroll);
    window.setContentView(Some(&container));
    let tab = Rc::new(Tab {
        id,
        window: window.clone(),
        doc: RefCell::new(doc),
        view: view.clone(),
        scroll,
        opened_at: Cell::new(None),
        first_paint_ms: Cell::new(None),
        notice: RefCell::new(None),
    });
    let front = NSApplication::sharedApplication(mtm).keyWindow();
    with_app(|a| a.tabs.push(tab.clone()));
    update_frames(&tab);
    match front {
        Some(front) if !std::ptr::eq(&*front, &*window) => {
            front.addTabbedWindow_ordered(&window, NSWindowOrderingMode::Above);
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
    window.makeFirstResponder(Some(&view));
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
    if let Some(window) = app.keyWindow().or_else(|| app.mainWindow())
        && let Some(tab) = with_app(|a| a.tab_for_window(&window))
    {
        return Some(tab);
    }
    crate::app::tabs()
        .into_iter()
        .rev()
        .find(|t| t.window.isVisible())
}
