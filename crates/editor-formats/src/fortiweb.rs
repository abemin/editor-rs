//! FortiWeb full-configuration backups.
//!
//! A backup is a text header followed by `[file]` sections. Readable sections
//! become editable text. Every other payload stays on disk as a protected
//! region, shown in the text as one placeholder character.

use crate::save::Chunk;
use memchr::memmem;
use std::borrow::Cow;
use std::fs::File;
use std::ops::Range;
use std::sync::Arc;

pub const MAGIC: &str = "CC05E358";
pub const PLACEHOLDER: char = '\u{FFFC}';

const HEADER_START: &[u8] = b"[header]\n";
const HEADER_END: &[u8] = b"\n[/header]\n";
const SECTION_START: &[u8] = b"[file]\n";
const SECTION_END: &[u8] = b"\n[/file]\n";

/// Bytes that are never decoded or edited.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtectedRegion {
    pub section: usize,
    pub name: String,
    pub offset: u64,
    pub len: u64,
    pub hash: [u8; 32],
}

#[derive(Clone, Debug)]
pub struct Container {
    pub separator: String,
    pub file_number: usize,
    pub regions: Vec<ProtectedRegion>,
    pub locked: Vec<usize>,
    pub zones: Vec<Range<usize>>,
    pub source: Arc<File>,
    /// Whether each section's payload ended with a newline when opened.
    pub ended_with_newline: Vec<bool>,
}

#[derive(Debug)]
pub enum ParseOutcome {
    NotContainer,
    Parsed { text: String, container: Container },
    Malformed { reason: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContainerError {
    SectionCount { header: usize, found: usize },
    SeparatorCount { expected: usize, found: usize },
    BadFraming { line: usize },
    LockedMissing,
}

impl std::fmt::Display for ContainerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContainerError::SectionCount { header, found } => {
                write!(
                    f,
                    "The header says file_number={header}, but the backup has {found} sections."
                )
            }
            ContainerError::SeparatorCount { expected, found } => {
                write!(
                    f,
                    "Expected {expected} section separators, but found {found}."
                )
            }
            ContainerError::BadFraming { line } => {
                write!(f, "The backup structure is damaged near line {line}.")
            }
            ContainerError::LockedMissing => {
                write!(f, "A protected section is missing from the backup.")
            }
        }
    }
}

struct RawSection {
    meta: Range<usize>,
    payload: Range<usize>,
}

fn header_value<'a>(header: &'a str, key: &str) -> Option<&'a str> {
    header
        .lines()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
}

fn line_of(bytes: &[u8], at: usize) -> usize {
    memchr::memchr_iter(b'\n', &bytes[..at.min(bytes.len())]).count() + 1
}

/// Walks the framing using the separator, never by searching payloads for markers.
fn walk(bytes: &[u8], start: usize, separator: &[u8]) -> Result<Vec<RawSection>, usize> {
    let finder = memmem::Finder::new(separator);
    let mut sections = Vec::new();
    let mut cursor = start;
    while cursor < bytes.len() {
        if !bytes[cursor..].starts_with(SECTION_START) {
            return Err(cursor);
        }
        let open = cursor + finder.find(&bytes[cursor..]).ok_or(cursor)?;
        let payload_start = open + separator.len() + 1;
        if bytes[open - 1] != b'\n'
            || bytes.get(open + separator.len()) != Some(&b'\n')
            || payload_start > bytes.len()
        {
            return Err(open);
        }
        let close = payload_start + finder.find(&bytes[payload_start..]).ok_or(payload_start)?;
        let after = close + separator.len();
        if !bytes[after..].starts_with(SECTION_END) {
            return Err(after);
        }
        sections.push(RawSection {
            meta: cursor + SECTION_START.len()..open,
            payload: payload_start..close,
        });
        cursor = after + SECTION_END.len();
    }
    Ok(sections)
}

fn malformed(reason: impl Into<String>) -> ParseOutcome {
    ParseOutcome::Malformed {
        reason: format!(
            "This FortiWeb backup could not be read safely, so it opens read-only. {}",
            reason.into()
        ),
    }
}

/// Recognizes and parses a backup.
pub fn parse(bytes: &[u8], source: Arc<File>) -> ParseOutcome {
    if !bytes.starts_with(HEADER_START) {
        return ParseOutcome::NotContainer;
    }
    let first_lines = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]);
    let Some(magic) = header_value(&first_lines, "magic") else {
        return ParseOutcome::NotContainer;
    };
    if magic != MAGIC {
        return malformed("Its format version is not recognized.");
    }
    let Some(end) = memmem::find(bytes, HEADER_END) else {
        return malformed("The header is incomplete.");
    };
    let body_start = end + HEADER_END.len();
    let Ok(header) = std::str::from_utf8(&bytes[..body_start]) else {
        return malformed("The header is not valid text.");
    };
    let Some(separator) = header_value(header, "file_split").filter(|s| !s.is_empty()) else {
        return malformed("The header has no section separator.");
    };
    let Some(file_number) =
        header_value(header, "file_number").and_then(|n| n.parse::<usize>().ok())
    else {
        return malformed("The header has no section count.");
    };
    let sections = match walk(bytes, body_start, separator.as_bytes()) {
        Ok(sections) => sections,
        Err(at) => {
            return malformed(format!(
                "The structure is damaged near line {}.",
                line_of(bytes, at)
            ));
        }
    };
    if sections.len() != file_number {
        return malformed(format!(
            "The header lists {file_number} sections, but {} were found.",
            sections.len()
        ));
    }

    let mut text = String::with_capacity(body_start);
    let mut chars = 0usize;
    let mut position = 0usize;
    let (mut regions, mut locked, mut zones, mut ended_with_newline) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut push = |text: &mut String, piece: &str| {
        text.push_str(piece);
        chars += piece.chars().count();
        chars
    };
    for (index, section) in sections.iter().enumerate() {
        let Ok(framing) = std::str::from_utf8(&bytes[position..section.payload.start]) else {
            return malformed("A section header is not valid text.");
        };
        let start = push(&mut text, framing);
        let meta = String::from_utf8_lossy(&bytes[section.meta.clone()]).into_owned();
        let payload = &bytes[section.payload.clone()];
        ended_with_newline.push(payload.last() == Some(&b'\n'));
        let plain = header_value(&meta, "encrypt") == Some("no")
            && header_value(&meta, "compress") == Some("no");
        match std::str::from_utf8(payload) {
            Ok(body) if plain && memchr::memchr(0, payload).is_none() => {
                let end = push(&mut text, body);
                zones.push(start..end);
            }
            _ => {
                locked.push(start);
                let mut buffer = [0u8; 4];
                push(&mut text, PLACEHOLDER.encode_utf8(&mut buffer));
                regions.push(ProtectedRegion {
                    section: index,
                    name: header_value(&meta, "name")
                        .unwrap_or("protected section")
                        .to_string(),
                    offset: section.payload.start as u64,
                    len: payload.len() as u64,
                    hash: *blake3::hash(payload).as_bytes(),
                });
            }
        }
        position = section.payload.end;
    }
    let Ok(tail) = std::str::from_utf8(&bytes[position..]) else {
        return malformed("The end of the backup is not valid text.");
    };
    push(&mut text, tail);
    let separator = separator.to_string();
    let container = Container {
        separator,
        file_number,
        regions,
        locked,
        zones,
        source,
        ended_with_newline,
    };
    ParseOutcome::Parsed { text, container }
}

fn walk_text(text: &str, locked: &[usize]) -> Result<Vec<RawSection>, ContainerError> {
    let bytes = text.as_bytes();
    let framing = ContainerError::BadFraming { line: 1 };
    let end = memmem::find(bytes, HEADER_END).ok_or(framing)?;
    let body_start = end + HEADER_END.len();
    let header = &text[..body_start];
    let separator = header_value(header, "file_split")
        .filter(|s| !s.is_empty())
        .ok_or(framing)?;
    let file_number: usize = header_value(header, "file_number")
        .and_then(|n| n.parse().ok())
        .ok_or(framing)?;

    let count = memmem::find_iter(bytes, separator.as_bytes()).count();
    let expected = 2 * file_number + 1;
    if count != expected {
        if count % 2 == 1 {
            return Err(ContainerError::SectionCount {
                header: file_number,
                found: (count - 1) / 2,
            });
        }
        return Err(ContainerError::SeparatorCount {
            expected,
            found: count,
        });
    }
    let sections =
        walk(bytes, body_start, separator.as_bytes()).map_err(|at| ContainerError::BadFraming {
            line: line_of(bytes, at),
        })?;
    if sections.len() != file_number {
        return Err(ContainerError::SectionCount {
            header: file_number,
            found: sections.len(),
        });
    }

    let mut placeholder = [0u8; 4];
    let placeholder = PLACEHOLDER.encode_utf8(&mut placeholder).as_bytes();
    let (mut chars, mut position, mut found) = (0usize, 0usize, 0usize);
    for section in &sections {
        chars += text[position..section.payload.start].chars().count();
        position = section.payload.start;
        if &bytes[section.payload.clone()] == placeholder {
            if locked.get(found) != Some(&chars) {
                return Err(ContainerError::LockedMissing);
            }
            found += 1;
        }
    }
    if found != locked.len() {
        return Err(ContainerError::LockedMissing);
    }
    Ok(sections)
}

/// Checks that edited text is still a complete backup.
pub fn validate(text: &str, locked: &[usize]) -> Result<(), ContainerError> {
    walk_text(text, locked).map(|_| ())
}

/// Builds the bytes to save: edited text with protected regions streamed from the source.
pub fn chunks<'a>(
    text: &'a str,
    locked: &[usize],
    container: &'a Container,
) -> Result<Vec<Chunk<'a>>, ContainerError> {
    let sections = walk_text(text, locked)?;
    if locked.len() != container.regions.len() {
        return Err(ContainerError::LockedMissing);
    }
    let mut placeholder = [0u8; 4];
    let placeholder = PLACEHOLDER
        .encode_utf8(&mut placeholder)
        .as_bytes()
        .to_vec();
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(sections.len() * 2 + 1);
    let mut regions = container.regions.iter();
    let mut position = 0usize;
    for (index, section) in sections.iter().enumerate() {
        let payload = &bytes[section.payload.clone()];
        if payload == placeholder.as_slice() {
            let region = regions.next().ok_or(ContainerError::LockedMissing)?;
            out.push(Chunk::Bytes(Cow::Borrowed(
                &bytes[position..section.payload.start],
            )));
            out.push(Chunk::SourceRange {
                file: &container.source,
                offset: region.offset,
                len: region.len,
                expected_hash: region.hash,
            });
            position = section.payload.end;
        } else if payload.last() != Some(&b'\n')
            && container.ended_with_newline.get(index) == Some(&true)
        {
            out.push(Chunk::Bytes(Cow::Borrowed(
                &bytes[position..section.payload.end],
            )));
            out.push(Chunk::Bytes(Cow::Borrowed(b"\n")));
            position = section.payload.end;
        }
    }
    out.push(Chunk::Bytes(Cow::Borrowed(&bytes[position..])));
    Ok(out)
}
