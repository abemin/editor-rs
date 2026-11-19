//! Opening files from disk.

use crate::encoding::{OpenMode, TextEncoding, decode_bytes, dominant_eol};
use crate::fortiweb::Container;
use editor_core::Eol;
use memmap2::Mmap;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Text,
    FortiWebBackup,
}

/// What the file looked like when it was opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceIdentity {
    pub path: PathBuf,
    pub len: u64,
    pub modified: SystemTime,
}

#[derive(Debug)]
pub struct LoadedFile {
    pub text: String,
    pub encoding: TextEncoding,
    pub eol: Eol,
    pub mode: OpenMode,
    pub kind: FileKind,
    pub source: Option<SourceIdentity>,
    pub container: Option<Container>,
}

pub fn load_file(path: &Path) -> io::Result<LoadedFile> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Only regular files can be opened.",
        ));
    }
    let source = Some(SourceIdentity {
        path: path.to_path_buf(),
        len: metadata.len(),
        modified: metadata.modified()?,
    });
    // SAFETY: the map is read once while loading and dropped before returning.
    let map = if metadata.len() == 0 {
        None
    } else {
        Some(unsafe { Mmap::map(&file)? })
    };
    let bytes: &[u8] = map.as_deref().unwrap_or(&[]);
    let (text, encoding, mode) = decode_bytes(bytes);
    let eol = dominant_eol(&text);
    Ok(LoadedFile {
        text,
        encoding,
        eol,
        mode,
        kind: FileKind::Text,
        source,
        container: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_a_text_file_with_its_identity() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "a\r\nb\r\n").unwrap();
        let file = load_file(&path).unwrap();
        assert_eq!(file.text, "a\r\nb\r\n");
        assert_eq!(file.eol, editor_core::Eol::CrLf);
        assert_eq!(file.kind, FileKind::Text);
        assert_eq!(file.source.unwrap().len, 6);
    }

    #[test]
    fn loads_an_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty");
        std::fs::write(&path, "").unwrap();
        assert_eq!(load_file(&path).unwrap().text, "");
    }

    #[test]
    fn missing_file_is_an_error() {
        assert!(load_file(std::path::Path::new("/nonexistent/file")).is_err());
    }

    #[test]
    fn folder_is_an_error() {
        assert!(load_file(tempfile::tempdir().unwrap().path()).is_err());
    }
}
