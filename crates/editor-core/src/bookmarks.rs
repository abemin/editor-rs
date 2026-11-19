//! Line bookmarks.

use crate::buffer::{BatchMap, EditBatch};
use crate::document::Document;

pub(crate) fn remap(doc: &mut Document, batch: &EditBatch) {
    let map = BatchMap::new(batch);
    doc.bookmarks = doc
        .bookmarks
        .iter()
        .filter_map(|&offset| map.map(offset, true))
        .collect();
}
