//! `OsFiles`, the real file layer (device.md §7.1, DEV-FILE-001).
//!
//! - `sync` is `File::sync_all`; on macOS the standard library implements it
//!   with `F_FULLFSYNC`.
//! - `sync_dir`: on Linux, Android and macOS the directory is opened and
//!   `sync_all` is called on it; on Windows it does nothing, because the
//!   write-through rename is durable on its own.
//! - `rename`: on Windows `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING |
//!   MOVEFILE_WRITE_THROUGH` (`crate::platform::windows`, the one module
//!   where DEV-GEN-001 allows `unsafe`); elsewhere `std::fs::rename`, which replaces atomically.
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
            crate::platform::windows::replace(from, to)
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
