//! A fast native text editor for macOS.

mod app;
mod app_delegate;
mod bookmark_actions;
mod commands;
mod config;
mod doc_model;
mod document_controller;
mod find_panel;
mod folds;
mod highlight;
mod input;
mod launch;
mod layout;
mod menus;
mod prompts;
mod rows;
mod session_driver;
mod settings;
mod status_bar;
mod test_mode;
mod text_view;
mod theme;
mod view_actions;
mod window;
mod window_delegate;

use objc2::MainThreadMarker;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use std::path::PathBuf;

/// Command-line options. The test options are only used by automated UI tests.
#[derive(Clone, Debug, Default)]
pub struct Args {
    pub files: Vec<PathBuf>,
    pub test_script: Option<PathBuf>,
    pub session_dir: Option<PathBuf>,
    pub report: Option<PathBuf>,
    pub artifacts: Option<PathBuf>,
    pub work_dir: Option<PathBuf>,
}

fn parse_args() -> Args {
    let mut args = Args::default();
    let mut it = std::env::args_os().skip(1);
    while let Some(arg) = it.next() {
        let mut next = || it.next().map(PathBuf::from);
        match arg.to_str() {
            Some("--print-info-plist") => {
                print!("{}", config::info_plist());
                std::process::exit(0);
            }
            Some("--test-script") => args.test_script = next(),
            Some("--session-dir") => args.session_dir = next(),
            Some("--report") => args.report = next(),
            Some("--artifacts") => args.artifacts = next(),
            Some("--work-dir") => args.work_dir = next(),
            Some(flag)
                if flag.starts_with("-psn_")
                    || flag.starts_with("-NS")
                    || flag.starts_with("-Apple") =>
            {
                let _ = next();
            }
            _ => args.files.push(PathBuf::from(arg)),
        }
    }
    args
}

fn main() {
    let args = parse_args();
    let mtm = MainThreadMarker::new().expect("runs on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    let policy = if args.test_script.is_some() {
        NSApplicationActivationPolicy::Accessory
    } else {
        NSApplicationActivationPolicy::Regular
    };
    app.setActivationPolicy(policy);
    let delegate = app_delegate::AppDelegate::new(mtm, args);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.run();
}
