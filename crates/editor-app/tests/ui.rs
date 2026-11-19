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

/// Runs one script and returns the report. Panics with every failure listed.
pub fn run_script(name: &str) -> serde_json::Value {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/scripts")
        .join(format!("{name}.json"));
    let work = tempfile::tempdir().unwrap();
    let report = work.path().join("report.json");
    let artifacts = repo().join("target/ui-artifacts");
    std::fs::create_dir_all(&artifacts).unwrap();
    let mut child = Command::new(app_binary())
        .arg("--test-script")
        .arg(&script)
        .arg("--session-dir")
        .arg(work.path().join("session"))
        .arg("--report")
        .arg(&report)
        .arg("--artifacts")
        .arg(&artifacts)
        .arg("--work-dir")
        .arg(work.path())
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if start.elapsed() > TIMEOUT {
            let _ = child.kill();
            panic!("{name}: app did not finish within {TIMEOUT:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let text =
        std::fs::read_to_string(&report).unwrap_or_else(|_| panic!("{name}: no report written"));
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    let failures = value["failures"].as_array().cloned().unwrap_or_default();
    assert!(
        failures.is_empty(),
        "{name} failed:\n{}",
        serde_json::to_string_pretty(&failures).unwrap()
    );
    value
}

macro_rules! scripts {
    ($($name:ident),* $(,)?) => { $( #[test] fn $name() { run_script(stringify!($name)); } )* };
}

scripts!(launch, render_large, long_line);
