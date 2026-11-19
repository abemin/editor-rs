//! Scripted runs of the real app, used by `tests/ui.rs`.
//!
//! Steps run one at a time on the main thread with a short pause between them.
//! Failures are collected into a JSON report, then the app quits.

use crate::app::args;
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, msg_send};
use objc2_app_kit::{
    NSApplication, NSBitmapImageFileType, NSEvent, NSEventModifierFlags, NSEventType,
};
use objc2_foundation::{NSDictionary, NSPoint, NSRange, NSString, NSTimer};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::path::Path;

const STEP_DELAY: f64 = 0.05;

struct Runner {
    steps: Vec<Value>,
    index: usize,
    failures: Vec<Value>,
    notes: Vec<Value>,
    generated: std::collections::HashMap<String, Vec<u8>>,
    saved_clipboard: Option<String>,
    render_stats: crate::text_view::DrawStats,
}

thread_local! {
    static RUNNER: RefCell<Option<Runner>> = const { RefCell::new(None) };
}

/// Queues answers for questions asked while the app is still launching.
pub fn preload_answers(script: &Path) {
    let parsed: Value = serde_json::from_str(&std::fs::read_to_string(script).unwrap_or_default())
        .unwrap_or(Value::Null);
    for answer in parsed["answers"].as_array().cloned().unwrap_or_default() {
        crate::prompts::queue_answer(answer.as_str().unwrap_or_default());
    }
}

thread_local! {
    static SECOND_EXIT: std::cell::Cell<Option<i32>> = const { std::cell::Cell::new(None) };
}

pub fn start(mtm: MainThreadMarker, script: &Path) {
    let text = std::fs::read_to_string(script).unwrap_or_default();
    let parsed: Value = serde_json::from_str(&text).unwrap_or(json!({ "steps": [] }));
    let steps = parsed["steps"].as_array().cloned().unwrap_or_default();
    RUNNER.with(|r| {
        *r.borrow_mut() = Some(Runner {
            steps,
            index: 0,
            failures: Vec::new(),
            notes: Vec::new(),
            generated: Default::default(),
            saved_clipboard: None,
            render_stats: Default::default(),
        })
    });
    schedule(mtm, STEP_DELAY);
}

fn schedule(mtm: MainThreadMarker, delay: f64) {
    let block = RcBlock::new(move |_timer: std::ptr::NonNull<NSTimer>| run_next(mtm));
    unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(delay, false, &block) };
}

fn fail(step: &Value, message: String) {
    RUNNER.with(|r| {
        if let Some(r) = r.borrow_mut().as_mut() {
            r.failures
                .push(json!({ "step": r.index, "action": step, "message": message }));
        }
    });
}

fn finish(mtm: MainThreadMarker) {
    let report = RUNNER.with(|r| {
        let r = r.borrow();
        let r = r.as_ref().expect("runner");
        json!({ "failures": r.failures, "notes": r.notes, "steps_run": r.index })
    });
    if let Some(path) = args().report {
        let _ = std::fs::write(
            path,
            serde_json::to_string_pretty(&report).unwrap_or_default(),
        );
    }
    NSApplication::sharedApplication(mtm).terminate(None);
}

fn run_next(mtm: MainThreadMarker) {
    let step = RUNNER.with(|r| {
        let mut r = r.borrow_mut();
        let r = r.as_mut()?;
        let step = r.steps.get(r.index).cloned();
        r.index += 1;
        step
    });
    let Some(step) = step else { return finish(mtm) };
    let delay = match run_step(mtm, &step) {
        Ok(delay) => delay,
        Err(message) => {
            fail(&step, message);
            STEP_DELAY
        }
    };
    if step["type"] == "quit" {
        return finish(mtm);
    }
    schedule(mtm, delay);
}

/// Values a script can assert on.
fn query(mtm: MainThreadMarker, what: &str, step: &Value) -> Result<Value, String> {
    Ok(match what {
        "window_count" => json!(crate::window::window_count()),
        "tab_count" => json!(crate::app::tabs().len()),
        "window_title" => {
            json!(crate::window::current_tab(mtm).map(|t| t.window.title().to_string()))
        }
        "menu_titles" => json!(crate::menus::titles(mtm)),
        "line_count" => json!(current(mtm)?.doc.borrow().doc.buffer().rope().len_lines()),
        "visible_line_layouts" => json!(render_stats().lines),
        "laid_out_columns" => json!(render_stats().max_columns),
        "first_paint_ms" => json!(current(mtm)?.first_paint_ms.get()),
        "text" => json!(current(mtm)?.doc.borrow().doc.text()),
        "selected_text" => json!(current(mtm)?.doc.borrow().doc.selected_text()),
        "selection_count" => json!(current(mtm)?.doc.borrow().doc.selections().ranges().len()),
        "dirty" => json!(current(mtm)?.doc.borrow().doc.is_dirty()),
        "marked" => json!(*current(mtm)?.view.ivars().marked.borrow()),
        "notice" => json!(*current(mtm)?.notice.borrow()),
        "clipboard" => json!(crate::input::clipboard_text()),
        "bookmarks" => json!(current(mtm)?.doc.borrow().doc.bookmarked_lines()),
        "status" => json!(current(mtm)?.status.stringValue().to_string()),
        "language" => json!(current(mtm)?.doc.borrow().language.name()),
        "role_at" => {
            let tab = current(mtm)?;
            let role = crate::highlight::role_at(
                &tab,
                step["line"].as_u64().unwrap_or(0) as usize,
                step["column"].as_u64().unwrap_or(0) as usize,
            );
            json!(format!("{role:?}"))
        }
        "visible_rows" => json!(crate::rows::row_count(&*current(mtm)?)),
        "row_line" => json!(
            current(mtm)?
                .folds
                .borrow()
                .line_of_row(step["row"].as_u64().unwrap_or(0) as usize)
        ),
        "matching_line" => {
            let tab = current(mtm)?;
            let line = tab
                .doc
                .borrow()
                .doc
                .line_col(tab.doc.borrow().doc.selections().primary().head)
                .0;
            json!(
                crate::highlight::structure(&tab)
                    .and_then(|s| editor_syntax::matching_block_line(&s, line))
            )
        }
        "issue_lines" => json!(
            crate::highlight::structure(&*current(mtm)?).map(|s| s
                .issues
                .iter()
                .map(|i| i.line)
                .collect::<Vec<_>>())
        ),
        "caret_line" => {
            let tab = current(mtm)?;
            let doc = tab.doc.borrow();
            json!(doc.doc.line_col(doc.doc.selections().primary().head).0)
        }
        "font_size" => json!(crate::app::with_app(|a| a.font_size)),
        "word_wrap" => json!(current(mtm)?.doc.borrow().word_wrap),
        "settings_open" => json!(crate::settings::is_open()),
        "ax_role" => {
            let view = current(mtm)?.view.clone();
            let role: Option<Retained<NSString>> = unsafe { msg_send![&*view, accessibilityRole] };
            json!(role.map(|r| r.to_string()))
        }
        "ax_line" => {
            let view = current(mtm)?.view.clone();
            let line: isize = unsafe { msg_send![&*view, accessibilityInsertionPointLineNumber] };
            json!(line)
        }
        "ax_selected_text" => {
            let view = current(mtm)?.view.clone();
            let text: Option<Retained<NSString>> =
                unsafe { msg_send![&*view, accessibilitySelectedText] };
            json!(text.map(|t| t.to_string()))
        }
        "ax_value" => {
            let view = current(mtm)?.view.clone();
            let value: Option<Retained<objc2::runtime::AnyObject>> =
                unsafe { msg_send![&*view, accessibilityValue] };
            json!(
                value
                    .and_then(|v| v.downcast::<NSString>().ok())
                    .map(|s| s.to_string())
            )
        }
        "find_status" => json!(crate::find_panel::status_text(&*current(mtm)?)),
        "search_running" => json!(crate::find_panel::is_running(&*current(mtm)?)),
        "tab_texts" => json!(
            crate::app::tabs()
                .iter()
                .map(|t| t.doc.borrow().doc.text())
                .collect::<Vec<_>>()
        ),
        "tab_dirty" => json!(
            crate::app::tabs()
                .iter()
                .map(|t| t.doc.borrow().doc.is_dirty())
                .collect::<Vec<_>>()
        ),
        "tab_bookmarks" => json!(
            crate::app::tabs()
                .iter()
                .map(|t| t.doc.borrow().doc.bookmarked_lines())
                .collect::<Vec<_>>()
        ),
        "second_exit" => json!(SECOND_EXIT.with(|c| c.get())),
        "session_file_count" => {
            let root = args().session_dir.ok_or("no session folder")?;
            json!(
                std::fs::read_dir(root)
                    .map(|d| d
                        .filter_map(Result::ok)
                        .filter(|e| {
                            let name = e.file_name().to_string_lossy().into_owned();
                            name != ".lock" && !name.ends_with(".tmp")
                        })
                        .count())
                    .unwrap_or(0)
            )
        }
        "last_prompt" => json!(crate::prompts::last_prompt()),
        "line_text" => {
            let line = step["line"].as_u64().unwrap_or(0) as usize;
            let tab = current(mtm)?;
            let doc = tab.doc.borrow();
            json!(
                crate::layout::display_line(&doc, line)
                    .chars
                    .iter()
                    .collect::<String>()
            )
        }
        "file_text" => json!(std::fs::read_to_string(work_path(step)?).map_err(|e| e.to_string())?),
        "file_hex" => json!(
            std::fs::read(work_path(step)?)
                .map_err(|e| e.to_string())?
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        ),
        "file_matches" => {
            let file = step["file"].as_str().unwrap_or_default().to_string();
            let original = match step["sections"].as_array() {
                Some(sections) => synthetic_backup(sections),
                None => RUNNER
                    .with(|r| {
                        r.borrow()
                            .as_ref()
                            .and_then(|r| r.generated.get(&file).cloned())
                    })
                    .ok_or("file was not generated")?,
            };
            let pair = step["replace"].as_array().ok_or("needs replace")?;
            let (from, to) = (
                pair[0].as_str().unwrap_or_default().as_bytes(),
                pair[1].as_str().unwrap_or_default().as_bytes(),
            );
            let at = original
                .windows(from.len())
                .position(|w| w == from)
                .ok_or("pattern not in generated file")?;
            let mut expected = original[..at].to_vec();
            expected.extend_from_slice(to);
            expected.extend_from_slice(&original[at + from.len()..]);
            json!(std::fs::read(work_path(step)?).ok() == Some(expected))
        }
        "file_unchanged" => {
            let path = work_path(step)?;
            let file = step["file"].as_str().unwrap_or_default().to_string();
            let original = RUNNER.with(|r| {
                r.borrow()
                    .as_ref()
                    .and_then(|r| r.generated.get(&file).cloned())
            });
            json!(original.is_some_and(|o| std::fs::read(path).ok() == Some(o)))
        }
        other => return Err(format!("unknown query {other}")),
    })
}

fn render_stats() -> crate::text_view::DrawStats {
    RUNNER.with(|r| {
        r.borrow()
            .as_ref()
            .map(|r| r.render_stats)
            .unwrap_or_default()
    })
}

fn current(mtm: MainThreadMarker) -> Result<std::rc::Rc<crate::app::Tab>, String> {
    crate::window::current_tab(mtm).ok_or_else(|| "no current tab".to_string())
}

fn work_path(step: &Value) -> Result<std::path::PathBuf, String> {
    let file = step["file"].as_str().ok_or("step needs file")?;
    Ok(args().work_dir.ok_or("no work folder")?.join(file))
}

fn check(actual: &Value, step: &Value) -> Result<(), String> {
    if let Some(limit) = step.get("greater_than").and_then(Value::as_f64)
        && actual.as_f64().is_none_or(|v| v <= limit)
    {
        return Err(format!("expected greater than {limit}, got {actual}"));
    }
    if let Some(expected) = step.get("equals")
        && actual != expected
    {
        return Err(format!("expected {expected}, got {actual}"));
    }
    if let Some(needle) = step.get("contains").and_then(Value::as_str)
        && !actual.as_str().unwrap_or_default().contains(needle)
    {
        return Err(format!("expected to contain {needle:?}, got {actual}"));
    }
    if let Some(limit) = step.get("less_than").and_then(Value::as_f64)
        && actual.as_f64().is_none_or(|v| v >= limit)
    {
        return Err(format!("expected less than {limit}, got {actual}"));
    }
    Ok(())
}

fn render(mtm: MainThreadMarker, name: &str) -> Result<(), String> {
    let tab = crate::window::current_tab(mtm).ok_or("no window to render")?;
    let view = tab.window.contentView().ok_or("no content view")?;
    let view = unsafe { view.superview() }.unwrap_or(view);
    let bounds = view.bounds();
    let rep = view
        .bitmapImageRepForCachingDisplayInRect(bounds)
        .ok_or("cannot create bitmap")?;
    view.cacheDisplayInRect_toBitmapImageRep(bounds, &rep);
    let stats = tab.view.ivars().stats.get();
    RUNNER.with(|r| r.borrow_mut().as_mut().map(|r| r.render_stats = stats));
    let data = unsafe {
        rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
    }
    .ok_or("cannot encode png")?;
    let dir = args().artifacts.ok_or("no artifacts folder")?;
    let path = dir.join(format!("{name}.png"));
    let ok = data.writeToFile_atomically(&NSString::from_str(&path.to_string_lossy()), true);
    if ok {
        Ok(())
    } else {
        Err(format!("could not write {}", path.display()))
    }
}

fn modifiers(step: &Value) -> crate::input::Modifiers {
    let has = |name: &str| {
        step["mods"]
            .as_array()
            .is_some_and(|m| m.iter().any(|v| v == name))
    };
    crate::input::Modifiers {
        command: has("cmd"),
        option: has("option"),
        shift: has("shift"),
    }
}

/// Sends a real key event through the view's key handling.
fn key(mtm: MainThreadMarker, step: &Value) -> Result<(), String> {
    let tab = current(mtm)?;
    let name = step["key"].as_str().ok_or("key needs key")?;
    let (code, chars): (u16, String) = match name {
        "left" => (123, "\u{F702}".into()),
        "right" => (124, "\u{F703}".into()),
        "down" => (125, "\u{F701}".into()),
        "up" => (126, "\u{F700}".into()),
        "backspace" => (51, "\u{7f}".into()),
        "delete" => (117, "\u{F728}".into()),
        "return" => (36, "\r".into()),
        "tab" => (48, "\t".into()),
        "escape" => (53, "\u{1b}".into()),
        "home" => (115, "\u{F729}".into()),
        "end" => (119, "\u{F72B}".into()),
        "pageup" => (116, "\u{F72C}".into()),
        "pagedown" => (121, "\u{F72D}".into()),
        "f2" => (120, "\u{F705}".into()),
        other if other.chars().count() == 1 => (0, other.into()),
        other => return Err(format!("unknown key {other}")),
    };
    let has = |m: &str| {
        step["mods"]
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v == m))
    };
    let mut flags = NSEventModifierFlags::empty();
    if has("cmd") {
        flags |= NSEventModifierFlags::Command;
    }
    if has("option") {
        flags |= NSEventModifierFlags::Option;
    }
    if has("shift") {
        flags |= NSEventModifierFlags::Shift;
    }
    if has("control") {
        flags |= NSEventModifierFlags::Control;
    }
    if chars.starts_with([
        '\u{F700}', '\u{F701}', '\u{F702}', '\u{F703}', '\u{F728}', '\u{F729}', '\u{F72B}',
        '\u{F72C}', '\u{F72D}', '\u{F705}',
    ]) {
        flags |= NSEventModifierFlags::Function;
        if matches!(name, "left" | "right" | "up" | "down") {
            flags |= NSEventModifierFlags::NumericPad;
        }
    }
    let characters = NSString::from_str(&chars);
    let event = unsafe {
        NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
            NSEventType::KeyDown,
            NSPoint::new(0.0, 0.0),
            flags,
            0.0,
            tab.window.windowNumber(),
            None,
            &characters,
            &characters,
            false,
            code,
        )
    }
    .ok_or("could not create key event")?;
    if flags.contains(NSEventModifierFlags::Command)
        && NSApplication::sharedApplication(mtm)
            .mainMenu()
            .is_some_and(|m| m.performKeyEquivalent(&event))
    {
        return Ok(());
    }
    let _: () = unsafe { msg_send![&*tab.view, keyDown: &*event] };
    Ok(())
}

/// A synthetic backup in the documented format. Contains no real data.
fn synthetic_backup(sections: &[Value]) -> Vec<u8> {
    const SEP: &str = "-------FV000X-0.00-FW-build0000-000000-------2000-01-01 00:00:00-------0000000000000000AAAA---------";
    let mut out = format!(
        "[header]\nmagic=CC05E358\nversion=1.4\nimage_version=FV000X-0.00-FW-build0000-000000\nmodel=FV000X\ntype=full_config\nfile_number={}\nfile_split={SEP}\n[/header]\n",
        sections.len()
    )
    .into_bytes();
    for (index, section) in sections.iter().enumerate() {
        let (encrypt, payload) = match section.get("binary").and_then(Value::as_u64) {
            Some(len) => (
                "yes",
                (0..len)
                    .map(|i| [0u8, b'\r', 0xff, b'\n', 0x80, 7][i as usize % 6] ^ (i as u8))
                    .collect::<Vec<u8>>(),
            ),
            None => (
                "no",
                section["text"]
                    .as_str()
                    .unwrap_or_default()
                    .as_bytes()
                    .to_vec(),
            ),
        };
        let compress = if encrypt == "yes" { "gzipped" } else { "no" };
        let name = section["name"].as_str().map_or_else(
            || format!("/data/config/section{index}.conf.gz"),
            str::to_string,
        );
        out.extend_from_slice(format!("[file]\nname={name}\ndomain=global\ntype=config\nencrypt={encrypt}\ncompress={compress}\n{SEP}\n").as_bytes());
        out.extend_from_slice(&payload);
        out.extend_from_slice(format!("{SEP}\n[/file]\n").as_bytes());
    }
    out
}

fn run_step(mtm: MainThreadMarker, step: &Value) -> Result<f64, String> {
    let kind = step["type"].as_str().unwrap_or_default();
    match kind {
        "assert" => {
            let what = step["what"].as_str().ok_or("assert needs what")?;
            check(&query(mtm, what, step)?, step)?;
        }
        "menu" => {
            let name = step["action"].as_str().ok_or("menu needs action")?;
            let action = crate::menus::MenuAction::from_name(name)
                .ok_or(format!("unknown action {name}"))?;
            crate::app_delegate::perform(mtm, action);
        }
        "render" => render(mtm, step["name"].as_str().unwrap_or("render"))?,
        "generate" => {
            let pattern = step["pattern"].as_str().unwrap_or_default();
            let times = step["times"].as_u64().unwrap_or(1) as usize;
            std::fs::write(work_path(step)?, pattern.repeat(times)).map_err(|e| e.to_string())?;
        }
        "open" => {
            crate::document_controller::open_path(mtm, &work_path(step)?)?;
        }
        "generate_backup" => {
            let bytes = synthetic_backup(step["sections"].as_array().ok_or("needs sections")?);
            let file = step["file"].as_str().unwrap_or_default().to_string();
            std::fs::write(work_path(step)?, &bytes).map_err(|e| e.to_string())?;
            RUNNER.with(|r| {
                r.borrow_mut()
                    .as_mut()
                    .map(|r| r.generated.insert(file, bytes))
            });
        }
        "text" => {
            let view = current(mtm)?.view.clone();
            let text = NSString::from_str(step["text"].as_str().unwrap_or_default());
            let _: () = unsafe {
                msg_send![&*view, insertText: &*text, replacementRange: NSRange::new(usize::MAX >> 1, 0)]
            };
        }
        "mark" => {
            let view = current(mtm)?.view.clone();
            let text = NSString::from_str(step["text"].as_str().unwrap_or_default());
            let none = NSRange::new(usize::MAX >> 1, 0);
            let _: () = unsafe {
                msg_send![&*view, setMarkedText: &*text, selectedRange: NSRange::new(0, 0), replacementRange: none]
            };
        }
        "key" => key(mtm, step)?,
        "send" => {
            let view = current(mtm)?.view.clone();
            let name = step["selector"].as_str().ok_or("send needs selector")?;
            let selector = objc2::runtime::Sel::register(
                &std::ffi::CString::new(name).map_err(|e| e.to_string())?,
            );
            if !unsafe { view.tryToPerform_with(selector, None) } {
                return Err(format!("nothing handled {name}"));
            }
        }
        "click" | "drag" => {
            let tab = current(mtm)?;
            let line = step["line"].as_u64().unwrap_or(0) as usize;
            let column = step["column"].as_u64().unwrap_or(0) as usize;
            let offset = {
                let doc = tab.doc.borrow();
                let start = doc.doc.buffer().rope().line_to_char(line);
                start + crate::layout::display_line(&doc, line).char_at_column(column)
            };
            let mods = modifiers(step);
            if kind == "click" {
                let count = step["count"].as_u64().unwrap_or(1) as usize;
                let anchor = crate::input::click(&tab, offset, line, column, mods, count);
                tab.view.ivars().drag.set(Some(anchor));
            } else {
                let anchor = tab.view.ivars().drag.get().ok_or("drag without click")?;
                crate::input::drag(&tab, anchor, offset, line, column);
            }
        }
        "toggle_bookmark" => crate::bookmark_actions::toggle_line(
            &current(mtm)?,
            step["line"].as_u64().unwrap_or(0) as usize,
        ),
        "spawn_second" => {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let mut child = std::process::Command::new(exe)
                .arg("--session-dir")
                .arg(args().session_dir.ok_or("no session folder")?)
                .arg(work_path(step)?)
                .spawn()
                .map_err(|e| e.to_string())?;
            let started = std::time::Instant::now();
            let code = loop {
                if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                    break status.code();
                }
                if started.elapsed().as_secs() > 30 {
                    let _ = child.kill();
                    break None;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            };
            SECOND_EXIT.with(|c| c.set(code));
        }
        "appearance" => {
            let name = if step["mode"] == "dark" {
                unsafe { objc2_app_kit::NSAppearanceNameDarkAqua }
            } else {
                unsafe { objc2_app_kit::NSAppearanceNameAqua }
            };
            let appearance = objc2_app_kit::NSAppearance::appearanceNamed(name);
            NSApplication::sharedApplication(mtm).setAppearance(appearance.as_deref());
            current(mtm)?.view.setNeedsDisplay(true);
        }
        "set_language" => crate::view_actions::set_language(
            &current(mtm)?,
            step["name"].as_str().unwrap_or_default(),
        ),
        "gutter_click" => {
            let tab = current(mtm)?;
            crate::input::gutter_click(
                &tab,
                step["line"].as_u64().unwrap_or(0) as usize,
                step["marker"].as_bool().unwrap_or(false),
            );
        }
        "reopen_encoding" => crate::view_actions::reopen_with_encoding(
            mtm,
            &current(mtm)?,
            step["name"].as_str().unwrap_or_default(),
        ),
        "render_settings" => {
            let window = crate::settings::window().ok_or("settings not open")?;
            let view = window.contentView().ok_or("no view")?;
            let bounds = view.bounds();
            let rep = view
                .bitmapImageRepForCachingDisplayInRect(bounds)
                .ok_or("bitmap")?;
            view.cacheDisplayInRect_toBitmapImageRep(bounds, &rep);
            let data = unsafe {
                rep.representationUsingType_properties(
                    NSBitmapImageFileType::PNG,
                    &NSDictionary::new(),
                )
            }
            .ok_or("png")?;
            let path = args().artifacts.ok_or("no artifacts")?.join(format!(
                "{}.png",
                step["name"].as_str().unwrap_or("settings")
            ));
            data.writeToFile_atomically(&NSString::from_str(&path.to_string_lossy()), true);
        }
        "find" => {
            let tab = current(mtm)?;
            let flag = |name: &str| step.get(name).and_then(Value::as_bool);
            crate::find_panel::set_fields(
                &tab,
                step["pattern"].as_str().unwrap_or_default(),
                step["replace"].as_str(),
                [
                    flag("regex"),
                    flag("ignore_case"),
                    flag("whole_word"),
                    flag("wrap"),
                    flag("in_selection"),
                ],
            );
        }
        "find_button" => {
            let tab = current(mtm)?;
            let title = step["button"].as_str().unwrap_or_default();
            let button = crate::find_panel::Button::from_title(title)
                .ok_or(format!("unknown button {title}"))?;
            crate::find_panel::press(mtm, &tab, button);
        }
        "answer" => crate::prompts::queue_answer(step["with"].as_str().unwrap_or_default()),
        "write_file" => std::fs::write(work_path(step)?, step["text"].as_str().unwrap_or_default())
            .map_err(|e| e.to_string())?,
        "write_bytes" => {
            let hex: String = step["hex"]
                .as_str()
                .unwrap_or_default()
                .split_whitespace()
                .collect();
            let bytes: Vec<u8> = (0..hex.len())
                .step_by(2)
                .filter_map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
                .collect();
            std::fs::write(work_path(step)?, bytes).map_err(|e| e.to_string())?;
        }
        "check_external" => crate::document_controller::check_all_external_changes(mtm),
        "save_as" => {
            let tab = current(mtm)?;
            if !crate::document_controller::save_to(mtm, &tab, &work_path(step)?) {
                return Err("save failed".into());
            }
        }
        "set_clipboard" => {
            crate::input::set_clipboard_text(step["text"].as_str().unwrap_or_default())
        }
        "save_clipboard" => {
            let text = crate::input::clipboard_text();
            RUNNER.with(|r| r.borrow_mut().as_mut().map(|r| r.saved_clipboard = text));
        }
        "restore_clipboard" => {
            if let Some(text) = RUNNER.with(|r| {
                r.borrow_mut()
                    .as_mut()
                    .and_then(|r| r.saved_clipboard.take())
            }) {
                crate::input::set_clipboard_text(&text);
            }
        }
        "scroll" => {
            let tab = current(mtm)?;
            let line = step["line"].as_u64().unwrap_or(0) as usize;
            let column = step["column"].as_u64().unwrap_or(0) as usize;
            crate::text_view::scroll_to(&tab, line, column);
        }
        "wait" => return Ok(step["ms"].as_f64().unwrap_or(100.0) / 1000.0),
        "quit" => {}
        other => return Err(format!("unknown step type {other}")),
    }
    Ok(STEP_DELAY)
}
