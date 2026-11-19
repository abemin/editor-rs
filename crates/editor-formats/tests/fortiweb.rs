mod fixtures;

use editor_core::{Buffer, Document, Eol, SelectionSet};
use editor_formats::fortiweb::{ContainerError, PLACEHOLDER, chunks, validate};
use editor_formats::{FileKind, LoadedFile, OpenMode, SaveError, atomic_write, load_file};
use fixtures::*;
use std::path::{Path, PathBuf};

const HARD: &[u8] = b"\x1f\x8b\0\r\xff\xfe\r\n\0\xc3\x28\rA\rB\r\r\n\n\x80";

fn write(bytes: &[u8]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("backup.conf");
    std::fs::write(&path, bytes).unwrap();
    (dir, path)
}

fn document(file: &LoadedFile) -> Document {
    let c = file.container.as_ref().unwrap();
    Document::from_buffer(
        Buffer::with_structure(&file.text, c.locked.clone(), c.zones.clone()),
        Eol::Lf,
    )
}

fn save(file: &LoadedFile, doc: &Document, path: &Path) -> Result<(), SaveError> {
    let c = chunks_for(file, doc);
    atomic_write(path, &c)
}

fn chunks_for<'a>(file: &'a LoadedFile, doc: &Document) -> Vec<editor_formats::Chunk<'a>> {
    let text: &'static str = Box::leak(doc.text().into_boxed_str());
    chunks(
        text,
        doc.buffer().locked(),
        file.container.as_ref().unwrap(),
    )
    .unwrap()
}

fn typical() -> Vec<u8> {
    build_backup(&[
        text(
            "/data/config/sys_global.conf.gz",
            "config system global\n  set hostname demo\nend\n",
        ),
        encrypted(HARD),
    ])
}

#[test]
fn opens_as_a_backup_with_one_locked_placeholder() {
    let (_d, path) = write(&typical());
    let file = load_file(&path).unwrap();
    assert_eq!(file.kind, FileKind::FortiWebBackup);
    assert_eq!(file.mode, OpenMode::Editable);
    assert_eq!(file.text.matches(PLACEHOLDER).count(), 1);
    let c = file.container.as_ref().unwrap();
    assert_eq!(c.regions.len(), 1);
    assert_eq!(c.regions[0].name, "/tmp/extend_tar_file");
    assert_eq!(c.regions[0].len, HARD.len() as u64);
    assert_eq!(c.zones.len(), 1);
}

#[test]
fn unedited_backup_saves_byte_identical() {
    let original = typical();
    let (_d, path) = write(&original);
    let file = load_file(&path).unwrap();
    save(&file, &document(&file), &path).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn edit_changes_only_the_edited_bytes() {
    let original = typical();
    let (_d, path) = write(&original);
    let file = load_file(&path).unwrap();
    let mut doc = document(&file);
    let at = file.text.chars().collect::<String>().find("demo").unwrap();
    doc.set_selections(SelectionSet::single(file.text[..at].chars().count()));
    doc.insert_text("new-").unwrap();
    save(&file, &doc, &path).unwrap();
    let saved = std::fs::read(&path).unwrap();
    assert_eq!(saved.len(), original.len() + 4);
    let pos = original.windows(4).position(|w| w == b"demo").unwrap();
    assert_eq!(&saved[..pos], &original[..pos]);
    assert_eq!(&saved[pos..pos + 4], b"new-");
    assert_eq!(&saved[pos + 4..], &original[pos..]);
}

#[test]
fn plain_section_after_encrypted_is_editable() {
    let bytes = build_backup(&[
        encrypted(HARD),
        text("/data/config/late.conf", "config late\nend\n"),
    ]);
    let (_d, path) = write(&bytes);
    let file = load_file(&path).unwrap();
    assert!(file.text.contains("config late"));
    assert_eq!(file.container.as_ref().unwrap().zones.len(), 1);
    save(&file, &document(&file), &path).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

#[test]
fn encrypt_no_but_binary_payload_is_protected() {
    let ml = FixtureSection {
        name: "/var/log/ml-backup/ml_db_bak.enc",
        kind: "ml",
        encrypt: "no",
        compress: "no",
        payload: HARD.to_vec(),
    };
    let (_d, path) = write(&build_backup(&[ml]));
    let file = load_file(&path).unwrap();
    assert_eq!(file.container.as_ref().unwrap().regions.len(), 1);
    assert!(file.container.as_ref().unwrap().zones.is_empty());
}

#[test]
fn payload_with_lone_cr_nul_and_invalid_utf8_is_preserved() {
    let bytes = build_backup(&[encrypted(HARD), encrypted(b"\r"), encrypted(b"")]);
    let (_d, path) = write(&bytes);
    let file = load_file(&path).unwrap();
    save(&file, &document(&file), &path).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

#[test]
fn payload_containing_file_marker_is_not_a_section() {
    let bytes = build_backup(&[text("/a", "x\n[file]\nname=fake\n[/file]\ny\n")]);
    let (_d, path) = write(&bytes);
    let file = load_file(&path).unwrap();
    assert_eq!(file.kind, FileKind::FortiWebBackup);
    validate(&file.text, &file.container.as_ref().unwrap().locked).unwrap();
    save(&file, &document(&file), &path).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

#[test]
fn final_newline_of_text_payload_is_restored() {
    let bytes = build_backup(&[text("/a", "line\n")]);
    let (_d, path) = write(&bytes);
    let file = load_file(&path).unwrap();
    let mut doc = document(&file);
    let z = file.container.as_ref().unwrap().zones[0].clone();
    doc.set_selections(SelectionSet::single(z.end));
    doc.backspace().unwrap();
    save(&file, &doc, &path).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

#[test]
fn text_payload_that_never_had_a_final_newline_stays_that_way() {
    let bytes = build_backup(&[text("/a", "no newline")]);
    let (_d, path) = write(&bytes);
    let file = load_file(&path).unwrap();
    save(&file, &document(&file), &path).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

#[test]
fn optional_header_keys_are_preserved() {
    let bytes = build_backup_with("private_key=synthetic\n", &[text("/a", "x\n")]);
    let (_d, path) = write(&bytes);
    let file = load_file(&path).unwrap();
    save(&file, &document(&file), &path).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

#[test]
fn renaming_the_separator_everywhere_is_valid() {
    let (_d, path) = write(&typical());
    let file = load_file(&path).unwrap();
    let renamed = file.text.replace("FV000X", "FV111Y");
    let locked = vec![renamed.chars().position(|c| c == PLACEHOLDER).unwrap()];
    validate(&renamed, &locked).unwrap();
}

#[test]
fn changed_file_number_refuses_save() {
    let (_d, path) = write(&typical());
    let file = load_file(&path).unwrap();
    let edited = file.text.replace("file_number=2", "file_number=3");
    let locked = vec![edited.chars().position(|c| c == PLACEHOLDER).unwrap()];
    assert_eq!(
        validate(&edited, &locked).unwrap_err(),
        ContainerError::SectionCount {
            header: 3,
            found: 2
        }
    );
}

#[test]
fn damaged_separator_refuses_save() {
    let (_d, path) = write(&typical());
    let file = load_file(&path).unwrap();
    let at = file.text.rfind(SEP).unwrap();
    let mut edited = file.text.clone();
    edited.replace_range(at..at + 1, "x");
    let locked = vec![edited.chars().position(|c| c == PLACEHOLDER).unwrap()];
    assert!(matches!(
        validate(&edited, &locked).unwrap_err(),
        ContainerError::SeparatorCount { .. }
    ));
}

#[test]
fn removed_placeholder_refuses_save() {
    let (_d, path) = write(&typical());
    let file = load_file(&path).unwrap();
    let edited = file.text.replace(PLACEHOLDER, "");
    let result = chunks(&edited, &[], file.container.as_ref().unwrap());
    assert_eq!(result.err(), Some(ContainerError::LockedMissing));
}

#[test]
fn malformed_container_is_reported_not_guessed() {
    let mut bytes = typical();
    bytes.truncate(bytes.len() - 12);
    let (_d, path) = write(&bytes);
    let file = load_file(&path).unwrap();
    assert_eq!(file.kind, FileKind::Text);
    assert!(file.container.is_none());
    assert!(matches!(file.mode, OpenMode::ReadOnly { .. }));
}

#[test]
fn plain_text_starting_with_header_is_not_a_container() {
    let (_d, path) = write(b"[header]\nnot a backup\n[/header]\n");
    let file = load_file(&path).unwrap();
    assert_eq!(file.kind, FileKind::Text);
    assert_eq!(file.mode, OpenMode::Editable);
}

#[test]
fn source_changed_after_open_refuses_save() {
    let original = typical();
    let (_d, path) = write(&original);
    let file = load_file(&path).unwrap();
    let doc = document(&file);
    let mut changed = original.clone();
    let at = original.len() - SEP.len() - 12;
    changed[at] ^= 0xFF;
    std::fs::write(&path, &changed).unwrap();
    assert!(matches!(
        save(&file, &doc, &path),
        Err(SaveError::SourceChanged)
    ));
    assert_eq!(std::fs::read(&path).unwrap(), changed);
}

#[test]
fn saving_twice_keeps_protected_bytes() {
    let original = typical();
    let (_d, path) = write(&original);
    let file = load_file(&path).unwrap();
    let doc = document(&file);
    save(&file, &doc, &path).unwrap();
    save(&file, &doc, &path).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), original);
}
