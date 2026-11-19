//! Keeps a private copy of a backup's protected sections.

use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloneKind {
    ApfsClone,
    FullCopy,
}

fn c_path(path: &Path) -> io::Result<CString> {
    CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
}

/// Clones `src` to `dst`, instantly on APFS and by copying elsewhere.
pub fn clone_protected(src: &Path, dst: &Path) -> io::Result<CloneKind> {
    match std::fs::remove_file(dst) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    // SAFETY: both paths are valid NUL-terminated strings for the duration of the call.
    let cloned = unsafe { libc::clonefile(c_path(src)?.as_ptr(), c_path(dst)?.as_ptr(), 0) } == 0;
    let kind = if cloned {
        CloneKind::ApfsClone
    } else {
        std::fs::copy(src, dst)?;
        CloneKind::FullCopy
    };
    std::fs::set_permissions(dst, std::fs::Permissions::from_mode(0o600))?;
    Ok(kind)
}
