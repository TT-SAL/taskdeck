//! Tests of the store (device.md §5 to §8): the encodings of §6, every
//! `PersistOp` through disk and back, reopen after each kind of record,
//! torn, bad and corrupt files at open, the lock and the steps of §5.4.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use taskdeck_core::ids::IdMinter;
use taskdeck_sync::{
    AlarmKind, AlarmRecord, Banner, BannerDetail, BannerKind, BeatInfo, Body, Bootstrap, Checkpoint, CtlIntent,
    DutyState, Evidence, ForkSource, HaltKind, HaltRecord, Hash32, HashRecord, Head, Kind, LastEntry,
    LocalReadResult, LogRecord, Loaded, Marker, Meta, NoticeKind, NoticeOp, OpRecord, Operation, OutboxOp,
    OutboxRecord, PersistOp, Registry, RelayTarget, Report, SnapInfo, SnapRecord, SnapshotMaterial, StopKind,
    StopRecord, UpgradeMode, Verdict, Via,
};
use zeroize::Zeroizing;

use super::codec::{self, StoreDescriptor};
use super::rjson;
use super::*;
use crate::adapt::CoreMachine;
use crate::{DeviceError, FileLayer, KeyStore, KeyStoreError, OpenWarning, OsFiles};

const NOW: i64 = 1_790_000_000_000;
const BOARD: [u8; 16] = [0xb0; 16];
const DEV: [u8; 16] = [0xde; 16];

// ---------------------------------------------------------------- fixtures

/// A key store in a map, for tests without `test-trust` (kind 5, as
/// `MemoryKeyStore`).
#[derive(Default)]
struct Keys(Mutex<KeyMap>);

type KeyMap = BTreeMap<([u8; 16], [u8; 16]), Zeroizing<[u8; 32]>>;

impl KeyStore for Keys {
    fn kind(&self) -> u8 {
        5
    }
    fn create(&self, b: &[u8; 16], d: &[u8; 16], k: &Zeroizing<[u8; 32]>) -> Result<(), KeyStoreError> {
        self.0.lock().map_err(|_| KeyStoreError::Failed)?.insert((*b, *d), k.clone());
        Ok(())
    }
    fn load(&self, b: &[u8; 16], d: &[u8; 16]) -> Result<Zeroizing<[u8; 32]>, KeyStoreError> {
        self.0.lock().map_err(|_| KeyStoreError::Failed)?.get(&(*b, *d)).cloned().ok_or(KeyStoreError::NotFound)
    }
    fn delete(&self, b: &[u8; 16], d: &[u8; 16]) -> Result<(), KeyStoreError> {
        self.0.lock().map_err(|_| KeyStoreError::Failed)?.remove(&(*b, *d));
        Ok(())
    }
}

fn target() -> RelayTarget {
    RelayTarget { host: "relay.test:8443".into(), door: [1, 2, 3, 4, 5], home_gen: 1 }
}

fn chain(seq: u64) -> Hash32 {
    let mut c = [0u8; 32];
    c[..8].copy_from_slice(&seq.to_be_bytes());
    c[31] = 0xcc;
    c
}

fn rec(seq: u64) -> LogRecord {
    let mut frame = vec![(seq % 251) as u8; 670 + (seq % 7) as usize];
    frame[..8].copy_from_slice(&seq.to_be_bytes());
    LogRecord { seq, chain: chain(seq), tau: 1_000 * seq as i64 - 5, frame }
}

fn recs(from: u64, to: u64) -> Vec<LogRecord> {
    (from..=to).map(rec).collect()
}

fn registry() -> Registry {
    let info = SnapInfo {
        seq: 64,
        hash: [3; 32],
        hash_version: 1,
        blob: [4; 16],
        sha256: [5; 32],
        size: 1_000,
        parts: 1,
        inflated: 4_000,
        epoch: 2,
    };
    Registry {
        snapshots: vec![SnapRecord {
            entry_seq: 70,
            author: [7; 16],
            info,
            entry_clock: -3,
            verdicts: vec![([8; 16], Verdict::Ack, 71), ([9; 16], Verdict::Mismatch, 72)],
            own: Some(Verdict::Ack),
        }],
        reports: vec![Report { seq: 64, dev: [7; 16], hash: [3; 32], hash_version: 1, report_seq: 66 }],
        pending_verdicts: vec![(70, Verdict::Ack)],
        last_entry: vec![LastEntry { dev: [7; 16], at_ms: 12, seq: 70, based_on_seq: 69 }],
        restored: Some((3, 9)),
    }
}

fn ckpt(seq: u64) -> Checkpoint {
    Checkpoint {
        board_seq: seq,
        board_chain: chain(seq),
        board: format!("{{\"board\":{seq}}}").into_bytes(),
        membership: vec![0x4d; 120],
        log_floor: 1,
        registry: if seq.is_multiple_of(2) { registry() } else { Registry::default() },
    }
}

fn origin(item: u64, block: u64) -> Vec<u8> {
    CoreMachine::origin_state(&IdMinter { device_no: 1, next_item: item, next_block: block })
}

fn outbox_record(n: u8) -> OutboxRecord {
    OutboxRecord {
        entry_id: [n; 16],
        order: u64::from(n),
        at_ms: 1_700_000_000_000 + i64::from(n),
        based_on: Head { seq: 3, chain: chain(3) },
        kind: Kind::Cmd,
        body: Body::Cmd(b"{\"op\":\"add\"}".to_vec()),
        origin_state: origin(u64::from(n) * 10, 5),
        via: Via::Append,
        hold: n.is_multiple_of(2),
        ctr: Some(u64::from(n)),
        frame: Some(vec![n; 700]),
        frame_hash: Some([n; 32]),
        epoch: Some(1),
        ack: Some((5, chain(5))),
        seen: None,
        op: Some(4),
        reseals: 1,
    }
}

fn ctl_record(n: u8, body: Body, via: Via) -> OutboxRecord {
    OutboxRecord {
        kind: Kind::Ctl,
        body,
        via,
        origin_state: Vec::new(),
        ctr: None,
        frame: None,
        frame_hash: None,
        epoch: None,
        ack: None,
        op: None,
        ..outbox_record(n)
    }
}

fn meta(attempt: u64) -> Meta {
    let alarm = |id: u64, kind: AlarmKind, evidence: Evidence| AlarmRecord {
        id,
        kind,
        raised_ms: 5,
        seq: Some(id),
        author: Some([1; 16]),
        code: Some("seal/x".into()),
        evidence,
        resolved: id.is_multiple_of(2),
    };
    Meta {
        attempt,
        last_failed: Some((9, 1)),
        build_id: "build-1".into(),
        halt: Some(HaltRecord {
            kind: HaltKind::Crash { frame_hash: [2; 32] },
            seq: 9,
            author: [1; 16],
            since_ms: -1,
            code: "core/panic".into(),
        }),
        stops: vec![
            StopRecord { kind: StopKind::Evidence, since_ms: 1, alarm: Some(1) },
            StopRecord { kind: StopKind::Revoked { by_log: true }, since_ms: 2, alarm: None },
            StopRecord { kind: StopKind::LocalFault { code: "engine/local_fault".into() }, since_ms: 3, alarm: None },
            StopRecord { kind: StopKind::Rebootstrap { after: 77 }, since_ms: 4, alarm: None },
        ],
        alarms: vec![
            alarm(
                1,
                AlarmKind::RelayBehind,
                Evidence::RelayBehind {
                    relay_head: Head { seq: 1, chain: chain(1) },
                    relay_last_ctr: None,
                    local_head: Head { seq: 2, chain: chain(2) },
                    ack: Some((2, chain(2))),
                },
            ),
            alarm(2, AlarmKind::Fork, Evidence::Fork { local_chain: [1; 32], relay_chain: [2; 32], source: ForkSource::PageHead }),
            alarm(
                3,
                AlarmKind::Withheld,
                Evidence::Withheld {
                    relay_head: 5,
                    claimed_ctr: Some(2),
                    cursor: 3,
                    missing_from: 4,
                    missing_to: 5,
                    counters: true,
                },
            ),
            alarm(4, AlarmKind::IdentityClone, Evidence::IdentityClone { session: Some([6; 16]), ctrs: vec![1, 2] }),
            alarm(5, AlarmKind::StateMismatch, Evidence::Mismatch { groups: vec![([1; 32], vec![[2; 16], [3; 16]])] }),
            alarm(6, AlarmKind::SkippedFrame, Evidence::Frame { frame_hash: None }),
            alarm(7, AlarmKind::RelayProtocol, Evidence::Request { request: "read".into() }),
            alarm(8, AlarmKind::BlobFailure, Evidence::Blob { blob: [9; 16] }),
            alarm(9, AlarmKind::RestoredCopy, Evidence::RestoredCopy { entries: 3 }),
        ],
        banners: vec![
            Banner {
                id: (9, 0),
                kind: BannerKind::Moved,
                seq: 9,
                at_ms: 1,
                author: [1; 16],
                author_name: "Desk \"one\"\n".into(),
                author_fp: "ABCD".into(),
                subject: None,
                subject_name: None,
                subject_fp: None,
                detail: BannerDetail::Moved { host: "relay.example".into(), home_gen: 2 },
                acked: false,
            },
            Banner {
                id: (10, 1),
                kind: BannerKind::RulesVote,
                seq: 10,
                at_ms: 2,
                author: [1; 16],
                author_name: "Ünïcode".into(),
                author_fp: "EF".into(),
                subject: Some([2; 16]),
                subject_name: Some("Phone".into()),
                subject_fp: Some("12".into()),
                detail: BannerDetail::None,
                acked: true,
            },
        ],
        relay: target(),
        prev_session: Some([5; 16]),
        conflicts: 2,
        clock: Some((1_700_000_000_000, -500)),
        last_beat_ms: 0,
        duties: DutyState { snapshot: Some((64, 9, 1)), last_compact_day: 20_000, purge: None, upgrade: Some((2, 7)) },
        ops: vec![
            OpRecord { ticket: 1, op: Operation::Purge, stage: 1, attempts: 0, started_ms: 3, mark: 0 },
            OpRecord {
                ticket: 2,
                op: Operation::FollowAddress { host: "h".into(), door: [1; 5], code: "c".into() },
                stage: 5,
                attempts: 1,
                started_ms: 4,
                mark: 9,
            },
        ],
        move_tries: (1, 99),
        continuity_unchecked: Some(64),
        restored_copy: None,
        based_on_unchecked: 3,
    }
}

/// A history record of seal.md §21.5, laid out by hand.
fn history_rec(seq: u64) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&seq.to_be_bytes());
    b.extend_from_slice(&[1; 16]);
    b.extend_from_slice(&seq.to_be_bytes());
    b.extend_from_slice(&[2; 16]);
    b.extend_from_slice(&5i64.to_be_bytes());
    b.extend_from_slice(&[3; 32]);
    b.extend_from_slice(&3u32.to_be_bytes());
    b.extend_from_slice(b"abc");
    b.extend_from_slice(&[4; 64]);
    b
}

fn genesis() -> Bootstrap {
    Bootstrap::Genesis { board: b"{}".to_vec(), frame_1: vec![9; 900], relay: target() }
}

/// What the store must present after each write: the in-memory store of
/// `adapt::flow_tests`, applying each op as engine.md §7.11 states.
#[derive(Clone, Default)]
struct Model {
    ckpt: Option<Checkpoint>,
    bases: BTreeMap<u64, Checkpoint>,
    log: BTreeMap<u64, LogRecord>,
    log_floor: u64,
    floor_chain: Option<Hash32>,
    hashes: Vec<HashRecord>,
    outbox: Vec<OutboxOp>,
    notices: Vec<NoticeOp>,
    meta: Option<Meta>,
    marker: Option<Marker>,
    identity: Vec<u8>,
    bootstrap: Option<Bootstrap>,
}

impl Model {
    fn apply(&mut self, op: &PersistOp) {
        match op {
            PersistOp::AppendLog(v) => {
                for r in v {
                    self.log.insert(r.seq, r.clone());
                }
            }
            PersistOp::SetMarker(m) => self.marker = Some(*m),
            PersistOp::ClearMarker => self.marker = None,
            PersistOp::WriteCheckpoint(c) => self.ckpt = Some(c.clone()),
            PersistOp::WriteBase(c) => {
                self.bases.insert(c.board_seq, c.clone());
            }
            PersistOp::DeleteBases(v) => {
                for s in v {
                    self.bases.remove(s);
                }
            }
            PersistOp::TruncateLogBelow(s, c) => {
                self.log_floor = *s;
                self.floor_chain = *c;
                self.log.retain(|k, _| k >= s);
            }
            PersistOp::AppendHashes(v) => self.hashes.extend(v.iter().copied()),
            PersistOp::Outbox(v) => self.outbox.extend(v.iter().cloned()),
            PersistOp::RewriteOutbox(v) => self.outbox = v.iter().cloned().map(OutboxOp::Put).collect(),
            PersistOp::Notices(v) => self.notices.extend(v.iter().cloned()),
            PersistOp::RewriteNotices(v) => self.notices = v.clone(),
            PersistOp::WriteMeta(m) => self.meta = Some(m.clone()),
            PersistOp::WriteIdentity(b) => self.identity = b.to_vec(),
        }
    }

    fn loaded(&self, session: [u8; 16]) -> Loaded {
        let b = self.ckpt.as_ref().map_or(0, |c| c.board_seq);
        Loaded {
            checkpoint: self.ckpt.clone(),
            bases: self.bases.values().map(|c| (c.board_seq, c.board_chain)).collect(),
            chains: self.log.values().map(|r| (r.seq, r.chain, r.tau)).collect(),
            log_floor: self.log_floor.max(1),
            floor_chain: self.floor_chain,
            tail: self.log.values().filter(|r| r.seq > b).cloned().collect(),
            hashes: self.hashes.clone(),
            outbox: self.outbox.clone(),
            notices: self.notices.clone(),
            meta: self.meta.clone(),
            marker: self.marker,
            build_id: "build-1".into(),
            session,
            bootstrap: if self.ckpt.is_some() { None } else { self.bootstrap.clone() },
        }
    }
}

/// A data folder with a fresh active identity, created as device.md §18.1
/// step 7 and activated as step 10.
struct Folder {
    _tmp: tempfile::TempDir,
    data: std::path::PathBuf,
    files: Arc<dyn FileLayer>,
    keys: Keys,
    lock: Lock,
}

impl Folder {
    fn new(bootstrap: &Bootstrap) -> (Folder, IdentityStore) {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let data = tmp.path().join("taskdeck_data");
        let files: Arc<dyn FileLayer> = Arc::new(OsFiles);
        let lock = take_lock(&data);
        assert!(lock.is_held());
        let keys = Keys::default();
        let new = NewIdentity {
            descriptor: StoreDescriptor { board_id: BOARD, dev: DEV, key_store: 5, created_ms: NOW },
            identity: b"identity-0",
            settings: DeviceSettings::new("Desk"),
            bootstrap,
            genesis_blob: Some(b"sealed genesis"),
        };
        let store = create_identity(files.clone(), &data, &keys, &Zeroizing::new([0x1d; 32]), new).expect("created");
        assert_eq!(read_pending(&*files, &data).expect("reads"), Some((BOARD, DEV)));
        activate(&*files, &data, &BOARD, &DEV).expect("activated");
        assert_eq!(read_pending(&*files, &data).expect("reads"), None);
        (Folder { _tmp: tmp, data, files, keys, lock }, store)
    }

    fn dir(&self) -> std::path::PathBuf {
        identity_dir(&self.data, &BOARD, &DEV)
    }

    fn open(&self) -> Result<Opened, DeviceError> {
        match open_folder(self.files.clone(), &self.data, &self.lock, false, &self.keys, "build-1", NOW)? {
            FolderState::Synced(o) => Ok(*o),
            _ => panic!("not synced"),
        }
    }
}

/// Reopens and checks `Loaded` against the model, with no warning.
fn reopen(f: &Folder, model: &Model) -> Opened {
    let o = f.open().expect("reopens");
    assert_eq!(o.loaded, model.loaded(o.loaded.session), "the loaded records");
    assert!(o.warnings.is_empty(), "{:?}", o.warnings);
    o
}

// ---------------------------------------------------------------- encodings

/// DEV-STORE-004: the descriptors' layouts and their refusals.
#[test]
fn descriptors_have_the_layout_of_6_2() {
    let a = encode_active(ACTIVE_MAGIC, &BOARD, &DEV);
    assert_eq!(a.len(), 45);
    assert_eq!(&a[..5], b"TDA1\x01");
    assert_eq!(decode_active(ACTIVE_MAGIC, &a).expect("decodes"), (BOARD, DEV));
    assert_eq!(decode_active(PENDING_MAGIC, &a).err().map(|e| e.code), Some("device/store_descriptor"));
    let mut bad = a.clone();
    bad[10] ^= 1;
    assert_eq!(decode_active(ACTIVE_MAGIC, &bad).err().map(|e| e.code), Some("device/store_descriptor"));
    assert!(decode_active(ACTIVE_MAGIC, &a[..44]).is_err());

    let d = StoreDescriptor { board_id: BOARD, dev: DEV, key_store: 4, created_ms: -7 };
    let b = d.encode();
    assert_eq!(b.len(), 54);
    assert_eq!(&b[..5], b"TDS1\x01");
    assert_eq!(b[37], 4);
    assert_eq!(StoreDescriptor::decode(&b).expect("decodes"), d);
    let mut bad = b.clone();
    bad[53] ^= 1;
    assert!(StoreDescriptor::decode(&bad).is_err());
}

/// DEV-STORE-005, DEV-STORE-006: the log page of §6.4.1.
#[test]
fn log_pages_round_trip_and_refuse_gaps() {
    let v = recs(5, 9);
    let b = encode_log_page(&v);
    assert_eq!(b.len(), 2 + v.iter().map(|r| 52 + r.frame.len()).sum::<usize>());
    assert_eq!(&b[..2], &[0, 5]);
    assert_eq!(decode_log_page(&b).expect("decodes"), v);
    let gap = encode_log_page(&[rec(5), rec(7)]);
    assert!(decode_log_page(&gap).is_err());
    let mut short = rec(5);
    short.frame.truncate(669);
    assert!(decode_log_page(&encode_log_page(&[short])).is_err());
    assert!(decode_log_page(&b[..b.len() - 1]).is_err());
    assert!(decode_log_page(&[0, 0]).is_err());
    let many = recs(1, 300);
    let pages = codec::split_log(&many);
    assert_eq!(pages.iter().map(|p| p.len()).collect::<Vec<_>>(), vec![256, 44]);
}

/// §6.4.2 to §6.4.6 and §6.4.9: every record kind round-trips.
#[test]
fn record_plaintexts_round_trip() {
    for c in [ckpt(1), ckpt(64)] {
        let b = encode_checkpoint(&c).expect("encodes");
        assert_eq!(&b[..4], b"TDC1");
        assert_eq!(decode_checkpoint(&b).expect("decodes"), c);
    }
    let h = vec![HashRecord { seq: 64, hash_version: 1, hash: [1; 32] }, HashRecord { seq: 128, hash_version: 2, hash: [2; 32] }];
    let hb = codec::encode_hashes(&h);
    assert_eq!(hb.len(), 2 + 44 * 2);
    assert_eq!(codec::decode_hashes(&hb).expect("decodes"), h);

    let ops = vec![
        OutboxOp::Put(outbox_record(1)),
        OutboxOp::Drop([1; 16]),
        OutboxOp::Put(ctl_record(
            2,
            Body::Ctl(CtlIntent::Rotate { revoke: vec![[3; 16]], reinstate: vec![], purge_before: Some(9) }),
            Via::Fence { host: "a.example".into(), door: [9; 5], home_gen: 2 },
        )),
        OutboxOp::Put(ctl_record(
            3,
            Body::Ctl(CtlIntent::AddDevice {
                dev: [4; 16],
                sign_pk: [5; 32],
                hpke_pk: [6; 32],
                name: "Phone".into(),
                transcript: [7; 32],
                bootstrap_seq: 12,
            }),
            Via::Register { dev: [4; 16], sign_pk: [5; 32] },
        )),
        OutboxOp::Put(ctl_record(4, Body::Ctl(CtlIntent::Skip { targets: vec![(3, [1; 32])] }), Via::Append)),
        OutboxOp::Put(ctl_record(
            5,
            Body::Ctl(CtlIntent::RulesUpgrade { rules: 2, mode: UpgradeMode::Override }),
            Via::Append,
        )),
        OutboxOp::Put(ctl_record(
            6,
            Body::Beat(BeatInfo {
                rules: vec![1],
                envelopes: vec![(1, 1)],
                hash_versions: vec![1],
                state: Some((64, [1; 32], 1)),
                acks: vec![(64, Verdict::Ack)],
            }),
            Via::Append,
        )),
    ];
    let enc: Vec<Vec<u8>> = ops.iter().map(|o| codec::encode_outbox_op(o).expect("encodes")).collect();
    let packed = codec::pack_ops(&enc);
    assert_eq!(packed.len(), 1);
    assert_eq!(codec::decode_outbox_ops(&packed[0]).expect("decodes"), ops);

    let n = vec![
        NoticeOp::Add { kind: NoticeKind::Notice, seq: 3, record: b"{\"n\":1}".to_vec() },
        NoticeOp::Add { kind: NoticeKind::Refusal, seq: 4, record: b"{\"r\":1}".to_vec() },
        NoticeOp::Dismiss { seq: 3, index: 0 },
    ];
    let enc: Vec<Vec<u8>> = n.iter().map(|o| codec::encode_notice_op(o).expect("encodes")).collect();
    assert_eq!(codec::decode_notice_ops(&codec::pack_ops(&enc)[0]).expect("decodes"), n);
    let big = NoticeOp::Add { kind: NoticeKind::Notice, seq: 1, record: vec![b'x'; 4_097] };
    assert!(codec::encode_notice_op(&big).is_err());

    let m = Marker { first: 3, last: 9, attempt: 2 };
    assert_eq!(codec::encode_marker(&m).len(), 24);
    assert_eq!(codec::decode_marker(&codec::encode_marker(&m)).expect("decodes"), m);

    let join = Bootstrap::Join {
        material: SnapshotMaterial {
            seq: 64,
            chain: [1; 32],
            hash_version: 1,
            state_hash: [2; 32],
            board: b"{}".to_vec(),
            membership: vec![3; 50],
        },
        added_seq: 70,
        relay: target(),
    };
    for b in [genesis(), join] {
        let bytes = codec::encode_bootstrap(&b).expect("encodes");
        assert_eq!(codec::decode_bootstrap(&bytes).expect("decodes"), b);
    }
    let bad = Bootstrap::Genesis { board: vec![], frame_1: vec![], relay: RelayTarget { host: String::new(), ..target() } };
    assert!(codec::encode_bootstrap(&bad).is_err());
}

/// More than 1,024 ops make more than one record (device.md §6.4.4).
#[test]
fn ops_are_split_at_1024() {
    let enc: Vec<Vec<u8>> = (0..2_500u32).map(|i| codec::encode_notice_op(&NoticeOp::Dismiss { seq: 1, index: i }).expect("ok")).collect();
    let packed = codec::pack_ops(&enc);
    assert_eq!(packed.len(), 3);
    let back: Vec<NoticeOp> = packed.iter().flat_map(|p| codec::decode_notice_ops(p).expect("decodes")).collect();
    assert_eq!(back.len(), 2_500);
}

/// DEV-STORE-007: record JSON is the canonical encoding of §6.5's mapping,
/// and the reader refuses what the canonical parser or the type refuses.
#[test]
fn record_json_is_canonical_and_strict() {
    let m = meta(3);
    let b = rjson::meta_bytes(&m).expect("encodes");
    assert!(taskdeck_core::canon::is_canonical(&b, rjson::META_MAX));
    assert_eq!(rjson::meta_from(&b).expect("decodes"), m);
    let r = outbox_record(7);
    let rb = rjson::outbox_record_bytes(&r).expect("encodes");
    assert_eq!(rjson::outbox_record_from(&rb).expect("decodes"), r);

    // The mapping's shapes: keys in byte order, unit variants as strings,
    // hexN and base64, an absent `None`.
    let reg = Registry { pending_verdicts: vec![(5, Verdict::Mismatch)], restored: None, ..Registry::default() };
    let t = String::from_utf8(rjson::registry_bytes(&reg).expect("encodes")).expect("utf-8");
    assert_eq!(t, r#"{"last_entry":[],"pending_verdicts":[[5,"mismatch"]],"reports":[],"snapshots":[]}"#);
    let t = String::from_utf8(rb).expect("utf-8");
    assert!(t.starts_with(r#"{"ack":[5,"#), "{t}");
    assert!(t.contains(r#""body":{"cmd":"eyJvcCI6ImFkZCJ9"}"#), "{t}");
    assert!(t.contains(r#""kind":"cmd""#) && t.contains(r#""via":"append""#) && !t.contains("seen"), "{t}");
    assert!(t.contains(r#""entry_id":"07070707070707070707070707070707""#), "{t}");

    // Refusals: not canonical, an unknown member, a missing one, a wrong type.
    let spaced = b"{\"last_entry\": [],\"pending_verdicts\":[],\"reports\":[],\"snapshots\":[]}";
    assert_eq!(rjson::registry_from(spaced).err().map(|e| e.code), Some("device/record_format"));
    let unknown = br#"{"extra":1,"last_entry":[],"pending_verdicts":[],"reports":[],"snapshots":[]}"#;
    assert!(rjson::registry_from(unknown).is_err());
    let missing = br#"{"last_entry":[],"pending_verdicts":[],"reports":[]}"#;
    assert!(rjson::registry_from(missing).is_err());
    let upper = br#"{"last_entry":[{"at_ms":1,"based_on_seq":1,"dev":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA","seq":1}],"pending_verdicts":[],"reports":[],"snapshots":[]}"#;
    assert!(rjson::registry_from(upper).is_err());
    // An integer the encoding cannot carry is a local fault.
    let wide = Registry { restored: Some((1 << 53, 0)), ..Registry::default() };
    assert_eq!(rjson::registry_bytes(&wide).err().map(|e| e.code), Some("device/record_range"));
}

/// §16: `device.tdk`'s record, its defaults and its refusals.
#[test]
fn settings_round_trip_with_defaults() {
    let s = DeviceSettings::new("Desk");
    assert_eq!(s.to_bytes().expect("encodes"), br#"{"device_name":"Desk"}"#);
    let mut s2 = s.clone();
    s2.log_floor = 100;
    s2.floor_chain = Some([0xab; 32]);
    s2.history_complete = true;
    s2.minter = Some(IdMinter::new(1));
    s2.other.insert("weather_source".into(), serde_json::Value::String("own".into()));
    let b = s2.to_bytes().expect("encodes");
    assert!(String::from_utf8_lossy(&b).contains(r#""minter":{"block":1,"device_no":1,"item":1}"#));
    assert_eq!(DeviceSettings::from_bytes(&b).expect("decodes"), s2);
    assert!(DeviceSettings::from_bytes(br#"{"device_name":"Desk","log_floor":1}"#).is_err(), "a default written");
    assert!(DeviceSettings::from_bytes(br#"{"device_name":"Desk","colour":1}"#).is_err(), "an unknown field");
    assert!(DeviceSettings::from_bytes(br#"{"log_floor":2}"#).is_err(), "no name");
}

// ---------------------------------------------------------------- persist and open

/// Every `PersistOp` through disk and back gives the `Loaded` of the
/// records written (engine.md §7.10, device.md §7.3, §7.4, DEV-FILE-009),
/// with a reopen after each kind of record.
#[test]
fn every_persist_op_through_disk_and_back() {
    let boot = genesis();
    let (f, mut store) = Folder::new(&boot);
    let mut model = Model { bootstrap: Some(boot), identity: b"identity-0".to_vec(), ..Model::default() };
    let o = reopen(&f, &model);
    assert_eq!(&o.identity[..], b"identity-0");
    assert!(f.dir().join("genesis.tdb").exists() && f.dir().join("bootstrap.tdk").exists());
    drop(o);

    let steps: Vec<Vec<PersistOp>> = vec![
        vec![PersistOp::AppendLog(recs(1, 3))],
        vec![PersistOp::SetMarker(Marker { first: 2, last: 3, attempt: 1 })],
        vec![PersistOp::WriteMeta(meta(1))],
        vec![PersistOp::WriteCheckpoint(ckpt(1)), PersistOp::WriteBase(ckpt(1))],
        vec![PersistOp::AppendHashes(vec![HashRecord { seq: 64, hash_version: 1, hash: [6; 32] }])],
        vec![PersistOp::Outbox(vec![OutboxOp::Put(outbox_record(1)), OutboxOp::Put(outbox_record(2))])],
        vec![PersistOp::Notices(vec![
            NoticeOp::Add { kind: NoticeKind::Notice, seq: 2, record: b"{}".to_vec() },
            NoticeOp::Add { kind: NoticeKind::Refusal, seq: 3, record: b"{\"r\":1}".to_vec() },
        ])],
        vec![PersistOp::ClearMarker, PersistOp::AppendLog(recs(4, 300))],
        vec![PersistOp::WriteCheckpoint(ckpt(200)), PersistOp::WriteBase(ckpt(64)), PersistOp::WriteBase(ckpt(128))],
        vec![PersistOp::WriteBase(ckpt(128)), PersistOp::DeleteBases(vec![1, 5])],
        vec![PersistOp::Outbox(vec![OutboxOp::Drop([1; 16])]), PersistOp::Notices(vec![NoticeOp::Dismiss { seq: 2, index: 0 }])],
        vec![PersistOp::RewriteOutbox(vec![outbox_record(2)])],
        vec![PersistOp::RewriteNotices(vec![NoticeOp::Add { kind: NoticeKind::Notice, seq: 3, record: b"{}".to_vec() }])],
        vec![PersistOp::TruncateLogBelow(100, Some(chain(99)))],
        vec![PersistOp::WriteIdentity(Zeroizing::new(b"identity-1".to_vec()))],
        vec![PersistOp::AppendLog(recs(301, 310)), PersistOp::WriteMeta(meta(2))],
        vec![PersistOp::AppendHashes(vec![HashRecord { seq: 128, hash_version: 1, hash: [7; 32] }])],
        vec![PersistOp::RewriteOutbox(vec![]), PersistOp::RewriteNotices(vec![])],
        vec![PersistOp::TruncateLogBelow(150, None)],
    ];
    for ops in steps {
        store.persist(&ops, &[]).expect("persisted");
        for op in &ops {
            model.apply(op);
        }
        drop(store);
        let o = reopen(&f, &model);
        assert_eq!(&o.identity[..], &model.identity[..]);
        store = o.store;
    }
    // After the first checkpoint the bootstrap and the genesis snapshot are gone.
    assert!(!f.dir().join("bootstrap.tdk").exists() && !f.dir().join("genesis.tdb").exists());
    // The floor in device.tdk (DEV-FILE-006); the minter raised over the
    // dropped records' origin states (DEV-RUN-005).
    assert_eq!(store.settings().log_floor, 150);
    assert_eq!(store.settings().floor_chain, None);
    assert_eq!(store.minter(), Some(IdMinter { device_no: 1, next_item: 20, next_block: 5 }));
    // The local reads (device.md §8.2, DEV-RUN-007).
    match store.read_log(150, 310) {
        LocalReadResult::Records(v) => assert_eq!(v, recs(150, 310)),
        other => panic!("{other:?}"),
    }
    assert!(matches!(store.read_log(300, 311), LocalReadResult::Missing { .. }));
    assert_eq!(store.read_base(128, NOW).0, LocalReadResult::Base(ckpt(128)));
    assert_eq!(store.read_base(1, NOW), (LocalReadResult::Missing { code: "device/store_corrupt" }, None));
    // Names (device.md §6.1).
    for n in ["store.tds", "identity.tdk", "device.tdk", "checkpoint.tdk", "hashes.tdk", "outbox.tdo", "notices.tdk", "meta.tdk"] {
        assert!(f.dir().join(n).exists(), "{n}");
    }
    assert!(f.dir().join("base-00000000000000000064.tdk").exists());
    assert!(f.dir().join("log-00000000000000000001.tdl").exists());
    assert!(!f.dir().join("marker.tdk").exists());
}

/// DEV-RUN-003 and DEV-RUN-006: history records after an `AppendLog` only
/// for its seqs above the file's last, and before a `WriteCheckpoint` those
/// at or below it; each once.
#[test]
fn history_records_follow_the_log_and_the_checkpoint() {
    let (f, mut store) = Folder::new(&genesis());
    let h: Vec<(u64, Vec<u8>)> = [2, 5, 9, 12].iter().map(|s| (*s, history_rec(*s))).collect();
    store.persist(&[PersistOp::AppendLog(recs(1, 6))], &h).expect("persisted");
    assert_eq!(store.history_last(), 5);
    store.persist(&[PersistOp::AppendLog(recs(7, 10))], &h).expect("persisted");
    assert_eq!(store.history_last(), 9);
    store.persist(&[PersistOp::AppendLog(recs(11, 11))], &h).expect("persisted");
    assert_eq!(store.history_last(), 9);
    // A rejoin's snapshot history: written before the checkpoint at 12.
    store.persist(&[PersistOp::WriteCheckpoint(ckpt(12)), PersistOp::WriteMeta(meta(1))], &h).expect("persisted");
    assert_eq!(store.history_last(), 12);
    store.persist(&[PersistOp::AppendLog(recs(12, 14))], &h).expect("persisted");
    drop(store);
    let o = f.open().expect("reopens");
    assert_eq!(o.history, h.iter().map(|(_, b)| b.clone()).collect::<Vec<_>>());
    assert_eq!(o.store.history_last(), 12);
}

/// DEV-STORE-003: a segment is closed at 16,384 entries and the next one is
/// named after its first seq; `TruncateLogBelow` removes only segments
/// wholly below the floor (DEV-FILE-006); a corrupt segment wholly at or
/// below the checkpoint is quarantined, one above it fails the open.
#[test]
fn segments_roll_over_truncate_and_quarantine() {
    let (f, mut store) = Folder::new(&genesis());
    let mut model = Model { bootstrap: Some(genesis()), identity: b"identity-0".to_vec(), ..Model::default() };
    let first = recs(1, 16_380);
    let ops = vec![PersistOp::AppendLog(first), PersistOp::AppendLog(recs(16_381, 16_400))];
    store.persist(&ops, &[]).expect("persisted");
    ops.iter().for_each(|o| model.apply(o));
    assert!(f.dir().join(segment_name(1)).exists());
    assert!(f.dir().join(segment_name(16_385)).exists(), "the second segment starts at 16,385");
    drop(store);
    let o = reopen(&f, &model);
    let mut store = o.store;
    let ops = vec![
        PersistOp::WriteCheckpoint(ckpt(16_390)),
        PersistOp::WriteMeta(meta(1)),
        PersistOp::AppendLog(recs(16_401, 16_402)),
    ];
    store.persist(&ops, &[]).expect("persisted");
    ops.iter().for_each(|o| model.apply(o));
    match store.read_log(16_380, 16_390) {
        LocalReadResult::Records(v) => assert_eq!(v, recs(16_380, 16_390)),
        other => panic!("{other:?}"),
    }
    drop(store);

    // The first segment, wholly below the checkpoint, damaged in its first record.
    let seg1 = f.dir().join(segment_name(1));
    let mut bytes = std::fs::read(&seg1).expect("reads");
    bytes[55 + 40] ^= 1;
    std::fs::write(&seg1, &bytes).expect("writes");
    let o = f.open().expect("reopens");
    assert_eq!(o.warnings, vec![OpenWarning::Quarantined { kind: 1, file: segment_name(1) }]);
    assert!(!seg1.exists());
    assert!(o.loaded.chains.iter().all(|c| c.0 >= 16_385));
    assert_eq!(o.loaded.tail, recs(16_391, 16_402));
    drop(o);

    // The second segment, which holds the tail, damaged in its first record
    // of two: the open fails.
    let seg2 = f.dir().join(segment_name(16_385));
    let good = std::fs::read(&seg2).expect("reads");
    let mut bytes = good.clone();
    bytes[55 + 40] ^= 1;
    std::fs::write(&seg2, &bytes).expect("writes");
    let e = f.open().err().expect("fails");
    assert_eq!((e.code, e.message.as_deref()), ("device/store_corrupt", Some("kind 1")));
    std::fs::write(&seg2, &good).expect("writes");

    // A truncation below 16,385 removes nothing that holds a seq at or above it.
    let mut store = f.open().expect("reopens").store;
    store.persist(&[PersistOp::TruncateLogBelow(16_386, Some(chain(16_385)))], &[]).expect("persisted");
    assert!(seg2.exists());
    store.persist(&[PersistOp::AppendLog(recs(16_403, 16_404))], &[]).expect("persisted");
    drop(store);
    let o = f.open().expect("reopens");
    assert_eq!(o.loaded.chains.first().map(|c| c.0), Some(16_386));
    assert_eq!(o.loaded.tail, recs(16_391, 16_404));
}

/// DEV-FILE-008: a torn tail and a bad tail of a log segment are truncated
/// at open, reported, and the next append continues the file.
#[test]
fn torn_and_bad_tails_of_a_segment() {
    let (f, mut store) = Folder::new(&genesis());
    let mut model = Model { bootstrap: Some(genesis()), identity: b"identity-0".to_vec(), ..Model::default() };
    let op = PersistOp::AppendLog(recs(1, 5));
    store.persist(std::slice::from_ref(&op), &[]).expect("persisted");
    model.apply(&op);
    drop(store);
    let seg = f.dir().join(segment_name(1));
    let len = std::fs::metadata(&seg).expect("exists").len();

    // Torn: a record cut short by a crash.
    let mut torn = vec![0, 0, 3, 0];
    torn.extend_from_slice(&[7; 100]);
    std::fs::OpenOptions::new().append(true).open(&seg).and_then(|mut h| std::io::Write::write_all(&mut h, &torn)).expect("appends");
    let o = f.open().expect("reopens");
    assert_eq!(o.warnings, vec![OpenWarning::TornTail { kind: 1 }]);
    assert_eq!(o.loaded, model.loaded(o.loaded.session));
    assert_eq!(std::fs::metadata(&seg).expect("exists").len(), len, "truncated to the valid bytes");
    let mut store = o.store;
    let op = PersistOp::AppendLog(recs(6, 7));
    store.persist(std::slice::from_ref(&op), &[]).expect("persisted");
    model.apply(&op);
    drop(store);
    let o = reopen(&f, &model);
    drop(o);

    // Bad: a whole last record that does not open.
    let len = std::fs::metadata(&seg).expect("exists").len();
    let mut bad = 40u32.to_be_bytes().to_vec();
    bad.extend_from_slice(&[9; 24 + 40]);
    std::fs::OpenOptions::new().append(true).open(&seg).and_then(|mut h| std::io::Write::write_all(&mut h, &bad)).expect("appends");
    let o = f.open().expect("reopens");
    assert_eq!(o.warnings, vec![OpenWarning::BadTail { kind: 1 }]);
    assert_eq!(o.loaded, model.loaded(o.loaded.session));
    assert_eq!(std::fs::metadata(&seg).expect("exists").len(), len);
    let mut store = o.store;
    let op = PersistOp::AppendLog(recs(8, 8));
    store.persist(std::slice::from_ref(&op), &[]).expect("persisted");
    model.apply(&op);
    drop(store);
    reopen(&f, &model);
}

/// The policies of device.md §7.4's second table and DEV-FILE-007.
#[test]
fn corrupt_files_at_open() {
    let (f, mut store) = Folder::new(&genesis());
    let ops = vec![
        PersistOp::AppendLog(recs(1, 3)),
        PersistOp::WriteCheckpoint(ckpt(2)),
        PersistOp::WriteBase(ckpt(2)),
        PersistOp::WriteMeta(meta(1)),
        PersistOp::SetMarker(Marker { first: 3, last: 3, attempt: 1 }),
        PersistOp::AppendHashes(vec![HashRecord { seq: 64, hash_version: 1, hash: [1; 32] }]),
        PersistOp::Notices(vec![NoticeOp::Dismiss { seq: 1, index: 0 }]),
        PersistOp::Outbox(vec![OutboxOp::Drop([1; 16])]),
    ];
    store.persist(&ops, &[]).expect("persisted");
    drop(store);
    // A damaged header (its `dev`): `seal/file_header`, as corrupt.
    let flip = |name: &str| {
        let p = f.dir().join(name);
        let mut b = std::fs::read(&p).expect("reads");
        b[10] ^= 1;
        std::fs::write(&p, &b).expect("writes");
    };

    // A leftover temporary file is removed first; an old quarantine too.
    std::fs::write(f.dir().join("meta.tdk.tmp"), b"half").expect("writes");
    std::fs::write(f.dir().join("hashes.tdk.corrupt-20200101-000000"), b"old").expect("writes");
    std::fs::write(f.dir().join("hashes.tdk.corrupt-20260901-000000"), b"recent").expect("writes");
    let o = f.open().expect("reopens");
    assert!(o.warnings.is_empty());
    assert!(!f.dir().join("meta.tdk.tmp").exists());
    assert!(!f.dir().join("hashes.tdk.corrupt-20200101-000000").exists());
    assert!(f.dir().join("hashes.tdk.corrupt-20260901-000000").exists());
    drop(o);

    // The quarantined kinds: renamed aside, treated as empty or absent.
    for (name, kind) in [("base-00000000000000000002.tdk", 3u8), ("hashes.tdk", 4), ("notices.tdk", 8), ("marker.tdk", 10)] {
        flip(name);
        let o = f.open().expect("reopens");
        assert_eq!(o.warnings, vec![OpenWarning::Quarantined { kind, file: name.to_string() }], "{name}");
        assert!(!f.dir().join(name).exists());
        assert!(f.dir().join(format!("{name}.corrupt-20260921-141320")).exists(), "{name}");
    }
    let o = f.open().expect("reopens");
    assert!(o.loaded.bases.is_empty() && o.loaded.hashes.is_empty() && o.loaded.notices.is_empty());
    assert!(o.loaded.marker.is_none());
    // A quarantined append file starts again with a new header.
    let mut store = o.store;
    let ops = vec![PersistOp::AppendHashes(vec![HashRecord { seq: 128, hash_version: 1, hash: [2; 32] }])];
    store.persist(&ops, &[]).expect("persisted");
    drop(store);
    assert_eq!(f.open().expect("reopens").loaded.hashes.len(), 1);

    // The required kinds fail the open and change nothing.
    for (name, kind) in [("outbox.tdo", 5u8), ("meta.tdk", 9), ("checkpoint.tdk", 2), ("identity.tdk", 7), ("device.tdk", 12)] {
        let p = f.dir().join(name);
        let good = std::fs::read(&p).expect("reads");
        flip(name);
        let e = f.open().err().expect("fails");
        assert_eq!(e.code, "device/store_corrupt", "{name}");
        assert_eq!(e.message, Some(format!("kind {kind}")));
        std::fs::write(&p, &good).expect("writes");
    }
    // A record moved from another file does not open (seal.md §19.3).
    let a = std::fs::read(f.dir().join("meta.tdk")).expect("reads");
    std::fs::write(f.dir().join("checkpoint.tdk"), &a).expect("writes");
    assert_eq!(f.open().err().map(|e| e.code), Some("device/store_corrupt"));
}

/// DEV-FILE-005: after a failed write every later write is refused until
/// the next open.
#[test]
fn a_failed_write_stops_the_store() {
    let (f, mut store) = Folder::new(&genesis());
    store.persist(&[PersistOp::AppendLog(recs(1, 2))], &[]).expect("persisted");
    // A gap is refused, and so is everything after it.
    assert!(store.persist(&[PersistOp::AppendLog(recs(4, 4))], &[]).is_err());
    assert!(store.failed());
    assert!(store.persist(&[PersistOp::WriteMeta(meta(1))], &[]).is_err());
    assert!(!f.dir().join("meta.tdk").exists());
    drop(store);
    let mut store = f.open().expect("reopens").store;
    store.persist(&[PersistOp::AppendLog(recs(3, 3))], &[]).expect("persisted");
}

// ---------------------------------------------------------------- the folder

/// DEV-LOCK-001 to 003: a second open of a locked folder is contended.
#[test]
fn a_second_open_of_a_locked_folder() {
    let tmp = tempfile::tempdir().expect("a temporary directory");
    let data = tmp.path().join("new").join("taskdeck_data");
    let first = take_lock(&data);
    assert!(first.is_held(), "the folder is created and locked");
    assert!(data.join(".lock").exists());
    let second = take_lock(&data);
    assert!(matches!(second, Lock::Contended));
    assert_eq!(second.refusal().map(|e| e.code), Some("device/folder_locked"));
    assert_eq!(second.warning(), Some(OpenWarning::FolderLocked));
    drop(first);
    assert!(take_lock(&data).is_held());
    assert_eq!(std::fs::metadata(data.join(".lock")).expect("exists").len(), 0, "never written");
}

/// The steps of device.md §5.4 (DEV-OPEN-002, DEV-OPEN-004, DEV-KEY-002).
#[test]
fn the_steps_of_5_4() {
    let tmp = tempfile::tempdir().expect("a temporary directory");
    let data = tmp.path().join("taskdeck_data");
    let files: Arc<dyn FileLayer> = Arc::new(OsFiles);
    let lock = take_lock(&data);
    let keys = Keys::default();
    let open = |lock: &Lock, cloud: bool, keys: &dyn KeyStore| {
        open_folder(files.clone(), &data, lock, cloud, keys, "build-1", NOW)
    };
    // An empty folder: local-only, nothing written but the lock.
    assert!(matches!(open(&lock, false, &keys), Ok(FolderState::LocalOnly { pending_create: false })));
    assert_eq!(std::fs::read_dir(&data).expect("lists").count(), 1);
    // A pending create.
    let new = NewIdentity {
        descriptor: StoreDescriptor { board_id: BOARD, dev: DEV, key_store: 5, created_ms: NOW },
        identity: b"identity-0",
        settings: DeviceSettings::new("Desk"),
        bootstrap: &genesis(),
        genesis_blob: None,
    };
    let wrong = Keys::default();
    let other_kind = NewIdentity { descriptor: StoreDescriptor { key_store: 4, ..new.descriptor }, ..new };
    assert!(create_identity(files.clone(), &data, &wrong, &Zeroizing::new([1; 32]), other_kind).is_err());
    let new = NewIdentity {
        descriptor: StoreDescriptor { board_id: BOARD, dev: DEV, key_store: 5, created_ms: NOW },
        identity: b"identity-0",
        settings: DeviceSettings::new("Desk"),
        bootstrap: &genesis(),
        genesis_blob: None,
    };
    drop(create_identity(files.clone(), &data, &keys, &Zeroizing::new([1; 32]), new).expect("created"));
    assert!(keys.load(&BOARD, &DEV).is_ok(), "the key first");
    assert!(matches!(open(&lock, false, &keys), Ok(FolderState::LocalOnly { pending_create: true })));
    assert!(identities(&*files, &data).is_empty(), "the pending identity is not listed");
    activate(&*files, &data, &BOARD, &DEV).expect("activated");
    assert_eq!(
        identities(&*files, &data),
        vec![crate::StoredIdentity { board_id: BOARD, dev: DEV, key_store: 5, created_ms: NOW, active: true }]
    );
    // Synced.
    assert!(matches!(open(&lock, false, &keys), Ok(FolderState::Synced(_))));
    // The lock, the cloud check, the key store.
    assert_eq!(open(&Lock::Contended, false, &keys).err().map(|e| e.code), Some("device/folder_locked"));
    assert_eq!(open(&Lock::Unsupported, false, &keys).err().map(|e| e.code), Some("device/folder_unlockable"));
    assert_eq!(open(&lock, true, &keys).err().map(|e| e.code), Some("device/cloud_folder"));
    assert!(matches!(open(&lock, false, &wrong), Ok(FolderState::KeyLost { reason: KeyStoreError::NotFound, .. })));
    struct Locked;
    impl KeyStore for Locked {
        fn kind(&self) -> u8 {
            5
        }
        fn create(&self, _: &[u8; 16], _: &[u8; 16], _: &Zeroizing<[u8; 32]>) -> Result<(), KeyStoreError> {
            Err(KeyStoreError::Unavailable)
        }
        fn load(&self, _: &[u8; 16], _: &[u8; 16]) -> Result<Zeroizing<[u8; 32]>, KeyStoreError> {
            Err(KeyStoreError::Unavailable)
        }
        fn delete(&self, _: &[u8; 16], _: &[u8; 16]) -> Result<(), KeyStoreError> {
            Ok(())
        }
    }
    assert_eq!(open(&lock, false, &Locked).err().map(|e| e.code), Some("device/keystore_unavailable"));
    // Another key: nothing opens.
    let other = Keys::default();
    other.create(&BOARD, &DEV, &Zeroizing::new([2; 32])).expect("created");
    assert_eq!(open(&lock, false, &other).err().map(|e| e.code), Some("device/store_corrupt"));
    // A damaged descriptor.
    let p = sync_dir(&data).join("active.tds");
    let mut b = std::fs::read(&p).expect("reads");
    b[0] = b'X';
    std::fs::write(&p, &b).expect("writes");
    assert_eq!(open(&lock, false, &keys).err().map(|e| e.code), Some("device/store_descriptor"));
}

/// DEV-OPEN-001, §5.3 rule 1.
#[test]
fn cloud_folder_names() {
    for p in ["/home/a/OneDrive/x", "/home/a/onedrive - Contoso/x", "/home/a/Dropbox (Personal)/x", "/u/My Drive/d", "/u/sync/d"] {
        assert!(is_cloud_folder(Path::new(p)), "{p}");
    }
    for p in ["/home/a/Documents/x", "/home/a/OneDriveX/x", "/home/a/Syncthing/x"] {
        assert!(!is_cloud_folder(Path::new(p)), "{p}");
    }
}
