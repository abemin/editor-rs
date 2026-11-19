//! Scripted runs of the real app, used by `tests/ui.rs`.
//!
//! Steps run one at a time on the main thread with a short pause between them.
//! Failures are collected into a JSON report, then the app quits.

use crate::app::args;
use block2::RcBlock;
use objc2::{DefinedClass, MainThreadMarker};
use objc2_app_kit::{NSApplication, NSBitmapImageFileType};
use objc2_foundation::{NSDictionary, NSString, NSTimer};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::path::Path;

const STEP_DELAY: f64 = 0.05;

struct Runner {
    steps: Vec<Value>,
    index: usize,
    failures: Vec<Value>,
    notes: Vec<Value>,
}

thread_local! {
    static RUNNER: RefCell<Option<Runner>> = const { RefCell::new(None) };
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
fn query(mtm: MainThreadMarker, what: &str, _step: &Value) -> Result<Value, String> {
    Ok(match what {
        "window_count" => json!(crate::window::window_count()),
        "tab_count" => json!(crate::app::tabs().len()),
        "window_title" => {
            json!(crate::window::current_tab(mtm).map(|t| t.window.title().to_string()))
        }
        "menu_titles" => json!(crate::menus::titles(mtm)),
        "line_count" => json!(current(mtm)?.doc.borrow().doc.buffer().rope().len_lines()),
        "visible_line_layouts" => json!(current(mtm)?.view.ivars().stats.get().lines),
        "laid_out_columns" => json!(current(mtm)?.view.ivars().stats.get().max_columns),
        "first_paint_ms" => json!(current(mtm)?.first_paint_ms.get()),
        other => return Err(format!("unknown query {other}")),
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
