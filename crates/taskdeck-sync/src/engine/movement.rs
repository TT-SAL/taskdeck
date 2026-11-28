//! Move board, the move code, and following a move (§28.3).
//!
//! Rules: `Operation::Move` runs in stages kept in its `OpRecord`:
//! 1 door on the new relay; 2 the Fence on the old relay (by the `fence` admin
//! operation with its frame, or without a frame under an evidence stop, or not
//! at all when the old relay does not answer); 3 the replay for the seed's
//! membership and material, then `Seed`; 4 `SeedTail`; 5 the new target and,
//! if the old relay did not sequence the Fence, the Fence as an `append`
//! there; 6 the snapshot upload; done when the Fence is seen in the log.
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
    pub fence_seq: Option<u64>,
    pub s: u64,
    pub first_seq: u64,
    pub carried: u64,
    pub blob: Option<SealedBlob>,
    pub info: Option<SnapInfo>,
    pub seed: Option<(Hash32, Vec<SeedDevice>)>,
    pub tail_next: u64,
    pub upload_failures: u8,
    pub uploaded: bool,
    pub reported: bool,
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

    /// After a `Start` with a move pending, the Fence's frame hash is read back from the log.
    pub(crate) fn fence_lookup(&mut self) {
        if !self.fence_pending() || self.opx.mv.fence_req || self.pipe.tail {
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

    pub(crate) fn move_begin(&mut self, ticket: u64, op: Operation) {
        self.opx.mv.door = None;
        self.opx.mv.old_answered = false;
        self.opx.mv.fence_seq = None;
        self.opx.mv.blob = None;
        self.opx.mv.info = None;
        self.opx.mv.seed = None;
        self.opx.mv.uploaded = false;
        self.opx.mv.reported = false;
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

    /// Looks at the move's stage again.
    pub(crate) fn move_drive(&mut self, ticket: u64) {
        let Some(rec) = self.op_rec(ticket).cloned() else { return };
        let Operation::Move { host } = &rec.op else { return };
        let host = host.clone();
        if !self.mv_ready(ticket) {
            return;
        }
        let relay_answers = matches!(self.sess.state, SessionState::Live | SessionState::Reconciling);
        match rec.stage {
            1 => {
                // The door of the board on the new relay; the runtime uses its create secret.
                self.mv_admin(ticket, AdminRequest::Door { host });
            }
            2 => {
                let Some(door) = self.opx.mv.door else { return self.op_stage(ticket, 1) };
                let home_gen = self.vk.home_gen + 1;
                if self.opx.mv.fence_seq.is_some() {
                    // ENG-MOVE-006: the old relay is read through its Fence before the seed.
                    if self.opx.mv.fence_seq.is_some_and(|f| self.l >= f && self.b >= f) {
                        self.opx.mv.home_gen = self.vk.home_gen;
                        self.op_stage(ticket, 3);
                    }
                } else if self.outbox.iter().any(|r| r.r.op == Some(ticket)) {
                    // The Fence record is on its way through the `fence` admin operation.
                } else if relay_answers && !self.evidence_stop() {
                    let via = Via::Fence { host: host.clone(), door, home_gen };
                    self.op_enqueue(ticket, CtlIntent::Fence { host, door, home_gen }, via);
                } else if relay_answers {
                    // Under an evidence stop: fence the old relay without a frame.
                    self.mv_admin(ticket, AdminRequest::Fence { host, door, home_gen, frame: None });
                } else {
                    // The old relay does not answer: on to the seed at once.
                    self.opx.mv.home_gen = home_gen;
                    self.op_stage(ticket, 3);
                }
            }
            3 => self.move_seed(ticket, host),
            4 => self.move_tail(ticket),
            5 => {
                let Some(door) = self.opx.mv.door else { return self.op_stage(ticket, 1) };
                let home_gen = self.opx.mv.home_gen;
                self.switch_relay(RelayTarget { host: host.clone(), door, home_gen });
                if !self.opx.mv.old_answered && !self.outbox.iter().any(|r| r.r.op == Some(ticket)) {
                    // ENG-MOVE-005: the Fence goes to the new relay as an ordinary append.
                    self.op_enqueue(ticket, CtlIntent::Fence { host, door, home_gen }, Via::Append);
                }
                self.op_stage(ticket, 6);
            }
            6 => {
                // The snapshot of stage 3, once the session on the new relay is Live (ENG-MOVE-007).
                if self.sess.state == SessionState::Live && !self.opx.mv.uploaded && self.opx.mv.upload_failures < 3
                    && let Some(blob) = self.opx.mv.blob.clone() {
                        self.mv_busy(ticket, true);
                        let req = self.want(Want::OpBlob(ticket));
                        let seq = self.opx.mv.s;
                        self.fx(Effect::Blob { req, op: BlobRequest::Upload { purpose: 1, seq, blob } });
                    }
                self.move_report(ticket);
            }
            _ => {}
        }
    }

    /// Stage 7: the Fence is seen in the log (ENG-MOVE-009).
    fn move_report(&mut self, ticket: u64) {
        if self.opx.mv.reported || self.fence_pending() || self.vk.home_gen != self.meta.relay.home_gen {
            return;
        }
        let Some((seq, hash)) = self.opx.mv.fence.filter(|f| f.0 == self.vk.home_seq) else { return };
        self.opx.mv.reported = true;
        let text = self.sealer.move_code(seq, &hash);
        self.notify_durable(Notification::MoveCode { text });
        self.notify_durable(Notification::MoveReport {
            carried_through: self.opx.mv.carried,
            old_relay_answered: self.opx.mv.old_answered,
        });
        self.op_done(ticket, Ok(()));
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
            (4, Ok(ok)) => {
                if let Some(h) = ok.head {
                    self.opx.mv.tail_next = h.seq + 1;
                }
                if self.opx.mv.tail_next > self.opx.mv.carried {
                    self.op_stage(ticket, 5);
                }
            }
            (4, Err(r)) if r.code == "relay/not_next" && r.head.is_some() => {
                // Resume from the head the relay names.
                self.opx.mv.tail_next = r.head.map_or(self.opx.mv.first_seq, |h| h.seq + 1);
                if self.opx.mv.tail_next > self.opx.mv.carried {
                    self.op_stage(ticket, 5);
                }
            }
            (_, Err(r)) => self.op_done(ticket, Err(super::send::intern(&r.code))),
            _ => {}
        }
    }

    /// Stage 3: `S`, `first_seq`, then the replay that gives `M_{first_seq-1}` and the material at `S`.
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
            return self.mv_admin(ticket, op);
        }
        if self.jobs.busy() {
            return;
        }
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
        while first > 1 && first <= s + 1 && self.base_at_or_below(first - 1).is_none() {
            first += 1;
        }
        if first > s + 1 || self.base_at_or_below((first - 1).min(s)).is_none() {
            return self.op_done(ticket, Err("engine/local_fault"));
        }
        self.opx.mv.s = s;
        self.opx.mv.first_seq = first;
        self.opx.mv.carried = self.l;
        let mut job = Job::new(Purpose::Seed { ticket }, (first - 1).min(s), (first - 1).max(s));
        job.want_states = vec![first - 1, s];
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
        let fail = |me: &mut Self| me.op_done(ticket, Err("engine/local_fault"));
        let before = out.stash.iter().find(|x| x.0 == first - 1);
        let at_s = out.stash.iter().find(|x| x.0 == s);
        let (Some(before), Some(at_s)) = (before, at_s) else { return fail(self) };
        let Ok(m_before) = self.sealer.membership_from_bytes(&before.2) else { return fail(self) };
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
                let chain_before = self.chain_at(first - 1).unwrap_or_else(|| self.sealer.chain_zero());
                self.opx.mv.seed = Some((chain_before, devices));
            }
        }
    }

    /// Stage 4: the frames `first_seq..=L` from the log, at most 256 frames and 1,048,576 bytes a request.
    fn move_tail(&mut self, ticket: u64) {
        let next = self.opx.mv.tail_next.max(self.opx.mv.first_seq);
        let last = self.opx.mv.carried;
        if next > last {
            return self.op_stage(ticket, 5);
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
        self.opx.mv.tail_next = first + frames.len() as u64;
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
