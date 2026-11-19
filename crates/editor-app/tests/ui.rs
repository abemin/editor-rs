//! Runs the real app with scripted actions. See `src/test_mode.rs`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(90);

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn app_binary() -> &'static Path {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT.get_or_init(|| {
        let status = Command::new(repo().join("scripts/bundle.sh"))
            .arg("debug")
            .status()
            .expect("bundle.sh runs");
        assert!(status.success(), "bundle.sh failed");
        let out = String::from_utf8(
            Command::new(repo().join("scripts/bundle.sh"))
                .args(["debug", "--print-path"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        PathBuf::from(out.trim()).join("Contents/MacOS/EditorRS")
    })
}

/// Runs one script and returns the last report. Panics with every failure listed.
///
/// A script is either `{ "steps": [...] }` or `{ "runs": [ { "steps": [...] }, ... ] }`.
/// Each run launches the app again with the same session and work folders, so
/// scripts can quit, crash or change files between launches.
pub fn run_script(name: &str) -> serde_json::Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/scripts")
        .join(format!("{name}.json"));
    let script: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let runs = match script.get("runs") {
        Some(runs) => runs.as_array().unwrap().clone(),
        None => vec![script.clone()],
    };
    let work = tempfile::tempdir().unwrap();
    let artifacts = repo().join("target/ui-artifacts");
    std::fs::create_dir_all(&artifacts).unwrap();
    let mut session = work.path().join("session");
    let mut last = serde_json::Value::Null;
    for (index, run) in runs.iter().enumerate() {
        for action in run["before"].as_array().cloned().unwrap_or_default() {
            if let Some(file) = action["delete"].as_str() {
                std::fs::remove_file(work.path().join(file)).unwrap();
            }
            if let Some(file) = action["write"].as_str() {
                std::fs::write(work.path().join(file), action["text"].as_str().unwrap()).unwrap();
            }
        }
        if run["session_dir_readonly"].as_bool() == Some(true) {
            let locked = work.path().join("locked");
            std::fs::create_dir_all(&locked).unwrap();
            std::fs::set_permissions(&locked, std::os::unix::fs::PermissionsExt::from_mode(0o500))
                .unwrap();
            session = locked.join("session");
        }
        let script_file = work.path().join(format!("run{index}.json"));
        std::fs::write(&script_file, run.to_string()).unwrap();
        let report = work.path().join(format!("report{index}.json"));
        let mut child = Command::new(app_binary())
            .arg("--test-script")
            .arg(&script_file)
            .arg("--session-dir")
            .arg(&session)
            .arg("--report")
            .arg(&report)
            .arg("--artifacts")
            .arg(&artifacts)
            .arg("--work-dir")
            .arg(work.path())
            .spawn()
            .unwrap();
        let start = Instant::now();
        let kill_after = run["kill_after_ms"].as_u64().map(Duration::from_millis);
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if kill_after.is_some_and(|k| start.elapsed() > k) {
                child.kill().unwrap();
                child.wait().unwrap();
                break;
            }
            if start.elapsed() > TIMEOUT {
                let _ = child.kill();
                panic!("{name}: app did not finish within {TIMEOUT:?}");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        if kill_after.is_some() {
            continue;
        }
        let text = std::fs::read_to_string(&report)
            .unwrap_or_else(|_| panic!("{name} run {index}: no report written"));
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        let failures = value["failures"].as_array().cloned().unwrap_or_default();
        assert!(
            failures.is_empty(),
            "{name} run {index} failed:\n{}",
            serde_json::to_string_pretty(&failures).unwrap()
        );
        last = value;
    }
    let locked = work.path().join("locked");
    if locked.exists() {
        let _ =
            std::fs::set_permissions(&locked, std::os::unix::fs::PermissionsExt::from_mode(0o700));
    }
    last
}

macro_rules! scripts {
    ($($name:ident),* $(,)?) => { $( #[test] fn $name() { run_script(stringify!($name)); } )* };
}

scripts!(
    launch,
    render_large,
    long_line,
    typing,
    multi_cursor,
    ime,
    clipboard,
    locked_paste,
    open_same_file_twice_focuses_tab,
    backup_presentation,
    backup_save,
    backup_invalid,
    external_change,
    close_dirty_tab,
    read_only_notice,
    unrepresentable,
    restore_after_quit,
    restore_after_kill,
    restore_backup_source_deleted,
    restore_source_changed,
    restore_source_missing,
    real_file_untouched,
    clear_session_data,
    disk_error_warning,
    second_launch,
    find_regex,
    find_invalid,
    find_selection,
    replace_all_undo,
    find_large_cancel,
    find_backup_locked,
    bookmark_all,
);
