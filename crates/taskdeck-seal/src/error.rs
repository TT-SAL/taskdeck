//! The error codes (seal.md §22, §26).

/// A failure, one variant per code of §22 (seal.md §26).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SealError {
    /// `seal/frame_format` (seal.md §22).
    FrameFormat,
    /// `seal/wrong_board` (seal.md §22).
    WrongBoard,
    /// `seal/unknown_author` (seal.md §22).
    UnknownAuthor,
    /// `seal/bad_signature` (seal.md §22).
    BadSignature,
    /// `seal/counter_replay` (seal.md §22).
    CounterReplay,
    /// `seal/counter_gap` (seal.md §22).
    CounterGap,
    /// `seal/revoked_author` (seal.md §22).
    RevokedAuthor,
    /// `seal/bad_version` (seal.md §22).
    BadVersion,
    /// `seal/bad_epoch` (seal.md §22).
    BadEpoch,
    /// `seal/old_epoch` (seal.md §22).
    OldEpoch,
    /// `seal/missing_root` (seal.md §22).
    MissingRoot,
    /// `seal/aead` (seal.md §22).
    Aead,
    /// `seal/bad_plaintext` (seal.md §22).
    BadPlaintext,
    /// `seal/based_on_future` (seal.md §22).
    BasedOnFuture,
    /// `seal/based_on_fork` (seal.md §22).
    BasedOnFork,
    /// `seal/not_authorized` (seal.md §22).
    NotAuthorized,
    /// `seal/stale_epoch` (seal.md §22).
    StaleEpoch,
    /// `seal/bad_payload` (seal.md §22).
    BadPayload,
    /// `seal/bad_ctl_sig` (seal.md §22).
    BadCtlSig,
    /// `seal/ctl_shape` (seal.md §22).
    CtlShape,
    /// `seal/ctl_genesis` (seal.md §22).
    CtlGenesis,
    /// `seal/ctl_author` (seal.md §22).
    CtlAuthor,
    /// `seal/ctl_target` (seal.md §22).
    CtlTarget,
    /// `seal/ctl_duplicate_key` (seal.md §22).
    CtlDuplicateKey,
    /// `seal/ctl_duplicate` (seal.md §22).
    CtlDuplicate,
    /// `seal/ctl_device_cap` (seal.md §22).
    CtlDeviceCap,
    /// `seal/ctl_cap` (seal.md §22).
    CtlCap,
    /// `seal/ctl_epoch` (seal.md §22).
    CtlEpoch,
    /// `seal/ctl_wraps` (seal.md §22).
    CtlWraps,
    /// `seal/ctl_last_device` (seal.md §22).
    CtlLastDevice,
    /// `seal/ctl_home_gen` (seal.md §22).
    CtlHomeGen,
    /// `seal/ctl_support` (seal.md §22).
    CtlSupport,
    /// `seal/commitment` (seal.md §22).
    Commitment,
    /// `seal/unknown_rules` (seal.md §22).
    UnknownRules,
    /// `seal/payload_too_large` (seal.md §22).
    PayloadTooLarge,
    /// `seal/entropy` (seal.md §22).
    Entropy,
    /// `seal/membership_format` (seal.md §22).
    MembershipFormat,
    /// `seal/challenge_length` (seal.md §22).
    ChallengeLength,
    /// `seal/host_format` (seal.md §22).
    HostFormat,
    /// `seal/move_code` (seal.md §22).
    MoveCode,
    /// `seal/code_format` (seal.md §22).
    CodeFormat,
    /// `seal/code_check` (seal.md §22).
    CodeCheck,
    /// `seal/slot_mac` (seal.md §22).
    SlotMac,
    /// `seal/slot_format` (seal.md §22).
    SlotFormat,
    /// `seal/commit_mismatch` (seal.md §22).
    CommitMismatch,
    /// `seal/dh_zero` (seal.md §22).
    DhZero,
    /// `seal/sas_mismatch` (seal.md §22).
    SasMismatch,
    /// `seal/pair_keys_conflict` (seal.md §22).
    PairKeysConflict,
    /// `seal/pair_rules` (seal.md §22).
    PairRules,
    /// `seal/pair_timeout` (seal.md §22).
    PairTimeout,
    /// `seal/pair_cancelled` (seal.md §22).
    PairCancelled,
    /// `seal/bundle_open` (seal.md §22).
    BundleOpen,
    /// `seal/bundle_invalid` (seal.md §22).
    BundleInvalid,
    /// `seal/device_added_mismatch` (seal.md §22).
    DeviceAddedMismatch,
    /// `seal/pair_raced` (seal.md §22).
    PairRaced,
    /// `seal/sheet_length` (seal.md §22).
    SheetLength,
    /// `seal/sheet_char` (seal.md §22).
    SheetChar,
    /// `seal/sheet_padding` (seal.md §22).
    SheetPadding,
    /// `seal/sheet_check` (seal.md §22).
    SheetCheck,
    /// `seal/file_header` (seal.md §22).
    FileHeader,
    /// `seal/file_torn_tail` (seal.md §22).
    FileTornTail,
    /// `seal/file_bad_tail` (seal.md §22).
    FileBadTail,
    /// `seal/file_corrupt` (seal.md §22).
    FileCorrupt,
    /// `seal/blob_size` (seal.md §22).
    BlobSize,
    /// `seal/blob_hash` (seal.md §22).
    BlobHash,
    /// `seal/blob_header` (seal.md §22).
    BlobHeader,
    /// `seal/blob_open` (seal.md §22).
    BlobOpen,
    /// `seal/snapshot_large` (seal.md §22).
    SnapshotLarge,
    /// `seal/inflate` (seal.md §22).
    Inflate,
    /// `seal/backup_size` (seal.md §22).
    BackupSize,
    /// `seal/backup_header` (seal.md §22).
    BackupHeader,
    /// `seal/backup_board` (seal.md §22).
    BackupBoard,
    /// `seal/backup_recipient` (seal.md §22).
    BackupRecipient,
    /// `seal/backup_signature` (seal.md §22).
    BackupSignature,
    /// `seal/backup_open` (seal.md §22).
    BackupOpen,
    /// `seal/backup_payload` (seal.md §22).
    BackupPayload,
    /// `seal/backup_history` (seal.md §22).
    BackupHistory,
    /// `seal/backup_writer` (seal.md §22).
    BackupWriter,
    /// `seal/backup_unpinned` (seal.md §22).
    BackupUnpinned,
    /// `seal/not_control` (seal.md §22).
    NotControl,
}

impl SealError {
    /// The `seal/…` code of this variant (seal.md §22, §26).
    pub fn code(&self) -> &'static str {
        match self {
            SealError::FrameFormat => "seal/frame_format",
            SealError::WrongBoard => "seal/wrong_board",
            SealError::UnknownAuthor => "seal/unknown_author",
            SealError::BadSignature => "seal/bad_signature",
            SealError::CounterReplay => "seal/counter_replay",
            SealError::CounterGap => "seal/counter_gap",
            SealError::RevokedAuthor => "seal/revoked_author",
            SealError::BadVersion => "seal/bad_version",
            SealError::BadEpoch => "seal/bad_epoch",
            SealError::OldEpoch => "seal/old_epoch",
            SealError::MissingRoot => "seal/missing_root",
            SealError::Aead => "seal/aead",
            SealError::BadPlaintext => "seal/bad_plaintext",
            SealError::BasedOnFuture => "seal/based_on_future",
            SealError::BasedOnFork => "seal/based_on_fork",
            SealError::NotAuthorized => "seal/not_authorized",
            SealError::StaleEpoch => "seal/stale_epoch",
            SealError::BadPayload => "seal/bad_payload",
            SealError::BadCtlSig => "seal/bad_ctl_sig",
            SealError::CtlShape => "seal/ctl_shape",
            SealError::CtlGenesis => "seal/ctl_genesis",
            SealError::CtlAuthor => "seal/ctl_author",
            SealError::CtlTarget => "seal/ctl_target",
            SealError::CtlDuplicateKey => "seal/ctl_duplicate_key",
            SealError::CtlDuplicate => "seal/ctl_duplicate",
            SealError::CtlDeviceCap => "seal/ctl_device_cap",
            SealError::CtlCap => "seal/ctl_cap",
            SealError::CtlEpoch => "seal/ctl_epoch",
            SealError::CtlWraps => "seal/ctl_wraps",
            SealError::CtlLastDevice => "seal/ctl_last_device",
            SealError::CtlHomeGen => "seal/ctl_home_gen",
            SealError::CtlSupport => "seal/ctl_support",
            SealError::Commitment => "seal/commitment",
            SealError::UnknownRules => "seal/unknown_rules",
            SealError::PayloadTooLarge => "seal/payload_too_large",
            SealError::Entropy => "seal/entropy",
            SealError::MembershipFormat => "seal/membership_format",
            SealError::ChallengeLength => "seal/challenge_length",
            SealError::HostFormat => "seal/host_format",
            SealError::MoveCode => "seal/move_code",
            SealError::CodeFormat => "seal/code_format",
            SealError::CodeCheck => "seal/code_check",
            SealError::SlotMac => "seal/slot_mac",
            SealError::SlotFormat => "seal/slot_format",
            SealError::CommitMismatch => "seal/commit_mismatch",
            SealError::DhZero => "seal/dh_zero",
            SealError::SasMismatch => "seal/sas_mismatch",
            SealError::PairKeysConflict => "seal/pair_keys_conflict",
            SealError::PairRules => "seal/pair_rules",
            SealError::PairTimeout => "seal/pair_timeout",
            SealError::PairCancelled => "seal/pair_cancelled",
            SealError::BundleOpen => "seal/bundle_open",
            SealError::BundleInvalid => "seal/bundle_invalid",
            SealError::DeviceAddedMismatch => "seal/device_added_mismatch",
            SealError::PairRaced => "seal/pair_raced",
            SealError::SheetLength => "seal/sheet_length",
            SealError::SheetChar => "seal/sheet_char",
            SealError::SheetPadding => "seal/sheet_padding",
            SealError::SheetCheck => "seal/sheet_check",
            SealError::FileHeader => "seal/file_header",
            SealError::FileTornTail => "seal/file_torn_tail",
            SealError::FileBadTail => "seal/file_bad_tail",
            SealError::FileCorrupt => "seal/file_corrupt",
            SealError::BlobSize => "seal/blob_size",
            SealError::BlobHash => "seal/blob_hash",
            SealError::BlobHeader => "seal/blob_header",
            SealError::BlobOpen => "seal/blob_open",
            SealError::SnapshotLarge => "seal/snapshot_large",
            SealError::Inflate => "seal/inflate",
            SealError::BackupSize => "seal/backup_size",
            SealError::BackupHeader => "seal/backup_header",
            SealError::BackupBoard => "seal/backup_board",
            SealError::BackupRecipient => "seal/backup_recipient",
            SealError::BackupSignature => "seal/backup_signature",
            SealError::BackupOpen => "seal/backup_open",
            SealError::BackupPayload => "seal/backup_payload",
            SealError::BackupHistory => "seal/backup_history",
            SealError::BackupWriter => "seal/backup_writer",
            SealError::BackupUnpinned => "seal/backup_unpinned",
            SealError::NotControl => "seal/not_control",
        }
    }
}

impl core::fmt::Display for SealError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for SealError {}

#[cfg(test)]
mod tests;

