//! Hand-built logs for the simulator's own tests (task D4).
//!
//! A test of the checks must hold whatever the engine does at a seed: the
//! situations the checks of §31.7 judge (a Skip of a healthy entry, a refused
//! pairing, a pairing or a rotation only a fork's copy holds) come out of a
//! run only from what the engine under test does under faults, and every fix
//! of the engine moves them to other seeds. Here they are built by hand: the
//! creator's fake ("Desk") seals the entries, the board's recovery identity
//! seals Skips (`seal.md` §13.6: a Skip's author is D or R, and a device may
//! not skip its own frame), the model relay sequences them, and the checks
//! (`check::check`) are handed the facts a test chooses. No engine runs here
//! except the §31.7 oracle inside the checks.

use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest, Sha256};

use super::check::{self, AckFact, BranchLog, Facts, IdentFacts, Injected, MadeUp, RestoreTry};
use super::relay::{AdminOp, Cred, LogRow, ModelRelay};
use super::world::WALL0;
use crate::fake::{FakeMembership, FakeSealer};
use crate::toy::{ToyCommand, ToyMachine};
use crate::{
    AppendResult, Body, ClassifyInput, CtlIntent, Dev, DeviceStatus, Hash32, Head, Kind, PlainEntry, RelayTarget, SeedDevice, Sealer,
    StateMachine,
};

/// One board's log, built entry by entry.
pub(crate) struct HandLog {
    desk: FakeSealer,
    template: FakeSealer,
    relay: ModelRelay,
    /// The membership after the last row (as Desk classified it).
    m: FakeMembership,
    tgen: u32,
    pub(crate) board: Vec<u8>,
    pub(crate) target: RelayTarget,
    /// The `at_ms` of the next entry (one second per entry).
    at: i64,
    /// The second device Desk paired (`pair`, "Laptop"): its fake and its
    /// token's generation at the relay.
    laptop: Option<(FakeSealer, u32)>,
}

impl HandLog {
    /// A board created by Desk from `seed` (as `World::setup` creates one).
    pub(crate) fn new(seed: u64) -> Self {
        let (mut desk, f1, board) = FakeSealer::create_at(seed, "Desk", WALL0);
        desk.set_supported_rules(&check::ORACLE_RULES);
        let (rsign_pk, salt) = desk.genesis_keys().expect("genesis keys");
        let board_id = super::relay::fake_board_id(&rsign_pk, &salt);
        let mut relay = ModelRelay::new(board_id, [1; 32]);
        let dev = desk.own_dev();
        let id = desk.export_identity();
        let mut sign_pk = [0u8; 32];
        sign_pk.copy_from_slice(&id[56..88]);
        let reply = relay.create(WALL0, &rsign_pk, &salt, &dev, &sign_pk, &f1);
        assert!(reply.result.is_ok(), "the model relay creates the board");
        let tgen = reply.token_gen.unwrap_or(1);
        let mut template = desk.observer(seed ^ 0x5eed_0b5e_77e7, &[]).expect("the creator holds the anchor");
        template.set_supported_rules(&check::ORACLE_RULES);
        let target = RelayTarget { host: "relay-a.example".to_owned(), door: relay.door(), home_gen: 1 };
        let mut log = HandLog { m: desk.membership_zero(), desk, template, relay, tgen, board, target, at: WALL0 + 1_000, laptop: None };
        log.classify_last();
        log
    }

    /// Every row the relay sequenced, from seq 1.
    pub(crate) fn rows(&self) -> Vec<LogRow> {
        self.relay.log().values().cloned().collect()
    }

    pub(crate) fn head(&self) -> Head {
        self.relay.head().expect("a board")
    }

    pub(crate) fn desk_dev(&self) -> Dev {
        self.desk.own_dev()
    }

    /// Every epoch root Desk holds.
    pub(crate) fn roots(&self) -> Vec<(u32, [u8; 32])> {
        self.desk.roots()
    }

    /// Desk classifies the last row, so that `m` is the membership after it.
    fn classify_last(&mut self) {
        let rows = self.rows();
        let r = rows.last().expect("a row");
        let prev = rows.iter().rev().nth(1).map_or_else(|| self.desk.chain_zero(), |p| p.chain);
        let chain_at = |q: u64| rows.iter().find(|x| x.seq == q).map(|x| x.chain);
        let own = |_: u64| None;
        let skip = |q: u64| rows.iter().find(|x| x.seq == q).map(|x| x.frame.as_slice());
        let input = ClassifyInput {
            seq: r.seq,
            frame: &r.frame,
            prev_chain: prev,
            membership: &self.m,
            chain_at: &chain_at,
            retained_from: 0,
            own_sealed: &own,
            skip_targets: &skip,
        };
        let rc = self.desk.classify(&input).expect("Desk classifies its board's frames");
        self.m = rc.membership;
    }

    fn entry(&mut self, body: Body) -> PlainEntry {
        let entry_id = self.desk.new_entry_id().expect("an entry id");
        self.at += 1_000;
        PlainEntry { entry_id, at_ms: self.at, based_on: self.head(), body }
    }

    fn append(&mut self, cred: Cred, frame: Vec<u8>) -> u64 {
        let reply = self.relay.append(self.at, &cred, std::slice::from_ref(&frame));
        let seq = match reply.result {
            Ok(r) => match r.first() {
                Some(AppendResult::New { seq, .. }) => *seq,
                other => panic!("the relay did not sequence the frame: {other:?}"),
            },
            Err(e) => panic!("the relay refused the frame: {}", e.code),
        };
        self.classify_last();
        seq
    }

    /// Desk seals `body` and the relay sequences it: (seq, entry id).
    fn desk_seal(&mut self, body: Body) -> (u64, [u8; 16]) {
        let entry = self.entry(body);
        (self.seal_entry(&entry), entry.entry_id)
    }

    /// Desk seals `entry` with its next counter and the relay sequences it: its seq.
    fn seal_entry(&mut self, entry: &PlainEntry) -> u64 {
        let me = self.desk.own_dev();
        let ctr = self.desk.view(&self.m).devices.iter().find(|d| d.dev == me).expect("Desk is a member").last_ctr + 1;
        let sealed = self.desk.seal(&self.m, ctr, entry).expect("Desk seals");
        let cred = Cred::Token { dev: me, generation: self.tgen };
        self.append(cred, sealed.frame)
    }

    /// Desk's `DeviceAdded` of the device `FakeSealer::join_keys(seed)` draws,
    /// one entry sealed twice (two counters): (the two seqs, entry id).
    pub(crate) fn add_device_twice(&mut self, seed: u64) -> ((u64, u64), [u8; 16]) {
        let (dev, sign_pk, hpke_pk) = FakeSealer::join_keys(seed);
        let bootstrap_seq = self.head().seq;
        let entry = self.entry(Body::Ctl(CtlIntent::AddDevice { dev, sign_pk, hpke_pk, name: "Phone".to_owned(), transcript: [7; 32], bootstrap_seq }));
        let first = self.seal_entry(&entry);
        ((first, self.seal_entry(&entry)), entry.entry_id)
    }

    /// Desk's beat reporting `rules` as the rules versions it supports (`seal.md` §13.10).
    pub(crate) fn beat(&mut self, rules: &[u32]) -> (u64, [u8; 16]) {
        let info = crate::BeatInfo { rules: rules.to_vec(), envelopes: vec![(1, 1)], hash_versions: vec![1], state: None, acks: vec![] };
        self.desk_seal(Body::Beat(info))
    }

    /// Desk's rules-upgrade proposal to `rules` (`UpgradeMode::Normal`, §25.3).
    pub(crate) fn upgrade(&mut self, rules: u32) -> (u64, [u8; 16]) {
        self.desk_seal(Body::Ctl(CtlIntent::RulesUpgrade { rules, mode: crate::UpgradeMode::Normal }))
    }

    /// A toy command of Desk's (sealed without the trial apply, as the faulty build of §31.5 does).
    pub(crate) fn cmd(&mut self, c: &ToyCommand) -> (u64, [u8; 16]) {
        let payload = ToyMachine::new(true).encode(c);
        self.desk_seal(Body::Cmd(payload))
    }

    /// `Set { key, value }` of Desk's.
    pub(crate) fn set(&mut self, key: u8, value: i64) -> (u64, [u8; 16]) {
        self.cmd(&ToyCommand::Set { key, value })
    }

    /// Desk's `DeviceAdded` of the device `FakeSealer::join_keys(seed)` draws
    /// (a pairing's entry, `bootstrap_seq` the head): (seq, entry id, dev).
    pub(crate) fn add_device(&mut self, seed: u64) -> (u64, [u8; 16], Dev) {
        let (dev, sign_pk, hpke_pk) = FakeSealer::join_keys(seed);
        let bootstrap_seq = self.head().seq;
        let intent = CtlIntent::AddDevice { dev, sign_pk, hpke_pk, name: "Phone".to_owned(), transcript: [7; 32], bootstrap_seq };
        let (seq, eid) = self.desk_seal(Body::Ctl(intent));
        (seq, eid, dev)
    }

    /// Desk's epoch rotation (no revocation): (seq, entry id).
    pub(crate) fn rotate(&mut self) -> (u64, [u8; 16]) {
        self.desk_seal(Body::Ctl(CtlIntent::Rotate { revoke: vec![], reinstate: vec![], purge_before: None }))
    }

    /// Desk's `ResetTo` of the board at `seq`, whose state hash is
    /// `state_hash` (§28.1 stage 2): (seq, entry id).
    pub(crate) fn reset_to(&mut self, seq: u64, state_hash: Hash32) -> (u64, [u8; 16]) {
        self.desk_seal(Body::Ctl(CtlIntent::ResetTo { seq, state_hash, hash_version: 1 }))
    }

    /// Desk's purge rotation with `purge_before = p` (§28.2 step 2): (seq, entry id).
    pub(crate) fn purge(&mut self, p: u64) -> (u64, [u8; 16]) {
        self.desk_seal(Body::Ctl(CtlIntent::Rotate { revoke: vec![], reinstate: vec![], purge_before: Some(p) }))
    }

    /// A Skip of `target` sealed by the board's recovery identity, appended with the door: (seq, entry id).
    pub(crate) fn skip(&mut self, target: u64) -> (u64, [u8; 16]) {
        let frame = self.relay.log().get(&target).expect("the target").frame.clone();
        let fh: Hash32 = Sha256::digest(&frame).into();
        let entry = self.entry(Body::Ctl(CtlIntent::Skip { targets: vec![(target, fh)] }));
        let ctr = self.desk.view(&self.m).recovery.iter().find(|r| r.until_seq == 0).expect("a recovery identity").last_ctr + 1;
        let mut tool = self.template.observer(0x7001, &[]).expect("an observer");
        let sealed = tool.seal_as_recovery(&self.m, ctr, &entry).expect("the recovery identity seals");
        (self.append(Cred::Door, sealed.frame), entry.entry_id)
    }

    /// A frame the relay makes up: another board's genesis frame, stored at
    /// the next seq (the `injected_frame` fault of §31.6): (seq, its hash).
    pub(crate) fn inject(&mut self, seed: u64) -> (u64, Hash32) {
        let frame = FakeSealer::create_at(seed, "Other", WALL0).1;
        let fh: Hash32 = Sha256::digest(&frame).into();
        self.at += 1_000;
        let (head, _) = self.relay.inject_frame(self.at, frame).expect("a board");
        (head.seq, fh)
    }

    /// The hash of the frame at `seq`.
    pub(crate) fn frame_at(&self, seq: u64) -> Hash32 {
        self.relay.log().get(&seq).expect("a row").hash
    }

    /// Desk moves the board to a new relay at `host` (§28.3, the first case
    /// of stage 2: the relay the board is on sequences the `Fence`). Desk
    /// seals `Fence { host, door, home_gen: M.home_gen + 1 }` and sends it with
    /// the `fence` admin operation (`relay.md` §12.6), which fences that relay
    /// at the `Fence`'s seq `f`; then it seeds the new relay (`relay_key`
    /// gives its door) with every device and recovery identity of the
    /// membership and the frames from seq 1 to `f` (`first_seq = 1`, so every
    /// counter below it is 0; `last_seq = f`, §28.3 stage 3), and the log goes
    /// on there. Returns `f` and the relay it fenced, which still shows what it
    /// held (a test may make up a frame above `f` there).
    pub(crate) fn move_to(&mut self, host: &str, relay_key: [u8; 32]) -> (u64, ModelRelay) {
        let mut new = ModelRelay::new(self.relay.board_id(), relay_key);
        let door = new.door();
        let home_gen = self.desk.view(&self.m).home_gen + 1;
        let entry = self.entry(Body::Ctl(CtlIntent::Fence { host: host.to_owned(), door, home_gen }));
        let me = self.desk.own_dev();
        let ctr = self.desk.view(&self.m).devices.iter().find(|d| d.dev == me).expect("Desk is a member").last_ctr + 1;
        let sealed = self.desk.seal(&self.m, ctr, &entry).expect("Desk seals");
        let cred = Cred::Token { dev: me, generation: self.tgen };
        let op = AdminOp::Fence { host: host.to_owned(), door, home_gen, frame: Some(sealed.frame) };
        let reply = self.relay.op(self.at, &cred, &me, &op);
        let f = reply.result.unwrap_or_else(|e| panic!("the relay refused the fence: {}", e.code)).seq.expect("the Fence's seq");
        self.classify_last();
        let rows = self.rows();
        let view = self.desk.view(&self.m);
        assert_eq!(view.home_gen, home_gen, "the Fence at {f} is accepted");
        let mut devices: Vec<SeedDevice> = view
            .devices
            .iter()
            .map(|d| SeedDevice { dev: d.dev, sign_pk: d.sign_pk, role: 1, status: if d.status == DeviceStatus::Current { 1 } else { 2 }, last_ctr: 0 })
            .chain(view.recovery.iter().map(|r| SeedDevice { dev: r.dev_r, sign_pk: r.sign_pk, role: 2, status: if r.until_seq == 0 { 1 } else { 2 }, last_ctr: 0 }))
            .collect();
        devices.sort_by_key(|d| d.dev);
        let chain_last = rows.last().expect("the Fence").chain;
        let seed = AdminOp::Seed { home_gen, first_seq: 1, chain_before: self.desk.chain_zero(), last_seq: f, chain_last, blob_id: [0x5e; 16], devices };
        let reply = new.op(self.at, &Cred::Create, &me, &seed);
        assert!(reply.result.is_ok(), "the new relay takes the seed: {:?}", reply.result.err().map(|e| e.code));
        let tgen = reply.token_gen.unwrap_or(1);
        let frames: Vec<Vec<u8>> = rows.iter().map(|r| r.frame.clone()).collect();
        let reply = new.seed_tail(self.at, &Cred::Token { dev: me, generation: tgen }, 1, &frames);
        assert_eq!(reply.result.map(|ok| ok.head.map(|h| h.seq)).map_err(|e| e.code), Ok(Some(f)), "the new relay takes the seed tail");
        let same = |r: &LogRow| (r.seq, r.hash, r.chain);
        assert!(new.log().values().map(same).eq(rows.iter().map(same)), "the new relay holds the log up to the Fence");
        self.tgen = tgen;
        (f, std::mem::replace(&mut self.relay, new))
    }

    /// The membership after the last row (as Desk classified it).
    pub(crate) fn membership(&self) -> FakeMembership {
        self.m.clone()
    }

    /// The board's epoch after the last row.
    pub(crate) fn epoch(&self) -> u32 {
        self.desk.view(&self.m).epoch
    }

    /// A `snap` body for the aligned seq `s` (§19.4 step 4), its blob made
    /// up, for a frame sealed under `epoch` (`seal.md` §11.5: the two must
    /// be equal, or the entry is refused `seal/bad_payload`).
    pub(crate) fn snap_body(s: u64, epoch: u32) -> Body {
        Body::Snap(crate::SnapInfo { seq: s, hash: [0x5a; 32], hash_version: 1, blob: [0x5b; 16], sha256: [0x5c; 32], size: 100, parts: 1, inflated: 100, epoch })
    }

    /// A beat body: rules 1, no state report, no verdicts (§23.2).
    pub(crate) fn beat_body() -> Body {
        Body::Beat(crate::BeatInfo { rules: vec![1], envelopes: vec![(1, 1)], hash_versions: vec![1], state: None, acks: vec![] })
    }

    /// Desk's snapshot of the aligned seq `s`, sealed under the current epoch: (seq, entry id).
    pub(crate) fn snap(&mut self, s: u64) -> (u64, [u8; 16]) {
        let body = Self::snap_body(s, self.epoch());
        self.desk_seal(body)
    }

    /// Desk pairs a second device, "Laptop", whose keys `FakeSealer::join_keys(seed)`
    /// draws (§28.4): Desk seals its `DeviceAdded`, the relay stores it with
    /// `register` (frame mode, `relay.md` §12.3) and gives Laptop a token, and
    /// Laptop's fake joins from Desk's bundle at that seq (`FakeSealer::join`:
    /// the roots the membership commits to, epoch 1 here). Returns the
    /// entry's seq and Laptop's device id.
    pub(crate) fn pair(&mut self, seed: u64) -> (u64, Dev) {
        let (dev, sign_pk, hpke_pk) = FakeSealer::join_keys(seed);
        let bootstrap_seq = self.head().seq;
        let entry = self.entry(Body::Ctl(CtlIntent::AddDevice { dev, sign_pk, hpke_pk, name: "Laptop".to_owned(), transcript: [7; 32], bootstrap_seq }));
        let me = self.desk.own_dev();
        let ctr = self.desk.view(&self.m).devices.iter().find(|d| d.dev == me).expect("Desk is a member").last_ctr + 1;
        let sealed = self.desk.seal(&self.m, ctr, &entry).expect("Desk seals");
        let frame_hash: Hash32 = Sha256::digest(&sealed.frame).into();
        let op = AdminOp::Register { dev, sign_pk, frame_hash, frame: Some(sealed.frame), seq: 0 };
        let reply = self.relay.op(self.at, &Cred::Token { dev: me, generation: self.tgen }, &me, &op);
        let tgen = reply.token_gen.unwrap_or(1);
        let seq = reply.result.unwrap_or_else(|e| panic!("the relay refused the register: {}", e.code)).seq.expect("the DeviceAdded's seq");
        self.classify_last();
        assert!(self.desk.view(&self.m).devices.iter().any(|d| d.dev == dev), "the DeviceAdded at {seq} is accepted");
        let head = self.head();
        let membership = self.desk.membership_bytes(&self.m);
        let material = crate::SnapshotMaterial { seq: head.seq, chain: head.chain, hash_version: 1, state_hash: [0; 32], board: Vec::new(), membership };
        let mut fake = FakeSealer::join(seed, "Laptop", self.desk.bundle(material, seq));
        fake.set_supported_rules(&check::ORACLE_RULES);
        assert_eq!(fake.own_dev(), dev, "Laptop's keys are the ones Desk added");
        self.laptop = Some((fake, tgen));
        (seq, dev)
    }

    /// Laptop (`pair`) seals `body` under the membership `m` (the one after
    /// the last row, or an earlier one: a frame sealed under the epoch before
    /// a rotation), with its next counter after the last row, based on the
    /// head; the relay does not sequence it yet (`laptop_append` does, before
    /// Laptop seals anything else): (frame, entry id).
    pub(crate) fn laptop_hold(&mut self, m: &FakeMembership, body: Body) -> (Vec<u8>, [u8; 16]) {
        let entry = self.entry(body);
        let (fake, _) = self.laptop.as_mut().expect("Laptop is paired");
        let me = fake.own_dev();
        let ctr = self.desk.view(&self.m).devices.iter().find(|d| d.dev == me).expect("Laptop is a member").last_ctr + 1;
        (fake.seal(m, ctr, &entry).expect("Laptop seals").frame, entry.entry_id)
    }

    /// Desk seals `body` under the entry id `entry_id` (one another frame
    /// carries too) and the relay sequences it: its seq.
    pub(crate) fn desk_seal_as(&mut self, entry_id: [u8; 16], body: Body) -> u64 {
        let mut entry = self.entry(body);
        entry.entry_id = entry_id;
        self.seal_entry(&entry)
    }

    /// The relay sequences a frame Laptop sealed (`laptop_hold`), sent with Laptop's token: its seq.
    pub(crate) fn laptop_append(&mut self, frame: Vec<u8>) -> u64 {
        let (fake, tgen) = self.laptop.as_ref().expect("Laptop is paired");
        let cred = Cred::Token { dev: fake.own_dev(), generation: *tgen };
        self.append(cred, frame)
    }
}

/// What a test hands the checks (`check::Facts`, owned).
pub(crate) struct Case {
    pub reference: Vec<LogRow>,
    pub branches: Vec<BranchLog>,
    pub template: FakeSealer,
    pub roots: Vec<(u32, [u8; 32])>,
    pub board: Vec<u8>,
    pub relay: RelayTarget,
    pub idents: Vec<IdentFacts>,
    pub acked: Vec<([u8; 16], usize, u64, bool)>,
    pub acks: Vec<AckFact>,
    pub enqueued: Vec<([u8; 16], usize, u64, u64)>,
    pub registered: BTreeMap<[u8; 16], Dev>,
    pub entry_ops: BTreeMap<[u8; 16], (usize, u64)>,
    pub frames: BTreeMap<Hash32, ([u8; 16], Kind, bool)>,
    pub snap_seqs: BTreeMap<[u8; 16], u64>,
    pub cancelled: BTreeSet<[u8; 16]>,
    pub made_up: BTreeMap<Hash32, MadeUp>,
    pub injected: Vec<Injected>,
    pub restore_tails: Vec<RestoreTry>,
    pub owed: Vec<RestoreTry>,
    pub deletions: Vec<(usize, super::world::Deletion, u64, u64)>,
}

impl Case {
    /// The case of `log` as the reference log, with no identity.
    pub(crate) fn of(log: &HandLog) -> Self {
        Case {
            reference: log.rows(),
            branches: Vec::new(),
            template: log.template.observer(0x0b5e, &[]).expect("an observer"),
            roots: log.roots(),
            board: log.board.clone(),
            relay: log.target.clone(),
            idents: Vec::new(),
            acked: Vec::new(),
            acks: Vec::new(),
            enqueued: Vec::new(),
            registered: BTreeMap::new(),
            entry_ops: BTreeMap::new(),
            frames: BTreeMap::new(),
            snap_seqs: BTreeMap::new(),
            cancelled: BTreeSet::new(),
            made_up: BTreeMap::new(),
            injected: Vec::new(),
            restore_tails: Vec::new(),
            owed: Vec::new(),
            deletions: Vec::new(),
        }
    }

    /// The §31.7 oracle on the reference log: `B → (C_B, board hash, state hash)`.
    pub(crate) fn oracle(&self) -> check::OracleStates {
        check::oracle(&self.reference, Some(&self.template), &self.roots, &self.board, &self.relay).0
    }

    /// The six invariants on these facts.
    pub(crate) fn check(&self) -> Vec<(u8, u64, String)> {
        let f = Facts {
            reference: &self.reference,
            branches: &self.branches,
            template: Some(&self.template),
            roots: self.roots.clone(),
            board: self.board.clone(),
            relay: self.relay.clone(),
            idents: self.idents.clone(),
            acked: &self.acked,
            acks: &self.acks,
            enqueued: &self.enqueued,
            registered: &self.registered,
            entry_ops: &self.entry_ops,
            frames: &self.frames,
            snap_seqs: &self.snap_seqs,
            cancelled: &self.cancelled,
            injected: &self.injected,
            made_up: &self.made_up,
            restore_tails: &self.restore_tails,
            owed: &self.owed,
            deletions: &self.deletions,
            t_f: 86_400_000,
        };
        check::check(&f)
    }
}

/// An identity that settled (live, caught up, nothing queued) and confirmed
/// `samples`, joined at `joined_at`; invariant 1 is not asked of it.
pub(crate) fn settled(dev: Dev, joined_at: u64, samples: Vec<(u64, Hash32, Hash32, u64)>) -> IdentFacts {
    IdentFacts {
        dev,
        desc: "a hand-built identity".to_owned(),
        retired: false,
        retired_at: None,
        met_reference: 0,
        log_reach: 0,
        converges: false,
        live: true,
        halted_or_stopped_persisted: false,
        revoked: false,
        final_b: None,
        caught_up: true,
        joined_at,
        samples,
        crossings: Vec::new(),
        crash_halts: BTreeSet::new(),
        applied: Vec::new(),
        alarms: Vec::new(),
        took: Vec::new(),
        snap_duty: Vec::new(),
        evidence: Vec::new(),
        outbox: Vec::new(),
        refusal_seqs: BTreeSet::new(),
        pair_failed: Vec::new(),
        op_done: Vec::new(),
        expect_alarm: None,
        last_line: 1,
    }
}

/// The oracle's own states at `seqs` as samples (an honest device), line 1.
pub(crate) fn honest(o: &check::OracleStates, seqs: impl IntoIterator<Item = u64>) -> Vec<(u64, Hash32, Hash32, u64)> {
    seqs.into_iter()
        .map(|b| {
            let (c, bh, _) = o.get(&b).unwrap_or_else(|| panic!("the oracle confirmed {b}"));
            (b, *c, *bh, 1)
        })
        .collect()
}


#[cfg(test)]
mod tests {
    //! The checks of §31.7 on hand-built logs (ENG-SIM-002). Each test builds
    //! the situation it judges, so it holds whatever the engine does at any
    //! seed; the oracle (the engine replaying the log with no faults) is the
    //! one engine that runs, and where a test leans on what it confirms, the
    //! test states that as a premise with its own message.
    use std::collections::BTreeMap;

    use super::*;
    use crate::AlarmKind;
    use crate::sim::check::{Answer, Crossing, EvidenceAlarm};
    use crate::sim::world::{Deletion, Sel};
    use crate::toy::ToyState;

    const SEED: u64 = 41;

    /// The texts of the violations of invariant `inv`.
    fn of(v: &[(u8, u64, String)], inv: u8) -> Vec<String> {
        v.iter().filter(|x| x.0 == inv).map(|x| x.2.clone()).collect()
    }

    /// A made-up device id for a hand-built identity.
    fn dev(b: u8) -> Dev {
        [b; 16]
    }

    /// A durable alarm of `kind` raised at trace line `line` by a device that held `held`.
    fn alarm(line: u64, kind: AlarmKind, held: &[Hash32]) -> EvidenceAlarm {
        EvidenceAlarm { line, kind, held: held.iter().copied().collect() }
    }

    /// A board where the recovery identity skipped Desk's healthy entry `t`
    /// (`Set { key: 1, value: 99 }`) at `s`, and the board hash each `B` of
    /// `t..s` has with `t` left out: the oracle's of a twin log with the same
    /// commands at the same seqs and, at `t`, a `Fail`, which `apply` refuses
    /// and which so changes nothing (the toy stamps a key with the seq that
    /// set it, so a twin without `t`, its later commands one seq lower, would
    /// not do).
    struct SkipBoard {
        case: Case,
        oracle: check::OracleStates,
        t: u64,
        s: u64,
        without: BTreeMap<u64, Hash32>,
    }

    fn skip_board() -> SkipBoard {
        let mut log = HandLog::new(SEED);
        let mut twin = HandLog::new(SEED);
        for k in 0..3u8 {
            log.set(k, 10 + i64::from(k));
            twin.set(k, 10 + i64::from(k));
        }
        let (t, _) = log.set(1, 99);
        twin.cmd(&ToyCommand::Fail { code: 0 });
        for (k, v) in [(2u8, 5i64), (4, 6)] {
            log.set(k, v);
            twin.set(k, v);
        }
        let (s, _) = log.skip(t);
        log.set(3, 1);
        let case = Case::of(&log);
        let oracle = case.oracle();
        let tw = Case::of(&twin).oracle();
        let without: BTreeMap<u64, Hash32> = (t..s).map(|b| (b, tw.get(&b).expect("the twin's oracle").1)).collect();
        // The premises: the Skip is accepted; the oracle confirmed every B
        // from the target to the Skip (nothing there goes uncompared); its
        // state there is the first one it had, with `t` applied (a Skip
        // replay does not overwrite it), which differs from the state without `t`.
        let led = check::ledger(&case.reference, Some(&case.template), &case.roots);
        assert!(led.iter().any(|l| l.seq == s && l.accept && l.skip_targets == vec![t]), "the Skip of {t} is accepted at {s}");
        for b in t..s {
            let o = oracle.get(&b).unwrap_or_else(|| panic!("the oracle confirmed B = {b}"));
            assert_ne!(o.1, without[&b], "the target changes the state at {b}");
        }
        SkipBoard { case, oracle, t, s, without }
    }

    impl SkipBoard {
        fn head(&self) -> u64 {
            self.case.reference.len() as u64
        }

        /// Samples at every B past genesis: the oracle's, except in `t..s`, where they lack `t`.
        fn lacking_t(&self) -> Vec<(u64, Hash32, Hash32, u64)> {
            let mut v = honest(&self.oracle, 2..=self.head());
            for x in &mut v {
                if let Some(bh) = self.without.get(&x.0) {
                    x.2 = *bh;
                }
            }
            v
        }

        fn differs(&self, q: u64) -> String {
            format!(
                "dev 0: confirmed state at {q} differs from the oracle's on the reference log (between a Skip's target and the Skip at {})",
                self.s
            )
        }

        /// The device's board crossed `t` (from `t − 1` to `t + 1`) at trace
        /// line `line` with its class head at `k`.
        fn crossing(&self, k: u64, line: u64) -> Crossing {
            let chain = |b: u64| self.case.reference[b as usize - 1].chain;
            Crossing { from: self.t - 1, to: self.t + 1, to_chain: chain(self.t + 1), k, k_chain: chain(k), line }
        }
    }

    // A board of honest devices holds every invariant: the checks raise
    // nothing a hand-built log does not put there (the base of every test
    // here).
    #[test]
    fn an_honest_board_holds_every_invariant() {
        let b = skip_board();
        let head = b.head();
        let mut c = b.case;
        c.idents.push(settled(dev(0xd1), 1, honest(&b.oracle, 2..=head)));
        assert_eq!(c.check(), Vec::new());
    }

    // §31.7 invariant 4 as the owner amended it (task D6): between a Skip's
    // target t and the Skip at s, the state of a device that never applied t
    // is compared with the reference log replayed with t left out, which the
    // check computes with the toy from the oracle's classes, no engine
    // (`check::replay_without`). It never applied t when its board crossed t
    // with the Skip in its class head (`K ≥ s`: §12.4 step 1, §17.3 step 1; a
    // crash halt on t holds the board below t until the Skip clears it). The
    // replay agrees with an independent construction (the oracle of a twin log
    // whose t changes nothing); a state with t is a violation there; a
    // crossing whose class head is on another chain proves nothing.
    #[test]
    fn a_device_that_never_applied_the_skip_target_is_judged_by_the_log_without_it() {
        let b = skip_board();
        let lacking = b.lacking_t();
        let with_t = honest(&b.oracle, 2..=b.head());
        let never = b.crossing(b.s, 0);
        let elsewhere = Crossing { k_chain: [0xee; 32], ..never };
        let (t, s) = (b.t, b.s);
        let held: Vec<String> = (t..s).map(|q| b.differs(q)).collect();
        let mut c = b.case;
        let mut x = settled(dev(0xd2), 1, lacking);
        x.crossings.push(never);
        x.crash_halts.insert(t);
        c.idents = vec![x.clone()];
        assert_eq!(c.check(), Vec::new(), "its states lack t, as the log replayed without t");
        let mut y = x.clone();
        y.samples = with_t;
        c.idents = vec![y];
        let want: Vec<String> = (t..s)
            .map(|q| {
                format!(
                    "dev 0: confirmed state at {q} differs from the reference log replayed without [{t}], which it never applied (between a Skip's target and the Skip at {s})"
                )
            })
            .collect();
        let v = c.check();
        assert_eq!(of(&v, 4), want, "{v:?}");
        assert_eq!(v.len(), want.len(), "{v:?}");
        x.crossings = vec![elsewhere];
        c.idents = vec![x];
        let v = c.check();
        let want: Vec<String> = held.iter().map(|w| format!("{w}; it crash-halted on the target {t}")).collect();
        assert_eq!(of(&v, 4), want, "{v:?}");
        assert_eq!(v.len(), want.len(), "{v:?}");
    }

    // Invariant 4 (task D6): a device that applied the Skip's target is held
    // to the oracle's state in `t..s` like everywhere else. It applied t when
    // its board last crossed t before the state with its class head below the
    // Skip, also after an earlier crossing with the Skip in it; a device whose
    // board never crossed t (one that joined above it) is held to the oracle
    // too. A crash halt on t is named in the text, and changes nothing.
    #[test]
    fn a_device_that_applied_the_skip_target_is_held_to_the_oracle() {
        let b = skip_board();
        let lacking = b.lacking_t();
        let (applied, never) = (b.crossing(b.t + 1, 0), b.crossing(b.s, 0));
        let t = b.t;
        let want: Vec<String> = (b.t..b.s).map(|q| b.differs(q)).collect();
        let mut c = b.case;
        let mut x0 = settled(dev(0xd2), 1, lacking.clone());
        x0.crossings.push(applied);
        let x1 = settled(dev(0xd3), 1, lacking.clone());
        let mut x2 = settled(dev(0xd4), 1, lacking.clone());
        x2.crossings.extend([never, applied]);
        let mut x3 = settled(dev(0xd5), 1, lacking);
        x3.crossings.push(applied);
        x3.crash_halts.insert(t);
        c.idents = vec![x0, x1, x2, x3];
        let v = c.check();
        let mut all: Vec<String> = Vec::new();
        for i in 0..3 {
            all.extend(want.iter().map(|w| w.replacen("dev 0", &format!("dev {i}"), 1)));
        }
        all.extend(want.iter().map(|w| format!("{}; it crash-halted on the target {t}", w.replacen("dev 0", "dev 3", 1))));
        assert_eq!(of(&v, 4), all, "{v:?}");
        assert_eq!(v.len(), all.len(), "{v:?}");
    }

    // Invariant 4 (task D6), per state: a process that applied t (its board
    // crossed t with its class head below the Skip) and died with t under its
    // marker, and a next process that crash-halted on t (§15) and crossed t
    // once the Skip cleared the halt (ENG-HALT-004), give one device states
    // with t and states without it at the same B. Each is judged by the
    // device's last crossing of t before it: the first held to the oracle,
    // the second to the log without t; swapped, each is a violation.
    #[test]
    fn a_state_in_a_skip_window_is_judged_by_the_last_crossing_of_the_target_before_it() {
        let b = skip_board();
        let (t, s) = (b.t, b.s);
        let at = |v: Vec<(u64, Hash32, Hash32, u64)>, line: u64| -> Vec<(u64, Hash32, Hash32, u64)> {
            v.into_iter().filter(|x| t <= x.0 && x.0 < s).map(|x| (x.0, x.1, x.2, line)).collect()
        };
        let (with_t, without) = (honest(&b.oracle, 2..=b.head()), b.lacking_t());
        let crossings = vec![b.crossing(t + 1, 5), b.crossing(s, 15)];
        let held: Vec<String> = (t..s).map(|q| format!("{}; it crash-halted on the target {t}", b.differs(q))).collect();
        let mut c = b.case;
        let mut x = settled(dev(0xd2), 1, [at(with_t.clone(), 10), at(without.clone(), 20)].concat());
        x.crossings = crossings;
        x.crash_halts.insert(t);
        c.idents = vec![x.clone()];
        assert_eq!(c.check(), Vec::new());
        x.samples = [at(without, 10), at(with_t, 20)].concat();
        c.idents = vec![x];
        let mut want: Vec<(u8, u64, String)> = held.into_iter().map(|w| (4, 10, w)).collect();
        want.extend((t..s).map(|q| {
            (4, 20, format!(
                "dev 0: confirmed state at {q} differs from the reference log replayed without [{t}], which it never applied (between a Skip's target and the Skip at {s})"
            ))
        }));
        assert_eq!(c.check(), want);
    }

    // Invariant 4 (ruling 6): a wrong state sampled between a Skip's target
    // and the Skip is reported and names the Skip; one elsewhere is reported
    // without it.
    #[test]
    fn a_wrong_state_between_a_skip_and_its_target_breaks_tampering() {
        let b = skip_board();
        let mut samples = honest(&b.oracle, 2..=b.head());
        for x in &mut samples {
            if x.0 == b.t + 1 || x.0 == b.s + 1 {
                x.2 = [0xee; 32];
            }
        }
        let mut c = b.case;
        c.idents.push(settled(dev(0xd2), 1, samples));
        let v = c.check();
        assert_eq!(
            of(&v, 4),
            vec![
                format!("dev 0: confirmed state at {} differs from the oracle's on the reference log (between a Skip's target and the Skip at {})", b.t + 1, b.s),
                format!("dev 0: confirmed state at {} differs from the oracle's on the reference log", b.s + 1),
            ],
            "{v:?}"
        );
        assert_eq!(v.len(), 2, "{v:?}");
    }

    /// A poison board: Desk's `Panic` entries `t1` and `t2` (sealed without
    /// the trial apply, as the faulty build of §31.5 does), a `Set` after
    /// each, and the recovery identity's Skip of `t1` at `s`.
    fn poison_board() -> (Case, u64, u64, u64) {
        let mut log = HandLog::new(SEED);
        for k in 0..3u8 {
            log.set(k, 10 + i64::from(k));
        }
        let (t1, _) = log.cmd(&ToyCommand::Panic);
        log.set(1, 5);
        let (t2, _) = log.cmd(&ToyCommand::Panic);
        log.set(2, 5);
        let (s, _) = log.skip(t1);
        let c = Case::of(&log);
        let led = check::ledger(&c.reference, Some(&c.template), &c.roots);
        assert!(led.iter().any(|l| l.seq == s && l.accept && l.skip_targets == vec![t1]), "the Skip of {t1} is accepted at {s}");
        (c, t1, t2, s)
    }

    // Invariant 4 (ruling 6, the D3 check's open finding): nothing between a
    // Skip's target and the Skip goes uncompared. A device held to the oracle
    // that confirmed a B there on the reference chain where the oracle
    // confirmed nothing is a violation. The oracle confirms nothing at a B it
    // never reaches: here it halts on a second poison entry `t2` inside the
    // window of the Skip of `t1` (ENG-POISON-003: two caught panics; the Skip
    // of `t1` clears only the halt on `t1`, ENG-HALT-004).
    #[test]
    fn a_b_in_a_skip_window_the_oracle_never_confirmed_is_a_violation() {
        let (mut c, t1, t2, s) = poison_board();
        let o = c.oracle();
        assert!(!o.contains_key(&t2), "the premise: the oracle halts on the poison entry {t2} and confirms nothing there");
        let chain = c.reference[t2 as usize - 1].chain;
        let mut samples = honest(&o, 2..t1);
        samples.push((t2, chain, [0xee; 32], 1));
        c.idents.push(settled(dev(0xd2), 1, samples));
        let v = c.check();
        assert_eq!(
            of(&v, 4),
            vec![format!("dev 0: confirmed state at {t2} between a Skip's target and the Skip at {s}, where the oracle confirmed nothing")],
            "{v:?}"
        );
        assert_eq!(v.len(), 1, "{v:?}");
    }

    // Invariant 4 (ruling 6, task D6): for a device that never applied the
    // target, a B of the window that the replay without it does not reach is
    // a violation. The replay stops before a poison entry (its apply panics:
    // a crash halt, §15); it is checked against the oracle where both ran
    // alike (the oracle crash-halted on `t1` too, so it never applied it
    // either, and confirmed `t2 − 1` once the Skip cleared that halt).
    #[test]
    fn a_b_in_a_skip_window_no_replay_without_the_target_reaches_is_a_violation() {
        let (mut c, t1, t2, s) = poison_board();
        let run = check::oracle_full(&c.reference, Some(&c.template), &c.roots, &c.board, &c.relay);
        assert!(run.halts.contains(&t1) && run.halts.contains(&t2), "the premise: the oracle crash-halts on both poison entries ({:?})", run.halts);
        assert!(run.states.contains_key(&(t2 - 1)) && !run.states.contains_key(&t2), "the premise: the oracle confirmed {} and nothing at {t2}", t2 - 1);
        let chain = |q: u64| c.reference[q as usize - 1].chain;
        let o = run.states;
        let mut x = settled(dev(0xd2), 1, honest(&o, 2..t1));
        x.samples.push((t2 - 1, chain(t2 - 1), o[&(t2 - 1)].1, 1));
        x.crossings.push(Crossing { from: t1 - 1, to: t2 - 1, to_chain: chain(t2 - 1), k: s, k_chain: chain(s), line: 0 });
        c.idents = vec![x.clone()];
        assert_eq!(c.check(), Vec::new(), "its state at {} is the log replayed without {t1}", t2 - 1);
        x.samples.push((t2, chain(t2), [0xee; 32], 1));
        c.idents = vec![x];
        assert_eq!(
            c.check(),
            vec![(4, 1, format!(
                "dev 0: confirmed state at {t2} between a Skip's target and the Skip at {s}, where no replay of the reference log without [{t1}], which it never applied, reaches"
            ))]
        );
    }

    // Invariant 4 (ruling 6, task D8, gate 5175): a `ResetTo` between a
    // Skip's target and the Skip is part of the log replayed without the
    // target. Desk's poison entry `t` (a `Panic`) is followed by Desk's
    // `ResetTo` of the board at `r < t` and by the recovery identity's Skip
    // of `t`. The replay without `t` sets the board to `SM::reset(state_r,
    // state_t)` at the `ResetTo`, as a device that never applied `t` does
    // (its state at `r < t` is the oracle's, so the comparison of §28.1
    // "Every device" step 1 passes: step 3) and as a replay through it does
    // (its last paragraph); the oracle, which halted on `t` and applied the
    // `ResetTo` once the Skip cleared the halt, agrees. A
    // device that never applied `t` and confirmed the `ResetTo`'s seq with
    // that board (here the oracle's of a twin log whose `t` is a `Fail`, so
    // the same log without it) holds; one with another board there breaks
    // invariant 4.
    #[test]
    fn a_reset_in_a_skip_window_is_part_of_the_replay_without_the_target() {
        let mut log = HandLog::new(SEED);
        let mut twin = HandLog::new(SEED);
        for l in [&mut log, &mut twin] {
            for k in 0..3u8 {
                l.set(k, 10 + i64::from(k));
            }
        }
        let r = log.head().seq;
        for l in [&mut log, &mut twin] {
            l.set(1, 5);
            l.set(4, 4);
        }
        let (t, _) = log.cmd(&ToyCommand::Panic);
        twin.cmd(&ToyCommand::Fail { code: 0 });
        let h_r = Case::of(&log).oracle().get(&r).expect("the oracle confirmed r").2;
        let (q, _) = log.reset_to(r, h_r);
        twin.reset_to(r, h_r);
        let (s, _) = log.skip(t);
        log.set(2, 7);
        let mut c = Case::of(&log);
        let led = check::ledger(&c.reference, Some(&c.template), &c.roots);
        assert!(led.iter().any(|l| l.seq == q && l.accept && l.reset_to == Some(r)), "the ResetTo of {r} is accepted at {q}");
        assert!(led.iter().any(|l| l.seq == s && l.accept && l.skip_targets == vec![t]), "the Skip of {t} is accepted at {s}");
        let tw = Case::of(&twin).oracle();
        let without = tw.get(&q).expect("the twin's oracle confirmed the ResetTo").1;
        let run = check::oracle_full(&c.reference, Some(&c.template), &c.roots, &c.board, &c.relay);
        // The premises: the oracle halted on `t`; the reset changed the board
        // (it differs from the one before it); the twin's board at `q` is the
        // oracle's own there, which never applied `t` either.
        assert!(run.halts.contains(&t), "the premise: the oracle crash-halts on the poison entry {t} ({:?})", run.halts);
        assert_ne!(without, tw.get(&(q - 1)).expect("the twin's oracle confirmed t").1, "the premise: the ResetTo changes the board");
        assert_eq!(run.states.get(&q).map(|x| x.1), Some(without), "the premise: the oracle's board at {q} is the log without {t}");
        let chain = |b: u64| c.reference[b as usize - 1].chain;
        let mut x = settled(dev(0xd2), 1, honest(&run.states, 2..t));
        x.samples.push((q, chain(q), without, 1));
        x.crossings.push(Crossing { from: t - 1, to: q, to_chain: chain(q), k: s, k_chain: chain(s), line: 0 });
        c.idents = vec![x.clone()];
        assert_eq!(c.check(), Vec::new(), "its state at {q} is the log replayed without {t}");
        x.samples.last_mut().expect("the sample at the ResetTo").2 = [0xee; 32];
        c.idents = vec![x];
        assert_eq!(
            c.check(),
            vec![(4, 1, format!(
                "dev 0: confirmed state at {q} differs from the reference log replayed without [{t}], which it never applied (between a Skip's target and the Skip at {s})"
            ))]
        );
    }

    // Invariant 4 (task D8 fix pass): a `ResetTo` at `q` whose target `r`
    // lies inside a Skip's window, where the target `t` left the board at `r`
    // as it is without `t` (Desk's healthy `Set` at `t`, Desk's `Set` of the
    // same key at `r = t + 1`, a `Set` of another key, Desk's `ResetTo` of
    // `r`, the Skip of `t`). A device that never applied `t` holds at `r` the
    // board of the oracle's state there, whose hash the entry names (§18.3:
    // the same `M_r`), so the comparison of §28.1 "Every device" step 1
    // passes and it resets as a replay through `q` does without one: a single
    // state, and the log replayed without `t` goes on through `q`. A device
    // that never applied `t` and confirmed `q` with that board (the oracle's
    // of a twin log whose `t` is a `Fail` and whose `ResetTo` is the log's
    // own) holds; one with another board there breaks invariant 4.
    #[test]
    fn a_reset_to_a_seq_the_skip_target_left_unchanged_is_part_of_the_replay_without_it() {
        let mut log = HandLog::new(SEED);
        let mut twin = HandLog::new(SEED);
        for l in [&mut log, &mut twin] {
            for k in 0..3u8 {
                l.set(k, 10 + i64::from(k));
            }
        }
        let (t, _) = log.set(1, 99);
        twin.cmd(&ToyCommand::Fail { code: 0 });
        let (r, _) = log.set(1, 5);
        twin.set(1, 5);
        for l in [&mut log, &mut twin] {
            l.set(4, 4);
        }
        let h = |l: &HandLog| Case::of(l).oracle().get(&r).expect("the oracle confirmed r").2;
        let h_log = h(&log);
        assert_eq!(h_log, h(&twin), "the premise: the state hash at {r} is the same without t");
        let (q, _) = log.reset_to(r, h_log);
        twin.reset_to(r, h_log);
        let (s, _) = log.skip(t);
        let mut c = Case::of(&log);
        let led = check::ledger(&c.reference, Some(&c.template), &c.roots);
        assert!(led.iter().any(|l| l.seq == q && l.accept && l.reset_to == Some(r)), "the ResetTo of {r} is accepted at {q}");
        assert!(led.iter().any(|l| l.seq == s && l.accept && l.skip_targets == vec![t]), "the Skip of {t} is accepted at {s}");
        let run = check::oracle_full(&c.reference, Some(&c.template), &c.roots, &c.board, &c.relay);
        let tw = Case::of(&twin).oracle();
        let board = |b: u64| tw.get(&b).unwrap_or_else(|| panic!("the twin's oracle confirmed {b}")).1;
        // The premises: `t` changed the board at `t`; the twin, which never
        // applied it, compared and applied the `ResetTo`, which changed its board.
        assert!(run.states.get(&t).is_some_and(|o| o.1 != board(t)), "the premise: t changes the board at {t}");
        assert_ne!(board(q), board(q - 1), "the premise: the ResetTo changes the board at {q}");
        let exp = check::replay_without(&led, &run.boards, t - 1, s - 1, &BTreeSet::from([t]));
        let all: BTreeMap<u64, Hash32> = (t..=q).map(|b| (b, board(b))).collect();
        assert_eq!(exp, all, "the replay without {t} goes on through the ResetTo at {q}");
        let chain = |b: u64| c.reference[b as usize - 1].chain;
        let mut x = settled(dev(0xd2), 1, honest(&run.states, 2..t));
        x.samples.extend((t..=q).map(|b| (b, chain(b), board(b), 1)));
        x.crossings.push(Crossing { from: t - 1, to: t, to_chain: chain(t), k: s, k_chain: chain(s), line: 0 });
        c.idents = vec![x.clone()];
        assert_eq!(c.check(), Vec::new(), "its states in {t}..={q} are the log replayed without {t}");
        x.samples.last_mut().expect("the sample at the ResetTo").2 = [0xee; 32];
        c.idents = vec![x];
        assert_eq!(
            c.check(),
            vec![(4, 1, format!(
                "dev 0: confirmed state at {q} differs from the reference log replayed without [{t}], which it never applied (between a Skip's target and the Skip at {s})"
            ))]
        );
    }

    // Invariant 4 (task D8 fix pass): a `ResetTo` at `q` whose target `r`
    // lies inside a Skip's window, where the target `t` changed the board
    // (Desk's healthy `Set` at `t`, a `Set` at `r = t + 1`, a `Set` of t's
    // key, Desk's `ResetTo` of `r` with the state hash it holds there, with
    // `t`, and the Skip of `t`). A device that never applied `t` applies the
    // `ResetTo` in an ordinary board apply (§16.2, §17.3 step 1), not in a
    // replay, so §28.1 "Every device" step 1 compares its state hash at `r`,
    // which lacks `t`, with the entry's (ENG-RESET-002): `ResetMismatch`, the
    // entry not applied, the board as it was (the oracle of a twin log whose
    // `t` is a `Fail` and whose `ResetTo` is the log's own). A replay through
    // `q` resets without the comparison (§28.1, last paragraph), as the
    // oracle's Skip replay at `s` does (ENG-SKIP-002). The spec gives two
    // states at `q` for "the reference log replayed with `t` left out" and
    // names neither (a spec question, reported): the check takes the stricter
    // reading, no expected value from `q` on (ruling 6), so the reset board,
    // which only an engine that skips the comparison holds there (the oracle
    // of a twin whose `ResetTo` names the twin's own state hash), breaks
    // invariant 4, as the unchanged board does. Below `q` the replay holds.
    #[test]
    fn a_reset_to_a_seq_the_skip_target_changed_has_no_expected_value_in_the_window() {
        let mut log = HandLog::new(SEED);
        // The log without `t`: with the log's own `ResetTo` (a device that
        // compares, §28.1 step 1) and with one naming its own state at `r`
        // (as an engine that skips the comparison resets).
        let mut compares = HandLog::new(SEED);
        let mut skips = HandLog::new(SEED);
        for l in [&mut log, &mut compares, &mut skips] {
            for k in 0..3u8 {
                l.set(k, 10 + i64::from(k));
            }
        }
        let (t, _) = log.set(1, 99);
        let (r, _) = log.set(4, 4);
        for l in [&mut compares, &mut skips] {
            l.cmd(&ToyCommand::Fail { code: 0 });
            l.set(4, 4);
        }
        for l in [&mut log, &mut compares, &mut skips] {
            l.set(1, 5);
        }
        let h = |l: &HandLog| Case::of(l).oracle().get(&r).expect("the oracle confirmed r").2;
        let (h_log, h_twin) = (h(&log), h(&skips));
        assert_ne!(h_log, h_twin, "the premise: t changes the state hash at {r}");
        let (q, _) = log.reset_to(r, h_log);
        compares.reset_to(r, h_log);
        skips.reset_to(r, h_twin);
        let (s, _) = log.skip(t);
        let mut c = Case::of(&log);
        let led = check::ledger(&c.reference, Some(&c.template), &c.roots);
        assert!(led.iter().any(|l| l.seq == q && l.accept && l.reset_to == Some(r)), "the ResetTo of {r} is accepted at {q}");
        assert!(led.iter().any(|l| l.seq == s && l.accept && l.skip_targets == vec![t]), "the Skip of {t} is accepted at {s}");
        let run = check::oracle_full(&c.reference, Some(&c.template), &c.roots, &c.board, &c.relay);
        let (cmp, skp) = (Case::of(&compares).oracle(), Case::of(&skips).oracle());
        let board = |o: &check::OracleStates, b: u64| o.get(&b).unwrap_or_else(|| panic!("the twin's oracle confirmed {b}")).1;
        // The premises: the device that compares keeps its board at `q`; the
        // one that does not resets it; the oracle, with `t`, holds a third.
        assert_eq!(board(&cmp, q), board(&cmp, q - 1), "the premise: ResetMismatch leaves the board at {q} as it was");
        assert_ne!(board(&skp, q), board(&skp, q - 1), "the premise: the reset changes the board at {q}");
        assert!(run.states.get(&q).is_some_and(|o| o.1 != board(&cmp, q) && o.1 != board(&skp, q)), "the premise: the oracle's board at {q} has t");
        let chain = |b: u64| c.reference[b as usize - 1].chain;
        let device = |at_q: Hash32| {
            let mut x = settled(dev(0xd2), 1, honest(&run.states, 2..t));
            x.samples.extend((t..q).map(|b| (b, chain(b), board(&cmp, b), 1)));
            x.samples.push((q, chain(q), at_q, 1));
            x.crossings.push(Crossing { from: t - 1, to: t, to_chain: chain(t), k: s, k_chain: chain(s), line: 0 });
            x
        };
        let want = vec![(4, 1, format!(
            "dev 0: confirmed state at {q} between a Skip's target and the Skip at {s}, where no replay of the reference log without [{t}], which it never applied, reaches"
        ))];
        c.idents = vec![device(board(&skp, q))];
        assert_eq!(c.check(), want, "the reset board at {q}, held only by an engine that skips the comparison");
        c.idents = vec![device(board(&cmp, q))];
        assert_eq!(c.check(), want, "the board §28.1 step 1 leaves at {q}: the stricter reading");
        let exp = check::replay_without(&led, &run.boards, t - 1, s - 1, &BTreeSet::from([t]));
        let below: BTreeMap<u64, Hash32> = (t..q).map(|b| (b, board(&cmp, b))).collect();
        assert_eq!(exp, below, "the replay without {t} covers {t}..{q} and stops before the ResetTo at {q}");
    }

    // §31.7 (task D4's open item): the oracle's state at each B is the first
    // one it had there (`check::keep_first`), and a later visit does not
    // overwrite it. No run of the oracle can tell `keep_first` from an
    // overwrite on this engine: its confirmed head never comes back to a seq
    // it passed, a Skip replay included (measured in task D6 on the boards of
    // `skip_board` and `poison_board`: no head below or equal to the last
    // one), so the rule is tested where it is applied; `skip_board` asserts
    // that the oracle's states in the window are the ones with the target.
    #[test]
    fn the_oracle_keeps_its_first_state_at_each_b() {
        let mut m = BTreeMap::new();
        check::keep_first(&mut m, 5, || 1);
        check::keep_first(&mut m, 5, || 2);
        check::keep_first(&mut m, 6, || 3);
        assert_eq!(m, BTreeMap::from([(5, 1), (6, 3)]));
    }

    /// A board Desk moves `moves` times (§28.3, the first case of stage 2:
    /// the relay the board is on sequences each `Fence`), with Desk's edits
    /// before, between and after the moves: the log and the `Fence`s' seqs.
    /// With `twin`, a beat stands where each `Fence` stands (the same commands
    /// at the same seqs: the run without a move).
    fn moved_board(moves: usize, twin: bool) -> (HandLog, Vec<u64>) {
        let mut log = HandLog::new(SEED);
        for k in 0..3u8 {
            log.set(k, 10 + i64::from(k));
        }
        let mut fences = Vec::new();
        for m in 0..moves {
            let f = if twin {
                log.beat(&check::ORACLE_RULES).0
            } else {
                let host = ["relay-b.example", "relay-c.example"][m];
                log.move_to(host, [2 + m as u8; 32]).0
            };
            fences.push(f);
            for k in 0..3u8 {
                log.set(k, 20 + 10 * m as i64 + i64::from(k));
            }
        }
        (log, fences)
    }

    /// The oracle on a board Desk moved `moves` times (§31.7, task D7): its
    /// user typed each `Fence`'s move code and the engine took it (§28.3 "On
    /// every other device" item 2, ENG-MOVE-008), and it reached the final
    /// head, which a relay fenced at a `Fence` would not let it read past
    /// (§12.2, ENG-RECV-007). Its board at every B is the toy's replay of the
    /// accepted commands (no engine), on the reference chain, and the board it
    /// has on the same log with a beat in place of each `Fence` (the run
    /// without a move, where its user types nothing); an honest device that
    /// followed the moves holds every invariant.
    fn the_oracle_follows(moves: usize) {
        let (log, fences) = moved_board(moves, false);
        let mut c = Case::of(&log);
        let head = log.head().seq;
        let led = check::ledger(&c.reference, Some(&c.template), &c.roots);
        for f in &fences {
            assert!(led[*f as usize - 1].accept && *f < head, "the premise: the Fence at {f} is accepted, below the final head {head}");
        }
        let run = check::oracle_full(&c.reference, Some(&c.template), &c.roots, &c.board, &c.relay);
        let typed: Vec<(u64, Option<Result<(), &str>>)> = fences.iter().map(|f| (*f, Some(Ok(())))).collect();
        assert_eq!(run.moves, typed, "the oracle's user typed each Fence's move code once, and the engine took it");
        assert_eq!(run.states.keys().next_back(), Some(&head), "the oracle reached the final head");
        let genesis = ToyState { rules: 1, keys: BTreeMap::new(), items: BTreeMap::new(), marks: BTreeMap::new() };
        assert_eq!(run.boards.get(&1), Some(&genesis), "the premise: the board starts empty, at rules 1");
        let toy = check::replay_without(&led, &run.boards, 1, head, &BTreeSet::new());
        for b in 2..=head {
            let (chain, bh, _) = run.states.get(&b).unwrap_or_else(|| panic!("the oracle confirmed B = {b}"));
            assert_eq!(*chain, c.reference[b as usize - 1].chain, "the oracle's chain at {b} is the reference log's");
            assert_eq!(Some(bh), toy.get(&b), "the oracle's board at {b} is the toy's replay");
        }
        let (twin, beats) = moved_board(moves, true);
        assert_eq!(beats, fences, "the premise: the beats stand at the Fences' seqs");
        let tc = Case::of(&twin);
        let trun = check::oracle_full(&tc.reference, Some(&tc.template), &tc.roots, &tc.board, &tc.relay);
        assert!(trun.moves.is_empty(), "no move code is typed on a log without a Fence");
        for b in 2..=head {
            assert_eq!(trun.states.get(&b).map(|s| s.1), Some(run.states[&b].1), "the board at {b} is the one on the run without a move");
        }
        c.idents.push(settled(dev(0xd1), 1, honest(&run.states, 2..=head)));
        assert_eq!(c.check(), Vec::new());
    }

    #[test]
    fn the_oracle_follows_one_move_to_the_final_head() {
        the_oracle_follows(1);
    }

    #[test]
    fn the_oracle_follows_two_moves_to_the_final_head() {
        the_oracle_follows(2);
    }

    // §31.7 invariant 4 as amended (task D7): a frame a relay made up above a
    // `Fence` that relay sequenced (the relay that `Fence` fenced, §12.2) is
    // taken by no device (§12.3 step 5, ENG-RECV-007), so it owes no
    // `SkippedFrame`; a device that took it into its log breaks the
    // invariant, with the alarm or without. Every other made-up frame still
    // owes one: here one the new relay showed above the same `Fence`, which
    // moved the board to it (it fenced no relay that shows it). On a move
    // that ended (the old relay's log is a branch) and on one the board never
    // left (the old relay's log is the reference log, which the oracle
    // follows past the `Fence`).
    #[test]
    fn a_made_up_frame_above_a_fence_its_relay_sequenced_owes_no_alarm_and_is_taken_by_no_device() {
        let mut log = HandLog::new(SEED);
        for k in 0..3u8 {
            log.set(k, i64::from(k));
        }
        let (f, mut old) = log.move_to("relay-b.example", [2; 32]);
        let other = FakeSealer::create_at(0x07e4, "Other", WALL0).1;
        let fh: Hash32 = Sha256::digest(&other).into();
        let (above, _) = old.inject_frame(WALL0 + 90_000, other).expect("the fenced relay stores what it makes up");
        assert_eq!(above.seq, f + 1, "the premise: the made-up frame lies just above the Fence at {f}");
        log.set(4, 4);
        let (s, sh) = log.inject(0x07e5);
        log.set(5, 5);
        let mut c = Case::of(&log);
        let old_rows: Vec<LogRow> = old.log().values().cloned().collect();
        c.branches.push(BranchLog { rows: old_rows.clone(), why: "the old relay after a move", line: 30, removed_from: 1 });
        c.made_up = BTreeMap::from([(fh, MadeUp::OtherBoard), (sh, MadeUp::OtherBoard)]);
        let fenced = Injected { seq: f + 1, chain: above.chain, line: 60, frame: fh, host: "relay-a.example".to_owned() };
        let shown = Injected { seq: s, chain: c.reference[s as usize - 1].chain, line: 61, frame: sh, host: "relay-b.example".to_owned() };
        c.injected = vec![fenced.clone(), shown];
        let o = c.oracle();
        assert!(o.contains_key(&s) && o.contains_key(&log.head().seq), "the premise: the oracle passes the new relay's made-up frame at {s}");
        let mut x = settled(dev(0xd1), 1, honest(&o, o.keys().copied().filter(|b| *b >= 2)));
        x.alarms = vec![(1, AlarmKind::SkippedFrame, Some(s), 0)];
        c.idents = vec![x.clone()];
        assert_eq!(c.check(), Vec::new(), "no device took the frame above the Fence, and the new relay's raised SkippedFrame");
        let took = vec![(4, 70, format!(
            "dev 0: took the injected frame at {} into its log, above the Fence at {f} that fenced the relay showing it (ENG-RECV-007)",
            f + 1
        ))];
        c.idents[0].took = vec![(f + 1, fh, 70)];
        assert_eq!(c.check(), took, "a device took it");
        c.idents[0].alarms.push((2, AlarmKind::SkippedFrame, Some(f + 1), 0));
        assert_eq!(c.check(), took, "a device took it and raised SkippedFrame");
        x.alarms.clear();
        c.idents = vec![x];
        assert_eq!(c.check(), vec![(4, 61, format!("dev 0: the injected frame at {s} raised no SkippedFrame"))], "the new relay's made-up frame");

        // The move never ended: the old relay's log is the reference log.
        c.reference = old_rows;
        c.branches.clear();
        c.injected = vec![fenced];
        let o = c.oracle();
        assert!(o.contains_key(&(f + 1)), "the premise: the oracle follows the Fence past it");
        c.idents = vec![settled(dev(0xd1), 1, honest(&o, 2..=f))];
        assert_eq!(c.check(), Vec::new(), "no device took it");
        c.idents[0].took = vec![(f + 1, fh, 70)];
        assert_eq!(c.check(), took, "a device took it");
    }

    // §31.7 invariant 2, second outcome (task D6): an entry the reference log
    // holds once, finally refused, is an outcome when its device reported the
    // refusal after the relay sequenced it: with a refusal record at its seq,
    // with the PairFailed of its pairing (a pairing's DeviceAdded refused
    // after register, §28.4 step 4; here the second DeviceAdded of one device,
    // `seal/ctl_duplicate_key`), or with the end of its operation (a Skip
    // whose target another device skipped first, §17.1: `seal/ctl_target`). A
    // report before the relay sequenced it, of another pairing or operation,
    // or by another device is none.
    #[test]
    fn a_final_refusal_is_an_outcome_only_when_its_device_reported_it() {
        let mut log = HandLog::new(SEED);
        log.set(0, 1);
        let (_, first, added) = log.add_device(9);
        let (r, again, _) = log.add_device(9);
        let (t, _) = log.set(5, 5);
        log.skip(t);
        let (raced, raced_id) = log.skip(t);
        log.set(1, 2);
        let mut c = Case::of(&log);
        let led = check::ledger(&c.reference, Some(&c.template), &c.roots);
        for (q, what) in [(r, "the second DeviceAdded of one device"), (raced, "the second Skip of one target")] {
            let l = &led[q as usize - 1];
            assert_eq!(l.class, "refuse", "{what} is refused");
            assert!(l.skip_targets.is_empty() && l.added.is_empty(), "a refused entry carries no items");
        }
        let ack = |eid: [u8; 16], seq: u64| AckFact { eid, ident: 0, sel: Sel::Main(0), seq, frame: log.frame_at(seq), line: 5 };
        c.acks = vec![ack(again, r), ack(raced_id, raced)];
        c.acked = vec![(first, 0, 4, true), (again, 0, 6, true), (raced_id, 0, 7, true)];
        c.registered = BTreeMap::from([(first, added), (again, added)]);
        c.entry_ops = BTreeMap::from([(raced_id, (0, 33))]);
        c.idents = vec![settled(log.desk_dev(), 0, Vec::new()), settled(dev(0xd9), 0, Vec::new())];
        let tail = "and its device reported no refusal record, PairFailed of its pairing or end of its operation for it";
        let unreported = vec![
            (2, 6, format!(
                "dev 0: acknowledged entry {} is accepted 0 times in the reference log (classes there: [\"refuse\"]): a pairing's DeviceAdded refused after register (§28.4 step 4), finally refused at {r}, {tail}",
                check::hexs(&again)
            )),
            (2, 7, format!(
                "dev 0: acknowledged entry {} is accepted 0 times in the reference log (classes there: [\"refuse\"]): an operation's entry, finally refused at {raced}, {tail}",
                check::hexs(&raced_id)
            )),
        ];
        assert_eq!(c.check(), unreported);
        let reports = |c: &mut Case, who: usize, pf: Vec<(Dev, u64)>, od: Vec<(u64, u64)>| {
            for x in &mut c.idents {
                (x.pair_failed, x.op_done) = (Vec::new(), Vec::new());
            }
            (c.idents[who].pair_failed, c.idents[who].op_done) = (pf, od);
        };
        reports(&mut c, 0, vec![(added, 9)], vec![(33, 9)]);
        assert_eq!(c.check(), Vec::new(), "the PairFailed of its pairing and the end of its operation");
        reports(&mut c, 0, Vec::new(), Vec::new());
        c.idents[0].refusal_seqs = BTreeSet::from([r, raced]);
        assert_eq!(c.check(), Vec::new(), "refusal records");
        c.idents[0].refusal_seqs = BTreeSet::new();
        for (name, who, pf, od) in [
            ("before the relay sequenced it", 0, vec![(added, 4)], vec![(33, 4)]),
            ("of another pairing and operation", 0, vec![(dev(0x77), 9)], vec![(34, 9)]),
            ("by another device", 1, vec![(added, 9)], vec![(33, 9)]),
        ] {
            reports(&mut c, who, pf, od);
            assert_eq!(c.check(), unreported, "{name}");
        }
    }

    // Two forks of one board: `HandLog`s of one seed built with the same
    // first steps hold the same rows there (every value is drawn from the
    // seed), then go their own ways.
    fn forked(prefix: impl Fn(&mut HandLog)) -> (HandLog, HandLog) {
        let mut a = HandLog::new(SEED);
        let mut b = HandLog::new(SEED);
        prefix(&mut a);
        prefix(&mut b);
        assert_eq!(a.rows(), b.rows(), "one prefix");
        (a, b)
    }

    /// A relay rollback (§31.6) that dropped Desk's entry `e` at `q`: the
    /// relay before it (`lost`: a prefix, then `e`, with a frame the relay
    /// made up, another board's, just before it for `made_up` `Some(true)` or
    /// just after it for `Some(false)`), and the reference log after the
    /// settle (the prefix, then, with `overwritten`, another entry at `q`).
    /// The relay acknowledged `e` at trace line 50 (delivered at 51); the
    /// fault is at line 77; Desk is identity 0. `prefix` is the relay's head
    /// right after the rollback (`q - 1`, on the lost tail); `over` the
    /// reference log's head at `q` with `overwritten` (another device's entry
    /// there: no prefix of the lost tail).
    struct Rolled {
        case: Case,
        e: [u8; 16],
        q: u64,
        frame: Hash32,
        made_up: Option<(u64, Hash32)>,
        prefix: Head,
        over: Option<Head>,
    }

    fn rolled(overwritten: bool, made_up: Option<bool>) -> Rolled {
        let (mut after, mut lost) = forked(|l| {
            for k in 0..3u8 {
                l.set(k, i64::from(k));
            }
        });
        // Both Desks draw entry ids from one seed: the lost one draws one it never seals.
        lost.desk.new_entry_id().expect("an entry id");
        let before = (made_up == Some(true)).then(|| lost.inject(0x07e4));
        let (q, e) = lost.set(5, 5);
        let made_up = before.or_else(|| made_up.map(|_| lost.inject(0x07e4)));
        let over = overwritten.then(|| {
            after.set(6, 6);
            after.head()
        });
        let frame = lost.frame_at(q);
        let rows = lost.rows();
        let prefix = Head { seq: q - 1, chain: rows[q as usize - 2].chain };
        let mut case = Case::of(&after);
        case.branches.push(BranchLog { rows, why: check::WHY_ROLLBACK, line: 77, removed_from: q });
        case.acks.push(AckFact { eid: e, ident: 0, sel: Sel::Main(0), seq: q, frame, line: 50 });
        case.acked.push((e, 0, 51, true));
        case.idents.push(settled(after.desk_dev(), 0, Vec::new()));
        Rolled { case, e, q, frame, made_up, prefix, over }
    }

    /// A restore tail of `frames` from `first_seq` to relay 0 that met it at
    /// `head` (answered, or lost on the way) at trace line `line`, its sender's
    /// durable log `holds`.
    fn tail_try(line: u64, first_seq: u64, frames: Vec<Hash32>, answer: Answer, head: Head, holds: Option<(u64, Head)>) -> RestoreTry {
        RestoreTry { sel: Sel::Main(0), line, first_seq, frames, answer, head: Some(head), holds }
    }

    fn refused(code: &str, index: Option<u32>) -> Answer {
        Answer::Refused(code.to_owned(), index)
    }

    // §31.7 (task D6): an acknowledged entry lost with a removed relay state
    // is never lost silently when every acknowledgement of it came from such
    // a state and a device that held its frame raised an alarm of §21.1 with
    // evidence (`check::is_evidence`: the kinds whose response is the
    // evidence stop, SYNC.md §4.8). After a rollback, a dropped seq where the
    // reference log holds another entry after the settle is removed when no
    // restore tail could have brought it back: here none was sent and none
    // was owed (the device that raised the alarm lost its session before one
    // fell due; `a_seq_a_restore_tail_was_owed_for_is_not_removed` has one
    // owed). Not excused: no alarm; one raised before the fault; one of a
    // kind that is no evidence of a relay state; one of a device that did not
    // hold the frame; one more acknowledgement from a state no rollback or
    // fork removed.
    #[test]
    fn an_entry_lost_with_an_overwritten_rolled_back_seq_needs_an_alarm() {
        let Rolled { case: mut c, e, q, frame, .. } = rolled(true, None);
        assert_ne!(c.reference[q as usize - 1].hash, frame, "the premise: another entry overwrote seq {q}");
        let base = format!("dev 0: acknowledged entry {} is accepted 0 times in the reference log (classes there: [])", check::hexs(&e));
        let silent = format!(
            "{base}: lost to a removed relay state (the relay before a rollback, fault at trace line 77), and no device that held its frame raised an alarm of §21.1 with evidence after it"
        );
        assert_eq!(c.check(), vec![(2, 51, silent.clone())]);
        c.idents[0].evidence = vec![alarm(80, AlarmKind::RelayBehind, &[frame])];
        assert_eq!(c.check(), Vec::new());
        for (name, a) in [
            ("before the fault", alarm(60, AlarmKind::RelayBehind, &[frame])),
            ("no evidence of a relay state", alarm(80, AlarmKind::SkippedFrame, &[frame])),
            ("a device that did not hold the frame", alarm(80, AlarmKind::Fork, &[[0xee; 32]])),
        ] {
            c.idents[0].evidence = vec![a];
            assert_eq!(c.check(), vec![(2, 51, silent.clone())], "{name}");
        }
        c.idents[0].evidence = vec![alarm(80, AlarmKind::RelayBehind, &[frame])];
        c.acks.push(AckFact { eid: e, ident: 0, sel: Sel::Main(0), seq: q, frame, line: 90 });
        assert_eq!(c.check(), vec![(2, 51, format!("{base}: acknowledged at {q} (trace line 90) by a relay state no rollback or fork removed"))]);
    }

    // §31.7 (task D6): a dropped seq that a restore tail could have brought
    // back is not removed, even where another entry took it. Judged by the
    // relay's state when a request met it (`check::could_restore`): it held
    // a prefix of the lost tail below the seq, and the request was stored,
    // refused as a whole (a wrong `chain_before`; `relay/not_next` with the
    // relay's head below its first seq), refused at a frame the relay did
    // not make up, or lost on the way: the loss is a violation. Not when
    // another device's entry already held the relay's head (the race §21.3
    // allows), for `relay/not_next` with the relay's head at or past its
    // first seq (another device restored those seqs first, §21.3 step 3: the
    // device resumes from that head, and its next request, or the one it
    // owed from that answer if it sent none, is judged on its own; here its
    // session ended first), nor for a request before the rollback.
    #[test]
    fn an_overwritten_seq_a_restore_tail_could_have_brought_back_is_not_removed() {
        let Rolled { case: mut c, e, q, frame, prefix, over, .. } = rolled(true, None);
        let over = over.expect("another entry at q");
        c.idents[0].evidence = vec![alarm(80, AlarmKind::RelayBehind, &[frame])];
        let b = c.branches[0].rows.clone();
        let at = |s: u64| Head { seq: s, chain: b[s as usize - 1].chain };
        let holds = Some((1, at(q)));
        let mine = |line: u64, answer: Answer, head: Head| tail_try(line, q, vec![frame], answer, head, holds);
        let base = format!(
            "dev 0: acknowledged entry {} is accepted 0 times in the reference log (classes there: []): lost to a rolled-back relay state that is not removed (the relay before a rollback, fault at trace line 77): another entry took {q}, but a restore tail could have brought back {q} (trace line 85: ",
            check::hexs(&e)
        );
        for (t, what) in [
            (mine(85, Answer::Stored, prefix), "the relay stored one that carried it".to_owned()),
            (mine(85, refused("relay/chain_mismatch", None), prefix), format!("the relay at {} on that tail refused one as a whole, relay/chain_mismatch", q - 1)),
            (mine(85, refused("relay/not_next", None), at(q - 2)), format!("the relay at {} on that tail refused one as a whole, relay/not_next", q - 2)),
            (
                mine(85, refused("relay/gap", Some(0)), prefix),
                format!("the relay at {} on that tail refused one at its frame 0, relay/gap, not for a frame it made up", q - 1),
            ),
            (mine(85, Answer::Unanswered, prefix), format!("one sent while the relay was at {} on that tail never reached it", q - 1)),
        ] {
            c.restore_tails = vec![t];
            assert_eq!(c.check(), vec![(2, 51, format!("{base}{what})"))], "{what}");
        }
        for (name, t) in [
            ("refused as a whole once another entry held the head", mine(85, refused("relay/chain_mismatch", None), over)),
            ("lost the race", mine(85, refused("relay/not_next", None), over)),
            ("lost on the way once another entry held the head", mine(85, Answer::Unanswered, over)),
            (
                "the relay's head at or past its first seq",
                tail_try(85, q - 1, vec![b[q as usize - 2].hash, frame], refused("relay/not_next", None), prefix, holds),
            ),
            ("before the rollback", mine(70, refused("relay/gap", Some(0)), prefix)),
        ] {
            c.restore_tails = vec![t];
            assert_eq!(c.check(), Vec::new(), "{name}");
        }
    }

    // §31.7 (task D6; §21.3, ENG-EVID-004): a dropped seq is not left to
    // another device's entry because the device that held it sent nothing.
    // A restore tail a device owed and never sent (`Answer::Owed`: in
    // session with the relay while the relay's head was below its durable log
    // on its chain, it sent none for the response timeout, `World::owe_from`)
    // is judged by the relay's state when it fell due, as one that never
    // reached the relay: owed while the relay held a prefix of the lost tail
    // below the seq, by a device that held the seq, it could have brought the
    // seq back; so after a `relay/not_next` whose head the device's log
    // extends, when it never resumed. Not one owed before the rollback, once
    // another device's entry held the relay's head (the race), by a device
    // whose log did not run from the relay's head through the seq, or behind
    // a frame the relay made up.
    #[test]
    fn a_seq_a_restore_tail_was_owed_for_is_not_removed() {
        let Rolled { case: mut c, e, q, frame, prefix, over, .. } = rolled(true, None);
        let over = over.expect("another entry at q");
        c.idents[0].evidence = vec![alarm(80, AlarmKind::RelayBehind, &[frame])];
        let b = c.branches[0].rows.clone();
        let at = |s: u64| Head { seq: s, chain: b[s as usize - 1].chain };
        let owed = |line: u64, head: Head, holds: Option<(u64, Head)>| tail_try(line, head.seq + 1, Vec::new(), Answer::Owed, head, holds);
        let could = |line: u64, h: u64| {
            vec![(2, 51, format!(
                "dev 0: acknowledged entry {} is accepted 0 times in the reference log (classes there: []): lost to a rolled-back relay state that is not removed (the relay before a rollback, fault at trace line 77): another entry took {q}, but a restore tail could have brought back {q} (trace line {line}: its device owed one while the relay was at {h} on that tail, and sent none in time)",
                check::hexs(&e)
            ))]
        };
        assert_eq!(c.check(), Vec::new(), "the premise: none sent or owed, the seq is removed and the alarm excuses its loss");
        c.owed = vec![owed(80, prefix, Some((1, at(q))))];
        assert_eq!(c.check(), could(80, q - 1), "owed while the relay held the lost tail's prefix");
        c.owed = vec![owed(80, at(q - 2), Some((1, at(q))))];
        assert_eq!(c.check(), could(80, q - 2), "from a lower head on that tail");
        // Refused `relay/not_next` with the relay's head at its first seq,
        // and the device never resumed: it owed one from that head.
        let stale = tail_try(85, q - 1, vec![b[q as usize - 2].hash, frame], refused("relay/not_next", None), prefix, Some((1, at(q))));
        c.restore_tails = vec![stale];
        c.owed = vec![owed(86, prefix, Some((1, at(q))))];
        assert_eq!(c.check(), could(86, q - 1), "owed after a relay/not_next it did not resume from");
        c.restore_tails.clear();
        for (name, x) in [
            ("before the rollback", owed(70, prefix, Some((1, at(q))))),
            ("another device's entry held the relay's head", owed(80, over, Some((1, at(q))))),
            ("its log ended below the seq", owed(80, at(q - 2), Some((1, at(q - 1))))),
            ("its log's run started above the relay's head", owed(80, at(q - 2), Some((q, at(q))))),
            ("another log", owed(80, prefix, Some((1, Head { seq: q, chain: [0xee; 32] })))),
            ("its log not known", owed(80, prefix, None)),
        ] {
            c.owed = vec![x];
            assert_eq!(c.check(), Vec::new(), "{name}");
        }
        // Two seqs, a frame the relay made up between them: owed from below
        // both, it could have brought back the first, not the second.
        let (mut c, r) = rolled_two(true);
        let (_, xh) = r.made_up.expect("a made-up frame");
        c.made_up.insert(xh, MadeUp::OtherBoard);
        c.owed = vec![tail_try(85, r.q[0], Vec::new(), Answer::Owed, r.at(r.q[0] - 1), Some((1, r.at(r.q[1]))))];
        let what = format!("its device owed one while the relay was at {} on that tail, and sent none in time", r.q[0] - 1);
        assert_eq!(c.check(), vec![r.could(0, &what)], "not the seq behind a frame the relay made up");
    }

    /// A relay rollback that dropped two entries of Desk's, `e[0]` at `q[0]`
    /// and `e[1]` at `q[1]` (right after it, or behind a frame the relay made
    /// up between them, another board's, for `made_up`); the reference log
    /// after the settle (the case `rolled_two` returns with it) holds other
    /// entries at both seqs. The relay acknowledged them at trace lines 50 and
    /// 52 (delivered at 51 and 53); the fault is at line 77; Desk (identity
    /// 0), which held both frames, raised `RelayBehind` at line 80. `rows` is
    /// the relay before the fault.
    struct Rolled2 {
        e: [[u8; 16]; 2],
        q: [u64; 2],
        frame: [Hash32; 2],
        made_up: Option<(u64, Hash32)>,
        rows: Vec<LogRow>,
    }

    impl Rolled2 {
        /// The lost tail's head at `s`.
        fn at(&self, s: u64) -> Head {
            Head { seq: s, chain: self.rows[s as usize - 1].chain }
        }

        /// The violation of `e[i]` when the restore tail at line 85 that `what` names could have brought it back.
        fn could(&self, i: usize, what: &str) -> (u8, u64, String) {
            (2, 51 + 2 * i as u64, format!(
                "dev 0: acknowledged entry {} is accepted 0 times in the reference log (classes there: []): lost to a rolled-back relay state that is not removed (the relay before a rollback, fault at trace line 77): another entry took {}, but a restore tail could have brought back {} (trace line 85: {what})",
                check::hexs(&self.e[i]),
                self.q[i],
                self.q[i]
            ))
        }
    }

    fn rolled_two(made_up: bool) -> (Case, Rolled2) {
        let (mut after, mut lost) = forked(|l| {
            for k in 0..3u8 {
                l.set(k, i64::from(k));
            }
        });
        lost.desk.new_entry_id().expect("an entry id");
        let (q0, e0) = lost.set(5, 5);
        let made_up = made_up.then(|| lost.inject(0x07e4));
        let (q1, e1) = lost.set(6, 6);
        for k in 0..3 {
            after.set(7, k);
        }
        assert!(after.head().seq >= q1, "the premise: the reference log holds other entries at {q0} and {q1}");
        let frame = [lost.frame_at(q0), lost.frame_at(q1)];
        let rows = lost.rows();
        let mut case = Case::of(&after);
        case.branches.push(BranchLog { rows: rows.clone(), why: check::WHY_ROLLBACK, line: 77, removed_from: q0 });
        for (i, (e, q)) in [(e0, q0), (e1, q1)].into_iter().enumerate() {
            case.acks.push(AckFact { eid: e, ident: 0, sel: Sel::Main(0), seq: q, frame: frame[i], line: 50 + 2 * i as u64 });
            case.acked.push((e, 0, 51 + 2 * i as u64, true));
        }
        let mut desk = settled(after.desk_dev(), 0, Vec::new());
        desk.evidence = vec![alarm(80, AlarmKind::RelayBehind, &frame)];
        case.idents.push(desk);
        (case, Rolled2 { e: [e0, e1], q: [q0, q1], frame, made_up, rows })
    }

    // §31.7 (task D6): a restore tail is sent request by request, each after
    // the repairs the one before it needs (§21.3 steps 1 and 2). A request the
    // relay refused at a frame it did not make up (a missing repair:
    // `relay/unknown_author`) while it held the lost tail's prefix leaves
    // every later seq its device held to that restore tail: such a seq is not
    // removed though no request carried it. Not a seq its device did not hold
    // (its durable log ended below it, is another log, or is not known; its
    // unbroken run started above the relay's head), nor one behind a frame
    // the relay made up, nor one a request met another device's entry at the
    // relay's head for (the race §21.3 allows).
    #[test]
    fn a_seq_behind_a_refused_restore_tail_could_have_been_brought_back() {
        let (mut c, r) = rolled_two(false);
        let (q, frame) = (r.q, r.frame);
        let stuck = |holds: Option<(u64, Head)>| tail_try(85, q[0], vec![frame[0]], refused("relay/unknown_author", Some(0)), r.at(q[0] - 1), holds);
        let what = format!("the relay at {} on that tail refused one at its frame 0, relay/unknown_author, not for a frame it made up", q[0] - 1);
        c.restore_tails = vec![stuck(Some((1, r.at(q[1]))))];
        assert_eq!(c.check(), vec![r.could(0, &what), r.could(1, &what)]);
        for (name, holds) in
            [("its log ended below it", Some((1, r.at(q[0])))), ("another log", Some((1, Head { seq: q[1], chain: [0xee; 32] }))), ("not known", None)]
        {
            c.restore_tails = vec![stuck(holds)];
            assert_eq!(c.check(), vec![r.could(0, &what)], "{name}");
        }
        // Refused `relay/not_next` with the relay's head two seqs lower: the
        // device must hold the seq between.
        let below = |lo: u64| tail_try(85, q[0], vec![frame[0]], refused("relay/not_next", None), r.at(q[0] - 2), Some((lo, r.at(q[1]))));
        c.restore_tails = vec![below(1)];
        let what = format!("the relay at {} on that tail refused one as a whole, relay/not_next", q[0] - 2);
        assert_eq!(c.check(), vec![r.could(0, &what), r.could(1, &what)]);
        c.restore_tails = vec![below(q[0])];
        assert_eq!(c.check(), Vec::new(), "its log's run started above the relay's head");
        // Another device's entry already held the relay's head at `q[0]`: the
        // race §21.3 allows, the relay held no prefix of the lost tail.
        let raced = Head { seq: q[0], chain: c.reference[q[0] as usize - 1].chain };
        assert_ne!(raced, r.at(q[0]), "the premise: another entry at {}", q[0]);
        c.restore_tails = vec![tail_try(85, q[1], vec![frame[1]], refused("relay/chain_mismatch", None), raced, Some((1, r.at(q[1]))))];
        assert_eq!(c.check(), Vec::new(), "the relay's head was another device's entry");
        // Behind a frame the relay made up.
        let (mut c, r) = rolled_two(true);
        let (x, xh) = r.made_up.expect("a made-up frame");
        assert_eq!((r.q[0] + 1, r.q[0] + 2), (x, r.q[1]), "the made-up frame lies between");
        c.made_up.insert(xh, MadeUp::OtherBoard);
        c.restore_tails = vec![tail_try(85, r.q[0], vec![r.frame[0]], Answer::Unanswered, r.at(r.q[0] - 1), Some((1, r.at(r.q[1]))))];
        let what = format!("one sent while the relay was at {} on that tail never reached it", r.q[0] - 1);
        assert_eq!(c.check(), vec![r.could(0, &what)]);
    }

    // §31.7 (task D6): a plain rollback is repaired, not excused. A dropped
    // seq the reference log holds nothing at after the settle is removed only
    // when every restore tail that would have brought it back was refused for
    // a frame the relay made up (`relay.md` §12.9): one that carried it was
    // refused at such a frame. Otherwise a loss there breaks invariant 2 even
    // with an alarm: no restore tail; one before the rollback; one refused at
    // a made-up frame above it that did not carry it; one a restore tail
    // could have brought back (refused at its own frame, refused as a whole,
    // stored by another request).
    #[test]
    fn a_plain_rollback_no_restore_tail_brought_back_is_not_excused() {
        let Rolled { case: mut c, e, q, frame, made_up, prefix, .. } = rolled(false, Some(false));
        let (x, xh) = made_up.expect("a made-up frame");
        assert!(c.reference.len() < q as usize, "the premise: the reference log holds nothing at {q}");
        assert_eq!(x, q + 1, "the made-up frame follows {q}");
        c.made_up.insert(xh, MadeUp::OtherBoard);
        c.idents[0].evidence = vec![alarm(80, AlarmKind::RelayBehind, &[frame])];
        let holds = Some((1, Head { seq: x, chain: c.branches[0].rows[x as usize - 1].chain }));
        let lost = format!(
            "dev 0: acknowledged entry {} is accepted 0 times in the reference log (classes there: []): lost to a rolled-back relay state that is not removed (the relay before a rollback, fault at trace line 77): the reference log holds nothing at {q}, and ",
            check::hexs(&e)
        );
        let repaired = format!(
            "dev 0: acknowledged entry {} is accepted 0 times in the reference log (classes there: []): lost to a rolled-back relay state that is not removed (the relay before a rollback, fault at trace line 77): the reference log holds nothing at {q}, nor another entry at a lower seq the rollback dropped, and no restore tail that would have brought it back was refused for a frame the relay made up",
            check::hexs(&e)
        );
        assert_eq!(c.check(), vec![(2, 51, repaired.clone())], "no restore tail");
        let tail = |line: u64, answer: Answer| tail_try(line, q, vec![frame, xh], answer, prefix, holds);
        c.restore_tails = vec![tail(85, refused("relay/wrong_board", Some(1)))];
        assert_eq!(c.check(), Vec::new(), "refused for the frame the relay made up");
        c.restore_tails.push(tail(86, refused("relay/not_next", None)));
        assert_eq!(c.check(), Vec::new(), "no other request brings back a seq a made-up frame blocked");
        let could = |line: u64, what: &str| format!("{lost}a restore tail could have brought back {q} (trace line {line}: {what})");
        for (name, tails, want) in [
            ("before the rollback", vec![tail(70, refused("relay/wrong_board", Some(1)))], repaired.clone()),
            (
                "refused at a made-up frame above it, without it",
                vec![tail_try(85, x, vec![xh], refused("relay/wrong_board", Some(0)), prefix, holds)],
                repaired.clone(),
            ),
            (
                "refused at its own frame",
                vec![tail(85, refused("relay/gap", Some(0)))],
                could(85, &format!("the relay at {} on that tail refused one at its frame 0, relay/gap, not for a frame it made up", q - 1)),
            ),
            (
                "refused as a whole",
                vec![tail(85, refused("relay/chain_mismatch", None))],
                could(85, &format!("the relay at {} on that tail refused one as a whole, relay/chain_mismatch", q - 1)),
            ),
            (
                "one stored it",
                vec![tail(85, refused("relay/wrong_board", Some(1))), tail_try(86, q, vec![frame], Answer::Stored, prefix, holds)],
                format!("{lost}a restore tail could have brought back {q} (trace line 86: the relay stored one that carried it)"),
            ),
        ] {
            c.restore_tails = tails;
            assert_eq!(c.check(), vec![(2, 51, want)], "{name}");
        }
        c.restore_tails = vec![tail(85, refused("relay/wrong_board", Some(1)))];
        c.made_up.clear();
        assert_eq!(
            c.check(),
            vec![(2, 51, could(85, &format!("the relay at {} on that tail refused one at its frame 1, relay/wrong_board, not for a frame it made up", q - 1)))],
            "a frame the relay did not make up"
        );
    }

    // §31.7 (task D6): "refused for a frame the relay made up" is a refusal at
    // that frame with the code the relay gives what it made up (`relay.md`
    // §12.9 runs the checks of §9.3 on each frame): `relay/wrong_board` for
    // another board's frame (P2, the code brief D6 names), and, for the other
    // kind §31.6 makes up, a frame with a bad signature, `relay/bad_signature`
    // (P7). Any other code at that frame refused the request for something
    // else, and the seq stays one a restore tail could have brought back.
    #[test]
    fn a_refusal_is_for_a_made_up_frame_only_under_the_code_the_relay_gives_it() {
        let Rolled { case: mut c, e, q, frame, made_up, prefix, .. } = rolled(false, Some(false));
        let (x, xh) = made_up.expect("a made-up frame");
        c.idents[0].evidence = vec![alarm(80, AlarmKind::RelayBehind, &[frame])];
        let holds = Some((1, Head { seq: x, chain: c.branches[0].rows[x as usize - 1].chain }));
        let at_xh = |code: &str| tail_try(85, q, vec![frame, xh], refused(code, Some(1)), prefix, holds);
        let could = |code: &str| {
            vec![(2, 51, format!(
                "dev 0: acknowledged entry {} is accepted 0 times in the reference log (classes there: []): lost to a rolled-back relay state that is not removed (the relay before a rollback, fault at trace line 77): the reference log holds nothing at {q}, and a restore tail could have brought back {q} (trace line 85: the relay at {} on that tail refused one at its frame 1, {code}, not for a frame it made up)",
                check::hexs(&e),
                q - 1
            ))]
        };
        for (how, code) in [(MadeUp::OtherBoard, "relay/wrong_board"), (MadeUp::BadSignature, "relay/bad_signature")] {
            c.made_up = BTreeMap::from([(xh, how)]);
            c.restore_tails = vec![at_xh(code)];
            assert_eq!(c.check(), Vec::new(), "{how:?} refused {code}");
            for other in ["relay/wrong_board", "relay/bad_signature", "relay/unknown_author", "relay/frame_mismatch"] {
                if other != code {
                    c.restore_tails = vec![at_xh(other)];
                    assert_eq!(c.check(), could(other), "{how:?} refused {other}");
                }
            }
        }
    }

    // §31.7 (task D6): a restore tail restores seqs in order, each request
    // after the one before it succeeded (§21.3 step 1). One refused at a
    // frame the relay made up below a dropped seq brings that seq back from
    // no request: the seq is removed though no restore tail carried it, and
    // its loss is excused by an alarm with evidence; so for one lost on the
    // way below that frame; not when a later restore tail stored it.
    #[test]
    fn a_dropped_seq_a_made_up_frame_below_it_blocked_is_removed() {
        let Rolled { case: mut c, e, q, frame, made_up, .. } = rolled(false, Some(true));
        let (x, xh) = made_up.expect("a made-up frame");
        assert_eq!(x + 1, q, "the made-up frame precedes {q}");
        c.made_up.insert(xh, MadeUp::OtherBoard);
        c.idents[0].evidence = vec![alarm(80, AlarmKind::RelayBehind, &[frame])];
        let b = c.branches[0].rows.clone();
        let at = |s: u64| Head { seq: s, chain: b[s as usize - 1].chain };
        let holds = Some((1, at(q)));
        let blocked = tail_try(85, x, vec![xh], refused("relay/wrong_board", Some(0)), at(x - 1), holds);
        c.restore_tails = vec![blocked.clone()];
        assert_eq!(c.check(), Vec::new());
        c.restore_tails.push(tail_try(86, x, vec![xh, frame], Answer::Unanswered, at(x - 1), holds));
        assert_eq!(c.check(), Vec::new(), "lost on the way below the made-up frame");
        c.restore_tails.push(tail_try(90, q, vec![frame], Answer::Stored, at(x), holds));
        assert_eq!(
            c.check(),
            vec![(2, 51, format!(
                "dev 0: acknowledged entry {} is accepted 0 times in the reference log (classes there: []): lost to a rolled-back relay state that is not removed (the relay before a rollback, fault at trace line 77): the reference log holds nothing at {q}, and a restore tail could have brought back {q} (trace line 90: the relay stored one that carried it)",
                check::hexs(&e)
            ))]
        );
    }

    /// A relay rollback (§31.6) that dropped Desk's entries at `q1` and at
    /// `q2 = q1 + 1` (the lost tail), and the reference log after the settle:
    /// the prefix to `q1 − 1`, then at `q1` another device's entry (`over`
    /// `Some(true)`), Desk's dropped frame again (`Some(false)`), or nothing
    /// (`None`), so that `q2` lies above its final head. The relay
    /// acknowledged Desk's entry `e` at `q2` at trace line 50 (delivered at
    /// 51); the fault is at line 77; Desk (identity 0) raised an alarm with
    /// evidence holding its frame at line 80. Returns the case, `e`, `q1`,
    /// `q2` and the lost tail.
    fn rolled_above_head(over: Option<bool>) -> (Case, [u8; 16], u64, u64, Vec<LogRow>) {
        let (mut after, mut lost) = forked(|l| {
            for k in 0..3u8 {
                l.set(k, i64::from(k));
            }
        });
        // Both Desks draw entry ids from one seed: the lost one draws one it never seals.
        lost.desk.new_entry_id().expect("an entry id");
        let (q1, _) = lost.set(5, 5);
        let (q2, e) = lost.set(7, 7);
        let rows = lost.rows();
        if over == Some(true) {
            after.set(6, 6);
        }
        let mut case = Case::of(&after);
        if over == Some(false) {
            case.reference = rows[..q1 as usize].to_vec();
        }
        let frame = lost.frame_at(q2);
        case.branches.push(BranchLog { rows: rows.clone(), why: check::WHY_ROLLBACK, line: 77, removed_from: q1 });
        case.acks.push(AckFact { eid: e, ident: 0, sel: Sel::Main(0), seq: q2, frame, line: 50 });
        case.acked.push((e, 0, 51, true));
        case.idents.push(settled(after.desk_dev(), 0, Vec::new()));
        case.idents[0].evidence = vec![alarm(80, AlarmKind::RelayBehind, &[frame])];
        (case, e, q1, q2, rows)
    }

    // §31.7 as amended (the owner's decision of 2026-11-25, task D7): after a
    // rollback, a dropped seq the reference log holds nothing at, above its
    // final head, is removed when a lower seq that the same rollback dropped
    // holds a different entry in the reference log (no restore tail can
    // bring back a frame whose chain no longer leads to it): the entry lost
    // there is never lost silently with an alarm with evidence, and is lost
    // silently without one. Nothing wider: with no such lower seq (the
    // reference log ends below the rollback's first dropped seq), or with
    // that lower seq holding the dropped frame again, the seq is held to the
    // other rules (here a plain rollback, not excused); and a seq that a
    // restore tail could have brought back is not removed (the general rule,
    // read strictly: here one lost on the way while the relay still held the
    // tail's prefix).
    #[test]
    fn a_dropped_seq_above_the_final_head_is_removed_only_above_an_overwritten_one() {
        let (mut c, e, q1, q2, rows) = rolled_above_head(Some(true));
        assert_eq!(c.reference.len() as u64, q1, "the premise: the reference log ends at {q1}, below {q2}");
        assert_ne!(c.reference[q1 as usize - 1].hash, rows[q1 as usize - 1].hash, "the premise: another entry took {q1}");
        assert_eq!(c.check(), Vec::new(), "removed: never lost silently");
        let base = format!("dev 0: acknowledged entry {} is accepted 0 times in the reference log (classes there: [])", check::hexs(&e));
        c.idents[0].evidence.clear();
        assert_eq!(
            c.check(),
            vec![(2, 51, format!(
                "{base}: lost to a removed relay state (the relay before a rollback, fault at trace line 77), and no device that held its frame raised an alarm of §21.1 with evidence after it"
            ))],
            "removed, and no alarm"
        );
        c.idents[0].evidence = vec![alarm(80, AlarmKind::RelayBehind, &[rows[q2 as usize - 1].hash])];
        let at = |s: u64| Head { seq: s, chain: rows[s as usize - 1].chain };
        let frames = vec![rows[q1 as usize - 1].hash, rows[q2 as usize - 1].hash];
        c.restore_tails = vec![tail_try(85, q1, frames, Answer::Unanswered, at(q1 - 1), Some((1, at(q2))))];
        assert_eq!(
            c.check(),
            vec![(2, 51, format!(
                "{base}: lost to a rolled-back relay state that is not removed (the relay before a rollback, fault at trace line 77): the reference log holds nothing at {q2}, and a restore tail could have brought back {q2} (trace line 85: one sent while the relay was at {} on that tail never reached it)",
                q1 - 1
            ))],
            "a restore tail could have brought it back"
        );
        let plain = format!(
            "{base}: lost to a rolled-back relay state that is not removed (the relay before a rollback, fault at trace line 77): the reference log holds nothing at {q2}, nor another entry at a lower seq the rollback dropped, and no restore tail that would have brought it back was refused for a frame the relay made up"
        );
        for (over, name) in [(None, "no lower seq taken"), (Some(false), "the lower seq holds the dropped frame again")] {
            let (c, e2, _, _, _) = rolled_above_head(over);
            assert_eq!(e2, e, "one entry");
            assert_eq!(c.check(), vec![(2, 51, plain.clone())], "{name}");
        }
    }

    // §31.7 (task D6): in a run with a fork, every branch the oracle does not
    // name is a removed relay state: an acknowledgement the fork's copy gave
    // comes from one. The entry lost with it is excused, in invariants 2 and
    // 3 alike, by an alarm with evidence that a device holding its frame
    // raised after the fork began, and not without.
    #[test]
    fn an_entry_lost_with_a_forks_copy_needs_an_alarm() {
        let (mut reference, mut fork) = forked(|l| {
            for k in 0..3u8 {
                l.set(k, i64::from(k));
            }
        });
        fork.desk.new_entry_id().expect("an entry id");
        let (q, e) = fork.set(5, 5);
        reference.set(6, 6);
        let frame = fork.frame_at(q);
        let mut c = Case::of(&reference);
        c.branches.push(BranchLog { rows: fork.rows(), why: check::WHY_FORK, line: 40, removed_from: q });
        c.acks.push(AckFact { eid: e, ident: 0, sel: Sel::Fork, seq: q, frame, line: 50 });
        c.acked.push((e, 0, 51, true));
        c.enqueued.push((e, 0, 0, 8));
        c.idents.push(settled(reference.desk_dev(), 0, Vec::new()));
        let silent = "lost to a removed relay state (the forked copy of the relay, fault at trace line 40), and no device that held its frame raised an alarm of §21.1 with evidence after it";
        let both = vec![
            (2, 51, format!("dev 0: acknowledged entry {} is accepted 0 times in the reference log (classes there: []): {silent}", check::hexs(&e))),
            (3, 8, format!("dev 0: enqueued entry {} is accepted 0 times and not held or refused (in its outbox: false): {silent}", check::hexs(&e))),
        ];
        assert_eq!(c.check(), both);
        c.idents[0].evidence = vec![alarm(90, AlarmKind::Fork, &[frame])];
        assert_eq!(c.check(), Vec::new());
        c.idents[0].evidence = vec![alarm(30, AlarmKind::Fork, &[frame])];
        assert_eq!(c.check(), both, "an alarm before the fork");
    }

    /// `x` was retired at trace line `at`, after a `ReplaceReady` that named
    /// it (`after_replace`) or otherwise, having met a relay state of the
    /// reference log last at trace line `met`, its log having reached `reach`.
    fn retire(x: &mut IdentFacts, at: u64, after_replace: bool, met: u64, reach: u64) {
        x.retired = true;
        x.retired_at = after_replace.then_some(at);
        x.met_reference = met;
        x.log_reach = reach;
    }

    // §31.7 invariants 2 and 3, last outcome (the owner's decision of
    // 2026-11-25, task D7; task D6's open reading): an entry lost with a
    // removed relay state, its frame held by no device that raised an alarm,
    // is excused when its device was retired while cut off: its machine
    // re-paired as a new device after a `ReplaceReady` that named it (§24.5,
    // §28.5 step 3), with no relay state of the reference log met from the
    // fault that removed the entry on. Here a device alone on a fork's copy,
    // and one whose entry a rollback dropped under another device's. Not
    // excused: a device not retired; one retired otherwise (it saw its own
    // revocation); one that met a relay state of the reference log after the
    // fault (a message the relay produced at the fault's trace line or
    // later); an entry one more acknowledgement of came from a state no fault
    // removed.
    #[test]
    fn a_loss_to_a_removed_state_is_excused_only_for_a_device_retired_while_cut_off() {
        let (mut reference, mut fork) = forked(|l| {
            for k in 0..3u8 {
                l.set(k, i64::from(k));
            }
        });
        fork.desk.new_entry_id().expect("an entry id");
        let (q, e) = fork.set(5, 5);
        reference.set(6, 6);
        let frame = fork.frame_at(q);
        let mut c = Case::of(&reference);
        c.branches.push(BranchLog { rows: fork.rows(), why: check::WHY_FORK, line: 40, removed_from: q });
        c.acks.push(AckFact { eid: e, ident: 0, sel: Sel::Fork, seq: q, frame, line: 50 });
        c.acked.push((e, 0, 51, true));
        c.enqueued.push((e, 0, 0, 8));
        c.idents.push(settled(reference.desk_dev(), 0, Vec::new()));
        let silent = "lost to a removed relay state (the forked copy of the relay, fault at trace line 40), and no device that held its frame raised an alarm of §21.1 with evidence after it";
        let both = |tail: &str| {
            vec![
                (2, 51, format!("dev 0: acknowledged entry {} is accepted 0 times in the reference log (classes there: []): {silent}{tail}", check::hexs(&e))),
                (3, 8, format!("dev 0: enqueued entry {} is accepted 0 times and not held or refused (in its outbox: false): {silent}{tail}", check::hexs(&e))),
            ]
        };
        assert_eq!(c.check(), both(""), "not retired");
        retire(&mut c.idents[0], 100, true, 39, q);
        assert_eq!(c.check(), Vec::new(), "retired after ReplaceReady, cut off from the fork on");
        retire(&mut c.idents[0], 100, true, 40, q);
        assert_eq!(
            c.check(),
            both("; its device (dev 0) was retired, but met a relay state of the reference log (trace line 40) after the fault at trace line 40"),
            "it met the reference relay after the fault"
        );
        retire(&mut c.idents[0], 100, false, 39, q);
        assert_eq!(c.check(), both("; its device (dev 0) was retired, but not after a ReplaceReady that named it"), "retired otherwise");
        retire(&mut c.idents[0], 100, true, 39, q);
        c.acks.push(AckFact { eid: e, ident: 0, sel: Sel::Main(0), seq: q, frame, line: 60 });
        let v = c.check();
        assert_eq!(v.len(), 2, "{v:?}");
        assert!(
            v.iter().all(|x| x.2.ends_with(&format!(
                "acknowledged at {q} (trace line 60) by a relay state no rollback or fork removed; its device was retired, but the relay acknowledged it at {q} (trace line 60) from a relay state no fault removed"
            ))),
            "{v:?}"
        );

        // A rollback: the overwritten seq is removed (no restore tail could
        // have brought it back), the fault at trace line 77.
        let Rolled { case: mut c, e, .. } = rolled(true, None);
        c.enqueued.push((e, 0, 0, 8));
        assert_eq!(c.check().len(), 2, "not retired");
        retire(&mut c.idents[0], 100, true, 76, 0);
        assert_eq!(c.check(), Vec::new(), "retired after ReplaceReady, cut off from the rollback on");
        retire(&mut c.idents[0], 100, true, 80, 0);
        let v = c.check();
        assert_eq!(v.len(), 2, "{v:?}");
        assert!(v.iter().all(|x| x.2.ends_with("(trace line 80) after the fault at trace line 77")), "{v:?}");
    }

    // §31.7 invariants 2 and 3, last outcome (task D7): an entry the
    // reference log holds once, finally refused, that its device reported
    // in no way, is excused when the device was retired (re-paired as a new
    // device after a `ReplaceReady` that named it, §24.5) and its log never
    // reached the refusal's seq: it never met what became of the entry. Not
    // excused: a device not retired; one whose log reached that seq; one
    // retired otherwise. A device retired while cut off excuses no
    // acknowledged entry of another device.
    #[test]
    fn a_final_refusal_is_excused_only_for_a_retired_device_whose_log_never_reached_it() {
        let mut log = HandLog::new(SEED);
        log.set(0, 1);
        let (_, first, added) = log.add_device(9);
        let (r, again, _) = log.add_device(9);
        log.set(1, 2);
        let mut c = Case::of(&log);
        let led = check::ledger(&c.reference, Some(&c.template), &c.roots);
        assert_eq!(led[r as usize - 1].class, "refuse", "the premise: the second DeviceAdded of one device is refused at {r}");
        c.acks = vec![AckFact { eid: again, ident: 0, sel: Sel::Main(0), seq: r, frame: log.frame_at(r), line: 5 }];
        c.acked = vec![(first, 0, 4, true), (again, 0, 6, true)];
        c.registered = BTreeMap::from([(first, added), (again, added)]);
        c.enqueued = vec![(again, 0, 0, 3)];
        c.idents = vec![settled(log.desk_dev(), 0, Vec::new()), settled(dev(0xd9), 0, Vec::new())];
        let both = |tail: &str| {
            vec![
                (2, 6, format!(
                    "dev 0: acknowledged entry {} is accepted 0 times in the reference log (classes there: [\"refuse\"]): a pairing's DeviceAdded refused after register (§28.4 step 4), finally refused at {r}, and its device reported no refusal record, PairFailed of its pairing or end of its operation for it{tail}",
                    check::hexs(&again)
                )),
                (3, 3, format!(
                    "dev 0: enqueued entry {} is accepted 0 times and not held or refused (in its outbox: false): acknowledged at {r} (trace line 5) by a relay state no rollback or fork removed{tail}",
                    check::hexs(&again)
                )),
            ]
        };
        assert_eq!(c.check(), both(""), "not retired");
        retire(&mut c.idents[0], 100, true, 99, r - 1);
        assert_eq!(c.check(), Vec::new(), "retired after ReplaceReady, its log below the refusal's seq");
        retire(&mut c.idents[0], 100, true, 99, r);
        assert_eq!(c.check(), both(&format!("; its device (dev 0) was retired, but its log reached {r} (the refusal is at {r})")), "its log reached it");
        retire(&mut c.idents[0], 100, false, 99, r - 1);
        assert_eq!(c.check(), both("; its device (dev 0) was retired, but not after a ReplaceReady that named it"), "retired otherwise");
        c.idents[0] = settled(log.desk_dev(), 0, Vec::new());
        retire(&mut c.idents[1], 100, true, 0, 0);
        assert_eq!(c.check(), both(""), "another device retired");
    }

    // §31.7 invariant 3 (task D6): "never lost silently as invariant 2
    // states" is read with at least one acknowledgement: an edit no relay
    // acknowledged was never lost with a relay state, and an alarm does not
    // excuse its loss. The same edit acknowledged by the removed state is.
    #[test]
    fn an_edit_no_relay_acknowledged_is_not_excused_by_an_alarm() {
        let Rolled { case: mut c, e, frame, .. } = rolled(true, None);
        c.acked.clear();
        c.enqueued.push((e, 0, 0, 8));
        c.idents[0].evidence = vec![alarm(80, AlarmKind::RelayBehind, &[frame])];
        assert_eq!(c.check(), Vec::new());
        c.acks.clear();
        assert_eq!(
            c.check(),
            vec![(3, 8, format!("dev 0: enqueued entry {} is accepted 0 times and not held or refused (in its outbox: false): no relay acknowledged it", check::hexs(&e)))]
        );
    }

    // Task D3: a device whose pairing only a forked (or rolled-back) relay
    // state holds is named in invariant 6 (`its pairing is only on …`); one
    // whose `DeviceAdded` is in the reference log is not. And a Skip refused
    // after the relay acknowledged it (another Skip of the same target came
    // first, `seal/ctl_target`), unreported, is not a pairing's `DeviceAdded`
    // (§28.4 step 4).
    #[test]
    fn a_pairing_lost_with_a_fork_is_named_and_a_raced_skip_is_not_a_pairing() {
        let (mut reference, mut fork) = forked(|l| {
            for k in 0..3u8 {
                l.set(k, i64::from(k));
            }
        });
        let (t, _) = reference.set(5, 5);
        reference.skip(t);
        let (raced, raced_id) = reference.skip(t);
        let (here, _, kept) = reference.add_device(8);
        fork.set(6, 6);
        let (there, _, lost) = fork.add_device(9);
        let mut c = Case::of(&reference);
        c.branches.push(BranchLog { rows: fork.rows(), why: check::WHY_FORK, line: 77, removed_from: 5 });
        let led = check::ledger(&c.reference, Some(&c.template), &c.roots);
        assert_eq!(led[raced as usize - 1].class, "refuse", "the second Skip of {t} is refused");
        c.acked.push((raced_id, 0, 4, true));
        let gone = |d: Dev, joined_at: u64| IdentFacts { live: false, caught_up: false, ..settled(d, joined_at, Vec::new()) };
        c.idents.push(settled(reference.desk_dev(), 0, Vec::new()));
        c.idents.push(gone(kept, here));
        c.idents.push(gone(lost, there));
        let v = c.check();
        assert_eq!(
            of(&v, 2),
            vec![format!(
                "dev 0: acknowledged entry {} is accepted 0 times in the reference log (classes there: [\"refuse\"]): finally refused at {raced}, and its device reported no refusal record, PairFailed of its pairing or end of its operation for it",
                check::hexs(&raced_id)
            )],
            "{v:?}"
        );
        let six = of(&v, 6);
        assert_eq!(six.len(), 2, "{v:?}");
        assert!(six[0].starts_with("dev 1: neither live and caught up") && !six[0].contains("rolled-back"), "{six:?}");
        assert!(
            six[1].starts_with("dev 2: neither live and caught up")
                && six[1].ends_with(": its pairing is only on a rolled-back relay state (the forked copy of the relay, fault at trace line 77)"),
            "{six:?}"
        );
        assert_eq!(v.len(), 3, "{v:?}");
    }

    // Task D3 (`check::pick_roots`): a fork's copy and the reference log
    // each rotate to epoch 2, with two roots. Each log is classified with
    // the root that opens it; one root per epoch would leave the fork's
    // epoch-2 frames unopened, so the record a device applied there and lost
    // would not be named in invariant 3's text.
    #[test]
    fn an_epoch_rotated_on_two_branches_keeps_both_roots() {
        let (mut reference, mut fork) = forked(|l| {
            l.set(0, 1);
        });
        reference.rotate();
        reference.set(1, 1);
        fork.set(2, 2);
        fork.rotate();
        let (q, lost) = fork.set(3, 3);
        let two: BTreeSet<(u32, [u8; 32])> = reference.roots().into_iter().chain(fork.roots()).filter(|r| r.0 == 2).collect();
        assert_eq!(two.len(), 2, "two roots of epoch 2");
        let mut c = Case::of(&reference);
        let mut all = c.roots.clone();
        all.extend(fork.roots());
        let fork_rows = fork.rows();
        // The premise: with the reference log's roots alone the fork's last frame is not opened.
        let alone = check::ledger(&fork_rows, Some(&c.template), &c.roots);
        assert_eq!(alone.last().map(|l| l.class), Some("error"), "the fork's epoch-2 frame needs its own root");
        for rows in [&c.reference, &fork_rows] {
            let led = check::ledger(rows, Some(&c.template), &all);
            assert!(led.iter().all(|l| l.accept), "each log is opened with its own root of epoch 2");
        }
        c.roots = all;
        c.branches.push(BranchLog { rows: fork_rows.clone(), why: check::WHY_FORK, line: 77, removed_from: 3 });
        let fo = check::oracle(&fork_rows, Some(&c.template), &c.roots, &c.board, &c.relay).0;
        c.idents.push(settled(fork.desk_dev(), 0, honest(&fo, 2..=q)));
        c.acks.push(AckFact { eid: lost, ident: 0, sel: Sel::Fork, seq: q, frame: fork.frame_at(q), line: 80 });
        c.acked.push((lost, 0, 9, false));
        c.enqueued.push((lost, 0, 1, 8));
        let v = c.check();
        let fault = "(the forked copy of the relay, fault at trace line 77)";
        let silent = format!("lost to a removed relay state {fault}, and no device that held its frame raised an alarm of §21.1 with evidence after it");
        assert_eq!(
            v,
            vec![
                (2, 9, format!("dev 0: acknowledged entry {} is accepted 0 times in the reference log (classes there: []): {silent}", check::hexs(&lost))),
                (3, 8, format!(
                    "dev 0: enqueued entry {} is accepted 0 times and not held or refused (in its outbox: false): {silent}; applied on a rolled-back relay state {fault}",
                    check::hexs(&lost)
                )),
            ]
        );
    }

    // Invariant 5 on a fork (task D3, `Deletion::Pending` with a chain): a
    // device that followed a fork's copy and drops the pending root of the
    // rotation it sealed there is judged against the log it followed, the
    // branch whose chain it holds at B, not the reference log (which never
    // holds that rotation). The same drop judged on the reference log, a
    // drop of the reference log's rotation on the fork, and a drop below the
    // fork's rotation on the fork are each not explained.
    #[test]
    fn a_pending_root_drop_on_a_fork_is_judged_by_the_forks_log() {
        let (mut reference, mut fork) = forked(|l| {
            l.set(0, 1);
        });
        let (_, on_ref) = reference.rotate();
        reference.set(1, 1);
        reference.set(4, 4);
        // Both Desks draw entry ids from one seed: the fork's draws one it
        // never seals, so no entry of the fork reuses an id of the reference log's.
        fork.desk.new_entry_id().expect("an entry id");
        fork.set(2, 2);
        let (r, on_fork) = fork.rotate();
        let (q, _) = fork.set(3, 3);
        let mut c = Case::of(&reference);
        c.roots.extend(fork.roots());
        let fork_rows = fork.rows();
        let chain = |b: u64| fork_rows[b as usize - 1].chain;
        // The premises: the fork's rows at r - 1 and q are not the reference
        // log's; the fork's rotation is in no row of the reference log.
        for b in [r - 1, q] {
            assert_ne!(c.reference.get(b as usize - 1).map(|x| x.chain), Some(chain(b)), "the fork differs from the reference log at {b}");
        }
        let led = check::ledger(&c.reference, Some(&c.template), &c.roots);
        assert!(led.iter().all(|l| l.entry_id != Some(on_fork)), "the fork's rotation is not in the reference log");
        let fl = check::ledger(&fork_rows, Some(&c.template), &c.roots);
        assert!(fl.iter().all(|l| l.entry_id != Some(on_ref)), "the reference log's rotation is not on the fork");
        assert!(fl.iter().any(|l| l.seq == r && l.entry_id == Some(on_fork)), "the fork's rotation is at {r} on the fork");
        c.branches.push(BranchLog { rows: fork_rows.clone(), why: check::WHY_FORK, line: 77, removed_from: 3 });
        let pending = |ids: Vec<[u8; 16]>, ch: Option<Hash32>, b: u64| (3usize, Deletion::Pending(ids, ch), b, 9u64);
        c.deletions = vec![pending(vec![on_fork], Some(chain(q)), q), pending(vec![on_fork], Some(chain(r)), r)];
        assert_eq!(c.check(), Vec::new());
        for d in [
            pending(vec![on_fork], None, q),
            pending(vec![on_ref], Some(chain(q)), q),
            pending(vec![on_fork], Some(chain(r - 1)), r - 1),
        ] {
            let text = format!("dev 3: {:?} with B = {} is not explained by §31.7 invariant 5", d.1, d.2);
            c.deletions = vec![d];
            assert_eq!(c.check(), vec![(5, 9, text)]);
        }
    }

    // Invariant 5 on a fork (task D8, gate 17163374614648704986 and 5685): a
    // device that followed a fork's copy, where Desk's purge rotation with
    // `purge_before = p` was accepted at `r`, truncates its log below `p`,
    // deletes its bases below `r` and, once its log clock is thirty days past
    // the rotation, forgets the roots below the purge's epoch (§28.2 "On every
    // device"). Each is explained by that accepted purge rotation on the log
    // it followed at B, the branch whose chain it holds at B, although the
    // reference log holds no purge. Nothing wider: the same deletions judged
    // on the reference log, a truncation to another seq, a base at `r`, a
    // deletion below `r`, and the roots before the thirty days are each not
    // explained.
    #[test]
    fn a_purges_deletions_on_a_fork_are_judged_by_the_forks_log() {
        let (mut reference, mut fork) = forked(|l| {
            for k in 0..3u8 {
                l.set(k, i64::from(k));
            }
        });
        reference.set(4, 4);
        reference.set(5, 5);
        reference.set(6, 6);
        // Both Desks draw entry ids from one seed: the fork's draws one it
        // never seals, so no entry of the fork reuses an id of the reference log's.
        fork.desk.new_entry_id().expect("an entry id");
        fork.set(7, 7);
        let p = fork.head().seq;
        let (r, _) = fork.purge(p);
        let (q, _) = fork.set(8, 8);
        fork.at += 2_600_000_000;
        let (z, _) = fork.set(9, 9);
        let mut c = Case::of(&reference);
        c.roots.extend(fork.roots());
        let fork_rows = fork.rows();
        let chain = |b: u64| fork_rows[b as usize - 1].chain;
        // The premises: the purge rotation is accepted at `r` on the fork,
        // with its new epoch; the reference log holds no purge; the fork's
        // rows from `r - 1` on are not the reference log's.
        let fl = check::ledger(&fork_rows, Some(&c.template), &c.roots);
        let n = fl
            .iter()
            .find(|l| l.seq == r && l.accept && l.purge_before == Some(p))
            .and_then(|l| l.new_epoch)
            .unwrap_or_else(|| panic!("the purge rotation is accepted at {r} on the fork: {:?}", fl.iter().find(|l| l.seq == r)));
        assert!(fl.iter().all(|l| l.accept), "every row of the fork is accepted");
        let led = check::ledger(&c.reference, Some(&c.template), &c.roots);
        assert!(led.iter().all(|l| l.purge_before.is_none()), "the reference log holds no purge");
        for b in [r - 1, r, q] {
            assert_ne!(c.reference.get(b as usize - 1).map(|x| x.chain), Some(chain(b)), "the fork differs from the reference log at {b}");
        }
        c.branches.push(BranchLog { rows: fork_rows.clone(), why: check::WHY_FORK, line: 77, removed_from: 5 });
        let d = |what: Deletion, b: u64| (3usize, what, b, 9u64);
        let on = |b: u64| Some(chain(b));
        c.deletions = vec![
            d(Deletion::Truncate(p, false, on(r)), r),
            d(Deletion::Bases(vec![2, r - 1], false, on(r)), r),
            d(Deletion::Truncate(p, false, on(q)), q),
            d(Deletion::Roots((1..n).collect(), on(z)), z),
        ];
        assert_eq!(c.check(), Vec::new());
        for x in [
            d(Deletion::Truncate(p, false, None), r),
            d(Deletion::Truncate(p, false, Some(c.reference[r as usize - 1].chain)), r),
            d(Deletion::Bases(vec![2], false, None), r),
            d(Deletion::Roots((1..n).collect(), None), z.min(c.reference.len() as u64)),
            d(Deletion::Truncate(p + 1, false, on(r)), r),
            d(Deletion::Truncate(p, false, on(r - 1)), r - 1),
            d(Deletion::Bases(vec![r], false, on(r)), r),
            d(Deletion::Bases(vec![2], false, on(r - 1)), r - 1),
            d(Deletion::Roots((1..n).collect(), on(q)), q),
        ] {
            let text = format!("dev 3: {:?} with B = {} is not explained by §31.7 invariant 5", x.1, x.2);
            c.deletions = vec![x];
            assert_eq!(c.check(), vec![(5, 9, text)]);
        }
    }

    // Invariant 5: an identity write that drops the pending root of a
    // rotation is explained only by that rotation's own entry, at or below
    // the B of the write; another rotation, or none, explains nothing.
    #[test]
    fn a_pending_root_drop_needs_its_own_rotation() {
        let mut log = HandLog::new(SEED);
        log.set(0, 1);
        let (r, own) = log.rotate();
        log.set(1, 1);
        let (_, other) = log.rotate();
        let mut c = Case::of(&log);
        c.idents.push(settled(log.desk_dev(), 0, Vec::new()));
        let pending = |ids: Vec<[u8; 16]>, b: u64| (0usize, Deletion::Pending(ids, None), b, 9u64);
        c.deletions = vec![pending(vec![own], r), pending(vec![own, other], r + 2)];
        assert_eq!(c.check(), Vec::new());
        for d in [pending(vec![own], r - 1), pending(vec![[0xee; 16]], r + 2), pending(vec![own, other], r)] {
            let text = format!("dev 0: {:?} with B = {} is not explained by §31.7 invariant 5", d.1, d.2);
            c.deletions = vec![d];
            assert_eq!(c.check(), vec![(5, 9, text)]);
        }
    }

    // Invariant 5, task D5 (§19.4 step 5, ENG-SNAP-004): an unsent `snap`
    // record whose blob epoch is no longer the current epoch must be dropped.
    // The drop is explained by an accepted rotation to a later epoch at or
    // below the B of the drop, in the log the device followed there, for an
    // entry no row of that log carries; nothing wider: the drop below the
    // rotation, a blob sealed under the current epoch, an entry the log
    // holds (it was sent), or a rotation only another branch holds explain
    // nothing.
    #[test]
    fn an_unsent_snap_dropped_after_a_rotation_is_explained() {
        let (mut reference, mut fork) = forked(|l| {
            l.set(0, 1);
        });
        let (r, _) = reference.rotate();
        let (_, sent) = reference.set(1, 1);
        fork.desk.new_entry_id().expect("an entry id");
        fork.set(2, 2);
        let (q, _) = fork.set(3, 3);
        let mut c = Case::of(&reference);
        c.idents.push(settled(reference.desk_dev(), 0, Vec::new()));
        let led = check::ledger(&c.reference, Some(&c.template), &c.roots);
        let new = led.iter().find(|l| l.seq == r && l.accept).and_then(|l| l.new_epoch).expect("the rotation at r is accepted with a new epoch");
        let old = new - 1;
        let fork_rows = fork.rows();
        assert!(check::ledger(&fork_rows, Some(&c.template), &c.roots).iter().all(|l| l.new_epoch.is_none()), "the fork rotates nothing");
        assert_ne!(c.reference.get(q as usize - 1).map(|x| x.chain), Some(fork_rows[q as usize - 1].chain), "the fork differs at {q}");
        c.branches.push(BranchLog { rows: fork_rows.clone(), why: check::WHY_FORK, line: 77, removed_from: 3 });
        let snap = |eid: [u8; 16], epoch: u32, ch: Option<Hash32>, b: u64| (0usize, Deletion::Snap(eid, epoch, ch), b, 9u64);
        let unsent = [0x5a; 16];
        c.deletions = vec![snap(unsent, old, None, r), snap(unsent, old, None, r + 1)];
        assert_eq!(c.check(), Vec::new());
        for d in [
            snap(unsent, old, None, r - 1),
            snap(unsent, new, None, r + 1),
            snap(sent, old, None, r + 1),
            snap(unsent, old, Some(fork_rows[q as usize - 1].chain), q),
        ] {
            let text = format!("dev 0: {:?} with B = {} is not explained by §31.7 invariant 5", d.1, d.2);
            c.deletions = vec![d];
            assert_eq!(c.check(), vec![(5, 9, text)]);
        }
    }
    // §31.7 invariant 2 (task D6): the second outcome asks that the reference
    // log hold the entry once. One sealed twice and refused at both seqs (a
    // second `DeviceAdded` of one device, `seal/ctl_duplicate_key`) is no
    // final refusal, reported or not.
    #[test]
    fn an_entry_the_reference_log_holds_twice_refused_is_no_final_refusal() {
        let mut log = HandLog::new(SEED);
        log.set(0, 1);
        let (_, _, added) = log.add_device(9);
        let ((r1, r2), again) = log.add_device_twice(9);
        log.set(1, 2);
        let mut c = Case::of(&log);
        let led = check::ledger(&c.reference, Some(&c.template), &c.roots);
        assert_eq!((led[r1 as usize - 1].class, led[r2 as usize - 1].class), ("refuse", "refuse"), "both are refused");
        c.acks = vec![AckFact { eid: again, ident: 0, sel: Sel::Main(0), seq: r1, frame: log.frame_at(r1), line: 5 }];
        c.acked = vec![(again, 0, 6, true)];
        c.registered = BTreeMap::from([(again, added)]);
        let mut x = settled(log.desk_dev(), 0, Vec::new());
        x.pair_failed = vec![(added, 9)];
        x.refusal_seqs = BTreeSet::from([r1, r2]);
        c.idents = vec![x];
        assert_eq!(
            c.check(),
            vec![(2, 6, format!("dev 0: acknowledged entry {} is accepted 0 times in the reference log (classes there: [\"refuse\", \"refuse\"])", check::hexs(&again)))]
        );
    }

    // Invariant 4 (task D6): the replay without a Skip's target models a
    // rules upgrade as the board apply does (§14.1: `SM::upgrade(v)` before
    // the entry after the one that activates `v`). Desk reports rules 2 in a
    // beat and proposes them; a `Mul` (rules 2 only) follows, all between the
    // target and its Skip. A device that never applied the target is judged
    // by the replay without it there: its states, the oracle's of a twin log
    // whose target changes nothing, hold.
    #[test]
    fn the_replay_without_a_skip_target_upgrades_the_rules_as_the_board_does() {
        let mut log = HandLog::new(SEED);
        let mut twin = HandLog::new(SEED);
        for l in [&mut log, &mut twin] {
            for k in 0..3u8 {
                l.set(k, 10 + i64::from(k));
            }
        }
        let (t, _) = log.set(1, 99);
        twin.cmd(&ToyCommand::Fail { code: 0 });
        for l in [&mut log, &mut twin] {
            l.beat(&[1, 2]);
            l.upgrade(2);
            l.cmd(&ToyCommand::Mul { key: 2, factor: 2 });
        }
        let (s, _) = log.skip(t);
        let mut c = Case::of(&log);
        let led = check::ledger(&c.reference, Some(&c.template), &c.roots);
        assert_eq!((led[t as usize - 1].rules, led[s as usize - 2].rules), (1, 2), "the upgrade activates rules 2 between {t} and {s}");
        assert!(led[s as usize - 2].cmd && led[s as usize - 2].accept, "the Mul is accepted");
        let o = c.oracle();
        let tw = Case::of(&twin).oracle();
        let chain = |q: u64| c.reference[q as usize - 1].chain;
        let mut samples = honest(&o, 2..t);
        samples.extend((t..s).map(|q| (q, chain(q), tw.get(&q).expect("the twin's oracle").1, 1)));
        assert_ne!(samples.last().map(|x| x.2), o.get(&(s - 1)).map(|x| x.1), "the target changes the state at {}", s - 1);
        let mut x = settled(dev(0xd2), 1, samples);
        x.crossings.push(Crossing { from: t - 1, to: t, to_chain: chain(t), k: s, k_chain: chain(s), line: 0 });
        c.idents.push(x);
        assert_eq!(c.check(), Vec::new());
    }

    // §31.7 invariant 2, the second outcome's last case (the owner's decision
    // of 2026-11-25, "Duty going on counts", task D9): an entry of the
    // engine's own duties refused with a re-sealable code, for which no
    // refusal record is written (ENG-REF-001) and no entry is sealed again
    // (§24.3), whose duty went on. The board: Laptop's `snap` entry for
    // 128, sealed under epoch 1 before Desk's rotation, which the relay
    // sequenced first, is refused `seal/stale_epoch` at `q` (`seal.md`
    // §13.9: no `snap` entry under the previous epoch), as an own snapshot
    // of gate seeds was; then Desk's snapshots of 64 (`below`, under `S`)
    // and of 128 (`cover`, at `S`). Facts: Desk is identity 0, Laptop
    // identity 1; the relay acknowledged the refused entry to Laptop at
    // trace line 10 (delivered at 11).
    struct DutyBoard {
        case: Case,
        q: u64,
        eid: [u8; 16],
        frame: Hash32,
        below: u64,
        cover: u64,
        desk: Dev,
    }

    fn duty_board() -> DutyBoard {
        let mut log = HandLog::new(SEED);
        for k in 0..130u8 {
            log.set(k % 8, i64::from(k));
        }
        let (added, laptop) = log.pair(9);
        let m1 = log.membership();
        let (frame, eid) = log.laptop_hold(&m1, HandLog::snap_body(128, 1));
        log.rotate();
        let q = log.laptop_append(frame.clone());
        let (below, _) = log.snap(64);
        let (cover, _) = log.snap(128);
        log.set(1, 1);
        let mut c = Case::of(&log);
        let led = check::ledger(&c.reference, Some(&c.template), &c.roots);
        let l = &led[q as usize - 1];
        assert_eq!((l.class, l.code, l.kind, l.entry_id), ("refuse", Some("seal/stale_epoch"), Some(Kind::Snap), Some(eid)), "Laptop's snapshot is refused for its epoch");
        for p in [below, cover] {
            let l = &led[p as usize - 1];
            assert!(l.accept && l.kind == Some(Kind::Snap) && l.author == log.desk_dev(), "Desk's snapshot at {p} is accepted");
        }
        let fh: Hash32 = Sha256::digest(&frame).into();
        c.idents = vec![settled(log.desk_dev(), 0, Vec::new()), settled(laptop, added, Vec::new())];
        c.acks = vec![AckFact { eid, ident: 1, sel: Sel::Main(0), seq: q, frame: fh, line: 10 }];
        c.acked = vec![(eid, 1, 11, true)];
        c.frames = BTreeMap::from([(fh, (eid, Kind::Snap, true))]);
        c.snap_seqs = BTreeMap::from([(eid, 128)]);
        DutyBoard { case: c, q, eid, frame: fh, below, cover, desk: log.desk_dev() }
    }

    /// A relay acknowledged to identity `ident`, at trace line `line`, an entry `eid` of kind `kind`.
    fn ack_later(c: &mut Case, ident: usize, line: u64, kind: Kind, eid: [u8; 16]) {
        let frame = [eid[0]; 32];
        c.acks.push(AckFact { eid, ident, sel: Sel::Main(0), seq: 900, frame, line });
        c.frames.insert(frame, (eid, kind, kind != Kind::Cmd));
    }

    // A refused `snap` entry passes when its duty went on, read from what the
    // run records: Laptop wrote a later `snap` entry that a relay acknowledged
    // after the relay sequenced the refused one (whatever became of it); its
    // durable snapshot duty for 128 ended with three failures (two counted,
    // then cleared after that); or Desk's snapshot of a seq at or above 128
    // reached Laptop's log (§19.2: accepting it clears the duty). Without
    // any of them it fails, and so does each near miss: a later entry
    // acknowledged no later than the refused one, to another device, of
    // another kind, or the refused entry itself; a duty cleared after one
    // failure, before the refused entry was sequenced, for another seq, or
    // replaced by a later seq rather than ended; Laptop's log reaching only
    // Desk's snapshot of 64, or the covering snapshot being Laptop's own; and
    // an entry sealed into two frames (§24.3: a duty entry is never sealed
    // again).
    #[test]
    fn a_refused_snap_entry_passes_only_when_its_duty_went_on() {
        let DutyBoard { case: mut c, q, eid, frame, below, cover, desk } = duty_board();
        let stopped = vec![(2, 11, format!(
            "dev 1: acknowledged entry {} is accepted 0 times in the reference log (classes there: [\"refuse\"]): finally refused at {q}, and its device reported no refusal record, PairFailed of its pairing or end of its operation for it; a snap entry refused at {q} with seal/stale_epoch, for which no refusal record is written (ENG-REF-001), and its snapshot duty did not go on: its device wrote no other snap entry after it, its duty for 128 did not end with three failures, and no other device's snapshot of a seq at or above 128 reached its log (§19.2, §19.4)",
            check::hexs(&eid)
        ))];
        assert_eq!(c.check(), stopped, "its duty did not go on");
        c.idents[1].refusal_seqs = BTreeSet::from([q]);
        assert_eq!(c.check(), Vec::new(), "a refusal record is a report, as before");
        c.idents[1].refusal_seqs = BTreeSet::new();
        // A later `snap` entry of its own, acknowledged.
        let (acks, frames) = (c.acks.clone(), c.frames.clone());
        let later = |c: &mut Case, ident: usize, line: u64, kind: Kind, e: [u8; 16]| {
            (c.acks, c.frames) = (acks.clone(), frames.clone());
            ack_later(c, ident, line, kind, e);
            c.check()
        };
        assert_eq!(later(&mut c, 1, 20, Kind::Snap, [0xa2; 16]), Vec::new(), "a later snap entry of its own");
        for (name, ident, line, kind, e) in [
            ("acknowledged with the refused one", 1, 10, Kind::Snap, [0xa2; 16]),
            ("acknowledged before it", 1, 9, Kind::Snap, [0xa2; 16]),
            ("another device's", 0, 20, Kind::Snap, [0xa2; 16]),
            ("a beat", 1, 20, Kind::Beat, [0xa3; 16]),
        ] {
            assert_eq!(later(&mut c, ident, line, kind, e), stopped, "a later entry {name}");
        }
        (c.acks, c.frames) = (acks.clone(), frames.clone());
        c.acks.push(AckFact { eid, ident: 1, sel: Sel::Main(0), seq: q, frame, line: 20 });
        assert_eq!(c.check(), stopped, "the refused entry acknowledged again (`dup`) is no later entry");
        // Sealed into two frames: no duty entry is sealed again (§24.3).
        (c.acks, c.frames) = (acks.clone(), frames.clone());
        ack_later(&mut c, 1, 20, Kind::Snap, [0xa2; 16]);
        c.frames.insert([0xee; 32], (eid, Kind::Snap, true));
        assert_eq!(
            c.check(),
            vec![(2, 11, format!(
                "dev 1: acknowledged entry {} is accepted 0 times in the reference log (classes there: [\"refuse\"]): finally refused at {q}, and its device reported no refusal record, PairFailed of its pairing or end of its operation for it; a snap entry refused at {q} with seal/stale_epoch, but sealed into 2 frames (§24.3: no duty entry is sealed again)",
                check::hexs(&eid)
            ))],
            "sealed twice"
        );
        (c.acks, c.frames) = (acks.clone(), frames.clone());
        assert!(c.frames.keys().eq([&frame]), "one frame again");
        // Its snapshot duty for 128 ended with three failures.
        let tau = WALL0 + 3_600_000;
        let duty = |c: &mut Case, d: Vec<(u64, check::SnapDuty)>| {
            c.idents[1].snap_duty = d;
            c.check()
        };
        assert_eq!(duty(&mut c, vec![(5, Some((128, tau, 2))), (12, None)]), Vec::new(), "the duty ended with three failures");
        for (name, d) in [
            ("after one failure", vec![(5, Some((128, tau, 1))), (12, None)]),
            ("before the refused entry was sequenced", vec![(5, Some((128, tau, 2))), (10, None)]),
            ("for another seq", vec![(5, Some((64, tau, 2))), (12, None)]),
            ("replaced by a later seq", vec![(5, Some((128, tau, 2))), (12, Some((192, tau, 0)))]),
        ] {
            assert_eq!(duty(&mut c, d), stopped, "a duty cleared {name}");
        }
        c.idents[1].snap_duty = Vec::new();
        // Another device's snapshot covered it.
        c.idents[1].log_reach = cover;
        assert_eq!(c.check(), Vec::new(), "Desk's snapshot of 128 reached Laptop's log");
        c.idents[1].log_reach = below;
        assert_eq!(c.check(), stopped, "Laptop's log reached only Desk's snapshot of 64");
        c.idents[1].log_reach = cover;
        c.idents[1].dev = desk;
        assert_eq!(c.check(), stopped, "the snapshot of 128 is its own");
    }

    // The duty that went on is the duty of the entry's device ("its duty went
    // on", §31.7 invariant 2 as the owner amended it): Laptop's refused `snap`
    // entry passes when Laptop's own duty for 128 ended with three failures,
    // or when Desk's snapshot of 128 reached Laptop's log; not when Desk's
    // duty for 128 ended so, nor when Desk's snapshot of 128 reached a third
    // device's log ("Phone", identity 2) and not Laptop's.
    #[test]
    fn another_devices_duty_going_on_does_not_pass_the_entry() {
        let DutyBoard { case: mut c, q, eid, cover, .. } = duty_board();
        let stopped = c.check();
        let why = format!(
            "a snap entry refused at {q} with seal/stale_epoch, for which no refusal record is written (ENG-REF-001), and its snapshot duty did not go on: its device wrote no other snap entry after it, its duty for 128 did not end with three failures, and no other device's snapshot of a seq at or above 128 reached its log (§19.2, §19.4)"
        );
        assert!(
            matches!(stopped.as_slice(), [(2, 11, t)] if t.starts_with(&format!("dev 1: acknowledged entry {}", check::hexs(&eid))) && t.ends_with(&why)),
            "Laptop's duty did not go on: {stopped:?}"
        );
        // Three failures: Laptop's duty for 128 ending so passes the entry, Desk's does not.
        let ended = vec![(5, Some((128, WALL0 + 3_600_000, 2))), (12, None)];
        c.idents[1].snap_duty.clone_from(&ended);
        assert_eq!(c.check(), Vec::new(), "Laptop's duty for 128 ended with three failures");
        c.idents[1].snap_duty = Vec::new();
        c.idents[0].snap_duty = ended;
        assert_eq!(c.check(), stopped, "Desk's duty for 128 ended with three failures");
        c.idents[0].snap_duty = Vec::new();
        // Covered: Desk's snapshot of 128 reaching Laptop's log passes the entry, reaching Phone's does not.
        let mut phone = settled(dev(0x77), 0, Vec::new());
        phone.log_reach = cover;
        c.idents.push(phone);
        assert_eq!(c.check(), stopped, "Desk's snapshot of 128 reached Phone's log, not Laptop's");
        c.idents[1].log_reach = cover;
        assert_eq!(c.check(), Vec::new(), "Desk's snapshot of 128 reached Laptop's log");
    }

    // A beat refused `seal/old_epoch` (two rotations late: the observer opens
    // no such frame, so the run's record of Laptop's outbox gives its kind
    // and the reference log holds its frame) passes when Laptop beat again:
    // a later beat a relay acknowledged to it after the refused one; not a
    // later `snap` entry, nor a beat acknowledged with it (§23).
    #[test]
    fn a_refused_beat_passes_only_when_its_device_beat_again() {
        let mut log = HandLog::new(SEED);
        log.set(0, 1);
        let (added, laptop) = log.pair(9);
        let m1 = log.membership();
        let (frame, eid) = log.laptop_hold(&m1, HandLog::beat_body());
        log.rotate();
        log.rotate();
        let q = log.laptop_append(frame.clone());
        log.set(1, 1);
        let mut c = Case::of(&log);
        let led = check::ledger(&c.reference, Some(&c.template), &c.roots);
        let l = &led[q as usize - 1];
        assert_eq!((l.class, l.code, l.kind, l.entry_id), ("refuse", Some("seal/old_epoch"), None, None), "Laptop's beat is refused two epochs late, unopened");
        let fh: Hash32 = Sha256::digest(&frame).into();
        c.idents = vec![settled(log.desk_dev(), 0, Vec::new()), settled(laptop, added, Vec::new())];
        c.acks = vec![AckFact { eid, ident: 1, sel: Sel::Main(0), seq: q, frame: fh, line: 10 }];
        c.acked = vec![(eid, 1, 11, true)];
        c.frames = BTreeMap::from([(fh, (eid, Kind::Beat, true))]);
        let stopped = vec![(2, 11, format!(
            "dev 1: acknowledged entry {} is accepted 0 times in the reference log (classes there: []): acknowledged at {q} (trace line 10) by a relay state no rollback or fork removed; a beat refused at {q} with seal/old_epoch, for which no refusal record is written (ENG-REF-001), and its device beat no more after it (§23)",
            check::hexs(&eid)
        ))];
        assert_eq!(c.check(), stopped, "it beat no more");
        let (acks, frames) = (c.acks.clone(), c.frames.clone());
        for (name, line, kind, want) in [
            ("a later beat", 20, Kind::Beat, Vec::new()),
            ("a later snap entry", 20, Kind::Snap, stopped.clone()),
            ("a beat acknowledged with it", 10, Kind::Beat, stopped.clone()),
        ] {
            (c.acks, c.frames) = (acks.clone(), frames.clone());
            ack_later(&mut c, 1, line, kind, [0xb3; 16]);
            assert_eq!(c.check(), want, "{name}");
        }
    }

    // Nothing wider: a `cmd` entry and an operation's control entry, each
    // refused `seal/stale_epoch` (sealed by Laptop under the epoch before
    // Desk's rotation, based on the head after it, `seal.md` §13.9) and never
    // sealed again, are held to the other outcomes as before, though Laptop
    // went on with later entries of their kinds; the `cmd` entry also fails
    // invariant 3, which asks a refusal record of a refused edit.
    #[test]
    fn a_cmd_or_control_entry_refused_for_its_epoch_is_not_a_duty_entry() {
        let mut log = HandLog::new(SEED);
        log.set(0, 1);
        let (added, laptop) = log.pair(9);
        let m1 = log.membership();
        log.rotate();
        let set = Body::Cmd(ToyMachine::new(true).encode(&ToyCommand::Set { key: 2, value: 2 }));
        let (cmd, cmd_id) = log.laptop_hold(&m1, set);
        let q1 = log.laptop_append(cmd.clone());
        let (ctl, ctl_id) = log.laptop_hold(&m1, Body::Ctl(CtlIntent::RulesUpgrade { rules: 2, mode: crate::UpgradeMode::Normal }));
        let q2 = log.laptop_append(ctl.clone());
        log.set(1, 1);
        let mut c = Case::of(&log);
        let led = check::ledger(&c.reference, Some(&c.template), &c.roots);
        for (q, eid, kind) in [(q1, cmd_id, Kind::Cmd), (q2, ctl_id, Kind::Ctl)] {
            let l = &led[q as usize - 1];
            assert_eq!((l.class, l.code, l.kind, l.entry_id), ("refuse", Some("seal/stale_epoch"), Some(kind), Some(eid)), "Laptop's {kind:?} entry is refused for its epoch");
        }
        let (h1, h2): (Hash32, Hash32) = (Sha256::digest(&cmd).into(), Sha256::digest(&ctl).into());
        c.idents = vec![settled(log.desk_dev(), 0, Vec::new()), settled(laptop, added, Vec::new())];
        c.acks = vec![
            AckFact { eid: cmd_id, ident: 1, sel: Sel::Main(0), seq: q1, frame: h1, line: 10 },
            AckFact { eid: ctl_id, ident: 1, sel: Sel::Main(0), seq: q2, frame: h2, line: 10 },
        ];
        c.acked = vec![(cmd_id, 1, 11, true), (ctl_id, 1, 11, true)];
        c.frames = BTreeMap::from([(h1, (cmd_id, Kind::Cmd, false)), (h2, (ctl_id, Kind::Ctl, false))]);
        c.entry_ops = BTreeMap::from([(ctl_id, (1, 33))]);
        c.enqueued = vec![(cmd_id, 1, 1, 12)];
        ack_later(&mut c, 1, 20, Kind::Cmd, [0xc1; 16]);
        ack_later(&mut c, 1, 20, Kind::Ctl, [0xc2; 16]);
        let tail = "and its device reported no refusal record, PairFailed of its pairing or end of its operation for it";
        assert_eq!(
            c.check(),
            vec![
                (2, 11, format!("dev 1: acknowledged entry {} is accepted 0 times in the reference log (classes there: [\"refuse\"]): finally refused at {q1}, {tail}", check::hexs(&cmd_id))),
                (2, 11, format!("dev 1: acknowledged entry {} is accepted 0 times in the reference log (classes there: [\"refuse\"]): an operation's entry, finally refused at {q2}, {tail}", check::hexs(&ctl_id))),
                (3, 12, format!(
                    "dev 1: enqueued entry {} is accepted 0 times and not held or refused (in its outbox: false): acknowledged at {q1} (trace line 10) by a relay state no rollback or fork removed",
                    check::hexs(&cmd_id)
                )),
            ]
        );
    }

    // Nothing wider: a `snap` entry refused with a code that is not
    // re-sealable (`seal/bad_payload`: its blob's epoch is not its frame's,
    // `seal.md` §11.5) still needs its refusal record (ENG-REF-001), though
    // its duty went on in every way the run records.
    #[test]
    fn a_duty_entry_refused_with_another_code_still_needs_its_refusal_record() {
        let mut log = HandLog::new(SEED);
        for k in 0..66u8 {
            log.set(k % 8, i64::from(k));
        }
        let (added, laptop) = log.pair(9);
        let m = log.membership();
        let (frame, eid) = log.laptop_hold(&m, HandLog::snap_body(64, 7));
        let q = log.laptop_append(frame.clone());
        let (cover, _) = log.snap(64);
        log.set(1, 1);
        let mut c = Case::of(&log);
        let led = check::ledger(&c.reference, Some(&c.template), &c.roots);
        let l = &led[q as usize - 1];
        assert_eq!((l.class, l.code, l.kind), ("refuse", Some("seal/bad_payload"), Some(Kind::Snap)), "Laptop's snapshot is refused for its payload");
        let fh: Hash32 = Sha256::digest(&frame).into();
        let mut x = settled(laptop, added, Vec::new());
        x.snap_duty = vec![(5, Some((64, WALL0, 2))), (12, None)];
        x.log_reach = cover;
        c.idents = vec![settled(log.desk_dev(), 0, Vec::new()), x];
        c.acks = vec![AckFact { eid, ident: 1, sel: Sel::Main(0), seq: q, frame: fh, line: 10 }];
        c.acked = vec![(eid, 1, 11, true)];
        c.frames = BTreeMap::from([(fh, (eid, Kind::Snap, true))]);
        c.snap_seqs = BTreeMap::from([(eid, 64)]);
        ack_later(&mut c, 1, 20, Kind::Snap, [0xa2; 16]);
        assert_eq!(
            c.check(),
            vec![(2, 11, format!(
                "dev 1: acknowledged entry {} is accepted 0 times in the reference log (classes there: [\"refuse\"]): finally refused at {q}, and its device reported no refusal record, PairFailed of its pairing or end of its operation for it",
                check::hexs(&eid)
            ))]
        );
        c.idents[1].refusal_seqs = BTreeSet::from([q]);
        assert_eq!(c.check(), Vec::new(), "with its refusal record");
    }

    // "The reference log holds it once": Laptop's beat, refused
    // `seal/old_epoch` at `q`, whose entry id a later frame of Desk's carries
    // too (refused `seal/bad_payload` at `q2`), is held twice; a later beat
    // of Laptop's does not make it a duty that went on.
    #[test]
    fn a_duty_entry_the_reference_log_holds_twice_is_not_one_whose_duty_went_on() {
        let mut log = HandLog::new(SEED);
        for k in 0..66u8 {
            log.set(k % 8, i64::from(k));
        }
        let (added, laptop) = log.pair(9);
        let m1 = log.membership();
        let (frame, eid) = log.laptop_hold(&m1, HandLog::beat_body());
        log.rotate();
        log.rotate();
        let q = log.laptop_append(frame.clone());
        let q2 = log.desk_seal_as(eid, HandLog::snap_body(64, 7));
        log.set(1, 1);
        let mut c = Case::of(&log);
        let led = check::ledger(&c.reference, Some(&c.template), &c.roots);
        let (l, l2) = (&led[q as usize - 1], &led[q2 as usize - 1]);
        assert_eq!((l.class, l.code, l.entry_id), ("refuse", Some("seal/old_epoch"), None), "Laptop's beat is refused unopened");
        assert_eq!((l2.class, l2.code, l2.entry_id), ("refuse", Some("seal/bad_payload"), Some(eid)), "Desk's frame of the same entry id is refused");
        let fh: Hash32 = Sha256::digest(&frame).into();
        c.idents = vec![settled(log.desk_dev(), 0, Vec::new()), settled(laptop, added, Vec::new())];
        c.acks = vec![AckFact { eid, ident: 1, sel: Sel::Main(0), seq: q, frame: fh, line: 10 }];
        c.acked = vec![(eid, 1, 11, true)];
        c.frames = BTreeMap::from([(fh, (eid, Kind::Beat, true))]);
        ack_later(&mut c, 1, 20, Kind::Beat, [0xb3; 16]);
        assert_eq!(
            c.check(),
            vec![(2, 11, format!(
                "dev 1: acknowledged entry {} is accepted 0 times in the reference log (classes there: [\"refuse\"]): finally refused at {q2}, and its device reported no refusal record, PairFailed of its pairing or end of its operation for it",
                check::hexs(&eid)
            ))]
        );
    }
}
