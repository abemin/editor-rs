//! One open document and everything the view needs to show it.

use editor_core::{Buffer, Document, Eol};
use editor_formats::fortiweb::Container;
use editor_formats::{FileKind, LoadedFile, OpenMode, SourceIdentity, TextEncoding};
use editor_syntax::Language;
use std::path::{Path, PathBuf};

pub struct EditorDoc {
    pub doc: Document,
    pub path: Option<PathBuf>,
    pub encoding: TextEncoding,
    pub container: Option<Container>,
    pub kind: FileKind,
    pub language: Language,
    pub read_only: Option<String>,
    pub source: Option<SourceIdentity>,
    pub word_wrap: bool,
    pub max_columns: usize,
    /// Name shown when there is no file path, such as a restored tab whose file is gone.
    pub name_hint: Option<String>,
}

/// Longest line in characters, measured quickly on bytes.
pub fn longest_line(text: &str) -> usize {
    let bytes = text.as_bytes();
    let mut longest = 0;
    let mut start = 0;
    for end in memchr_newlines(bytes) {
        longest = longest.max(end - start);
        start = end + 1;
    }
    longest.max(bytes.len() - start.min(bytes.len()))
}

fn memchr_newlines(bytes: &[u8]) -> impl Iterator<Item = usize> + '_ {
    bytes
        .iter()
        .enumerate()
        .filter(|(_, b)| **b == b'\n' || **b == b'\r')
        .map(|(i, _)| i)
}

impl EditorDoc {
    pub fn untitled() -> Self {
        Self {
            doc: Document::new("", Eol::Lf),
            path: None,
            encoding: TextEncoding::Utf8 { bom: false },
            container: None,
            kind: FileKind::Text,
            language: Language::Plain,
            read_only: None,
            source: None,
            word_wrap: false,
            max_columns: 0,
            name_hint: None,
        }
    }

    pub fn from_loaded(file: LoadedFile, path: &Path) -> Self {
        let max_columns = longest_line(&file.text);
        let buffer = match &file.container {
            Some(c) => Buffer::with_structure(&file.text, c.locked.clone(), c.zones.clone()),
            None => Buffer::new(&file.text),
        };
        let first_lines: String = file.text.chars().take(4096).collect();
        let language = editor_syntax::detect(Some(path), &first_lines);
        let read_only = match file.mode {
            OpenMode::ReadOnly { reason } => Some(reason),
            OpenMode::Editable => None,
        };
        let mut doc = Document::from_buffer(buffer, file.eol);
        if read_only.is_some() {
            let mut buffer = doc.buffer().clone();
            buffer.set_read_only(true);
            doc = Document::from_buffer(buffer, file.eol);
        }
        Self {
            doc,
            path: Some(path.to_path_buf()),
            encoding: file.encoding,
            container: file.container,
            kind: file.kind,
            language,
            read_only,
            source: file.source,
            word_wrap: false,
            max_columns,
            name_hint: None,
        }
    }

    pub fn title(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .or_else(|| self.name_hint.clone())
            .unwrap_or_else(|| "Untitled".to_string())
    }

    /// Label shown in place of a protected section.
    pub fn locked_label(&self, index: usize) -> String {
        let Some(region) = self.container.as_ref().and_then(|c| c.regions.get(index)) else {
            return "[locked section]".to_string();
        };
        format!(
            "[locked: {} \u{b7} {}]",
            region.name,
            format_size(region.len)
        )
    }
}

pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["bytes", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit + 1 < UNITS.len() {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} bytes")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
