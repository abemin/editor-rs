//! The application delegate: launch, quit and menu commands.

use crate::Args;
use crate::app::with_app;
use crate::menus::MenuAction;
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSApplication, NSApplicationDelegate, NSApplicationTerminateReply, NSMenu, NSMenuDelegate,
    NSMenuItem,
};
use objc2_foundation::{NSArray, NSNotification, NSString, NSTimer, NSURL};

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ERSAppDelegate"]
    pub struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSMenuDelegate for AppDelegate {
        #[unsafe(method(menuNeedsUpdate:))]
        fn menu_needs_update(&self, menu: &NSMenu) {
            crate::menus::update_dynamic_menu(self.mtm(), menu, self);
        }
    }

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _notification: &NSNotification) {
            let mtm = self.mtm();
            with_app(|a| a.window_delegate = Some(crate::window_delegate::WindowDelegate::new(mtm)));
            crate::menus::install(mtm, self);
            if let Some(script) = crate::app::args().test_script {
                crate::test_mode::preload_answers(&script);
            }
            crate::launch::restore_or_start(mtm);
            let pending = with_app(|a| {
                a.launched = true;
                std::mem::take(&mut a.pending_urls)
            });
            crate::document_controller::open_urls(mtm, &pending);
            for file in crate::app::args().files {
                crate::document_controller::open_or_tell(mtm, &file);
            }
            start_change_timer(mtm);
            if let Some(script) = crate::app::args().test_script {
                crate::test_mode::start(mtm, &script);
            } else {
                NSApplication::sharedApplication(mtm).activate();
            }
        }

        #[unsafe(method(applicationShouldTerminate:))]
        fn should_terminate(&self, _sender: &NSApplication) -> NSApplicationTerminateReply {
            crate::session_driver::flush_now();
            NSApplicationTerminateReply::TerminateNow
        }

        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn terminate_after_last_window(&self, _sender: &NSApplication) -> bool {
            crate::app::args().test_script.is_none()
        }

        #[unsafe(method(applicationWillResignActive:))]
        fn will_resign_active(&self, _notification: &NSNotification) {
            crate::session_driver::flush_now();
        }

        #[unsafe(method(application:openURLs:))]
        fn open_urls(&self, _application: &NSApplication, urls: &NSArray<NSURL>) {
            let urls: Vec<_> = urls.iter().collect();
            if with_app(|a| a.launched) {
                crate::document_controller::open_urls(self.mtm(), &urls);
            } else {
                with_app(|a| a.pending_urls.extend(urls));
            }
        }
    }

    impl AppDelegate {
        #[unsafe(method(menuAction:))]
        fn menu_action(&self, sender: &NSMenuItem) {
            let action = MenuAction::from_tag(sender.tag());
            if matches!(action, Some(MenuAction::OpenRecent | MenuAction::SetLanguage | MenuAction::ReopenWithEncoding)) {
                if let Some(value) = sender.representedObject().and_then(|o| o.downcast::<NSString>().ok()) {
                    crate::app_delegate::perform_with_value(self.mtm(), action.expect("action"), &value.to_string());
                }
                return;
            }
            if let Some(action) = action {
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

fn start_change_timer(mtm: MainThreadMarker) {
    let block = block2::RcBlock::new(move |_timer: std::ptr::NonNull<NSTimer>| {
        crate::document_controller::check_all_external_changes(mtm);
    });
    unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(1.5, true, &block) };
}

/// Menu commands that carry a value, such as a recent file or a language name.
pub fn perform_with_value(mtm: MainThreadMarker, action: MenuAction, value: &str) {
    match action {
        MenuAction::OpenRecent => {
            crate::document_controller::open_or_tell(mtm, std::path::Path::new(value))
        }
        other => crate::commands::perform_with_value(mtm, other, value),
    }
}

/// Runs a menu command.
pub fn perform(mtm: MainThreadMarker, action: MenuAction) {
    use crate::document_controller as dc;
    let tab = crate::window::current_tab(mtm);
    match action {
        MenuAction::NewTab => {
            crate::window::open_tab(mtm, crate::doc_model::EditorDoc::untitled());
            crate::session_driver::note_structure();
        }
        MenuAction::Open => dc::open_dialog(mtm),
        MenuAction::CloseTab => {
            if let Some(tab) = tab {
                dc::close_tab(mtm, &tab);
            }
        }
        MenuAction::CloseWindow => dc::close_window(mtm),
        MenuAction::Save => {
            if let Some(tab) = tab {
                dc::save(mtm, &tab);
            }
        }
        MenuAction::SaveAs => {
            if let Some(tab) = tab {
                dc::save_as_dialog(mtm, &tab);
            }
        }
        MenuAction::Revert => {
            if let Some(tab) = tab {
                dc::revert(mtm, &tab);
            }
        }
        MenuAction::ClearRecent => dc::clear_recent(),
        other => crate::commands::perform(mtm, other),
    }
}

/// Whether a menu command can run now.
pub fn is_enabled(mtm: MainThreadMarker, action: MenuAction) -> bool {
    let tab = crate::window::current_tab(mtm);
    match action {
        MenuAction::Save => tab.is_some_and(|t| t.doc.borrow().read_only.is_none()),
        MenuAction::Revert => {
            tab.is_some_and(|t| t.doc.borrow().path.is_some() && t.doc.borrow().doc.is_dirty())
        }
        MenuAction::CloseTab | MenuAction::CloseWindow | MenuAction::SaveAs => tab.is_some(),
        MenuAction::NewTab | MenuAction::Open | MenuAction::ClearRecent | MenuAction::About => true,
        other => crate::commands::is_enabled(mtm, other),
    }
}
