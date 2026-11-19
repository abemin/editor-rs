//! Text engine: buffer, editing, undo, selections and bookmarks.

pub mod bookmarks;
pub mod buffer;
pub mod document;
mod history;
pub mod selection;

pub use buffer::{AppliedBatch, BatchMap, Buffer, Edit, EditBatch, EditError, map_offset};
pub use document::{Document, Eol, Motion};
pub use selection::{Selection, SelectionSet};
