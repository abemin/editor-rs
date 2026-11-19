//! Text engine: buffer, editing, undo, selections and bookmarks.
#![cfg_attr(test, allow(clippy::single_range_in_vec_init))]

pub mod bookmarks;
pub mod buffer;
pub mod document;
mod history;
pub mod selection;

pub use buffer::{AppliedBatch, BatchMap, Buffer, Edit, EditBatch, EditError, map_offset};
pub use document::{Document, Eol, Motion};
pub use selection::{Selection, SelectionSet};
