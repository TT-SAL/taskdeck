//! The outbox (§7.4), enqueue (§9), the naming hold (§9.3), the pending view
//! and its rebase (§8), entries the engine composes (§9.2), export and
//! resubmit of unsent commands (§24.5).
//!
//! Rules: `outbox` is ordered by `order`. A record's `seen` is set when its
//! entry is classified (the membership commit), and the record is dropped when
//! the entry reaches the board head. The pending view is `state` plus the
//! unapplied `cmd` records; `Rec::prov` is a record's provisional seq and
//! `Rec::doomed` its "will be refused" code, both as of the last rebase.

use super::{After, Engine, guard};
use crate::{
    Body, Coalesce, Ctx, Head, Kind, Notification, OutboxOp, OutboxRecord, PersistOp, Sealer, StateMachine, StopKind,
    Via,
};

/// One outbox record with what the engine keeps beside it in memory.
pub(crate) struct Rec<C> {
    pub r: OutboxRecord,
    /// The parsed command of a `cmd` record.
    pub cmd: Option<C>,
    /// The write that carries the record's current frame (0: it was loaded at `Start`).
    pub pid: u64,
    /// `mono_ms` when it was enqueued or last coalesced, while it is held (§9.3).
    pub held_at: Option<u64>,
    pub doomed: Option<&'static str>,
    pub prov: Option<u64>,
}

impl<C> Rec<C> {
    pub(crate) fn new(r: OutboxRecord, cmd: Option<C>) -> Self {
        Rec { r, cmd, pid: 0, held_at: None, doomed: None, prov: None }
    }
}

impl<SM: StateMachine, S: Sealer> Engine<SM, S> {
    // ------------------------------------------------------------------ the pending view (§8.2)

    /// Is this record an unapplied `cmd` record (a member of `U`)?
    fn in_u(r: &OutboxRecord, b: u64) -> bool {
        r.kind == Kind::Cmd && r.seen.is_none_or(|s| s > b)
    }

    /// Recomputes the pending view in full (a rebase, §8.4) and raises `view_version`.
    pub(crate) fn rebase(&mut self) {
        self.rebase_due = false;
        let Some(confirmed) = self.state.as_ref() else { return };
        let mut p = confirmed.clone();
        let b = self.b;
        let mut applied: Vec<usize> = Vec::new();
        let mut i = 0u64;
        for idx in 0..self.outbox.len() {
            let rec = &mut self.outbox[idx];
            rec.doomed = None;
            rec.prov = None;
            if !Self::in_u(&rec.r, b) {
                continue;
            }
            // A device with no `device_no` in `M_B` shows the confirmed state alone.
            let Some(d) = self.no_b else { continue };
            i += 1;
            rec.prov = Some(b + i);
            let ctx = Ctx { seq: b + i, device_no: d, at_ms: rec.r.at_ms, based_on: rec.r.based_on.seq.min(b) };
            let Some(cmd) = rec.cmd.as_ref() else {
                rec.doomed = Some("engine/trial_panic");
                continue;
            };
            let sm = &self.sm;
            match guard(|| sm.apply(&mut p, cmd, &ctx)) {
                Some(Ok(_)) => applied.push(idx),
                Some(Err(refusal)) => rec.doomed = Some(refusal.code),
                None => {
                    // ENG-PEND-002: the view is rebuilt from the records before it that were not doomed.
                    rec.doomed = Some("engine/trial_panic");
                    p = confirmed.clone();
                    for &j in &applied {
                        let r = &self.outbox[j];
                        if let (Some(c), Some(seq)) = (r.cmd.as_ref(), r.prov) {
                            let ctx =
                                Ctx { seq, device_no: d, at_ms: r.r.at_ms, based_on: r.r.based_on.seq.min(b) };
                            let _ = guard(|| sm.apply(&mut p, c, &ctx));
                        }
                    }
                }
            }
        }
        self.view = Some(p);
        self.view_version += 1;
        self.status_dirty = true;
    }

    /// Step 3 of §4.5.
    pub(crate) fn step_rebase(&mut self) {
        if self.rebase_due {
            self.rebase();
        }
    }

    /// The number of records in `U`.
    fn u_len(&self) -> u64 {
        self.outbox.iter().filter(|r| Self::in_u(&r.r, self.b)).count() as u64
    }

    /// The trial apply of §9.1 step 3 on a copy of the pending view. On success the
    /// copy becomes the view.
    fn trial(&mut self, command: &SM::Command, ctx: &Ctx) -> Result<(), &'static str> {
        let Some(view) = self.view.as_ref() else { return Err("engine/not_joined") };
        let mut copy = view.clone();
        let sm = &self.sm;
        match guard(|| sm.apply(&mut copy, command, ctx)) {
            None => Err("engine/trial_panic"),
            Some(Err(refusal)) => Err(refusal.code),
            Some(Ok(_)) => {
                self.view = Some(copy);
                self.view_version += 1;
                Ok(())
            }
        }
    }

    // ------------------------------------------------------------------ enqueue (§9.1)

    pub(crate) fn next_order(&self) -> u64 {
        self.outbox.iter().map(|r| r.r.order).max().map_or(1, |o| o + 1)
    }

    /// The code that refuses an `Enqueue` or a composed entry, in the order of §9.1 step 1.
    fn enqueue_refusal(&self, at_ms: i64) -> Option<&'static str> {
        if self.no_b.is_none() {
            return Some("engine/not_joined");
        }
        if self.has_stop(|k| matches!(k, StopKind::Revoked { by_log: true })) {
            return Some("engine/revoked");
        }
        if self.clock_blocked() {
            return Some("engine/clock_unconfirmed");
        }
        if (at_ms as i128 - self.wall as i128).abs() > 60_000 {
            return Some("engine/runtime_protocol");
        }
        None
    }

    pub(crate) fn on_enqueue(
        &mut self,
        ticket: u64,
        command: SM::Command,
        at_ms: i64,
        origin_state: Vec<u8>,
        hold: bool,
    ) {
        if let Some(code) = self.enqueue_refusal(at_ms) {
            self.notify(Notification::EnqueueRefused { ticket, code });
            return;
        }
        // §9.3: the last record is held.
        if self.last_is_held() {
            let last = self.outbox.len() - 1;
            let sm = &self.sm;
            let held = self.outbox[last].cmd.as_ref();
            let verdict = held.and_then(|h| guard(|| sm.coalesce(h, &command)));
            match verdict {
                Some(Coalesce::Merge(m)) => return self.merge_into_held(ticket, m, origin_state),
                Some(Coalesce::CancelBoth) => {
                    let rec = self.outbox.remove(last);
                    let pid = self.persist(vec![PersistOp::Outbox(vec![OutboxOp::Drop(rec.r.entry_id)])]);
                    self.rebase();
                    self.after(
                        pid,
                        After::Notify(Notification::Enqueued { ticket, entry_id: None, provisional_seq: None }),
                    );
                    return;
                }
                Some(Coalesce::Keep) | None => self.end_hold(),
            }
        }
        match self.enqueue_cmd(command, at_ms, origin_state, hold) {
            Ok((pid, entry_id, prov)) => self.after(
                pid,
                After::Notify(Notification::Enqueued {
                    ticket,
                    entry_id: Some(entry_id),
                    provisional_seq: Some(prov),
                }),
            ),
            Err(code) => self.notify(Notification::EnqueueRefused { ticket, code }),
        }
    }

    /// §9.1 steps 3 and 4: the trial apply, then the record and its write.
    fn enqueue_cmd(
        &mut self,
        command: SM::Command,
        at_ms: i64,
        origin_state: Vec<u8>,
        hold: bool,
    ) -> Result<(u64, [u8; 16], u64), &'static str> {
        let Some(d) = self.no_b else { return Err("engine/not_joined") };
        let prov = self.b + self.u_len() + 1;
        let ctx = Ctx { seq: prov, device_no: d, at_ms, based_on: self.b };
        self.trial(&command, &ctx)?;
        let entry_id = self.sealer.new_entry_id().map_err(|e| e.code)?;
        let sm = &self.sm;
        let payload = guard(|| sm.encode(&command)).ok_or("engine/local_fault")?;
        let held = hold && guard(|| sm.hold(&command)).flatten().is_some();
        let r = OutboxRecord {
            entry_id,
            order: self.next_order(),
            at_ms,
            based_on: Head { seq: self.b, chain: self.cb },
            kind: Kind::Cmd,
            body: Body::Cmd(payload),
            origin_state,
            via: Via::Append,
            hold: held,
            ctr: None,
            frame: None,
            frame_hash: None,
            epoch: None,
            ack: None,
            seen: None,
            op: None,
            reseals: 0,
        };
        let pid = self.persist(vec![PersistOp::Outbox(vec![OutboxOp::Put(r.clone())])]);
        let mut rec = Rec::new(r, Some(command));
        rec.prov = Some(prov);
        rec.held_at = held.then_some(self.mono);
        self.outbox.push(rec);
        self.status_dirty = true;
        Ok((pid, entry_id, prov))
    }

    // ------------------------------------------------------------------ the naming hold (§9.3)

    /// Is the last outbox record held now (ENG-HOLD-001)?
    pub(crate) fn last_is_held(&self) -> bool {
        match self.outbox.last() {
            Some(r) => {
                r.r.hold && r.held_at.is_some_and(|t| self.mono.saturating_sub(t) < super::lim::HOLD_MS)
            }
            None => false,
        }
    }

    /// Ends the hold of the last record (ENG-HOLD-004).
    pub(crate) fn end_hold(&mut self) {
        for r in &mut self.outbox {
            r.r.hold = false;
            r.held_at = None;
        }
    }

    /// When the hold of the last record times out, if it is held.
    pub(crate) fn hold_deadline(&self) -> Option<u64> {
        let r = self.outbox.last()?;
        if r.r.hold { r.held_at.map(|t| t + super::lim::HOLD_MS) } else { None }
    }

    /// `Merge(m)`: `m` takes the held record's place (ENG-HOLD-002).
    fn merge_into_held(&mut self, ticket: u64, m: SM::Command, origin_state: Vec<u8>) {
        let Some(mut rec) = self.outbox.pop() else { return };
        let (Some(d), Some(prov)) = (self.no_b, rec.prov) else {
            self.outbox.push(rec);
            self.notify(Notification::EnqueueRefused { ticket, code: "engine/not_joined" });
            return;
        };
        // The view without the held record, then `m` with the record's own stamps.
        self.rebase();
        let ctx = Ctx { seq: prov, device_no: d, at_ms: rec.r.at_ms, based_on: rec.r.based_on.seq.min(self.b) };
        let sm = &self.sm;
        let payload = guard(|| sm.encode(&m));
        let result = match payload {
            Some(p) => self.trial(&m, &ctx).map(|()| p),
            None => Err("engine/local_fault"),
        };
        match result {
            Ok(payload) => {
                rec.r.body = Body::Cmd(payload);
                rec.r.origin_state = origin_state;
                rec.cmd = Some(m);
                rec.held_at = Some(self.mono);
                rec.prov = Some(prov);
                rec.doomed = None;
                let entry_id = rec.r.entry_id;
                let pid = self.persist(vec![PersistOp::Outbox(vec![OutboxOp::Put(rec.r.clone())])]);
                self.outbox.push(rec);
                self.after(
                    pid,
                    After::Notify(Notification::Enqueued {
                        ticket,
                        entry_id: Some(entry_id),
                        provisional_seq: Some(prov),
                    }),
                );
            }
            Err(code) => {
                self.outbox.push(rec);
                self.rebase();
                self.notify(Notification::EnqueueRefused { ticket, code });
            }
        }
    }

    // ------------------------------------------------------------------ entries the engine composes (§9.2)

    /// A new record of the engine's own (control, beat or `snap`): a fresh
    /// `entry_id`, `at_ms = wall_ms`, `based_on = (B, C_B)`, no trial apply.
    /// The caller persists it (`put`) and pushes it (`push_own`).
    pub(crate) fn compose(&mut self, kind: Kind, body: Body, via: Via, op: Option<u64>) -> Option<OutboxRecord> {
        let entry_id = self.sealer.new_entry_id().ok()?;
        Some(OutboxRecord {
            entry_id,
            order: self.next_order(),
            at_ms: self.wall,
            based_on: Head { seq: self.b, chain: self.cb },
            kind,
            body,
            origin_state: Vec::new(),
            via,
            hold: false,
            ctr: None,
            frame: None,
            frame_hash: None,
            epoch: None,
            ack: None,
            seen: None,
            op,
            reseals: 0,
        })
    }

    pub(crate) fn push_own(&mut self, r: OutboxRecord) {
        self.end_hold();
        self.outbox.push(Rec::new(r, None));
        self.status_dirty = true;
    }

    /// Composes, persists and queues one own record; returns its `entry_id`.
    pub(crate) fn enqueue_own(&mut self, kind: Kind, body: Body, via: Via, op: Option<u64>) -> Option<[u8; 16]> {
        let r = self.compose(kind, body, via, op)?;
        let id = r.entry_id;
        self.persist(vec![PersistOp::Outbox(vec![OutboxOp::Put(r.clone())])]);
        self.push_own(r);
        Some(id)
    }

    pub(crate) fn rec_index(&self, entry_id: &[u8; 16]) -> Option<usize> {
        self.outbox.iter().position(|r| &r.r.entry_id == entry_id)
    }

    /// Rewrites the outbox file when it holds more than `2 × live + 64` ops (§7.13).
    pub(crate) fn maybe_rewrite_outbox(&mut self) {
        if self.outbox_ops > 2 * self.outbox.len() + 64 {
            let live: Vec<OutboxRecord> = self.outbox.iter().map(|r| r.r.clone()).collect();
            self.persist(vec![PersistOp::RewriteOutbox(live)]);
        }
    }

    // ------------------------------------------------------------------ re-pair and resubmit (§24.5)

    /// `UserAction::ExportUnsent`: every `cmd` record that is not yet accepted, in `order`.
    pub(crate) fn export_unsent(&mut self) {
        let recs: Vec<&Rec<SM::Command>> = self.outbox.iter().filter(|r| r.r.kind == Kind::Cmd).collect();
        let commands: Vec<Vec<u8>> = recs
            .iter()
            .filter_map(|r| match &r.r.body {
                Body::Cmd(p) => Some(p.clone()),
                _ => None,
            })
            .collect();
        let origin_state = recs.last().map(|r| r.r.origin_state.clone()).unwrap_or_default();
        let from_device_no = self.no_b.or_else(|| self.me().map(|d| d.device_no)).unwrap_or(0);
        self.notify(Notification::Unsent { commands, from_device_no, origin_state });
    }

    /// `Resubmit` (ENG-EPOCH-003): rebind, then §9.1 steps 3 to 5 for each command in order.
    pub(crate) fn on_resubmit(
        &mut self,
        ticket: u64,
        commands: Vec<SM::Command>,
        from_device_no: u16,
        origin_state: Vec<u8>,
    ) {
        let n = commands.len() as u32;
        let all = |code: &'static str| (0..n).map(|i| (i, code)).collect::<Vec<_>>();
        if let Some(code) = self.enqueue_refusal(self.wall) {
            self.notify(Notification::ResubmitReport { ticket, queued: 0, refused: all(code) });
            return;
        }
        let (Some(d), Some(view)) = (self.no_b, self.view.as_ref()) else { return };
        let sm = &self.sm;
        let rebound = guard(|| sm.rebind(&commands, from_device_no, d, view, &origin_state));
        let (rebound, origin_after) = match rebound {
            Some(Ok(x)) => x,
            Some(Err(e)) => {
                self.notify(Notification::ResubmitReport { ticket, queued: 0, refused: all(e.code) });
                return;
            }
            None => {
                self.fault("engine/local_fault");
                self.notify(Notification::ResubmitReport { ticket, queued: 0, refused: all("engine/local_fault") });
                return;
            }
        };
        self.end_hold();
        let mut queued = 0u32;
        let mut refused = Vec::new();
        let mut last_pid = 0;
        let at_ms = self.wall;
        for (i, c) in rebound.into_iter().enumerate() {
            match self.enqueue_cmd(c, at_ms, origin_after.clone(), false) {
                Ok((pid, _, _)) => {
                    queued += 1;
                    last_pid = pid;
                }
                Err(code) => refused.push((i as u32, code)),
            }
        }
        self.after(last_pid, After::Notify(Notification::ResubmitReport { ticket, queued, refused }));
    }
}
