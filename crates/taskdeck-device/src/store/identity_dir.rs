//! One identity directory (device.md §6.1): its sealed files written by the
//! procedures of §7.2, `Persist` executed as §7.3 and §8.3 state, local reads
//! (§8.2), and the reading at open of §7.4.
//!
//! Every file system call goes through the `FileLayer` (DEV-FILE-001). Every
//! sealed record is sealed under `K_file(kind)` with the identity's own
//! `dev` and `board_id` in the header (DEV-STORE-002, SEAL-KEY-014), every
//! replace and create draws a new `file_id`, and every appended record takes
//! the index of the records already in the file (DEV-FILE-003): the store
//! keeps each append file's header and record count.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use taskdeck_core::ids::IdMinter;
use taskdeck_seal::{BoardId, DevId, Entropy as _, FileKind, FileSealer, FileTail, HistoryRecord, OsEntropy, StorageKey};
use taskdeck_sync::{
    Bootstrap, Hash32, HashRecord, LocalReadResult, LogRecord, Loaded, Marker, Meta, NoticeOp,
    OutboxOp, OutboxRecord, PersistOp,
};
use zeroize::Zeroizing;

use super::codec::{self, err};
use super::rjson;
use super::settings::{DeviceSettings, max_minter};
use crate::adapt::{CoreMachine, SealSealer};
use crate::{DeviceError, FileLayer, OpenWarning};

/// `device/store_corrupt` (device.md §21).
pub(crate) const STORE_CORRUPT: &str = "device/store_corrupt";
/// The code of a failed store write (finding f2-6).
pub(crate) const WRITE_FAILED: &str = "device/write_failed";

/// A segment is closed at this many entries or bytes (DEV-STORE-003).
pub const SEGMENT_ENTRIES: u64 = 16_384;
pub const SEGMENT_BYTES: u64 = 67_108_864;
/// A quarantined file older than this at an `open` is removed (device.md §7.5).
pub const QUARANTINE_MS: i64 = 2_592_000_000;

pub(crate) const STORE_TDS: &str = "store.tds";
const IDENTITY: &str = "identity.tdk";
const DEVICE: &str = "device.tdk";
const BOOTSTRAP: &str = "bootstrap.tdk";
const GENESIS: &str = "genesis.tdb";
const CHECKPOINT: &str = "checkpoint.tdk";
const HASHES: &str = "hashes.tdk";
const OUTBOX: &str = "outbox.tdo";
const NOTICES: &str = "notices.tdk";
const META: &str = "meta.tdk";
const MARKER: &str = "marker.tdk";
const HISTORY: &str = "history.tdk";

/// `base-<seq>.tdk`, the seq zero-padded to 20 digits (device.md §6.1).
pub fn base_name(seq: u64) -> String {
    format!("base-{seq:020}.tdk")
}

/// `log-<first seq>.tdl` (device.md §6.1).
pub fn segment_name(first: u64) -> String {
    format!("log-{first:020}.tdl")
}

/// The seq in a name `<prefix><20 digits><suffix>`.
fn seq_of(name: &str, prefix: &str, suffix: &str) -> Option<u64> {
    let d = name.strip_prefix(prefix)?.strip_suffix(suffix)?;
    if d.len() != 20 || !d.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    d.parse().ok()
}

/// The `FileKind` of a kind number (seal.md §19.2); `FileKind` is not `Copy`.
fn kind(k: u8) -> FileKind {
    match k {
        1 => FileKind::Log,
        2 => FileKind::Checkpoint,
        3 => FileKind::Base,
        4 => FileKind::Hashes,
        5 => FileKind::Outbox,
        6 => FileKind::Overlay,
        7 => FileKind::Identity,
        8 => FileKind::Notices,
        9 => FileKind::Meta,
        10 => FileKind::Marker,
        11 => FileKind::History,
        12 => FileKind::Device,
        13 => FileKind::Picture,
        14 => FileKind::Weather,
        15 => FileKind::Draft,
        16 => FileKind::Feed,
        17 => FileKind::Bootstrap,
        18 => FileKind::Unsent,
        _ => FileKind::OwnPicture,
    }
}

fn io(_: std::io::Error) -> DeviceError {
    err(WRITE_FAILED)
}

fn corrupt(kind: u8) -> DeviceError {
    DeviceError { code: STORE_CORRUPT, status: None, message: Some(format!("kind {kind}")) }
}

fn seal_err(_: taskdeck_seal::SealError) -> DeviceError {
    err(WRITE_FAILED)
}

/// An append file's opened records, its header and its valid length.
type Records = (Vec<Zeroizing<Vec<u8>>>, Vec<u8>, u64);

/// An append file's header and record count (DEV-FILE-003).
struct AppendFile {
    header: Vec<u8>,
    records: u64,
}

/// A log segment (device.md §6.1, DEV-STORE-003).
struct Segment {
    first: u64,
    /// The last seq it holds; `None` while it holds none.
    last: Option<u64>,
    entries: u64,
    bytes: u64,
    header: Vec<u8>,
    records: u64,
}

impl Segment {
    fn is_open(&self) -> bool {
        self.entries < SEGMENT_ENTRIES && self.bytes < SEGMENT_BYTES
    }
}

/// What a new identity directory starts with (device.md §18.1 step 7,
/// §18.3).
pub struct NewIdentity<'a> {
    pub descriptor: codec::StoreDescriptor,
    /// `Sealer::export_identity`'s bytes (device.md §6.4.7).
    pub identity: &'a [u8],
    pub settings: DeviceSettings,
    pub bootstrap: &'a Bootstrap,
    /// The sealed genesis snapshot of an imported board (device.md §6.1, §18.1 step 4).
    pub genesis_blob: Option<&'a [u8]>,
}

/// An open identity directory (device.md §6.1, §7).
pub struct IdentityStore {
    files: Arc<dyn FileLayer>,
    dir: PathBuf,
    board: BoardId,
    dev: DevId,
    key: StorageKey,
    /// Set by the first failed write; every later write is refused until the
    /// next `open` (DEV-FILE-005).
    failed: bool,
    appends: BTreeMap<&'static str, AppendFile>,
    segments: Vec<Segment>,
    /// The last seq of the log, 0 when the store holds none.
    head: u64,
    history_last: u64,
    settings: DeviceSettings,
    /// The element-wise maximum of every `Put`'s origin state in
    /// `outbox.tdo` (device.md §8.3).
    outbox_minter: Option<IdMinter>,
}

/// What `IdentityStore::open` read (device.md §7.4).
pub struct Opened {
    pub store: IdentityStore,
    pub loaded: Loaded,
    /// The records of `history.tdk`, ascending by seq (kind 11).
    pub history: Vec<Vec<u8>>,
    /// The plaintext of `identity.tdk` (kind 7, SEAL-FILE-004).
    pub identity: Zeroizing<Vec<u8>>,
    pub warnings: Vec<OpenWarning>,
}

impl Opened {
    /// The sealer adapter from the identity and the history (device.md
    /// §6.4.7, §6.4.8); identity bytes it refuses are `device/store_corrupt`
    /// naming kind 7 (§7.4).
    pub fn sealer(&self) -> Result<SealSealer, DeviceError> {
        SealSealer::open(self.store.board.0, &self.identity, &self.history).map_err(|_| corrupt(7))
    }
}

impl IdentityStore {
    /// The identity directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The per-device settings as last written (device.md §16).
    pub fn settings(&self) -> &DeviceSettings {
        &self.settings
    }

    /// The minter floor at open (device.md §8.3): the element-wise maximum
    /// of `device.tdk`'s `minter` and every `Put`'s origin state in
    /// `outbox.tdo`.
    pub fn minter(&self) -> Option<IdMinter> {
        max_minter(self.settings.minter, self.outbox_minter)
    }

    /// The last seq `history.tdk` holds, 0 when none.
    pub fn history_last(&self) -> u64 {
        self.history_last
    }

    /// Whether a write failed since `open` (DEV-FILE-005).
    pub fn failed(&self) -> bool {
        self.failed
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    // ------------------------------------------------------------ procedures (§7.2)

    /// **replace** of plain bytes: `write_new(tmp)`, `sync(tmp)`,
    /// `rename(tmp, name)`, `sync_dir`.
    fn replace_raw(&self, name: &str, bytes: &[u8]) -> Result<(), DeviceError> {
        replace_raw(&*self.files, &self.dir, name, bytes)
    }

    /// A new file's header and its sealed records, under a fresh `file_id`.
    fn seal_new(&self, k: u8, plaintexts: &[&[u8]]) -> Result<(Vec<u8>, Vec<u8>), DeviceError> {
        let (mut s, header) = FileSealer::create(&self.key, kind(k), &self.board, &self.dev, &mut OsEntropy).map_err(seal_err)?;
        let mut body = header.clone();
        for p in plaintexts {
            body.extend_from_slice(&s.seal_record(p, &mut OsEntropy).map_err(seal_err)?);
        }
        Ok((header, body))
    }

    /// **replace** (and **create**) of a sealed file.
    fn replace_sealed(&self, name: &str, k: u8, plaintexts: &[&[u8]]) -> Result<Vec<u8>, DeviceError> {
        let (header, body) = self.seal_new(k, plaintexts)?;
        self.replace_raw(name, &body)?;
        Ok(header)
    }

    /// A rewrite of an append file by the replace procedure (`RewriteOutbox`,
    /// `RewriteNotices`): its header and count become the new file's.
    fn rewrite_append(&mut self, name: &'static str, k: u8, plaintexts: &[Vec<u8>]) -> Result<(), DeviceError> {
        let refs: Vec<&[u8]> = plaintexts.iter().map(Vec::as_slice).collect();
        let header = self.replace_sealed(name, k, &refs)?;
        self.appends.insert(name, AppendFile { header, records: plaintexts.len() as u64 });
        Ok(())
    }

    /// **append**: when the file does not exist, its header by `write_new` of
    /// the temporary name, `sync`, `rename`, `sync_dir`; then the sealed
    /// records by one `append` and a `sync`.
    fn append_sealed(&mut self, name: &'static str, k: u8, plaintexts: &[Vec<u8>]) -> Result<(), DeviceError> {
        if plaintexts.is_empty() {
            return Ok(());
        }
        if !self.appends.contains_key(name) {
            let (header, _) = self.seal_new(k, &[])?;
            self.replace_raw(name, &header)?;
            self.appends.insert(name, AppendFile { header, records: 0 });
        }
        let f = self.appends.get(name).ok_or_else(|| err(WRITE_FAILED))?;
        let mut s = FileSealer::resume(&self.key, &f.header, f.records).map_err(seal_err)?;
        let mut body = Vec::new();
        for p in plaintexts {
            body.extend_from_slice(&s.seal_record(p, &mut OsEntropy).map_err(seal_err)?);
        }
        let path = self.path(name);
        self.files.append(&path, &body).map_err(io)?;
        self.files.sync(&path).map_err(io)?;
        if let Some(f) = self.appends.get_mut(name) {
            f.records += plaintexts.len() as u64;
        }
        Ok(())
    }

    /// **remove**: `remove`, `sync_dir`; nothing when the file is absent.
    fn remove(&self, name: &str) -> Result<(), DeviceError> {
        let p = self.path(name);
        if self.files.exists(&p) {
            self.files.remove(&p).map_err(io)?;
            self.files.sync_dir(&self.dir).map_err(io)?;
        }
        Ok(())
    }

    /// Replaces `device.tdk` with the settings (device.md §16, replace
    /// procedure); also the store's own writes outside `Persist` (§8.3).
    pub fn write_settings(&mut self, settings: DeviceSettings) -> Result<(), DeviceError> {
        if self.failed {
            return Err(err(WRITE_FAILED));
        }
        self.settings = settings;
        let r = self.put_settings();
        if r.is_err() {
            self.failed = true;
        }
        r
    }

    fn put_settings(&self) -> Result<(), DeviceError> {
        let b = self.settings.to_bytes()?;
        self.replace_sealed(DEVICE, 12, &[&b])?;
        Ok(())
    }

    // ------------------------------------------------------------ Persist (§7.3)

    /// Executes a `Persist`'s ops in order (device.md §7.3), each with every
    /// `sync` and `sync_dir` of its procedure, and returns once all are
    /// durable (DEV-FILE-004). `history` is the sealer adapter's
    /// `SealSealer::history()` (ascending seq): after an `AppendLog` the
    /// records of its seqs above `history.tdk`'s last are appended
    /// (DEV-RUN-003), and before a `WriteCheckpoint` those at or below its
    /// `board_seq`, which holds the history of a snapshot opened at a rejoin
    /// (DEV-RUN-006). The first failure stops it and every later write until
    /// the next `open` (DEV-FILE-005): the runtime then delivers
    /// `PersistFailed`.
    pub fn persist(&mut self, ops: &[PersistOp], history: &[(u64, Vec<u8>)]) -> Result<(), DeviceError> {
        if self.failed {
            return Err(err(WRITE_FAILED));
        }
        for op in ops {
            if let Err(e) = self.op(op, history) {
                self.failed = true;
                return Err(e);
            }
        }
        Ok(())
    }

    fn op(&mut self, op: &PersistOp, history: &[(u64, Vec<u8>)]) -> Result<(), DeviceError> {
        match op {
            PersistOp::AppendLog(records) => {
                self.append_log(records)?;
                if let Some(last) = records.last() {
                    self.append_history(history, last.seq)?;
                }
            }
            PersistOp::SetMarker(m) => {
                self.replace_sealed(MARKER, 10, &[&codec::encode_marker(m)])?;
            }
            PersistOp::ClearMarker => self.remove(MARKER)?,
            PersistOp::WriteCheckpoint(c) => {
                self.append_history(history, c.board_seq)?;
                let b = codec::encode_checkpoint(c)?;
                self.replace_sealed(CHECKPOINT, 2, &[&b])?;
                self.remove(BOOTSTRAP)?;
                self.remove(GENESIS)?;
            }
            PersistOp::WriteBase(c) => {
                let b = codec::encode_checkpoint(c)?;
                self.replace_sealed(&base_name(c.board_seq), 3, &[&b])?;
            }
            PersistOp::DeleteBases(seqs) => {
                for s in seqs {
                    self.remove(&base_name(*s))?;
                }
            }
            PersistOp::TruncateLogBelow(s, c) => self.truncate_log_below(*s, *c)?,
            PersistOp::AppendHashes(v) => {
                let recs: Vec<Vec<u8>> = v.chunks(codec::OPS_MAX).map(codec::encode_hashes).collect();
                self.append_sealed(HASHES, 4, &recs)?;
            }
            PersistOp::Outbox(ops) => {
                let enc = ops.iter().map(codec::encode_outbox_op).collect::<Result<Vec<_>, _>>()?;
                self.append_sealed(OUTBOX, 5, &codec::pack_ops(&enc))?;
                for o in ops {
                    if let OutboxOp::Put(r) = o {
                        self.outbox_minter = max_minter(self.outbox_minter, CoreMachine::minter(&r.origin_state));
                    }
                }
            }
            PersistOp::RewriteOutbox(records) => self.rewrite_outbox(records)?,
            PersistOp::Notices(ops) => {
                let enc = ops.iter().map(codec::encode_notice_op).collect::<Result<Vec<_>, _>>()?;
                self.append_sealed(NOTICES, 8, &codec::pack_ops(&enc))?;
            }
            PersistOp::RewriteNotices(ops) => {
                let enc = ops.iter().map(codec::encode_notice_op).collect::<Result<Vec<_>, _>>()?;
                self.rewrite_append(NOTICES, 8, &codec::pack_ops(&enc))?;
            }
            PersistOp::WriteMeta(m) => {
                let b = rjson::meta_bytes(m)?;
                self.replace_sealed(META, 9, &[&b])?;
            }
            PersistOp::WriteIdentity(b) => {
                self.replace_sealed(IDENTITY, 7, &[&b[..]])?;
            }
        }
        Ok(())
    }

    /// `AppendLog` (device.md §6.4.1, §7.3): page records of at most 256
    /// entries, appended to the current segment, or a new segment created
    /// with its header and first page in one `write_new` when the current
    /// one is closed (DEV-STORE-003). The seqs continue the log without a gap
    /// (DEV-STORE-006).
    fn append_log(&mut self, records: &[LogRecord]) -> Result<(), DeviceError> {
        if records.is_empty() {
            return Ok(());
        }
        let contiguous = records.windows(2).all(|w| w[0].seq.checked_add(1) == Some(w[1].seq));
        if !contiguous || (self.head != 0 && records[0].seq != self.head + 1) {
            return Err(err(WRITE_FAILED));
        }
        let mut i = 0;
        while i < records.len() {
            let next = records[i].seq;
            let open = self.segments.last().filter(|s| s.is_open() && (s.entries > 0 || s.first == next));
            let room = open.map_or(SEGMENT_ENTRIES, |s| SEGMENT_ENTRIES - s.entries);
            let end = records.len().min(i + usize::try_from(room).unwrap_or(usize::MAX));
            let page = codec::split_log(&records[i..end])[0];
            let plain = codec::encode_log_page(page);
            let last = page[page.len() - 1].seq;
            if open.is_some() {
                let seg = self.segments.last().ok_or_else(|| err(WRITE_FAILED))?;
                let mut s = FileSealer::resume(&self.key, &seg.header, seg.records).map_err(seal_err)?;
                let rec = s.seal_record(&plain, &mut OsEntropy).map_err(seal_err)?;
                let path = self.path(&segment_name(seg.first));
                self.files.append(&path, &rec).map_err(io)?;
                self.files.sync(&path).map_err(io)?;
                if let Some(seg) = self.segments.last_mut() {
                    seg.records += 1;
                    seg.entries += page.len() as u64;
                    seg.bytes += rec.len() as u64;
                    seg.last = Some(last);
                }
            } else {
                let (header, body) = self.seal_new(1, &[&plain])?;
                self.replace_raw(&segment_name(next), &body)?;
                self.segments.push(Segment {
                    first: next,
                    last: Some(last),
                    entries: page.len() as u64,
                    bytes: body.len() as u64,
                    header,
                    records: 1,
                });
            }
            self.head = last;
            i += page.len();
        }
        Ok(())
    }

    /// The sealer's history records above `history.tdk`'s last seq and at or
    /// below `upto`, in seq order, one sealed record each (device.md §8.3).
    fn append_history(&mut self, history: &[(u64, Vec<u8>)], upto: u64) -> Result<(), DeviceError> {
        let mut last = self.history_last;
        let mut recs = Vec::new();
        for (s, b) in history {
            if *s > last && *s <= upto {
                recs.push(b.clone());
                last = *s;
            }
        }
        if recs.is_empty() {
            return Ok(());
        }
        self.append_sealed(HISTORY, 11, &recs)?;
        self.history_last = last;
        Ok(())
    }

    /// `TruncateLogBelow(s, c)` (device.md §7.3, DEV-FILE-006): `log_floor`
    /// and `floor_chain` durable in `device.tdk` first, then every segment
    /// whose last seq is below `s` removed.
    fn truncate_log_below(&mut self, s: u64, c: Option<Hash32>) -> Result<(), DeviceError> {
        self.settings.log_floor = s;
        self.settings.floor_chain = c;
        self.put_settings()?;
        let (gone, kept): (Vec<Segment>, Vec<Segment>) =
            std::mem::take(&mut self.segments).into_iter().partition(|g| g.last.map_or(g.first < s, |l| l < s));
        self.segments = kept;
        for g in gone {
            self.remove(&segment_name(g.first))?;
        }
        Ok(())
    }

    /// `RewriteOutbox` (device.md §7.3, §8.3, DEV-RUN-005): `device.tdk` with
    /// `minter` raised to cover every origin state in `outbox.tdo`, then the
    /// file replaced with one `Put` per record, in order.
    fn rewrite_outbox(&mut self, records: &[OutboxRecord]) -> Result<(), DeviceError> {
        self.settings.minter = max_minter(self.settings.minter, self.outbox_minter);
        self.put_settings()?;
        let enc = records
            .iter()
            .map(|r| codec::encode_outbox_op(&OutboxOp::Put(r.clone())))
            .collect::<Result<Vec<_>, _>>()?;
        self.rewrite_append(OUTBOX, 5, &codec::pack_ops(&enc))?;
        self.outbox_minter = records.iter().fold(None, |m, r| max_minter(m, CoreMachine::minter(&r.origin_state)));
        Ok(())
    }

    // ------------------------------------------------------------ local reads (§8.2)

    /// `ReadLog { from, to }` (device.md §8.2, DEV-RUN-007): the records from
    /// the segments, or `Missing { code: "device/store_corrupt" }` when one
    /// of `from..=to` is not held or does not open.
    pub fn read_log(&self, from: u64, to: u64) -> LocalReadResult {
        let missing = LocalReadResult::Missing { code: STORE_CORRUPT };
        if from > to {
            return missing;
        }
        let mut out: Vec<LogRecord> = Vec::new();
        for seg in &self.segments {
            let Some(last) = seg.last else { continue };
            if seg.first > to || last < from {
                continue;
            }
            let Ok(bytes) = self.files.read(&self.path(&segment_name(seg.first))) else { return missing };
            let Ok(f) = taskdeck_seal::open_file(&self.key, FileKind::Log, &self.board, &self.dev, &bytes) else {
                return missing;
            };
            for rec in &f.records {
                let Ok(page) = codec::decode_log_page(rec) else { return missing };
                out.extend(page.into_iter().filter(|r| (from..=to).contains(&r.seq)));
            }
        }
        let complete = out.len() as u64 == to - from + 1 && out.iter().zip(from..).all(|(r, s)| r.seq == s);
        if complete { LocalReadResult::Records(out) } else { missing }
    }

    /// `ReadBase { seq }` (device.md §8.2): the base, or, for a missing or
    /// corrupt one, `Missing { code: "device/store_corrupt" }`; a corrupt one
    /// is renamed aside as §7.4 does and reported as `quarantined`.
    pub fn read_base(&mut self, seq: u64, now_ms: i64) -> (LocalReadResult, Option<OpenWarning>) {
        let missing = LocalReadResult::Missing { code: STORE_CORRUPT };
        let name = base_name(seq);
        let p = self.path(&name);
        if !self.files.exists(&p) {
            return (missing, None);
        }
        let got = self.files.read(&p).ok().and_then(|b| {
            let f = taskdeck_seal::open_file(&self.key, FileKind::Base, &self.board, &self.dev, &b).ok()?;
            if !matches!(f.tail, FileTail::Clean) || f.records.len() != 1 {
                return None;
            }
            codec::decode_checkpoint(&f.records[0]).ok().filter(|c| c.board_seq == seq)
        });
        match got {
            Some(c) => (LocalReadResult::Base(c), None),
            None => {
                let mut w = Vec::new();
                let _ = quarantine(&*self.files, &self.dir, &name, 3, now_ms, &mut w);
                (missing, w.pop())
            }
        }
    }

    // ------------------------------------------------------------ create and open

    /// Creates a new identity directory (device.md §18.1 step 7, §18.3) in
    /// this order: the directory (`create_dir`, then `sync_dir` of `sync/`),
    /// `store.tds`, `identity.tdk`, `device.tdk`, `genesis.tdb` when there is
    /// one, `bootstrap.tdk`; each by the replace procedure. The caller has
    /// made `L_d` durable in the key store first (DEV-KEY-002) and writes
    /// `pending.tds` after.
    pub fn create(
        files: Arc<dyn FileLayer>,
        dir: &Path,
        l_d: &Zeroizing<[u8; 32]>,
        new: NewIdentity<'_>,
    ) -> Result<IdentityStore, DeviceError> {
        files.create_dir(dir).map_err(io)?;
        if let Some(parent) = dir.parent() {
            files.sync_dir(parent).map_err(io)?;
        }
        replace_raw(&*files, dir, STORE_TDS, &new.descriptor.encode())?;
        let store = IdentityStore {
            files,
            dir: dir.to_path_buf(),
            board: BoardId(new.descriptor.board_id),
            dev: DevId(new.descriptor.dev),
            key: StorageKey::from_bytes(**l_d),
            failed: false,
            appends: BTreeMap::new(),
            segments: Vec::new(),
            head: 0,
            history_last: 0,
            settings: new.settings,
            outbox_minter: None,
        };
        store.replace_sealed(IDENTITY, 7, &[new.identity])?;
        store.put_settings()?;
        if let Some(g) = new.genesis_blob {
            store.replace_raw(GENESIS, g)?;
        }
        let b = codec::encode_bootstrap(new.bootstrap)?;
        store.replace_sealed(BOOTSTRAP, 17, &[&b])?;
        Ok(store)
    }

    /// Reads an identity directory at open (device.md §7.4) in the order of
    /// §7.4, after removing every `*.tmp` file (DEV-FILE-007) and every
    /// quarantined file older than 30 days (§7.5): torn and bad tails of
    /// append files truncated and reported (DEV-FILE-008), corrupt files
    /// handled as the table of §7.4 states, records below `log_floor`
    /// dropped and segments wholly below it removed, and `Loaded` built as
    /// §7.4 maps it (DEV-FILE-009). `board`, `dev` are those of `store.tds`,
    /// which the caller has checked; `now_ms` is the device clock.
    pub fn open(
        files: Arc<dyn FileLayer>,
        dir: &Path,
        board: [u8; 16],
        dev: [u8; 16],
        l_d: &Zeroizing<[u8; 32]>,
        build_id: &str,
        now_ms: i64,
    ) -> Result<Opened, DeviceError> {
        let mut rd = Reader {
            files: files.clone(),
            dir: dir.to_path_buf(),
            key: StorageKey::from_bytes(**l_d),
            board: BoardId(board),
            dev: DevId(dev),
            now_ms,
            warnings: Vec::new(),
        };
        rd.sweep()?;
        let names = rd.names()?;

        // device.tdk, identity.tdk
        let settings = match rd.required(DEVICE, 12)? {
            Some(b) => DeviceSettings::from_bytes(&b).map_err(|_| corrupt(12))?,
            None => return Err(corrupt(12)),
        };
        let identity = rd.required(IDENTITY, 7)?.ok_or_else(|| corrupt(7))?;

        // bootstrap.tdk (only while there is no checkpoint), checkpoint.tdk
        let has_checkpoint = files.exists(&dir.join(CHECKPOINT));
        let bootstrap = if has_checkpoint {
            None
        } else {
            let b = rd.required(BOOTSTRAP, 17)?.ok_or_else(|| corrupt(17))?;
            Some(codec::decode_bootstrap(&b).map_err(|_| corrupt(17))?)
        };
        let checkpoint = if has_checkpoint {
            let b = rd.required(CHECKPOINT, 2)?.ok_or_else(|| corrupt(2))?;
            Some(codec::decode_checkpoint(&b).map_err(|_| corrupt(2))?)
        } else {
            None
        };
        let b_seq = checkpoint.as_ref().map_or(0, |c| c.board_seq);

        // the bases
        let mut bases = Vec::new();
        for n in &names {
            let Some(seq) = seq_of(n, "base-", ".tdk") else { continue };
            let ok = rd
                .replace_file(n, 3)
                .ok()
                .flatten()
                .and_then(|b| codec::decode_checkpoint(&b).ok())
                .filter(|c| c.board_seq == seq);
            match ok {
                Some(c) => bases.push((c.board_seq, c.board_chain)),
                None => rd.quarantine(n, 3)?,
            }
        }

        // the log segments, in name order
        let seg_names: Vec<(u64, &String)> =
            names.iter().filter_map(|n| seq_of(n, "log-", ".tdl").map(|s| (s, n))).collect();
        let mut segments = Vec::new();
        let mut log: Vec<LogRecord> = Vec::new();
        for (i, (first, n)) in seg_names.iter().enumerate() {
            let below = seg_names.get(i + 1).is_some_and(|(next, _)| next.saturating_sub(1) <= b_seq);
            let read = rd.append_file(n, 1).and_then(|got| {
                let Some((recs, header, len)) = got else { return Ok(None) };
                let mut seg = Segment { first: *first, last: None, entries: 0, bytes: len, header, records: 0 };
                let mut out = Vec::new();
                for r in &recs {
                    let page = codec::decode_log_page(r)?;
                    let expect = seg.last.map_or(*first, |l| l + 1);
                    if page[0].seq != expect {
                        return Err(codec::fmt());
                    }
                    seg.last = page.last().map(|r| r.seq);
                    seg.entries += page.len() as u64;
                    seg.records += 1;
                    out.extend(page);
                }
                Ok(Some((seg, out)))
            });
            match read {
                Ok(Some((seg, recs))) => {
                    segments.push(seg);
                    log.extend(recs);
                }
                Ok(None) => {}
                Err(e) if e.code == WRITE_FAILED => return Err(e),
                Err(_) if below => rd.quarantine(n, 1)?,
                Err(_) => return Err(corrupt(1)),
            }
        }
        // The end of an interrupted `TruncateLogBelow` (§7.4).
        let floor = settings.log_floor.max(1);
        let (gone, kept): (Vec<Segment>, Vec<Segment>) =
            segments.into_iter().partition(|g| g.last.map_or(g.first < floor, |l| l < floor));
        for g in gone {
            let p = dir.join(segment_name(g.first));
            files.remove(&p).map_err(io)?;
            files.sync_dir(dir).map_err(io)?;
        }
        let segments = kept;
        log.retain(|r| r.seq >= floor);
        let tail: Vec<LogRecord> = log.iter().filter(|r| r.seq > b_seq).cloned().collect();
        let tail_ok = tail.windows(2).all(|w| w[0].seq + 1 == w[1].seq)
            && (checkpoint.is_none() || tail.first().is_none_or(|r| r.seq == b_seq + 1));
        if !tail_ok {
            return Err(corrupt(1));
        }
        let head = log.last().map_or(0, |r| r.seq);

        // hashes.tdk
        let mut appends = BTreeMap::new();
        let mut hashes: Vec<HashRecord> = Vec::new();
        match rd.append_file(HASHES, 4).and_then(|g| decode_all(g, codec::decode_hashes)) {
            Ok(Some((v, f))) => {
                hashes = v;
                appends.insert(HASHES, f);
            }
            Ok(None) => {}
            Err(e) if e.code == WRITE_FAILED => return Err(e),
            Err(_) => rd.quarantine(HASHES, 4)?,
        }
        // outbox.tdo
        let mut outbox: Vec<OutboxOp> = Vec::new();
        match rd.append_file(OUTBOX, 5).and_then(|g| decode_all(g, codec::decode_outbox_ops)) {
            Ok(Some((v, f))) => {
                outbox = v;
                appends.insert(OUTBOX, f);
            }
            Ok(None) => {}
            Err(e) if e.code == WRITE_FAILED => return Err(e),
            Err(_) => return Err(corrupt(5)),
        }
        let outbox_minter = outbox.iter().fold(None, |m, o| match o {
            OutboxOp::Put(r) => max_minter(m, CoreMachine::minter(&r.origin_state)),
            OutboxOp::Drop(_) => m,
        });
        // notices.tdk
        let mut notices: Vec<NoticeOp> = Vec::new();
        match rd.append_file(NOTICES, 8).and_then(|g| decode_all(g, codec::decode_notice_ops)) {
            Ok(Some((v, f))) => {
                notices = v;
                appends.insert(NOTICES, f);
            }
            Ok(None) => {}
            Err(e) if e.code == WRITE_FAILED => return Err(e),
            Err(_) => rd.quarantine(NOTICES, 8)?,
        }
        // meta.tdk
        let meta: Option<Meta> = match rd.required(META, 9)? {
            Some(b) => Some(rjson::meta_from(&b).map_err(|_| corrupt(9))?),
            None if checkpoint.is_some() => return Err(corrupt(9)),
            None => None,
        };
        // marker.tdk
        let marker: Option<Marker> = match rd.replace_file(MARKER, 10) {
            Ok(Some(b)) => match codec::decode_marker(&b) {
                Ok(m) => Some(m),
                Err(_) => {
                    rd.quarantine(MARKER, 10)?;
                    None
                }
            },
            Ok(None) => None,
            Err(e) if e.code == WRITE_FAILED => return Err(e),
            Err(_) => {
                rd.quarantine(MARKER, 10)?;
                None
            }
        };
        // history.tdk
        let mut history: Vec<Vec<u8>> = Vec::new();
        let mut history_last = 0;
        match rd.append_file(HISTORY, 11) {
            Ok(Some((recs, header, _))) => {
                for r in &recs {
                    let h = HistoryRecord::from_bytes(r).map_err(|_| corrupt(11))?;
                    if h.seq() <= history_last {
                        return Err(corrupt(11));
                    }
                    history_last = h.seq();
                    history.push(r.to_vec());
                }
                appends.insert(HISTORY, AppendFile { header, records: recs.len() as u64 });
            }
            Ok(None) => {}
            Err(e) if e.code == WRITE_FAILED => return Err(e),
            Err(_) => return Err(corrupt(11)),
        }

        let mut session = [0u8; 16];
        OsEntropy.fill(&mut session).map_err(|_| err(WRITE_FAILED))?;
        let loaded = Loaded {
            checkpoint,
            bases,
            chains: log.iter().map(|r| (r.seq, r.chain, r.tau)).collect(),
            log_floor: floor,
            floor_chain: settings.floor_chain,
            tail,
            hashes,
            outbox,
            notices,
            meta,
            marker,
            build_id: build_id.to_string(),
            session,
            bootstrap,
        };
        let store = IdentityStore {
            files,
            dir: dir.to_path_buf(),
            board: BoardId(board),
            dev: DevId(dev),
            key: rd.key,
            failed: false,
            appends,
            segments,
            head,
            history_last,
            settings,
            outbox_minter,
        };
        Ok(Opened { store, loaded, history, identity, warnings: rd.warnings })
    }
}

/// Every record of an append file decoded; any record that does not decode
/// makes the whole file `device/record_format` (§7.4).
fn decode_all<T>(
    got: Option<Records>,
    dec: fn(&[u8]) -> Result<Vec<T>, DeviceError>,
) -> Result<Option<(Vec<T>, AppendFile)>, DeviceError> {
    let Some((recs, header, _)) = got else { return Ok(None) };
    let mut out = Vec::new();
    for r in &recs {
        out.extend(dec(r)?);
    }
    Ok(Some((out, AppendFile { header, records: recs.len() as u64 })))
}

/// The replace procedure of plain bytes (device.md §7.2).
pub(crate) fn replace_raw(files: &dyn FileLayer, dir: &Path, name: &str, bytes: &[u8]) -> Result<(), DeviceError> {
    let tmp = dir.join(format!("{name}.tmp"));
    files.write_new(&tmp, bytes).map_err(io)?;
    files.sync(&tmp).map_err(io)?;
    files.rename(&tmp, &dir.join(name)).map_err(io)?;
    files.sync_dir(dir).map_err(io)
}

/// `YYYYMMDD-HHMMSS` of the device clock, UTC (finding f2-7).
fn stamp(now_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(now_ms)
        .unwrap_or_default()
        .format("%Y%m%d-%H%M%S")
        .to_string()
}

/// The time a quarantined name was set aside, from its stamp.
fn stamp_ms(name: &str) -> Option<i64> {
    let s = name.rsplit_once(".corrupt-")?.1;
    let t = chrono::NaiveDateTime::parse_from_str(s, "%Y%m%d-%H%M%S").ok()?;
    Some(t.and_utc().timestamp_millis())
}

/// Renames a file to `<name>.corrupt-<YYYYMMDD-HHMMSS>` and reports it
/// (device.md §7.4).
fn quarantine(
    files: &dyn FileLayer,
    dir: &Path,
    name: &str,
    kind: u8,
    now_ms: i64,
    warnings: &mut Vec<OpenWarning>,
) -> Result<(), DeviceError> {
    let to = dir.join(format!("{name}.corrupt-{}", stamp(now_ms)));
    files.rename(&dir.join(name), &to).map_err(io)?;
    files.sync_dir(dir).map_err(io)?;
    warnings.push(OpenWarning::Quarantined { kind, file: name.to_string() });
    Ok(())
}

/// The reading at open (device.md §7.4).
struct Reader {
    files: Arc<dyn FileLayer>,
    dir: PathBuf,
    key: StorageKey,
    board: BoardId,
    dev: DevId,
    now_ms: i64,
    warnings: Vec<OpenWarning>,
}

impl Reader {
    fn names(&self) -> Result<Vec<String>, DeviceError> {
        let mut v: Vec<String> = self
            .files
            .list(&self.dir)
            .map_err(|_| err(STORE_CORRUPT))?
            .into_iter()
            .filter_map(|n| n.into_string().ok())
            .collect();
        v.sort();
        Ok(v)
    }

    /// Removes every `*.tmp` (DEV-FILE-007) and every quarantined file more
    /// than 30 days old (device.md §7.5).
    fn sweep(&self) -> Result<(), DeviceError> {
        let mut removed = false;
        for n in self.names()? {
            let old = stamp_ms(&n).is_some_and(|t| self.now_ms.saturating_sub(t) > QUARANTINE_MS);
            if n.ends_with(".tmp") || old {
                self.files.remove(&self.dir.join(&n)).map_err(io)?;
                removed = true;
            }
        }
        if removed {
            self.files.sync_dir(&self.dir).map_err(io)?;
        }
        Ok(())
    }

    fn quarantine(&mut self, name: &str, kind: u8) -> Result<(), DeviceError> {
        quarantine(&*self.files, &self.dir, name, kind, self.now_ms, &mut self.warnings)
    }

    /// A replace file's one record, `None` when absent; a file that does not
    /// open clean with exactly one record is `device/record_format` (a torn or
    /// bad tail is as corrupt: §7.4).
    fn replace_file(&self, name: &str, k: u8) -> Result<Option<Zeroizing<Vec<u8>>>, DeviceError> {
        let p = self.dir.join(name);
        if !self.files.exists(&p) {
            return Ok(None);
        }
        let bytes = self.files.read(&p).map_err(|_| codec::fmt())?;
        let f = taskdeck_seal::open_file(&self.key, kind(k), &self.board, &self.dev, &bytes).map_err(|_| codec::fmt())?;
        if !matches!(f.tail, FileTail::Clean) || f.records.len() != 1 {
            return Err(codec::fmt());
        }
        Ok(f.records.into_iter().next())
    }

    /// A replace file of a required kind: one that does not open is
    /// `device/store_corrupt` naming the kind (§7.4).
    fn required(&self, name: &str, k: u8) -> Result<Option<Zeroizing<Vec<u8>>>, DeviceError> {
        self.replace_file(name, k).map_err(|_| corrupt(k))
    }

    /// An append file's records, header and valid length, `None` when
    /// absent. A torn or bad tail is truncated (`truncate`, `sync`) and
    /// reported (DEV-FILE-008); a header or corrupt record is
    /// `device/record_format`; a failed truncate is `device/write_failed`.
    fn append_file(&mut self, name: &str, k: u8) -> Result<Option<Records>, DeviceError> {
        let p = self.dir.join(name);
        if !self.files.exists(&p) {
            return Ok(None);
        }
        let bytes = self.files.read(&p).map_err(|_| codec::fmt())?;
        let f = taskdeck_seal::open_file(&self.key, kind(k), &self.board, &self.dev, &bytes).map_err(|_| codec::fmt())?;
        let header = bytes[..55].to_vec();
        let len = match f.tail {
            FileTail::Clean => bytes.len() as u64,
            FileTail::Torn { valid_bytes } | FileTail::Bad { valid_bytes } => {
                self.files.truncate(&p, valid_bytes).map_err(io)?;
                self.files.sync(&p).map_err(io)?;
                self.warnings.push(if matches!(f.tail, FileTail::Torn { .. }) {
                    OpenWarning::TornTail { kind: k }
                } else {
                    OpenWarning::BadTail { kind: k }
                });
                valid_bytes
            }
        };
        Ok(Some((f.records, header, len)))
    }
}
