//! The engine on the real core and seal, in process (task F1): one device,
//! from genesis to an applied edit and back after a restart.
//!
//! The driver is the engine lead's kit (`crates/taskdeck-sync/src/engine/
//! kit_tests.rs`: `Store`, `Node`, `Hub`), copied and cut down: a durable
//! in-memory store that applies each `PersistOp` and answers `Persisted` in
//! the order of the writes (ENG-PERSIST-001) and local reads from what is
//! durable (D11), and a small relay that sequences frames as `relay.md` §9
//! says (P1 to P6 and T1 to T5; P7, the signature, is not checked: the
//! device crate has no Ed25519 verifier of its own and every frame here is
//! honest) with the real chain function (`taskdeck_seal::chain_step`), and
//! answers `hello` (§10.4) and `read` (§10.5). An independent reader, which
//! holds only `R_1` and the anchor and classifies the relay's log with
//! `taskdeck_seal::classify` itself, rebuilds the board with
//! `taskdeck_core::apply` to check the engine's confirmed state.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use taskdeck_core::apply as capply;
use taskdeck_core::board::BoardData;
use taskdeck_core::command::{LogCommand, parse_command};
use taskdeck_core::compile::{CompileCtx, compile};
use taskdeck_core::ids::IdMinter;
use taskdeck_core::intent::Intent;
use taskdeck_core::overlay::Overlay;
use taskdeck_core::time::{FixedZone, Instant};
use taskdeck_seal as seal;
use taskdeck_seal::{DeviceKeys, Entropy, EpochRoot, GenesisAnchor, Membership, OsEntropy, RecoverySeed};
use taskdeck_sync::{
    AppendResult, Bootstrap, Checkpoint, Config, Dev, Effect, Engine, Event, Hash32, HashRecord, Head, Input, Loaded,
    LocalReadResult, LogRecord, Marker, Meta, NoticeOp, Notification, OutboxOp, Page, PageEntry, PersistOp,
    RelayTarget, Sealer, StateMachine, Welcome,
};

use super::{CoreMachine, GenesisInput, SealSealer};
use crate::store::{self as dstore, DeviceSettings, FolderState, IdentityStore, NewIdentity, StoreDescriptor};
use crate::{FileLayer, KeyStore, OsFiles};

type E = Engine<CoreMachine, SealSealer>;

fn target() -> RelayTarget {
    RelayTarget { host: "relay.test".into(), door: [1, 2, 3, 4, 5], home_gen: 1 }
}

// ---------------------------------------------------------------- the store

/// What the runtime holds durably: exactly the writes it confirmed.
#[derive(Clone, Default)]
struct Store {
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
    identity: Option<Vec<u8>>,
}

impl Store {
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
            PersistOp::WriteIdentity(b) => self.identity = Some(b.to_vec()),
        }
    }

    /// `Loaded` as engine.md §7.10 lists it, from the durable records.
    fn loaded(&self, session: [u8; 16], bootstrap: Option<Bootstrap>) -> Loaded {
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
            bootstrap: if self.ckpt.is_some() { None } else { bootstrap },
        }
    }
}

// ---------------------------------------------------------------- the data folder

/// The device's store on disk (task F2): a data folder in a temporary
/// directory, its lock held, `L_d` in the test key store, the identity
/// directory written as device.md §18.1 step 7 and activated as step 10.
/// Every write goes to it as well as to the in-memory store, local reads
/// are answered from it, and a restart reads it back.
struct Disk {
    _tmp: tempfile::TempDir,
    data: std::path::PathBuf,
    files: Arc<dyn FileLayer>,
    keys: Box<dyn KeyStore>,
    lock: dstore::Lock,
    store: Option<IdentityStore>,
}

/// The key store of the flow: `FileKeyStore` in the temporary directory
/// with `test-trust` (device.md §12.6); without it, a map.
#[cfg(feature = "test-trust")]
fn key_store(dir: &std::path::Path) -> Box<dyn KeyStore> {
    Box::new(crate::FileKeyStore::new(dir.join("keys")))
}

#[cfg(not(feature = "test-trust"))]
fn key_store(_: &std::path::Path) -> Box<dyn KeyStore> {
    type Keys = BTreeMap<([u8; 16], [u8; 16]), zeroize::Zeroizing<[u8; 32]>>;
    #[derive(Default)]
    struct Map(std::sync::Mutex<Keys>);
    impl KeyStore for Map {
        fn kind(&self) -> u8 {
            5
        }
        fn create(
            &self,
            b: &[u8; 16],
            d: &[u8; 16],
            k: &zeroize::Zeroizing<[u8; 32]>,
        ) -> Result<(), crate::KeyStoreError> {
            self.0.lock().map_err(|_| crate::KeyStoreError::Failed)?.insert((*b, *d), k.clone());
            Ok(())
        }
        fn load(&self, b: &[u8; 16], d: &[u8; 16]) -> Result<zeroize::Zeroizing<[u8; 32]>, crate::KeyStoreError> {
            let m = self.0.lock().map_err(|_| crate::KeyStoreError::Failed)?;
            m.get(&(*b, *d)).cloned().ok_or(crate::KeyStoreError::NotFound)
        }
        fn delete(&self, b: &[u8; 16], d: &[u8; 16]) -> Result<(), crate::KeyStoreError> {
            self.0.lock().map_err(|_| crate::KeyStoreError::Failed)?.remove(&(*b, *d));
            Ok(())
        }
    }
    Box::new(Map::default())
}

impl Disk {
    /// A new identity: `L_d` drawn, the key store first, then the
    /// directory with `store.tds`, `identity.tdk`, `device.tdk` and
    /// `bootstrap.tdk`, then `pending.tds`; then activated.
    fn create(board: [u8; 16], dev: [u8; 16], identity: &[u8], bootstrap: &Bootstrap, at_ms: i64) -> Disk {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let data = tmp.path().join("taskdeck_data");
        let files: Arc<dyn FileLayer> = Arc::new(OsFiles);
        let lock = dstore::take_lock(&data);
        assert!(lock.is_held());
        let keys = key_store(tmp.path());
        let mut l_d = zeroize::Zeroizing::new([0u8; 32]);
        OsEntropy.fill(&mut l_d[..]).expect("entropy");
        let mut settings = DeviceSettings::new("Desk");
        settings.history_complete = true;
        settings.minter = Some(IdMinter::new(1));
        let new = NewIdentity {
            descriptor: StoreDescriptor { board_id: board, dev, key_store: keys.kind(), created_ms: at_ms },
            identity,
            settings,
            bootstrap,
            genesis_blob: None,
            history: &[],
        };
        let store = dstore::create_identity(files.clone(), &data, &*keys, &l_d, new).expect("created");
        dstore::activate(&*files, &data, &board, &dev).expect("activated");
        Disk { _tmp: tmp, data, files, keys, lock, store: Some(store) }
    }

    /// Closes the store and reads the data folder back (device.md §5.4, §7.4).
    fn reopen(&mut self, now: i64) -> dstore::Opened {
        self.store = None;
        match dstore::open_folder(self.files.clone(), &self.data, &self.lock, false, &*self.keys, "build-1", now) {
            Ok(FolderState::Synced(o)) => *o,
            Ok(_) => panic!("not synced"),
            Err(e) => panic!("does not open: {e:?}"),
        }
    }
}

// ---------------------------------------------------------------- the device

type PendingRead = (u64, Option<(u64, u64)>, u64);

struct Node {
    e: E,
    store: Store,
    wall: i64,
    mono: u64,
    pq: VecDeque<(u64, Vec<PersistOp>)>,
    /// Local reads not yet answered: the id, a `ReadLog`'s range, a `ReadBase`'s seq.
    reads: VecDeque<PendingRead>,
    fx: VecDeque<Effect>,
    notes: Vec<Notification>,
    session: u8,
    bootstrap: Option<Bootstrap>,
    /// The identity the runtime wrote before the first `Start`.
    identity0: Vec<u8>,
    board_id: [u8; 16],
    draining: bool,
    /// The store on disk, when the flow runs over it.
    disk: Option<Disk>,
}

impl Node {
    fn feed(&mut self, event: Event<LogCommand>) {
        let out = self.e.handle(Input { wall_ms: self.wall, mono_ms: self.mono, event });
        self.absorb(out);
    }

    /// Writes confirmed one at a time in the order emitted; other effects
    /// handed on in order; local reads answered from the durable store.
    fn absorb(&mut self, out: Vec<Effect>) {
        for fx in out {
            match fx {
                Effect::Persist { id, ops } => self.pq.push_back((id, ops)),
                Effect::Notify(n) => self.notes.push(n),
                Effect::ReadLog { req, from, to } => self.reads.push_back((req, Some((from, to)), 0)),
                Effect::ReadBase { req, seq } => self.reads.push_back((req, None, seq)),
                other => self.fx.push_back(other),
            }
        }
        if self.draining {
            return;
        }
        self.draining = true;
        while !(self.pq.is_empty() && self.reads.is_empty()) {
            while let Some((id, ops)) = self.pq.pop_front() {
                for op in &ops {
                    self.store.apply(op);
                }
                if let Some(d) = self.disk.as_mut() {
                    let store = d.store.as_mut().expect("open");
                    store.persist(&ops, &self.e.sealer().history()).expect("durable");
                }
                self.feed(Event::Persisted { id });
            }
            if let Some((req, range, seq)) = self.reads.pop_front() {
                let mut result = match range {
                    Some((from, to)) => {
                        let v: Vec<LogRecord> = (from..=to).filter_map(|s| self.store.log.get(&s).cloned()).collect();
                        if v.len() as u64 == to - from + 1 {
                            LocalReadResult::Records(v)
                        } else {
                            LocalReadResult::Missing { code: "device/missing" }
                        }
                    }
                    None => match self.store.bases.get(&seq) {
                        Some(c) => LocalReadResult::Base(c.clone()),
                        None => LocalReadResult::Missing { code: "device/missing" },
                    },
                };
                // From the files when the flow runs on disk: the same records.
                if let Some(store) = self.disk.as_mut().and_then(|d| d.store.as_mut()) {
                    let from_disk = match range {
                        Some((from, to)) => store.read_log(from, to),
                        None => store.read_base(seq, self.wall).0,
                    };
                    if !matches!(result, LocalReadResult::Missing { .. }) {
                        assert_eq!(from_disk, result, "the files hold what the writes confirmed");
                    }
                    result = from_disk;
                }
                self.feed(Event::LocalRead { req, result });
            }
        }
        self.draining = false;
    }

    fn start(&mut self) {
        self.session = self.session.wrapping_add(1);
        let mut loaded = self.store.loaded([self.session; 16], self.bootstrap.clone());
        if let Some(d) = self.disk.as_mut() {
            // The first `Start` of a creating device reads the identity
            // directory it just wrote (device.md §18.1 step 10).
            let o = d.reopen(self.wall);
            assert!(o.warnings.is_empty(), "{:?}", o.warnings);
            assert_eq!(o.loaded, Loaded { session: o.loaded.session, ..loaded.clone() }, "Loaded from the files");
            loaded = o.loaded;
            d.store = Some(o.store);
        }
        self.feed(Event::Start(loaded));
    }

    /// The process ends and a new one opens the same records: the sealer
    /// from the durable identity (or the one written before the first
    /// `Start`) and the history records, a new engine, `Start`.
    fn restart(&mut self) {
        let history: Vec<Vec<u8>> = self.e.sealer().history().into_iter().map(|(_, b)| b).collect();
        let identity = self.store.identity.clone().unwrap_or_else(|| self.identity0.clone());
        let sealer = match self.disk.as_mut() {
            None => SealSealer::open(self.board_id, &identity, &history).expect("the identity opens"),
            Some(d) => {
                // The new process reads the data folder: the identity and the
                // history from their files (device.md §6.4.7, §6.4.8).
                let o = d.reopen(self.wall);
                assert_eq!(&o.identity[..], &identity[..], "identity.tdk");
                assert_eq!(o.history, history, "history.tdk");
                assert!(!o.history.is_empty(), "genesis is a control entry");
                let sealer = o.sealer().expect("the identity opens");
                d.store = Some(o.store);
                sealer
            }
        };
        self.e = Engine::new(CoreMachine::new(), sealer, Config {});
        self.pq.clear();
        self.fx.clear();
        self.reads.clear();
        self.start();
    }

    fn tick(&mut self, ms: u64) {
        self.wall += ms as i64;
        self.mono += ms;
        self.feed(Event::Tick);
    }

    /// Compiles an intent against the engine's pending view and enqueues it
    /// in the same turn (DEV-FACADE-001): `at_ms` the compile clock, the
    /// origin state the minter after compiling (device.md §8.3).
    fn submit(&mut self, ticket: u64, intent: &Intent, minter: &mut IdMinter) -> LogCommand {
        let overlay = Overlay::default();
        let now = Instant::from_at_ms(self.wall).expect("in range");
        let device_no = self.e.device_no().expect("a device_no");
        assert_eq!(device_no, minter.device_no);
        let cx = CompileCtx { view: self.e.view(), zone: &FixedZone(0), now, device_no, overlay: &overlay };
        let command = compile(intent, &cx, minter).expect("compiles").command.expect("a command");
        let origin_state = CoreMachine::origin_state(minter);
        self.feed(Event::Enqueue { ticket, command: command.clone(), at_ms: self.wall, origin_state, hold: false });
        assert!(
            self.notes.iter().any(|n| matches!(n, Notification::Enqueued { ticket: t, .. } if *t == ticket)),
            "not enqueued: {:?}",
            self.notes
        );
        command
    }

    fn board_bytes(&self) -> Vec<u8> {
        CoreMachine::new().canonical_bytes(self.e.confirmed().0)
    }

    /// `taskdeck_sync::state_hash` of the confirmed state and `M_B`.
    fn state_hash(&self) -> Hash32 {
        let sm = CoreMachine::new();
        taskdeck_sync::state_hash(1, &sm.board_hash(self.e.confirmed().0), &self.e.membership_bytes())
    }
}

// ---------------------------------------------------------------- the relay

/// An author row (relay.md §7): role 1 a device, 2 the recovery identity.
#[derive(Clone, Copy)]
struct Author {
    role: u8,
    last_ctr: u64,
    last_seq: u64,
}

struct Relay {
    board: seal::BoardId,
    log: Vec<Vec<u8>>,
    chains: Vec<Hash32>,
    authors: BTreeMap<Dev, Author>,
    by_ctr: BTreeMap<(Dev, u64), u64>,
    /// The device whose token the requests carry.
    token_dev: Dev,
}

impl Relay {
    /// `create` (relay.md §10.2): seq 1 stored, the recovery identity
    /// (role 2) and the first device (role 1) registered.
    fn create(board: [u8; 16], frame_1: &[u8], dev_r: Dev, device: Dev) -> Relay {
        let board = seal::BoardId(board);
        let mut r = Relay {
            board,
            log: Vec::new(),
            chains: Vec::new(),
            authors: BTreeMap::new(),
            by_ctr: BTreeMap::new(),
            token_dev: device,
        };
        r.authors.insert(dev_r, Author { role: 2, last_ctr: 0, last_seq: 0 });
        r.authors.insert(device, Author { role: 1, last_ctr: 0, last_seq: 0 });
        let res = r.append(&[frame_1.to_vec()]);
        assert!(matches!(res[..], [AppendResult::New { seq: 1, .. }]), "seq 1 stored");
        r
    }

    fn head(&self) -> Head {
        Head {
            seq: self.log.len() as u64,
            chain: self.chains.last().copied().unwrap_or_else(|| seal::chain_zero(&self.board).0),
        }
    }

    /// relay.md §9.3 and §9.4, one batch.
    fn append(&mut self, frames: &[Vec<u8>]) -> Vec<AppendResult> {
        let mut out = Vec::with_capacity(frames.len());
        let refused = |status: u16, code: &str| AppendResult::Refused { status, code: code.into() };
        let mut stopped = false;
        for f in frames {
            if stopped {
                out.push(AppendResult::Skipped);
                continue;
            }
            // P1 to P3.
            let Ok(h) = seal::frame_header(f) else {
                out.push(refused(400, "relay/bad_frame"));
                stopped = true;
                continue;
            };
            if !(670..=262_144).contains(&f.len()) || h.ctr == 0 || h.ctr > (1 << 53) - 1 {
                out.push(refused(400, "relay/bad_frame"));
                stopped = true;
                continue;
            }
            if h.board != self.board {
                out.push(refused(400, "relay/wrong_board"));
                stopped = true;
                continue;
            }
            // P4, P5 (P6: nobody is revoked here; P7: see the module doc).
            let Some(a) = self.authors.get(&h.dev.0).copied() else {
                out.push(refused(400, "relay/unknown_author"));
                stopped = true;
                continue;
            };
            if a.role != 2 && h.dev.0 != self.token_dev {
                out.push(refused(403, "relay/not_author"));
                stopped = true;
                continue;
            }
            // T2 to T5.
            if h.ctr == a.last_ctr + 1 {
                let seq = self.log.len() as u64 + 1;
                let chain = seal::chain_step(&seal::Chain(self.head().chain), seq, f).0;
                self.log.push(f.clone());
                self.chains.push(chain);
                self.authors.insert(h.dev.0, Author { last_ctr: h.ctr, last_seq: seq, ..a });
                self.by_ctr.insert((h.dev.0, h.ctr), seq);
                out.push(AppendResult::New { seq, chain });
            } else if h.ctr <= a.last_ctr {
                match self.by_ctr.get(&(h.dev.0, h.ctr)).copied() {
                    Some(seq) if self.log[seq as usize - 1] == *f => {
                        out.push(AppendResult::Dup { seq, chain: self.chains[seq as usize - 1] })
                    }
                    Some(seq) => {
                        out.push(AppendResult::Conflict { seq, frame: self.log[seq as usize - 1].clone() });
                        stopped = true;
                    }
                    None => {
                        out.push(AppendResult::Conflict { seq: 0, frame: Vec::new() });
                        stopped = true;
                    }
                }
            } else {
                out.push(AppendResult::Gap { last_ctr: a.last_ctr, last_seq: a.last_seq });
                stopped = true;
            }
        }
        out
    }

    /// `hello` (relay.md §10.4) for the token's device.
    fn welcome(&self, now: i64) -> Welcome {
        let me = self.authors.get(&self.token_dev).copied();
        Welcome {
            head: self.head(),
            last_ctr: me.map(|a| a.last_ctr),
            last_seq: me.map(|a| a.last_seq),
            relay_now: now,
            floor: 1,
            purge_floor: 0,
            fenced: false,
            fence: None,
            appends_left: Some(3_000),
            push: false,
        }
    }

    /// `read` (relay.md §10.5): rows after `after`, at most `limit` and at
    /// most 1 MiB of frames, the first row always.
    fn page(&self, after: u64, limit: u16, now: i64) -> Page {
        let mut entries = Vec::new();
        let mut bytes = 0usize;
        for (i, f) in self.log.iter().enumerate().skip(after as usize) {
            if entries.len() >= usize::from(limit.min(256)) || (!entries.is_empty() && bytes + f.len() > 1_048_576) {
                break;
            }
            bytes += f.len();
            entries.push(PageEntry { seq: i as u64 + 1, at: now, frame: f.clone() });
        }
        let last = entries.last().map_or(after, |e: &PageEntry| e.seq);
        Page {
            head: self.head(),
            relay_now: now,
            floor: 1,
            purge_floor: 0,
            more: last < self.log.len() as u64,
            truncated: false,
            fenced: false,
            entries,
        }
    }

    /// Answers every relay effect the device emitted until it emits none.
    fn serve(&mut self, n: &mut Node) {
        for _ in 0..10_000 {
            let Some(fx) = n.fx.pop_front() else { return };
            match fx {
                Effect::Connect { .. } => n.feed(Event::Connected { push: false }),
                Effect::Hello => {
                    let w = self.welcome(n.wall);
                    n.feed(Event::Welcome(w));
                }
                Effect::Read { req, after, limit } => {
                    let page = self.page(after, limit, n.wall);
                    n.feed(Event::Page { req: Some(req), page });
                }
                Effect::Append { req, frames } => {
                    let results = self.append(&frames);
                    n.feed(Event::AppendResults { req, results });
                }
                Effect::ScheduleTick { .. } | Effect::Disconnect => {}
                other => panic!("this flow expects no {other:?}"),
            }
        }
        panic!("the device did not settle");
    }

    /// Serves until the device has caught up and nothing is left, with a
    /// poll (a tick 30 s later) between rounds.
    fn settle(&mut self, n: &mut Node) {
        for _ in 0..50 {
            let before = self.log.len();
            self.serve(n);
            n.tick(30_000);
            self.serve(n);
            let st = n.e.status();
            if self.log.len() == before && st.head.seq == self.log.len() as u64 && st.queued == 0 && n.fx.is_empty() {
                return;
            }
        }
        let st = n.e.status();
        panic!(
            "did not settle: relay {} head {} board {} queued {} stops {:?} halt {:?} alarms {:?}",
            self.log.len(),
            st.head.seq,
            st.board_seq,
            st.queued,
            st.stops,
            st.halt.map(|h| h.code),
            st.alarms.iter().map(|a| (a.kind, a.seq, a.code.clone())).collect::<Vec<_>>()
        );
    }
}

// ---------------------------------------------------------------- an independent reader

/// What a reader that holds `R_1` and the anchor finds in the relay's log:
/// the board rebuilt with `taskdeck_core::apply` from the accepted `cmd`
/// entries, `M_L`, and the commands with their seqs.
fn read_back(relay: &Relay, anchor: &GenesisAnchor, root1: &EpochRoot) -> (BoardData, Vec<u8>, Vec<(u64, LogCommand)>) {
    let mut m = Membership::from_canonical_bytes(&super::membership::Fields::default().encode().expect("M_0"))
        .expect("M_0 parses");
    let mut prev = seal::chain_zero(&relay.board);
    let mut board = BoardData::empty();
    let mut cmds = Vec::new();
    for (i, f) in relay.log.iter().enumerate() {
        let s = i as u64 + 1;
        let roots = |e: u32| (e == 1).then_some(root1);
        let chains = relay.chains.clone();
        let chain_at = move |b: u64| (b >= 1).then(|| chains.get(b as usize - 1).copied().map(seal::Chain)).flatten();
        let none = |_: u64| -> Option<&[u8]> { None };
        let input = seal::ReceiptInput {
            seq: s,
            frame: f,
            prev_chain: prev,
            membership: &m,
            roots: &roots,
            chain_at: &chain_at,
            retained_from: 1,
            own: None,
            supported_rules: &[1],
            genesis: (s == 1).then_some(anchor),
            skip_targets: &none,
        };
        let r = seal::classify(&input).expect("classifies");
        assert!(matches!(r.class, seal::Class::Accept), "seq {s}: {:?}", r.code);
        assert_eq!(r.chain.0, relay.chains[i], "the relay's chain value at {s}");
        let o = r.opened.expect("opened");
        if o.kind == seal::Kind::Cmd {
            let c = parse_command(&o.payload).expect("parses");
            let no = m.device_no(&seal::frame_header(f).expect("a header").dev).expect("an author");
            let at = Instant::from_at_ms(o.at_ms).expect("in range");
            let cx = capply::Ctx { seq: s, device_no: no, at, based_on: o.based_on.seq };
            capply::apply(&mut board, &c, &cx).expect("applies");
            cmds.push((s, c));
        }
        m = r.membership;
        prev = r.chain;
    }
    (board, m.to_canonical_bytes(), cmds)
}

// ---------------------------------------------------------------- the flow

/// One device from genesis to an applied edit and back after a restart.
///
/// Checks: genesis built as a creating device builds it (seal.md §12.2,
/// §13.3; device.md §18.1 step 4) and accepted at `Start` with
/// `Loaded.bootstrap = Genesis` (engine.md §19.8, ENG-BOOT-004); an edit
/// compiled against the engine's view (DEV-FACADE-001), sealed, appended,
/// read back and applied, equal to `taskdeck_core::apply` on the genesis
/// board, with `taskdeck_sync::state_hash` of the result; a restart from the
/// persisted records (ENG-STATE-002), the same state and hash, and a second
/// edit after it.
#[test]
fn genesis_edit_restart_edit() {
    flow(false);
}

/// The same flow over the store of device.md §6 and §7 (task F2): every
/// write executed on the data folder's files, local reads answered from
/// them, `Loaded` at each `Start` read back from them and equal to the
/// writes, and the restart's sealer built from `identity.tdk` and
/// `history.tdk`.
#[test]
fn genesis_edit_restart_edit_on_disk() {
    flow(true);
}

fn flow(on_disk: bool) {
    // The keys a creating device draws (device.md §18.1 step 3).
    let seed = RecoverySeed::generate(&mut OsEntropy).expect("a seed");
    let recovery = seed.keys();
    let mut salt = [0u8; 16];
    OsEntropy.fill(&mut salt).expect("entropy");
    let keys = DeviceKeys::generate(&mut OsEntropy).expect("keys");
    let me = keys.public();
    let root1 = EpochRoot::generate(&mut OsEntropy).expect("a root");
    let reader_root = EpochRoot::from_bytes(*root1.to_bytes());
    let rp = recovery.public();
    let board_id = seal::board_id(&rp.sign_pk, &salt);
    let dev_r = rp.dev(&board_id);
    let anchor = GenesisAnchor { rsign_pk: rp.sign_pk, salt, root1: EpochRoot::from_bytes(*root1.to_bytes()) };

    // Seq 1 (device.md §18.1 step 4).
    let sm = CoreMachine::new();
    let genesis_board = BoardData::empty();
    let at0: i64 = 1_700_000_000_000;
    let (sealer, frame_1) = SealSealer::create(GenesisInput {
        recovery: &recovery,
        salt,
        keys,
        root1,
        device_name: "Desk",
        at_ms: at0,
        board_hash: sm.board_hash(&genesis_board),
    })
    .expect("the genesis frame");
    let h = seal::frame_header(&frame_1).expect("a header");
    assert_eq!((h.dev, h.ctr, h.board), (dev_r, 1, board_id), "signed by the recovery identity, ctr 1");
    let identity0 = sealer.export_identity().to_vec();

    let mut relay = Relay::create(board_id.0, &frame_1, dev_r.0, me.dev.0);
    let bootstrap = Bootstrap::Genesis { board: sm.canonical_bytes(&genesis_board), frame_1: frame_1.clone(), relay: target() };
    let disk = on_disk.then(|| Disk::create(board_id.0, me.dev.0, &identity0, &bootstrap, at0));
    let mut n = Node {
        e: Engine::new(CoreMachine::new(), sealer, Config {}),
        store: Store::default(),
        wall: at0 + 1_000,
        mono: 1_000,
        pq: VecDeque::new(),
        reads: VecDeque::new(),
        fx: VecDeque::new(),
        notes: Vec::new(),
        session: 0,
        bootstrap: Some(bootstrap),
        identity0,
        board_id: board_id.0,
        draining: false,
        disk,
    };
    n.start();

    // Genesis accepted: B = 1, no stop, no halt (ENG-BOOT-004).
    let st = n.e.status();
    assert!(st.stops.is_empty() && st.halt.is_none(), "stops {:?} halt {:?}", st.stops, st.halt.map(|h| h.code));
    assert_eq!(n.e.confirmed().1.seq, 1);
    assert_eq!(n.e.confirmed().1.chain, relay.chains[0]);
    assert_eq!(n.e.device_no(), Some(1));
    assert_eq!(n.board_bytes(), sm.canonical_bytes(&genesis_board));
    assert!(n.store.ckpt.as_ref().is_some_and(|c| c.board_seq == 1), "a checkpoint at 1");
    assert!(n.store.bases.contains_key(&1), "a base at 1");
    // The Genesis item's state hash is that of the genesis board and M_1.
    let (_, m1, _) = read_back(&relay, &anchor, &reader_root);
    assert_eq!(n.e.membership_bytes(), m1);
    relay.settle(&mut n);

    // An edit.
    let mut minter = IdMinter::new(1);
    let add = Intent::Add { name: "Water the plants".into(), deadline: None, importance: None, is_event: false, time_importance: None };
    let c1 = n.submit(1, &add, &mut minter);
    relay.settle(&mut n);
    let (board, m_l, cmds) = read_back(&relay, &anchor, &reader_root);
    assert_eq!(cmds.iter().map(|(_, c)| c.clone()).collect::<Vec<_>>(), vec![c1.clone()]);
    assert_eq!(n.e.confirmed().1.seq, relay.log.len() as u64);
    assert_eq!(n.board_bytes(), sm.canonical_bytes(&board), "the confirmed state is core's apply of the log");
    assert_eq!(board.items.len(), 1);
    assert_eq!(board.items[0].name, "Water the plants");
    let hash1 = n.state_hash();
    assert_eq!(hash1, taskdeck_sync::state_hash(1, &sm.board_hash(&board), &m_l));

    // A restart from the persisted records.
    let bytes1 = n.board_bytes();
    // The restart classifies and applies a tail above the checkpoint again.
    let b = n.store.ckpt.as_ref().map_or(0, |c| c.board_seq);
    assert!(n.store.log.keys().any(|s| *s > b), "a tail above the checkpoint at {b}");
    n.restart();
    let st = n.e.status();
    assert!(st.stops.is_empty() && st.halt.is_none(), "stops {:?} halt {:?}", st.stops, st.halt.map(|h| h.code));
    assert_eq!(n.board_bytes(), bytes1);
    assert_eq!(n.state_hash(), hash1);
    relay.settle(&mut n);

    // A second edit after it.
    let id = c1.target().expect("a create's id");
    let rename = Intent::Rename { id, name: "Water the garden".into() };
    let c2 = n.submit(2, &rename, &mut minter);
    relay.settle(&mut n);
    let (board, m_l, cmds) = read_back(&relay, &anchor, &reader_root);
    assert_eq!(cmds.iter().map(|(_, c)| c.clone()).collect::<Vec<_>>(), vec![c1, c2]);
    assert_eq!(n.board_bytes(), sm.canonical_bytes(&board));
    assert_eq!(board.items[0].name, "Water the garden");
    assert_eq!(n.state_hash(), taskdeck_sync::state_hash(1, &sm.board_hash(&board), &m_l));
}
