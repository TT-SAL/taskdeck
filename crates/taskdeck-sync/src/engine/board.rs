//! The board: applying classified entries to the confirmed state (§12.4, §13),
//! poison pills and markers (§15), what halts hold back (§16), refusal and
//! notice records (§13.3, §13.4), the registry (§7.3), re-sealing (§24.3),
//! state hashes, bases, checkpoints and retention (§18, §7.13).
//!
//! Rules: `pipe.queue` holds the classified entries `B+1..=K`. `pipe.batches`
//! is the plan: each `(first, last)` is applied only while the durable marker
//! (`pipe.marker`) is exactly `{first, last, meta.attempt}`; otherwise the
//! marker is written first and the board waits (ENG-POISON-001). A batch of one
//! entry is single mode. `pipe.pre` is the confirmed state as it was before the
//! commands in `pipe.redo` were applied: after a caught panic the state is
//! rebuilt from it and never taken from what the panic left (ENG-POISON-004).
//! Records the applies produce collect in `pipe.adds` and `pipe.drops` and are
//! written with `ClearMarker` when the batch is done (ENG-RECV-006).

use std::collections::{BTreeSet, VecDeque};
use std::marker::PhantomData;

use super::recv::{Entry, Flight, Stash};
use super::{After, Engine, guard, lim};
use crate::{
    AlarmKind, Body, Checkpoint, Class, CtlIntent, CtlItem, Ctx, Dev, Evidence, HaltKind, HashRecord, Hash32, Kind,
    LastEntry, Marker, NoticeKind, NoticeOp, Notification, OutboxOp, PersistOp, Report, Sealer, SmRefusal, SnapRecord,
    StateMachine, StopKind, Verdict, state_hash,
};

pub(crate) struct Pipe<SM: StateMachine, S: Sealer> {
    pub queue: VecDeque<Entry<S::Membership>>,
    pub flight: Option<Flight<S::Membership>>,
    pub stash: Option<Stash>,
    /// The frames a `NeedFrames` still wants, the fetch in progress `(req, seq, from the relay)`,
    /// the frames fetched and checked, and the authors of Skip targets seen so far.
    pub need: Vec<(u64, Hash32)>,
    pub need_req: Option<(u64, u64, bool)>,
    pub skip_frames: Vec<(u64, Vec<u8>)>,
    pub skip_authors: Vec<(u64, Dev)>,
    pub batches: VecDeque<(u64, u64)>,
    /// The marker as the durable store holds it.
    pub marker: Option<Marker>,
    /// The write of a marker the board waits for (0: none).
    pub marker_pid: u64,
    /// The marker `Start` found (§15.2), until the tail is planned.
    pub start_marker: Option<Marker>,
    pub pre: Option<SM::State>,
    pub redo: Vec<(SM::Command, Ctx)>,
    /// The seqs treated as skipped for the current confirmed state (§17.3 step 1).
    pub skipped: BTreeSet<u64>,
    /// Skips committed to the membership whose entry has not reached the board yet, as
    /// (target, seq of the Skip). A report for a seq at or above the target that arrives in an
    /// entry below the Skip describes the state before it (§17.3) and is not compared.
    pub skip_at: Vec<(u64, u64)>,
    pub adds: Vec<NoticeOp>,
    pub drops: Vec<OutboxOp>,
    pub since_ckpt: u32,
    pub ckpt_mono: u64,
    pub ckpt_b: u64,
    pub ckpt_pid: u64,
    pub pages_since_rebase: u32,
    /// The board changed since the last rebase.
    pub view_stale: bool,
    /// The tail of this `Start` is not yet classified and applied (ENG-STATE-002).
    pub tail: bool,
    /// The purge whose old roots wait for their thirty days: `(epoch, τ_r)` (§28.2 step 4).
    pub shred: Option<(u32, i64)>,
    pub shred_checked: bool,
    /// A purge rotation is being applied: `(r, p, epoch, τ_r)`, run once `B = r` (§28.2).
    pub purge_after: Option<(u64, u64, u32, i64)>,
    _p: PhantomData<SM>,
}

impl<SM: StateMachine, S: Sealer> Default for Pipe<SM, S> {
    fn default() -> Self {
        Pipe {
            queue: VecDeque::new(),
            flight: None,
            stash: None,
            need: Vec::new(),
            need_req: None,
            skip_frames: Vec::new(),
            skip_authors: Vec::new(),
            batches: VecDeque::new(),
            marker: None,
            marker_pid: 0,
            start_marker: None,
            pre: None,
            redo: Vec::new(),
            skipped: BTreeSet::new(),
            skip_at: Vec::new(),
            adds: Vec::new(),
            drops: Vec::new(),
            since_ckpt: 0,
            ckpt_mono: 0,
            ckpt_b: 0,
            ckpt_pid: 0,
            pages_since_rebase: 0,
            view_stale: false,
            tail: false,
            shred: None,
            shred_checked: false,
            purge_after: None,
            _p: PhantomData,
        }
    }
}

enum Step {
    Applied,
    Panicked,
    Blocked,
}

impl<SM: StateMachine, S: Sealer> Engine<SM, S> {
    // ------------------------------------------------------------------ the plan (§15.2)

    /// Plans the batches of a `Start`'s tail `first..=last` under the marker rules.
    pub(crate) fn plan_tail(&mut self, first: u64, last: u64) {
        let mut from = first.max(self.b + 1);
        if let Some(m) = self.pipe.start_marker.take()
            && m.last > self.b {
                if m.first == m.last {
                    // The previous attempt died while applying `x` alone.
                    let x = m.first;
                    if self.meta.last_failed == Some((x, m.attempt.wrapping_sub(1))) {
                        self.crash_halt(x);
                    } else {
                        self.meta.last_failed = Some((x, m.attempt));
                        self.touch_meta();
                    }
                }
                let mf = m.first.max(from);
                let ml = m.last.min(last);
                self.plan_pages(from, mf.saturating_sub(1));
                for s in mf..=ml {
                    self.pipe.batches.push_back((s, s));
                }
                from = ml + 1;
            }
        self.plan_pages(from, last);
    }

    fn plan_pages(&mut self, mut from: u64, last: u64) {
        while from <= last {
            let to = last.min(from + lim::PAGE_ENTRIES as u64 - 1);
            self.pipe.batches.push_back((from, to));
            from = to + 1;
        }
    }

    /// Halts on `x` (§16.1 `Crash`, `engine/crash_halt`).
    fn crash_halt(&mut self, x: u64) {
        let (author, frame_hash) =
            self.pipe.queue.iter().find(|e| e.seq == x).map(|e| (e.author, e.frame_hash)).unwrap_or(([0; 16], [0; 32]));
        self.flush_batch_records(false);
        self.halt(HaltKind::Crash { frame_hash }, x, author, "engine/crash_halt");
        self.rebase_due = true;
    }

    // ------------------------------------------------------------------ the apply loop

    /// Does a halt or a stop hold the board at `B` (§16, §19.9, §21.4)?
    pub(crate) fn board_held(&self) -> bool {
        if self.dead {
            return true;
        }
        if let Some(h) = &self.meta.halt {
            match h.kind {
                HaltKind::Crash { .. } | HaltKind::Rules { .. } => {
                    if self.b + 1 >= h.seq {
                        return true;
                    }
                }
                HaltKind::Commitment => {}
            }
        }
        self.meta.stops.iter().any(|s| match &s.kind {
            StopKind::LocalFault { .. } => true,
            StopKind::Rebootstrap { after } => self.b + 1 >= *after,
            _ => false,
        })
    }

    /// Applies what the plan allows (§12.4, §15).
    pub(crate) fn run_board(&mut self) {
        loop {
            if self.board_held() || self.jobs.holds_board() {
                break;
            }
            let Some(&(first, last)) = self.pipe.batches.front() else { break };
            if self.b >= last {
                self.pipe.batches.pop_front();
                continue;
            }
            let want = Marker { first, last, attempt: self.meta.attempt };
            if self.pipe.marker != Some(want) {
                if self.pipe.marker_pid == 0 || self.is_durable(self.pipe.marker_pid) {
                    // D5, D6: the attempt and `last_failed` are written ahead of the marker, in one write.
                    let mut ops = Vec::new();
                    let with_meta = self.meta_dirty;
                    if with_meta {
                        self.meta_dirty = false;
                        ops.push(PersistOp::WriteMeta(self.meta.clone()));
                    }
                    ops.push(PersistOp::SetMarker(want));
                    let pid = self.persist(ops);
                    self.pipe.marker_pid = pid;
                    if with_meta {
                        self.meta_pid = pid;
                    }
                    if pid == 0 {
                        break;
                    }
                }
                break;
            }
            if self.pipe.pre.is_none() {
                self.pipe.pre = self.state.clone();
                self.pipe.redo.clear();
            }
            let mut done = true;
            let mut panicked = false;
            while self.b < last {
                match self.apply_next() {
                    Step::Applied => {}
                    Step::Panicked => {
                        self.on_apply_panic(first, last);
                        done = false;
                        panicked = true;
                        break;
                    }
                    Step::Blocked => {
                        done = false;
                        break;
                    }
                }
            }
            if let Some(panicked) = done.then_some(()).map_or(Some(panicked), |()| None) {
                // A panic changed the plan (single mode follows); anything else waits.
                if panicked && !self.board_held() {
                    continue;
                }
                break;
            }
            // ENG-RECV-006, ENG-POISON-005: the records, the drops and ClearMarker in one write.
            if first == last && self.meta.last_failed.is_some_and(|f| f.0 == first) {
                self.meta.last_failed = None;
                self.touch_meta();
            }
            self.flush_batch_records(true);
            self.pipe.batches.pop_front();
            self.pipe.pre = None;
            self.pipe.redo.clear();
        }
        if self.pipe.tail && self.pipe.stash.is_none() && self.pipe.flight.is_none()
            && (self.pipe.batches.is_empty() || self.board_held()) {
                self.pipe.tail = false;
                self.rebase_due = true;
            }
    }

    /// The notices and outbox `Drop`s of the entries applied so far in a batch, written before a
    /// checkpoint or base that covers them; the batch's `ClearMarker` still comes at its end.
    fn flush_settled(&mut self) {
        if !self.pipe.adds.is_empty() || !self.pipe.drops.is_empty() {
            self.flush_batch_records(false);
        }
    }

    /// Writes the notice and refusal records and the outbox drops the applies
    /// produced, with `ClearMarker` when the marker's entries are all applied.
    fn flush_batch_records(&mut self, clear: bool) {
        let adds = std::mem::take(&mut self.pipe.adds);
        let drops = std::mem::take(&mut self.pipe.drops);
        let (mut notices, mut refusals) = (0u32, 0u32);
        for a in &adds {
            if let NoticeOp::Add { kind, .. } = a {
                match kind {
                    NoticeKind::Notice => notices += 1,
                    NoticeKind::Refusal => refusals += 1,
                }
            }
        }
        let mut ops = Vec::new();
        if !adds.is_empty() {
            ops.push(PersistOp::Notices(adds));
        }
        if !drops.is_empty() {
            ops.push(PersistOp::Outbox(drops));
        }
        if clear {
            ops.push(PersistOp::ClearMarker);
        }
        let pid = self.persist(ops);
        if notices + refusals > 0 {
            self.after(pid, After::Notify(Notification::NoticesAdded { notices, refusals }));
            self.trim_notices();
        }
        self.maybe_rewrite_outbox();
    }

    /// §15.3: `apply` panicked on the queue's front entry `x` in this attempt.
    fn on_apply_panic(&mut self, first: u64, last: u64) {
        let x = self.b + 1;
        let a = self.meta.attempt;
        // ENG-POISON-004: the state is rebuilt, never taken from what the panic left.
        if let Some(pre) = self.pipe.pre.clone() {
            let mut state = pre;
            let sm = &self.sm;
            for (c, ctx) in &self.pipe.redo {
                let _ = guard(|| sm.apply(&mut state, c, ctx));
            }
            self.state = Some(state);
        }
        if self.meta.last_failed == Some((x, a.wrapping_sub(1))) {
            // ENG-POISON-003: failed in two consecutive attempts.
            self.crash_halt(x);
            return;
        }
        self.meta.last_failed = Some((x, a));
        self.meta.attempt = a + 1;
        self.touch_meta();
        // `x` alone next, then the rest of the batch page-wise.
        self.pipe.batches.pop_front();
        if x < last {
            self.pipe.batches.push_front((x + 1, last));
        }
        self.pipe.batches.push_front((x, x));
        let _ = first;
    }

    /// Applies the queue's front entry to the board (§12.4 steps 1 to 4).
    fn apply_next(&mut self) -> Step {
        let Some(e) = self.pipe.queue.pop_front() else { return Step::Blocked };
        let s = e.seq;
        debug_assert_eq!(s, self.b + 1);
        let record_ok = s > self.notices_through;
        let adds_before = self.pipe.adds.len();
        let own_no = self.no_b.or_else(|| self.me().map(|d| d.device_no));
        let skipped = self.vk.skips.iter().any(|(t, _)| *t == s);
        // What the entry did, for the own record: None = accepted and applied (or benign).
        let mut refused: Option<SmRefusal> = None;
        let mut resealable = false;
        let mut accepted_kind: Option<Kind> = None;
        let plain = |code: &'static str| SmRefusal { code, op: None, target: None, base: None, current: None };
        if skipped {
            // Step 1: a Skip names it: treated as refused, nothing applied.
            self.pipe.skipped.insert(s);
            let mut r = plain("engine/skipped");
            if let Some(o) = e.opened.as_ref().filter(|o| o.kind == Kind::Cmd) {
                let sm = &self.sm;
                if let Some(Ok(c)) = guard(|| sm.parse(&o.payload)) {
                    r.op = Some(sm.op_name(&c));
                    r.target = sm.target(&c);
                }
            }
            refused = Some(r);
        } else {
            match e.class {
                Class::Skip | Class::Tamper | Class::NeedFrames(_) => {}
                Class::Refuse => {
                    let code = e.code.unwrap_or("seal/refused");
                    let kind = e.opened.as_ref().map(|o| o.kind);
                    resealable = e.own
                        && (code == "seal/stale_epoch" || code == "seal/old_epoch")
                        && kind.is_none_or(|k| k == Kind::Cmd);
                    refused = Some(plain(code));
                }
                Class::Accept => {
                    let Some(o) = e.opened.as_ref() else {
                        self.pipe.queue.push_front(e);
                        self.fault("engine/local_fault");
                        return Step::Blocked;
                    };
                    accepted_kind = Some(o.kind);
                    match o.kind {
                        Kind::Cmd => {
                            let sm = &self.sm;
                            match guard(|| sm.parse(&o.payload)) {
                                None => {
                                    self.pipe.queue.push_front(e);
                                    self.fault("engine/local_fault");
                                    return Step::Blocked;
                                }
                                Some(Err(err)) => {
                                    // ENG-SM-003: a refusal on every device, with an alarm naming the author.
                                    self.alarm(
                                        AlarmKind::RefusedFrame,
                                        Some(s),
                                        Some(e.author),
                                        Some(err.code),
                                        Evidence::Frame { frame_hash: None },
                                    );
                                    refused = Some(plain(err.code));
                                }
                                Some(Ok(cmd)) => {
                                    let ctx = Ctx {
                                        seq: s,
                                        device_no: e.author_no.unwrap_or(0),
                                        at_ms: o.at_ms,
                                        based_on: o.based_on.seq,
                                    };
                                    let Some(state) = self.state.as_mut() else { return Step::Blocked };
                                    match guard(|| sm.apply(state, &cmd, &ctx)) {
                                        None => {
                                            self.pipe.queue.push_front(e);
                                            return Step::Panicked;
                                        }
                                        Some(Ok(applied)) => {
                                            if record_ok
                                                && let Some(no) = own_no {
                                                    for n in &applied.notices {
                                                        if sm.notice_concerns(n, no) {
                                                            let record = sm.notice_bytes(n);
                                                            self.pipe.adds.push(NoticeOp::Add {
                                                                kind: NoticeKind::Notice,
                                                                seq: s,
                                                                record,
                                                            });
                                                        }
                                                    }
                                                }
                                            self.pipe.redo.push((cmd, ctx));
                                        }
                                        Some(Err(r)) => refused = Some(r),
                                    }
                                }
                            }
                        }
                        Kind::Ctl => {
                            if !self.apply_ctl(&e) {
                                self.pipe.queue.push_front(e);
                                return Step::Blocked;
                            }
                        }
                        Kind::Beat => self.apply_beat(&e),
                        Kind::Snap => self.apply_snap(&e),
                    }
                    if e.unchecked && !e.from_log {
                        // §13.1: counted once, when first applied.
                        self.meta.based_on_unchecked = self.meta.based_on_unchecked.saturating_add(1);
                        self.touch_meta();
                    }
                }
            }
        }
        // Step 3: the own record.
        if e.own {
            self.settle_own(&e, refused.as_ref(), resealable, record_ok, own_no);
        }
        // Step 4: the board head.
        self.b = s;
        self.cb = e.chain;
        if accepted_kind.is_some() && !skipped
            && let Some(o) = &e.opened {
                self.note_last_entry(e.author, o.at_ms, s, o.based_on.seq);
            }
        if accepted_kind == Some(Kind::Ctl) || self.no_b.is_none() {
            let v = self.sealer.view(&e.m);
            self.no_b = v.devices.iter().find(|d| d.dev == self.own).map(|d| d.device_no);
        }
        let tau = e.tau;
        self.mb = Some(e.m);
        self.pipe.since_ckpt += 1;
        self.pipe.view_stale = true;
        self.status_dirty = true;
        if self.pipe.adds.len() > adds_before {
            for a in &self.pipe.adds[adds_before..] {
                if let NoticeOp::Add { kind, seq, record } = a {
                    self.notices.push(super::NoticeRow { kind: *kind, seq: *seq, record: record.clone(), dismissed: false });
                }
            }
            self.notices_through = s;
        }
        if s >= lim::STRIDE && s % lim::STRIDE == 0 {
            self.aligned(s, tau);
        }
        if let Some((r, p, epoch, tau_r)) = self.pipe.purge_after.take() {
            self.local_purge(r, p, epoch, tau_r);
        }
        Step::Applied
    }

    /// §12.4 step 3 and §13.3, §24.3: what happens to the record of an own entry
    /// that reached the board head.
    fn settle_own(
        &mut self,
        e: &Entry<S::Membership>,
        refused: Option<&SmRefusal>,
        resealable: bool,
        record_ok: bool,
        own_no: Option<u16>,
    ) {
        let idx = match e.opened.as_ref().and_then(|o| self.rec_index(&o.entry_id)) {
            Some(i) => Some(i),
            None => self.outbox.iter().position(|r| r.r.ctr == Some(e.ctr) && r.r.frame_hash == Some(e.frame_hash)),
        };
        let Some(idx) = idx else {
            // The record left earlier (a restart above an older checkpoint): only the refusal record is owed.
            if let (Some(r), true, Some(no)) = (refused, record_ok && !resealable, own_no)
                && r.code != "seal/stale_epoch" && r.code != "seal/old_epoch" {
                    self.add_refusal(e.seq, no, r);
                }
            return;
        };
        let kind = self.outbox[idx].r.kind;
        let mut final_refusal = refused.cloned();
        if resealable && refused.is_some() {
            if self.outbox[idx].r.reseals >= 3 {
                // §24.3: after its third re-seal is refused.
                final_refusal = Some(SmRefusal {
                    code: "engine/reseal_limit",
                    op: None,
                    target: None,
                    base: None,
                    current: None,
                });
            } else {
                self.reseal_from(idx);
                return;
            }
        }
        if let Some(mut r) = final_refusal {
            if kind == Kind::Cmd && r.op.is_none()
                && let Some(c) = self.outbox[idx].cmd.as_ref() {
                    r.op = Some(self.sm.op_name(c));
                    r.target = self.sm.target(c);
                }
            // ENG-REF-001: one refusal record for every own entry refused for a reason that is
            // not re-sealable, commands and the engine's own entries alike.
            let resealable_code = r.code == "seal/stale_epoch" || r.code == "seal/old_epoch";
            if let (true, Some(no)) = (record_ok && !resealable_code, own_no) {
                self.add_refusal(e.seq, no, &r);
            }
            let rec = self.outbox.remove(idx);
            self.own_entry_done(&rec.r, e.seq, Some(r.code));
            if matches!(&rec.r.body, Body::Ctl(CtlIntent::Rotate { .. })) {
                // ENG-CTL-002: the pending root of a refused own rotation goes, and the identity is written.
                if self.sealer.discard_pending(&rec.r.entry_id) {
                    let id = self.sealer.export_identity();
                    self.send.identity_pid = self.persist(vec![PersistOp::WriteIdentity(id)]);
                }
            }
            self.pipe.drops.push(OutboxOp::Drop(rec.r.entry_id));
            if kind == Kind::Cmd {
                // §8.4 case 4: it left the outbox other than by being applied.
                self.rebase_due = true;
            }
        } else {
            let rec = self.outbox.remove(idx);
            self.own_entry_done(&rec.r, e.seq, None);
            self.pipe.drops.push(OutboxOp::Drop(rec.r.entry_id));
        }
        self.status_dirty = true;
    }

    fn add_refusal(&mut self, seq: u64, no: u16, r: &SmRefusal) {
        let sm = &self.sm;
        if let Some(record) = guard(|| sm.refusal_bytes(seq, no, r)) {
            self.pipe.adds.push(NoticeOp::Add { kind: NoticeKind::Refusal, seq, record });
        }
    }

    /// §24.3: clears the record at `idx` and every later own record that is sealed
    /// and not yet seen, for sealing again in their order (ENG-RESEAL-001, D9).
    pub(crate) fn reseal_from(&mut self, idx: usize) {
        let mut puts = Vec::new();
        for (i, rec) in self.outbox.iter_mut().enumerate().skip(idx) {
            if i > idx && (rec.r.frame.is_none() || rec.r.seen.is_some()) {
                continue;
            }
            rec.r.ctr = None;
            rec.r.frame = None;
            rec.r.frame_hash = None;
            rec.r.epoch = None;
            rec.r.ack = None;
            rec.r.seen = None;
            rec.r.reseals = rec.r.reseals.saturating_add(1);
            puts.push(OutboxOp::Put(rec.r.clone()));
        }
        let pid = self.persist(vec![PersistOp::Outbox(puts)]);
        for rec in self.outbox.iter_mut().skip(idx) {
            if rec.r.frame.is_none() {
                rec.pid = rec.pid.max(pid);
            }
        }
        self.send.clear_pid = pid;
        self.rebase_due = true;
        self.status_dirty = true;
    }

    // ------------------------------------------------------------------ control entries on the board (§14.1)

    /// The board effects of an accepted control entry. False: the board waits
    /// (a job runs, or a halt or stop holds it) and the entry stays at the front.
    fn apply_ctl(&mut self, e: &Entry<S::Membership>) -> bool {
        let Some(o) = e.opened.as_ref() else { return true };
        let s = e.seq;
        for item in &o.items {
            match item {
                CtlItem::ResetTo { seq: t, state_hash: h, .. } => {
                    if !self.jobs.take_done_reset(s) {
                        return self.begin_reset(s, *t, *h, e.author);
                    }
                }
                CtlItem::Skip { seq: t, .. } => {
                    let t = *t;
                    if self.pipe.skipped.contains(&t) || t >= s {
                        continue;
                    }
                    if self.jobs.take_done_skip(s, t) {
                        continue;
                    }
                    // A beat or `snap` entry: its reports and verdicts go; the board is unchanged.
                    let had = self.registry.snapshots.iter().any(|r| r.entry_seq == t)
                        || self.registry.reports.iter().any(|r| r.report_seq == t)
                        || self.registry.snapshots.iter().any(|r| r.verdicts.iter().any(|v| v.2 == t));
                    if had {
                        self.registry.snapshots.retain(|r| r.entry_seq != t);
                        self.registry.reports.retain(|r| r.report_seq != t);
                        for r in &mut self.registry.snapshots {
                            r.verdicts.retain(|v| v.2 != t);
                        }
                        self.pipe.skipped.insert(t);
                        continue;
                    }
                    return self.begin_skip_replay(s, t);
                }
                CtlItem::EpochRotated { epoch, purge_before: Some(p) } => {
                    self.pipe.purge_after = Some((s, *p, *epoch, e.tau))
                }
                _ => {}
            }
        }
        // ENG-SM-002: the version this entry activates, before any later entry.
        let v = self.sealer.view(&e.m).rules;
        let Some(state) = self.state.as_mut() else { return true };
        let sm = &self.sm;
        let current = guard(|| sm.rules(state));
        if current.is_some_and(|c| c != v) {
            match guard(|| sm.upgrade(state, v)) {
                Some(Ok(())) => {
                    self.mb = Some(e.m.clone());
                    self.after_state_swap(s, e.chain);
                }
                Some(Err(_)) => {
                    // §16.1 `Rules`: the board stays at `s - 1`.
                    self.halt(HaltKind::Rules { version: v }, s, e.author, "core/unknown_rules");
                    return false;
                }
                None => {
                    self.fault("engine/local_fault");
                    return false;
                }
            }
        }
        true
    }

    /// After the confirmed state was replaced or upgraded while applying `s`: the
    /// rebuild point moves, and a checkpoint and a rebase follow (§18.1 case 4, §8.4 case 3).
    pub(crate) fn after_state_swap(&mut self, _s: u64, _chain: Hash32) {
        self.pipe.pre = self.state.clone();
        self.pipe.redo.clear();
        self.duty.ckpt_wanted = true;
        self.rebase_due = true;
    }

    /// §19.9: the board resumes from a snapshot at `s`. The entries up to `s` are not
    /// applied to the state; the registry, the own records and the heads follow them.
    pub(crate) fn board_jump_to(&mut self, s: u64) {
        while self.pipe.queue.front().is_some_and(|e| e.seq <= s) {
            let Some(e) = self.pipe.queue.pop_front() else { break };
            if e.class == Class::Accept
                && let Some(o) = e.opened.as_ref() {
                    match o.kind {
                        Kind::Beat => self.apply_beat(&e),
                        Kind::Snap => self.apply_snap(&e),
                        _ => {}
                    }
                    self.note_last_entry(e.author, o.at_ms, e.seq, o.based_on.seq);
                }
            if e.own {
                let idx = e.opened.as_ref().and_then(|o| self.rec_index(&o.entry_id));
                if let Some(i) = idx {
                    let rec = self.outbox.remove(i);
                    self.own_entry_done(&rec.r, e.seq, None);
                    self.pipe.drops.push(OutboxOp::Drop(rec.r.entry_id));
                }
            }
            self.b = e.seq;
            self.cb = e.chain;
            let v = self.sealer.view(&e.m);
            self.no_b = v.devices.iter().find(|d| d.dev == self.own).map(|d| d.device_no);
            self.mb = Some(e.m);
        }
        self.flush_batch_records(false);
        while self.pipe.batches.front().is_some_and(|b| b.1 <= s) {
            self.pipe.batches.pop_front();
        }
        if let Some(b) = self.pipe.batches.front_mut() {
            b.0 = b.0.max(s + 1);
        }
        self.pipe.skipped.clear();
        self.status_dirty = true;
    }

    // ------------------------------------------------------------------ the registry (§7.3, §19.5, §23.3)

    fn note_last_entry(&mut self, dev: Dev, at_ms: i64, seq: u64, based_on_seq: u64) {
        let v = &mut self.registry.last_entry;
        match v.binary_search_by(|x| x.dev.cmp(&dev)) {
            Ok(i) => v[i] = LastEntry { dev, at_ms, seq, based_on_seq },
            Err(i) => v.insert(i, LastEntry { dev, at_ms, seq, based_on_seq }),
        }
    }

    /// The latest report of a device for an aligned seq (§22.1); at most 1,024 kept.
    pub(crate) fn add_report(&mut self, s: u64, dev: Dev, hash: Hash32, hash_version: u32, report_seq: u64) {
        let v = &mut self.registry.reports;
        let rep = Report { seq: s, dev, hash, hash_version, report_seq };
        match v.binary_search_by(|x| (x.seq, x.dev).cmp(&(s, dev))) {
            Ok(i) => v[i] = rep,
            Err(i) => v.insert(i, rep),
        }
        if v.len() > 1024 {
            v.remove(0);
        }
    }

    /// §17.3, ENG-SKIP-003: does a report for the seq `at`, carried by the entry at `e`, describe
    /// the state before a Skip this device already knows of?
    fn before_skip(&self, at: u64, e: u64) -> bool {
        self.pipe.skip_at.iter().any(|(t, s)| *t <= at && e < *s)
    }

    fn apply_beat(&mut self, e: &Entry<S::Membership>) {
        let Some(beat) = e.opened.as_ref().and_then(|o| o.beat.as_ref()) else { return };
        let s = e.seq;
        if let Some((at, hash, hv)) = beat.state
            && !self.before_skip(at, s)
        {
            self.add_report(at, e.author, hash, hv, s);
            self.check_divergence(at);
        }
        for (entry_seq, verdict) in &beat.acks {
            if let Some(r) = self.registry.snapshots.iter_mut().find(|r| r.entry_seq == *entry_seq) {
                if r.author == e.author {
                    continue; // the writer never judges its own snapshot
                }
                match r.verdicts.binary_search_by(|x| x.0.cmp(&e.author)) {
                    Ok(i) => r.verdicts[i] = (e.author, *verdict, s),
                    Err(i) if r.verdicts.len() < 16 => r.verdicts.insert(i, (e.author, *verdict, s)),
                    Err(_) => {}
                }
            }
        }
        if e.own {
            // §23.2: the verdicts this beat carried are sent.
            let sent: Vec<u64> = beat.acks.iter().map(|a| a.0).collect();
            self.registry.pending_verdicts.retain(|v| !sent.contains(&v.0));
        }
    }

    fn apply_snap(&mut self, e: &Entry<S::Membership>) {
        let Some(info) = e.opened.as_ref().and_then(|o| o.snap.clone()) else { return };
        let s = e.seq;
        let mut rec =
            SnapRecord { entry_seq: s, author: e.author, info: info.clone(), entry_clock: e.tau, verdicts: Vec::new(), own: None };
        // §19.5: this device's verdict, when it holds its own hash at that seq.
        if e.author != self.own
            && !self.before_skip(info.seq, s)
            && let Some(h) = self.hashes.get(&info.seq).filter(|h| h.hash_version == info.hash_version) {
                let verdict = if h.hash == info.hash { Verdict::Ack } else { Verdict::Mismatch };
                rec.own = Some(verdict);
                let own_hash = h.hash;
                let pv = &mut self.registry.pending_verdicts;
                if !pv.iter().any(|v| v.0 == s) {
                    pv.push((s, verdict));
                    pv.sort_by_key(|v| v.0);
                    if pv.len() > 64 {
                        pv.remove(0);
                    }
                }
                if verdict == Verdict::Mismatch {
                    let mut groups = vec![(own_hash, vec![self.own]), (info.hash, vec![e.author])];
                    groups.sort();
                    self.alarm(
                        AlarmKind::SnapshotMismatch,
                        Some(info.seq),
                        Some(e.author),
                        None,
                        Evidence::Mismatch { groups },
                    );
                    self.add_report(info.seq, e.author, info.hash, info.hash_version, s);
                    self.check_divergence(info.seq);
                }
            }
        let v = &mut self.registry.snapshots;
        v.retain(|r| r.entry_seq != s);
        v.push(rec);
        v.sort_by_key(|r| r.entry_seq);
        while v.len() > 64 {
            v.remove(0);
        }
        self.snap_accepted(s, e.author, &info);
    }

    // ------------------------------------------------------------------ aligned seqs: hashes and bases (§18.2, §18.3)

    /// The state hash of the confirmed state with `M_B` (§18.3).
    pub(crate) fn own_state_hash(&self) -> Option<Hash32> {
        let state = self.state.as_ref()?;
        let m = self.mb.as_ref()?;
        let sm = &self.sm;
        let bh = guard(|| sm.board_hash(state))?;
        Some(state_hash(1, &bh, &self.sealer.membership_bytes(m)))
    }

    /// After applying an aligned `S`: the hash record, a base when due, the
    /// snapshot duty check (ENG-HASH-002, ENG-CKPT-003).
    fn aligned(&mut self, s: u64, tau: i64) {
        let Some(hash) = self.own_state_hash() else {
            self.fault("engine/local_fault");
            return;
        };
        let rec = HashRecord { seq: s, hash_version: 1, hash };
        self.hashes.insert(s, rec);
        self.persist(vec![PersistOp::AppendHashes(vec![rec])]);
        let snapshot_due = self.snapshot_due_at(s, tau);
        let newest = self.bases.last().map(|b| b.0);
        let base_due = match newest {
            None => true,
            Some(n) => {
                let t = self.tau_at(n).or_else(|| self.tau_at(n + 1)).unwrap_or(i64::MIN);
                tau.saturating_sub(t) >= lim::WEEK
            }
        };
        if base_due || snapshot_due {
            self.write_base();
            self.retention();
        }
        self.check_divergence(s);
    }

    /// The checkpoint record of the confirmed state at `B` (§7.3).
    pub(crate) fn checkpoint(&self) -> Option<Checkpoint> {
        let state = self.state.as_ref()?;
        let m = self.mb.as_ref()?;
        let sm = &self.sm;
        let board = guard(|| sm.canonical_bytes(state))?;
        Some(Checkpoint {
            board_seq: self.b,
            board_chain: self.cb,
            board,
            membership: self.sealer.membership_bytes(m),
            log_floor: self.floor,
            registry: self.registry.clone(),
        })
    }

    /// Writes a base at `B` (§18.2) and returns its write, 0 when it could not.
    pub(crate) fn write_base(&mut self) -> u64 {
        if self.bases.iter().any(|b| b.0 == self.b) {
            return self.last_pid;
        }
        self.flush_settled();
        let Some(c) = self.checkpoint() else {
            self.fault("engine/local_fault");
            return 0;
        };
        self.bases.push((self.b, self.cb));
        self.bases.sort();
        self.persist(vec![PersistOp::WriteBase(c)])
    }

    /// Writes the checkpoint at `B` (§18.1) and returns its write.
    pub(crate) fn write_checkpoint(&mut self) -> u64 {
        // Nothing at or below a checkpoint is settled again after a `Start`: the notices and the
        // outbox `Drop`s of the entries already applied go first (§7.12 D12, D13).
        self.flush_settled();
        let Some(c) = self.checkpoint() else {
            self.fault("engine/local_fault");
            return 0;
        };
        let pid = self.persist(vec![PersistOp::WriteCheckpoint(c)]);
        self.pipe.since_ckpt = 0;
        self.pipe.ckpt_mono = self.mono;
        self.pipe.ckpt_b = self.b;
        self.pipe.ckpt_pid = pid;
        self.duty.ckpt_wanted = false;
        pid
    }

    /// Step 7 of §4.5: the checkpoint cadence (ENG-CKPT-001 cases 1, 2 and 4).
    pub(crate) fn step_checkpoint(&mut self) {
        if self.dead || self.state.is_none() || self.jobs.holds_board() {
            return;
        }
        let changed = self.b != self.pipe.ckpt_b;
        let due = self.duty.ckpt_wanted
            || self.pipe.since_ckpt >= lim::CKPT_ENTRIES
            || (changed && self.mono.saturating_sub(self.pipe.ckpt_mono) >= lim::CKPT_MS);
        if due && (changed || self.duty.ckpt_wanted) {
            self.write_checkpoint();
        }
    }

    /// When the 600,000 ms cadence next comes due, if `B` changed.
    pub(crate) fn checkpoint_deadline(&self) -> Option<u64> {
        (self.b != self.pipe.ckpt_b).then_some(self.pipe.ckpt_mono + lim::CKPT_MS)
    }

    /// `Shutdown` (§18.1 case 3, ENG-CKPT-002).
    pub(crate) fn on_shutdown(&mut self) {
        self.exiting = true;
        // §18.1 case 3 has no condition: the registry can change while `B` stands still.
        if self.state.is_some() && !self.jobs.holds_board() {
            self.write_checkpoint();
        }
        self.flush_meta();
        let pid = self.last_pid;
        self.after(pid, After::ReadyToExit);
    }

    // ------------------------------------------------------------------ retention (§7.13)

    /// Deletes what §7.13 lets go: old bases, the log below the oldest kept base
    /// and the newest 16,384 seqs (ENG-STATE-004).
    pub(crate) fn retention(&mut self) {
        // Bases older than 400 days of log clock, the newest two always kept.
        let tau_b = self.tau_at(self.b).unwrap_or(self.vk.clock_ms);
        let mut gone = Vec::new();
        while self.bases.len() > 2 {
            let (seq, _) = self.bases[0];
            let Some(t) = self.tau_at(seq).or_else(|| self.tau_at(seq + 1)) else { break };
            if tau_b.saturating_sub(t) > 34_560_000_000 {
                gone.push(seq);
                self.bases.remove(0);
            } else {
                break;
            }
        }
        if !gone.is_empty() {
            self.persist(vec![PersistOp::DeleteBases(gone)]);
        }
        let Some(oldest) = self.bases.first().map(|b| b.0) else { return };
        let keep_from = oldest.min(self.l.saturating_sub(lim::SKIP_WINDOW - 1)).max(1);
        if keep_from > self.floor {
            self.truncate_log(keep_from);
        }
    }

    /// `TruncateLogBelow(seq, C_{seq-1})` once a checkpoint at or above `seq - 1` is durable (D8).
    pub(crate) fn truncate_log(&mut self, seq: u64) {
        let floor_chain = self.chain_at(seq - 1);
        if self.pipe.ckpt_b + 1 < seq {
            self.write_checkpoint();
        }
        let pid = self.pipe.ckpt_pid;
        self.floor = seq;
        self.retained = if floor_chain.is_some() { seq - 1 } else { seq };
        self.trim_chains(seq);
        self.hashes.retain(|s, _| *s + 1 >= seq);
        self.after(pid, After::Truncate { below: seq, floor_chain });
    }

    /// At most 1,000 notice and refusal records: the oldest dismissed go first, then the oldest (§7.13).
    fn trim_notices(&mut self) {
        if self.notices.len() <= 1000 {
            return;
        }
        while self.notices.len() > 1000 {
            let at = self.notices.iter().position(|n| n.dismissed).unwrap_or(0);
            self.notices.remove(at);
        }
        self.rewrite_notices();
    }

    /// Replaces the notices file by what is held (§7.11 `RewriteNotices`).
    pub(crate) fn rewrite_notices(&mut self) {
        let mut ops = Vec::with_capacity(self.notices.len());
        let mut dismiss = Vec::new();
        let mut index_in_seq: Vec<(u64, u32)> = Vec::new();
        for n in &self.notices {
            let idx = match index_in_seq.iter_mut().find(|x| x.0 == n.seq) {
                Some(x) => {
                    x.1 += 1;
                    x.1
                }
                None => {
                    index_in_seq.push((n.seq, 0));
                    0
                }
            };
            ops.push(NoticeOp::Add { kind: n.kind, seq: n.seq, record: n.record.clone() });
            if n.dismissed {
                dismiss.push(NoticeOp::Dismiss { seq: n.seq, index: idx });
            }
        }
        ops.extend(dismiss);
        self.persist(vec![PersistOp::RewriteNotices(ops)]);
    }
}
