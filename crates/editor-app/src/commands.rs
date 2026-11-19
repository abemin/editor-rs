//! Editing, view, search and bookmark commands from the menus.

use crate::menus::MenuAction;
use objc2::MainThreadMarker;

pub fn perform(mtm: MainThreadMarker, action: MenuAction) {
    let _ = (mtm, action);
}

pub fn is_enabled(mtm: MainThreadMarker, action: MenuAction) -> bool {
    crate::window::current_tab(mtm).is_some() && !matches!(action, MenuAction::OpenRecent)
}

pub fn perform_with_value(mtm: MainThreadMarker, action: MenuAction, value: &str) {
    let _ = (mtm, action, value);
}
