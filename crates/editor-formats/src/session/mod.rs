//! Crash-safe storage for open tabs and unsaved changes.

mod clone;
mod journal;
mod store;

pub use clone::{CloneKind, clone_protected};
pub use store::{MANIFEST_VERSION, Manifest, RestoredTab, SessionError, SessionStore, TabRecord};

#[cfg(test)]
use std::path::Path;

#[cfg(test)]
mod tests;
