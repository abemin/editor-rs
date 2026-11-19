//! Checks that real FortiWeb backups survive open, save and a one-line edit.
//! Prints only structure and sizes, never configuration contents.

use editor_core::{Buffer, Document, SelectionSet};
use editor_formats::fortiweb::chunks;
use editor_formats::{FileKind, atomic_write, load_file};
use std::path::Path;

fn check(path: &Path) -> Result<String, String> {
    let original = std::fs::read(path).map_err(|e| e.to_string())?;
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let copy = dir.path().join("copy.conf");
    std::fs::write(&copy, &original).map_err(|e| e.to_string())?;
    let file = load_file(&copy).map_err(|e| e.to_string())?;
    if file.kind != FileKind::FortiWebBackup {
        return Err("not recognized as a FortiWeb backup".into());
    }
    let container = file.container.as_ref().expect("container");
    let save = |doc: &Document, target: &Path| -> Result<(), String> {
        let text = doc.text();
        let parts = chunks(&text, doc.buffer().locked(), container).map_err(|e| e.to_string())?;
        atomic_write(target, &parts).map_err(|e| e.to_string())
    };
    let new_doc = || {
        Document::from_buffer(
            Buffer::with_structure(
                &file.text,
                container.locked.clone(),
                container.zones.clone(),
            ),
            file.eol,
        )
    };

    let unchanged = dir.path().join("unchanged.conf");
    save(&new_doc(), &unchanged)?;
    if std::fs::read(&unchanged).map_err(|e| e.to_string())? != original {
        return Err("unedited save is not byte-identical".into());
    }

    let mut doc = new_doc();
    let zone = container.zones.first().ok_or("no editable section")?;
    let rope = doc.buffer().rope().clone();
    let line = rope.char_to_line(zone.start) + 1;
    let at = doc.line_text_end(line);
    doc.set_selections(SelectionSet::single(at));
    doc.insert_text(" ").map_err(|e| format!("{e:?}"))?;
    let edited = dir.path().join("edited.conf");
    save(&doc, &edited)?;
    let saved = std::fs::read(&edited).map_err(|e| e.to_string())?;
    let byte_at = rope.char_to_byte(at);
    let mut expected = original[..byte_at].to_vec();
    expected.push(b' ');
    expected.extend_from_slice(&original[byte_at..]);
    if saved != expected {
        return Err("edited save changed bytes outside the edit".into());
    }
    let reloaded = load_file(&edited).map_err(|e| e.to_string())?;
    let regions = &reloaded
        .container
        .as_ref()
        .ok_or("edited file no longer a backup")?
        .regions;
    let same = regions.len() == container.regions.len()
        && regions
            .iter()
            .zip(&container.regions)
            .all(|(a, b)| a.hash == b.hash && a.len == b.len);
    if !same {
        return Err("protected sections changed".into());
    }
    let header = String::from_utf8_lossy(&original[..original.len().min(400)]).into_owned();
    let firmware = header
        .lines()
        .find_map(|l| l.strip_prefix("image_version="))
        .unwrap_or("?")
        .to_string();
    let protected: u64 = container.regions.iter().map(|r| r.len).sum();
    Ok(format!(
        "firmware {firmware}, {} sections, {} editable, {} protected ({:.1} MB)",
        container.file_number,
        container.zones.len(),
        container.regions.len(),
        protected as f64 / 1e6
    ))
}

fn main() {
    let mut failed = false;
    for arg in std::env::args().skip(1) {
        let path = Path::new(&arg);
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match check(path) {
            Ok(summary) => println!("PASS  {name}: {summary}"),
            Err(problem) => {
                failed = true;
                println!("FAIL  {name}: {problem}");
            }
        }
    }
    std::process::exit(i32::from(failed));
}
