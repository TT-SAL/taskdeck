//! The lead's scripted driver for the engine's unit tests: a durable store that
//! applies `PersistOp`s and builds `Loaded`, one device under test (`Node`), and
//! a small relay (`Hub`) that sequences frames and serves pages. It is not the
//! model relay of §31.3 (task C) and not the simulator: it only moves events.

// A kit: not every helper is used by every test file.
#![allow(dead_code)]

use std::collections::{BTreeMap, VecDeque};

use super::{Config, Engine};
use crate::fake::FakeSealer;
use crate::toy::{ToyCommand, ToyMachine};
use crate::{
    AppendResult, Body, Bootstrap, Checkpoint, CtlIntent, Dev, Effect, Event, Hash32, HashRecord, Head, Input, Kind,
    Loaded, LogRecord, Marker, Meta, NoticeOp, Notification, OutboxOp, Page, PageEntry, PersistOp, RelayTarget,
    Sealer, SnapshotMaterial, StateMachine, Via, Welcome,
};

pub(crate) type E = Engine<ToyMachine, FakeSealer>;

pub(crate) fn relay() -> RelayTarget {
    RelayTarget { host: "relay.test".into(), door: [1, 2, 3, 4, 5], home_gen: 1 }
}

/// What the runtime holds durably (§7): exactly the writes it confirmed.
#[derive(Clone, Default)]
pub(crate) struct Store {
    pub ckpt: Option<Checkpoint>,
    pub bases: BTreeMap<u64, Checkpoint>,
    pub log: BTreeMap<u64, LogRecord>,
    pub log_floor: u64,
    pub floor_chain: Option<Hash32>,
    pub hashes: Vec<HashRecord>,
    pub outbox: Vec<OutboxOp>,
    pub notices: Vec<NoticeOp>,
    pub meta: Option<Meta>,
    pub marker: Option<Marker>,
    pub identity: Option<Vec<u8>>,
    pub truncations: Vec<u64>,
    pub deleted_bases: Vec<u64>,
}

impl Store {
    pub(crate) fn apply(&mut self, op: &PersistOp) {
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
                    self.deleted_bases.push(*s);
                }
            }
            PersistOp::TruncateLogBelow(s, c) => {
                self.log_floor = *s;
                self.floor_chain = *c;
                self.log.retain(|k, _| k >= s);
                self.truncations.push(*s);
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

    pub(crate) fn loaded(&self, build: &str, session: [u8; 16], bootstrap: Option<Bootstrap>) -> Loaded {
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
            build_id: build.to_owned(),
            session,
            bootstrap: if self.ckpt.is_some() { None } else { bootstrap },
        }
    }
}

/// One device: an engine, its store, its clocks, and everything it emitted.
pub(crate) struct Node {
    pub e: E,
    pub store: Store,
    pub wall: i64,
    pub mono: u64,
    /// Persist effects not yet confirmed (only when `auto` is off).
    pub pq: VecDeque<(u64, Vec<PersistOp>)>,
    pub auto: bool,
    draining: bool,
    /// `ReadLog` (a range) and `ReadBase` (a seq) effects not yet answered.
    pub reads: VecDeque<(u64, Option<(u64, u64)>, u64)>,
    /// Effects other than `Persist` and `Notify`, in order, until a test takes them.
    pub fx: VecDeque<Effect>,
    pub notes: Vec<Notification>,
    pub persists: Vec<Vec<PersistOp>>,
    pub build: String,
    pub session: u8,
    pub bootstrap: Option<Bootstrap>,
    pub seed: u64,
    /// The identity the runtime wrote before the first `Start` (the engine writes later ones).
    pub identity0: Vec<u8>,
}

impl Node {
    pub(crate) fn feed(&mut self, event: Event<ToyCommand>) {
        let out = self.e.handle(Input { wall_ms: self.wall, mono_ms: self.mono, event });
        self.absorb(out);
    }

    pub(crate) fn absorb(&mut self, out: Vec<Effect>) {
        for fx in out {
            match fx {
                Effect::Persist { id, ops } => {
                    self.persists.push(ops.clone());
                    self.pq.push_back((id, ops));
                }
                Effect::Notify(n) => self.notes.push(n),
                Effect::ReadLog { req, from, to } => self.reads.push_back((req, Some((from, to)), 0)),
                Effect::ReadBase { req, seq } => self.reads.push_back((req, None, seq)),
                other => self.fx.push_back(other),
            }
        }
        // What the runtime owes the engine, as this driver does it:
        // spec: ENG-EFFECT-001 ENG-PERSIST-001 ENG-PERSIST-004
        // Writes are confirmed one at a time, in the order they were emitted; the other effects
        // are handed on in order without waiting for any write; a restart presents exactly the
        // writes that were confirmed.
        if self.auto && !self.draining {
            self.draining = true;
            while let Some((id, ops)) = self.pq.pop_front() {
                for op in &ops {
                    self.store.apply(op);
                }
                self.feed(Event::Persisted { id });
            }
            // Local reads see the durable store.
            while let Some((req, range, seq)) = self.reads.pop_front() {
                let result = match range {
                    Some((from, to)) => {
                        let v: Vec<LogRecord> = (from..=to).filter_map(|s| self.store.log.get(&s).cloned()).collect();
                        if v.len() as u64 == to - from + 1 {
                            crate::LocalReadResult::Records(v)
                        } else {
                            crate::LocalReadResult::Missing { code: "device/missing" }
                        }
                    }
                    None => match self.store.bases.get(&seq) {
                        Some(c) => crate::LocalReadResult::Base(c.clone()),
                        None => crate::LocalReadResult::Missing { code: "device/missing" },
                    },
                };
                self.feed(Event::LocalRead { req, result });
            }
            self.draining = false;
        }
    }

    /// Confirms the oldest unconfirmed write (manual mode).
    pub(crate) fn persist_one(&mut self) -> bool {
        let Some((id, ops)) = self.pq.pop_front() else { return false };
        for op in &ops {
            self.store.apply(op);
        }
        self.feed(Event::Persisted { id });
        true
    }

    pub(crate) fn persist_all(&mut self) {
        while self.persist_one() {}
    }

    pub(crate) fn advance(&mut self, ms: u64) {
        self.wall += ms as i64;
        self.mono += ms;
    }

    pub(crate) fn tick(&mut self, ms: u64) {
        self.advance(ms);
        self.feed(Event::Tick);
    }

    pub(crate) fn start(&mut self) {
        self.session = self.session.wrapping_add(1);
        let loaded = self.store.loaded(&self.build, [self.session; 16], self.bootstrap.clone());
        self.feed(Event::Start(loaded));
    }

    /// The process dies and a new one starts from the durable store. The fake is rebuilt from
    /// the last durable identity and its control history, with a new generator.
    pub(crate) fn restart(&mut self, reseed: u64) {
        let history = self.e.sealer().history();
        let identity = self.store.identity.clone().unwrap_or_else(|| self.identity0.clone());
        let sealer = FakeSealer::restore(&identity, history, reseed, &[1]).expect("the identity restores");
        self.e = Engine::new(ToyMachine::new(false), sealer, Config {});
        self.pq.clear();
        self.fx.clear();
        self.start();
    }

    pub(crate) fn enqueue(&mut self, ticket: u64, command: ToyCommand) {
        let at_ms = self.wall;
        self.feed(Event::Enqueue { ticket, command, at_ms, origin_state: 1u64.to_be_bytes().to_vec(), hold: false });
    }

    pub(crate) fn set(&mut self, ticket: u64, key: u8, value: i64) {
        self.enqueue(ticket, ToyCommand::Set { key, value });
    }

    pub(crate) fn dev(&self) -> Dev {
        self.e.sealer().own_dev()
    }

    pub(crate) fn board_bytes(&self) -> Vec<u8> {
        let (s, _) = self.e.confirmed();
        ToyMachine::new(false).canonical_bytes(s)
    }

    pub(crate) fn view_bytes(&self) -> Vec<u8> {
        ToyMachine::new(false).canonical_bytes(self.e.view())
    }

    pub(crate) fn take_fx(&mut self) -> Vec<Effect> {
        self.fx.drain(..).collect()
    }

    pub(crate) fn has_note(&self, f: impl Fn(&Notification) -> bool) -> bool {
        self.notes.iter().any(f)
    }
}

/// The device that creates the board (§19.8 Genesis). Returns it started, with the genesis frame.
pub(crate) fn creator(seed: u64) -> (Node, Vec<u8>) {
    let (sealer, f1, board) = FakeSealer::create(seed, "Desk");
    let identity0 = sealer.export_identity().to_vec();
    let e = Engine::new(ToyMachine::new(false), sealer, Config {});
    let mut n = Node {
        e,
        store: Store::default(),
        wall: 1_700_000_000_000,
        mono: 1_000,
        pq: VecDeque::new(),
        auto: true,
        draining: false,
        reads: VecDeque::new(),
        fx: VecDeque::new(),
        notes: Vec::new(),
        persists: Vec::new(),
        build: "build-1".into(),
        session: 0,
        bootstrap: Some(Bootstrap::Genesis { board, frame_1: f1.clone(), relay: relay() }),
        seed,
        identity0,
    };
    n.start();
    (n, f1)
}

/// A small relay: one log, counters per device, pages on demand. No signatures, no budget.
#[derive(Default)]
pub(crate) struct Hub {
    pub log: Vec<(u64, Vec<u8>)>,
    pub chains: Vec<Hash32>,
    pub ctrs: BTreeMap<Dev, (u64, u64)>,
    pub by_ctr: BTreeMap<(Dev, u64), u64>,
    pub push: bool,
}

impl Hub {
    pub(crate) fn with_genesis(n: &Node, f1: &[u8]) -> Hub {
        let mut h = Hub::default();
        h.store(n.e.sealer(), f1.to_vec());
        h
    }

    pub(crate) fn head(&self, s: &FakeSealer) -> Head {
        Head { seq: self.log.len() as u64, chain: self.chains.last().copied().unwrap_or_else(|| s.chain_zero()) }
    }

    /// Appends a frame whatever it is (also what a hostile relay would store).
    pub(crate) fn store(&mut self, s: &FakeSealer, frame: Vec<u8>) -> (u64, Hash32) {
        let seq = self.log.len() as u64 + 1;
        let prev = self.chains.last().copied().unwrap_or_else(|| s.chain_zero());
        let c = s.chain_step(&prev, seq, &frame);
        if let Ok((dev, ctr)) = s.frame_author(&frame) {
            self.ctrs.insert(dev, (ctr, seq));
            self.by_ctr.insert((dev, ctr), seq);
        }
        self.log.push((seq, frame));
        self.chains.push(c);
        (seq, c)
    }

    pub(crate) fn append(&mut self, s: &FakeSealer, frames: Vec<Vec<u8>>) -> Vec<AppendResult> {
        let mut out = Vec::new();
        for f in frames {
            let Ok((dev, ctr)) = s.frame_author(&f) else {
                out.push(AppendResult::Refused { status: 400, code: "relay/bad_frame".into() });
                continue;
            };
            let (last, last_seq) = self.ctrs.get(&dev).copied().unwrap_or((0, 0));
            if ctr == last + 1 {
                let (seq, chain) = self.store(s, f);
                out.push(AppendResult::New { seq, chain });
            } else if ctr <= last {
                let seq = self.by_ctr.get(&(dev, ctr)).copied().unwrap_or(0);
                if seq != 0 && self.log[seq as usize - 1].1 == f {
                    out.push(AppendResult::Dup { seq, chain: self.chains[seq as usize - 1] });
                } else if seq != 0 {
                    out.push(AppendResult::Conflict { seq, frame: self.log[seq as usize - 1].1.clone() });
                } else {
                    out.push(AppendResult::Conflict { seq: 0, frame: Vec::new() });
                }
            } else {
                out.push(AppendResult::Gap { last_ctr: last, last_seq });
            }
        }
        out
    }

    pub(crate) fn welcome(&self, n: &Node) -> Welcome {
        let me = self.ctrs.get(&n.dev()).copied();
        Welcome {
            head: self.head(n.e.sealer()),
            last_ctr: me.map(|x| x.0),
            last_seq: me.map(|x| x.1),
            relay_now: n.wall,
            floor: 1,
            purge_floor: 0,
            fenced: false,
            fence: None,
            appends_left: None,
            push: self.push,
        }
    }

    pub(crate) fn page(&self, n: &Node, after: u64, limit: u16) -> Page {
        let entries: Vec<PageEntry> = self
            .log
            .iter()
            .filter(|(s, _)| *s > after)
            .take(limit as usize)
            .map(|(s, f)| PageEntry { seq: *s, at: 0, frame: f.clone() })
            .collect();
        let last = entries.last().map_or(after, |e| e.seq);
        Page {
            head: self.head(n.e.sealer()),
            relay_now: n.wall,
            floor: 1,
            purge_floor: 0,
            more: last < self.log.len() as u64,
            truncated: false,
            fenced: false,
            entries,
        }
    }

    /// Answers every relay effect a node emitted, until it emits none.
    pub(crate) fn serve(&mut self, n: &mut Node) {
        for _ in 0..10_000 {
            let Some(fx) = n.fx.pop_front() else { return };
            match fx {
                Effect::Connect { .. } => {
                    let push = self.push;
                    n.feed(Event::Connected { push });
                    if push {
                        let w = self.welcome(n);
                        n.feed(Event::Welcome(w));
                    }
                }
                Effect::Hello => {
                    let w = self.welcome(n);
                    n.feed(Event::Welcome(w));
                }
                Effect::Read { req, after, limit } => {
                    let page = self.page(n, after, limit);
                    n.feed(Event::Page { req: Some(req), page });
                }
                Effect::Append { req, frames } => {
                    let results = self.append(n.e.sealer(), frames);
                    n.feed(Event::AppendResults { req, results });
                }
                _ => {}
            }
        }
        panic!("the node did not settle");
    }

    /// Serves every node until none has anything left, pushing heads by another hello round.
    pub(crate) fn settle(&mut self, nodes: &mut [&mut Node]) {
        for _ in 0..50 {
            let before = self.log.len();
            for n in nodes.iter_mut() {
                self.serve(n);
            }
            for n in nodes.iter_mut() {
                // A poll: thirty seconds later every node says hello again.
                n.tick(30_000);
                self.serve(n);
            }
            let caught = nodes.iter().all(|n| n.e.status().head.seq == self.log.len() as u64);
            if self.log.len() == before && caught && nodes.iter().all(|n| n.fx.is_empty()) {
                return;
            }
        }
        let d: Vec<String> = nodes
            .iter()
            .map(|n| {
                let st = n.e.status();
                format!(
                    "head {} board {} session {:?} queued {} stops {:?} halt {:?} alarms {:?} no {:?}",
                    st.head.seq,
                    st.board_seq,
                    st.session,
                    st.queued,
                    st.stops,
                    st.halt.map(|h| h.code),
                    st.alarms.iter().map(|a| (a.kind, a.seq, a.code.clone())).collect::<Vec<_>>(),
                    n.e.device_no()
                )
            })
            .collect();
        panic!("the nodes did not settle: hub {} {:#?}", self.log.len(), d);
    }
}

/// Pairs a second device by hand (the pairing operation of §28.4 has tests of its own): the
/// existing device appends a `DeviceAdded` with its snapshot at `h = B`, and the new one starts
/// from that snapshot (§19.8 Join).
pub(crate) fn join(hub: &mut Hub, a: &mut Node, seed: u64, name: &str) -> Node {
    hub.settle(&mut [a]);
    let (dev, sign_pk, hpke_pk) = FakeSealer::join_keys(seed);
    let (state, head) = a.e.confirmed();
    let sm = ToyMachine::new(false);
    let board = sm.canonical_bytes(state);
    let membership = a.e.membership_bytes();
    let material = SnapshotMaterial {
        seq: head.seq,
        chain: head.chain,
        hash_version: 1,
        state_hash: crate::state_hash(1, &sm.board_hash(state), &membership),
        board,
        membership,
    };
    let intent = CtlIntent::AddDevice {
        dev,
        sign_pk,
        hpke_pk,
        name: name.to_owned(),
        transcript: [7; 32],
        bootstrap_seq: head.seq,
    };
    a.e.enqueue_own(Kind::Ctl, Body::Ctl(intent), Via::Append, None).expect("an entry id");
    // The call above is not an input: its Persist effect is taken by hand, then a tick sends.
    let out = std::mem::take(&mut a.e.out);
    a.absorb(out);
    a.feed(Event::Tick);
    hub.settle(&mut [a]);
    let added_seq = hub.log.len() as u64;
    let bundle = a.e.sealer().bundle(material.clone(), added_seq);
    let sealer = FakeSealer::join(seed, name, bundle);
    let identity0 = sealer.export_identity().to_vec();
    let e = Engine::new(ToyMachine::new(false), sealer, Config {});
    let mut n = Node {
        e,
        store: Store::default(),
        wall: a.wall,
        mono: 5_000,
        pq: VecDeque::new(),
        auto: true,
        draining: false,
        reads: VecDeque::new(),
        fx: VecDeque::new(),
        notes: Vec::new(),
        persists: Vec::new(),
        build: "build-1".into(),
        session: 100,
        bootstrap: Some(Bootstrap::Join { material, added_seq, relay: relay() }),
        seed,
        identity0,
    };
    n.start();
    hub.settle(&mut [a, &mut n]);
    n
}

pub(crate) fn fx_name(f: &Effect) -> &'static str {
    match f {
        Effect::Persist { .. } => "Persist",
        Effect::ReadLog { .. } => "ReadLog",
        Effect::ReadBase { .. } => "ReadBase",
        Effect::Connect { .. } => "Connect",
        Effect::Disconnect => "Disconnect",
        Effect::Hello => "Hello",
        Effect::Read { .. } => "Read",
        Effect::Append { .. } => "Append",
        Effect::Admin { .. } => "Admin",
        Effect::Blob { .. } => "Blob",
        Effect::ScheduleTick { .. } => "ScheduleTick",
        Effect::Notify(_) => "Notify",
    }
}
