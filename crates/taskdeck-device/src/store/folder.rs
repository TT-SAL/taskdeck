//! The data folder (device.md §5): its lock (§5.2), the cloud-folder check
//! (§5.3), the plain descriptors of `sync/` (§6.2), the steps of §5.4 that
//! decide the mode and open the active identity, and the creation and
//! activation of an identity directory (§18.1 steps 7 and 10).

use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use zeroize::Zeroizing;

use super::codec::{self, ACTIVE_MAGIC, PENDING_MAGIC, STORE_DESCRIPTOR, StoreDescriptor, err};
use super::identity_dir::{IdentityStore, NewIdentity, Opened, STORE_TDS, WRITE_FAILED, replace_raw};
use crate::{DeviceError, FileLayer, KeyStore, KeyStoreError, OpenWarning, StoredIdentity};

/// `device/folder_locked` (device.md §21).
pub const FOLDER_LOCKED: &str = "device/folder_locked";
/// `device/folder_unlockable` (device.md §21).
pub const FOLDER_UNLOCKABLE: &str = "device/folder_unlockable";
/// `device/cloud_folder` (device.md §21).
pub const CLOUD_FOLDER: &str = "device/cloud_folder";
/// `device/keystore_unavailable` (device.md §21).
pub const KEYSTORE_UNAVAILABLE: &str = "device/keystore_unavailable";
/// `device/keystore_failed` (device.md §21).
pub const KEYSTORE_FAILED: &str = "device/keystore_failed";

/// The `(board_id, dev)` an `active.tds` or `pending.tds` names (device.md §6.2).
pub type IdentityName = ([u8; 16], [u8; 16]);

const ACTIVE: &str = "active.tds";
const PENDING: &str = "pending.tds";

/// The result of taking the lock (device.md §5.2). `Held` keeps the open
/// file, and so the lock, until it is dropped (DEV-LOCK-001).
pub enum Lock {
    Held(File),
    Contended,
    Unsupported,
}

impl Lock {
    /// `Held`.
    pub fn is_held(&self) -> bool {
        matches!(self, Lock::Held(_))
    }

    /// The refusal of synced mode, `begin_create` and `join` for a lock that
    /// is not held (DEV-LOCK-002).
    pub fn refusal(&self) -> Option<DeviceError> {
        match self {
            Lock::Held(_) => None,
            Lock::Contended => Some(err(FOLDER_LOCKED)),
            Lock::Unsupported => Some(err(FOLDER_UNLOCKABLE)),
        }
    }

    /// The open report's warning in local-only mode (DEV-LOCK-003).
    pub fn warning(&self) -> Option<OpenWarning> {
        match self {
            Lock::Held(_) => None,
            Lock::Contended => Some(OpenWarning::FolderLocked),
            Lock::Unsupported => Some(OpenWarning::FolderUnlockable),
        }
    }
}

/// Takes the lock of device.md §5.2: creates the data folder when it is
/// missing, opens `.lock` with create, write and no truncation, and calls
/// `File::try_lock`. The lock file is never written.
///
/// The lock is a held file handle, which the `FileLayer` has no call for:
/// it goes to the standard library directly (finding f2-5).
pub fn take_lock(data_dir: &Path) -> Lock {
    if std::fs::create_dir_all(data_dir).is_err() {
        return Lock::Unsupported;
    }
    let Ok(f) = std::fs::OpenOptions::new().create(true).write(true).truncate(false).open(data_dir.join(".lock"))
    else {
        return Lock::Unsupported;
    };
    match f.try_lock() {
        Ok(()) => Lock::Held(f),
        Err(TryLockError::WouldBlock) => Lock::Contended,
        Err(TryLockError::Error(_)) => Lock::Unsupported,
    }
}

/// The names of §5.3 rule 1, compared ignoring ASCII case.
const CLOUD_NAMES: &[&str] = &[
    "OneDrive",
    "iCloud Drive",
    "iCloudDrive",
    "Mobile Documents",
    "CloudStorage",
    "Dropbox",
    "Google Drive",
    "GoogleDrive",
    "My Drive",
    "Box",
    "Box Sync",
    "pCloud Drive",
    "Nextcloud",
    "ownCloud",
    "MEGA",
    "MEGAsync",
    "Sync",
    "SynologyDrive",
    "Tresorit",
];

/// Rule 1 of device.md §5.3 on a path's components.
fn cloud_component(path: &Path) -> bool {
    path.components().any(|c| {
        let Some(s) = c.as_os_str().to_str() else { return false };
        let lower = s.to_ascii_lowercase();
        CLOUD_NAMES.iter().any(|n| n.eq_ignore_ascii_case(s))
            || lower.starts_with("onedrive - ")
            || lower.starts_with("dropbox (")
    })
}

/// The cloud-folder check of device.md §5.3 (DEV-OPEN-001) on the
/// canonical path of the data folder: rule 1 everywhere, rule 2 on Windows,
/// rule 3 on macOS. A path that cannot be canonicalised is checked as given.
/// Android skips the check; the caller does not call it there.
pub fn is_cloud_folder(data_dir: &Path) -> bool {
    let p = std::fs::canonicalize(data_dir).unwrap_or_else(|_| data_dir.to_path_buf());
    if cloud_component(&p) {
        return true;
    }
    #[cfg(windows)]
    for var in ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"] {
        if let Some(v) = std::env::var_os(var).filter(|v| !v.is_empty()) {
            let root = std::fs::canonicalize(&v).unwrap_or_else(|_| PathBuf::from(&v));
            if p.starts_with(&root) {
                return true;
            }
        }
    }
    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        let lib = home.join("Library");
        if p.starts_with(lib.join("Mobile Documents")) || p.starts_with(lib.join("CloudStorage")) {
            return true;
        }
        let icloud_desktop = lib.join("Mobile Documents/com~apple~CloudDocs/Desktop");
        if icloud_desktop.exists() && (p.starts_with(home.join("Desktop")) || p.starts_with(home.join("Documents"))) {
            return true;
        }
    }
    false
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// `taskdeck_data/sync/`.
pub fn sync_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("sync")
}

/// `sync/id-<board hex>-<dev hex>/` (device.md §6.1).
pub fn identity_dir(data_dir: &Path, board: &[u8; 16], dev: &[u8; 16]) -> PathBuf {
    sync_dir(data_dir).join(format!("id-{}-{}", hex(board), hex(dev)))
}

/// Reads `sync/active.tds` (device.md §6.2): `None` when absent.
pub fn read_active(files: &dyn FileLayer, data_dir: &Path) -> Result<Option<IdentityName>, DeviceError> {
    read_descriptor(files, data_dir, ACTIVE, ACTIVE_MAGIC)
}

/// Reads `sync/pending.tds` (device.md §6.2, §18.1): `None` when absent.
pub fn read_pending(files: &dyn FileLayer, data_dir: &Path) -> Result<Option<IdentityName>, DeviceError> {
    read_descriptor(files, data_dir, PENDING, PENDING_MAGIC)
}

fn read_descriptor(
    files: &dyn FileLayer,
    data_dir: &Path,
    name: &str,
    magic: &[u8; 4],
) -> Result<Option<IdentityName>, DeviceError> {
    let p = sync_dir(data_dir).join(name);
    if !files.exists(&p) {
        return Ok(None);
    }
    let b = files.read(&p).map_err(|_| err(STORE_DESCRIPTOR))?;
    codec::decode_active(magic, &b).map(Some)
}

/// Reads an identity directory's `store.tds` (device.md §6.2).
pub fn read_store_descriptor(files: &dyn FileLayer, dir: &Path) -> Result<StoreDescriptor, DeviceError> {
    let b = files.read(&dir.join(STORE_TDS)).map_err(|_| err(STORE_DESCRIPTOR))?;
    StoreDescriptor::decode(&b)
}

/// Writes `sync/pending.tds` naming an identity (device.md §18.1 step 7),
/// by the replace procedure.
pub fn write_pending(files: &dyn FileLayer, data_dir: &Path, board: &[u8; 16], dev: &[u8; 16]) -> Result<(), DeviceError> {
    replace_raw(files, &sync_dir(data_dir), PENDING, &codec::encode_active(PENDING_MAGIC, board, dev))
}

/// Activates an identity (device.md §18.1 step 10, DEV-CREATE-003):
/// replaces `sync/active.tds`, then removes `pending.tds` when it exists.
pub fn activate(files: &dyn FileLayer, data_dir: &Path, board: &[u8; 16], dev: &[u8; 16]) -> Result<(), DeviceError> {
    let sync = sync_dir(data_dir);
    replace_raw(files, &sync, ACTIVE, &codec::encode_active(ACTIVE_MAGIC, board, dev))?;
    let p = sync.join(PENDING);
    if files.exists(&p) {
        files.remove(&p).map_err(|_| err(WRITE_FAILED))?;
        files.sync_dir(&sync).map_err(|_| err(WRITE_FAILED))?;
    }
    Ok(())
}

/// Every identity directory under `sync/` whose `store.tds` is valid,
/// except the one `pending.tds` names, with `active` for the one
/// `active.tds` names (device.md §17.2 `identities`).
pub fn identities(files: &dyn FileLayer, data_dir: &Path) -> Vec<StoredIdentity> {
    let sync = sync_dir(data_dir);
    let active = read_active(files, data_dir).ok().flatten();
    let pending = read_pending(files, data_dir).ok().flatten();
    let mut out = Vec::new();
    for n in files.list(&sync).unwrap_or_default() {
        let Some(n) = n.to_str() else { continue };
        if !n.starts_with("id-") {
            continue;
        }
        let Ok(d) = read_store_descriptor(files, &sync.join(n)) else { continue };
        if identity_dir(data_dir, &d.board_id, &d.dev) != sync.join(n) || pending == Some((d.board_id, d.dev)) {
            continue;
        }
        out.push(StoredIdentity {
            board_id: d.board_id,
            dev: d.dev,
            key_store: d.key_store,
            created_ms: d.created_ms,
            active: active == Some((d.board_id, d.dev)),
        });
    }
    out
}

/// Creates an identity directory for a new board or a join (device.md
/// §18.1 step 7, §18.3): `KeyStore::create(L_d)` first (DEV-KEY-002), then
/// `sync/` when it is missing (`create_dir`, `sync_dir` of the data folder),
/// then the directory and its files (`IdentityStore::create`), then
/// `pending.tds` naming it (DEV-CREATE-001). Nothing is active yet.
pub fn create_identity(
    files: Arc<dyn FileLayer>,
    data_dir: &Path,
    key_store: &dyn KeyStore,
    l_d: &Zeroizing<[u8; 32]>,
    new: NewIdentity<'_>,
) -> Result<IdentityStore, DeviceError> {
    let (board, dev) = (new.descriptor.board_id, new.descriptor.dev);
    if new.descriptor.key_store != key_store.kind() {
        return Err(err(STORE_DESCRIPTOR));
    }
    key_store.create(&board, &dev, l_d).map_err(|e| err(keystore_code(e)))?;
    let sync = sync_dir(data_dir);
    if !files.exists(&sync) {
        files.create_dir(&sync).map_err(|_| err(WRITE_FAILED))?;
        files.sync_dir(data_dir).map_err(|_| err(WRITE_FAILED))?;
    }
    let store = IdentityStore::create(files.clone(), &identity_dir(data_dir, &board, &dev), l_d, new)?;
    write_pending(&*files, data_dir, &board, &dev)?;
    Ok(store)
}

fn keystore_code(e: KeyStoreError) -> &'static str {
    match e {
        KeyStoreError::Unavailable => KEYSTORE_UNAVAILABLE,
        _ => KEYSTORE_FAILED,
    }
}

/// What steps 2 to 5 of device.md §5.4 found.
pub enum FolderState {
    /// No `active.tds`: local-only mode; `pending_create` when `pending.tds`
    /// exists (the open report's `PendingCreate`).
    LocalOnly { pending_create: bool },
    /// `L_d` cannot be loaded (§12.6): the identity's descriptor and why.
    KeyLost { descriptor: StoreDescriptor, reason: KeyStoreError },
    /// The active identity, read (§7.4).
    Synced(Box<Opened>),
}

/// Steps 2 to 5 of device.md §5.4 (DEV-OPEN-002), after the caller took the
/// lock (step 1):
///
/// 2. `sync/active.tds` absent: `LocalOnly`, nothing written.
/// 3. Present: the lock must be held and the cloud check pass (`folder_locked`,
///    `folder_unlockable`, `cloud_folder`, DEV-LOCK-002, DEV-OPEN-001);
///    `store.tds` of the named directory must name the same board and `dev`
///    (`store_descriptor`).
/// 4. `L_d` from the key store, whose kind must be `store.tds`'s
///    (`store_descriptor`): `Unavailable` fails with `keystore_unavailable`,
///    `Failed` with `keystore_failed`, `NotFound` and `CannotUnwrap` are
///    `KeyLost`.
/// 5. The identity directory read (`IdentityStore::open`): `Synced`.
pub fn open_folder(
    files: Arc<dyn FileLayer>,
    data_dir: &Path,
    lock: &Lock,
    cloud: bool,
    key_store: &dyn KeyStore,
    build_id: &str,
    now_ms: i64,
) -> Result<FolderState, DeviceError> {
    let Some((board, dev)) = read_active(&*files, data_dir)? else {
        let pending_create = files.exists(&sync_dir(data_dir).join(PENDING));
        return Ok(FolderState::LocalOnly { pending_create });
    };
    if let Some(e) = lock.refusal() {
        return Err(e);
    }
    if cloud {
        return Err(err(CLOUD_FOLDER));
    }
    let dir = identity_dir(data_dir, &board, &dev);
    let d = read_store_descriptor(&*files, &dir)?;
    if d.board_id != board || d.dev != dev || d.key_store != key_store.kind() {
        return Err(err(STORE_DESCRIPTOR));
    }
    let l_d = match key_store.load(&board, &dev) {
        Ok(k) => k,
        Err(reason @ (KeyStoreError::NotFound | KeyStoreError::CannotUnwrap)) => {
            return Ok(FolderState::KeyLost { descriptor: d, reason });
        }
        Err(e) => return Err(err(keystore_code(e))),
    };
    let opened = IdentityStore::open(files, &dir, board, dev, &l_d, build_id, now_ms)?;
    Ok(FolderState::Synced(Box::new(opened)))
}
