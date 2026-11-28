//! Operations (§28) and what goes with them: Skip (§17.1), the rules override
//! and the upgrade proposal (§25), revoke (§28.5), purge (§28.2), restore to a
//! point (§28.1), adoption (§22.3), close board (§28.7), and the existing
//! device's part of pairing (§28.4). The move is in movement.rs.
//!
//! Rules: an operation is an `OpRecord` in `meta.ops` (its stage survives a
//! restart, ENG-OPS-003) plus an `OpMem` here. `OpMem::pid` is the write that
//! holds the current stage; the relay call of a stage goes out only once that
//! write is durable (D10) and while `busy` is false. `drive` is looked at on
//! every call. A record an operation enqueued carries `op = ticket`; when its
//! entry reaches the board head, `own_entry_done` moves the operation on.
//! Stages: 1 = the first relay call, read or replay; 2 = its control record is
//! in the outbox; later stages are the move's.

use super::replay::{Job, Purpose, Replayed};
use super::{Engine, Want, guard, lim};
use crate::{
    AdminOk, AdminRequest, AlarmKind, BlobOk, BlobRequest, Body, CtlIntent, Dev, DeviceStatus, DeviceView, Effect,
    Evidence, HaltKind, Hash32, Head, Kind, LocalReadResult, NewDevice, Notification, OpRecord, Operation,
    OutboxOp, OutboxRecord, PersistOp, RelayRefusal, Sealer, SessionState, SnapInfo, SnapshotMaterial, StateMachine,
    StopKind, UpgradeMode, Via, state_hash,
};

pub(crate) struct OpMem {
    pub ticket: u64,
    pub pid: u64,
    pub busy: bool,
    /// Adopt: the snapshot chosen. Skip: nothing. Others: nothing.
    pub snap: Option<SnapInfo>,
}

pub(crate) struct Pair {
    pub ticket: u64,
    pub device: NewDevice,
    pub head: Head,
    pub info: Option<SnapInfo>,
    pub entry: Option<[u8; 16]>,
    pub fail: Option<&'static str>,
}

#[derive(Default)]
pub(crate) struct Ops {
    pub mem: Vec<OpMem>,
    pub pair: Option<Pair>,
    /// The `Register` records found at the `Start` that no relay had acknowledged: pairings whose
    /// machine ended with the process (§28.4). Each still ends with one `PairFailed`.
    pub orphans: Vec<[u8; 16]>,
    pub purge_day: i64,
    pub purge_busy: bool,
    pub mv: super::movement::MoveState,
}

/// The `Operation` variant in snake_case (§30.1 `OpStatus.kind`).
pub(crate) fn op_kind(op: &Operation) -> &'static str {
    match op {
        Operation::RestoreTo { .. } => "restore_to",
        Operation::Purge => "purge",
        Operation::Move { .. } => "move",
        Operation::FollowAddress { .. } => "follow_address",
        Operation::Revoke { .. } => "revoke",
        Operation::Skip { .. } => "skip",
        Operation::RulesOverride { .. } => "rules_override",
        Operation::CloseBoard { .. } => "close_board",
        Operation::Adopt { .. } => "adopt",
    }
}

/// `seal.md` §5.4: a host in a Fence (SEAL-ENC-006, ENG-OPS-007).
pub(crate) fn host_ok(host: &str) -> bool {
    if host.is_empty() || host.len() > 255 {
        return false;
    }
    let (name, port) = match host.split_once(':') {
        Some((n, p)) => (n, Some(p)),
        None => (host, None),
    };
    if name.is_empty() || name.len() > 253 {
        return false;
    }
    let label_ok = |l: &str| {
        !l.is_empty()
            && l.len() <= 63
            && l.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && !l.starts_with('-')
            && !l.ends_with('-')
    };
    if !name.split('.').all(label_ok) {
        return false;
    }
    match port {
        None => true,
        Some(p) => {
            !p.is_empty()
                && p.len() <= 5
                && p.bytes().all(|b| b.is_ascii_digit())
                && !p.starts_with('0')
                && p.parse::<u32>().is_ok_and(|v| (1..=65535).contains(&v) && v != 443)
        }
    }
}

/// `seal.md` §5.2: typed Crockford text, normalised.
pub(crate) fn crockford(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, ' ' | '-' | '\t'))
        .map(|c| match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            x => x,
        })
        .collect()
}

impl<SM: StateMachine, S: Sealer> Engine<SM, S> {
    // ------------------------------------------------------------------ the frame of operations

    pub(crate) fn op_rec(&self, ticket: u64) -> Option<&OpRecord> {
        self.meta.ops.iter().find(|o| o.ticket == ticket)
    }

    fn op_mem(&mut self, ticket: u64) -> Option<&mut OpMem> {
        self.opx.mem.iter_mut().find(|m| m.ticket == ticket)
    }

    /// Records an operation at `stage` and writes it (D10).
    pub(crate) fn op_begin(&mut self, ticket: u64, op: Operation, stage: u8) {
        if self.meta.ops.len() >= lim::OPS {
            self.notify(Notification::OpDone { ticket, result: Err("engine/op_busy") });
            return;
        }
        self.meta.ops.push(OpRecord { ticket, op, stage, attempts: 0, started_ms: self.wall, mark: 0 });
        self.touch_meta();
        let pid = self.flush_meta();
        self.opx.mem.push(OpMem { ticket, pid, busy: false, snap: None });
        self.notify_durable(Notification::OpProgress { ticket, stage });
    }

    /// Moves an operation to `stage` and writes it.
    pub(crate) fn op_stage(&mut self, ticket: u64, stage: u8) {
        if let Some(o) = self.meta.ops.iter_mut().find(|o| o.ticket == ticket) {
            o.stage = stage;
        }
        self.touch_meta();
        let pid = self.flush_meta();
        if let Some(m) = self.op_mem(ticket) {
            m.pid = pid;
            m.busy = false;
        }
        self.notify_durable(Notification::OpProgress { ticket, stage });
    }

    pub(crate) fn op_done(&mut self, ticket: u64, result: Result<(), &'static str>) {
        if self.op_rec(ticket).is_none() {
            return;
        }
        self.meta.ops.retain(|o| o.ticket != ticket);
        self.opx.mem.retain(|m| m.ticket != ticket);
        self.touch_meta();
        // In this call, ahead of the write that drops the record: a process that stops before
        // that write is durable resumes the operation from its record and reports it again; one
        // that stops after it has reported it (§28.8; no row of §7.12 holds it back).
        self.notify(Notification::OpDone { ticket, result });
    }

    fn op_ready(&self, ticket: u64) -> bool {
        self.opx.mem.iter().find(|m| m.ticket == ticket).is_some_and(|m| !m.busy && self.is_durable(m.pid))
    }

    fn set_busy(&mut self, ticket: u64, busy: bool) {
        if let Some(m) = self.op_mem(ticket) {
            m.busy = busy;
        }
    }

    /// May relay calls of operations go out now?
    pub(crate) fn can_call(&self) -> bool {
        matches!(self.sess.state, SessionState::Live | SessionState::Reconciling)
            && !self.dead
            // §11.9: a fenced relay reads only; what would be sent waits for the relay its Fence leads to.
            && !self.sess.fenced
            && self.sess.pause_writes_until.is_none_or(|t| self.wall >= t)
    }

    /// `Event::Op` (§28).
    pub(crate) fn on_op(&mut self, ticket: u64, op: Operation) {
        let refuse = |me: &mut Self, code: &'static str| me.notify(Notification::OpDone { ticket, result: Err(code) });
        // Hosts first, before any effect (ENG-OPS-007).
        let host = match &op {
            Operation::Move { host } | Operation::FollowAddress { host, .. } | Operation::CloseBoard { host, .. } => {
                Some(host)
            }
            _ => None,
        };
        if host.is_some_and(|h| !host_ok(h)) {
            return refuse(self, "engine/bad_host");
        }
        let current = self.current_member();
        let crash = self.crash_halt_seq().is_some();
        let only = |me: &Self, f: fn(&StopKind) -> bool| me.meta.stops.iter().all(|s| f(&s.kind));
        let allowed = match &op {
            Operation::Skip { .. } => current && (!self.halted() || crash) && !self.stopped(),
            Operation::Move { .. } => current && !self.halted() && only(self, |k| matches!(k, StopKind::Evidence)),
            Operation::Adopt { .. } => current && !self.halted() && only(self, |k| matches!(k, StopKind::Divergence)),
            Operation::FollowAddress { .. } => self.no_b.is_some() && !self.halted(),
            _ => current && !self.halted() && !self.stopped(),
        };
        if !allowed {
            return refuse(self, if self.halted() { "engine/halted" } else { "engine/not_eligible" });
        }
        let exclusive = !matches!(op, Operation::Skip { .. } | Operation::RulesOverride { .. });
        if exclusive
            && self.meta.ops.iter().any(|o| !matches!(o.op, Operation::Skip { .. } | Operation::RulesOverride { .. }))
        {
            return refuse(self, "engine/op_busy");
        }
        if self.clock_blocked() {
            return refuse(self, "engine/clock_unconfirmed");
        }
        match &op {
            Operation::Skip { seq } => {
                if let Err(code) = self.skip_allowed(*seq) {
                    return refuse(self, code);
                }
                self.op_begin(ticket, op.clone(), 1);
            }
            Operation::RulesOverride { version } => {
                let v = *version;
                self.op_begin(ticket, op.clone(), 2);
                self.op_enqueue(ticket, CtlIntent::RulesUpgrade { rules: v, mode: UpgradeMode::Override }, Via::Append);
            }
            Operation::Revoke { dev, .. } => {
                // Ruling S14: not this device, not the last current device.
                let currents = self.vk.devices.iter().filter(|d| d.status == DeviceStatus::Current).count();
                let target_ok = self.vk.devices.iter().any(|d| d.dev == *dev && d.status == DeviceStatus::Current);
                if !target_ok || *dev == self.own || currents <= 1 {
                    return refuse(self, "engine/revoke_target");
                }
                self.op_begin(ticket, op.clone(), 1);
            }
            Operation::Purge => match self.purge_preview() {
                Err(code) => refuse(self, code),
                Ok((p, _)) => {
                    self.op_begin(ticket, op.clone(), 2);
                    let intent = CtlIntent::Rotate { revoke: vec![], reinstate: vec![], purge_before: Some(p) };
                    self.op_enqueue(ticket, intent, Via::Append);
                }
            },
            Operation::RestoreTo { seq } => {
                if !self.restore_ok(*seq) {
                    return refuse(self, "engine/restore_target");
                }
                self.op_begin(ticket, op.clone(), 1);
            }
            Operation::Adopt { dev, typed_fp } => {
                // §22.3 step 1 (ENG-DIVERGE-003): ends at once when the fingerprint does not match.
                let fp = self.vk.devices.iter().find(|d| d.dev == *dev).map(|d| crockford(&d.key_fp));
                if *dev == self.own || fp.is_none() || fp != Some(crockford(typed_fp)) {
                    return refuse(self, "engine/fingerprint");
                }
                self.op_begin(ticket, op.clone(), 1);
            }
            Operation::CloseBoard { host, door } => {
                // ENG-OPS-010: exactly one Fence, by the `fence` admin operation.
                let (host, door, home_gen) = (host.clone(), *door, self.vk.home_gen + 1);
                self.op_begin(ticket, op.clone(), 2);
                let via = Via::Fence { host: host.clone(), door, home_gen };
                self.op_enqueue(ticket, CtlIntent::Fence { host, door, home_gen }, via);
            }
            Operation::Move { .. } => self.move_begin(ticket, op.clone()),
            Operation::FollowAddress { .. } => self.follow_begin(ticket, op.clone()),
        }
        self.drive_ops();
    }

    /// Enqueues an operation's control record (§9.2).
    pub(crate) fn op_enqueue(&mut self, ticket: u64, intent: CtlIntent, via: Via) -> Option<[u8; 16]> {
        self.enqueue_own(Kind::Ctl, Body::Ctl(intent), via, Some(ticket))
    }

    /// Step 4 of §4.5.
    pub(crate) fn step_ops(&mut self) {
        // ENG-OPS-003: operations found at a Start go on from their stage.
        let missing: Vec<u64> = self
            .meta
            .ops
            .iter()
            .map(|o| o.ticket)
            .filter(|t| !self.opx.mem.iter().any(|m| m.ticket == *t))
            .collect();
        for t in missing {
            let pid = self.meta_pid;
            self.opx.mem.push(OpMem { ticket: t, pid, busy: false, snap: None });
        }
        // §28.8: thirty days without progress.
        let timed_out: Vec<u64> = self
            .meta
            .ops
            .iter()
            .filter(|o| self.wall.saturating_sub(o.started_ms) > lim::THIRTY_DAYS)
            .map(|o| o.ticket)
            .collect();
        for t in timed_out {
            self.outbox_drop_op(t);
            self.op_done(t, Err("engine/op_timeout"));
        }
        self.drive_ops();
        self.pair_step();
    }

    /// Drops the unsent records of an operation that ended.
    fn outbox_drop_op(&mut self, ticket: u64) {
        let ids: Vec<[u8; 16]> = self
            .outbox
            .iter()
            .filter(|r| r.r.op == Some(ticket) && r.r.frame.is_none())
            .map(|r| r.r.entry_id)
            .collect();
        if ids.is_empty() {
            return;
        }
        self.outbox.retain(|r| !ids.contains(&r.r.entry_id));
        self.persist(vec![PersistOp::Outbox(ids.into_iter().map(OutboxOp::Drop).collect())]);
    }

    /// Looks at every operation's stage again.
    fn drive_ops(&mut self) {
        if self.sess.wait_welcome {
            // §11.9: after a whole-request refusal nothing is repeated until the next welcome.
            return;
        }
        let tickets: Vec<u64> = self.meta.ops.iter().map(|o| o.ticket).collect();
        for t in tickets {
            if !self.op_ready(t) {
                continue;
            }
            let Some(rec) = self.op_rec(t).cloned() else { continue };
            let has_record = self.outbox.iter().any(|r| r.r.op == Some(t));
            match (&rec.op, rec.stage) {
                (Operation::Skip { seq }, 1) => {
                    let seq = *seq;
                    match self.meta.halt.as_ref() {
                        Some(h) if h.seq == seq => {
                            // The halt names the entry's author and frame hash.
                            if let HaltKind::Crash { frame_hash } = h.kind {
                                let author = h.author;
                                self.skip_compose(t, seq, author, frame_hash);
                            }
                        }
                        _ => {
                            self.set_busy(t, true);
                            let req = self.want(Want::OpFrame(t));
                            self.fx(Effect::ReadLog { req, from: seq, to: seq });
                        }
                    }
                }
                (Operation::Revoke { dev, .. }, 1) => {
                    if self.can_call() {
                        let dev = *dev;
                        self.set_busy(t, true);
                        let req = self.want(Want::OpAdmin(t));
                        self.fx(Effect::Admin { req, op: AdminRequest::Revoke { dev } });
                    }
                }
                (Operation::Revoke { dev, .. }, 2) if !has_record => {
                    // After a Start in stage 2 whose record was lost, or a race: compose again.
                    let dev = *dev;
                    self.revoke_compose(t, dev);
                }
                (Operation::RestoreTo { seq }, 1) => {
                    let seq = *seq;
                    if !self.jobs.busy() {
                        let mut job = Job::new(Purpose::RestoreHash { ticket: t, t: seq }, seq, seq);
                        job.want_states.push(seq);
                        if self.start_job(job) {
                            self.set_busy(t, true);
                        } else {
                            self.op_done(t, Err("engine/restore_target"));
                        }
                    }
                }
                (Operation::Adopt { dev, .. }, 1) => {
                    let dev = *dev;
                    self.adopt_fetch(t, dev);
                }
                (Operation::Move { .. }, _) | (Operation::FollowAddress { .. }, _) => self.move_drive(t),
                (_, 2) if !has_record && !matches!(rec.op, Operation::Revoke { .. }) => {
                    // The record of stage 2 is gone without an answer (a Start that lost it).
                    self.op_done(t, Err("engine/local_fault"));
                }
                _ => {}
            }
        }
    }

    // ------------------------------------------------------------------ Skip (§17.1)

    /// §17.1: may this device compose a Skip of `t` (ENG-SKIP-004)?
    pub(crate) fn skip_allowed(&self, t: u64) -> Result<(), &'static str> {
        const NO: &str = "engine/skip_target";
        if t < self.floor || t > self.l || t == 0 {
            return Err(NO);
        }
        if self.vk.ctl_seqs.contains(&t) || self.vk.skips.iter().any(|s| s.0 == t) {
            return Err(NO);
        }
        if self.outbox.iter().any(|r| r.r.seen == Some(t)) {
            return Err(NO); // this device's own entry
        }
        let on_its_way = |b: &Body| matches!(b, Body::Ctl(CtlIntent::Skip { targets }) if targets.iter().any(|x| x.0 == t));
        if self.outbox.iter().any(|r| on_its_way(&r.r.body)) {
            // This device's Skip of it is in its outbox: a second one would be refused for good.
            return Err(NO);
        }
        if self.meta.halt.as_ref().is_some_and(|h| h.seq == t && h.author == self.own) {
            return Err(NO);
        }
        let floor = self.vk.purge_seq.max((self.l + 1).saturating_sub(lim::SKIP_WINDOW));
        if t < floor {
            return Err(NO);
        }
        Ok(())
    }

    /// The frame of a Skip's target arrived from the log.
    fn skip_frame(&mut self, ticket: u64, result: LocalReadResult) {
        let Some(Operation::Skip { seq }) = self.op_rec(ticket).map(|o| o.op.clone()) else { return };
        let frame = match result {
            LocalReadResult::Records(mut v) if v.len() == 1 && v[0].seq == seq => v.remove(0).frame,
            _ => return self.op_done(ticket, Err("engine/skip_target")),
        };
        match self.sealer.frame_author(&frame) {
            Ok((author, _)) => {
                let hash = self.sealer.frame_hash(&frame);
                self.skip_compose(ticket, seq, author, hash);
            }
            Err(_) => self.op_done(ticket, Err("engine/skip_target")),
        }
    }

    fn skip_compose(&mut self, ticket: u64, t: u64, author: Dev, hash: Hash32) {
        if author == self.own {
            // §17.1 check 3: a device may not skip its own entry.
            return self.op_done(ticket, Err("engine/skip_target"));
        }
        self.op_stage(ticket, 2);
        self.op_enqueue(ticket, CtlIntent::Skip { targets: vec![(t, hash)] }, Via::Append);
    }

    // ------------------------------------------------------------------ revoke (§28.5)

    /// Stage 2 (ENG-OPS-009): the stage and the rotation's record in one write.
    fn revoke_compose(&mut self, ticket: u64, dev: Dev) {
        if self.vk.devices.iter().any(|d| d.dev == dev && d.status == DeviceStatus::Revoked) {
            return self.revoke_finish(ticket, dev);
        }
        let intent = CtlIntent::Rotate { revoke: vec![dev], reinstate: vec![], purge_before: None };
        let Some(r) = self.compose(Kind::Ctl, Body::Ctl(intent), Via::Append, Some(ticket)) else {
            return self.op_done(ticket, Err("engine/local_fault"));
        };
        if let Some(o) = self.meta.ops.iter_mut().find(|o| o.ticket == ticket) {
            o.stage = 2;
            o.attempts = o.attempts.saturating_add(1);
        }
        self.meta_dirty = false;
        let pid =
            self.persist(vec![PersistOp::WriteMeta(self.meta.clone()), PersistOp::Outbox(vec![OutboxOp::Put(r.clone())])]);
        self.meta_pid = pid;
        self.status_dirty = true;
        if let Some(m) = self.op_mem(ticket) {
            m.pid = pid;
            m.busy = false;
        }
        self.push_own(r);
        self.notify_durable(Notification::OpProgress { ticket, stage: 2 });
    }

    fn revoke_finish(&mut self, ticket: u64, dev: Dev) {
        let replace = matches!(self.op_rec(ticket).map(|o| &o.op), Some(Operation::Revoke { replace: true, .. }));
        self.op_done(ticket, Ok(()));
        if replace {
            self.notify(Notification::ReplaceReady { dev });
        }
    }

    // ------------------------------------------------------------------ purge (§28.2)

    /// §28.2 step 1: `(p, τ_{p-1})`, or `engine/nothing_to_purge` (ENG-PURGE-001, ENG-PURGE-004).
    pub fn purge_preview(&self) -> Result<(u64, i64), &'static str> {
        let mut p = self.b + 1;
        for d in self.vk.devices.iter().filter(|d| d.status == DeviceStatus::Current && d.dev != self.own) {
            let head = self.registry.last_entry.iter().find(|e| e.dev == d.dev).map_or(0, |e| e.based_on_seq);
            p = p.min(head + 1);
        }
        if p <= self.vk.purge_seq || p <= 1 {
            return Err("engine/nothing_to_purge");
        }
        let tau = self.tau_at(p - 1).or_else(|| self.tau_at(p)).unwrap_or(self.vk.clock_ms);
        Ok((p, tau))
    }

    /// Every device, at the board apply of a rotation with `purge_before = p` at `r` (ENG-PURGE-002).
    /// Runs after `B = r`.
    pub(crate) fn local_purge(&mut self, r: u64, p: u64, epoch: u32, tau_r: i64) {
        // 1: a base at r (D7), and the checkpoint the truncation waits for (D8).
        let base_pid = self.write_base();
        let ckpt_pid = self.write_checkpoint();
        if base_pid == 0 || ckpt_pid == 0 {
            return;
        }
        // 2: the log below p.
        if self.floor < p {
            let floor_chain = if p > self.retained { self.chain_at(p - 1) } else { None };
            self.floor = p;
            self.retained = if floor_chain.is_some() { p - 1 } else { p };
            self.trim_chains(p);
            self.after(ckpt_pid, super::After::Truncate { below: p, floor_chain });
        }
        self.hashes.retain(|s, _| *s >= p);
        self.registry.snapshots.retain(|x| x.info.seq >= p);
        // 3: every base below r.
        let gone: Vec<u64> = self.bases.iter().map(|b| b.0).filter(|s| *s < r).collect();
        if !gone.is_empty() {
            self.bases.retain(|b| b.0 >= r);
            self.after(ckpt_pid, super::After::DeleteBases(gone));
        }
        // 4: the old roots, once the log clock is thirty days past the rotation.
        self.pipe.shred = Some((epoch, tau_r));
        self.rebase_due = true;
    }

    /// §28.2 step 4 (ENG-PURGE-003): old roots go once `τ_K > τ_r + 2,592,000,000`.
    pub(crate) fn shred_step(&mut self) {
        match self.pipe.shred {
            Some((epoch, tau_r)) => {
                if self.vk.clock_ms > tau_r.saturating_add(lim::THIRTY_DAYS) {
                    self.pipe.shred = None;
                    if self.sealer.forget_roots_below(epoch) {
                        let id = self.sealer.export_identity();
                        self.persist(vec![PersistOp::WriteIdentity(id)]);
                    }
                }
            }
            None => {
                // After a Start: the oldest base of a purged board is the base at the purge's rotation.
                if !self.pipe.shred_checked && self.vk.purge_seq > 0 && !self.pipe.tail {
                    self.pipe.shred_checked = true;
                    if let Some(&(seq, _)) = self.bases.first() {
                        let req = self.want(Want::ShredBase(seq));
                        self.fx(Effect::ReadBase { req, seq });
                    }
                }
            }
        }
    }

    fn shred_base(&mut self, seq: u64, result: LocalReadResult) {
        let LocalReadResult::Base(c) = result else { return };
        let Ok(m) = self.sealer.membership_from_bytes(&c.membership) else { return };
        let v = self.sealer.view(&m);
        if v.epoch_seq == seq && v.purge_seq == self.vk.purge_seq && v.purge_seq > 0 {
            self.pipe.shred = Some((v.epoch, v.epoch_clock_ms));
        }
    }

    /// The purging device's daily `Purge` until the relay reports done (§28.2 step 3).
    pub(crate) fn purge_step(&mut self) {
        let Some((before_seq, epoch)) = self.meta.duties.purge else { return };
        let day = self.wall.div_euclid(lim::DAY);
        if self.opx.purge_busy || self.opx.purge_day >= day || !self.can_call() {
            return;
        }
        self.opx.purge_busy = true;
        let req = self.want(Want::PurgeDuty);
        self.fx(Effect::Admin { req, op: AdminRequest::Purge { before_seq, epoch } });
    }

    fn purge_result(&mut self, result: Result<AdminOk, RelayRefusal>) {
        self.opx.purge_busy = false;
        self.opx.purge_day = self.wall.div_euclid(lim::DAY);
        match result {
            Ok(ok) => {
                if ok.done.unwrap_or(0) >= ok.purge_floor.unwrap_or(0) && ok.more != Some(true) {
                    self.meta.duties.purge = None;
                    self.touch_meta();
                }
            }
            Err(r) if r.status == 429 => {} // counts as the day's call; no alarm
            Err(r) => {
                self.alarm(
                    AlarmKind::RelayProtocol,
                    None,
                    None,
                    Some(&r.code),
                    Evidence::Request { request: "admin/purge".to_owned() },
                );
            }
        }
    }

    // ------------------------------------------------------------------ restore to a point (§28.1)

    /// ENG-RESET-001: every condition of "Offered targets" for `t`.
    fn restore_ok(&self, t: u64) -> bool {
        let tau_b = self.tau_at(self.b).unwrap_or(self.vk.clock_ms);
        t < self.b
            && t >= self.vk.purge_seq
            && t >= 1
            && self.vk.devices.iter().filter(|d| d.status == DeviceStatus::Current).all(|d| t > d.bootstrap_seq)
            && self.base_at_or_below(t).is_some_and(|base| base + 1 >= self.floor || base >= t)
            && self.tau_at(t).is_some_and(|x| x >= tau_b.saturating_sub(31_536_000_000))
    }

    /// §28.1: the seqs offered as restore targets, with their log clock.
    pub fn restore_targets(&self) -> Vec<(u64, i64)> {
        let Some(first_base) = self.bases.first().map(|b| b.0) else { return Vec::new() };
        (first_base.max(self.floor).max(1)..self.b)
            .filter(|t| self.restore_ok(*t))
            .filter_map(|t| self.tau_at(t).map(|x| (t, x)))
            .collect()
    }

    /// Stage 1 is done: `h_t` from the replay; stage 2 enqueues the `ResetTo`.
    pub(crate) fn restore_hash_ready(&mut self, ticket: u64, t: u64, out: Replayed<SM, S>) {
        if self.op_rec(ticket).is_none() {
            return;
        }
        let sm = &self.sm;
        let Some(bh) = guard(|| sm.board_hash(&out.state)) else {
            return self.op_done(ticket, Err("engine/local_fault"));
        };
        let hash = state_hash(1, &bh, &self.sealer.membership_bytes(&out.m));
        self.op_stage(ticket, 2);
        self.op_enqueue(ticket, CtlIntent::ResetTo { seq: t, state_hash: hash, hash_version: 1 }, Via::Append);
    }

    // ------------------------------------------------------------------ adoption (§22.3)

    /// §22.3 step 2: the newest snapshot the chosen device wrote, at or below `B` and at or above
    /// the disagreement; then step 3, its blob.
    fn adopt_fetch(&mut self, ticket: u64, dev: Dev) {
        let low = self.evid.diverged_at.unwrap_or(0);
        let Some(info) = self
            .registry
            .snapshots
            .iter()
            .filter(|r| r.author == dev && r.info.seq <= self.b && r.info.seq >= low)
            .max_by_key(|r| r.entry_seq)
            .map(|r| r.info.clone())
        else {
            return; // waiting for that device to write a snapshot; §28.8's timeout applies
        };
        if !self.can_call() {
            return;
        }
        self.set_busy(ticket, true);
        let (id, parts) = (info.blob, info.parts);
        if let Some(m) = self.op_mem(ticket) {
            m.snap = Some(info);
        }
        let req = self.want(Want::OpBlob(ticket));
        self.fx(Effect::Blob { req, op: BlobRequest::Fetch { id, parts } });
    }

    fn adopt_blob(&mut self, ticket: u64, result: Result<BlobOk, RelayRefusal>) {
        let Some(Operation::Adopt { dev, .. }) = self.op_rec(ticket).map(|o| o.op.clone()) else { return };
        let Some(info) = self.opx.mem.iter().find(|m| m.ticket == ticket).and_then(|m| m.snap.clone()) else { return };
        let fail = |me: &mut Self, code: &'static str| {
            me.alarm(AlarmKind::BlobFailure, None, Some(dev), Some(code), Evidence::Blob { blob: info.blob });
            me.op_done(ticket, Err(code));
        };
        let parts = match result {
            Ok(BlobOk::Parts(p)) => p,
            Ok(_) => return fail(self, "relay/no_blob"),
            Err(r) => return fail(self, super::send::intern(&r.code)),
        };
        let refs: Vec<&[u8]> = parts.iter().map(Vec::as_slice).collect();
        let sealer = &self.sealer;
        let opened = guard(|| sealer.open_snapshot(&refs, &info, &dev, true));
        let material = match opened {
            Some(Ok((m, _, _))) => m,
            Some(Err(e)) => return fail(self, e.code),
            None => {
                self.fault("engine/local_fault");
                return self.op_done(ticket, Err("engine/local_fault"));
            }
        };
        let sm = &self.sm;
        let state = match guard(|| sm.load(&material.board)) {
            Some(Ok(s)) => s,
            Some(Err(e)) => return fail(self, e.code),
            None => return fail(self, "engine/local_fault"),
        };
        let ok = guard(|| sm.board_hash(&state))
            .is_some_and(|bh| state_hash(info.hash_version, &bh, &material.membership) == info.hash);
        if !ok {
            return fail(self, "seal/blob_open");
        }
        // Steps 4 and 5: this device's own M_{S'} must equal the snapshot's; then the board from
        // the snapshot at S' is replayed to B from the log.
        let s = material.seq;
        let rules = guard(|| sm.rules(&state));
        let mut job = Job::new(Purpose::AdoptBoard { ticket, s }, s, self.b);
        job.from_rec = s + 1;
        job.swap = Some((s, state, material.membership, rules.unwrap_or(0)));
        if self.jobs.busy() || !self.start_job(job) {
            self.op_done(ticket, Err("engine/adopt_membership"));
        }
    }

    /// ENG-DIVERGE-004: the job met a membership that differs from the snapshot's.
    pub(crate) fn adopt_refused(&mut self, ticket: u64) {
        self.op_done(ticket, Err("engine/adopt_membership"));
    }


    /// §22.3 step 5: the adopted board, replayed to `B`.
    pub(crate) fn adopt_board_ready(&mut self, ticket: u64, s: u64, out: Replayed<SM, S>) {
        if self.op_rec(ticket).is_none() {
            return;
        }
        if out.seq != self.b {
            // The board moved while the replay ran: adopt again from the same snapshot.
            return self.set_busy(ticket, false);
        }
        self.state = Some(out.state);
        let mut hashes = out.hashes;
        if s >= lim::STRIDE && s.is_multiple_of(lim::STRIDE)
            && let Some(info) = self.opx.mem.iter().find(|m| m.ticket == ticket).and_then(|m| m.snap.clone()) {
                hashes.insert(0, crate::HashRecord { seq: s, hash_version: info.hash_version, hash: info.hash });
            }
        self.replace_records_from(s + 1, out.recs, hashes);
        self.pipe.pre = self.state.clone();
        self.pipe.redo.clear();
        self.write_checkpoint();
        self.rebase_due = true;
        // The divergence stop ends.
        self.evid.diverged_at = None;
        self.end_stops(|x| x.kind == StopKind::Divergence);
        self.resolve_alarms(|a| {
            matches!(a.kind, AlarmKind::StateMismatch | AlarmKind::ResetMismatch | AlarmKind::SnapshotMismatch)
        });
        self.op_done(ticket, Ok(()));
    }

    // ------------------------------------------------------------------ rules upgrades (§25)

    /// The active members of `seal.md` §13.10, with this device as the author.
    pub(crate) fn active_members(&self) -> Vec<&DeviceView> {
        let now = self.vk.clock_ms;
        self.vk
            .devices
            .iter()
            .filter(|d| d.status == DeviceStatus::Current)
            .filter(|d| d.dev == self.own || now.saturating_sub(d.beat_ms) <= lim::THIRTY_DAYS)
            .collect()
    }

    /// §25.3: the proposal duty, by rank.
    pub(crate) fn upgrade_step(&mut self) {
        let active = self.vk.rules;
        let best = self.sm.supported_rules().iter().copied().filter(|v| *v > active).max();
        let unblocked = best.filter(|v| self.active_members().iter().all(|d| d.rules.contains(v)));
        match unblocked {
            None => {
                if self.meta.duties.upgrade.is_some() {
                    self.meta.duties.upgrade = None;
                    self.touch_meta();
                }
            }
            Some(v) => {
                let t = match self.meta.duties.upgrade {
                    Some((x, t)) if x == v => t,
                    _ => {
                        self.meta.duties.upgrade = Some((v, self.wall));
                        self.touch_meta();
                        self.wall
                    }
                };
                let rank = super::duty::rank_of(&self.vk, &self.own).unwrap_or(0) as i64;
                let queued = self
                    .outbox
                    .iter()
                    .any(|r| matches!(&r.r.body, Body::Ctl(CtlIntent::RulesUpgrade { rules, .. }) if *rules == v));
                if !queued && self.wall >= t.saturating_add(rank * lim::HOUR) {
                    self.enqueue_own(
                        Kind::Ctl,
                        Body::Ctl(CtlIntent::RulesUpgrade { rules: v, mode: UpgradeMode::Normal }),
                        Via::Append,
                        None,
                    );
                }
            }
        }
    }

    // ------------------------------------------------------------------ own entries settle

    /// An own record's entry reached the board head (`code`: refused), or could not be sealed (`seq` 0).
    pub(crate) fn own_entry_done(&mut self, r: &OutboxRecord, seq: u64, code: Option<&'static str>) {
        if let Body::Ctl(CtlIntent::RulesUpgrade { mode: UpgradeMode::Normal, .. }) = &r.body {
            // §25.3: accepted or refused, the duty looks again.
            if self.meta.duties.upgrade.is_some() {
                self.meta.duties.upgrade = None;
                self.touch_meta();
            }
        }
        if r.kind == Kind::Snap && code.is_some() {
            // A `snap` entry refused or not sealed counts against the duty (§19.4: three failures end it).
            self.bump_snapshot_failure();
        }
        let Some(ticket) = r.op else { return };
        if self.opx.pair.as_ref().is_some_and(|p| p.ticket == ticket) {
            return self.pair_entry_done(code);
        }
        if let Via::Register { dev, .. } = r.via {
            // A pairing from before a `Start`: nobody waits for it, and it is still reported.
            let open = self.opx.orphans.contains(&r.entry_id);
            self.opx.orphans.retain(|x| *x != r.entry_id);
            match code {
                Some(c) => {
                    // ENG-OPS-004: the relay drops the row and token `register` made, then the failure.
                    if self.can_call() {
                        let req = self.id();
                        self.fx(Effect::Admin { req, op: AdminRequest::Revoke { dev } });
                    }
                    self.notify(Notification::PairFailed { ticket, code: c });
                }
                // Accepted, and never acknowledged to the machine that asked for it.
                None if open => self.notify(Notification::PairFailed { ticket, code: "engine/pair_interrupted" }),
                None => {}
            }
            return;
        }
        let Some(op) = self.op_rec(ticket).map(|o| o.op.clone()) else { return };
        if let (Operation::Skip { seq }, Some("seal/ctl_target")) = (&op, code)
            && self.vk.skips.iter().any(|s| s.0 == *seq)
        {
            // §17.1: another entry skipped the target first; what the user asked for holds.
            return self.op_done(ticket, Ok(()));
        }
        match (&op, code) {
            (Operation::Revoke { dev, .. }, None) => self.revoke_finish(ticket, *dev),
            (
                Operation::Revoke { dev, .. },
                Some("seal/stale_epoch" | "seal/old_epoch" | "seal/ctl_epoch" | "seal/ctl_wraps"),
            ) => {
                // ENG-OPS-002: composed again after every race, until the target is revoked.
                self.revoke_compose(ticket, *dev);
            }
            (Operation::Purge, None) => {
                if let Body::Ctl(CtlIntent::Rotate { purge_before: Some(p), .. }) = &r.body {
                    let p = *p;
                    let epoch = self.vk.epoch;
                    self.meta.duties.purge = Some((p, epoch));
                    self.touch_meta();
                    let before_ms = self.tau_at(p.saturating_sub(1)).or_else(|| self.tau_at(p)).unwrap_or(0);
                    self.notify_durable(Notification::PurgeBackups { before_ms });
                    self.notify_durable(Notification::ReuploadPicture { epoch });
                }
                self.op_done(ticket, Ok(()));
            }
            (Operation::Move { .. }, _) => self.move_entry_done(ticket, seq, code),
            (Operation::CloseBoard { .. }, None) => {
                // The devices of the closed board follow its Fence with this code (§28.3 step 2).
                if let Some(hash) = r.frame_hash {
                    let text = self.sealer.move_code(seq, &hash);
                    self.notify_durable(Notification::MoveCode { text });
                }
                self.op_done(ticket, Ok(()));
            }
            (_, None) => self.op_done(ticket, Ok(())),
            (_, Some(c)) => self.op_done(ticket, Err(c)),
        }
    }

    /// The answer of an operation's admin call.
    pub(crate) fn op_admin(&mut self, ticket: u64, result: Result<AdminOk, RelayRefusal>) {
        self.set_busy(ticket, false);
        let Some(op) = self.op_rec(ticket).map(|o| o.op.clone()) else { return };
        match (&op, result) {
            (Operation::Revoke { dev, .. }, Ok(_)) => self.revoke_compose(ticket, *dev),
            (Operation::Revoke { dev, .. }, Err(r)) if r.status == 409 && r.code == "relay/target" => {
                self.revoke_compose(ticket, *dev)
            }
            (Operation::Revoke { .. }, Err(r)) => self.op_done(ticket, Err(super::send::intern(&r.code))),
            (Operation::Move { .. }, r) => self.move_admin(ticket, r),
            _ => {}
        }
    }

    /// The answers of what operations asked the local store, the relay's blobs or its admin calls.
    pub(crate) fn op_want_local(&mut self, w: Want, result: LocalReadResult) {
        match w {
            Want::OpFrame(t) => {
                self.set_busy(t, false);
                self.skip_frame(t, result);
            }
            Want::ShredBase(seq) => self.shred_base(seq, result),
            Want::MoveLog(t) => self.move_log(t, result),
            _ => {}
        }
    }

    pub(crate) fn op_want_admin(&mut self, w: Want, result: Result<AdminOk, RelayRefusal>) {
        match w {
            Want::OpAdmin(t) => self.op_admin(t, result),
            Want::PurgeDuty => self.purge_result(result),
            _ => {}
        }
    }

    pub(crate) fn op_want_blob(&mut self, w: Want, result: Result<BlobOk, RelayRefusal>) {
        match w {
            Want::OpBlob(t) => {
                self.set_busy(t, false);
                match self.op_rec(t).map(|o| o.op.clone()) {
                    Some(Operation::Adopt { .. }) => self.adopt_blob(t, result),
                    Some(Operation::Move { .. }) => self.move_blob(t, result),
                    _ => {}
                }
            }
            Want::PairUpload => self.pair_uploaded(result),
            _ => {}
        }
    }

    /// A relay request of an operation got no answer: the stage is looked at again.
    pub(crate) fn op_want_lost(&mut self, w: &Want) -> &'static str {
        match w {
            Want::OpAdmin(t) => {
                self.set_busy(*t, false);
                "admin/op"
            }
            Want::OpBlob(t) => {
                self.set_busy(*t, false);
                "blob"
            }
            Want::PurgeDuty => {
                self.opx.purge_busy = false;
                "admin/purge"
            }
            Want::PairUpload => {
                self.pair_fail("engine/not_eligible");
                "blob"
            }
            _ => "local",
        }
    }

    // ------------------------------------------------------------------ pairing (§28.4)

    /// `PairRegister` (ENG-HALT-007).
    pub(crate) fn on_pair_register(&mut self, ticket: u64, device: NewDevice) {
        let code = if self.halted() {
            Some("engine/halted")
        } else if !self.current_member() || self.stopped() || self.sess.state != SessionState::Live {
            Some("engine/not_eligible")
        } else if self.opx.pair.is_some() {
            Some("engine/pair_busy")
        } else {
            None
        };
        if let Some(code) = code {
            return self.notify(Notification::PairFailed { ticket, code });
        }
        // Step 1: a fresh snapshot of this device's own state at h = B (V1-10).
        let Some(c) = self.checkpoint() else { return self.notify(Notification::PairFailed { ticket, code: "engine/local_fault" }) };
        let (Some(hash), Some(m)) = (self.own_state_hash(), self.mk.clone()) else { return };
        let head = Head { seq: self.b, chain: self.cb };
        let material = SnapshotMaterial {
            seq: head.seq,
            chain: head.chain,
            hash_version: 1,
            state_hash: hash,
            board: c.board,
            membership: c.membership,
        };
        match self.sealer.seal_snapshot(&m, &material) {
            Err(e) => self.notify(Notification::PairFailed { ticket, code: e.code }),
            Ok(blob) => {
                let info = SnapInfo {
                    seq: head.seq,
                    hash,
                    hash_version: 1,
                    blob: blob.id,
                    sha256: blob.sha256,
                    size: blob.bytes.len() as u64,
                    parts: blob.parts,
                    inflated: blob.inflated,
                    epoch: blob.epoch,
                };
                self.opx.pair = Some(Pair { ticket, device, head, info: Some(info), entry: None, fail: None });
                let req = self.want(Want::PairUpload);
                self.fx(Effect::Blob { req, op: BlobRequest::Upload { purpose: 2, seq: head.seq, blob } });
            }
        }
    }

    fn pair_fail(&mut self, code: &'static str) {
        if let Some(p) = self.opx.pair.take() {
            self.notify(Notification::PairFailed { ticket: p.ticket, code });
        }
    }

    /// Step 2 (ENG-OPS-008): the `DeviceAdded` only after the upload.
    fn pair_uploaded(&mut self, result: Result<BlobOk, RelayRefusal>) {
        let Some(p) = self.opx.pair.as_ref() else { return };
        match result {
            Ok(BlobOk::Uploaded) => {
                let d = &p.device;
                let intent = CtlIntent::AddDevice {
                    dev: d.dev,
                    sign_pk: d.sign_pk,
                    hpke_pk: d.hpke_pk,
                    name: d.name.clone(),
                    transcript: d.transcript,
                    bootstrap_seq: p.head.seq,
                };
                let via = Via::Register { dev: d.dev, sign_pk: d.sign_pk };
                let (ticket, head) = (p.ticket, p.head);
                // ENG-OPS-001: based on the head of the snapshot it uploaded.
                match self.compose(Kind::Ctl, Body::Ctl(intent), via, Some(ticket)) {
                    Some(mut r) => {
                        r.based_on = head;
                        let id = r.entry_id;
                        self.persist(vec![PersistOp::Outbox(vec![OutboxOp::Put(r.clone())])]);
                        self.push_own(r);
                        if let Some(p) = self.opx.pair.as_mut() {
                            p.entry = Some(id);
                        }
                    }
                    None => self.pair_fail("engine/local_fault"),
                }
            }
            Ok(_) => self.pair_fail("relay/refused"),
            Err(r) => self.pair_fail(super::send::intern(&r.code)),
        }
    }

    /// Step 3 (ENG-OPS-006): never sealed under another epoch than the snapshot's.
    pub(crate) fn before_carried_seal(&mut self, i: usize) -> bool {
        let id = self.outbox[i].r.entry_id;
        let Some(p) = self.opx.pair.as_ref().filter(|p| p.entry == Some(id)) else {
            if matches!(self.outbox[i].r.via, Via::Register { .. }) {
                // A pairing this process does not know (a Start in between): the record goes
                // unsent, and the pairing is reported failed under the ticket the record kept.
                let ticket = self.outbox[i].r.op.unwrap_or(0);
                self.opx.orphans.retain(|x| *x != id);
                self.outbox.remove(i);
                self.persist(vec![PersistOp::Outbox(vec![OutboxOp::Drop(id)])]);
                self.notify(Notification::PairFailed { ticket, code: "engine/pair_interrupted" });
                return false;
            }
            return true;
        };
        if p.info.as_ref().is_some_and(|x| x.epoch != self.vk.epoch) {
            self.outbox.remove(i);
            self.persist(vec![PersistOp::Outbox(vec![OutboxOp::Drop(id)])]);
            self.pair_fail("seal/pair_raced");
            return false;
        }
        true
    }

    /// The answer of a `Register` or `Fence` that carried a record.
    pub(crate) fn carried_done(&mut self, id: &[u8; 16], result: Result<(u64, Hash32), &'static str>) {
        if let Some(p) = self.opx.pair.as_ref().filter(|p| p.entry == Some(*id)) {
            match (result, p.info.clone()) {
                (Ok((seq, _)), Some(snapshot)) => {
                    let (ticket, head) = (p.ticket, p.head);
                    // The acknowledgement is durable before the report: a record found after a
                    // `Start` with its acknowledgement had its `PairRegistered`, and one without
                    // it is reported failed (§28.4 "After a Start"), never both for one ticket.
                    let note = Notification::PairRegistered { ticket, head, snapshot, added_seq: seq };
                    match self.rec_index(id).map(|i| self.outbox[i].r.clone()) {
                        Some(rec) => {
                            self.send.ack_puts.retain(|x| x != id);
                            let pid = self.persist(vec![PersistOp::Outbox(vec![OutboxOp::Put(rec)])]);
                            self.after(pid, super::After::Notify(note));
                        }
                        None => self.notify(note),
                    }
                }
                (Err(code), _) => self.pair_fail(code),
                _ => {}
            }
            return;
        }
        // A Fence: the operation that owns it goes on when the entry is applied; a refusal ends it.
        if let Err(code) = result {
            let ticket = self.meta.ops.iter().find(|o| matches!(o.op, Operation::CloseBoard { .. } | Operation::Move { .. })).map(|o| o.ticket);
            if let Some(t) = ticket {
                match self.op_rec(t).map(|o| o.op.clone()) {
                    Some(Operation::Move { .. }) => self.move_fence_refused(t, code),
                    _ => self.op_done(t, Err(code)),
                }
            }
        }
    }

    /// A `409 relay/conflict` on a carried record: once the frame is seen in the log, the
    /// registration is repeated in seq mode (`relay.md` §12.3).
    pub(crate) fn carried_conflict(&mut self, id: &[u8; 16]) {
        if let Some(p) = self.opx.pair.as_mut().filter(|p| p.entry == Some(*id)) {
            p.fail = Some("relay/conflict");
        }
    }

    /// Step 4 (ENG-OPS-004): the `DeviceAdded` reached the board head.
    fn pair_entry_done(&mut self, code: Option<&'static str>) {
        let Some(p) = self.opx.pair.as_mut() else { return };
        match code {
            None => self.opx.pair = None,
            Some(c) => {
                // `revoke` for the new device first, so that the relay drops its row and token
                // (an answer `relay/target` is already done); the failure follows in this call,
                // before the record's `Drop` is written, so that no stop of the process between
                // the two leaves a pairing that is never reported.
                p.fail = Some(c);
                let dev = p.device.dev;
                if self.can_call() {
                    let req = self.id();
                    self.fx(Effect::Admin { req, op: AdminRequest::Revoke { dev } });
                }
                self.pair_revoked();
            }
        }
    }

    fn pair_revoked(&mut self) {
        let code = self.opx.pair.as_ref().and_then(|p| p.fail).unwrap_or("seal/pair_raced");
        self.pair_fail(code);
    }

    /// A stored `DeviceAdded` whose register was answered `conflict`: seq mode once it is seen.
    fn pair_step(&mut self) {
        let Some(p) = self.opx.pair.as_ref() else { return };
        if p.fail != Some("relay/conflict") || self.send.admin.is_some() || !self.can_call() {
            return;
        }
        let Some(id) = p.entry else { return };
        let Some(i) = self.rec_index(&id) else { return };
        let r = &self.outbox[i].r;
        if let (Some(seq), Some(frame_hash), Some(ctr)) = (r.seen, r.frame_hash, r.ctr) {
            let (dev, sign_pk) = (p.device.dev, p.device.sign_pk);
            if let Some(p) = self.opx.pair.as_mut() {
                p.fail = None;
            }
            let req = self.id();
            self.send.admin = Some((req, id, ctr));
            self.fx(Effect::Admin { req, op: AdminRequest::RegisterSeq { seq, dev, sign_pk, frame_hash } });
        }
    }
}
