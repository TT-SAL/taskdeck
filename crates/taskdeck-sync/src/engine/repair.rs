//! Repairs (§21.3): restore tail. A device with `RelayBehind` evidence whose
//! log extends the relay's chain sends the relay what it holds, at the
//! original seqs (ENG-EVID-004, ENG-EVID-007).
//!
//! Rules: `evid.tail` is the restore in progress. Frames come from the durable
//! log (`ReadLog`), at most 64 frames and 1,048,576 bytes a request; a request
//! ends with the first control entry it contains, and the repairs that entry
//! needs at the relay (`RegisterSeq`, `RecoveryRegisterSeq`, `Revoke`, `Fence`
//! without a frame, `Purge`) go out before the next request. The evidence is
//! resolved only by a welcome whose head is the log head with this device's
//! chain, or is beyond it (ENG-EVID-005).

use std::collections::VecDeque;

use super::{Engine, Want, lim};
use crate::{
    AdminOk, AdminRequest, AlarmKind, DeviceStatus, Effect, Evidence, ForkSource, Head, LocalReadResult,
    RelayRefusal, Sealer, SessionState, StateMachine,
};

pub(crate) struct Tail {
    /// The next seq to restore.
    pub next: u64,
    pub busy: bool,
    /// The repairs the control entry just restored needs, in item order.
    pub repairs: VecDeque<AdminRequest>,
    /// The last seq of the request in flight.
    pub sending_to: u64,
}

impl<SM: StateMachine, S: Sealer> Engine<SM, S> {
    /// A welcome arrived under evidence (§11.2 "only its step 1"): the repair looks at the relay's head.
    pub(crate) fn repair_on_welcome(&mut self, head: Head) {
        if !self.has_unresolved(AlarmKind::RelayBehind) {
            return;
        }
        let acks_echo = self.outbox.iter().filter_map(|r| r.r.ack).all(|a| a.0 <= head.seq);
        if head.seq >= self.l && acks_echo && self.chain_at(self.l).is_some() {
            if head.seq > self.l || self.chain_at(self.l) == Some(head.chain) {
                // §21.3 step 5: the relay holds the log head again. Beyond it, the pages that
                // follow are checked against this device's chain like any others.
                self.evid.tail = None;
                self.sess.relay_head = Some(head);
                self.resolve_evidence(|k| k == AlarmKind::RelayBehind);
            }
            return;
        }
        // The relay is a prefix of this device's log: a plain rollback, restored by whoever is awake.
        let prefix = head.seq < self.l && self.chain_at(head.seq) == Some(head.chain);
        let may = self.current_member() && !self.halted() && self.meta.stops.iter().all(|s| s.kind == crate::StopKind::Evidence);
        if prefix && may {
            match self.evid.tail.as_mut() {
                Some(t) if t.busy => {}
                Some(t) => t.next = head.seq + 1,
                None => {
                    self.evid.tail =
                        Some(Tail { next: head.seq + 1, busy: false, repairs: VecDeque::new(), sending_to: 0 })
                }
            }
        }
    }

    /// Looked at on every call: the next repair, the next request, or the hello that confirms.
    pub(crate) fn repair_step(&mut self) {
        if self.dead || self.sess.state == SessionState::Offline {
            return;
        }
        let Some(t) = self.evid.tail.as_mut() else { return };
        if t.busy {
            return;
        }
        if let Some(op) = t.repairs.pop_front() {
            t.busy = true;
            let req = self.want(Want::Repair);
            self.fx(Effect::Admin { req, op });
            return;
        }
        if t.next > self.l {
            // Everything is restored: a hello shows the relay's head, and the welcome resolves.
            self.evid.tail = None;
            self.say_hello();
            return;
        }
        // At most 64 seqs, ending with the first control entry (ENG-EVID-004).
        let from = t.next;
        let mut to = self.l.min(from + 63);
        if let Some(c) = self.vk.ctl_seqs.iter().copied().find(|c| *c >= from && *c <= to) {
            to = c;
        }
        t.busy = true;
        let req = self.want(Want::RestoreChunk);
        self.fx(Effect::ReadLog { req, from, to });
    }

    /// The frames of a restore-tail request arrived from the log (ENG-EVID-007).
    pub(crate) fn restore_chunk(&mut self, result: LocalReadResult) {
        let Some(t) = self.evid.tail.as_mut() else { return };
        let records = match result {
            LocalReadResult::Records(v) if v.first().is_some_and(|r| r.seq == t.next) => v,
            _ => {
                // `Missing`: nothing is sent.
                self.evid.tail = None;
                self.fault("engine/local_fault");
                return;
            }
        };
        let first = t.next;
        let mut frames = Vec::new();
        let mut bytes = 0usize;
        let mut last = first;
        let mut last_hash = [0u8; 32];
        for r in records {
            if !frames.is_empty() && bytes + r.frame.len() > lim::PAGE_BYTES {
                break;
            }
            bytes += r.frame.len();
            last = r.seq;
            last_hash = self.sealer.frame_hash(&r.frame);
            frames.push(r.frame);
        }
        let Some(chain_before) = self.chain_at(first - 1) else {
            self.evid.tail = None;
            self.fault("engine/local_fault");
            return;
        };
        let repairs = if self.vk.ctl_seqs.contains(&last) { self.repairs_for(last, last_hash) } else { VecDeque::new() };
        if let Some(t) = self.evid.tail.as_mut() {
            t.sending_to = last;
            t.repairs = repairs;
        }
        let req = self.want(Want::RestoreCall);
        self.fx(Effect::Admin { req, op: AdminRequest::RestoreTail { first_seq: first, chain_before, frames } });
    }

    /// §21.3 step 2: what the relay needs for the accepted control entry at `s`, from `M_K`.
    fn repairs_for(&mut self, s: u64, frame_hash: [u8; 32]) -> VecDeque<AdminRequest> {
        let mut out = VecDeque::new();
        let v = self.vk.clone();
        for d in &v.devices {
            if d.added_seq == s && s > 1 {
                out.push_back(AdminRequest::RegisterSeq { seq: s, dev: d.dev, sign_pk: d.sign_pk, frame_hash });
            }
        }
        for r in &v.recovery {
            if r.since_seq == s && s > 1 {
                // ENG-OPS-005: only in seq mode, only for an entry classified `Accept`.
                out.push_back(AdminRequest::RecoveryRegisterSeq { seq: s, dev_r: r.dev_r, sign_pk: r.sign_pk, frame_hash });
            }
        }
        for d in &v.devices {
            if d.status_seq != s || d.added_seq == s {
                continue;
            }
            match d.status {
                DeviceStatus::Revoked if d.dev != self.own => out.push_back(AdminRequest::Revoke { dev: d.dev }),
                DeviceStatus::Revoked => {}
                DeviceStatus::Current => {
                    // A `DeviceReinstated` needs the recovery identity's signature.
                    self.alarm(
                        AlarmKind::RelayProtocol,
                        Some(s),
                        None,
                        Some("engine/restore_incomplete"),
                        Evidence::Request { request: "admin/reinstate".to_owned() },
                    );
                }
            }
        }
        if v.home_seq == s && s > 1 {
            out.push_back(AdminRequest::Fence {
                host: v.home_host.clone(),
                door: v.home_door,
                home_gen: v.home_gen,
                frame: None,
            });
        }
        if v.epoch_seq == s && v.purge_seq > 0 && self.pipe.shred.is_some_and(|x| x.0 == v.epoch) {
            out.push_back(AdminRequest::Purge { before_seq: v.purge_seq, epoch: v.epoch });
            self.meta.duties.purge = Some((v.purge_seq, v.epoch));
            self.touch_meta();
        }
        out
    }

    /// The answer of a `RestoreTail` (§21.3 steps 3 and 4).
    pub(crate) fn restore_result(&mut self, result: Result<AdminOk, RelayRefusal>) {
        let Some(t) = self.evid.tail.as_mut() else { return };
        t.busy = false;
        match result {
            Ok(_) => t.next = t.sending_to + 1,
            Err(r) if r.status == 409 && r.code == "relay/not_next" => {
                t.repairs.clear();
                match r.head {
                    Some(h) if h.seq <= self.l && self.chain_at(h.seq) == Some(h.chain) => {
                        if let Some(t) = self.evid.tail.as_mut() {
                            t.next = h.seq + 1;
                        }
                    }
                    Some(h) if h.seq > self.l => {
                        // Another device restored further: the welcome decides.
                        self.evid.tail = None;
                        self.say_hello();
                    }
                    Some(h) => {
                        self.evid.tail = None;
                        let local = self.chain_at(h.seq).unwrap_or([0; 32]);
                        self.raise_evidence(
                            AlarmKind::Fork,
                            Some(h.seq),
                            None,
                            None,
                            Evidence::Fork { local_chain: local, relay_chain: h.chain, source: ForkSource::Welcome },
                        );
                    }
                    None => self.evid.tail = None,
                }
            }
            Err(r) if r.status == 409 && r.code == "relay/chain_mismatch" => {
                let at = t.next.saturating_sub(1);
                self.evid.tail = None;
                let local = self.chain_at(at).unwrap_or([0; 32]);
                self.raise_evidence(
                    AlarmKind::Fork,
                    Some(at),
                    None,
                    Some("relay/chain_mismatch"),
                    Evidence::Fork { local_chain: local, relay_chain: [0; 32], source: ForkSource::Welcome },
                );
            }
            Err(r) => {
                self.evid.tail = None;
                self.alarm(
                    AlarmKind::RelayProtocol,
                    None,
                    None,
                    Some(&r.code),
                    Evidence::Request { request: "admin/restore_tail".to_owned() },
                );
            }
        }
    }

    /// The answer of one repair call: an already repaired row (`relay/target`) counts as done.
    pub(crate) fn repair_result(&mut self, result: Result<AdminOk, RelayRefusal>) {
        if let Some(t) = self.evid.tail.as_mut() {
            t.busy = false;
        }
        if let Err(r) = result {
            let done = r.status == 409 && matches!(r.code.as_str(), "relay/target" | "relay/conflict" | "relay/home_gen");
            if !done && r.status != 429 {
                self.alarm(
                    AlarmKind::RelayProtocol,
                    None,
                    None,
                    Some(&r.code),
                    Evidence::Request { request: "admin/repair".to_owned() },
                );
            }
        }
    }

    /// A repair call got no answer: the restore starts again from the next welcome.
    pub(crate) fn repair_lost(&mut self) {
        self.evid.tail = None;
    }
}
