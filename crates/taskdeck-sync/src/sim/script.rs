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

use std::collections::BTreeSet;

use sha2::{Digest, Sha256};

use super::check::{self, BranchLog, Facts, IdentFacts};
use super::relay::{Cred, LogRow, ModelRelay};
use super::world::WALL0;
use crate::fake::{FakeMembership, FakeSealer};
use crate::toy::{ToyCommand, ToyMachine};
use crate::{AppendResult, Body, ClassifyInput, CtlIntent, Dev, Hash32, Head, PlainEntry, RelayTarget, Sealer, StateMachine};

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
        let mut log = HandLog { m: desk.membership_zero(), desk, template, relay, tgen, board, target, at: WALL0 + 1_000 };
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
        let me = self.desk.own_dev();
        let ctr = self.desk.view(&self.m).devices.iter().find(|d| d.dev == me).expect("Desk is a member").last_ctr + 1;
        let sealed = self.desk.seal(&self.m, ctr, &entry).expect("Desk seals");
        let cred = Cred::Token { dev: me, generation: self.tgen };
        (self.append(cred, sealed.frame), entry.entry_id)
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
    pub enqueued: Vec<([u8; 16], usize, u64, u64)>,
    pub registered: BTreeSet<[u8; 16]>,
    pub cancelled: BTreeSet<[u8; 16]>,
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
            enqueued: Vec::new(),
            registered: BTreeSet::new(),
            cancelled: BTreeSet::new(),
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
            enqueued: &self.enqueued,
            registered: &self.registered,
            cancelled: &self.cancelled,
            injected: &[],
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
        converges: false,
        live: true,
        halted_or_stopped_persisted: false,
        revoked: false,
        final_b: None,
        caught_up: true,
        joined_at,
        samples,
        crash_halts: BTreeSet::new(),
        applied: Vec::new(),
        alarms: Vec::new(),
        outbox: Vec::new(),
        refusal_seqs: BTreeSet::new(),
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
    use crate::sim::world::Deletion;

    const SEED: u64 = 41;

    /// The texts of the violations of invariant `inv`.
    fn of(v: &[(u8, u64, String)], inv: u8) -> Vec<String> {
        v.iter().filter(|x| x.0 == inv).map(|x| x.2.clone()).collect()
    }

    /// A made-up device id for a hand-built identity.
    fn dev(b: u8) -> Dev {
        [b; 16]
    }

    /// A board where the recovery identity skipped Desk's healthy entry `t`
    /// (`Set { key: 1, value: 99 }`) at `s`, and the board hash each `B` of
    /// `t..s` has without `t`: the oracle's of a twin log with the same
    /// commands and no `t` (at `B − 1` there).
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
        for (k, v) in [(2u8, 5i64), (4, 6)] {
            log.set(k, v);
            twin.set(k, v);
        }
        let (s, _) = log.skip(t);
        log.set(3, 1);
        let case = Case::of(&log);
        let oracle = case.oracle();
        let tw = Case::of(&twin).oracle();
        let without: BTreeMap<u64, Hash32> = (t..s).map(|b| (b, tw.get(&(b - 1)).expect("the twin's oracle").1)).collect();
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
        /// Samples at every B past genesis: the oracle's, except in `t..s`, where they lack `t`.
        fn lacking_t(&self) -> Vec<(u64, Hash32, Hash32, u64)> {
            let head = self.case.reference.len() as u64;
            let mut v = honest(&self.oracle, 2..=head);
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
    }

    // A board of honest devices holds every invariant: the checks raise
    // nothing a hand-built log does not put there (the base of every test
    // here).
    #[test]
    fn an_honest_board_holds_every_invariant() {
        let b = skip_board();
        let head = b.case.reference.len() as u64;
        let mut c = b.case;
        c.idents.push(settled(dev(0xd1), 1, honest(&b.oracle, 2..=head)));
        assert_eq!(c.check(), Vec::new());
    }

    // §31.7 invariant 4 (ruling 6 of the D2 brief, task D3 fix pass): a
    // device's confirmed state at every B is compared with the no-fault
    // oracle's only, a B between a Skip's target and the Skip included. A
    // device that crash-halted on the target `t` and, after the Skip cleared
    // the halt (ENG-HALT-004), confirmed the B's of `t..s` without `t`
    // (ENG-SKIP-002) differs from the oracle there: each such B is a
    // violation (no second expected value accepts it), and its text names
    // the spec question (api-simulator.md). Built by hand: in a run a crash
    // halt on a healthy entry and its Skip come only out of what the engine
    // does under crashes.
    #[test]
    fn invariant_4_at_a_skip_window_is_judged_by_the_no_fault_oracle_only() {
        let b = skip_board();
        let mut x = settled(dev(0xd2), 1, b.lacking_t());
        x.crash_halts.insert(b.t);
        let want: Vec<String> = (b.t..b.s)
            .map(|q| {
                format!(
                    "{}; it crash-halted on the target {}: ENG-HALT-004 against §31.7 invariant 4, a spec question (api-simulator.md)",
                    b.differs(q),
                    b.t
                )
            })
            .collect();
        let mut c = b.case;
        c.idents.push(x);
        let v = c.check();
        assert_eq!(of(&v, 4), want, "{v:?}");
        assert_eq!(v.len(), want.len(), "{v:?}");
    }

    // Invariant 4: a device that did not crash-halt on the Skip's target (it
    // applied it, or its crash halt was on another seq) is held to the
    // oracle's state in `t..s` like everywhere else, and its violation names
    // no spec question.
    #[test]
    fn a_device_that_applied_the_skip_target_is_held_to_the_oracle() {
        let b = skip_board();
        let x = settled(dev(0xd2), 1, b.lacking_t());
        let mut y = settled(dev(0xd3), 1, b.lacking_t());
        y.crash_halts.insert(b.t - 1);
        let want: Vec<String> = (b.t..b.s).map(|q| b.differs(q)).collect();
        let want_y: Vec<String> = want.iter().map(|w| w.replacen("dev 0", "dev 1", 1)).collect();
        let mut c = b.case;
        c.idents.extend([x, y]);
        let v = c.check();
        assert_eq!(of(&v, 4), [want, want_y].concat(), "{v:?}");
        assert_eq!(v.len(), 2 * (b.s - b.t) as usize, "{v:?}");
    }

    // Invariant 4 (ruling 6): a wrong state sampled between a Skip's target
    // and the Skip is reported and names the Skip; one elsewhere is reported
    // without it.
    #[test]
    fn a_wrong_state_between_a_skip_and_its_target_breaks_tampering() {
        let b = skip_board();
        let head = b.case.reference.len() as u64;
        let mut samples = honest(&b.oracle, 2..=head);
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

    // Invariant 4 (ruling 6, the D3 check's open finding): nothing between a
    // Skip's target and the Skip goes uncompared. A device that confirmed a
    // B there on the reference chain where the oracle confirmed nothing is a
    // violation. The oracle confirms nothing at a B it never reaches: here
    // it halts on a second poison entry `t2` inside the window of the Skip
    // of `t1` (ENG-POISON-003: two caught panics; the Skip of `t1` clears
    // only the halt on `t1`, ENG-HALT-004).
    #[test]
    fn a_b_in_a_skip_window_the_oracle_never_confirmed_is_a_violation() {
        let mut log = HandLog::new(SEED);
        for k in 0..3u8 {
            log.set(k, 10 + i64::from(k));
        }
        let (t1, _) = log.cmd(&ToyCommand::Panic);
        log.set(1, 5);
        let (t2, _) = log.cmd(&ToyCommand::Panic);
        log.set(2, 5);
        let (s, _) = log.skip(t1);
        let mut c = Case::of(&log);
        let o = c.oracle();
        let led = check::ledger(&c.reference, Some(&c.template), &c.roots);
        assert!(led.iter().any(|l| l.seq == s && l.accept && l.skip_targets == vec![t1]), "the Skip of {t1} is accepted at {s}");
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

    // §28.4 step 4 (task D3): a pairing's `DeviceAdded` that `register`
    // acknowledged and the board then refused is named as such. A refused
    // frame carries no items (`Opened::items` is filled on accept), so the
    // check tells it by the `register` that acknowledged it. Here the second
    // `DeviceAdded` of one device is refused (`seal/ctl_duplicate_key`); an
    // acknowledged refused entry no `register` acknowledged is not named a
    // pairing.
    #[test]
    fn a_refused_entry_register_acknowledged_is_named_a_pairing() {
        let mut log = HandLog::new(SEED);
        log.set(0, 1);
        let (a, first, _) = log.add_device(9);
        let (r, again, _) = log.add_device(9);
        log.set(1, 2);
        let mut c = Case::of(&log);
        let led = check::ledger(&c.reference, Some(&c.template), &c.roots);
        assert!(led[a as usize - 1].accept, "the first DeviceAdded is accepted");
        let l = &led[r as usize - 1];
        assert_eq!(l.class, "refuse", "the second DeviceAdded of one device is refused");
        assert!(l.skip_targets.is_empty() && l.added.is_empty(), "a refused entry carries no items");
        c.acked = vec![(first, 0, 5, true), (again, 0, 6, true)];
        c.registered = [first, again].into();
        let v = c.check();
        let base = format!("dev 0: acknowledged entry {} is accepted 0 times in the reference log (classes there: [\"refuse\"])", check::hexs(&again));
        assert_eq!(v, vec![(2, 6, format!("{base}: a pairing's DeviceAdded refused after register (§28.4 step 4)"))]);
        c.registered = [first].into();
        assert_eq!(c.check(), vec![(2, 6, base)]);
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

    // Task D3: a device whose pairing only a forked (or rolled-back) relay
    // state holds is named in invariant 6 (`its pairing is only on …`); one
    // whose `DeviceAdded` is in the reference log is not. And a Skip refused
    // after the relay acknowledged it (another Skip of the same target came
    // first, `seal/ctl_target`) is not a pairing's `DeviceAdded` (§28.4 step 4).
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
        c.branches.push(BranchLog { rows: fork.rows(), why: check::WHY_FORK, line: 77 });
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
            vec![format!("dev 0: acknowledged entry {} is accepted 0 times in the reference log (classes there: [\"refuse\"])", check::hexs(&raced_id))],
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
    // epoch-2 frames unopened, so the entry only the fork sequenced would
    // not read as lost there. Every record a device applied there and lost
    // is named (invariants 2 and 3).
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
        c.branches.push(BranchLog { rows: fork_rows.clone(), why: check::WHY_FORK, line: 77 });
        let fo = check::oracle(&fork_rows, Some(&c.template), &c.roots, &c.board, &c.relay).0;
        c.idents.push(settled(fork.desk_dev(), 0, honest(&fo, 2..=q)));
        c.acked.push((lost, 0, 9, false));
        c.enqueued.push((lost, 0, 1, 8));
        let v = c.check();
        let fault = "(the forked copy of the relay, fault at trace line 77)";
        assert_eq!(
            v,
            vec![
                (2, 9, format!("dev 0: acknowledged entry {} is accepted 0 times in the reference log (classes there: []): lost to a rolled-back relay state {fault}", check::hexs(&lost))),
                (3, 8, format!("dev 0: enqueued entry {} is accepted 0 times and not held or refused (in its outbox: false): applied on a rolled-back relay state {fault}", check::hexs(&lost))),
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
        c.branches.push(BranchLog { rows: fork_rows.clone(), why: check::WHY_FORK, line: 77 });
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
        c.branches.push(BranchLog { rows: fork_rows.clone(), why: check::WHY_FORK, line: 77 });
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
}
