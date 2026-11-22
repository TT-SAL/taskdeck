//! Context strings, one const per row of the registry (seal.md §4.1, §26).

/// `frame` (seal.md §4.1; used in §8.3).
pub const FRAME: &[u8] = b"taskdeck/v1/frame\0";
/// `login` (seal.md §4.1; used in §16.1).
pub const LOGIN: &[u8] = b"taskdeck/v1/login\0";
/// `admin` (seal.md §4.1; used in §16.2).
pub const ADMIN: &[u8] = b"taskdeck/v1/admin\0";
/// `backup` (seal.md §4.1; used in §16.3).
pub const BACKUP: &[u8] = b"taskdeck/v1/backup\0";
/// `ctl` (seal.md §4.1; used in §11.3).
pub const CTL: &[u8] = b"taskdeck/v1/ctl\0";
/// `entry` (seal.md §4.1; used in §9.2).
pub const ENTRY: &[u8] = b"taskdeck/v1/entry\0";
/// `blob` (seal.md §4.1; used in §20.2).
pub const BLOB: &[u8] = b"taskdeck/v1/blob\0";
/// `file` (seal.md §4.1; used in §19.3).
pub const FILE: &[u8] = b"taskdeck/v1/file\0";
/// `pair-bundle` (seal.md §4.1; used in §17.8).
pub const PAIR_BUNDLE: &[u8] = b"taskdeck/v1/pair-bundle\0";
/// `wrap` (seal.md §4.1; used in §7).
pub const WRAP: &[u8] = b"taskdeck/v1/wrap\0";
/// `wrap-aad` (seal.md §4.1; used in §7).
pub const WRAP_AAD: &[u8] = b"taskdeck/v1/wrap-aad\0";
/// `backup-hpke` (seal.md §4.1; used in §21.3).
pub const BACKUP_HPKE: &[u8] = b"taskdeck/v1/backup-hpke\0";
/// `backup-aad` (seal.md §4.1; used in §21.3).
pub const BACKUP_AAD: &[u8] = b"taskdeck/v1/backup-aad\0";
/// `key-entry` (seal.md §4.1; used in §6.5).
pub const KEY_ENTRY: &[u8] = b"taskdeck/v1/key-entry\0";
/// `key-snap` (seal.md §4.1; used in §6.5).
pub const KEY_SNAP: &[u8] = b"taskdeck/v1/key-snap\0";
/// `key-blob` (seal.md §4.1; used in §6.5).
pub const KEY_BLOB: &[u8] = b"taskdeck/v1/key-blob\0";
/// `file-key` (seal.md §4.1; used in §6.6).
pub const FILE_KEY: &[u8] = b"taskdeck/v1/file-key\0";
/// `recovery-hpke` (seal.md §4.1; used in §6.1).
pub const RECOVERY_HPKE: &[u8] = b"taskdeck/v1/recovery-hpke\0";
/// `recovery-sign` (seal.md §4.1; used in §6.1).
pub const RECOVERY_SIGN: &[u8] = b"taskdeck/v1/recovery-sign\0";
/// `pair-ticket` (seal.md §4.1; used in §17.3).
pub const PAIR_TICKET: &[u8] = b"taskdeck/v1/pair-ticket\0";
/// `pair-mac` (seal.md §4.1; used in §17.3).
pub const PAIR_MAC: &[u8] = b"taskdeck/v1/pair-mac\0";
/// `pair-bundle-key` (seal.md §4.1; used in §17.8).
pub const PAIR_BUNDLE_KEY: &[u8] = b"taskdeck/v1/pair-bundle-key\0";
/// `sas` (seal.md §4.1; used in §17.7).
pub const SAS: &[u8] = b"taskdeck/v1/sas\0";
/// `pair` (seal.md §4.1; used in §17.4).
pub const PAIR: &[u8] = b"taskdeck/v1/pair\0";
/// `pair-tid` (seal.md §4.1; used in §17.3).
pub const PAIR_TID: &[u8] = b"taskdeck/v1/pair-tid\0";
/// `pair-commit` (seal.md §4.1; used in §17.5).
pub const PAIR_COMMIT: &[u8] = b"taskdeck/v1/pair-commit\0";
/// `pair-transcript` (seal.md §4.1; used in §17.7).
pub const PAIR_TRANSCRIPT: &[u8] = b"taskdeck/v1/pair-transcript\0";
/// `pair-code` (seal.md §4.1; used in §17.2).
pub const PAIR_CODE: &[u8] = b"taskdeck/v1/pair-code\0";
/// `board` (seal.md §4.1; used in §6.2).
pub const BOARD: &[u8] = b"taskdeck/v1/board\0";
/// `recovery-dev` (seal.md §4.1; used in §6.3).
pub const RECOVERY_DEV: &[u8] = b"taskdeck/v1/recovery-dev\0";
/// `epoch` (seal.md §4.1; used in §6.5).
pub const EPOCH: &[u8] = b"taskdeck/v1/epoch\0";
/// `chain` (seal.md §4.1; used in §15.1).
pub const CHAIN: &[u8] = b"taskdeck/v1/chain\0";
/// `fingerprint` (seal.md §4.1; used in §15.3).
pub const FINGERPRINT: &[u8] = b"taskdeck/v1/fingerprint\0";
/// `key-fp` (seal.md §4.1; used in §6.8).
pub const KEY_FP: &[u8] = b"taskdeck/v1/key-fp\0";
/// `move` (seal.md §4.1; used in §15.4).
pub const MOVE: &[u8] = b"taskdeck/v1/move\0";
/// `sheet-check` (seal.md §4.1; used in §18.1).
pub const SHEET_CHECK: &[u8] = b"taskdeck/v1/sheet-check\0";
/// `state` (seal.md §4.1; used in §25).
pub const STATE: &[u8] = b"taskdeck/v1/state\0";
/// `door` (seal.md §4.1; used in §25).
pub const DOOR: &[u8] = b"taskdeck/v1/door\0";
/// `token` (seal.md §4.1; used in §6.7).
pub const TOKEN: &[u8] = b"taskdeck/v1/token\0";
/// `local-key` (seal.md §4.1; used in §6.6).
pub const LOCAL_KEY: &[u8] = b"taskdeck/v1/local-key\0";
