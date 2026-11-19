//! The find and replace bar above the text.
//!
//! Large documents are searched on a background thread so typing never waits.
//! Results are applied only if the document has not changed since the search began.

use crate::app::{Tab, tab_by_id, with_app};
use editor_core::{Selection, SelectionSet};
use editor_search::{
    Query, SearchOptions, SearchText, expand_replacement, find_all, find_next, replace_all,
};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSButton, NSColor, NSControlStateValueOff, NSControlStateValueOn,
    NSFont, NSTextField, NSView,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

const BACKGROUND_CHARS: usize = 2_000_000;
const ROW: f64 = 30.0;

#[derive(Clone, Default)]
pub struct FindState {
    pub pattern: String,
    pub replace: String,
    pub options: SearchOptions,
    pub in_selection: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(isize)]
pub enum Button {
    Previous,
    Next,
    FindAll,
    Count,
    BookmarkAll,
    Replace,
    ReplaceAll,
    Stop,
    Close,
    Option,
}

impl Button {
    const ALL: [Button; 10] = [
        Button::Previous,
        Button::Next,
        Button::FindAll,
        Button::Count,
        Button::BookmarkAll,
        Button::Replace,
        Button::ReplaceAll,
        Button::Stop,
        Button::Close,
        Button::Option,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Button::Previous => "Previous",
            Button::Next => "Next",
            Button::FindAll => "Find All",
            Button::Count => "Count",
            Button::BookmarkAll => "Bookmark All",
            Button::Replace => "Replace",
            Button::ReplaceAll => "Replace All",
            Button::Stop => "Stop",
            Button::Close => "Done",
            Button::Option => "",
        }
    }

    pub fn from_title(title: &str) -> Option<Button> {
        Button::ALL.into_iter().find(|b| b.title() == title)
    }
}

pub struct FindBar {
    pub view: Retained<NSView>,
    pub find: Retained<NSTextField>,
    pub replace: Retained<NSTextField>,
    replace_row: Vec<Retained<NSView>>,
    pub regex: Retained<NSButton>,
    pub ignore_case: Retained<NSButton>,
    pub whole_word: Retained<NSButton>,
    pub wrap: Retained<NSButton>,
    pub in_selection: Retained<NSButton>,
    pub status: Retained<NSTextField>,
    _controller: Retained<FindController>,
    pub replace_visible: bool,
    pub scope: Option<Range<usize>>,
}

pub struct ControllerIvars {
    tab_id: String,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ERSFindController"]
    #[ivars = ControllerIvars]
    pub struct FindController;

    unsafe impl NSObjectProtocol for FindController {}

    impl FindController {
        #[unsafe(method(findAction:))]
        fn find_action(&self, sender: &NSButton) {
            if let (Some(tab), Some(button)) = (tab_by_id(&self.ivars().tab_id), Button::ALL.get(sender.tag() as usize)) {
                press(self.mtm(), &tab, *button);
            }
        }

        #[unsafe(method(findFieldAction:))]
        fn find_field_action(&self, _sender: &AnyObject) {
            if let Some(tab) = tab_by_id(&self.ivars().tab_id) {
                press(self.mtm(), &tab, Button::Next);
            }
        }
    }
);

impl FindController {
    fn new(mtm: MainThreadMarker, tab_id: &str) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ControllerIvars {
            tab_id: tab_id.to_string(),
        });
        unsafe { msg_send![super(this), init] }
    }
}

fn button(mtm: MainThreadMarker, target: &FindController, kind: Button) -> Retained<NSButton> {
    let b = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str(kind.title()),
            Some(target),
            Some(sel!(findAction:)),
            mtm,
        )
    };
    b.setTag(kind as isize);
    b.setControlSize(objc2_app_kit::NSControlSize::Small);
    b.setFont(Some(&NSFont::systemFontOfSize(11.0)));
    b
}

fn checkbox(
    mtm: MainThreadMarker,
    target: &FindController,
    title: &str,
    on: bool,
) -> Retained<NSButton> {
    let b = unsafe {
        NSButton::checkboxWithTitle_target_action(
            &NSString::from_str(title),
            Some(target),
            Some(sel!(findAction:)),
            mtm,
        )
    };
    b.setTag(Button::Option as isize);
    b.setState(if on {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    });
    b.setFont(Some(&NSFont::systemFontOfSize(11.0)));
    b
}

fn place(view: &NSView, x: f64, y: f64, width: f64) {
    let height = view.fittingSize().height.max(22.0);
    view.setFrame(NSRect::new(
        NSPoint::new(x, y + (ROW - height) / 2.0),
        NSSize::new(width, height),
    ));
}

fn row_of_buttons(parent: &NSView, buttons: &[&NSButton], mut x: f64, y: f64) -> f64 {
    for b in buttons {
        let width = b.fittingSize().width.max(60.0);
        place(b, x, y, width);
        parent.addSubview(b);
        x += width + 4.0;
    }
    x
}

fn build(mtm: MainThreadMarker, tab: &Tab, state: &FindState) -> FindBar {
    let controller = FindController::new(mtm, &tab.id);
    let view = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(900.0, ROW * 3.0)),
    );
    view.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewMinYMargin,
    );
    let field = |placeholder: &str, value: &str| {
        let f = NSTextField::textFieldWithString(&NSString::from_str(value), mtm);
        f.setPlaceholderString(Some(&NSString::from_str(placeholder)));
        f.setFont(Some(&NSFont::monospacedSystemFontOfSize_weight(
            12.0,
            unsafe { objc2_app_kit::NSFontWeightRegular },
        )));
        f
    };
    let find = field("Find", &state.pattern);
    unsafe {
        find.setTarget(Some(&controller));
        find.setAction(Some(sel!(findFieldAction:)));
    }
    let replace = field("Replace", &state.replace);
    let (top, middle, bottom) = (ROW * 2.0, ROW, 0.0);
    place(&find, 10.0, top, 280.0);
    view.addSubview(&find);
    let mut buttons = Vec::new();
    for kind in [
        Button::Previous,
        Button::Next,
        Button::FindAll,
        Button::Count,
        Button::BookmarkAll,
    ] {
        buttons.push(button(mtm, &controller, kind));
    }
    row_of_buttons(
        &view,
        &buttons.iter().map(|b| &**b).collect::<Vec<_>>(),
        300.0,
        top,
    );
    place(&replace, 10.0, middle, 280.0);
    view.addSubview(&replace);
    let replace_buttons = [
        button(mtm, &controller, Button::Replace),
        button(mtm, &controller, Button::ReplaceAll),
    ];
    row_of_buttons(
        &view,
        &replace_buttons.iter().map(|b| &**b).collect::<Vec<_>>(),
        300.0,
        middle,
    );
    let options = &state.options;
    let regex = checkbox(mtm, &controller, "Regular expression", options.regex);
    let ignore_case = checkbox(mtm, &controller, "Ignore case", options.ignore_case);
    let whole_word = checkbox(mtm, &controller, "Whole word", options.whole_word);
    let wrap = checkbox(mtm, &controller, "Wrap around", options.wrap);
    let in_selection = checkbox(mtm, &controller, "In selection", state.in_selection);
    let mut x = 10.0;
    for b in [&regex, &ignore_case, &whole_word, &wrap, &in_selection] {
        let width = b.fittingSize().width;
        place(b, x, bottom, width);
        view.addSubview(b);
        x += width + 12.0;
    }
    let stop = button(mtm, &controller, Button::Stop);
    let close = button(mtm, &controller, Button::Close);
    let x = row_of_buttons(&view, &[&stop, &close], x, bottom);
    let status = NSTextField::labelWithString(&NSString::from_str(""), mtm);
    status.setFont(Some(&NSFont::systemFontOfSize(11.0)));
    status.setTextColor(Some(&NSColor::secondaryLabelColor()));
    place(&status, x + 8.0, bottom, 400.0);
    status.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
    view.addSubview(&status);
    let mut replace_row: Vec<Retained<NSView>> =
        vec![Retained::into_super(Retained::into_super(replace.clone()))];
    replace_row.extend(
        replace_buttons
            .iter()
            .map(|b| Retained::into_super(Retained::into_super(b.clone()))),
    );
    FindBar {
        view,
        find,
        replace,
        replace_row,
        regex,
        ignore_case,
        whole_word,
        wrap,
        in_selection,
        status,
        _controller: controller,
        replace_visible: true,
        scope: None,
    }
}

fn bar_height() -> f64 {
    ROW * 3.0 + 6.0
}

/// Lays out the find bar, scroll view and status bar inside the window.
pub fn layout(tab: &Tab) {
    let Some(container) = tab.window.contentView() else {
        return;
    };
    let bounds = container.bounds();
    let status_height = crate::window::STATUS_HEIGHT;
    let bar_h = match tab.find.borrow().as_ref().filter(|b| !b.view.isHidden()) {
        Some(bar) => {
            let height = bar_height();
            bar.view.setFrame(NSRect::new(
                NSPoint::new(0.0, bounds.size.height - height),
                NSSize::new(bounds.size.width, height),
            ));
            for v in &bar.replace_row {
                v.setHidden(!bar.replace_visible);
            }
            height
        }
        None => 0.0,
    };
    tab.scroll.setFrame(NSRect::new(
        NSPoint::new(0.0, status_height),
        NSSize::new(
            bounds.size.width,
            bounds.size.height - status_height - bar_h,
        ),
    ));
    crate::text_view::update_frames(tab);
}

fn state() -> FindState {
    with_app(|a| a.find.clone())
}

fn read_bar(tab: &Tab) -> FindState {
    let mut state = state();
    if let Some(bar) = tab.find.borrow().as_ref() {
        let on = |b: &NSButton| b.state() == NSControlStateValueOn;
        state.pattern = bar.find.stringValue().to_string();
        state.replace = bar.replace.stringValue().to_string();
        state.options = SearchOptions {
            regex: on(&bar.regex),
            ignore_case: on(&bar.ignore_case),
            whole_word: on(&bar.whole_word),
            wrap: on(&bar.wrap),
        };
        state.in_selection = on(&bar.in_selection);
    }
    with_app(|a| a.find = state.clone());
    state
}

/// Shows the bar, with or without the replace row.
pub fn show(mtm: MainThreadMarker, tab: &Rc<Tab>, replace: bool) {
    if tab.find.borrow().is_none() {
        let bar = build(mtm, tab, &state());
        if let Some(container) = tab.window.contentView() {
            container.addSubview(&bar.view);
        }
        *tab.find.borrow_mut() = Some(bar);
    }
    {
        let selection = tab.doc.borrow().doc.selections().primary();
        let mut find = tab.find.borrow_mut();
        let bar = find.as_mut().expect("bar");
        bar.view.setHidden(false);
        bar.replace_visible = replace;
        bar.scope = (!selection.is_caret()).then(|| selection.min()..selection.max());
    }
    layout(tab);
    let field = tab.find.borrow().as_ref().map(|b| b.find.clone());
    if let Some(field) = field {
        tab.window.makeFirstResponder(Some(&field));
    }
}

pub fn hide(tab: &Tab) {
    cancel(tab);
    let hidden = match tab.find.borrow().as_ref() {
        Some(bar) if !bar.view.isHidden() => {
            bar.view.setHidden(true);
            true
        }
        _ => false,
    };
    if hidden {
        layout(tab);
        tab.window.makeFirstResponder(Some(&tab.view));
    }
}

fn set_status(tab: &Tab, message: &str) {
    match tab.find.borrow().as_ref().filter(|b| !b.view.isHidden()) {
        Some(bar) => bar.status.setStringValue(&NSString::from_str(message)),
        None => *tab.notice.borrow_mut() = Some(message.to_string()),
    }
    crate::status_bar::refresh(tab);
}

pub fn status_text(tab: &Tab) -> Option<String> {
    tab.find
        .borrow()
        .as_ref()
        .map(|b| b.status.stringValue().to_string())
}

fn compile(tab: &Tab, state: &FindState) -> Option<Query> {
    match Query::compile(&state.pattern, &state.options) {
        Ok(query) => Some(query),
        Err(error) => {
            set_status(tab, &format!("Invalid pattern: {}", error.message));
            objc2_app_kit::NSBeep();
            None
        }
    }
}

fn snapshot(tab: &Tab) -> (Arc<SearchText>, u64) {
    let doc = tab.doc.borrow();
    let revision = doc.doc.buffer().revision();
    if let Some((cached, text)) = tab.search_cache.borrow().as_ref()
        && *cached == revision
    {
        return (text.clone(), revision);
    }
    let text = Arc::new(SearchText::new(
        doc.doc.buffer().rope(),
        doc.doc.buffer().locked(),
    ));
    *tab.search_cache.borrow_mut() = Some((revision, text.clone()));
    (text, revision)
}

fn scope(tab: &Tab, state: &FindState) -> Option<Range<usize>> {
    if !state.in_selection {
        return None;
    }
    let bar_scope = tab.find.borrow().as_ref().and_then(|b| b.scope.clone());
    bar_scope.or_else(|| {
        let s = tab.doc.borrow().doc.selections().primary();
        (!s.is_caret()).then(|| s.min()..s.max())
    })
}

fn select(tab: &Rc<Tab>, ranges: &[Range<usize>]) {
    let selections: Vec<Selection> = ranges
        .iter()
        .map(|r| Selection {
            anchor: r.start,
            head: r.end,
        })
        .collect();
    if selections.is_empty() {
        return;
    }
    tab.doc
        .borrow_mut()
        .doc
        .set_selections(SelectionSet::from_ranges(selections, 0));
    crate::input::after_change(tab, false);
}

pub fn find_next_match(tab: &Rc<Tab>, backward: bool) {
    let state = read_bar(tab);
    let Some(query) = compile(tab, &state) else {
        return;
    };
    let (text, _) = snapshot(tab);
    let primary = tab.doc.borrow().doc.selections().primary();
    let from = if backward {
        primary.min()
    } else {
        primary.max()
    };
    match find_next(
        &query,
        &text,
        from,
        backward,
        state.options.wrap,
        scope(tab, &state),
    ) {
        Some(found) => {
            select(tab, std::slice::from_ref(&found));
            set_status(tab, "");
        }
        None => {
            set_status(tab, &format!("No more matches for “{}”.", state.pattern));
            objc2_app_kit::NSBeep();
        }
    }
}

fn replace_one(tab: &Rc<Tab>) {
    let state = read_bar(tab);
    let Some(query) = compile(tab, &state) else {
        return;
    };
    let (text, _) = snapshot(tab);
    let primary = tab.doc.borrow().doc.selections().primary();
    let current = find_next(
        &query,
        &text,
        primary.min(),
        false,
        false,
        scope(tab, &state),
    );
    if let Some(found) = current.filter(|f| f.start == primary.min() && f.end == primary.max()) {
        let replacement = expand_replacement(&query, &text, &found, &state.replace);
        let batch = editor_core::EditBatch(vec![editor_core::Edit {
            range: found,
            text: replacement,
        }]);
        crate::input::edit(tab, |d| d.apply_batch(batch, "Replace"));
    }
    find_next_match(tab, false);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Bulk {
    FindAll,
    Count,
    ReplaceAll,
    BookmarkAll,
}

enum Outcome {
    Matches(Vec<Range<usize>>),
    Replacements(editor_core::EditBatch),
}

fn work(
    kind: Bulk,
    query: &Query,
    text: &SearchText,
    state: &FindState,
    scope: Option<Range<usize>>,
    cancel: &AtomicBool,
) -> Option<Outcome> {
    match kind {
        Bulk::ReplaceAll => replace_all(query, text, &state.replace, scope, cancel)
            .ok()
            .map(Outcome::Replacements),
        _ => find_all(query, text, scope, cancel)
            .ok()
            .map(Outcome::Matches),
    }
}

fn finish(tab: &Rc<Tab>, kind: Bulk, state: &FindState, revision: u64, outcome: Outcome) {
    *tab.search_cancel.borrow_mut() = None;
    if tab.doc.borrow().doc.buffer().revision() != revision {
        set_status(tab, "The document changed during the search. Run it again.");
        return;
    }
    match (kind, outcome) {
        (Bulk::Count, Outcome::Matches(m)) => set_status(
            tab,
            &format!("{} matches for “{}”.", m.len(), state.pattern),
        ),
        (Bulk::FindAll, Outcome::Matches(m)) => {
            set_status(
                tab,
                &format!("{} matches for “{}”.", m.len(), state.pattern),
            );
            select(tab, &m);
        }
        (Bulk::BookmarkAll, Outcome::Matches(m)) => {
            let lines = tab.doc.borrow_mut().doc.bookmark_lines_of(&m);
            tab.view.setNeedsDisplay(true);
            crate::session_driver::note_change(tab);
            set_status(
                tab,
                &format!(
                    "Bookmarked {lines} lines ({} matches for “{}”).",
                    m.len(),
                    state.pattern
                ),
            );
        }
        (Bulk::ReplaceAll, Outcome::Replacements(batch)) => {
            let count = batch.0.len();
            if count > 0 && crate::input::edit(tab, |d| d.apply_batch(batch, "Replace All")) {
                set_status(tab, &format!("{count} replacements made."));
            } else if count == 0 {
                set_status(tab, &format!("No matches for “{}”.", state.pattern));
            }
        }
        _ => {}
    }
}

fn run(tab: &Rc<Tab>, kind: Bulk) {
    let state = read_bar(tab);
    let Some(query) = compile(tab, &state) else {
        return;
    };
    cancel(tab);
    let (text, revision) = snapshot(tab);
    let scope = scope(tab, &state);
    let big = text_len(tab) > BACKGROUND_CHARS;
    if !big {
        if let Some(outcome) = work(kind, &query, &text, &state, scope, &AtomicBool::new(false)) {
            finish(tab, kind, &state, revision, outcome);
        }
        return;
    }
    let flag = Arc::new(AtomicBool::new(false));
    *tab.search_cancel.borrow_mut() = Some(flag.clone());
    set_status(tab, "Searching…");
    let id = tab.id.clone();
    let state_for_thread = state.clone();
    std::thread::spawn(move || {
        let outcome = work(kind, &query, &text, &state_for_thread, scope, &flag);
        if flag.load(Ordering::Relaxed) {
            return;
        }
        let deliver = SendOutcome(outcome);
        dispatch2::DispatchQueue::main().exec_async(move || {
            let outcome = deliver;
            if let (Some(tab), Some(outcome)) = (tab_by_id(&id), outcome.0) {
                finish(&tab, kind, &state_for_thread, revision, outcome);
            }
        });
    });
}

struct SendOutcome(Option<Outcome>);
// SAFETY: outcomes hold only plain Rust data (ranges and strings).
unsafe impl Send for SendOutcome {}

fn text_len(tab: &Tab) -> usize {
    tab.doc.borrow().doc.buffer().rope().len_chars()
}

pub fn cancel(tab: &Tab) {
    if let Some(flag) = tab.search_cancel.borrow_mut().take() {
        flag.store(true, Ordering::Relaxed);
        set_status(tab, "Search stopped.");
    }
}

pub fn is_running(tab: &Tab) -> bool {
    tab.search_cancel.borrow().is_some()
}

/// Runs a find bar button.
pub fn press(mtm: MainThreadMarker, tab: &Rc<Tab>, button: Button) {
    match button {
        Button::Previous => find_next_match(tab, true),
        Button::Next => find_next_match(tab, false),
        Button::FindAll => run(tab, Bulk::FindAll),
        Button::Count => run(tab, Bulk::Count),
        Button::BookmarkAll => run(tab, Bulk::BookmarkAll),
        Button::Replace => replace_one(tab),
        Button::ReplaceAll => run(tab, Bulk::ReplaceAll),
        Button::Stop => cancel(tab),
        Button::Close => hide(tab),
        Button::Option => {
            let selection = tab.doc.borrow().doc.selections().primary();
            if let Some(bar) = tab.find.borrow_mut().as_mut() {
                bar.scope = (!selection.is_caret())
                    .then(|| selection.min()..selection.max())
                    .or(bar.scope.clone());
            }
            read_bar(tab);
        }
    }
    let _ = mtm;
}

/// Puts the selected text into the find field as a literal search.
pub fn use_selection(mtm: MainThreadMarker, tab: &Rc<Tab>) {
    let text = tab.doc.borrow().doc.selected_text();
    if text.is_empty() {
        return;
    }
    with_app(|a| {
        a.find.pattern = text.clone();
        a.find.options.regex = false;
    });
    if let Some(bar) = tab.find.borrow().as_ref() {
        bar.find.setStringValue(&NSString::from_str(&text));
        bar.regex.setState(NSControlStateValueOff);
    }
    let _ = mtm;
}

/// Sets bar fields from a script.
pub fn set_fields(tab: &Tab, pattern: &str, replace: Option<&str>, flags: [Option<bool>; 5]) {
    let find = tab.find.borrow();
    let Some(bar) = find.as_ref() else { return };
    bar.find.setStringValue(&NSString::from_str(pattern));
    if let Some(replace) = replace {
        bar.replace.setStringValue(&NSString::from_str(replace));
    }
    for (button, value) in [
        &bar.regex,
        &bar.ignore_case,
        &bar.whole_word,
        &bar.wrap,
        &bar.in_selection,
    ]
    .into_iter()
    .zip(flags)
    {
        if let Some(on) = value {
            button.setState(if on {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
        }
    }
}

pub fn bulk_from_menu(tab: &Rc<Tab>, which: &str) {
    match which {
        "find_all" => run(tab, Bulk::FindAll),
        "count" => run(tab, Bulk::Count),
        "replace_all" => run(tab, Bulk::ReplaceAll),
        _ => run(tab, Bulk::BookmarkAll),
    }
}

impl Drop for FindBar {
    fn drop(&mut self) {
        self.view.removeFromSuperview();
    }
}
