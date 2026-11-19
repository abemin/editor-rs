//! What the app shows when it starts. Extended by the session task.

use objc2::MainThreadMarker;

pub fn restore_or_start(mtm: MainThreadMarker) {
    crate::window::open_tab(mtm, crate::doc_model::EditorDoc::untitled());
}
