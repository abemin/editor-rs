//! Questions and messages shown to the user.
//!
//! In test mode nothing is shown: answers come from the script and every
//! question is recorded so scripts can check it.

use objc2::MainThreadMarker;
use objc2_app_kit::{NSAlert, NSAlertStyle};
use objc2_foundation::NSString;
use std::cell::RefCell;
use std::collections::VecDeque;

thread_local! {
    static ANSWERS: RefCell<VecDeque<String>> = const { RefCell::new(VecDeque::new()) };
    static LAST: RefCell<Option<String>> = const { RefCell::new(None) };
}

pub fn queue_answer(button: &str) {
    ANSWERS.with(|a| a.borrow_mut().push_back(button.to_string()));
}

pub fn last_prompt() -> Option<String> {
    LAST.with(|l| l.borrow().clone())
}

/// Asks a question and returns the index of the chosen button.
pub fn ask(
    mtm: MainThreadMarker,
    message: &str,
    info: &str,
    buttons: &[&str],
    warning: bool,
) -> usize {
    LAST.with(|l| *l.borrow_mut() = Some(format!("{message} {info}")));
    if crate::app::args().test_script.is_some() {
        let answer = ANSWERS.with(|a| a.borrow_mut().pop_front());
        return answer
            .and_then(|a| buttons.iter().position(|b| *b == a))
            .unwrap_or(buttons.len() - 1);
    }
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(message));
    alert.setInformativeText(&NSString::from_str(info));
    alert.setAlertStyle(if warning {
        NSAlertStyle::Warning
    } else {
        NSAlertStyle::Informational
    });
    for button in buttons {
        alert.addButtonWithTitle(&NSString::from_str(button));
    }
    let response = alert.runModal();
    (response - 1000).clamp(0, buttons.len() as isize - 1) as usize
}

/// Shows a message with a single OK button.
pub fn tell(mtm: MainThreadMarker, message: &str, info: &str) {
    ask(mtm, message, info, &["OK"], true);
}

/// Asks for a line of text. In test mode the next queued answer is the text.
pub fn ask_text(mtm: MainThreadMarker, message: &str, info: &str) -> Option<String> {
    LAST.with(|l| *l.borrow_mut() = Some(format!("{message} {info}")));
    if crate::app::args().test_script.is_some() {
        return ANSWERS.with(|a| a.borrow_mut().pop_front());
    }
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(message));
    alert.setInformativeText(&NSString::from_str(info));
    alert.addButtonWithTitle(&NSString::from_str("Go"));
    alert.addButtonWithTitle(&NSString::from_str("Cancel"));
    let field = objc2_app_kit::NSTextField::textFieldWithString(&NSString::from_str(""), mtm);
    field.setFrame(objc2_foundation::NSRect::new(
        objc2_foundation::NSPoint::new(0.0, 0.0),
        objc2_foundation::NSSize::new(200.0, 24.0),
    ));
    alert.setAccessoryView(Some(&field));
    alert.window().setInitialFirstResponder(Some(&field));
    (alert.runModal() == 1000).then(|| field.stringValue().to_string())
}
