use super::*;
use editor_core::{Edit, EditBatch};
use std::os::unix::fs::PermissionsExt;

fn record(id: &str) -> TabRecord {
    TabRecord {
        id: id.into(),
        path: None,
        title: "Untitled".into(),
        dirty: true,
        caret: 0,
        scroll_line: 0,
        bookmarks: vec![2],
        word_wrap: false,
        language: None,
        encoding: "utf-8".into(),
        source_len: None,
        source_modified_ms: None,
        has_protected_clone: false,
    }
}
fn manifest(ids: &[&str]) -> Manifest {
    Manifest {
        version: MANIFEST_VERSION,
        active: ids.first().map(|s| s.to_string()),
        tabs: ids.iter().map(|i| record(i)).collect(),
    }
}
fn insert(at: usize, text: &str) -> EditBatch {
    EditBatch(vec![Edit {
        range: at..at,
        text: text.into(),
    }])
}
fn root() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

#[test]
fn snapshot_plus_journal_restores_text() {
    let dir = root();
    let store = SessionStore::open(dir.path()).unwrap();
    store.write_manifest(&manifest(&["a"])).unwrap();
    store.snapshot("a", "hello").unwrap();
    store
        .append("a", &[insert(5, " world"), insert(0, ">")])
        .unwrap();
    drop(store);
    let store = SessionStore::open(dir.path()).unwrap();
    let tabs = store.restore();
    assert_eq!(tabs.len(), 1);
    assert_eq!(tabs[0].text.as_deref(), Some(">hello world"));
    assert!(!tabs[0].recovered_partial);
    assert_eq!(tabs[0].record.bookmarks, vec![2]);
}

#[test]
fn append_without_snapshot_is_refused() {
    let dir = root();
    let store = SessionStore::open(dir.path()).unwrap();
    assert!(store.append("a", &[insert(0, "typed")]).is_err());
    assert!(!store.journal_path("a").exists());
}

#[test]
fn damaged_snapshot_restores_nothing_instead_of_empty_text() {
    let dir = root();
    let store = SessionStore::open(dir.path()).unwrap();
    store.write_manifest(&manifest(&["a"])).unwrap();
    store.snapshot("a", "precious text").unwrap();
    store.append("a", &[insert(0, "x")]).unwrap();
    let path = store.snapshot_path("a");
    let mut bytes = std::fs::read(&path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xFF;
    std::fs::write(&path, bytes).unwrap();
    let tab = &store.restore()[0];
    assert_eq!(tab.text, None);
    assert!(tab.recovered_partial);
}

#[test]
fn unusable_manifest_still_offers_every_snapshot() {
    let dir = root();
    let store = SessionStore::open(dir.path()).unwrap();
    store.snapshot("lost1", "first draft").unwrap();
    store.append("lost1", &[insert(11, "!")]).unwrap();
    std::fs::write(dir.path().join("manifest.json"), "{not json").unwrap();
    let tabs = store.restore();
    assert_eq!(tabs.len(), 1);
    assert_eq!(tabs[0].text.as_deref(), Some("first draft!"));
    assert!(tabs[0].record.dirty);
    assert_eq!(tabs[0].record.path, None);
}

#[test]
fn clean_tab_has_no_stored_text() {
    let dir = root();
    let store = SessionStore::open(dir.path()).unwrap();
    store.write_manifest(&manifest(&["a"])).unwrap();
    assert_eq!(store.restore()[0].text, None);
}

#[test]
fn new_snapshot_replaces_old_journal() {
    let dir = root();
    let store = SessionStore::open(dir.path()).unwrap();
    store.write_manifest(&manifest(&["a"])).unwrap();
    store.snapshot("a", "").unwrap();
    store.append("a", &[insert(0, "old")]).unwrap();
    store.snapshot("a", "fresh").unwrap();
    store.append("a", &[insert(5, "!")]).unwrap();
    assert_eq!(store.restore()[0].text.as_deref(), Some("fresh!"));
}

#[test]
fn stale_journal_is_ignored_after_a_crash_between_snapshot_and_truncate() {
    let dir = root();
    let store = SessionStore::open(dir.path()).unwrap();
    store.write_manifest(&manifest(&["a"])).unwrap();
    store.snapshot("a", "one").unwrap();
    store.append("a", &[insert(3, " two")]).unwrap();
    let stale = std::fs::read(store.journal_path("a")).unwrap();
    store.snapshot("a", "one two").unwrap();
    std::fs::write(store.journal_path("a"), stale).unwrap();
    assert_eq!(store.restore()[0].text.as_deref(), Some("one two"));
}

#[test]
fn files_are_private() {
    let dir = root();
    let path = dir.path().join("session");
    let store = SessionStore::open(&path).unwrap();
    store.write_manifest(&manifest(&["a"])).unwrap();
    store.snapshot("a", "secret").unwrap();
    store.append("a", &[insert(0, "x")]).unwrap();
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o700
    );
    for entry in std::fs::read_dir(&path).unwrap() {
        let entry = entry.unwrap();
        assert_eq!(
            entry.metadata().unwrap().permissions().mode() & 0o777,
            0o600,
            "{:?}",
            entry.path()
        );
    }
}

#[test]
fn folder_is_excluded_from_backups() {
    let dir = root();
    let path = dir.path().join("session");
    SessionStore::open(&path).unwrap();
    let out = std::process::Command::new("xattr")
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&out.stdout)
            .contains("com.apple.metadata:com_apple_backup_excludeItem")
    );
}

#[test]
fn truncated_journal_keeps_valid_prefix() {
    let dir = root();
    let store = SessionStore::open(dir.path()).unwrap();
    store.write_manifest(&manifest(&["a"])).unwrap();
    store.snapshot("a", "").unwrap();
    store.append("a", &[insert(0, "first")]).unwrap();
    store.append("a", &[insert(5, " second")]).unwrap();
    let path = store.journal_path("a");
    let len = std::fs::metadata(&path).unwrap().len();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(len - 3)
        .unwrap();
    let tab = &store.restore()[0];
    assert_eq!(tab.text.as_deref(), Some("first"));
    assert!(tab.recovered_partial);
}

#[test]
fn corrupt_record_stops_replay_there() {
    let dir = root();
    let store = SessionStore::open(dir.path()).unwrap();
    store.write_manifest(&manifest(&["a"])).unwrap();
    store.snapshot("a", "").unwrap();
    store.append("a", &[insert(0, "first")]).unwrap();
    store.append("a", &[insert(5, " second")]).unwrap();
    store.append("a", &[insert(12, " third")]).unwrap();
    let path = store.journal_path("a");
    let mut bytes = std::fs::read(&path).unwrap();
    let at = bytes.windows(7).position(|w| w == b" second").unwrap();
    bytes[at] ^= 0xFF;
    std::fs::write(&path, bytes).unwrap();
    let tab = &store.restore()[0];
    assert_eq!(tab.text.as_deref(), Some("first"));
    assert!(tab.recovered_partial);
}

#[test]
fn unknown_manifest_version_is_set_aside() {
    let dir = root();
    std::fs::write(
        dir.path().join("manifest.json"),
        r#"{"version":99,"active":null,"tabs":[]}"#,
    )
    .unwrap();
    let store = SessionStore::open(dir.path()).unwrap();
    assert!(store.restore().is_empty());
    assert!(dir.path().join("manifest.json.unsupported").exists());
}

#[test]
fn unreadable_manifest_is_set_aside() {
    let dir = root();
    std::fs::write(dir.path().join("manifest.json"), "{not json").unwrap();
    let store = SessionStore::open(dir.path()).unwrap();
    assert!(store.restore().is_empty());
    assert!(dir.path().join("manifest.json.unsupported").exists());
}

#[test]
fn second_store_on_same_root_is_busy() {
    let dir = root();
    let _first = SessionStore::open(dir.path()).unwrap();
    assert!(matches!(
        SessionStore::open(dir.path()),
        Err(SessionError::Busy)
    ));
}

#[test]
fn discard_removes_all_tab_files() {
    let dir = root();
    let store = SessionStore::open(dir.path()).unwrap();
    store.snapshot("a", "x").unwrap();
    store.append("a", &[insert(0, "y")]).unwrap();
    std::fs::write(store.protected_path("a"), "z").unwrap();
    store.discard("a").unwrap();
    assert!(!store.journal_path("a").exists());
    assert!(!store.snapshot_path("a").exists());
    assert!(!store.protected_path("a").exists());
}

#[test]
fn clear_all_removes_everything_but_keeps_the_lock() {
    let dir = root();
    let store = SessionStore::open(dir.path()).unwrap();
    store.write_manifest(&manifest(&["a"])).unwrap();
    store.snapshot("a", "x").unwrap();
    store.clear_all().unwrap();
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, vec![std::ffi::OsString::from(".lock")]);
    assert!(matches!(
        SessionStore::open(dir.path()),
        Err(SessionError::Busy)
    ));
}

#[test]
fn unsafe_tab_ids_are_rejected() {
    let dir = root();
    let store = SessionStore::open(dir.path()).unwrap();
    assert!(store.snapshot("../escape", "x").is_err());
}

#[test]
fn clone_survives_source_deletion() {
    let dir = root();
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    std::fs::write(&src, "protected bytes").unwrap();
    clone_protected(&src, &dst).unwrap();
    std::fs::remove_file(&src).unwrap();
    assert_eq!(std::fs::read(&dst).unwrap(), b"protected bytes");
    assert_eq!(
        std::fs::metadata(&dst).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn crash_child() {
    let Ok(root) = std::env::var("EDITOR_RS_CRASH_ROOT") else {
        return;
    };
    let store = SessionStore::open(Path::new(&root)).unwrap();
    store.write_manifest(&manifest(&["a"])).unwrap();
    store.snapshot("a", "").unwrap();
    let mut at = 0;
    loop {
        let digit = char::from(b'0' + (at % 10) as u8);
        store
            .append("a", &[insert(at, &digit.to_string())])
            .unwrap();
        at += 1;
    }
}

#[test]
fn killed_writer_always_leaves_a_valid_prefix() {
    let mut progressed = 0;
    for run in 0..50u64 {
        let dir = root();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "session::tests::crash_child", "--nocapture"])
            .env("EDITOR_RS_CRASH_ROOT", dir.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20 + run * 3));
        child.kill().unwrap();
        child.wait().unwrap();
        let store = SessionStore::open(dir.path()).unwrap();
        if let Some(tab) = store.restore().first()
            && let Some(text) = &tab.text
        {
            let expected: String = (0..text.len())
                .map(|i| char::from(b'0' + (i % 10) as u8))
                .collect();
            assert_eq!(text, &expected);
            progressed += usize::from(!text.is_empty());
        }
    }
    assert!(progressed > 0);
}
