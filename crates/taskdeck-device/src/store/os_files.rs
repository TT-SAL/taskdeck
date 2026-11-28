//! `OsFiles`, the real file layer (device.md §7.1, DEV-FILE-001).
//!
//! - `sync` is `File::sync_all`; on macOS the standard library implements it
//!   with `F_FULLFSYNC`.
//! - `sync_dir`: on Linux, Android and macOS the directory is opened and
//!   `sync_all` is called on it; on Windows it does nothing, because the
//!   write-through rename is durable on its own.
//! - `rename`: on Windows `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING |
//!   MOVEFILE_WRITE_THROUGH` (`platform_windows`, the one `unsafe` call of
//!   the store); elsewhere `std::fs::rename`, which replaces atomically.
//! - `create_dir` creates the directory and any missing parent; an existing
//!   directory is no error.

use std::ffi::OsString;
use std::io::Write as _;
use std::path::Path;

use crate::{FileLayer, OsFiles};

impl FileLayer for OsFiles {
    fn create_dir(&self, path: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(path)
    }

    fn write_new(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        let mut f = std::fs::File::create(path)?;
        f.write_all(bytes)
    }

    fn append(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new().append(true).open(path)?;
        f.write_all(bytes)
    }

    fn sync(&self, path: &Path) -> std::io::Result<()> {
        // Write access: Windows' FlushFileBuffers needs it.
        std::fs::OpenOptions::new().write(true).open(path)?.sync_all()
    }

    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        #[cfg(windows)]
        {
            platform_windows::replace(from, to)
        }
        #[cfg(not(windows))]
        {
            std::fs::rename(from, to)
        }
    }

    fn remove(&self, path: &Path) -> std::io::Result<()> {
        std::fs::remove_file(path)
    }

    fn truncate(&self, path: &Path, len: u64) -> std::io::Result<()> {
        std::fs::OpenOptions::new().write(true).open(path)?.set_len(len)
    }

    fn sync_dir(&self, dir: &Path) -> std::io::Result<()> {
        #[cfg(windows)]
        {
            let _ = dir;
            Ok(())
        }
        #[cfg(not(windows))]
        {
            std::fs::File::open(dir)?.sync_all()
        }
    }

    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    fn list(&self, dir: &Path) -> std::io::Result<Vec<OsString>> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(dir)? {
            out.push(e?.file_name());
        }
        out.sort();
        Ok(out)
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }
}

/// The Windows replace step (device.md §7.1; DEV-GEN-001 allows `unsafe` in
/// the Windows platform code alone).
#[cfg(windows)]
#[allow(unsafe_code)]
mod platform_windows {
    use std::os::windows::ffi::OsStrExt as _;
    use std::path::Path;

    use windows::Win32::Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW};
    use windows::core::PCWSTR;

    fn wide(p: &Path) -> Vec<u16> {
        p.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
    }

    /// `MoveFileExW(from, to, MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)`.
    pub(super) fn replace(from: &Path, to: &Path) -> std::io::Result<()> {
        let f = wide(from);
        let t = wide(to);
        // SAFETY: both pointers name NUL-terminated UTF-16 buffers that live
        // until the call returns; the call reads them and keeps neither.
        let r = unsafe {
            MoveFileExW(PCWSTR(f.as_ptr()), PCWSTR(t.as_ptr()), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)
        };
        r.map_err(|e| std::io::Error::from_raw_os_error(e.code().0 & 0xFFFF))
    }
}
