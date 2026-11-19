//! The menu bar. Every custom command is a `MenuAction` sent to the app delegate.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::sel;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSMenu, NSMenuItem};
use objc2_foundation::NSString;

macro_rules! actions {
    ($($name:ident),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(isize)]
        pub enum MenuAction { $($name),* }
        impl MenuAction {
            pub const ALL: &'static [MenuAction] = &[$(MenuAction::$name),*];
            pub fn from_tag(tag: isize) -> Option<MenuAction> { Self::ALL.get(usize::try_from(tag).ok()?).copied() }
            pub fn name(self) -> &'static str { match self { $(MenuAction::$name => stringify!($name)),* } }
            pub fn from_name(name: &str) -> Option<MenuAction> { Self::ALL.iter().copied().find(|a| a.name() == name) }
        }
    };
}

actions!(
    About,
    Preferences,
    ClearSessionData,
    NewTab,
    Open,
    CloseTab,
    CloseWindow,
    Save,
    SaveAs,
    Revert,
    Indent,
    Outdent,
    GoToLine,
    BiggerText,
    SmallerText,
    ActualSize,
    WordWrap,
    FoldBlock,
    UnfoldBlock,
    FoldAll,
    UnfoldAll,
    Find,
    FindAndReplace,
    FindNext,
    FindPrevious,
    UseSelectionForFind,
    FindAll,
    ReplaceAll,
    CountMatches,
    ToggleBookmark,
    NextBookmark,
    PreviousBookmark,
    BookmarkAllMatches,
    ClearBookmarks,
    InverseBookmarks,
    CutBookmarkedLines,
    CopyBookmarkedLines,
    PasteToBookmarkedLines,
    RemoveBookmarkedLines,
    RemoveUnbookmarkedLines,
    ShowSessionFolder,
    SetLanguage,
    ReopenWithEncoding,
    OpenRecent,
    ClearRecent,
);

pub const F2: char = '\u{F705}';

const CMD: NSEventModifierFlags = NSEventModifierFlags::Command;
const SHIFT: NSEventModifierFlags = NSEventModifierFlags::Shift;
const OPT: NSEventModifierFlags = NSEventModifierFlags::Option;
const NONE: NSEventModifierFlags = NSEventModifierFlags::empty();

enum Item {
    Action(&'static str, MenuAction, &'static str, NSEventModifierFlags),
    Standard(&'static str, Sel, &'static str, NSEventModifierFlags),
    Submenu(&'static str, Vec<Item>),
    Separator,
}

fn key(c: char) -> String {
    c.to_string()
}

fn build_item(mtm: MainThreadMarker, item: &Item, target: &AnyObject) -> Retained<NSMenuItem> {
    match item {
        Item::Separator => NSMenuItem::separatorItem(mtm),
        Item::Submenu(title, items) => {
            let menu_item = make(mtm, title, None, "", NONE);
            let submenu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
            if matches!(*title, "Open Recent" | "Language" | "Reopen with Encoding")
                && let Some(delegate) = target.downcast_ref::<crate::app_delegate::AppDelegate>()
            {
                submenu.setDelegate(Some(objc2::runtime::ProtocolObject::from_ref(delegate)));
            }
            for child in items {
                submenu.addItem(&build_item(mtm, child, target));
            }
            menu_item.setSubmenu(Some(&submenu));
            menu_item
        }
        Item::Standard(title, action, key, mods) => make(mtm, title, Some(*action), key, *mods),
        Item::Action(title, action, key, mods) => {
            let menu_item = make(mtm, title, Some(sel!(menuAction:)), key, *mods);
            unsafe { menu_item.setTarget(Some(target)) };
            menu_item.setTag(*action as isize);
            menu_item
        }
    }
}

fn make(
    mtm: MainThreadMarker,
    title: &str,
    action: Option<Sel>,
    key: &str,
    mods: NSEventModifierFlags,
) -> Retained<NSMenuItem> {
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            action,
            &NSString::from_str(key),
        )
    };
    if !key.is_empty() {
        item.setKeyEquivalentModifierMask(mods);
    }
    item
}

/// Builds and installs the menu bar.
pub fn install(mtm: MainThreadMarker, target: &AnyObject) {
    use Item::*;
    use MenuAction as A;
    let name = crate::config::APP.display_name;
    let about: &'static str = Box::leak(format!("About {name}").into_boxed_str());
    let hide: &'static str = Box::leak(format!("Hide {name}").into_boxed_str());
    let quit: &'static str = Box::leak(format!("Quit {name}").into_boxed_str());
    let f2 = key(F2);
    let f2: &'static str = Box::leak(f2.into_boxed_str());
    let menus = vec![
        Submenu(
            name,
            vec![
                Action(about, A::About, "", NONE),
                Separator,
                Action("Settings…", A::Preferences, ",", CMD),
                Action("Clear Session Data…", A::ClearSessionData, "", NONE),
                Separator,
                Standard(hide, sel!(hide:), "h", CMD),
                Standard("Hide Others", sel!(hideOtherApplications:), "h", CMD | OPT),
                Standard("Show All", sel!(unhideAllApplications:), "", NONE),
                Separator,
                Standard(quit, sel!(terminate:), "q", CMD),
            ],
        ),
        Submenu(
            "File",
            vec![
                Action("New Tab", A::NewTab, "n", CMD),
                Action("Open…", A::Open, "o", CMD),
                Submenu(
                    "Open Recent",
                    vec![Action("Clear Menu", A::ClearRecent, "", NONE)],
                ),
                Separator,
                Action("Close Tab", A::CloseTab, "w", CMD),
                Action("Close Window", A::CloseWindow, "w", CMD | SHIFT),
                Action("Save", A::Save, "s", CMD),
                Action("Save As…", A::SaveAs, "s", CMD | SHIFT),
                Action("Revert to Saved", A::Revert, "", NONE),
                Separator,
                Submenu("Reopen with Encoding", Vec::new()),
            ],
        ),
        Submenu(
            "Edit",
            vec![
                Standard("Undo", sel!(undo:), "z", CMD),
                Standard("Redo", sel!(redo:), "z", CMD | SHIFT),
                Separator,
                Standard("Cut", sel!(cut:), "x", CMD),
                Standard("Copy", sel!(copy:), "c", CMD),
                Standard("Paste", sel!(paste:), "v", CMD),
                Standard("Select All", sel!(selectAll:), "a", CMD),
                Separator,
                Action("Indent", A::Indent, "]", CMD),
                Action("Outdent", A::Outdent, "[", CMD),
                Action("Go to Line…", A::GoToLine, "l", CMD),
            ],
        ),
        Submenu(
            "View",
            vec![
                Action("Bigger Text", A::BiggerText, "+", CMD),
                Action("Smaller Text", A::SmallerText, "-", CMD),
                Action("Actual Size", A::ActualSize, "0", CMD),
                Separator,
                Action("Word Wrap", A::WordWrap, "w", CMD | OPT),
                Submenu("Language", Vec::new()),
                Separator,
                Action("Fold Block", A::FoldBlock, "[", CMD | OPT),
                Action("Unfold Block", A::UnfoldBlock, "]", CMD | OPT),
                Action("Fold All", A::FoldAll, "", NONE),
                Action("Unfold All", A::UnfoldAll, "", NONE),
            ],
        ),
        Submenu(
            "Search",
            vec![
                Action("Find…", A::Find, "f", CMD),
                Action("Find and Replace…", A::FindAndReplace, "f", CMD | OPT),
                Action("Find Next", A::FindNext, "g", CMD),
                Action("Find Previous", A::FindPrevious, "g", CMD | SHIFT),
                Action("Use Selection for Find", A::UseSelectionForFind, "e", CMD),
                Separator,
                Action("Find All", A::FindAll, "", NONE),
                Action("Replace All", A::ReplaceAll, "", NONE),
                Action("Count Matches", A::CountMatches, "", NONE),
            ],
        ),
        Submenu(
            "Bookmarks",
            vec![
                Action("Toggle Bookmark", A::ToggleBookmark, f2, CMD),
                Action("Next Bookmark", A::NextBookmark, f2, NONE),
                Action("Previous Bookmark", A::PreviousBookmark, f2, SHIFT),
                Action("Bookmark All Matches", A::BookmarkAllMatches, f2, CMD | OPT),
                Action("Clear All Bookmarks", A::ClearBookmarks, f2, CMD | SHIFT),
                Action("Inverse Bookmarks", A::InverseBookmarks, "", NONE),
                Separator,
                Action("Cut Bookmarked Lines", A::CutBookmarkedLines, "", NONE),
                Action("Copy Bookmarked Lines", A::CopyBookmarkedLines, "", NONE),
                Action(
                    "Paste to (Replace) Bookmarked Lines",
                    A::PasteToBookmarkedLines,
                    "",
                    NONE,
                ),
                Action(
                    "Remove Bookmarked Lines",
                    A::RemoveBookmarkedLines,
                    "",
                    NONE,
                ),
                Action(
                    "Remove Unbookmarked Lines",
                    A::RemoveUnbookmarkedLines,
                    "",
                    NONE,
                ),
            ],
        ),
        Submenu(
            "Window",
            vec![
                Standard("Minimize", sel!(performMiniaturize:), "m", CMD),
                Standard("Zoom", sel!(performZoom:), "", NONE),
                Separator,
                Standard("Bring All to Front", sel!(arrangeInFront:), "", NONE),
            ],
        ),
        Submenu(
            "Help",
            vec![Action(
                "Show Session Folder",
                A::ShowSessionFolder,
                "",
                NONE,
            )],
        ),
    ];
    let bar = NSMenu::new(mtm);
    for menu in &menus {
        bar.addItem(&build_item(mtm, menu, target));
    }
    let app = NSApplication::sharedApplication(mtm);
    app.setMainMenu(Some(&bar));
    if let Some(window_menu) = bar
        .itemWithTitle(&NSString::from_str("Window"))
        .and_then(|i| i.submenu())
    {
        app.setWindowsMenu(Some(&window_menu));
    }
    if let Some(help_menu) = bar
        .itemWithTitle(&NSString::from_str("Help"))
        .and_then(|i| i.submenu())
    {
        app.setHelpMenu(Some(&help_menu));
    }
}

/// Titles of the top-level menus after the app menu.
pub fn titles(mtm: MainThreadMarker) -> Vec<String> {
    let Some(bar) = NSApplication::sharedApplication(mtm).mainMenu() else {
        return Vec::new();
    };
    (1..bar.numberOfItems())
        .filter_map(|i| bar.itemAtIndex(i))
        .map(|i| i.title().to_string())
        .collect()
}

/// Finds a menu item by action, for tests and validation.
pub fn find_item(mtm: MainThreadMarker, action: MenuAction) -> Option<Retained<NSMenuItem>> {
    fn search(menu: &NSMenu, action: MenuAction) -> Option<Retained<NSMenuItem>> {
        for i in 0..menu.numberOfItems() {
            let item = menu.itemAtIndex(i)?;
            if item.action() == Some(sel!(menuAction:)) && item.tag() == action as isize {
                return Some(item);
            }
            if let Some(sub) = item.submenu()
                && let Some(found) = search(&sub, action)
            {
                return Some(found);
            }
        }
        None
    }
    let bar = NSApplication::sharedApplication(mtm).mainMenu()?;
    search(&bar, action)
}

/// Fills the Open Recent menu each time it opens.
pub fn update_dynamic_menu(mtm: MainThreadMarker, menu: &NSMenu, target: &AnyObject) {
    let title = menu.title().to_string();
    if title == "Language" || title == "Reopen with Encoding" {
        menu.removeAllItems();
        let tab = crate::window::current_tab(mtm);
        let (action, names, current): (MenuAction, Vec<String>, Option<String>) =
            if title == "Language" {
                (
                    MenuAction::SetLanguage,
                    editor_syntax::Language::ALL
                        .iter()
                        .map(|l| l.name().to_string())
                        .collect(),
                    tab.map(|t| t.doc.borrow().language.name().to_string()),
                )
            } else {
                (
                    MenuAction::ReopenWithEncoding,
                    crate::view_actions::REOPEN_ENCODINGS
                        .iter()
                        .map(|e| e.to_string())
                        .collect(),
                    tab.map(|t| crate::status_bar::encoding_name(&t.doc.borrow().encoding)),
                )
            };
        for name in names {
            let item = make(mtm, &name, Some(sel!(menuAction:)), "", NONE);
            unsafe {
                item.setTarget(Some(target));
                item.setRepresentedObject(Some(&NSString::from_str(&name)));
            }
            item.setTag(action as isize);
            if current.as_deref() == Some(name.as_str()) {
                item.setState(objc2_app_kit::NSControlStateValueOn);
            }
            menu.addItem(&item);
        }
        return;
    }
    if title != "Open Recent" {
        return;
    }
    menu.removeAllItems();
    let recent = crate::document_controller::recent_files();
    for path in &recent {
        let title = path.file_name().map_or_else(
            || path.to_string_lossy().into_owned(),
            |n| n.to_string_lossy().into_owned(),
        );
        let item = make(mtm, &title, Some(sel!(menuAction:)), "", NONE);
        unsafe {
            item.setTarget(Some(target));
            item.setRepresentedObject(Some(&NSString::from_str(&path.to_string_lossy())));
        }
        item.setTag(MenuAction::OpenRecent as isize);
        item.setToolTip(Some(&NSString::from_str(&path.to_string_lossy())));
        menu.addItem(&item);
    }
    if !recent.is_empty() {
        menu.addItem(&NSMenuItem::separatorItem(mtm));
    }
    menu.addItem(&build_item(
        mtm,
        &Item::Action("Clear Menu", MenuAction::ClearRecent, "", NONE),
        target,
    ));
}
