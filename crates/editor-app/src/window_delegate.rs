//! Window events: closing, resizing and focus.

use crate::app::with_app;
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSWindow, NSWindowDelegate};
use objc2_foundation::NSNotification;

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ERSWindowDelegate"]
    pub struct WindowDelegate;

    unsafe impl NSObjectProtocol for WindowDelegate {}

    unsafe impl NSWindowDelegate for WindowDelegate {
        #[unsafe(method(windowShouldClose:))]
        fn should_close(&self, window: &NSWindow) -> bool {
            match with_app(|a| a.tab_for_window(window)) {
                Some(tab) => crate::document_controller::should_close_window(self.mtm(), &tab),
                None => true,
            }
        }

        #[unsafe(method(windowWillClose:))]
        fn will_close(&self, notification: &NSNotification) {
            if let Some(window) = notification
                .object()
                .and_then(|o| o.downcast::<NSWindow>().ok())
            {
                crate::document_controller::forget_window(&window);
            }
        }

        #[unsafe(method(windowDidResize:))]
        fn did_resize(&self, notification: &NSNotification) {
            if let Some(window) = notification
                .object()
                .and_then(|o| o.downcast::<NSWindow>().ok())
                && let Some(tab) = with_app(|a| a.tab_for_window(&window))
            {
                crate::text_view::update_frames(&tab);
            }
        }

        #[unsafe(method(windowDidBecomeKey:))]
        fn did_become_key(&self, notification: &NSNotification) {
            if let Some(window) = notification
                .object()
                .and_then(|o| o.downcast::<NSWindow>().ok())
                && let Some(tab) = with_app(|a| a.tab_for_window(&window))
            {
                crate::status_bar::refresh(&tab);
                crate::document_controller::check_external_change(self.mtm(), &tab);
            }
        }
    }
);

impl WindowDelegate {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}
