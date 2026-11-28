//! Move board, the move code, and following a move (§28.3).
//!
//! Rules: `Operation::Move` runs in stages kept in its `OpRecord`:
//! 1 door on the new relay; 2 the Fence on the old relay (by the `fence` admin
//! operation with its frame, or without a frame under an evidence stop, or not
//! at all when the old relay does not answer); 3 the replay for the seed's
//! membership and material, then `Seed`; 4 `SeedTail`; 5 the new target and,
//! if the old relay did not sequence the Fence, the Fence as an `append`
//! there; 6 the snapshot upload; 7 once the Fence is seen in the log and
//! reported; done when the snapshot is uploaded or given up. What a `Start`
//! takes from memory (the door, the generation, whether the old relay
//! sequenced the Fence) is read back from the durable records: the Fence
//! record in the outbox, the Fence in `M_K`, the new target, and the seed's
//! last seq in the operation's record (ENG-OPS-003).
//! Following (`UserAction::MoveCode`, `Operation::FollowAddress`) changes the
//! relay target only after the sealer checked a typed code against an accepted
//! `Fence` (ENG-MOVE-001); a candidate relay is only read, in a dry run that
//! persists nothing, and nothing is sent to it (ENG-MOVE-004).

use super::recv::Stash;
use super::replay::{Job, Purpose, Replayed};
use super::{Engine, Want, guard, lim};
use crate::{
    AdminOk, AdminRequest, BlobOk, BlobRequest, Body, Class, CtlIntent, CtlItem, DeviceStatus, Effect, Hash32, Kind,
    LocalReadResult, Notification, Operation, Page, RelayRefusal, RelayTarget, SealedBlob, Sealer, SeedDevice,
    SessionState, SnapInfo, SnapshotMaterial, StateMachine, Verdict, Via, state_hash,
};

#[derive(Default)]
pub(crate) struct MoveState {
    /// The latest accepted `Fence` this device saw: its seq and frame hash (for the move code).
    pub fence: Option<(u64, Hash32)>,
    pub fence_req: bool,
    // ---- the moving device
    pub door: Option<[u8; 5]>,
    pub home_gen: u32,
    /// Did the old relay sequence this move's Fence (the first case of stage 2)?
    pub old_answered: bool,
    /// The user said the old relay is gone: stage 2 no longer waits for it.
    pub gone: bool,
    pub fence_seq: Option<u64>,
    pub s: u64,
    pub first_seq: u64,
    pub carried: u64,
    pub blob: Option<SealedBlob>,
    pub info: Option<SnapInfo>,
    pub seed: Option<(Hash32, Vec<SeedDevice>)>,
    /// The first seq the new relay does not hold yet, as its answers say.
    pub tail_next: u64,
    /// The seq after the last frame of the `SeedTail` request in flight.
    pub tail_sent: u64,
    /// `Seed` requests this move may have sent, counting one for a `Start` at or after stage 3.
    pub seeds: u8,
    pub upload_failures: u8,
    pub uploaded: bool,
    /// A `Start` found the move reported (stage 7): the runtime may have stopped before it
    /// showed the code and the report, so both are emitted once more.
    pub again: bool,
    // ---- following an address (the candidate relay)
    pub follow: Option<Follow>,
}

pub(crate) struct Follow {
    pub ticket: u64,
    pub old: RelayTarget,
    pub host: String,
    pub door: [u8; 5],
    pub code: String,
    pub entries: Vec<(u64, Vec<u8>)>,
}

impl<SM: StateMachine, S: Sealer> Engine<SM, S> {
    // ------------------------------------------------------------------ every device: the Fence and the code

    /// An accepted `Fence` was committed (§28.3 "every other device" step 1).
    pub(crate) fn fence_accepted(&mut self, seq: u64, frame_hash: Hash32, _own: bool) {
        self.opx.mv.fence = Some((seq, frame_hash));
        self.status_dirty = true;
    }

    /// After a `Start` with a move pending, or with this device's own move past its switch (stages
    /// 6 and 7, whose code and report need the Fence: §28.3 "After a `Start`"), the Fence's frame
    /// hash is read back from the log.
    pub(crate) fn fence_lookup(&mut self) {
        let own_move = self.vk.home_seq > 0
            && self.vk.home_gen == self.meta.relay.home_gen
            && self.meta.ops.iter().any(|o| matches!(o.op, Operation::Move { .. }) && o.stage >= 6);
        if !(self.fence_pending() || own_move) || self.opx.mv.fence_req || self.pipe.tail {
            return;
        }
        let seq = self.vk.home_seq;
        if self.opx.mv.fence.is_some_and(|f| f.0 == seq) || seq < self.floor || seq > self.l {
            return;
        }
        self.opx.mv.fence_req = true;
        let req = self.want(Want::FenceFrame(seq));
        self.fx(Effect::ReadLog { req, from: seq, to: seq });
    }

    pub(crate) fn fence_frame(&mut self, seq: u64, result: LocalReadResult) {
        if let LocalReadResult::Records(v) = result
            && let Some(r) = v.first().filter(|r| r.seq == seq) {
                self.opx.mv.fence = Some((seq, self.sealer.frame_hash(&r.frame)));
            }
    }

    fn move_locked(&self) -> bool {
        self.wall < self.meta.move_tries.1
    }

    /// A wrong code: the fifth in a row locks entry for 3,600,000 ms (ENG-MOVE-002).
    fn wrong_code(&mut self) {
        let n = self.meta.move_tries.0.saturating_add(1);
        self.meta.move_tries = if n >= 5 { (0, self.wall.saturating_add(lim::HOUR)) } else { (n, self.meta.move_tries.1) };
        self.touch_meta();
    }

    /// `UserAction::MoveCode` (ENG-MOVE-001, ENG-MOVE-008): one `MoveCodeResult` in this call.
    pub(crate) fn on_move_code(&mut self, text: String) {
        let result = self.try_move_code(&text);
        self.notify(Notification::MoveCodeResult { result });
    }

    fn try_move_code(&mut self, text: &str) -> Result<(), &'static str> {
        if self.move_locked() {
            return Err("engine/move_locked");
        }
        let fence = self.opx.mv.fence.filter(|f| f.0 == self.vk.home_seq);
        let (true, Some((seq, hash))) = (self.fence_pending(), fence) else { return Err("engine/move_no_fence") };
        if self.sealer.check_move_code(seq, &hash, text).is_err() {
            self.wrong_code();
            return Err("seal/move_code");
        }
        let target =
            RelayTarget { host: self.vk.home_host.clone(), door: self.vk.home_door, home_gen: self.vk.home_gen };
        self.switch_relay(target);
        self.meta.move_tries = (0, 0);
        self.touch_meta();
        Ok(())
    }

    /// The relay target changes: the old session ends, and the session table connects to the new one.
    pub(crate) fn switch_relay(&mut self, target: RelayTarget) {
        self.meta.relay = target;
        self.touch_meta();
        if self.sess.state != SessionState::Offline {
            self.fx(Effect::Disconnect);
            self.on_disconnected(false, None);
        }
        self.sess.connect_mono = None;
        self.sess.refused_login = false;
        self.sess.no_board = false;
        self.sess.fenced_alarmed = false;
        self.sess.w = None;
        // ENG-EVID-005: an evidence stop ends when the device follows a Fence to a relay whose
        // log agrees with its own; the welcome there decides, so the alarms are resolved here.
        self.resolve_evidence(|_| true);
    }

    // ------------------------------------------------------------------ following an address (§28.3 step 4)

    pub(crate) fn follow_begin(&mut self, ticket: u64, op: Operation) {
        let Operation::FollowAddress { host, door, code } = &op else { return };
        if self.move_locked() {
            return self.notify(Notification::OpDone { ticket, result: Err("engine/move_locked") });
        }
        let follow = Follow {
            ticket,
            old: self.meta.relay.clone(),
            host: host.clone(),
            door: *door,
            code: code.clone(),
            entries: Vec::new(),
        };
        self.op_begin(ticket, op.clone(), 1);
        // The candidate is read, never written to: the target in `Meta` stays the old relay.
        let candidate = RelayTarget { host: follow.host.clone(), door: follow.door, home_gen: self.vk.home_gen + 1 };
        self.opx.mv.follow = Some(follow);
        if self.sess.state != SessionState::Offline {
            self.fx(Effect::Disconnect);
            self.on_disconnected(false, None);
        }
        self.sess.connect_mono = Some(self.mono);
        self.fx(Effect::Connect { relay: candidate });
        self.sess.state = SessionState::Connecting;
    }

    /// While a candidate relay is read, nothing is sent and its pages are not taken (ENG-MOVE-004).
    pub(crate) fn following(&self) -> bool {
        self.opx.mv.follow.is_some()
    }

    pub(crate) fn follow_holds_reads(&self) -> bool {
        false
    }

    fn follow_end(&mut self, result: Result<(), &'static str>) {
        let Some(f) = self.opx.mv.follow.take() else { return };
        if self.sess.state != SessionState::Offline {
            self.fx(Effect::Disconnect);
            self.on_disconnected(false, None);
        }
        self.sess.connect_mono = None;
        match result {
            Ok(()) => {
                let target = RelayTarget { host: f.host, door: f.door, home_gen: self.vk.home_gen + 1 };
                self.meta.relay = target;
                self.meta.move_tries = (0, 0);
                self.touch_meta();
            }
            Err("seal/move_code") => {
                self.meta.relay = f.old;
                self.wrong_code();
            }
            Err(_) => self.meta.relay = f.old,
        }
        self.op_done(f.ticket, result);
    }

    /// A page of the candidate relay: a dry run that persists nothing. True: the page is consumed.
    pub(crate) fn follow_page(&mut self, page: &Page) -> bool {
        if self.opx.mv.follow.is_none() {
            return false;
        }
        let mut next = self.l + 1;
        if let Some(f) = self.opx.mv.follow.as_mut() {
            next += f.entries.len() as u64;
            for e in &page.entries {
                if e.seq == next {
                    f.entries.push((e.seq, e.frame.clone()));
                    next += 1;
                }
            }
        }
        let (entries, host, door, code) = match self.opx.mv.follow.as_ref() {
            Some(f) => (f.entries.clone(), f.host.clone(), f.door, f.code.clone()),
            None => return true,
        };
        let stash = Stash { entries, head: None, from_log: false, keys_changed: false, quiet: true };
        let found = self.scan_for_fence(&stash, &host, &door);
        match found {
            Some((seq, hash)) => {
                let ok = self.sealer.check_move_code(seq, &hash, &code).is_ok();
                self.follow_end(if ok { Ok(()) } else { Err("seal/move_code") });
            }
            None => {
                if page.head.seq < next || (!page.more && page.entries.is_empty()) {
                    // The candidate's head is reached: no such Fence.
                    self.follow_end(Err("engine/move_no_fence"));
                } else if self.sess.read.is_none() {
                    let req = self.id();
                    self.sess.read = Some(req);
                    self.fx(Effect::Read { req, after: next - 1, limit: lim::PAGE_ENTRIES as u16 });
                }
            }
        }
        true
    }

    /// The welcome of a candidate relay: read from the log head on.
    pub(crate) fn follow_welcome(&mut self, head_seq: u64) -> bool {
        if self.opx.mv.follow.is_none() {
            return false;
        }
        if head_seq <= self.l {
            self.follow_end(Err("engine/move_no_fence"));
        } else {
            self.sess.state = SessionState::Reconciling;
            let req = self.id();
            self.sess.read = Some(req);
            let after = self.l;
            self.fx(Effect::Read { req, after, limit: lim::PAGE_ENTRIES as u16 });
        }
        true
    }

    /// Dry-runs `stash` and returns the accepted `Fence` naming `host` and `door`, if any.
    fn scan_for_fence(&mut self, stash: &Stash, host: &str, door: &[u8; 5]) -> Option<(u64, Hash32)> {
        let entries = self.dry_entries(stash);
        entries.iter().find_map(|e| {
            let o = e.opened.as_ref()?;
            (e.class == Class::Accept && o.kind == Kind::Ctl).then_some(())?;
            o.items.iter().find_map(|i| match i {
                CtlItem::Fence { host: h, door: d, .. } if h == host && d == door => Some((e.seq, e.frame_hash)),
                _ => None,
            })
        })
    }

    // ------------------------------------------------------------------ the moving device

    /// `UserAction::OldRelayGone`: a move waiting in stage 2 for its relay goes on without it.
    pub(crate) fn on_old_relay_gone(&mut self) {
        if self.meta.ops.iter().any(|o| matches!(o.op, Operation::Move { .. }) && o.stage <= 2) {
            self.opx.mv.gone = true;
        }
    }

    pub(crate) fn move_begin(&mut self, ticket: u64, op: Operation) {
        self.opx.mv.door = None;
        self.opx.mv.old_answered = false;
        self.opx.mv.gone = false;
        self.opx.mv.fence_seq = None;
        self.opx.mv.blob = None;
        self.opx.mv.info = None;
        self.opx.mv.seed = None;
        self.opx.mv.uploaded = false;
        self.opx.mv.seeds = 0;
        self.opx.mv.again = false;
        self.opx.mv.tail_next = 0;
        self.opx.mv.tail_sent = 0;
        self.opx.mv.upload_failures = 0;
        self.op_begin(ticket, op, 1);
    }

    fn mv_ready(&self, ticket: u64) -> bool {
        self.opx.mem.iter().find(|m| m.ticket == ticket).is_some_and(|m| !m.busy && self.is_durable(m.pid))
    }

    fn mv_busy(&mut self, ticket: u64, busy: bool) {
        if let Some(m) = self.opx.mem.iter_mut().find(|m| m.ticket == ticket) {
            m.busy = busy;
        }
    }

    fn mv_admin(&mut self, ticket: u64, op: AdminRequest) {
        self.mv_busy(ticket, true);
        let req = self.want(Want::OpAdmin(ticket));
        self.fx(Effect::Admin { req, op });
    }

    /// After a `Start` the move's memory is gone and its stage is not (ENG-OPS-003): what the
    /// durable records still say about it. False when none of them names the new relay's door.
    fn mv_restore(&mut self, rec: &crate::OpRecord, host: &str) -> bool {
        self.opx.mv.seeds = u8::from(rec.stage >= 3);
        self.opx.mv.again = rec.stage >= 7;
        self.opx.mv.carried = rec.mark;
        if rec.stage >= 6 {
            // The target changed in the write that recorded stage 6.
            self.opx.mv.door = Some(self.meta.relay.door);
            self.opx.mv.home_gen = self.meta.relay.home_gen;
            // The old relay sequenced the Fence exactly when it lies inside the seeded tail.
            self.opx.mv.old_answered =
                self.vk.home_gen == self.meta.relay.home_gen && self.vk.home_seq > 0 && self.vk.home_seq <= rec.mark;
            return true;
        }
        let waiting = self.outbox.iter().find(|r| r.r.op == Some(rec.ticket)).map(|r| r.r.via.clone());
        if let Some(Via::Fence { door, .. }) = waiting {
            // Stage 2: the Fence record is on its way; `move_entry_done` says when it is accepted.
            self.opx.mv.door = Some(door);
            return true;
        }
        if self.vk.home_seq > 0 && self.vk.home_gen == self.meta.relay.home_gen + 1 && self.vk.home_host == host {
            // This move's Fence, sequenced by the old relay (the first case of stage 2).
            self.opx.mv.door = Some(self.vk.home_door);
            self.opx.mv.home_gen = self.vk.home_gen;
            self.opx.mv.old_answered = true;
            self.opx.mv.fence_seq = Some(self.vk.home_seq);
            return true;
        }
        // No Fence record exists (the second and third cases of stage 2): the door is asked for again.
        self.opx.mv.home_gen = self.vk.home_gen + 1;
        false
    }

    /// Stage 4 is over: the seed's last seq goes into the record with stage 5.
    fn mv_switch(&mut self, ticket: u64) {
        let mark = self.opx.mv.carried;
        if let Some(o) = self.meta.ops.iter_mut().find(|o| o.ticket == ticket) {
            o.mark = mark;
        }
        self.op_stage(ticket, 5);
    }

    /// Looks at the move's stage again.
    pub(crate) fn move_drive(&mut self, ticket: u64) {
        let Some(rec) = self.op_rec(ticket).cloned() else { return };
        let Operation::Move { host } = &rec.op else { return };
        let host = host.clone();
        if !self.mv_ready(ticket) {
            return;
        }
        if rec.stage >= 2 && self.opx.mv.door.is_none() && !self.mv_restore(&rec, &host) {
            // Stage 1's call again, the stage kept: a relay has one door for a board.
            return self.mv_admin(ticket, AdminRequest::Door { host });
        }
        let relay_answers = matches!(self.sess.state, SessionState::Live | SessionState::Reconciling);
        match rec.stage {
            1 => {
                // The door of the board on the new relay; the runtime uses its create secret.
                self.mv_admin(ticket, AdminRequest::Door { host });
            }
            2 => {
                let Some(door) = self.opx.mv.door else { return };
                let home_gen = self.vk.home_gen + 1;
                if self.opx.mv.fence_seq.is_some() {
                    // ENG-MOVE-006: the old relay is read through its Fence before the seed.
                    if self.opx.mv.fence_seq.is_some_and(|f| self.l >= f && self.b >= f) {
                        self.opx.mv.home_gen = self.vk.home_gen;
                        self.op_stage(ticket, 3);
                    }
                } else if self.outbox.iter().any(|r| r.r.op == Some(ticket)) {
                    // The Fence record is on its way through the `fence` admin operation.
                } else if let Some(code) = self.seed_bounds().err().filter(|_| relay_answers || self.opx.mv.gone) {
                    // Nothing is fenced for a move whose seed cannot be made.
                    self.op_done(ticket, Err(code));
                } else if relay_answers && !self.evidence_stop() {
                    let via = Via::Fence { host: host.clone(), door, home_gen };
                    self.op_enqueue(ticket, CtlIntent::Fence { host, door, home_gen }, via);
                } else if relay_answers {
                    // Under an evidence stop: fence the old relay without a frame.
                    self.mv_admin(ticket, AdminRequest::Fence { host, door, home_gen, frame: None });
                } else if self.opx.mv.gone {
                    // The user says the old relay is gone: on to the seed at once.
                    self.opx.mv.home_gen = home_gen;
                    self.op_stage(ticket, 3);
                }
                // Otherwise the move waits for the session: a relay that does not answer right
                // now is not gone, and seeding without its Fence would leave it accepting entries.
            }
            3 => self.move_seed(ticket, host),
            // After a `Start` the device seals and seeds again: a new seed replaces an unfinished one.
            4 if self.opx.mv.seed.is_none() => self.op_stage(ticket, 3),
            4 => self.move_tail(ticket),
            5 => {
                let Some(door) = self.opx.mv.door else { return };
                let home_gen = self.opx.mv.home_gen;
                self.switch_relay(RelayTarget { host: host.clone(), door, home_gen });
                if !self.opx.mv.old_answered && !self.outbox.iter().any(|r| r.r.op == Some(ticket)) {
                    // ENG-MOVE-005: the Fence goes to the new relay as an ordinary append.
                    self.op_enqueue(ticket, CtlIntent::Fence { host, door, home_gen }, Via::Append);
                }
                self.op_stage(ticket, 6);
            }
            6 | 7 => {
                // The snapshot of stage 3, once the session on the new relay is Live (ENG-MOVE-007).
                if self.sess.state == SessionState::Live && !self.opx.mv.uploaded && self.opx.mv.upload_failures < 3 {
                    if let Some(blob) = self.opx.mv.blob.clone() {
                        self.mv_busy(ticket, true);
                        let req = self.want(Want::OpBlob(ticket));
                        let seq = self.opx.mv.s;
                        self.fx(Effect::Blob { req, op: BlobRequest::Upload { purpose: 1, seq, blob } });
                        return;
                    }
                    // After a `Start` it is sealed again, with a new id (§28.3 stage 6).
                    self.move_reseal(ticket);
                    if !self.mv_ready(ticket) {
                        return;
                    }
                }
                // Stage 7 is the report (ENG-MOVE-009): written with its stage, so a process makes it once.
                if rec.stage == 6 {
                    if let Some(notes) = self.move_notes() {
                        for n in notes {
                            self.notify_durable(n);
                        }
                        self.op_stage(ticket, 7);
                    }
                    return;
                }
                if self.opx.mv.again {
                    let Some(notes) = self.move_notes() else { return };
                    self.opx.mv.again = false;
                    for n in notes {
                        self.notify(n);
                    }
                }
                // Done when the Fence is reported and the snapshot is uploaded or given up.
                if self.opx.mv.uploaded || self.opx.mv.upload_failures >= 3 {
                    self.op_done(ticket, Ok(()));
                }
            }
            _ => {}
        }
    }

    /// Stage 7: the code and the report, once the Fence is seen in the log (ENG-MOVE-009).
    fn move_notes(&self) -> Option<[Notification; 2]> {
        if self.fence_pending() || self.vk.home_gen != self.meta.relay.home_gen {
            return None;
        }
        let (seq, hash) = self.opx.mv.fence.filter(|f| f.0 == self.vk.home_seq)?;
        let text = self.sealer.move_code(seq, &hash);
        let report = Notification::MoveReport {
            carried_through: self.opx.mv.carried,
            old_relay_answered: self.opx.mv.old_answered,
        };
        Some([Notification::MoveCode { text }, report])
    }

    /// Stage 6 after a `Start`: the move's snapshot is sealed again from this device's state at `S`.
    fn move_reseal(&mut self, ticket: u64) {
        if self.jobs.busy() {
            return;
        }
        let Ok((s, _)) = self.seed_bounds() else {
            self.opx.mv.upload_failures = 3;
            return;
        };
        let own = self.own;
        let recorded = self.registry.snapshots.iter().any(|r| r.author == own && r.info.seq == s)
            || self.outbox.iter().any(|r| matches!(&r.r.body, Body::Snap(i) if i.seq == s));
        if recorded {
            // Uploaded and announced before the `Start`.
            self.opx.mv.uploaded = true;
            return;
        }
        self.opx.mv.s = s;
        self.opx.mv.first_seq = s + 1;
        let mut job = Job::new(Purpose::Seed { ticket }, s, s);
        job.want_states = vec![s];
        if self.start_job(job) {
            self.mv_busy(ticket, true);
        } else {
            self.opx.mv.upload_failures = 3;
        }
    }

    /// The Fence record's entry reached the board head.
    pub(crate) fn move_entry_done(&mut self, ticket: u64, seq: u64, code: Option<&'static str>) {
        match code {
            None => {
                if self.op_rec(ticket).is_some_and(|o| o.stage == 2) {
                    self.opx.mv.old_answered = true;
                    self.opx.mv.fence_seq = Some(seq);
                }
            }
            Some(c) => {
                self.op_done(ticket, Err(c));
            }
        }
    }

    pub(crate) fn move_fence_refused(&mut self, ticket: u64, code: &'static str) {
        self.op_done(ticket, Err(code));
    }

    /// The answer of one of the move's admin calls.
    pub(crate) fn move_admin(&mut self, ticket: u64, result: Result<AdminOk, RelayRefusal>) {
        let Some(stage) = self.op_rec(ticket).map(|o| o.stage) else { return };
        if stage >= 2 && self.opx.mv.door.is_none() {
            // The door asked for again after a `Start`.
            return match result {
                Ok(AdminOk { door: Some(d), .. }) => self.opx.mv.door = Some(d),
                Ok(_) => self.op_done(ticket, Err("relay/refused")),
                Err(r) => self.op_done(ticket, Err(super::send::intern(&r.code))),
            };
        }
        match (stage, result) {
            (1, Ok(ok)) => match ok.door {
                Some(d) => {
                    self.opx.mv.door = Some(d);
                    self.op_stage(ticket, 2);
                }
                None => self.op_done(ticket, Err("relay/refused")),
            },
            (2, Ok(_)) => {
                // The old relay is fenced without a frame; the Fence follows on the new relay.
                self.opx.mv.home_gen = self.vk.home_gen + 1;
                self.op_stage(ticket, 3);
            }
            (3, Ok(_)) => {
                self.opx.mv.tail_next = self.opx.mv.first_seq;
                self.op_stage(ticket, 4);
            }
            (3, Err(r)) if r.code == "relay/exists" && self.opx.mv.seeds >= 2 => {
                // A seed sent before (in this process or before a `Start`) had its whole tail
                // stored: the board is open there, and what this device holds beyond that tail
                // it carries as on any relay behind its log (§21.3).
                self.mv_switch(ticket);
            }
            (4, Ok(ok)) => {
                // Only an answer moves the tail on: a request nobody answered is sent again.
                self.opx.mv.tail_next = ok.head.map_or(self.opx.mv.tail_sent, |h| h.seq + 1);
                if self.opx.mv.tail_next > self.opx.mv.carried {
                    self.mv_switch(ticket);
                }
            }
            (4, Err(r)) if r.code == "relay/target" && self.opx.mv.tail_sent > self.opx.mv.carried => {
                // The last request of the tail was stored and its answer lost: the board is open
                // and takes no tail any more (`relay.md` §12.7).
                self.mv_switch(ticket);
            }
            (4, Err(r)) if r.code == "relay/not_next" && r.head.is_some() => {
                // Resume from the head the relay names.
                self.opx.mv.tail_next = r.head.map_or(self.opx.mv.first_seq, |h| h.seq + 1);
                if self.opx.mv.tail_next > self.opx.mv.carried {
                    self.mv_switch(ticket);
                }
            }
            (_, Err(r)) => self.op_done(ticket, Err(super::send::intern(&r.code))),
            _ => {}
        }
    }

    /// Stage 3: `S`, `first_seq`, then the replay that gives `M_{first_seq-1}` and the material at `S`.
    /// Stage 3's `S` and `first_seq` (§28.3), or why no seed can be made now.
    fn seed_bounds(&self) -> Result<(u64, u64), &'static str> {
        // The newest usable snapshot this device acknowledged, or wrote and another acknowledged.
        let usable_seq = self
            .registry
            .snapshots
            .iter()
            .filter(|r| super::duty::usable(r, &self.own))
            .filter(|r| r.own == Some(Verdict::Ack) || r.author == self.own)
            .map(|r| r.info.seq)
            .filter(|s| *s <= self.b && self.base_at_or_below(*s).is_some())
            .max();
        let s = usable_seq.or_else(|| self.bases.last().map(|b| b.0)).unwrap_or(self.b);
        let window = (self.l + 1).saturating_sub(lim::SKIP_WINDOW);
        let mut first = (s + 1).min(self.vk.purge_seq.max(window).max(1));
        // The frames this device holds, and a base at or below the seq before the first.
        first = first.max(self.floor);
        // A frame this device classified `Skip` is one the new relay refuses in a seed tail
        // (`relay.md` §12.7): the tail starts above the newest of them. The tail ends at this
        // move's Fence when the old relay sequenced it, and what lies above is never carried.
        let last = self.opx.mv.fence_seq.filter(|_| self.opx.mv.old_answered).unwrap_or(self.l);
        let unverified = self
            .meta
            .alarms
            .iter()
            .filter(|a| a.kind == crate::AlarmKind::SkippedFrame)
            .filter_map(|a| a.seq)
            .filter(|x| *x >= first && *x <= last)
            .max();
        if let Some(x) = unverified {
            if x > s {
                // No snapshot lies above it yet: the move waits for one.
                return Err("engine/move_tail");
            }
            first = x + 1;
        }
        // The seed's `chain_before` is `C_{first_seq-1}`: a seq whose chain this device holds.
        while first > 1
            && first <= s + 1
            && (self.base_at_or_below(first - 1).is_none() || self.chain_held(first - 1).is_none())
        {
            first += 1;
        }
        // Before seq 1 there is nothing to rebuild (`M_0`, every counter zero): no base is needed for it.
        if first > s + 1
            || (first > 1 && (self.base_at_or_below((first - 1).min(s)).is_none() || self.chain_held(first - 1).is_none()))
        {
            return Err("engine/local_fault");
        }
        Ok((s, first))
    }

    /// `C_x` as this device holds it: from its chain index, or from a base at `x` (§7.10); `C_0`
    /// for the seq before the first.
    fn chain_held(&self, x: u64) -> Option<crate::Hash32> {
        if x == 0 {
            return Some(self.sealer.chain_zero());
        }
        self.chain_at(x).or_else(|| self.bases.iter().find(|b| b.0 == x).map(|b| b.1))
    }

    fn move_seed(&mut self, ticket: u64, host: String) {
        if let Some((chain_before, devices)) = self.opx.mv.seed.clone() {
            // The replay is done and the fresh snapshot sealed: seed the new relay (ENG-MOVE-003).
            let Some(info) = self.opx.mv.info.clone() else { return };
            let last = self.opx.mv.carried;
            let op = AdminRequest::Seed {
                host,
                home_gen: self.opx.mv.home_gen,
                first_seq: self.opx.mv.first_seq,
                chain_before,
                last_seq: last,
                chain_last: self.chain_at(last).unwrap_or(self.cb),
                blob_id: info.blob,
                devices,
            };
            self.opx.mv.seeds = self.opx.mv.seeds.saturating_add(1);
            return self.mv_admin(ticket, op);
        }
        if self.jobs.busy() {
            return;
        }
        let (s, first) = match self.seed_bounds() {
            Ok(x) => x,
            Err(code) => return self.op_done(ticket, Err(code)),
        };
        self.opx.mv.s = s;
        self.opx.mv.first_seq = first;
        // The tail ends at this move's Fence when the old relay sequenced it: what a fenced relay
        // shows above its Fence was not accepted from a device.
        self.opx.mv.carried = self.opx.mv.fence_seq.filter(|_| self.opx.mv.old_answered).unwrap_or(self.l);
        let from = if first > 1 { (first - 1).min(s) } else { s };
        let mut job = Job::new(Purpose::Seed { ticket }, from, (first - 1).max(s));
        job.want_states = if first > 1 { vec![first - 1, s] } else { vec![s] };
        if self.start_job(job) {
            self.mv_busy(ticket, true);
        } else {
            self.op_done(ticket, Err("engine/local_fault"));
        }
    }

    /// The seed's replay is done: the device list and the fresh snapshot.
    pub(crate) fn seed_replayed(&mut self, ticket: u64, out: Replayed<SM, S>) {
        self.mv_busy(ticket, false);
        if self.op_rec(ticket).is_none() {
            return;
        }
        let (first, s) = (self.opx.mv.first_seq, self.opx.mv.s);
        let late = self.op_rec(ticket).is_some_and(|o| o.stage >= 6);
        // From stage 6 on only the snapshot is at stake, and the move does not fail for it.
        let fail = |me: &mut Self| {
            if late {
                me.opx.mv.upload_failures = 3;
            } else {
                me.op_done(ticket, Err("engine/local_fault"));
            }
        };
        let Some(at_s) = out.stash.iter().find(|x| x.0 == s) else { return fail(self) };
        let m_before = if first > 1 {
            let Some(before) = out.stash.iter().find(|x| x.0 == first - 1) else { return fail(self) };
            let Ok(m) = self.sealer.membership_from_bytes(&before.2) else { return fail(self) };
            m
        } else {
            // Before seq 1 nobody had sealed anything.
            self.sealer.membership_zero()
        };
        let v0 = self.sealer.view(&m_before);
        let v = &self.vk;
        // Every device and recovery identity of M_L; a revoked device or a retired recovery
        // identity only when it authors a frame at or after first_seq.
        let mut devices = Vec::new();
        for d in &v.devices {
            let last0 = v0.devices.iter().find(|x| x.dev == d.dev).map_or(0, |x| x.last_ctr);
            let current = d.status == DeviceStatus::Current;
            if current || d.last_ctr > last0 {
                devices.push(SeedDevice {
                    dev: d.dev,
                    sign_pk: d.sign_pk,
                    role: 1,
                    status: if current { 1 } else { 2 },
                    last_ctr: last0,
                });
            }
        }
        for r in &v.recovery {
            let last0 = v0.recovery.iter().find(|x| x.dev_r == r.dev_r).map_or(0, |x| x.last_ctr);
            let current = r.until_seq == 0;
            if current || r.last_ctr > last0 {
                devices.push(SeedDevice {
                    dev: r.dev_r,
                    sign_pk: r.sign_pk,
                    role: 2,
                    status: if current { 1 } else { 2 },
                    last_ctr: last0,
                });
            }
        }
        devices.sort_by_key(|a| a.dev);
        // A fresh snapshot of this device's own state at S, with itself as writer.
        let sm = &self.sm;
        let (Some(board), Some(bh)) = (guard(|| sm.canonical_bytes(&at_s.1)), guard(|| sm.board_hash(&at_s.1))) else {
            return fail(self);
        };
        let hash = state_hash(1, &bh, &at_s.2);
        if self.hashes.get(&s).is_some_and(|h| h.hash != hash) {
            return fail(self);
        }
        let Some(chain) = self.chain_at(s) else { return fail(self) };
        let material =
            SnapshotMaterial { seq: s, chain, hash_version: 1, state_hash: hash, board, membership: at_s.2.clone() };
        let Some(mk) = self.mk.clone() else { return fail(self) };
        match self.sealer.seal_snapshot(&mk, &material) {
            Err(_) if late => self.opx.mv.upload_failures = 3,
            Err(e) => self.op_done(ticket, Err(e.code)),
            Ok(blob) => {
                self.opx.mv.info = Some(SnapInfo {
                    seq: s,
                    hash,
                    hash_version: 1,
                    blob: blob.id,
                    sha256: blob.sha256,
                    size: blob.bytes.len() as u64,
                    parts: blob.parts,
                    inflated: blob.inflated,
                    epoch: blob.epoch,
                });
                self.opx.mv.blob = Some(blob);
                let Some(chain_before) = self.chain_held(first - 1) else { return fail(self) };
                self.opx.mv.seed = Some((chain_before, devices));
            }
        }
    }

    /// Stage 4: the frames `first_seq..=L` from the log, at most 256 frames and 1,048,576 bytes a request.
    fn move_tail(&mut self, ticket: u64) {
        let next = self.opx.mv.tail_next.max(self.opx.mv.first_seq);
        let last = self.opx.mv.carried;
        if next > last {
            return self.mv_switch(ticket);
        }
        self.mv_busy(ticket, true);
        let to = last.min(next + lim::PAGE_ENTRIES as u64 - 1);
        let req = self.want(Want::MoveLog(ticket));
        self.fx(Effect::ReadLog { req, from: next, to });
    }

    pub(crate) fn move_log(&mut self, ticket: u64, result: LocalReadResult) {
        self.mv_busy(ticket, false);
        let Some(Operation::Move { host }) = self.op_rec(ticket).map(|o| o.op.clone()) else { return };
        let LocalReadResult::Records(records) = result else {
            self.fault("engine/local_fault");
            return self.op_done(ticket, Err("engine/local_fault"));
        };
        let first = records.first().map_or(0, |r| r.seq);
        let mut frames = Vec::new();
        let mut bytes = 0usize;
        for r in records {
            if !frames.is_empty() && bytes + r.frame.len() > lim::PAGE_BYTES {
                break;
            }
            bytes += r.frame.len();
            frames.push(r.frame);
        }
        if frames.is_empty() {
            return self.op_done(ticket, Err("engine/local_fault"));
        }
        self.opx.mv.tail_sent = first + frames.len() as u64;
        self.mv_admin(ticket, AdminRequest::SeedTail { host, first, frames });
    }

    /// Stage 6: the upload of the move's snapshot finished (§19.4 step 4 applies).
    pub(crate) fn move_blob(&mut self, ticket: u64, result: Result<BlobOk, RelayRefusal>) {
        match result {
            Ok(BlobOk::Uploaded) => {
                self.opx.mv.uploaded = true;
                self.opx.mv.blob = None;
                if let Some(info) = self.opx.mv.info.clone() {
                    self.enqueue_own(Kind::Snap, Body::Snap(info), Via::Append, None);
                }
            }
            _ => self.opx.mv.upload_failures += 1,
        }
        let _ = ticket;
    }

}
