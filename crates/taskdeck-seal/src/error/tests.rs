use super::SealError;

/// Every variant, in the order of §22.
const ALL: &[(SealError, &str)] = &[
    (SealError::FrameFormat, "seal/frame_format"),
    (SealError::WrongBoard, "seal/wrong_board"),
    (SealError::UnknownAuthor, "seal/unknown_author"),
    (SealError::BadSignature, "seal/bad_signature"),
    (SealError::CounterReplay, "seal/counter_replay"),
    (SealError::CounterGap, "seal/counter_gap"),
    (SealError::RevokedAuthor, "seal/revoked_author"),
    (SealError::BadVersion, "seal/bad_version"),
    (SealError::BadEpoch, "seal/bad_epoch"),
    (SealError::OldEpoch, "seal/old_epoch"),
    (SealError::MissingRoot, "seal/missing_root"),
    (SealError::Aead, "seal/aead"),
    (SealError::BadPlaintext, "seal/bad_plaintext"),
    (SealError::BasedOnFuture, "seal/based_on_future"),
    (SealError::BasedOnFork, "seal/based_on_fork"),
    (SealError::NotAuthorized, "seal/not_authorized"),
    (SealError::StaleEpoch, "seal/stale_epoch"),
    (SealError::BadPayload, "seal/bad_payload"),
    (SealError::BadCtlSig, "seal/bad_ctl_sig"),
    (SealError::CtlShape, "seal/ctl_shape"),
    (SealError::CtlGenesis, "seal/ctl_genesis"),
    (SealError::CtlAuthor, "seal/ctl_author"),
    (SealError::CtlTarget, "seal/ctl_target"),
    (SealError::CtlDuplicateKey, "seal/ctl_duplicate_key"),
    (SealError::CtlDuplicate, "seal/ctl_duplicate"),
    (SealError::CtlDeviceCap, "seal/ctl_device_cap"),
    (SealError::CtlCap, "seal/ctl_cap"),
    (SealError::CtlEpoch, "seal/ctl_epoch"),
    (SealError::CtlWraps, "seal/ctl_wraps"),
    (SealError::CtlLastDevice, "seal/ctl_last_device"),
    (SealError::CtlHomeGen, "seal/ctl_home_gen"),
    (SealError::CtlSupport, "seal/ctl_support"),
    (SealError::Commitment, "seal/commitment"),
    (SealError::UnknownRules, "seal/unknown_rules"),
    (SealError::PayloadTooLarge, "seal/payload_too_large"),
    (SealError::Entropy, "seal/entropy"),
    (SealError::MembershipFormat, "seal/membership_format"),
    (SealError::ChallengeLength, "seal/challenge_length"),
    (SealError::HostFormat, "seal/host_format"),
    (SealError::MoveCode, "seal/move_code"),
    (SealError::CodeFormat, "seal/code_format"),
    (SealError::CodeCheck, "seal/code_check"),
    (SealError::SlotMac, "seal/slot_mac"),
    (SealError::SlotFormat, "seal/slot_format"),
    (SealError::CommitMismatch, "seal/commit_mismatch"),
    (SealError::DhZero, "seal/dh_zero"),
    (SealError::SasMismatch, "seal/sas_mismatch"),
    (SealError::PairKeysConflict, "seal/pair_keys_conflict"),
    (SealError::PairRules, "seal/pair_rules"),
    (SealError::PairTimeout, "seal/pair_timeout"),
    (SealError::PairCancelled, "seal/pair_cancelled"),
    (SealError::BundleOpen, "seal/bundle_open"),
    (SealError::BundleInvalid, "seal/bundle_invalid"),
    (SealError::DeviceAddedMismatch, "seal/device_added_mismatch"),
    (SealError::PairRaced, "seal/pair_raced"),
    (SealError::SheetLength, "seal/sheet_length"),
    (SealError::SheetChar, "seal/sheet_char"),
    (SealError::SheetPadding, "seal/sheet_padding"),
    (SealError::SheetCheck, "seal/sheet_check"),
    (SealError::FileHeader, "seal/file_header"),
    (SealError::FileTornTail, "seal/file_torn_tail"),
    (SealError::FileBadTail, "seal/file_bad_tail"),
    (SealError::FileCorrupt, "seal/file_corrupt"),
    (SealError::BlobSize, "seal/blob_size"),
    (SealError::BlobHash, "seal/blob_hash"),
    (SealError::BlobHeader, "seal/blob_header"),
    (SealError::BlobOpen, "seal/blob_open"),
    (SealError::SnapshotLarge, "seal/snapshot_large"),
    (SealError::Inflate, "seal/inflate"),
    (SealError::BackupSize, "seal/backup_size"),
    (SealError::BackupHeader, "seal/backup_header"),
    (SealError::BackupBoard, "seal/backup_board"),
    (SealError::BackupRecipient, "seal/backup_recipient"),
    (SealError::BackupSignature, "seal/backup_signature"),
    (SealError::BackupOpen, "seal/backup_open"),
    (SealError::BackupPayload, "seal/backup_payload"),
    (SealError::BackupHistory, "seal/backup_history"),
    (SealError::BackupWriter, "seal/backup_writer"),
    (SealError::BackupUnpinned, "seal/backup_unpinned"),
    (SealError::NotControl, "seal/not_control"),
];

// The codes callers match on are spelled exactly as §22 writes them, among
// them the `seal/entropy` of SEAL-GEN-004; `Display` shows the code.
// spec: SEAL-GEN-004
#[test]
fn every_code_is_spelled_as_section_22_writes_it() {
    assert_eq!(ALL.len(), 80);
    let mut seen = std::collections::HashSet::new();
    for (e, code) in ALL {
        assert_eq!(e.code(), *code);
        assert_eq!(e.to_string(), *code);
        assert!(seen.insert(*code), "{code} twice");
        // The variant is the CamelCase of the code's name (§26).
        let camel: String = code["seal/".len()..]
            .split('_')
            .map(|w| {
                let mut c = w.chars();
                c.next().map(|f| f.to_ascii_uppercase().to_string() + c.as_str()).unwrap_or_default()
            })
            .collect();
        assert_eq!(format!("{e:?}"), camel);
    }
    let boxed: Box<dyn std::error::Error> = Box::new(SealError::Entropy);
    assert_eq!(boxed.to_string(), "seal/entropy");
}
