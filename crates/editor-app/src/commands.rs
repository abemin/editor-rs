//! Editing, view, search and bookmark commands from the menus.

use crate::menus::MenuAction;
use objc2::MainThreadMarker;

pub fn perform(mtm: MainThreadMarker, action: MenuAction) {
    match action {
        MenuAction::ClearSessionData => {
            let choice = crate::prompts::ask(
                mtm,
                "Delete all auto-saved changes?",
                "Unsaved changes kept for open and closed tabs will be deleted. Your files are not affected.",
                &["Clear", "Cancel"],
                true,
            );
            if choice == 0 {
                crate::session_driver::clear_all();
            }
        }
        MenuAction::ShowSessionFolder => {
            if let Some(root) = crate::session_driver::root() {
                let url = objc2_foundation::NSURL::fileURLWithPath(
                    &objc2_foundation::NSString::from_str(&root.to_string_lossy()),
                );
                objc2_app_kit::NSWorkspace::sharedWorkspace().activateFileViewerSelectingURLs(
                    &objc2_foundation::NSArray::from_retained_slice(&[url]),
                );
            }
        }
        _ => {}
    }
}

pub fn is_enabled(mtm: MainThreadMarker, action: MenuAction) -> bool {
    crate::window::current_tab(mtm).is_some() && !matches!(action, MenuAction::OpenRecent)
}

pub fn perform_with_value(mtm: MainThreadMarker, action: MenuAction, value: &str) {
    let _ = (mtm, action, value);
}
