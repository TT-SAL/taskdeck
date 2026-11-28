//! A scripted sealer (the lead's test double: each frame opens to whatever
//! receipt the test planned for it) and the test that loads `engine-receipts.json`:
//! every row of §13.1 made concrete, with expected values derived from the spec
//! by task B.

// A kit: later test files use the parts this one does not.
#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::vectors_tests::cases;
use super::{Config, Engine};
use crate::toy::{ToyCommand, ToyMachine};
use crate::{
    AlarmKind, AppendResult, BeatInfo, Body, Checkpoint, Class, ClassifyInput, CtlItem, Dev, DeviceStatus,
    DeviceView, Effect, Event, Evidence, ForkSource, Halt, HaltKind, Hash32, Head, Input, Kind, Loaded, LogRecord,
    MembershipView, Meta, NoticeKind, Notification, Opened, Page, PageEntry, PersistOp, PlainEntry, Receipt,
    RelayTarget, Sealed, SealedBlob, Sealer, SealerError, SnapInfo, SnapshotMaterial, StateMachine, StopKind, Via,
    Welcome,
};

pub(crate) const ME: Dev = [0x11; 16];
pub(crate) const OTHER: Dev = [0x22; 16];

/// What a frame opens to.
#[derive(Clone)]
pub(crate) struct Plan {
    pub class: Class,
    pub code: Option<&'static str>,
    pub error: Option<&'static str>,
    pub kind: Option<Kind>,
    pub author: Dev,
    pub ctr: u64,
    pub entry_id: [u8; 16],
    pub at_ms: i64,
    pub based_on: Head,
    pub payload: Vec<u8>,
    pub items: Vec<CtlItem>,
    pub beat: Option<BeatInfo>,
    pub snap: Option<SnapInfo>,
    pub foreign: bool,
    pub unchecked: bool,
    pub halt: Option<Halt>,
    pub need: Vec<(u64, Hash32)>,
    /// An accepted entry that rotates the epoch.
    pub rotate: bool,
}

impl Plan {
    pub(crate) fn accept_cmd(author: Dev, ctr: u64, payload: Vec<u8>) -> Plan {
        Plan {
            class: Class::Accept,
            code: None,
            error: None,
            kind: Some(Kind::Cmd),
            author,
            ctr,
            entry_id: [0xEE; 16],
            at_ms: 1_700_000_000_000,
            based_on: Head { seq: 1, chain: [0; 32] },
            payload,
            items: Vec::new(),
            beat: None,
            snap: None,
            foreign: false,
            unchecked: false,
            halt: None,
            need: Vec::new(),
            rotate: false,
        }
    }
}

/// The scripted membership: just enough for the engine's questions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ScriptM {
    pub changes: u64,
    pub clock: i64,
    pub my_ctr: u64,
    pub other_ctr: u64,
    pub revoked: bool,
    pub rules: u32,
    pub epoch: u32,
    pub skips: Vec<u64>,
}

impl Default for ScriptM {
    fn default() -> Self {
        ScriptM { changes: 0, clock: 0, my_ctr: 0, other_ctr: 0, revoked: false, rules: 1, epoch: 1, skips: Vec::new() }
    }
}

#[derive(Default)]
pub(crate) struct Script {
    pub plans: Vec<(Vec<u8>, Plan)>,
    pub next_id: u8,
    pub frames: u64,
    /// What each `classify` was given: the seq, `own_sealed(ctr)`, `retained_from`, and whether
    /// `chain_at` answered for `seq - 1` and for `retained_from - 1`.
    pub probes: Vec<Probe>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Probe {
    pub seq: u64,
    pub ctr: u64,
    pub own_sealed: Option<Hash32>,
    pub retained_from: u64,
    pub chain_prev: Option<Hash32>,
    pub chain_below: Option<Hash32>,
    pub prev_chain: Hash32,
}

#[derive(Clone)]
pub(crate) struct ScriptedSealer(pub Rc<RefCell<Script>>);

fn sha(b: &[u8]) -> Hash32 {
    Sha256::digest(b).into()
}

impl ScriptedSealer {
    /// A frame of 700 bytes that names its author and counter and is otherwise unique.
    pub(crate) fn frame(&self, author: Dev, ctr: u64) -> Vec<u8> {
        let mut s = self.0.borrow_mut();
        s.frames += 1;
        let mut f = vec![0u8; 700];
        f[..16].copy_from_slice(&author);
        f[16..24].copy_from_slice(&ctr.to_be_bytes());
        f[24..32].copy_from_slice(&s.frames.to_be_bytes());
        f
    }

    pub(crate) fn plan(&self, frame: &[u8], plan: Plan) {
        let mut s = self.0.borrow_mut();
        s.plans.retain(|p| p.0 != frame);
        s.plans.push((frame.to_vec(), plan));
    }

    pub(crate) fn plan_of(&self, frame: &[u8]) -> Option<Plan> {
        self.0.borrow().plans.iter().find(|p| p.0 == frame).map(|p| p.1.clone())
    }
}

impl Sealer for ScriptedSealer {
    type Membership = ScriptM;

    fn membership_zero(&self) -> ScriptM {
        ScriptM::default()
    }
    fn own_dev(&self) -> Dev {
        ME
    }
    fn view(&self, m: &ScriptM) -> MembershipView {
        let dev = |dev: Dev, no: u16, status, ctr| DeviceView {
            dev,
            sign_pk: [0; 32],
            device_no: no,
            status,
            name: if no == 1 { "Desk".into() } else { "Laptop".into() },
            key_fp: "AAAA-BBBB".into(),
            added_seq: 1,
            added_by: ME,
            bootstrap_seq: 0,
            status_seq: 1,
            status_by: ME,
            last_ctr: ctr,
            beat_ms: 0,
            rules: vec![1],
            envelopes: vec![(1, 1)],
            hash_versions: vec![1],
        };
        let me = if m.revoked { DeviceStatus::Revoked } else { DeviceStatus::Current };
        MembershipView {
            board_id: [7; 16],
            rules: m.rules,
            epoch: m.epoch,
            clock_ms: m.clock,
            home_gen: 1,
            devices: vec![dev(ME, 1, me, m.my_ctr), dev(OTHER, 2, DeviceStatus::Current, m.other_ctr)],
            skips: m.skips.iter().map(|s| (*s, [0; 32])).collect(),
            ..Default::default()
        }
    }
    fn membership_bytes(&self, m: &ScriptM) -> Vec<u8> {
        let mut v = Vec::new();
        for x in [m.changes, m.clock as u64, m.my_ctr, m.other_ctr, m.revoked as u64, m.rules as u64, m.epoch as u64] {
            v.extend_from_slice(&x.to_be_bytes());
        }
        for s in &m.skips {
            v.extend_from_slice(&s.to_be_bytes());
        }
        v
    }
    fn membership_from_bytes(&self, b: &[u8]) -> Result<ScriptM, SealerError> {
        if b.len() < 48 || !b.len().is_multiple_of(8) {
            return Err(SealerError { code: "seal/membership_format" });
        }
        let w = |i: usize| u64::from_be_bytes(b[8 * i..8 * i + 8].try_into().unwrap());
        Ok(ScriptM {
            changes: w(0),
            clock: w(1) as i64,
            my_ctr: w(2),
            other_ctr: w(3),
            revoked: w(4) != 0,
            rules: w(5) as u32,
            epoch: w(6) as u32,
            skips: (7..b.len() / 8).map(w).collect(),
        })
    }
    fn supported_envelopes(&self) -> Vec<(u8, u8)> {
        vec![(1, 1)]
    }
    fn chain_zero(&self) -> Hash32 {
        [0; 32]
    }
    fn chain_step(&self, prev: &Hash32, seq: u64, frame: &[u8]) -> Hash32 {
        let mut h = Sha256::new();
        h.update(prev);
        h.update(seq.to_be_bytes());
        h.update(sha(frame));
        h.finalize().into()
    }
    fn frame_hash(&self, frame: &[u8]) -> Hash32 {
        sha(frame)
    }
    fn frame_author(&self, frame: &[u8]) -> Result<(Dev, u64), SealerError> {
        if frame.len() < 24 {
            return Err(SealerError { code: "seal/frame_format" });
        }
        Ok((frame[..16].try_into().unwrap(), u64::from_be_bytes(frame[16..24].try_into().unwrap())))
    }
    fn classify(&mut self, input: &ClassifyInput<'_, ScriptM>) -> Result<Receipt<ScriptM>, SealerError> {
        let plan = self.plan_of(input.frame).expect("every frame of a scripted test has a plan");
        self.0.borrow_mut().probes.push(Probe {
            seq: input.seq,
            ctr: plan.ctr,
            own_sealed: (input.own_sealed)(plan.ctr),
            retained_from: input.retained_from,
            chain_prev: (input.chain_at)(input.seq - 1),
            chain_below: input.retained_from.checked_sub(1).and_then(|b| (input.chain_at)(b)),
            prev_chain: input.prev_chain,
        });
        if let Some(code) = plan.error {
            return Err(SealerError { code });
        }
        let mut m = input.membership.clone();
        let wanted: Vec<(u64, Hash32)> =
            plan.need.iter().copied().filter(|n| (input.skip_targets)(n.0).is_none()).collect();
        let class = if wanted.is_empty() { plan.class.clone() } else { Class::NeedFrames(wanted) };
        if matches!(class, Class::Refuse | Class::Accept) && !(class == Class::Refuse && input.seq == 1) {
            m.changes += 1;
            if plan.author == ME {
                m.my_ctr = plan.ctr;
            } else {
                m.other_ctr = plan.ctr;
            }
            if class == Class::Accept {
                m.clock = m.clock.max(plan.at_ms);
                if plan.rotate {
                    m.epoch += 1;
                }
            }
        }
        let opened = plan.kind.map(|kind| Opened {
            entry_id: plan.entry_id,
            kind,
            at_ms: plan.at_ms,
            based_on: plan.based_on,
            payload: plan.payload.clone(),
            items: plan.items.clone(),
            beat: plan.beat.clone(),
            snap: plan.snap.clone(),
        });
        Ok(Receipt {
            class,
            code: plan.code,
            chain: self.chain_step(&input.prev_chain, input.seq, input.frame),
            author: plan.author,
            ctr: plan.ctr,
            frame_hash: sha(input.frame),
            own_dev_foreign: plan.foreign,
            based_on_unchecked: plan.unchecked,
            opened,
            membership: m,
            keys_changed: false,
            halt: plan.halt,
        })
    }
    fn new_entry_id(&mut self) -> Result<[u8; 16], SealerError> {
        let mut s = self.0.borrow_mut();
        s.next_id += 1;
        Ok([s.next_id; 16])
    }
    fn seal(&mut self, m: &ScriptM, ctr: u64, entry: &PlainEntry) -> Result<Sealed, SealerError> {
        let frame = self.frame(ME, ctr);
        let mut plan = Plan::accept_cmd(ME, ctr, Vec::new());
        plan.entry_id = entry.entry_id;
        plan.at_ms = entry.at_ms;
        plan.based_on = entry.based_on;
        match &entry.body {
            Body::Cmd(p) => plan.payload = p.clone(),
            Body::Ctl(_) => plan.kind = Some(Kind::Ctl),
            Body::Beat(b) => {
                plan.kind = Some(Kind::Beat);
                plan.beat = Some(b.clone());
            }
            Body::Snap(s) => {
                plan.kind = Some(Kind::Snap);
                plan.snap = Some(s.clone());
            }
        }
        self.plan(&frame, plan);
        Ok(Sealed { frame_hash: sha(&frame), frame, epoch: m.epoch, keys_changed: false })
    }
    fn open_own(&self, _m: &ScriptM, frame: &[u8]) -> Result<[u8; 16], SealerError> {
        match self.plan_of(frame) {
            Some(p) if p.author == ME => Ok(p.entry_id),
            _ => Err(SealerError { code: "seal/aead" }),
        }
    }
    fn discard_pending(&mut self, _entry_id: &[u8; 16]) -> bool {
        false
    }
    fn forget_roots_below(&mut self, _epoch: u32) -> bool {
        false
    }
    fn take_missed_wraps(&mut self, _m: &ScriptM, _first: u32, _wraps: &[[u8; 96]]) -> Result<bool, SealerError> {
        Ok(false)
    }
    fn seal_snapshot(&mut self, _m: &ScriptM, _material: &SnapshotMaterial) -> Result<SealedBlob, SealerError> {
        Err(SealerError { code: "seal/snapshot_large" })
    }
    fn open_snapshot(
        &self,
        _parts: &[&[u8]],
        _expected: &SnapInfo,
        _writer: &Dev,
        _exact: bool,
    ) -> Result<(SnapshotMaterial, u32, u64), SealerError> {
        Err(SealerError { code: "seal/blob_open" })
    }
    fn sync_fingerprint(&self, head: &Head) -> String {
        format!("{}", head.seq)
    }
    fn move_code(&self, _seq: u64, _hash: &Hash32) -> String {
        "0000000000".into()
    }
    fn check_move_code(&self, _seq: u64, _hash: &Hash32, typed: &str) -> Result<(), SealerError> {
        if typed == "0000000000" { Ok(()) } else { Err(SealerError { code: "seal/move_code" }) }
    }
    fn export_identity(&self) -> zeroize::Zeroizing<Vec<u8>> {
        zeroize::Zeroizing::new(vec![1, 2, 3])
    }
}

/// A device with the scripted sealer, started from a checkpoint at `b`.
pub(crate) struct SNode {
    pub e: Engine<ToyMachine, ScriptedSealer>,
    pub s: ScriptedSealer,
    pub wall: i64,
    pub mono: u64,
    pub pq: VecDeque<(u64, Vec<PersistOp>)>,
    pub persists: Vec<Vec<PersistOp>>,
    pub fx: VecDeque<Effect>,
    pub notes: Vec<Notification>,
    pub auto: bool,
    draining: bool,
    pub chain_b: Hash32,
    pub b: u64,
}

/// The toy board the scripted tests start from: key 5 = 10, rules 1.
pub(crate) fn board() -> Vec<u8> {
    let sm = ToyMachine::new(false);
    let mut st = sm.load(&super::vectors_tests::hex("544f5931000000010000000000000000")).unwrap();
    st.keys.insert(5, (10, 2, 1));
    sm.canonical_bytes(&st)
}

impl SNode {
    pub(crate) fn new(b: u64, m: ScriptM, tail: Vec<(Vec<u8>, Plan)>) -> SNode {
        let s = ScriptedSealer(Rc::new(RefCell::new(Script::default())));
        let e = Engine::new(ToyMachine::new(false), s.clone(), Config {});
        let chain_b = [9; 32];
        let mut n = SNode {
            e,
            s: s.clone(),
            wall: 1_700_000_100_000,
            mono: 10_000,
            pq: VecDeque::new(),
            persists: Vec::new(),
            fx: VecDeque::new(),
            notes: Vec::new(),
            auto: true,
            draining: false,
            chain_b,
            b,
        };
        let mut chains = Vec::new();
        let mut records = Vec::new();
        let mut prev = chain_b;
        for (i, (frame, plan)) in tail.into_iter().enumerate() {
            let seq = b + 1 + i as u64;
            let c = s.chain_step(&prev, seq, &frame);
            s.plan(&frame, plan);
            chains.push((seq, c, 0));
            records.push(LogRecord { seq, chain: c, tau: 0, frame });
            prev = c;
        }
        let meta = Meta {
            relay: RelayTarget { host: "relay.test".into(), door: [1; 5], home_gen: 1 },
            last_beat_ms: n.wall, // no beat during the test
            build_id: "b".into(),
            ..Default::default()
        };
        let loaded = Loaded {
            checkpoint: Some(Checkpoint {
                board_seq: b,
                board_chain: chain_b,
                board: board(),
                membership: s.membership_bytes(&m),
                log_floor: b + 1,
                registry: Default::default(),
            }),
            bases: vec![],
            chains,
            log_floor: b + 1,
            floor_chain: Some(chain_b),
            tail: records,
            hashes: vec![],
            outbox: vec![],
            notices: vec![],
            meta: Some(meta),
            marker: None,
            build_id: "b".into(),
            session: [1; 16],
            bootstrap: None,
        };
        n.feed(Event::Start(loaded));
        n
    }

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
                other => self.fx.push_back(other),
            }
        }
        if self.auto && !self.draining {
            self.draining = true;
            while let Some((id, _)) = self.pq.pop_front() {
                self.feed(Event::Persisted { id });
            }
            self.draining = false;
        }
    }

    /// Connects and makes the session `Live` with the relay's head at `head`.
    pub(crate) fn live(&mut self, head: Head) {
        self.feed(Event::Connected { push: true });
        self.feed(Event::Welcome(self.welcome(head)));
    }

    pub(crate) fn welcome(&self, head: Head) -> Welcome {
        let a = self.e.me().map_or(0, |d| d.last_ctr);
        Welcome {
            head,
            last_ctr: Some(a),
            last_seq: Some(head.seq),
            relay_now: self.wall,
            floor: 1,
            purge_floor: 0,
            fenced: false,
            fence: None,
            appends_left: None,
            push: true,
        }
    }

    /// A page of frames from `first` on, whose head is its last entry, as a push.
    pub(crate) fn page(&mut self, first: u64, frames: &[Vec<u8>]) {
        self.page_for(None, first, frames);
    }

    /// The same page as the answer to the read `req`.
    pub(crate) fn page_for(&mut self, req: Option<u64>, first: u64, frames: &[Vec<u8>]) {
        let mut chain = self.e.chain_at(first - 1).unwrap_or(self.chain_b);
        let mut entries = Vec::new();
        for (i, f) in frames.iter().enumerate() {
            let seq = first + i as u64;
            chain = self.s.chain_step(&chain, seq, f);
            entries.push(PageEntry { seq, at: 0, frame: f.clone() });
        }
        let head = Head { seq: first + frames.len() as u64 - 1, chain };
        let page = Page {
            head,
            relay_now: self.wall,
            floor: 1,
            purge_floor: 0,
            more: false,
            truncated: false,
            fenced: false,
            entries,
        };
        self.feed(Event::Page { req, page });
    }

    pub(crate) fn appended(&mut self) -> Option<(u64, Vec<Vec<u8>>)> {
        let i = self.fx.iter().position(|f| matches!(f, Effect::Append { .. }))?;
        match self.fx.remove(i) {
            Some(Effect::Append { req, frames }) => Some((req, frames)),
            _ => None,
        }
    }

    pub(crate) fn logged(&self, seq: u64) -> bool {
        self.persists.iter().flatten().any(|op| matches!(op, PersistOp::AppendLog(v) if v.iter().any(|r| r.seq == seq)))
    }

    pub(crate) fn board_bytes(&self) -> Vec<u8> {
        ToyMachine::new(false).canonical_bytes(self.e.confirmed().0)
    }
}

fn code_of(v: &Value) -> Option<&'static str> {
    v.as_str().map(|s| -> &'static str { Box::leak(s.to_owned().into_boxed_str()) })
}

/// The code of a toy refusal record: `u64 seq ‖ u16 author ‖ vb8(code) ‖ …` (§5.3).
fn refusal_code(record: &[u8]) -> String {
    let n = record[10] as usize;
    String::from_utf8(record[11..11 + n].to_vec()).unwrap()
}

// spec: ENG-CLASS-001 ENG-CLASS-002 ENG-CLASS-003 ENG-CLASS-004 ENG-CLASS-005 ENG-REF-001 ENG-REF-002 ENG-REF-003
// spec: ENG-SM-002 ENG-SM-003 ENG-GEN-008 ENG-GEN-012 ENG-RESEAL-001 ENG-RESEAL-002 ENG-HALT-001 ENG-HALT-002 ENG-HALT-005
// spec: ENG-EVID-001 ENG-EVID-002 ENG-EVID-003 ENG-UI-005 ENG-EPOCH-005 ENG-SKIP-001
#[test]
fn engine_receipts_vectors() {
    let all = cases(include_str!("../../tests/vectors/engine-receipts.json"));
    assert!(all.len() >= 55);
    for c in &all {
        let name = c["name"].as_str().unwrap();
        let r = &c["receipt"];
        let x = &c["expected"];
        let seq = r["seq"].as_u64().unwrap();
        let own = r["own"].as_bool().unwrap_or(false);
        let kind = match r["kind"].as_str() {
            Some("cmd") => Some(Kind::Cmd),
            Some("ctl") => Some(Kind::Ctl),
            Some("beat") => Some(Kind::Beat),
            Some("snap") => Some(Kind::Snap),
            _ => None,
        };
        let class = match r["class"].as_str().unwrap() {
            "Skip" => Class::Skip,
            "Tamper" => Class::Tamper,
            "Refuse" => Class::Refuse,
            "NeedFrames" => Class::Accept,
            _ => Class::Accept,
        };
        let first_apply = r["first_apply"].as_bool().unwrap_or(true);
        let b = seq - 1;
        let mut m = ScriptM { my_ctr: 3, other_ctr: 4, ..Default::default() };
        if r["in_skips"].as_bool().unwrap_or(false) {
            m.skips.push(seq);
        }
        if r["revoked_in_prev"].as_bool().unwrap_or(false) {
            m.revoked = true;
        }
        // The payload the entry opens to.
        let payload = match (r["parse"].as_str(), r["apply"].as_str()) {
            (Some("fail"), _) => vec![0xff],
            (_, Some("benign")) => vec![3, 77],
            (_, Some("refused")) => {
                if r["apply_code"].as_str() == Some("engine/toy_fail") { vec![7, 1] } else { vec![2, 99, 0, 0, 0, 0, 0, 0, 0, 1] }
            }
            (_, Some("panic")) => vec![8],
            _ => vec![1, 6, 0, 0, 0, 0, 0, 0, 0, 9],
        };
        let finish = |mut p: Plan| -> Plan {
            p.class = class.clone();
            p.code = code_of(&r["code"]);
            p.kind = kind;
            if kind == Some(Kind::Cmd) {
                p.payload = payload.clone();
            }
            if kind == Some(Kind::Beat) && p.beat.is_none() {
                p.beat = Some(BeatInfo { rules: vec![1], envelopes: vec![(1, 1)], hash_versions: vec![1], state: None, acks: vec![] });
            }
            if kind == Some(Kind::Snap) && p.snap.is_none() {
                p.snap = Some(SnapInfo { seq: 64, hash: [1; 32], hash_version: 1, blob: [2; 16], sha256: [3; 32], size: 9, parts: 1, inflated: 9, epoch: 1 });
            }
            p.foreign = r["own_dev_foreign"].as_bool().unwrap_or(false);
            p.unchecked = r["based_on_unchecked"].as_bool().unwrap_or(false);
            p.halt = match r["halt"].as_str() {
                Some("Commitment") => Some(Halt::Commitment),
                Some("UnknownRules") => Some(Halt::UnknownRules(9)),
                _ => None,
            };
            if r["class"].as_str() == Some("SealerError") {
                p.error = Some("seal/missing_root");
            }
            if r["class"].as_str() == Some("NeedFrames") {
                p.need = vec![(b.max(1), [5; 32])];
            }
            if p.halt == Some(Halt::UnknownRules(9)) {
                p.items = vec![CtlItem::RulesUpgrade { rules: 9, mode: crate::UpgradeMode::Normal }];
            }
            p
        };
        // The frame: another device's, or an own record's that the engine sealed.
        let mut n;
        let frame;
        let mut own_id = None;
        if own {
            n = SNode::new(b, m.clone(), vec![]);
            let head = Head { seq: b, chain: n.chain_b };
            n.live(head);
            match kind {
                Some(Kind::Cmd) | None => n.feed(Event::Enqueue {
                    ticket: 1,
                    command: ToyCommand::Set { key: 6, value: 9 },
                    at_ms: n.wall,
                    origin_state: vec![0; 8],
                    hold: false,
                }),
                Some(k) => {
                    let body = match k {
                        Kind::Beat => Body::Beat(BeatInfo { rules: vec![1], envelopes: vec![(1, 1)], hash_versions: vec![1], state: None, acks: vec![] }),
                        Kind::Snap => Body::Snap(SnapInfo { seq: 64, hash: [1; 32], hash_version: 1, blob: [2; 16], sha256: [3; 32], size: 9, parts: 1, inflated: 9, epoch: 1 }),
                        _ => Body::Ctl(crate::CtlIntent::Skip { targets: vec![(1, [0; 32])] }),
                    };
                    n.e.enqueue_own(k, body, Via::Append, None).unwrap();
                    let out = std::mem::take(&mut n.e.out);
                    n.absorb(out);
                    n.feed(Event::Tick);
                }
            }
            let (req, frames) = n.appended().unwrap_or_else(|| panic!("{name}: the own record was sent"));
            frame = frames[0].clone();
            let chain = n.s.chain_step(&n.chain_b, seq, &frame);
            let sealed = n.s.plan_of(&frame).unwrap();
            own_id = Some(sealed.entry_id);
            n.s.plan(&frame, finish(sealed));
            if let Some(i) = n.e.rec_index(&own_id.unwrap()) {
                n.e.outbox[i].r.reseals = r["reseals"].as_u64().unwrap_or(0) as u32;
            }
            n.feed(Event::AppendResults { req, results: vec![AppendResult::New { seq, chain }] });
        } else if !first_apply {
            // Classified again from the device's own log at a Start.
            let s0 = ScriptedSealer(Rc::new(RefCell::new(Script::default())));
            frame = s0.frame(OTHER, 5);
            n = SNode::new(b, m.clone(), vec![(frame.clone(), finish(Plan::accept_cmd(OTHER, 5, vec![])))]);
        } else {
            n = SNode::new(b, m.clone(), vec![]);
            let head = Head { seq: b, chain: n.chain_b };
            n.live(head);
            let author = if r["own_dev_foreign"].as_bool().unwrap_or(false) { ME } else { OTHER };
            frame = n.s.frame(author, 5);
            n.s.plan(&frame, finish(Plan::accept_cmd(author, 5, vec![])));
        }
        let board_before = n.board_bytes();
        let m_before = if first_apply {
            n.e.mk.as_ref().map(|m| n.s.membership_bytes(m)).unwrap()
        } else {
            n.s.membership_bytes(&m) // the tail was classified during the Start
        };
        let unchecked_before = n.e.meta.based_on_unchecked;
        if first_apply {
            n.page(seq, std::slice::from_ref(&frame));
        }
        // ---- what the engine did
        let st = n.e.status();
        // A frame read back from the device's own log at a Start was persisted by an earlier process.
        if let Some(p) = x.get("frame_persisted").and_then(Value::as_bool).filter(|_| first_apply) {
            assert_eq!(n.logged(seq), p, "{name}: frame_persisted");
        }
        if let Some(ch) = x.get("membership_changes").and_then(Value::as_bool) {
            let after = n.e.mk.as_ref().map(|m| n.s.membership_bytes(m)).unwrap();
            assert_eq!(after != m_before, ch, "{name}: membership_changes");
        }
        match x["board"].as_str() {
            Some("none") => assert_eq!(n.board_bytes(), board_before, "{name}: board none"),
            Some("applied") => {
                assert_eq!(st.board_seq, seq, "{name}: applied");
                assert!(st.halt.is_none(), "{name}");
            }
            Some("ctl_items") => assert_eq!(st.board_seq, seq, "{name}: ctl items"),
            Some("held") => assert_eq!(st.board_seq, seq - 1, "{name}: held"),
            _ => {}
        }
        match x.get("alarm") {
            Some(a) if a.is_object() => {
                let kind = match a["kind"].as_str().unwrap() {
                    "Counter" => AlarmKind::Counter,
                    "ForeignSigner" => AlarmKind::ForeignSigner,
                    "Fork" => AlarmKind::Fork,
                    "RefusedFrame" => AlarmKind::RefusedFrame,
                    _ => AlarmKind::SkippedFrame,
                };
                let al = st.alarms.iter().find(|al| al.kind == kind).unwrap_or_else(|| panic!("{name}: alarm {kind:?} in {:?}", st.alarms));
                let variant = match &al.evidence {
                    Evidence::Counter { .. } => "Counter",
                    Evidence::ForeignSigner { .. } => "ForeignSigner",
                    Evidence::Fork { .. } => "Fork",
                    Evidence::Frame { .. } => "Frame",
                    _ => "other",
                };
                assert_eq!(Some(variant), a["evidence"].as_str(), "{name}: evidence");
                if let Some(s) = a.get("seq").and_then(Value::as_u64) {
                    assert_eq!(al.seq, Some(s), "{name}: alarm seq");
                }
                if let Some(code) = a.get("code").and_then(Value::as_str) {
                    assert_eq!(al.code.as_deref(), Some(code), "{name}: alarm code");
                }
                match a.get("author").and_then(Value::as_str) {
                    Some("this_device") => assert_eq!(al.author, Some(ME), "{name}: alarm author"),
                    Some("author") => assert!(al.author.is_some(), "{name}: alarm author"),
                    _ => {}
                }
                if let (Some(want), Evidence::Frame { frame_hash }) = (a.get("frame_hash_some").and_then(Value::as_bool), &al.evidence) {
                    assert_eq!(frame_hash.is_some(), want, "{name}: frame hash");
                }
                if a.get("source").is_some() {
                    assert!(matches!(al.evidence, Evidence::Fork { source: ForkSource::BasedOn, .. }), "{name}");
                }
            }
            Some(a) if a.is_null() => {
                assert!(st.alarms.is_empty(), "{name}: no alarm, got {:?}", st.alarms);
            }
            _ => {}
        }
        match x.get("stop") {
            Some(s) if s.is_object() => {
                let ok = st.stops.iter().any(|v| match (&v.kind, s["kind"].as_str().unwrap()) {
                    (StopKind::Evidence, "Evidence") => true,
                    (StopKind::ForeignSigner, "ForeignSigner") => true,
                    (StopKind::LocalFault { code }, "LocalFault") => Some(code.as_str()) == s["code"].as_str(),
                    _ => false,
                });
                assert!(ok, "{name}: stop {s} in {:?}", st.stops);
            }
            Some(s) if s.is_null() => assert!(st.stops.is_empty(), "{name}: no stop, got {:?}", st.stops),
            _ => {}
        }
        match x.get("halt").map(|h| h.as_str()) {
            Some(Some("Rules")) => assert!(matches!(st.halt.as_ref().map(|h| &h.kind), Some(HaltKind::Rules { .. })), "{name}"),
            Some(Some("Commitment")) => assert!(matches!(st.halt.as_ref().map(|h| &h.kind), Some(HaltKind::Commitment)), "{name}"),
            // `halt` is what the receipt decides (§16.1 Rules, Commitment); a poison pill's crash
            // halt is the `poison` field's.
            Some(None) if x.get("poison").and_then(Value::as_bool) != Some(true) => {
                assert!(st.halt.is_none(), "{name}: no halt")
            }
            _ => {}
        }
        if let Some(rr) = x.get("refusal_record") {
            let recs: Vec<String> =
                n.e.notices().iter().filter(|e| e.kind == NoticeKind::Refusal).map(|e| refusal_code(&e.record)).collect();
            match rr.as_str() {
                Some(code) => assert_eq!(recs, vec![code.to_owned()], "{name}: refusal record"),
                None => assert!(recs.is_empty(), "{name}: no refusal record, got {recs:?}"),
            }
        }
        if let (Some(want), Some(id)) = (x.get("own_record").and_then(Value::as_str), own_id) {
            let rec = n.e.rec_index(&id).map(|i| &n.e.outbox[i].r);
            match want {
                "dropped" => assert!(rec.is_none(), "{name}: dropped"),
                "kept_for_reseal" => {
                    let rec = rec.unwrap_or_else(|| panic!("{name}: kept"));
                    // Cleared for re-sealing; a Live engine seals it again at once, under a new counter.
                    assert!(rec.seen.is_none() && rec.ack.is_none(), "{name}");
                    assert!(rec.frame.as_ref() != Some(&frame), "{name}: the refused frame is not kept");
                    assert_eq!(u64::from(rec.reseals), r["reseals"].as_u64().unwrap_or(0) + 1, "{name}");
                }
                "untouched" => assert!(rec.is_some_and(|r| r.frame.is_some()), "{name}: untouched"),
                _ => {}
            }
        }
        if let Some(rises) = x.get("based_on_unchecked_rises").and_then(Value::as_bool) {
            assert_eq!(n.e.meta.based_on_unchecked > unchecked_before, rises, "{name}: based_on_unchecked");
        }
        if x.get("poison").and_then(Value::as_bool) == Some(true) {
            // §15: a deterministic panic halts within one run, after its second attempt.
            assert!(matches!(st.halt.as_ref().map(|h| &h.kind), Some(HaltKind::Crash { .. })), "{name}: poison");
        }
        if x.get("need_frames").and_then(Value::as_bool) == Some(true) {
            assert!(!n.e.pipe.need.is_empty() || n.fx.iter().any(|f| matches!(f, Effect::ReadLog { .. } | Effect::Read { .. })), "{name}: need frames");
        }
        if x.get("paused").and_then(Value::as_bool) == Some(true) {
            assert!(n.e.sess.paused_root, "{name}: paused");
            assert!(!st.stops.iter().any(|s| matches!(s.kind, StopKind::LocalFault { .. })), "{name}");
        }
    }
}
