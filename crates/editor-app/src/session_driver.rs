//! Auto-save. Filled in by the session task.

use crate::app::Tab;

pub fn note_change(_tab: &Tab) {}

pub fn note_structure() {}
pub fn saved(_tab: &Tab) {}
pub fn discard(_tab: &Tab) {}
pub fn flush_now() {}
