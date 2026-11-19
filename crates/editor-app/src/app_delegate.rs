//! The application delegate: launch, quit and menu commands.

use crate::Args;
use crate::app::with_app;
use crate::menus::MenuAction;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSApplication, NSApplicationDelegate, NSApplicationTerminateReply, NSMenuItem,
};
use objc2_foundation::NSNotification;

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ERSAppDelegate"]
    pub struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _notification: &NSNotification) {
            let mtm = self.mtm();
            crate::menus::install(mtm, self);
            crate::window::open_tab(mtm, "Untitled");
            if let Some(script) = crate::app::args().test_script {
                crate::test_mode::start(mtm, &script);
            } else {
                NSApplication::sharedApplication(mtm).activate();
            }
        }

        #[unsafe(method(applicationShouldTerminate:))]
        fn should_terminate(&self, _sender: &NSApplication) -> NSApplicationTerminateReply {
            NSApplicationTerminateReply::TerminateNow
        }

        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn terminate_after_last_window(&self, _sender: &NSApplication) -> bool {
            true
        }
    }

    impl AppDelegate {
        #[unsafe(method(menuAction:))]
        fn menu_action(&self, sender: &NSMenuItem) {
            if let Some(action) = MenuAction::from_tag(sender.tag()) {
                crate::app_delegate::perform(self.mtm(), action);
            }
        }

        #[unsafe(method(validateMenuItem:))]
        fn validate_menu_item(&self, item: &NSMenuItem) -> bool {
            MenuAction::from_tag(item.tag()).is_none_or(|action| crate::app_delegate::is_enabled(self.mtm(), action))
        }
    }
);

impl AppDelegate {
    pub fn new(mtm: MainThreadMarker, args: Args) -> Retained<Self> {
        with_app(|a| a.args = args);
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

/// Runs a menu command.
pub fn perform(mtm: MainThreadMarker, action: MenuAction) {
    match action {
        MenuAction::NewTab => {
            crate::window::open_tab(mtm, "Untitled");
        }
        MenuAction::CloseTab => {
            if let Some(tab) = crate::window::current_tab(mtm) {
                tab.window.performClose(None);
            }
        }
        _ => {}
    }
}

/// Whether a menu command can run now.
pub fn is_enabled(_mtm: MainThreadMarker, action: MenuAction) -> bool {
    matches!(action, MenuAction::NewTab | MenuAction::CloseTab)
}

#[allow(dead_code)]
fn _assert_object(_: &AnyObject) {}
