//! Platform code (device.md §7.1, §12.3). DEV-GEN-001: the crate denies
//! `unsafe` at its root and allows it only in `platform::windows`, where every
//! `unsafe` block carries a `// SAFETY:` comment and calls only
//! `CryptProtectData`, `CryptUnprotectData`, `LocalFree` and `MoveFileExW` of
//! the `windows` crate.

/// The Windows calls: the replace step of the file layer (§7.1).
#[cfg(windows)]
#[allow(unsafe_code)]
pub(crate) mod windows {
    use std::os::windows::ffi::OsStrExt as _;
    use std::path::Path;

    use ::windows::Win32::Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW};
    use ::windows::core::PCWSTR;

    fn wide(p: &Path) -> Vec<u16> {
        p.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
    }

    /// `MoveFileExW(from, to, MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)`.
    pub(crate) fn replace(from: &Path, to: &Path) -> std::io::Result<()> {
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
