//! Bookmark commands. Extended by the bookmarks UI task.

use crate::app::Tab;
use std::rc::Rc;

pub fn toggle_line(tab: &Rc<Tab>, line: usize) {
    tab.doc.borrow_mut().doc.toggle_bookmark(line);
    tab.view.setNeedsDisplay(true);
    crate::session_driver::note_change(tab);
}
