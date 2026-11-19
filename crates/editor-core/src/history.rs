//! Undo and redo groups.

use crate::buffer::EditBatch;
use crate::selection::SelectionSet;
use std::time::{Duration, Instant};

const COALESCE_WINDOW: Duration = Duration::from_millis(1000);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Typing,
    Deleting,
    Other,
}

pub(crate) struct Step {
    pub forward: EditBatch,
    pub inverse: EditBatch,
}

pub(crate) struct Group {
    pub steps: Vec<Step>,
    pub selections_before: SelectionSet,
    pub selections_after: SelectionSet,
    pub bookmarks_before: Vec<usize>,
    pub bookmarks_after: Vec<usize>,
    state_before: u64,
    state_after: u64,
    kind: Kind,
    last: Instant,
    open: bool,
}

pub(crate) struct Snapshot {
    pub selections: SelectionSet,
    pub bookmarks: Vec<usize>,
}

#[derive(Default)]
pub(crate) struct History {
    undo: Vec<Group>,
    redo: Vec<Group>,
    next_state: u64,
    current: u64,
    saved: u64,
}

impl History {
    pub fn record(&mut self, step: Step, before: Snapshot, after: Snapshot, kind: Kind) {
        self.redo.clear();
        self.next_state += 1;
        let state = self.next_state;
        let now = Instant::now();
        let coalesce = self.undo.last().is_some_and(|g| {
            g.open
                && kind != Kind::Other
                && g.kind == kind
                && now.duration_since(g.last) < COALESCE_WINDOW
        });
        if coalesce {
            let group = self.undo.last_mut().expect("group exists");
            group.steps.push(step);
            group.selections_after = after.selections;
            group.bookmarks_after = after.bookmarks;
            group.state_after = state;
            group.last = now;
        } else {
            self.close();
            self.undo.push(Group {
                steps: vec![step],
                selections_before: before.selections,
                selections_after: after.selections,
                bookmarks_before: before.bookmarks,
                bookmarks_after: after.bookmarks,
                state_before: self.current,
                state_after: state,
                kind,
                last: now,
                open: kind != Kind::Other,
            });
        }
        self.current = state;
    }

    /// Ends the current typing run.
    pub fn close(&mut self) {
        if let Some(group) = self.undo.last_mut() {
            group.open = false;
        }
    }

    pub fn pop_undo(&mut self) -> Option<Group> {
        self.close();
        let group = self.undo.pop()?;
        self.current = group.state_before;
        Some(group)
    }

    pub fn push_redo(&mut self, group: Group) {
        self.redo.push(group);
    }

    pub fn pop_redo(&mut self) -> Option<Group> {
        let group = self.redo.pop()?;
        self.current = group.state_after;
        Some(group)
    }

    pub fn push_undo(&mut self, group: Group) {
        self.undo.push(group);
    }

    pub fn mark_saved(&mut self) {
        self.close();
        self.saved = self.current;
    }

    pub fn is_dirty(&self) -> bool {
        self.current != self.saved
    }
}
