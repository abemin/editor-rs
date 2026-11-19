//! Opening, saving and closing documents.

use crate::app::Tab;
use crate::doc_model::EditorDoc;
use objc2::MainThreadMarker;
use std::path::Path;
use std::rc::Rc;

/// Opens a file in a new tab.
pub fn open_path(mtm: MainThreadMarker, path: &Path) -> Result<Rc<Tab>, String> {
    let started = std::time::Instant::now();
    let file = editor_formats::load_file(path)
        .map_err(|e| format!("{} could not be opened: {e}", path.display()))?;
    let doc = EditorDoc::from_loaded(file, path);
    let tab = crate::window::open_tab(mtm, doc);
    tab.opened_at.set(Some(started));
    Ok(tab)
}
