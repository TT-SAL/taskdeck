//! The test key stores of `test-trust` (device.md §12.6, §11.1 item 4,
//! DEV-KEY-007): `FileKeyStore` keeps `L_d` in plain 32-byte files
//! `<dir>/<board hex>-<dev hex>.testkey`, `MemoryKeyStore` in a process map.
//! Neither takes associated data: `AD_L` (§12.1) is only for stores that
//! accept it (DPAPI, the Android Keystore), so these keep the bare 32 bytes
//! as §12.6 states.
//!
//! The file store writes its key file by the replace procedure (§7.2) on the
//! real file system, outside the identity's `FileLayer`, as every key store
//! does (§12.5): `<name>.tmp`, `sync`, `rename`, `sync` of the directory, so
//! that `create` is durable before the identity directory's first sealed file
//! (DEV-KEY-002).

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Mutex;

use zeroize::Zeroizing;

use crate::{KeyStore, KeyStoreError};

/// The `store.tds` kind of `FileKeyStore` (device.md §6.2, §12.2).
pub const KIND_FILE_KEY_STORE: u8 = 4;
/// The `store.tds` kind of `MemoryKeyStore` (device.md §6.2, §12.2).
pub const KIND_MEMORY_KEY_STORE: u8 = 5;

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The test key store in plain files (device.md §12.6, §11.1 item 4).
pub struct FileKeyStore {
    pub dir: PathBuf,
}

/// The test key store in a process map (device.md §12.6, §11.1 item 4).
pub struct MemoryKeyStore {
    keys: Mutex<KeyMap>,
}

type KeyMap = BTreeMap<([u8; 16], [u8; 16]), Zeroizing<[u8; 32]>>;

impl FileKeyStore {
    /// A file store over `dir`; the directory is created at the first `create`.
    pub fn new(dir: impl Into<PathBuf>) -> FileKeyStore {
        FileKeyStore { dir: dir.into() }
    }

    fn path(&self, board: &[u8; 16], dev: &[u8; 16]) -> PathBuf {
        self.dir.join(format!("{}-{}.testkey", hex(board), hex(dev)))
    }
}

impl KeyStore for FileKeyStore {
    fn kind(&self) -> u8 {
        KIND_FILE_KEY_STORE
    }

    fn create(&self, board: &[u8; 16], dev: &[u8; 16], key: &Zeroizing<[u8; 32]>) -> Result<(), KeyStoreError> {
        let path = self.path(board, dev);
        let tmp = path.with_extension("testkey.tmp");
        let write = || -> std::io::Result<()> {
            std::fs::create_dir_all(&self.dir)?;
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(&key[..])?;
            f.sync_all()?;
            drop(f);
            std::fs::rename(&tmp, &path)?;
            #[cfg(unix)]
            std::fs::File::open(&self.dir)?.sync_all()?;
            Ok(())
        };
        write().map_err(|_| KeyStoreError::Failed)
    }

    fn load(&self, board: &[u8; 16], dev: &[u8; 16]) -> Result<Zeroizing<[u8; 32]>, KeyStoreError> {
        let bytes = match std::fs::read(self.path(board, dev)) {
            Ok(b) => Zeroizing::new(b),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(KeyStoreError::NotFound),
            Err(_) => return Err(KeyStoreError::Failed),
        };
        let mut key = Zeroizing::new([0u8; 32]);
        if bytes.len() != 32 {
            return Err(KeyStoreError::CannotUnwrap);
        }
        key.copy_from_slice(&bytes);
        Ok(key)
    }

    fn delete(&self, board: &[u8; 16], dev: &[u8; 16]) -> Result<(), KeyStoreError> {
        match std::fs::remove_file(self.path(board, dev)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(KeyStoreError::Failed),
        }
    }
}

impl MemoryKeyStore {
    /// An empty memory store.
    pub fn new() -> MemoryKeyStore {
        MemoryKeyStore { keys: Mutex::new(BTreeMap::new()) }
    }
}

impl Default for MemoryKeyStore {
    fn default() -> Self {
        MemoryKeyStore::new()
    }
}

impl KeyStore for MemoryKeyStore {
    fn kind(&self) -> u8 {
        KIND_MEMORY_KEY_STORE
    }

    fn create(&self, board: &[u8; 16], dev: &[u8; 16], key: &Zeroizing<[u8; 32]>) -> Result<(), KeyStoreError> {
        let mut m = self.keys.lock().map_err(|_| KeyStoreError::Failed)?;
        m.insert((*board, *dev), key.clone());
        Ok(())
    }

    fn load(&self, board: &[u8; 16], dev: &[u8; 16]) -> Result<Zeroizing<[u8; 32]>, KeyStoreError> {
        let m = self.keys.lock().map_err(|_| KeyStoreError::Failed)?;
        m.get(&(*board, *dev)).cloned().ok_or(KeyStoreError::NotFound)
    }

    fn delete(&self, board: &[u8; 16], dev: &[u8; 16]) -> Result<(), KeyStoreError> {
        let mut m = self.keys.lock().map_err(|_| KeyStoreError::Failed)?;
        m.remove(&(*board, *dev));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DEV-KEY-007 and §12.6: a key round-trips; a missing one is
    /// `NotFound`, a file of another length `CannotUnwrap`.
    #[test]
    fn file_key_store_round_trips() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let ks = FileKeyStore::new(dir.path().join("keys"));
        let (b, d) = ([1u8; 16], [2u8; 16]);
        assert_eq!(ks.kind(), 4);
        assert_eq!(ks.load(&b, &d).err(), Some(KeyStoreError::NotFound));
        ks.create(&b, &d, &Zeroizing::new([7u8; 32])).expect("created");
        assert_eq!(*ks.load(&b, &d).expect("loads"), [7u8; 32]);
        let name = format!("{}-{}.testkey", hex(&b), hex(&d));
        assert!(dir.path().join("keys").join(&name).exists());
        std::fs::write(dir.path().join("keys").join(&name), [0u8; 31]).expect("written");
        assert_eq!(ks.load(&b, &d).err(), Some(KeyStoreError::CannotUnwrap));
        ks.delete(&b, &d).expect("deleted");
        ks.delete(&b, &d).expect("deleting twice is no error");
        assert_eq!(ks.load(&b, &d).err(), Some(KeyStoreError::NotFound));
    }

    #[test]
    fn memory_key_store_round_trips() {
        let ks = MemoryKeyStore::new();
        let (b, d) = ([1u8; 16], [2u8; 16]);
        assert_eq!(ks.kind(), 5);
        assert_eq!(ks.load(&b, &d).err(), Some(KeyStoreError::NotFound));
        ks.create(&b, &d, &Zeroizing::new([9u8; 32])).expect("created");
        assert_eq!(*ks.load(&b, &d).expect("loads"), [9u8; 32]);
        ks.delete(&b, &d).expect("deleted");
        assert_eq!(ks.load(&b, &d).err(), Some(KeyStoreError::NotFound));
    }
}
