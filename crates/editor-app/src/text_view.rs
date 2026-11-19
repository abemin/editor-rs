//! The text view: draws only the lines and columns that are visible.

use crate::app::{Tab, tab_by_id, with_app};
use crate::layout::{DisplayLine, display_line};
use crate::theme::{self, Theme, color};
use editor_syntax::Role;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAttributedStringNSStringDrawing, NSBeep, NSBezierPath, NSEvent, NSEventModifierFlags, NSFont,
    NSFontAttributeName, NSFontWeightRegular, NSForegroundColorAttributeName, NSResponder,
    NSTextInputClient, NSUnderlineStyleAttributeName, NSView,
};
use objc2_foundation::{
    NSArray, NSAttributedString, NSAttributedStringKey, NSMutableAttributedString, NSNumber,
    NSPoint, NSRange, NSRect, NSSize, NSString, NSUInteger,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub const DEFAULT_FONT_SIZE: f64 = 13.0;
const GUTTER_PADDING: f64 = 22.0;
const TEXT_GAP: f64 = 8.0;
const COLUMN_MARGIN: usize = 2;
const BOTTOM_LINES: f64 = 5.0;

#[derive(Clone, Copy, Debug, Default)]
pub struct DrawStats {
    pub lines: usize,
    pub max_columns: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    pub advance: f64,
    pub line_height: f64,
    pub baseline_pad: f64,
}

pub fn font() -> Retained<NSFont> {
    let size = with_app(|a| a.font_size);
    unsafe { NSFont::monospacedSystemFontOfSize_weight(size, NSFontWeightRegular) }
}

pub fn metrics() -> Metrics {
    let font = font();
    let sample = attributed(&"M".repeat(10), &font, theme::LIGHT.text, &[]);
    let advance = sample.size().width / 10.0;
    let line_height = (font.ascender() - font.descender() + font.leading()).ceil() + 3.0;
    Metrics {
        advance,
        line_height,
        baseline_pad: 1.5,
    }
}

pub fn gutter_width(lines: usize, m: &Metrics) -> f64 {
    let digits = lines.max(1).ilog10() as f64 + 1.0;
    (digits.max(3.0) * m.advance + GUTTER_PADDING).ceil()
}

pub fn text_left(lines: usize, m: &Metrics) -> f64 {
    gutter_width(lines, m) + TEXT_GAP
}

/// Builds an attributed string with optional colored character ranges.
pub fn attributed(
    text: &str,
    font: &NSFont,
    base: (f64, f64, f64),
    colors: &[(std::ops::Range<usize>, (f64, f64, f64))],
) -> Retained<NSMutableAttributedString> {
    let string = NSMutableAttributedString::from_nsstring(&NSString::from_str(text));
    let utf16_len = text.encode_utf16().count();
    let full = NSRange::new(0, utf16_len);
    unsafe {
        string.addAttribute_value_range(NSFontAttributeName, font as &AnyObject, full);
        string.addAttribute_value_range(
            NSForegroundColorAttributeName,
            &*color(base) as &AnyObject,
            full,
        );
    }
    if !colors.is_empty() {
        let mut utf16 = Vec::with_capacity(text.len() + 1);
        let mut at = 0;
        for c in text.chars() {
            utf16.push(at);
            at += c.len_utf16();
        }
        utf16.push(at);
        for (range, rgb) in colors {
            let start = utf16[range.start.min(utf16.len() - 1)];
            let end = utf16[range.end.min(utf16.len() - 1)];
            if end > start {
                unsafe {
                    string.addAttribute_value_range(
                        NSForegroundColorAttributeName,
                        &*color(*rgb) as &AnyObject,
                        NSRange::new(start, end - start),
                    );
                }
            }
        }
    }
    string
}

fn fill(rect: NSRect, rgb: (f64, f64, f64)) {
    color(rgb).setFill();
    NSBezierPath::fillRect(rect);
}

pub struct TextViewIvars {
    pub tab_id: RefCell<String>,
    pub stats: Cell<DrawStats>,
    pub marked: RefCell<Option<String>>,
    pub drag: Cell<Option<crate::input::DragAnchor>>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ERSTextView"]
    #[ivars = TextViewIvars]
    pub struct TextView;

    unsafe impl NSObjectProtocol for TextView {}

    unsafe impl NSTextInputClient for TextView {
        #[unsafe(method(insertText:replacementRange:))]
        fn insert_text(&self, string: &AnyObject, _range: NSRange) {
            let text = object_text(string);
            *self.ivars().marked.borrow_mut() = None;
            if let Some(tab) = self.tab() {
                crate::input::edit(&tab, |d| d.insert_text(&text));
            }
        }

        #[unsafe(method(doCommandBySelector:))]
        fn do_command(&self, selector: Sel) {
            if let Some(tab) = self.tab()
                && !crate::input::command(&tab, selector.name().to_str().unwrap_or_default())
            {
                NSBeep();
            }
        }

        #[unsafe(method(setMarkedText:selectedRange:replacementRange:))]
        fn set_marked_text(&self, string: &AnyObject, _selected: NSRange, _replacement: NSRange) {
            let text = object_text(string);
            *self.ivars().marked.borrow_mut() = if text.is_empty() { None } else { Some(text) };
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(unmarkText))]
        fn unmark_text(&self) {
            let marked = self.ivars().marked.borrow_mut().take();
            if let (Some(text), Some(tab)) = (marked, self.tab()) {
                crate::input::edit(&tab, |d| d.insert_text(&text));
            }
        }

        #[unsafe(method(selectedRange))]
        fn selected_range(&self) -> NSRange {
            self.tab().map_or(NSRange::new(0, 0), |tab| {
                let s = tab.doc.borrow().doc.selections().primary();
                NSRange::new(s.min(), s.max() - s.min())
            })
        }

        #[unsafe(method(markedRange))]
        fn marked_range(&self) -> NSRange {
            match (&*self.ivars().marked.borrow(), self.tab()) {
                (Some(text), Some(tab)) => {
                    let head = tab.doc.borrow().doc.selections().primary().head;
                    NSRange::new(head, text.encode_utf16().count())
                }
                _ => NSRange::new(usize::MAX >> 1, 0),
            }
        }

        #[unsafe(method(hasMarkedText))]
        fn has_marked_text(&self) -> bool {
            self.ivars().marked.borrow().is_some()
        }

        #[unsafe(method_id(attributedSubstringForProposedRange:actualRange:))]
        fn attributed_substring(&self, _range: NSRange, _actual: *mut NSRange) -> Option<Retained<NSAttributedString>> {
            None
        }

        #[unsafe(method_id(validAttributesForMarkedText))]
        fn valid_attributes(&self) -> Retained<NSArray<NSAttributedStringKey>> {
            NSArray::new()
        }

        #[unsafe(method(firstRectForCharacterRange:actualRange:))]
        fn first_rect(&self, _range: NSRange, _actual: *mut NSRange) -> NSRect {
            let Some(tab) = self.tab() else { return NSRect::ZERO };
            let (line, column) = {
                let doc = tab.doc.borrow();
                let (line, column) = doc.doc.line_col(doc.doc.selections().primary().head);
                (line, display_line(&doc, line).column_of(column))
            };
            let rect = rect_for(&tab, line, column);
            let in_window = self.convertRect_toView(rect, None);
            tab.window.convertRectToScreen(in_window)
        }

        #[unsafe(method(characterIndexForPoint:))]
        fn character_index(&self, point: NSPoint) -> NSUInteger {
            let Some(tab) = self.tab() else { return 0 };
            let in_window = tab.window.convertPointFromScreen(point);
            let local = self.convertPoint_fromView(in_window, None);
            crate::input::offset_at_point(&tab, local).0
        }
    }

    impl TextView {
        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            self.interpretKeyEvents(&NSArray::from_slice(&[event]));
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let Some(tab) = self.tab() else { return };
            if let Some(window) = self.window() {
                window.makeFirstResponder(Some(self));
            }
            let point = self.convertPoint_fromView(event.locationInWindow(), None);
            let m = metrics();
            let lines = tab.doc.borrow().doc.buffer().rope().len_lines();
            if point.x < self.visibleRect().origin.x + gutter_width(lines, &m) {
                let line = (point.y / m.line_height).floor().max(0.0) as usize;
                crate::bookmark_actions::toggle_line(&tab, line);
                return;
            }
            let flags = event.modifierFlags();
            let mods = crate::input::Modifiers {
                command: flags.contains(NSEventModifierFlags::Command),
                option: flags.contains(NSEventModifierFlags::Option),
                shift: flags.contains(NSEventModifierFlags::Shift),
            };
            let (offset, line, column) = crate::input::offset_at_point(&tab, point);
            let anchor = crate::input::click(&tab, offset, line, column, mods, event.clickCount().max(1) as usize);
            self.ivars().drag.set(Some(anchor));
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            let (Some(tab), Some(anchor)) = (self.tab(), self.ivars().drag.get()) else { return };
            self.autoscroll(event);
            let point = self.convertPoint_fromView(event.locationInWindow(), None);
            let (offset, line, column) = crate::input::offset_at_point(&tab, point);
            crate::input::drag(&tab, anchor, offset, line, column);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, _event: &NSEvent) {
            self.ivars().drag.set(None);
        }

        #[unsafe(method(copy:))]
        fn copy_action(&self, _sender: Option<&AnyObject>) {
            if let Some(tab) = self.tab() {
                crate::input::copy(&tab);
            }
        }

        #[unsafe(method(cut:))]
        fn cut_action(&self, _sender: Option<&AnyObject>) {
            if let Some(tab) = self.tab() {
                crate::input::cut(&tab);
            }
        }

        #[unsafe(method(paste:))]
        fn paste_action(&self, _sender: Option<&AnyObject>) {
            if let Some(tab) = self.tab() {
                crate::input::paste(&tab);
            }
        }

        #[unsafe(method(undo:))]
        fn undo_action(&self, _sender: Option<&AnyObject>) {
            if let Some(tab) = self.tab() {
                crate::input::undo(&tab);
            }
        }

        #[unsafe(method(redo:))]
        fn redo_action(&self, _sender: Option<&AnyObject>) {
            if let Some(tab) = self.tab() {
                crate::input::redo(&tab);
            }
        }

        #[unsafe(method(selectAll:))]
        fn select_all_action(&self, _sender: Option<&AnyObject>) {
            if let Some(tab) = self.tab() {
                crate::input::select_all(&tab);
            }
        }

        #[unsafe(method(deleteBackward:))]
        fn delete_backward_action(&self, _sender: Option<&AnyObject>) {
            if let Some(tab) = self.tab() {
                crate::input::command(&tab, "deleteBackward:");
            }
        }

        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(isOpaque))]
        fn is_opaque(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, dirty: NSRect) {
            let id = self.ivars().tab_id.borrow().clone();
            if let Some(tab) = tab_by_id(&id) {
                let stats = draw_text(self, &tab, dirty);
                draw_gutter(self, &tab, dirty);
                self.ivars().stats.set(stats);
                if tab.first_paint_ms.get().is_none()
                    && let Some(opened) = tab.opened_at.get()
                {
                    tab.first_paint_ms.set(Some(opened.elapsed().as_secs_f64() * 1000.0));
                }
            }
        }
    }
);

fn object_text(object: &AnyObject) -> String {
    if let Some(attributed) = object.downcast_ref::<NSAttributedString>() {
        attributed.string().to_string()
    } else if let Some(string) = object.downcast_ref::<NSString>() {
        string.to_string()
    } else {
        String::new()
    }
}

impl TextView {
    pub fn tab(&self) -> Option<Rc<Tab>> {
        tab_by_id(&self.ivars().tab_id.borrow())
    }

    pub fn new(mtm: MainThreadMarker, tab_id: &str) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(TextViewIvars {
            tab_id: RefCell::new(tab_id.to_string()),
            stats: Cell::new(DrawStats::default()),
            marked: RefCell::new(None),
            drag: Cell::new(None),
        });
        unsafe {
            msg_send![super(this), initWithFrame: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(800.0, 600.0))]
        }
    }
}

/// Colors for a display line: syntax roles first, then locked labels.
fn line_colors(
    line: &DisplayLine,
    theme: &Theme,
    extra: &[(std::ops::Range<usize>, Role)],
) -> Vec<(std::ops::Range<usize>, (f64, f64, f64))> {
    let mut out: Vec<_> = line
        .roles
        .iter()
        .chain(extra)
        .filter_map(|(r, role)| Some((r.clone(), theme.role(*role)?)))
        .collect();
    if let Some(framing) = theme.role(Role::Framing) {
        out.extend(line.locked.iter().map(|r| (r.clone(), framing)));
    }
    out
}

fn draw_text(view: &TextView, tab: &Tab, dirty: NSRect) -> DrawStats {
    let doc = tab.doc.borrow();
    let theme = theme::for_view(view);
    let m = metrics();
    let font = font();
    fill(dirty, theme.background);
    let rope = doc.doc.buffer().rope();
    let lines = rope.len_lines();
    let left = text_left(lines, &m);
    let first = (dirty.origin.y / m.line_height).floor().max(0.0) as usize;
    let last = (((dirty.origin.y + dirty.size.height) / m.line_height).ceil() as usize)
        .min(lines.saturating_sub(1));
    let first_column = (((dirty.origin.x - left) / m.advance).floor().max(0.0) as usize)
        .saturating_sub(COLUMN_MARGIN);
    let last_column = (((dirty.origin.x + dirty.size.width - left) / m.advance)
        .ceil()
        .max(0.0) as usize)
        + COLUMN_MARGIN;

    let primary = doc.doc.selections().primary();
    let (caret_line, _) = doc.doc.line_col(primary.head);
    let mut stats = DrawStats::default();
    if lines == 0 || first > last {
        return stats;
    }
    for line in first..=last {
        let y = line as f64 * m.line_height;
        if line == caret_line {
            fill(
                NSRect::new(
                    NSPoint::new(dirty.origin.x, y),
                    NSSize::new(dirty.size.width, m.line_height),
                ),
                theme.current_line,
            );
        }
        let display = display_line(&doc, line);
        for locked in &display.locked {
            let x = left + locked.start as f64 * m.advance;
            fill(
                NSRect::new(
                    NSPoint::new(x, y + 1.0),
                    NSSize::new(locked.len() as f64 * m.advance, m.line_height - 2.0),
                ),
                theme.locked_background,
            );
        }
        draw_selections(&doc, line, &display, theme, &m, left, y);
        let end = display.width().min(last_column);
        if first_column >= end {
            continue;
        }
        let visible: String = display.chars[first_column..end].iter().collect();
        let colors: Vec<_> = line_colors(
            &display,
            theme,
            &crate::highlight::spans(&doc, line, &display),
        )
        .into_iter()
        .filter_map(|(r, c)| {
            let s = r.start.max(first_column);
            let e = r.end.min(end);
            (s < e).then(|| (s - first_column..e - first_column, c))
        })
        .collect();
        let text = attributed(&visible, &font, theme.text, &colors);
        text.drawAtPoint(NSPoint::new(
            left + first_column as f64 * m.advance,
            y + m.baseline_pad,
        ));
        stats.lines += 1;
        stats.max_columns = stats.max_columns.max(end - first_column);
    }
    draw_carets(&doc, first, last, theme, &m, left);
    if let Some(marked) = view.ivars().marked.borrow().as_ref() {
        let (line, column) = doc.doc.line_col(doc.doc.selections().primary().head);
        let column = display_line(&doc, line).column_of(column);
        let origin = NSPoint::new(
            left + column as f64 * m.advance,
            line as f64 * m.line_height,
        );
        let width = marked.chars().count() as f64 * m.advance;
        fill(
            NSRect::new(origin, NSSize::new(width, m.line_height)),
            theme.background,
        );
        let text = attributed(marked, &font, theme.text, &[]);
        unsafe {
            text.addAttribute_value_range(
                NSUnderlineStyleAttributeName,
                &*NSNumber::new_isize(1) as &AnyObject,
                NSRange::new(0, marked.encode_utf16().count()),
            );
        }
        text.drawAtPoint(NSPoint::new(origin.x, origin.y + m.baseline_pad));
    }
    stats
}

fn draw_selections(
    doc: &crate::doc_model::EditorDoc,
    line: usize,
    display: &DisplayLine,
    theme: &Theme,
    m: &Metrics,
    left: f64,
    y: f64,
) {
    let rope = doc.doc.buffer().rope();
    let line_start = rope.line_to_char(line);
    let line_end = doc.doc.line_text_end(line);
    for s in doc
        .doc
        .selections()
        .ranges()
        .iter()
        .filter(|s| !s.is_caret())
    {
        if s.max() < line_start || s.min() > line_end {
            continue;
        }
        let from = display.column_of(s.min().max(line_start) - line_start);
        let mut to = display.column_of(s.max().min(line_end) - line_start);
        if s.max() > line_end {
            to += 1;
        }
        if to > from {
            fill(
                NSRect::new(
                    NSPoint::new(left + from as f64 * m.advance, y),
                    NSSize::new((to - from) as f64 * m.advance, m.line_height),
                ),
                theme.selection,
            );
        }
    }
}

fn draw_carets(
    doc: &crate::doc_model::EditorDoc,
    first: usize,
    last: usize,
    theme: &Theme,
    m: &Metrics,
    left: f64,
) {
    for s in doc.doc.selections().ranges() {
        let (line, column) = doc.doc.line_col(s.head);
        if line < first || line > last {
            continue;
        }
        let display = display_line(doc, line);
        let x = left + display.column_of(column) as f64 * m.advance;
        fill(
            NSRect::new(
                NSPoint::new(x - 0.5, line as f64 * m.line_height),
                NSSize::new(2.0, m.line_height),
            ),
            theme.caret,
        );
    }
}

/// Resizes the text and gutter views to fit the document.
pub fn update_frames(tab: &Tab) {
    let m = metrics();
    let (lines, columns) = {
        let doc = tab.doc.borrow();
        (doc.doc.buffer().rope().len_lines(), doc.max_columns)
    };
    let visible = tab.scroll.contentSize();
    let height = (lines as f64 + BOTTOM_LINES) * m.line_height;
    let width = text_left(lines, &m) + (columns as f64 + 4.0) * m.advance;
    tab.view.setFrameSize(NSSize::new(
        width.max(visible.width),
        height.max(visible.height),
    ));
    tab.view.setNeedsDisplay(true);
}

/// Rectangle of a character position, in view coordinates.
pub fn rect_for(tab: &Tab, line: usize, column: usize) -> NSRect {
    let m = metrics();
    let lines = tab.doc.borrow().doc.buffer().rope().len_lines();
    NSRect::new(
        NSPoint::new(
            text_left(lines, &m) + column as f64 * m.advance,
            line as f64 * m.line_height,
        ),
        NSSize::new(m.advance, m.line_height),
    )
}

pub fn scroll_to(tab: &Rc<Tab>, line: usize, column: usize) {
    let mut rect = rect_for(tab, line, column);
    let visible = tab.scroll.contentSize();
    rect.size.height = rect.size.height.max(visible.height * 0.5);
    rect.origin.y = (rect.origin.y - visible.height * 0.25).max(0.0);
    tab.view.scrollRectToVisible(rect);
}

fn draw_gutter(view: &TextView, tab: &Tab, dirty: NSRect) {
    let doc = tab.doc.borrow();
    let theme = theme::for_view(view);
    let m = metrics();
    let font = font();
    let visible = view.visibleRect();
    let lines = doc.doc.buffer().rope().len_lines();
    let width = gutter_width(lines, &m);
    let left = visible.origin.x;
    if dirty.origin.x > left + width {
        return;
    }
    fill(
        NSRect::new(
            NSPoint::new(left, dirty.origin.y),
            NSSize::new(width, dirty.size.height),
        ),
        theme.gutter_background,
    );
    let first = (dirty.origin.y / m.line_height).floor().max(0.0) as usize;
    let last = (((dirty.origin.y + dirty.size.height) / m.line_height).ceil() as usize)
        .min(lines.saturating_sub(1));
    let bookmarks = doc.doc.bookmarked_lines();
    for line in first..=last.max(first) {
        if line >= lines {
            break;
        }
        let y = line as f64 * m.line_height;
        if bookmarks.binary_search(&line).is_ok() {
            let size = (m.line_height * 0.5).min(9.0);
            color(theme.bookmark).setFill();
            NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
                NSPoint::new(left + 5.0, y + (m.line_height - size) / 2.0),
                NSSize::new(size, size),
            ))
            .fill();
        }
        let number = (line + 1).to_string();
        let x = left + width - 6.0 - number.len() as f64 * m.advance;
        attributed(&number, &font, theme.gutter_text, &[])
            .drawAtPoint(NSPoint::new(x, y + m.baseline_pad));
    }
}
