//! The Settings window: font size and default word wrap.

use crate::app::with_app;
use crate::text_view::DEFAULT_FONT_SIZE;
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSBackingStoreType, NSButton, NSControlStateValueOff, NSControlStateValueOn, NSTextField,
    NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString, NSUserDefaults};

const FONT_KEY: &str = "FontSize";
const WRAP_KEY: &str = "WrapByDefault";
pub const MIN_FONT: f64 = 8.0;
pub const MAX_FONT: f64 = 36.0;

pub struct Settings {
    pub window: Retained<NSWindow>,
    size_label: Retained<NSTextField>,
    wrap: Retained<NSButton>,
    _controller: Retained<SettingsController>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ERSSettingsController"]
    pub struct SettingsController;

    unsafe impl NSObjectProtocol for SettingsController {}

    impl SettingsController {
        #[unsafe(method(smaller:))]
        fn smaller(&self, _sender: &NSObject) {
            change_font_size(self.mtm(), -1.0);
        }

        #[unsafe(method(bigger:))]
        fn bigger(&self, _sender: &NSObject) {
            change_font_size(self.mtm(), 1.0);
        }

        #[unsafe(method(toggleWrap:))]
        fn toggle_wrap(&self, sender: &NSButton) {
            store_bool(WRAP_KEY, sender.state() == NSControlStateValueOn);
        }
    }
);

fn testing() -> bool {
    crate::app::args().test_script.is_some()
}

fn defaults() -> Retained<NSUserDefaults> {
    NSUserDefaults::standardUserDefaults()
}

fn store_bool(key: &str, value: bool) {
    if !testing() {
        defaults().setBool_forKey(value, &NSString::from_str(key));
    }
}

/// Whether new documents start with word wrap on.
pub fn wrap_by_default() -> bool {
    !testing() && defaults().boolForKey(&NSString::from_str(WRAP_KEY))
}

/// Font size saved from the last session.
pub fn saved_font_size() -> f64 {
    if testing() {
        return DEFAULT_FONT_SIZE;
    }
    let size = defaults().doubleForKey(&NSString::from_str(FONT_KEY));
    if size >= MIN_FONT && size <= MAX_FONT {
        size
    } else {
        DEFAULT_FONT_SIZE
    }
}

/// Applies a new font size to every tab.
pub fn set_font_size(mtm: MainThreadMarker, size: f64) {
    let size = size.clamp(MIN_FONT, MAX_FONT);
    with_app(|a| a.font_size = size);
    if !testing() {
        defaults().setDouble_forKey(size, &NSString::from_str(FONT_KEY));
    }
    for tab in crate::app::tabs() {
        crate::rows::invalidate(&tab);
        crate::text_view::update_frames(&tab);
    }
    refresh(mtm);
}

pub fn change_font_size(mtm: MainThreadMarker, delta: f64) {
    let size = with_app(|a| a.font_size) + delta;
    set_font_size(mtm, size);
}

fn refresh(_mtm: MainThreadMarker) {
    let size = with_app(|a| a.font_size);
    let label = with_app(|a| a.settings.as_ref().map(|s| s.size_label.clone()));
    if let Some(label) = label {
        label.setStringValue(&NSString::from_str(&format!("Font size: {size:.0} pt")));
    }
}

pub fn show(mtm: MainThreadMarker) {
    let existing = with_app(|a| a.settings.as_ref().map(|s| s.window.clone()));
    if let Some(window) = existing {
        window.makeKeyAndOrderFront(None);
        return;
    }
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(380.0, 140.0));
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            frame,
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&NSString::from_str("Settings"));
    let controller: Retained<SettingsController> = {
        let this = SettingsController::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    };
    let content = NSView::initWithFrame(NSView::alloc(mtm), frame);
    let size_label = NSTextField::labelWithString(&NSString::from_str(""), mtm);
    size_label.setFrame(NSRect::new(
        NSPoint::new(20.0, 90.0),
        NSSize::new(160.0, 22.0),
    ));
    content.addSubview(&size_label);
    let smaller = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str("Smaller"),
            Some(&controller),
            Some(sel!(smaller:)),
            mtm,
        )
    };
    smaller.setFrame(NSRect::new(
        NSPoint::new(190.0, 86.0),
        NSSize::new(80.0, 28.0),
    ));
    content.addSubview(&smaller);
    let bigger = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str("Bigger"),
            Some(&controller),
            Some(sel!(bigger:)),
            mtm,
        )
    };
    bigger.setFrame(NSRect::new(
        NSPoint::new(275.0, 86.0),
        NSSize::new(80.0, 28.0),
    ));
    content.addSubview(&bigger);
    let wrap = unsafe {
        NSButton::checkboxWithTitle_target_action(
            &NSString::from_str("Wrap long lines in newly opened documents"),
            Some(&controller),
            Some(sel!(toggleWrap:)),
            mtm,
        )
    };
    wrap.setFrame(NSRect::new(
        NSPoint::new(20.0, 40.0),
        NSSize::new(340.0, 24.0),
    ));
    wrap.setState(if wrap_by_default() {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    });
    content.addSubview(&wrap);
    window.setContentView(Some(&content));
    if testing() {
        window.setFrameOrigin(NSPoint::new(-20000.0, -20000.0));
    } else {
        window.center();
    }
    window.makeKeyAndOrderFront(None);
    with_app(|a| {
        a.settings = Some(Settings {
            window,
            size_label,
            wrap,
            _controller: controller,
        })
    });
    refresh(mtm);
}

pub fn is_open() -> bool {
    with_app(|a| a.settings.as_ref().is_some_and(|s| s.window.isVisible()))
}

pub fn window() -> Option<Retained<NSWindow>> {
    with_app(|a| a.settings.as_ref().map(|s| s.window.clone()))
}

#[allow(dead_code)]
fn _wrap_button(s: &Settings) -> &NSButton {
    &s.wrap
}
