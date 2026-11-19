//! Text engine: buffer, editing, undo, selections and bookmarks.

pub mod buffer;

pub use buffer::{AppliedBatch, BatchMap, Buffer, Edit, EditBatch, EditError, map_offset};
