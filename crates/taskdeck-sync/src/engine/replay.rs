//! Replays from a base (§17.3, §28.1, §22.3, §28.3): a job reads a base and
//! the log (`ReadBase`, `ReadLog`), classifies and applies again into a state
//! of its own, and hands the result to the board or to an operation.
//!
//! Rules: one job at a time (`jobs.cur`); a job that will replace the confirmed
//! state (`Skip`, `Reset`, `Adopt`) holds the board (`holds_board`). A replay
//! never touches the engine's confirmed state, registry or outbox until it is
//! done; it collects the notice and refusal records and the state hashes it
//! produces from `from_rec` on. A `ResetTo` met on the way needs the state at
//! its target: the job stashes the states at `want_states`, and starts again
//! from an older base when a target it did not expect turns up. Entries in
//! `skips` (the current `M_K.skips`) are treated as skipped (§17.3 step 3).

use std::marker::PhantomData;

use super::{Engine, Want, guard, lim};
use crate::{
    AlarmKind, Checkpoint, Class, ClassifyInput, CtlItem, Ctx, Dev, Effect, Evidence, HashRecord, Hash32, Kind,
    LocalReadResult, LogRecord, MembershipView, NoticeKind, NoticeOp, PersistOp, Sealer, SmRefusal, StateMachine,
    StopKind, state_hash,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Purpose {
    /// §17.3 step 3: the Skip at `s` (the queue's front) names `t`, which this device applied.
    Skip { s: u64, t: u64 },
    /// §28.1 "every device": the `ResetTo` at `r` for `t`.
    Reset { r: u64, t: u64, hash: Hash32, author: Dev },
    /// §28.1 stage 1 of `Operation::RestoreTo`: the state hash at `t`.
    RestoreHash { ticket: u64, t: u64 },
    /// §22.3 step 4: this device's own `M_{S'}` for an adoption.
    AdoptMembership { ticket: u64, s: u64 },
    /// §22.3 step 5: the board from the adopted snapshot at `S'`, replayed to `K`.
    AdoptBoard { ticket: u64, s: u64 },
    /// §28.3 stage 3: `M_{first_seq - 1}` and the material at `S` for a move.
    Seed { ticket: u64 },
}

struct Pos<SM: StateMachine, S: Sealer> {
    seq: u64,
    chain: Hash32,
    state: SM::State,
    m: S::Membership,
    view: MembershipView,
}

pub(crate) struct Job<SM: StateMachine, S: Sealer> {
    pub purpose: Purpose,
    /// The newest base at or below this seq is the start.
    pub base_limit: u64,
    pub to: u64,
    pub want_states: Vec<u64>,
    /// Collect records and aligned hashes from this seq on (0: none).
    pub from_rec: u64,
    /// A state to start from instead of the base's board (an adoption).
    pub seed_state: Option<SM::State>,
    stash: Vec<(u64, SM::State, Vec<u8>)>,
    pos: Option<Pos<SM, S>>,
    recs: Vec<(u64, NoticeKind, Vec<u8>)>,
    hashes: Vec<HashRecord>,
    need: Vec<(u64, Hash32)>,
    frames: Vec<(u64, Vec<u8>)>,
    pending: Vec<LogRecord>,
    skips: Vec<u64>,
}

/// What a finished replay hands over.
pub(crate) struct Replayed<SM: StateMachine, S: Sealer> {
    pub seq: u64,
    pub chain: Hash32,
    pub state: SM::State,
    pub m: S::Membership,
    pub stash: Vec<(u64, SM::State, Vec<u8>)>,
    pub recs: Vec<(u64, NoticeKind, Vec<u8>)>,
    pub hashes: Vec<HashRecord>,
}

pub(crate) struct Jobs<SM: StateMachine, S: Sealer> {
    cur: Option<Job<SM, S>>,
    /// The `ResetTo` / Skip whose job is done: the board applies the entry now.
    done_reset: Option<u64>,
    done_skip: Option<(u64, u64)>,
    _p: PhantomData<(SM, S)>,
}

impl<SM: StateMachine, S: Sealer> Default for Jobs<SM, S> {
    fn default() -> Self {
        Jobs { cur: None, done_reset: None, done_skip: None, _p: PhantomData }
    }
}

impl<SM: StateMachine, S: Sealer> Jobs<SM, S> {
    /// A job is about to replace the confirmed state: the board waits.
    pub(crate) fn holds_board(&self) -> bool {
        matches!(
            self.cur.as_ref().map(|j| &j.purpose),
            Some(Purpose::Skip { .. } | Purpose::Reset { .. } | Purpose::AdoptBoard { .. })
        )
    }
    pub(crate) fn holds_log(&self) -> bool {
        false
    }
    pub(crate) fn busy(&self) -> bool {
        self.cur.is_some()
    }
    pub(crate) fn take_done_reset(&mut self, r: u64) -> bool {
        if self.done_reset == Some(r) {
            self.done_reset = None;
            return true;
        }
        false
    }
    pub(crate) fn take_done_skip(&mut self, s: u64, t: u64) -> bool {
        if self.done_skip == Some((s, t)) {
            self.done_skip = None;
            return true;
        }
        false
    }
}

impl<SM: StateMachine, S: Sealer> Job<SM, S> {
    pub(crate) fn new(purpose: Purpose, base_limit: u64, to: u64) -> Self {
        Job {
            purpose,
            base_limit,
            to,
            want_states: Vec::new(),
            from_rec: 0,
            seed_state: None,
            stash: Vec::new(),
            pos: None,
            recs: Vec::new(),
            hashes: Vec::new(),
            need: Vec::new(),
            frames: Vec::new(),
            pending: Vec::new(),
            skips: Vec::new(),
        }
    }
}

impl<SM: StateMachine, S: Sealer> Engine<SM, S> {
    /// The newest base at or below `limit`.
    pub(crate) fn base_at_or_below(&self, limit: u64) -> Option<u64> {
        self.bases.iter().rev().map(|b| b.0).find(|s| *s <= limit)
    }

    /// Starts a replay. False: no base at or below its limit, or another job runs.
    pub(crate) fn start_job(&mut self, mut job: Job<SM, S>) -> bool {
        if self.jobs.cur.is_some() {
            return false;
        }
        let Some(base) = self.base_at_or_below(job.base_limit) else { return false };
        if base + 1 < self.floor && base < job.to {
            return false; // the log from that base on is gone
        }
        job.skips = self.vk.skips.iter().map(|s| s.0).collect();
        job.pos = None;
        job.stash.clear();
        job.recs.clear();
        job.hashes.clear();
        self.jobs.cur = Some(job);
        let req = self.want(Want::JobBase);
        self.fx(Effect::ReadBase { req, seq: base });
        true
    }

    /// The job's base arrived.
    pub(crate) fn job_base(&mut self, base: Option<Checkpoint>) {
        let Some(mut job) = self.jobs.cur.take() else { return };
        let Some(c) = base else { return self.job_fault() };
        let sm = &self.sm;
        let state = match job.seed_state.take() {
            Some(s) => Some(s),
            None => guard(|| sm.load(&c.board)).and_then(Result::ok),
        };
        let m = self.sealer.membership_from_bytes(&c.membership).ok();
        let (Some(state), Some(m)) = (state, m) else { return self.job_fault() };
        let view = self.sealer.view(&m);
        if job.want_states.contains(&c.board_seq) {
            let bytes = self.sealer.membership_bytes(&m);
            job.stash.push((c.board_seq, state.clone(), bytes));
        }
        job.pos = Some(Pos { seq: c.board_seq, chain: c.board_chain, state, m, view });
        self.jobs.cur = Some(job);
        self.job_read();
    }

    /// A local fault ends the job; the board stays held by the fault (§21.4).
    fn job_fault(&mut self) {
        self.jobs.cur = None;
        self.fault("engine/local_fault");
    }

    /// Reads the next chunk of the log, or finishes.
    fn job_read(&mut self) {
        let Some(job) = self.jobs.cur.as_ref() else { return };
        let Some(pos) = job.pos.as_ref() else { return };
        if pos.seq >= job.to {
            return self.job_done();
        }
        let from = pos.seq + 1;
        let to = job.to.min(pos.seq + lim::PAGE_ENTRIES as u64);
        let req = self.want(Want::JobLog);
        self.fx(Effect::ReadLog { req, from, to });
    }

    /// A chunk of the log arrived.
    pub(crate) fn job_log(&mut self, result: LocalReadResult) {
        let records = match result {
            LocalReadResult::Records(v) => v,
            _ => return self.job_fault(),
        };
        if let Some(job) = self.jobs.cur.as_mut() {
            job.pending = records;
        }
        self.job_run();
    }

    /// A frame a Skip in the replay needs arrived (§17.2).
    pub(crate) fn job_frame(&mut self, t: u64, result: LocalReadResult) {
        let frame = match result {
            LocalReadResult::Records(mut v) if v.len() == 1 && v[0].seq == t => v.remove(0).frame,
            _ => return self.job_fault(),
        };
        let ok = match self.jobs.cur.as_mut() {
            Some(job) => match job.need.first().copied() {
                Some((want, hash)) if want == t && self.sealer.frame_hash(&frame) == hash => {
                    job.need.remove(0);
                    job.frames.push((t, frame));
                    true
                }
                _ => false,
            },
            None => return,
        };
        if !ok {
            return self.job_fault();
        }
        self.job_run();
    }

    /// Applies the pending records of the job, pausing for a Skip's frames.
    fn job_run(&mut self) {
        loop {
            let Some(job) = self.jobs.cur.as_mut() else { return };
            if let Some(&(t, _)) = job.need.first() {
                if t < self.floor || t > self.l {
                    return self.job_fault(); // held nowhere this device can read without the relay
                }
                let req = self.want(Want::JobFrame(t));
                self.fx(Effect::ReadLog { req, from: t, to: t });
                return;
            }
            if job.pending.is_empty() {
                return self.job_read();
            }
            let rec = job.pending.remove(0);
            match self.job_step(&rec) {
                JobStep::Ok => {}
                JobStep::Need(wants) => {
                    if let Some(job) = self.jobs.cur.as_mut() {
                        job.pending.insert(0, rec);
                        job.need = wants;
                    }
                }
                JobStep::Restart(target) => {
                    // A ResetTo on the way needs the state at `target`: start again from a base below it.
                    let Some(mut job) = self.jobs.cur.take() else { return };
                    job.want_states.push(target);
                    job.base_limit = job.base_limit.min(target);
                    job.pending.clear();
                    if !self.start_job(job) {
                        self.job_no_base();
                    }
                    return;
                }
                JobStep::Fault => return self.job_fault(),
            }
        }
    }

    /// One log record of a replay: classify it again and apply it to the job's state.
    fn job_step(&mut self, rec: &LogRecord) -> JobStep {
        let Some(job) = self.jobs.cur.as_mut() else { return JobStep::Fault };
        let Some(pos) = job.pos.as_mut() else { return JobStep::Fault };
        let seq = rec.seq;
        if seq != pos.seq + 1 {
            return JobStep::Fault;
        }
        let c = self.sealer.chain_step(&pos.chain, seq, &rec.frame);
        if c != rec.chain {
            return JobStep::Fault;
        }
        let receipt = {
            let chains = &self.chains;
            let (retained, chain_first) = (self.retained, self.chain_first);
            let chain_at = |b: u64| -> Option<Hash32> {
                if b < retained || b < chain_first { None } else { chains.get((b - chain_first) as usize).map(|x| x.0) }
            };
            let fh = self.sealer.frame_hash(&rec.frame);
            // A frame from the device's own log was vetted when it was first classified.
            let own_sealed = |_ctr: u64| -> Option<Hash32> { Some(fh) };
            let frames = &job.frames;
            let skip_targets = |t: u64| -> Option<&[u8]> { frames.iter().find(|f| f.0 == t).map(|f| f.1.as_slice()) };
            let input = ClassifyInput {
                seq,
                frame: &rec.frame,
                prev_chain: pos.chain,
                membership: &pos.m,
                chain_at: &chain_at,
                retained_from: retained,
                own_sealed: &own_sealed,
                skip_targets: &skip_targets,
            };
            let sealer = &mut self.sealer;
            guard(|| sealer.classify(&input))
        };
        let r = match receipt {
            Some(Ok(r)) => r,
            _ => return JobStep::Fault,
        };
        if let Class::NeedFrames(wants) = &r.class {
            return JobStep::Need(wants.clone());
        }
        if r.class == Class::Tamper {
            return JobStep::Fault;
        }
        let own = r.author == self.own;
        let own_no = self.no_b.or_else(|| self.vk.devices.iter().find(|d| d.dev == self.own).map(|d| d.device_no));
        let record = job.from_rec != 0 && seq >= job.from_rec;
        let sm = &self.sm;
        let mut refusal: Option<SmRefusal> = None;
        let plain = |code: &'static str| SmRefusal { code, op: None, target: None, base: None, current: None };
        let skipped = job.skips.contains(&seq);
        let opened = r.opened.as_ref();
        if skipped {
            let mut x = plain("engine/skipped");
            if let Some(o) = opened.filter(|o| o.kind == Kind::Cmd) {
                if let Some(Ok(cmd)) = guard(|| sm.parse(&o.payload)) {
                    x.op = Some(sm.op_name(&cmd));
                    x.target = sm.target(&cmd);
                }
            }
            refusal = Some(x);
        } else if r.class == Class::Refuse {
            let code = r.code.unwrap_or("seal/refused");
            if code != "seal/stale_epoch" && code != "seal/old_epoch" {
                refusal = Some(plain(code));
            }
        } else if r.class == Class::Accept {
            let Some(o) = opened else { return JobStep::Fault };
            match o.kind {
                Kind::Cmd => match guard(|| sm.parse(&o.payload)) {
                    None => return JobStep::Fault,
                    Some(Err(e)) => refusal = Some(plain(e.code)),
                    Some(Ok(cmd)) => {
                        let author_no = pos.view.devices.iter().find(|d| d.dev == r.author).map_or(0, |d| d.device_no);
                        let ctx = Ctx { seq, device_no: author_no, at_ms: o.at_ms, based_on: o.based_on.seq };
                        let state = &mut pos.state;
                        match guard(|| sm.apply(state, &cmd, &ctx)) {
                            None => return JobStep::Fault,
                            Some(Ok(applied)) => {
                                if let (true, Some(no)) = (record, own_no) {
                                    for n in &applied.notices {
                                        if sm.notice_concerns(n, no) {
                                            job.recs.push((seq, NoticeKind::Notice, sm.notice_bytes(n)));
                                        }
                                    }
                                }
                            }
                            Some(Err(x)) => refusal = Some(x),
                        }
                    }
                },
                Kind::Ctl => {
                    for item in &o.items {
                        if let CtlItem::ResetTo { seq: t, .. } = item {
                            // During a replay the reset is applied without the comparison (§28.1).
                            let Some(target) = job.stash.iter().find(|s| s.0 == *t).map(|s| s.1.clone()) else {
                                return JobStep::Restart(*t);
                            };
                            let current = &pos.state;
                            match guard(|| sm.reset(&target, current)) {
                                Some(Ok(s)) => pos.state = s,
                                _ => return JobStep::Fault,
                            }
                        }
                    }
                    let v = self.sealer.view(&r.membership).rules;
                    let state = &mut pos.state;
                    if guard(|| sm.rules(state)).is_some_and(|cur| cur != v) {
                        match guard(|| sm.upgrade(state, v)) {
                            Some(Ok(())) => {}
                            _ => return JobStep::Fault,
                        }
                    }
                }
                Kind::Beat | Kind::Snap => {}
            }
        }
        if let (Some(x), true, true, Some(no)) = (refusal.as_ref(), own, record, own_no) {
            if let Some(bytes) = guard(|| sm.refusal_bytes(seq, no, x)) {
                job.recs.push((seq, NoticeKind::Refusal, bytes));
            }
        }
        let is_ctl = r.class == Class::Accept && opened.is_some_and(|o| o.kind == Kind::Ctl);
        pos.seq = seq;
        pos.chain = c;
        pos.m = r.membership;
        if is_ctl {
            pos.view = self.sealer.view(&pos.m);
        }
        let aligned = seq >= lim::STRIDE && seq % lim::STRIDE == 0;
        if (aligned && record) || job.want_states.contains(&seq) {
            let mbytes = self.sealer.membership_bytes(&pos.m);
            if aligned && record {
                let state = &pos.state;
                let Some(bh) = guard(|| sm.board_hash(state)) else { return JobStep::Fault };
                job.hashes.push(HashRecord { seq, hash_version: 1, hash: state_hash(1, &bh, &mbytes) });
            }
            if job.want_states.contains(&seq) {
                job.stash.push((seq, pos.state.clone(), mbytes));
            }
        }
        JobStep::Ok
    }

    /// No base reaches back far enough (§19.9): the board waits for a usable snapshot.
    fn job_no_base(&mut self) {
        let after = self.b + 1;
        self.add_stop(StopKind::Rebootstrap { after }, None);
    }

    /// The replay reached its end: hand the result to its purpose.
    fn job_done(&mut self) {
        let Some(job) = self.jobs.cur.take() else { return };
        let Some(pos) = job.pos else { return };
        let out = Replayed {
            seq: pos.seq,
            chain: pos.chain,
            state: pos.state,
            m: pos.m,
            stash: job.stash,
            recs: job.recs,
            hashes: job.hashes,
        };
        match job.purpose {
            Purpose::Skip { s, t } => self.skip_replayed(s, t, out),
            Purpose::Reset { r, t, hash, author } => self.reset_replayed(r, t, hash, author, out),
            Purpose::RestoreHash { ticket, t } => self.restore_hash_ready(ticket, t, out),
            Purpose::AdoptMembership { ticket, s } => self.adopt_membership_ready(ticket, s, out),
            Purpose::AdoptBoard { ticket, s } => self.adopt_board_ready(ticket, s, out),
            Purpose::Seed { ticket } => self.seed_replayed(ticket, out),
        }
    }

    // ------------------------------------------------------------------ Skip (§17.3)

    /// §17.3 step 3 for the Skip at `s` of the applied entry `t`. False: the board waits.
    pub(crate) fn begin_skip_replay(&mut self, s: u64, t: u64) -> bool {
        if self.jobs.busy() {
            return false;
        }
        let mut job = Job::new(Purpose::Skip { s, t }, t.saturating_sub(1), s - 1);
        job.from_rec = t;
        if !self.start_job(job) {
            // §17.3 step 4, §19.9: this device cannot replay.
            self.add_stop(StopKind::Rebootstrap { after: s }, None);
        }
        false
    }

    fn skip_replayed(&mut self, s: u64, t: u64, out: Replayed<SM, S>) {
        // ENG-SKIP-002: the state of a device that never applied the target.
        self.state = Some(out.state);
        self.pipe.skipped.insert(t);
        self.replace_records_from(t, out.recs, out.hashes);
        // ENG-SKIP-003: reports for seqs at or above the target describe the state before the Skip.
        self.registry.reports.retain(|r| r.seq < t || r.report_seq >= s);
        self.jobs.done_skip = Some((s, t));
        self.pipe.pre = self.state.clone();
        self.pipe.redo.clear();
        self.duty.ckpt_wanted = true;
        self.rebase_due = true;
    }

    /// ENG-REF-004: the notice and refusal records from `first` on are replaced by a replay's,
    /// a new record staying dismissed when a dismissed old one of its seq has equal bytes; the
    /// hash records of the replayed aligned seqs are written again (the newer wins, §7.5).
    pub(crate) fn replace_records_from(
        &mut self,
        first: u64,
        recs: Vec<(u64, NoticeKind, Vec<u8>)>,
        hashes: Vec<HashRecord>,
    ) {
        let mut old: Vec<super::NoticeRow> = Vec::new();
        let mut keep = Vec::new();
        for n in std::mem::take(&mut self.notices) {
            if n.seq < first { keep.push(n) } else { old.push(n) }
        }
        for (seq, kind, record) in recs {
            let dismissed = match old.iter().position(|o| o.dismissed && o.seq == seq && o.record == record) {
                Some(i) => {
                    old.remove(i);
                    true
                }
                None => false,
            };
            keep.push(super::NoticeRow { kind, seq, record, dismissed });
        }
        self.notices = keep;
        self.notices_through = self.notices.iter().map(|n| n.seq).max().unwrap_or(0).max(self.notices_through);
        self.rewrite_notices();
        if !hashes.is_empty() {
            for h in &hashes {
                self.hashes.insert(h.seq, *h);
            }
            self.persist(vec![PersistOp::AppendHashes(hashes)]);
        }
        self.status_dirty = true;
    }

    // ------------------------------------------------------------------ ResetTo (§28.1)

    /// §28.1 "every device" for the `ResetTo` at `r`. False: the board waits for the replay.
    pub(crate) fn begin_reset(&mut self, r: u64, t: u64, hash: Hash32, author: Dev) -> bool {
        if self.jobs.busy() {
            return false;
        }
        let mut job = Job::new(Purpose::Reset { r, t, hash, author }, t, t);
        job.want_states.push(t);
        if !self.start_job(job) {
            // Step 2: it cannot rebuild `t` (§19.9).
            self.add_stop(StopKind::Rebootstrap { after: r }, None);
        }
        false
    }

    fn reset_replayed(&mut self, r: u64, t: u64, hash: Hash32, author: Dev, out: Replayed<SM, S>) {
        let sm = &self.sm;
        let mbytes = self.sealer.membership_bytes(&out.m);
        let Some(bh) = guard(|| sm.board_hash(&out.state)) else { return self.fault("engine/local_fault") };
        let own = state_hash(1, &bh, &mbytes);
        self.jobs.done_reset = Some(r);
        if own != hash {
            // ENG-RESET-002: this device's replay disagrees with the author's: not applied.
            let mut groups = vec![(own, vec![self.own]), (hash, vec![author])];
            groups.sort();
            let id =
                self.alarm(AlarmKind::ResetMismatch, Some(t), Some(author), None, Evidence::Mismatch { groups });
            self.enter_divergence(t, id);
            return;
        }
        let Some(current) = self.state.as_ref() else { return };
        match guard(|| sm.reset(&out.state, current)) {
            Some(Ok(s)) => {
                // ENG-RESET-003: only the board state changes.
                self.state = Some(s);
                self.pipe.pre = self.state.clone();
                self.pipe.redo.clear();
                self.duty.ckpt_wanted = true;
                self.rebase_due = true;
            }
            _ => {
                self.jobs.done_reset = None;
                self.fault("engine/local_fault");
            }
        }
    }
}

enum JobStep {
    Ok,
    Need(Vec<(u64, Hash32)>),
    Restart(u64),
    Fault,
}
