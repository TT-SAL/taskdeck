//! Send (§10): when the engine sends, the send loop, counters, append results,
//! conflicts and gaps (§11.4), the echo check (§11.5), records carried by an
//! admin operation (§11.7), and the epoch gate (§24.2).
//!
//! Rules: the loop is evaluated again on every call. A batch is the leading
//! unacknowledged, unseen records that hold a frame, then newly sealed ones;
//! it leaves only when every frame in it is durable (`Rec::pid`, D2) and no
//! `Append` or frame-carrying `Admin` is outstanding (ENG-EFFECT-003). A frame
//! is sealed once and its bytes are reused for every retry (ENG-SEALER-005).

use super::{Engine, Want, lim};
use crate::{
    AdminOk, AdminRequest, AlarmKind, AppendResult, Body, CtlIntent, Effect, Evidence, Kind, OutboxOp, PersistOp,
    PlainEntry, RelayRefusal, Sealer, SessionState, StateMachine, Via,
};

#[derive(Default)]
pub(crate) struct Send {
    /// The outstanding `Append`: its id and the records of its frames, in order.
    pub append: Option<(u64, Vec<[u8; 16]>)>,
    /// The outstanding frame-carrying `Admin`: its id, the record and the counter it was sealed with.
    pub admin: Option<(u64, [u8; 16], u64)>,
    /// Records acknowledged since the last `Persist`: their `Put`s go into the next one (§10.4).
    pub ack_puts: Vec<[u8; 16]>,
    /// A `WriteIdentity` that sealing waits for (ENG-CTL-002).
    pub identity_pid: u64,
    /// The write that cleared frames for a re-seal (D9).
    pub clear_pid: u64,
    /// A conflict asked for a new reconcile: nothing is sealed before its welcome (§11.4).
    pub await_welcome: bool,
    /// The counters of the unmatched conflicts in a row.
    pub conflict_ctrs: Vec<u64>,
    /// Records acknowledged through a conflict whose chain value is not known yet.
    pub chain_pending: Vec<[u8; 16]>,
    /// A `gap` result's `last_ctr`: the relay holds this device's frames up to that counter, so
    /// they are not resent (§11.4); the next welcome's reconcile takes over.
    pub relay_ctr: Option<u64>,
}

impl<SM: StateMachine, S: Sealer> Engine<SM, S> {
    /// The largest counter of a record that holds a frame (the `h` of §10.3 before `a`).
    pub(crate) fn sealed_high(&self) -> u64 {
        self.outbox.iter().filter(|r| r.r.frame.is_some()).filter_map(|r| r.r.ctr).max().unwrap_or(0)
    }

    /// `next_ctr = 1 + max(a, h, w)` (ENG-SEND-006, ENG-RECON-009).
    pub(crate) fn next_ctr(&self) -> u64 {
        let a = self.me().map_or(0, |d| d.last_ctr);
        1 + a.max(self.sealed_high()).max(self.sess.w.unwrap_or(0))
    }

    /// Is a move pending: an accepted `Fence` leads away from this device's relay (§28.3)?
    pub(crate) fn fence_pending(&self) -> bool {
        self.vk.home_seq > 0 && self.vk.home_gen > self.meta.relay.home_gen
    }

    /// §10.1: may the engine send now?
    pub(crate) fn may_send(&self) -> bool {
        self.sess.state == SessionState::Live
            && !self.stopped()
            && !self.sess.fenced
            && !self.fence_pending()
            && !self.following()
            && !self.pipe.tail
            && self.recon_done()
            && !self.send.await_welcome
            && !self.sess.wait_welcome
            && self.send.append.is_none()
            && self.send.admin.is_none()
            && self.sess.pause_append_until.is_none_or(|t| self.wall >= t)
            && self.sess.pause_writes_until.is_none_or(|t| self.wall >= t)
            && self.no_b.is_some()
    }

    /// Step 6 of §4.5: the send loop of §10.2.
    pub(crate) fn step_send(&mut self) {
        if !self.may_send() {
            return;
        }
        let halted = self.halted();
        if halted && self.crash_halt_seq().is_none() {
            return; // ENG-HALT-006: only a crash-halted device sends, and only its Skip
        }
        let is_skip = |b: &Body| matches!(b, Body::Ctl(CtlIntent::Skip { .. }));
        // Step 1.
        let relay_ctr = self.send.relay_ctr;
        let r: Vec<usize> = (0..self.outbox.len())
            .filter(|&i| {
                let x = &self.outbox[i].r;
                x.ack.is_none()
                    && x.seen.is_none()
                    && (!halted || is_skip(&x.body))
                    && !(x.frame.is_some() && x.ctr.is_some_and(|c| relay_ctr.is_some_and(|rc| c <= rc)))
            })
            .collect();
        let Some(&first) = r.first() else { return };
        // Step 2: a record carried by an admin operation goes alone (ENG-SEND-005).
        if self.outbox[first].r.via != Via::Append {
            let earlier_done = self.outbox[..first].iter().all(|x| x.r.ack.is_some() || x.r.seen.is_some());
            if earlier_done {
                self.send_carried(first);
            }
            return;
        }
        // Step 3: resends.
        let mut batch: Vec<usize> = Vec::new();
        let mut bytes = 0usize;
        let mut k = 0;
        while k < r.len() && batch.len() < lim::BATCH_FRAMES {
            let x = &self.outbox[r[k]].r;
            let Some(f) = &x.frame else { break };
            if x.via != Via::Append || bytes + f.len() > lim::BATCH_BYTES {
                break;
            }
            bytes += f.len();
            batch.push(r[k]);
            k += 1;
        }
        // Step 4: new seals, in order (ENG-SEND-004), behind the epoch gate (ENG-EPOCH-001).
        let epoch_now = self.vk.epoch;
        let gate_closed = self
            .outbox
            .iter()
            .any(|x| x.r.frame.is_some() && x.r.seen.is_none() && x.r.epoch.is_some_and(|e| e < epoch_now));
        let waits_write = !self.is_durable(self.send.identity_pid) || !self.is_durable(self.send.clear_pid);
        let mut sealed: Vec<usize> = Vec::new();
        let mut keys_changed = false;
        let mut dropped = Vec::new();
        if !gate_closed && !waits_write {
            let last = self.outbox.len().saturating_sub(1);
            while k < r.len() && batch.len() < lim::BATCH_FRAMES {
                let i = r[k];
                if self.outbox[i].r.frame.is_some() || self.outbox[i].r.via != Via::Append {
                    break;
                }
                if i == last && self.last_is_held() {
                    break; // ENG-HOLD-001
                }
                if let Body::Snap(info) = &self.outbox[i].r.body
                    && info.epoch != epoch_now {
                        // ENG-SNAP-004: an unsent `snap` record of another epoch is dropped; the duty stays.
                        dropped.push(i);
                        k += 1;
                        continue;
                    }
                match self.seal_record(i) {
                    Ok(changed) => {
                        keys_changed |= changed;
                        let len = self.outbox[i].r.frame.as_ref().map_or(0, Vec::len);
                        if bytes + len > lim::BATCH_BYTES && !batch.is_empty() {
                            sealed.push(i);
                            break;
                        }
                        bytes += len;
                        sealed.push(i);
                        batch.push(i);
                        k += 1;
                    }
                    Err(code) => {
                        self.seal_failed(i, code);
                        return;
                    }
                }
            }
        }
        if !sealed.is_empty() {
            let mut ops =
                vec![PersistOp::Outbox(sealed.iter().map(|&i| OutboxOp::Put(self.outbox[i].r.clone())).collect())];
            if keys_changed {
                ops.push(PersistOp::WriteIdentity(self.sealer.export_identity()));
            }
            let pid = self.persist(ops);
            for &i in &sealed {
                self.outbox[i].pid = pid;
            }
        }
        if !dropped.is_empty() {
            let ids: Vec<[u8; 16]> = dropped.iter().map(|&i| self.outbox[i].r.entry_id).collect();
            self.outbox.retain(|x| !ids.contains(&x.r.entry_id));
            self.persist(vec![PersistOp::Outbox(ids.into_iter().map(OutboxOp::Drop).collect())]);
            return; // indices moved; the next call goes on
        }
        // Step 5.
        if batch.is_empty() || batch.iter().any(|&i| !self.is_durable(self.outbox[i].pid)) {
            return;
        }
        let frames: Vec<Vec<u8>> = batch.iter().filter_map(|&i| self.outbox[i].r.frame.clone()).collect();
        let ids: Vec<[u8; 16]> = batch.iter().map(|&i| self.outbox[i].r.entry_id).collect();
        let req = self.id();
        self.send.append = Some((req, ids));
        self.fx(Effect::Append { req, frames });
    }

    /// Seals the record at `i` with `next_ctr` under `M_K` (ENG-SEND-002). True: a secret changed.
    fn seal_record(&mut self, i: usize) -> Result<bool, &'static str> {
        let ctr = self.next_ctr();
        let Some(m) = self.mk.as_ref() else { return Err("engine/local_fault") };
        let x = &self.outbox[i].r;
        let entry = PlainEntry { entry_id: x.entry_id, at_ms: x.at_ms, based_on: x.based_on, body: x.body.clone() };
        let sealed = self.sealer.seal(m, ctr, &entry).map_err(|e| e.code)?;
        let x = &mut self.outbox[i].r;
        x.ctr = Some(ctr);
        x.frame_hash = Some(sealed.frame_hash);
        x.epoch = Some(sealed.epoch);
        x.frame = Some(sealed.frame);
        Ok(sealed.keys_changed)
    }

    /// The sealer would not seal the record at `i`: an operation's record fails
    /// its stage; anything else is a local fault.
    fn seal_failed(&mut self, i: usize, code: &'static str) {
        if self.outbox[i].r.kind == Kind::Cmd {
            self.fault("engine/local_fault");
            return;
        }
        let rec = self.outbox.remove(i);
        self.persist(vec![PersistOp::Outbox(vec![OutboxOp::Drop(rec.r.entry_id)])]);
        self.own_entry_done(&rec.r, 0, Some(code));
    }

    /// §10.2 step 2: the record at `i` travels in its admin operation.
    fn send_carried(&mut self, i: usize) {
        if self.outbox[i].r.frame.is_none() {
            if !self.is_durable(self.send.identity_pid) || !self.is_durable(self.send.clear_pid) {
                return;
            }
            if !self.before_carried_seal(i) {
                return; // the pairing raced a rotation: the record was dropped (ENG-OPS-006)
            }
            match self.seal_record(i) {
                Ok(changed) => {
                    let mut ops = vec![PersistOp::Outbox(vec![OutboxOp::Put(self.outbox[i].r.clone())])];
                    if changed {
                        ops.push(PersistOp::WriteIdentity(self.sealer.export_identity()));
                    }
                    self.outbox[i].pid = self.persist(ops);
                }
                Err(code) => {
                    self.seal_failed(i, code);
                    return;
                }
            }
        }
        if !self.is_durable(self.outbox[i].pid) {
            return;
        }
        let x = &self.outbox[i].r;
        let (Some(frame), Some(ctr)) = (x.frame.clone(), x.ctr) else { return };
        let op = match &x.via {
            Via::Register { dev, sign_pk } => AdminRequest::Register { frame, dev: *dev, sign_pk: *sign_pk },
            Via::Fence { host, door, home_gen } => {
                AdminRequest::Fence { host: host.clone(), door: *door, home_gen: *home_gen, frame: Some(frame) }
            }
            Via::Append => return,
        };
        let id = x.entry_id;
        let req = self.id();
        self.send.admin = Some((req, id, ctr));
        self.fx(Effect::Admin { req, op });
    }

    // ------------------------------------------------------------------ append results (§10.4)

    /// The `Outbox` op of the acknowledgement `Put`s left for the next `Persist`.
    pub(crate) fn take_ack_puts(&mut self) -> Option<PersistOp> {
        let ids = std::mem::take(&mut self.send.ack_puts);
        let puts: Vec<OutboxOp> = ids
            .iter()
            .filter_map(|id| self.rec_index(id))
            .map(|i| OutboxOp::Put(self.outbox[i].r.clone()))
            .collect();
        if puts.is_empty() { None } else { Some(PersistOp::Outbox(puts)) }
    }

    pub(crate) fn on_append_results(&mut self, req: u64, results: Vec<AppendResult>) {
        let Some((r, ids)) = self.send.append.take() else { return };
        if r != req {
            self.send.append = Some((r, ids));
            return;
        }
        if results.len() > lim::BATCH_FRAMES || results.len() > ids.len() {
            self.protocol("append_results");
            return;
        }
        for (id, result) in ids.into_iter().zip(results) {
            match result {
                AppendResult::New { seq, chain } | AppendResult::Dup { seq, chain } => {
                    self.acknowledge(&id, seq, Some(chain));
                    if self.meta.conflicts != 0 {
                        self.meta.conflicts = 0;
                        self.send.conflict_ctrs.clear();
                        self.touch_meta();
                    }
                }
                AppendResult::Conflict { seq, frame } => self.on_conflict(&id, seq, &frame),
                AppendResult::Gap { last_ctr, last_seq: _ } => self.on_gap(last_ctr),
                AppendResult::Refused { status, code } => self.on_frame_refused(&id, status, &code),
                AppendResult::Skipped => {}
            }
        }
    }

    /// ENG-RESULT-001: the acknowledgement of a record; with the echo check when the log holds the seq.
    fn acknowledge(&mut self, id: &[u8; 16], seq: u64, chain: Option<[u8; 32]>) {
        let Some(i) = self.rec_index(id) else { return };
        let known = chain.or_else(|| self.chain_at(seq));
        self.outbox[i].r.ack = Some((seq, known.unwrap_or([0; 32])));
        if known.is_none() {
            self.send.chain_pending.push(*id);
        }
        self.send.ack_puts.push(*id);
        self.status_dirty = true;
        if seq <= self.l
            && let Some(c) = chain {
                // ENG-ECHO-001: the log already holds that seq.
                let rec = &self.outbox[i].r;
                let log_chain = self.chain_at(seq);
                if rec.seen != Some(seq) || log_chain != Some(c) {
                    self.raise_evidence(
                        AlarmKind::Unechoed,
                        Some(seq),
                        None,
                        None,
                        Evidence::Unechoed {
                            ack_frame_hash: rec.frame_hash.unwrap_or([0; 32]),
                            ack_chain: c,
                            log_frame_hash: [0; 32],
                            log_chain: log_chain.unwrap_or([0; 32]),
                        },
                    );
                }
            }
    }

    /// ENG-ECHO-001 at the commit of the entry at `seq`: every record acknowledged
    /// at that seq must be the frame the log now holds there.
    pub(crate) fn echo_entry(&mut self, seq: u64, chain: [u8; 32], frame_hash: [u8; 32]) {
        let mut bad = None;
        for rec in self.outbox.iter_mut() {
            let Some((s, c)) = rec.r.ack else { continue };
            if s != seq {
                continue;
            }
            if let Some(p) = self.send.chain_pending.iter().position(|x| *x == rec.r.entry_id) {
                // Acknowledged through a conflict: the chain value is known only now.
                self.send.chain_pending.remove(p);
                rec.r.ack = Some((s, chain));
                if rec.r.frame_hash == Some(frame_hash) {
                    continue;
                }
            } else if rec.r.frame_hash == Some(frame_hash) && c == chain {
                continue;
            }
            bad = Some((rec.r.frame_hash.unwrap_or([0; 32]), c));
        }
        if let Some((ack_frame_hash, ack_chain)) = bad {
            self.raise_evidence(
                AlarmKind::Unechoed,
                Some(seq),
                None,
                None,
                Evidence::Unechoed { ack_frame_hash, ack_chain, log_frame_hash: frame_hash, log_chain: chain },
            );
        }
    }

    /// §11.3 step 3 (ENG-RECON-003): during a restored-copy catch-up, a record whose
    /// frame sits at the counter of an accepted own frame of the relay, with another
    /// hash and another `entry_id`, is cleared for re-sealing.
    pub(crate) fn restored_clear(&mut self, e: &super::recv::Entry<S::Membership>) {
        let Some((a, z)) = self.registry.restored else { return };
        if e.ctr < a || e.ctr > z {
            return;
        }
        let eid = e.opened.as_ref().map(|o| o.entry_id);
        let idx: Vec<usize> = (0..self.outbox.len())
            .filter(|&i| {
                let r = &self.outbox[i].r;
                r.ctr == Some(e.ctr) && r.frame_hash != Some(e.frame_hash) && Some(r.entry_id) != eid
            })
            .collect();
        for i in idx {
            self.clear_frame(i);
        }
    }

    /// Clears the frame of the record at `i` for sealing again under a new counter (D9).
    fn clear_frame(&mut self, i: usize) {
        let rec = &mut self.outbox[i];
        rec.r.ctr = None;
        rec.r.frame = None;
        rec.r.frame_hash = None;
        rec.r.epoch = None;
        rec.r.ack = None;
        let put = OutboxOp::Put(rec.r.clone());
        let pid = self.persist(vec![PersistOp::Outbox(vec![put])]);
        self.outbox[i].pid = pid;
        self.send.clear_pid = pid;
    }

    /// §11.4: a `conflict` result for the record `id`.
    fn on_conflict(&mut self, id: &[u8; 16], seq: u64, frame: &[u8]) {
        let Some(x) = self.rec_index(id) else { return };
        let ctr = self.outbox[x].r.ctr.unwrap_or(0);
        let mut matched = false;
        if seq != 0 {
            let opened = match self.mk.as_ref() {
                Some(m) => self.sealer.open_own(m, frame).ok(),
                None => None,
            };
            match opened {
                Some(eid) => match self.rec_index(&eid) {
                    Some(y) => {
                        // ENG-RECON-005: the relay holds that record as an earlier copy sealed it.
                        let hash = self.sealer.frame_hash(frame);
                        self.outbox[y].r.frame_hash = Some(hash);
                        self.acknowledge(&eid, seq, None);
                        if y != x {
                            self.clear_frame(x);
                        }
                        matched = true;
                    }
                    None => {
                        self.alarm(
                            AlarmKind::RestoredCopy,
                            None,
                            None,
                            None,
                            Evidence::RestoredCopy { entries: 1 },
                        );
                        self.clear_frame(x);
                    }
                },
                None => {
                    self.alarm(
                        AlarmKind::RelayProtocol,
                        Some(seq),
                        None,
                        Some("relay/conflict"),
                        Evidence::Request { request: format!("append ctr {ctr}") },
                    );
                    self.clear_frame(x);
                }
            }
        } else {
            self.clear_frame(x);
        }
        if matched {
            self.meta.conflicts = 0;
            self.send.conflict_ctrs.clear();
        } else {
            self.meta.conflicts = self.meta.conflicts.saturating_add(1);
            self.send.conflict_ctrs.push(ctr);
        }
        self.touch_meta();
        // A new reconcile before anything is sealed again.
        self.send.await_welcome = true;
        self.sess.need_hello = true;
        if self.meta.conflicts >= 3 {
            // ENG-RECON-006.
            let ctrs = std::mem::take(&mut self.send.conflict_ctrs);
            self.identity_clone(None, ctrs);
        }
    }

    /// §11.4: a `gap` result (ENG-RECON-007).
    fn on_gap(&mut self, last_ctr: u64) {
        let a = self.me().map_or(0, |d| d.last_ctr);
        let ack_above = self
            .outbox
            .iter()
            .filter(|r| r.r.ctr.is_some_and(|c| c > last_ctr))
            .filter_map(|r| r.r.ack)
            .max_by_key(|x| x.0);
        if ack_above.is_some() || a > last_ctr {
            let local_head = self.log_head();
            let relay_head = self.sess.relay_head.unwrap_or(local_head);
            self.raise_evidence(
                AlarmKind::RelayBehind,
                None,
                None,
                None,
                Evidence::RelayBehind { relay_head, relay_last_ctr: Some(last_ctr), local_head, ack: ack_above },
            );
        }
        // Otherwise the engine resends, identical, from the record with counter `last_ctr + 1`:
        // the frames up to it are at the relay and come back with the log. A hello shows how far.
        if ack_above.is_none() && a <= last_ctr {
            self.send.relay_ctr = Some(last_ctr);
            self.sess.need_hello = true;
        }
    }

    /// §10.4: the relay refused one frame.
    fn on_frame_refused(&mut self, id: &[u8; 16], status: u16, code: &str) {
        match (status, code) {
            (429, "relay/budget") => {
                // ENG-RESULT-002: no `Append` until the next 00:00 UTC; the frame stays sealed.
                self.sess.pause_append_until = Some(super::sess::next_utc_midnight(self.wall));
                self.status_dirty = true;
            }
            (403, "relay/revoked") => self.set_revoked(false),
            (507, "relay/storage_refused") => {
                self.sess.pause_writes_until = Some(self.wall + 60_000);
            }
            (400, "relay/bad_signature" | "relay/bad_frame" | "relay/wrong_board" | "relay/unknown_author")
            | (403, "relay/not_author") => {
                // ENG-RESULT-003: the frame is neither changed nor dropped.
                let ctr = self.rec_index(id).and_then(|i| self.outbox[i].r.ctr).unwrap_or(0);
                self.alarm(
                    AlarmKind::RelayProtocol,
                    None,
                    None,
                    Some(code),
                    Evidence::Request { request: format!("append ctr {ctr}") },
                );
                self.fault("engine/own_frame_refused");
            }
            _ => {
                self.alarm(
                    AlarmKind::RelayProtocol,
                    None,
                    None,
                    Some(code),
                    Evidence::Request { request: "append".to_owned() },
                );
                self.sess.wait_welcome = true;
            }
        }
    }

    // ------------------------------------------------------------------ admin answers for carried records (§11.7)

    /// The answer of the frame-carrying `Admin` `req`. True: it was this one.
    pub(crate) fn carried_result(&mut self, req: u64, result: &Result<AdminOk, RelayRefusal>) -> bool {
        let Some((r, id, ctr)) = self.send.admin else { return false };
        if r != req {
            return false;
        }
        self.send.admin = None;
        match result {
            Ok(ok) => {
                if let (Some(seq), Some(chain)) = (ok.seq, ok.chain) {
                    if self.rec_index(&id).is_some() {
                        self.acknowledge(&id, seq, Some(chain));
                    } else if seq <= self.l && self.chain_at(seq) != Some(chain) {
                        // ENG-ECHO-002: the record left with the push; the answer must match the log.
                        self.raise_evidence(
                            AlarmKind::Unechoed,
                            Some(seq),
                            None,
                            None,
                            Evidence::Unechoed {
                                ack_frame_hash: [0; 32],
                                ack_chain: chain,
                                log_frame_hash: [0; 32],
                                log_chain: self.chain_at(seq).unwrap_or([0; 32]),
                            },
                        );
                    }
                    let _ = ctr;
                    self.carried_done(&id, Ok((seq, chain)));
                }
            }
            Err(refusal) => match (refusal.status, refusal.code.as_str()) {
                (409, "relay/conflict") | (409, "relay/gap") => {
                    // §11.7: mapped to §11.4. The frame may already be stored: read, then
                    // register it by seq when it is seen in the log.
                    self.send.await_welcome = true;
                    self.sess.need_hello = true;
                    self.carried_conflict(&id);
                }
                (429, _) | (507, _) => {
                    self.sess.pause_writes_until =
                        Some(refusal.until.unwrap_or(self.wall + 60_000));
                }
                (_, code) => {
                    let code = intern(code);
                    if let Some(i) = self.rec_index(&id) {
                        let rec = self.outbox.remove(i);
                        self.persist(vec![PersistOp::Outbox(vec![OutboxOp::Drop(rec.r.entry_id)])]);
                    }
                    self.carried_done(&id, Err(code));
                }
            },
        }
        true
    }

    /// The name of the outstanding request `req` for an alarm (§21.1 `Request`),
    /// and the request is forgotten.
    pub(crate) fn forget_request(&mut self, req: u64) -> Option<&'static str> {
        if self.sess.read == Some(req) {
            self.sess.read = None;
            if self.pipe.need_req.is_some_and(|r| r.0 == req) {
                self.pipe.need_req = None;
            }
            return Some("read");
        }
        if self.send.append.as_ref().is_some_and(|a| a.0 == req) {
            self.send.append = None;
            return Some("append");
        }
        if self.send.admin.is_some_and(|a| a.0 == req) {
            self.send.admin = None;
            return Some("admin/frame");
        }
        match self.take_want(req) {
            Some(w) => Some(self.want_refused(w)),
            None => None,
        }
    }


    pub(crate) fn want_refused(&mut self, w: Want) -> &'static str {
        self.want_lost(w)
    }
}

/// Relay codes the engine passes on as `&'static str` (§33: codes of `relay.md` §19 pass through).
pub(crate) fn intern(code: &str) -> &'static str {
    const KNOWN: &[&str] = &[
        "relay/unauthorized",
        "relay/revoked",
        "relay/fenced",
        "relay/budget",
        "relay/delete_budget",
        "relay/no_board",
        "relay/seeding",
        "relay/storage_refused",
        "relay/conflict",
        "relay/gap",
        "relay/target",
        "relay/too_recent",
        "relay/no_snapshot",
        "relay/not_next",
        "relay/chain_mismatch",
        "relay/home_gen",
        "relay/bad_args",
        "relay/bad_frame",
        "relay/bad_signature",
        "relay/wrong_board",
        "relay/not_author",
        "relay/unknown_author",
        "relay/too_large",
        "relay/device_cap",
        "relay/blob_cap",
        "relay/no_blob",
        "relay/token_stale",
        "relay/no_nonce",
        "relay/exists",
    ];
    KNOWN.iter().copied().find(|k| *k == code).unwrap_or("relay/refused")
}
