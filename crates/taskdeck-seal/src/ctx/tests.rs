use super::*;

/// The registry of §4.1, row by row: the const and its `<use>`.
const REGISTRY: &[(&[u8], &str)] = &[
    (FRAME, "frame"),
    (LOGIN, "login"),
    (ADMIN, "admin"),
    (BACKUP, "backup"),
    (CTL, "ctl"),
    (ENTRY, "entry"),
    (BLOB, "blob"),
    (FILE, "file"),
    (PAIR_BUNDLE, "pair-bundle"),
    (WRAP, "wrap"),
    (WRAP_AAD, "wrap-aad"),
    (BACKUP_HPKE, "backup-hpke"),
    (BACKUP_AAD, "backup-aad"),
    (KEY_ENTRY, "key-entry"),
    (KEY_SNAP, "key-snap"),
    (KEY_BLOB, "key-blob"),
    (FILE_KEY, "file-key"),
    (RECOVERY_HPKE, "recovery-hpke"),
    (RECOVERY_SIGN, "recovery-sign"),
    (PAIR_TICKET, "pair-ticket"),
    (PAIR_MAC, "pair-mac"),
    (PAIR_BUNDLE_KEY, "pair-bundle-key"),
    (SAS, "sas"),
    (PAIR, "pair"),
    (PAIR_TID, "pair-tid"),
    (PAIR_COMMIT, "pair-commit"),
    (PAIR_TRANSCRIPT, "pair-transcript"),
    (PAIR_CODE, "pair-code"),
    (BOARD, "board"),
    (RECOVERY_DEV, "recovery-dev"),
    (EPOCH, "epoch"),
    (CHAIN, "chain"),
    (FINGERPRINT, "fingerprint"),
    (KEY_FP, "key-fp"),
    (MOVE, "move"),
    (SHEET_CHECK, "sheet-check"),
    (STATE, "state"),
    (DOOR, "door"),
    (TOKEN, "token"),
    (LOCAL_KEY, "local-key"),
];

// Each const is `taskdeck/v1/<use>` and one 0x00 byte, for each of the 40
// rows of §4.1, and no two are equal; the source declares exactly these.
// spec: SEAL-CTX-001
#[test]
fn one_const_per_registry_row() {
    assert_eq!(REGISTRY.len(), 40);
    let mut seen = std::collections::HashSet::new();
    for (c, name) in REGISTRY {
        let want = [b"taskdeck/v1/".as_slice(), name.as_bytes(), &[0]].concat();
        assert_eq!(*c, want.as_slice(), "{name}");
        assert!(seen.insert(*c), "{name} twice");
    }
    let src = include_str!("../ctx.rs");
    let declared = src.lines().filter(|l| l.starts_with("pub const ")).count();
    assert_eq!(declared, REGISTRY.len());
}
