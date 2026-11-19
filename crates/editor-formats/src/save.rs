//! Atomic, verified saving.

use std::borrow::Cow;
use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const COPY_BUFFER: usize = 1024 * 1024;
const COPYFILE_ACL: u32 = 1 << 0;
const COPYFILE_XATTR: u32 = 1 << 2;

unsafe extern "C" {
    fn copyfile(
        from: *const libc::c_char,
        to: *const libc::c_char,
        state: *mut libc::c_void,
        flags: u32,
    ) -> libc::c_int;
}

/// One piece of the file being written.
pub enum Chunk<'a> {
    Bytes(Cow<'a, [u8]>),
    SourceRange {
        file: &'a File,
        offset: u64,
        len: u64,
        expected_hash: [u8; 32],
    },
}

#[derive(Debug)]
pub enum SaveError {
    Io(io::Error),
    SourceChanged,
    VerifyFailed,
    ReadOnly,
}

impl From<io::Error> for SaveError {
    fn from(error: io::Error) -> Self {
        SaveError::Io(error)
    }
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::Io(error) => write!(f, "The file could not be saved: {error}"),
            SaveError::SourceChanged => write!(
                f,
                "The original file changed after it was opened, so its protected sections can no longer be copied safely."
            ),
            SaveError::VerifyFailed => write!(
                f,
                "The saved data did not read back correctly, so the original file was kept."
            ),
            SaveError::ReadOnly => write!(f, "This file is read-only."),
        }
    }
}

struct TempFile(PathBuf, bool);

impl Drop for TempFile {
    fn drop(&mut self) {
        if !self.1 {
            let _ = std::fs::remove_file(&self.0);
        }
    }
}

fn c_path(path: &Path) -> io::Result<CString> {
    CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
}

fn resolve_target(target: &Path) -> io::Result<PathBuf> {
    match std::fs::canonicalize(target) {
        Ok(real) => Ok(real),
        Err(error) if error.kind() == io::ErrorKind::NotFound => match std::fs::read_link(target) {
            Ok(link) if link.is_absolute() => Ok(link),
            Ok(link) => Ok(target.parent().unwrap_or(Path::new(".")).join(link)),
            Err(_) => Ok(target.to_path_buf()),
        },
        Err(error) => Err(error),
    }
}

fn write_chunk(
    out: &mut File,
    chunk: &Chunk<'_>,
    hasher: &mut blake3::Hasher,
) -> Result<(), SaveError> {
    match chunk {
        Chunk::Bytes(bytes) => {
            out.write_all(bytes)?;
            hasher.update(bytes);
        }
        Chunk::SourceRange {
            file,
            offset,
            len,
            expected_hash,
        } => {
            let mut buffer = vec![0u8; COPY_BUFFER];
            let mut range_hash = blake3::Hasher::new();
            let mut done = 0u64;
            while done < *len {
                let want = (*len - done).min(COPY_BUFFER as u64) as usize;
                file.read_exact_at(&mut buffer[..want], offset + done)
                    .map_err(|_| SaveError::SourceChanged)?;
                out.write_all(&buffer[..want])?;
                hasher.update(&buffer[..want]);
                range_hash.update(&buffer[..want]);
                done += want as u64;
            }
            if range_hash.finalize().as_bytes() != expected_hash {
                return Err(SaveError::SourceChanged);
            }
        }
    }
    Ok(())
}

fn hash_file(path: &Path) -> io::Result<blake3::Hash> {
    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0u8; COPY_BUFFER];
    loop {
        match file.read(&mut buffer)? {
            0 => return Ok(hasher.finalize()),
            n => {
                hasher.update(&buffer[..n]);
            }
        }
    }
}

/// Writes `chunks` to `target`. The original stays untouched unless everything succeeds.
pub fn atomic_write(target: &Path, chunks: &[Chunk<'_>]) -> Result<(), SaveError> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let real = resolve_target(target)?;
    let existing = std::fs::metadata(&real).ok();
    if existing
        .as_ref()
        .is_some_and(|m| m.permissions().mode() & 0o222 == 0)
    {
        return Err(SaveError::ReadOnly);
    }
    if existing.is_some() && unsafe { libc::access(c_path(&real)?.as_ptr(), libc::W_OK) } != 0 {
        return Err(SaveError::ReadOnly);
    }
    let folder = real
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = real
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp_path = folder.join(format!(".{name}.saving-{}-{unique}", std::process::id()));
    let mut out = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o666)
        .open(&temp_path)?;
    let mut temp = TempFile(temp_path, false);

    let mut hasher = blake3::Hasher::new();
    for chunk in chunks {
        write_chunk(&mut out, chunk, &mut hasher)?;
    }
    out.sync_all()?;
    drop(out);
    if hash_file(&temp.0)? != hasher.finalize() {
        return Err(SaveError::VerifyFailed);
    }
    if let Some(metadata) = &existing {
        let (from, to) = (c_path(&real)?, c_path(&temp.0)?);
        // Extended attributes are best effort: some file systems do not support them.
        let _ = unsafe {
            copyfile(
                from.as_ptr(),
                to.as_ptr(),
                std::ptr::null_mut(),
                COPYFILE_XATTR | COPYFILE_ACL,
            )
        };
        std::fs::set_permissions(&temp.0, metadata.permissions())?;
    }
    std::fs::rename(&temp.0, &real)?;
    temp.1 = true;
    File::open(folder)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;
    use std::os::unix::fs::{PermissionsExt, symlink};

    fn bytes(b: &[u8]) -> Chunk<'_> {
        Chunk::Bytes(Cow::Borrowed(b))
    }
    fn entries(dir: &Path) -> usize {
        std::fs::read_dir(dir).unwrap().count()
    }

    #[test]
    fn writes_exact_bytes_and_keeps_mode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a");
        std::fs::write(&path, "old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        atomic_write(&path, &[bytes(b"new\r\n"), bytes(b"\0\xFF")]).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new\r\n\0\xFF");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o640
        );
        assert_eq!(entries(dir.path()), 1);
    }

    #[test]
    fn creates_a_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new.txt");
        atomic_write(&path, &[bytes(b"x")]).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"x");
    }

    #[test]
    fn save_through_symlink_keeps_link() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        let link = dir.path().join("link");
        std::fs::write(&real, "old").unwrap();
        symlink(&real, &link).unwrap();
        atomic_write(&link, &[bytes(b"new")]).unwrap();
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read(&real).unwrap(), b"new");
    }

    #[test]
    fn save_to_read_only_file_fails_and_keeps_original() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a");
        std::fs::write(&path, "old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
        assert!(matches!(
            atomic_write(&path, &[bytes(b"new")]),
            Err(SaveError::ReadOnly)
        ));
        assert_eq!(std::fs::read(&path).unwrap(), b"old");
        assert_eq!(entries(dir.path()), 1);
    }

    #[test]
    fn failed_write_keeps_original_and_removes_temp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a");
        let source = dir.path().join("source");
        std::fs::write(&path, "old").unwrap();
        std::fs::write(&source, "payload").unwrap();
        let file = File::open(&source).unwrap();
        let chunk = Chunk::SourceRange {
            file: &file,
            offset: 0,
            len: 7,
            expected_hash: [0; 32],
        };
        assert!(matches!(
            atomic_write(&path, &[bytes(b"x"), chunk]),
            Err(SaveError::SourceChanged)
        ));
        assert_eq!(std::fs::read(&path).unwrap(), b"old");
        assert_eq!(entries(dir.path()), 2);
    }

    #[test]
    fn truncated_source_is_reported_as_changed() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        std::fs::write(&source, "abc").unwrap();
        let file = File::open(&source).unwrap();
        let chunk = Chunk::SourceRange {
            file: &file,
            offset: 0,
            len: 9,
            expected_hash: [0; 32],
        };
        assert!(matches!(
            atomic_write(&dir.path().join("a"), &[chunk]),
            Err(SaveError::SourceChanged)
        ));
    }

    #[test]
    fn source_range_is_streamed_and_verified() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let payload: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&source, &payload).unwrap();
        let file = File::open(&source).unwrap();
        let range = 10..payload.len() - 5;
        let chunk = Chunk::SourceRange {
            file: &file,
            offset: range.start as u64,
            len: range.len() as u64,
            expected_hash: *blake3::hash(&payload[range.clone()]).as_bytes(),
        };
        let path = dir.path().join("out");
        atomic_write(&path, &[bytes(b"head"), chunk, bytes(b"tail")]).unwrap();
        let mut expected = b"head".to_vec();
        expected.extend_from_slice(&payload[range]);
        expected.extend_from_slice(b"tail");
        assert_eq!(std::fs::read(&path).unwrap(), expected);
    }

    #[test]
    fn keeps_extended_attributes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a");
        std::fs::write(&path, "old").unwrap();
        let status = std::process::Command::new("xattr")
            .args(["-w", "com.example.keep", "1"])
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success());
        atomic_write(&path, &[bytes(b"new")]).unwrap();
        let out = std::process::Command::new("xattr")
            .args(["-p", "com.example.keep"])
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "1");
    }
}
