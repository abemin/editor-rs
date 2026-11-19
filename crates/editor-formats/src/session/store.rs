//! The on-disk session folder.

use super::journal::{decode_records, encode_record};
use editor_core::{Buffer, EditBatch};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const MANIFEST_VERSION: u32 = 1;

const MANIFEST: &str = "manifest.json";
const LOCK: &str = ".lock";
const SNAPSHOT_MAGIC: &[u8; 4] = b"ERS1";
const SNAPSHOT_HEADER: usize = 4 + 8 + 32;
const GENERATION_LEN: usize = 8;
const EXCLUDE_ATTRIBUTE: &str = "com.apple.metadata:com_apple_backup_excludeItem";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabRecord {
    pub id: String,
    pub path: Option<PathBuf>,
    pub title: String,
    pub dirty: bool,
    pub caret: usize,
    pub scroll_line: usize,
    pub bookmarks: Vec<usize>,
    pub word_wrap: bool,
    pub language: Option<String>,
    pub encoding: String,
    pub source_len: Option<u64>,
    pub source_modified_ms: Option<i64>,
    pub has_protected_clone: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub active: Option<String>,
    pub tabs: Vec<TabRecord>,
}

#[derive(Debug)]
pub enum SessionError {
    Busy,
    Io(io::Error),
}

impl From<io::Error> for SessionError {
    fn from(error: io::Error) -> Self {
        SessionError::Io(error)
    }
}

#[derive(Clone, Debug)]
pub struct RestoredTab {
    pub record: TabRecord,
    pub text: Option<String>,
    pub recovered_partial: bool,
}

pub struct SessionStore {
    root: PathBuf,
    _lock: File,
    generations: Mutex<HashMap<String, u64>>,
}

fn private_file() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.write(true).create(true).mode(0o600);
    options
}

fn check_id(id: &str) -> io::Result<()> {
    let valid = !id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if valid {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid tab id",
        ))
    }
}

fn exclude_from_backups(path: &Path) -> io::Result<()> {
    let target = CString::new(path.as_os_str().as_bytes())?;
    let name = CString::new(EXCLUDE_ATTRIBUTE)?;
    let value = b"com.apple.backupd";
    // SAFETY: pointers are valid for the call and the length matches `value`.
    let status = unsafe {
        libc::setxattr(
            target.as_ptr(),
            name.as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
            0,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

impl SessionStore {
    /// Opens the session folder and takes exclusive ownership of it.
    pub fn open(root: &Path) -> Result<Self, SessionError> {
        std::fs::create_dir_all(root)?;
        std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))?;
        exclude_from_backups(root)?;
        let lock = private_file().truncate(false).open(root.join(LOCK))?;
        // SAFETY: the descriptor is owned by `lock` and open.
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            let error = io::Error::last_os_error();
            return Err(if error.raw_os_error() == Some(libc::EWOULDBLOCK) {
                SessionError::Busy
            } else {
                SessionError::Io(error)
            });
        }
        Ok(Self {
            root: root.to_path_buf(),
            _lock: lock,
            generations: Mutex::new(HashMap::new()),
        })
    }

    pub(super) fn snapshot_path(&self, id: &str) -> PathBuf {
        self.root.join(format!("{id}.snap"))
    }

    pub(super) fn journal_path(&self, id: &str) -> PathBuf {
        self.root.join(format!("{id}.jrnl"))
    }

    /// Where a backup's protected sections are cloned for this tab.
    pub fn protected_path(&self, id: &str) -> PathBuf {
        self.root.join(format!("{id}.protected"))
    }

    fn write_atomic(&self, path: &Path, chunks: &[&[u8]]) -> io::Result<()> {
        let temp = path.with_extension("tmp");
        let mut file = private_file().truncate(true).open(&temp)?;
        for chunk in chunks {
            file.write_all(chunk)?;
        }
        file.sync_all()?;
        std::fs::rename(&temp, path)?;
        File::open(&self.root)?.sync_all()
    }

    pub fn write_manifest(&self, manifest: &Manifest) -> io::Result<()> {
        self.write_atomic(&self.root.join(MANIFEST), &[&serde_json::to_vec(manifest)?])
    }

    fn generation(&self, id: &str) -> u64 {
        let mut generations = self.generations.lock().expect("generation lock");
        *generations.entry(id.to_string()).or_insert_with(|| {
            read_snapshot(&self.snapshot_path(id)).map_or(0, |(generation, _)| generation)
        })
    }

    /// Stores the full text and starts a new journal.
    pub fn snapshot(&self, id: &str, text: &str) -> io::Result<()> {
        check_id(id)?;
        let generation = self.generation(id) + 1;
        let hash = blake3::hash(text.as_bytes());
        let number = generation.to_le_bytes();
        let parts: [&[u8]; 4] = [SNAPSHOT_MAGIC, &number, hash.as_bytes(), text.as_bytes()];
        self.write_atomic(&self.snapshot_path(id), &parts)?;
        self.generations
            .lock()
            .expect("generation lock")
            .insert(id.to_string(), generation);
        match std::fs::remove_file(self.journal_path(id)) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }

    /// Durably records edits made since the last snapshot.
    pub fn append(&self, id: &str, batches: &[EditBatch]) -> io::Result<()> {
        check_id(id)?;
        let generation = self.generation(id);
        let path = self.journal_path(id);
        let mut data = Vec::new();
        if !path.exists() {
            data.extend_from_slice(&generation.to_le_bytes());
        }
        batches
            .iter()
            .for_each(|batch| encode_record(batch, &mut data));
        let mut file = private_file().append(true).open(&path)?;
        file.write_all(&data)?;
        file.sync_all()
    }

    fn restore_text(&self, id: &str) -> (Option<String>, bool) {
        if check_id(id).is_err() {
            return (None, true);
        }
        let snapshot_exists = self.snapshot_path(id).exists();
        let snapshot = read_snapshot(&self.snapshot_path(id));
        let mut partial = snapshot_exists && snapshot.is_none();
        let (generation, base) = snapshot.unwrap_or((0, String::new()));
        let journal = std::fs::read(self.journal_path(id)).ok();
        if !snapshot_exists && journal.is_none() {
            return (None, false);
        }
        let mut buffer = Buffer::new(&base);
        if let Some(bytes) = journal
            && bytes.len() >= GENERATION_LEN
            && bytes[..GENERATION_LEN] == generation.to_le_bytes()
        {
            let (batches, damaged) = decode_records(&bytes[GENERATION_LEN..]);
            partial |= damaged;
            for batch in batches {
                if buffer.apply(&batch).is_err() {
                    partial = true;
                    break;
                }
            }
        }
        (Some(buffer.rope().to_string()), partial)
    }

    fn set_manifest_aside(&self) {
        let path = self.root.join(MANIFEST);
        let _ = std::fs::rename(&path, path.with_extension("json.unsupported"));
    }

    /// Reads everything that can be recovered. Damaged data never stops the app.
    pub fn restore(&self) -> Vec<RestoredTab> {
        let Ok(bytes) = std::fs::read(self.root.join(MANIFEST)) else {
            return Vec::new();
        };
        let manifest: Manifest = match serde_json::from_slice(&bytes) {
            Ok(manifest) => manifest,
            Err(_) => {
                self.set_manifest_aside();
                return Vec::new();
            }
        };
        if manifest.version != MANIFEST_VERSION {
            self.set_manifest_aside();
            return Vec::new();
        }
        manifest
            .tabs
            .into_iter()
            .map(|record| {
                let (text, recovered_partial) = self.restore_text(&record.id);
                RestoredTab {
                    record,
                    text,
                    recovered_partial,
                }
            })
            .collect()
    }

    /// Removes everything stored for one tab.
    pub fn discard(&self, id: &str) -> io::Result<()> {
        check_id(id)?;
        self.generations.lock().expect("generation lock").remove(id);
        for path in [
            self.snapshot_path(id),
            self.journal_path(id),
            self.protected_path(id),
        ] {
            match std::fs::remove_file(path) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
                _ => {}
            }
        }
        Ok(())
    }

    /// Removes all session data.
    pub fn clear_all(&self) -> io::Result<()> {
        self.generations.lock().expect("generation lock").clear();
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            if entry.file_name() != LOCK {
                std::fs::remove_file(entry.path())?;
            }
        }
        Ok(())
    }
}

fn read_snapshot(path: &Path) -> Option<(u64, String)> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() < SNAPSHOT_HEADER || &bytes[..4] != SNAPSHOT_MAGIC {
        return None;
    }
    let generation = u64::from_le_bytes(bytes[4..12].try_into().ok()?);
    let body = &bytes[SNAPSHOT_HEADER..];
    if blake3::hash(body).as_bytes().as_slice() != &bytes[12..SNAPSHOT_HEADER] {
        return None;
    }
    Some((generation, String::from_utf8(body.to_vec()).ok()?))
}
