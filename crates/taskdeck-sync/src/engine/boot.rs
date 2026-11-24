//! Rejoin after a long absence (§19.7) and the device that cannot replay a
//! Skip or `ResetTo` (§19.9).
//!
//! Rules: a rejoin replaces the log only after `UserAction::ConfirmRejoin`
//! (ENG-BOOT-001); the session is `Rejoining` until a candidate is installed,
//! then it reads on like any session while `boot.cur` waits for the `snap`
//! entry that announces the candidate and for the snapshot to be usable at the
//! relay's head (ENG-BOOT-002). The rebootstrap stop holds the board; the
//! classified entries wait in `pipe.queue`, where the engine looks for a
//! usable snapshot above the entry it could not apply (ENG-BOOT-005).

use super::{Engine, Want, guard};
use crate::{
    AlarmKind, BlobMeta, BlobOk, BlobRequest, Class, DeviceStatus, Dev, Effect, Evidence, Kind, PersistOp,
    RelayRefusal, Sealer, SessionState, SnapInfo, StateMachine, StopKind, Verdict, state_hash,
};

pub(crate) struct Candidate {
    pub meta: BlobMeta,
    pub epoch: u32,
    pub inflated: u64,
    pub hash: [u8; 32],
    pub hash_version: u32,
    /// The `snap` entry naming this blob was classified and matched what was opened.
    pub announced: Option<u64>,
}

#[derive(Default)]
pub(crate) struct Boot {
    pub confirmed: bool,
    pub busy: bool,
    pub candidates: Vec<BlobMeta>,
    pub fetching: Option<BlobMeta>,
    pub cur: Option<Candidate>,
    /// Snapshots the rebootstrap passed over, and the one it is fetching.
    pub bad: Vec<[u8; 16]>,
    pub reboot: Option<(u64, Dev, SnapInfo)>,
}

impl<SM: StateMachine, S: Sealer> Engine<SM, S> {
    // ------------------------------------------------------------------ rejoin (§19.7)

    /// Step 1: the relay no longer holds the log this device needs (ENG-BOOT-007).
    pub(crate) fn enter_rejoin(&mut self) {
        if self.sess.state != SessionState::Rejoining && self.boot.cur.is_none() {
            self.sess.state = SessionState::Rejoining;
            self.sess.rejoin_since = Some(self.wall);
            self.boot.confirmed = false;
            self.status_dirty = true;
        }
    }

    /// `UserAction::ConfirmRejoin`: step 2.
    pub(crate) fn confirm_rejoin(&mut self) {
        if self.sess.state != SessionState::Rejoining || self.boot.confirmed {
            return;
        }
        self.boot.confirmed = true;
        // The state before the rejoin is kept as a base (ENG-BOOT-001).
        self.write_base();
        self.boot.busy = true;
        let req = self.want(Want::RejoinList);
        self.fx(Effect::Blob { req, op: BlobRequest::List });
    }

    pub(crate) fn rejoin_blob(&mut self, w: Want, result: Result<BlobOk, RelayRefusal>) {
        self.boot.busy = false;
        match (w, result) {
            (Want::RejoinList, Ok(BlobOk::List(list))) => {
                let low = self.sess.floor.saturating_sub(1);
                let mut c: Vec<BlobMeta> =
                    list.into_iter().filter(|m| m.complete && m.purpose == 1 && m.seq >= low).collect();
                c.sort_by(|a, b| b.seq.cmp(&a.seq).then(a.id.cmp(&b.id)));
                self.boot.candidates = c;
                self.next_candidate();
            }
            (Want::RejoinFetch, Ok(BlobOk::Parts(parts))) => self.candidate_parts(parts),
            (Want::RejoinFetch, _) => self.next_candidate(),
            (Want::RejoinList, _) => self.boot.confirmed = false, // asked again at the next confirmation
            _ => {}
        }
    }

    /// A blob request of the rejoin got no answer: the user confirms again once reconnected.
    pub(crate) fn rejoin_lost(&mut self) {
        self.boot.busy = false;
        if self.boot.cur.is_none() {
            self.boot.confirmed = false;
            self.boot.fetching = None;
        }
    }

    /// Step 3 for the next candidate, or step 6 when none is left.
    fn next_candidate(&mut self) {
        if self.boot.candidates.is_empty() {
            self.boot.fetching = None;
            self.boot.cur = None;
            self.add_stop(StopKind::Stranded, None);
            return;
        }
        let meta = self.boot.candidates.remove(0);
        let (id, parts) = (meta.id, meta.parts);
        self.boot.fetching = Some(meta);
        self.boot.busy = true;
        let req = self.want(Want::RejoinFetch);
        self.fx(Effect::Blob { req, op: BlobRequest::Fetch { id, parts } });
    }

    fn candidate_parts(&mut self, parts: Vec<Vec<u8>>) {
        let Some(meta) = self.boot.fetching.take() else { return };
        let expected = SnapInfo {
            seq: meta.seq,
            hash: [0; 32],
            hash_version: 1,
            blob: meta.id,
            sha256: meta.sha256,
            size: meta.size,
            parts: meta.parts,
            inflated: 0,
            epoch: 0,
        };
        let refs: Vec<&[u8]> = parts.iter().map(Vec::as_slice).collect();
        let sealer = &self.sealer;
        // The declared mode of `seal.md` §20.5: only seq, blob, sha256, size and parts are read.
        let opened = guard(|| sealer.open_snapshot(&refs, &expected, &meta.author, false));
        let Some(Ok((material, epoch, inflated))) = opened else { return self.next_candidate() };
        let Ok(m) = self.sealer.membership_from_bytes(&material.membership) else { return self.next_candidate() };
        let v = self.sealer.view(&m);
        let member = v.devices.iter().any(|d| d.dev == self.own && d.status == DeviceStatus::Current);
        if v.board_id != self.vk.board_id || !member || material.seq != meta.seq {
            return self.next_candidate();
        }
        let sm = &self.sm;
        let state = match guard(|| sm.load(&material.board)) {
            Some(Ok(s)) => s,
            Some(Err(e)) => {
                self.alarm(AlarmKind::BlobFailure, None, Some(meta.author), Some(e.code), Evidence::Blob { blob: meta.id });
                return self.next_candidate();
            }
            None => return self.next_candidate(),
        };
        let rules_ok = guard(|| sm.rules(&state)) == Some(v.rules);
        let hash_ok = guard(|| sm.board_hash(&state))
            .is_some_and(|bh| state_hash(material.hash_version, &bh, &material.membership) == material.state_hash);
        if !rules_ok || !hash_ok {
            return self.next_candidate();
        }
        // Step 4: a candidate passes. A log left by an earlier candidate goes first.
        if self.l > material.seq {
            let all = self.l + 1;
            self.persist(vec![PersistOp::TruncateLogBelow(all, None)]);
        }
        if self.install_snapshot(&material).is_err() {
            return self.next_candidate();
        }
        self.boot.cur = Some(Candidate {
            meta,
            epoch,
            inflated,
            hash: material.state_hash,
            hash_version: material.hash_version,
            announced: None,
        });
        self.registry.snapshots.retain(|r| r.info.seq >= material.seq);
        self.sess.state = SessionState::Reconciling;
        self.sess.rejoin_since = None;
        self.status_dirty = true;
    }

    /// Step 5: a `snap` entry was accepted while a rejoin's candidate waits for its announcement.
    pub(crate) fn rejoin_snap_seen(&mut self, author: Dev, info: &SnapInfo) {
        let Some(c) = self.boot.cur.as_mut() else { return };
        if info.blob != c.meta.id {
            return;
        }
        let same = author == c.meta.author
            && info.seq == c.meta.seq
            && info.hash == c.hash
            && info.hash_version == c.hash_version
            && info.sha256 == c.meta.sha256
            && info.size == c.meta.size
            && info.parts == c.meta.parts
            && info.inflated == c.inflated
            && info.epoch == c.epoch;
        if same {
            c.announced = Some(self.b + 1);
            return;
        }
        let (writer, at, own) = (c.meta.author, c.meta.seq, c.hash);
        let mut groups = vec![(own, vec![self.own]), (info.hash, vec![author])];
        groups.sort();
        self.alarm(AlarmKind::SnapshotMismatch, Some(at), Some(writer), None, Evidence::Mismatch { groups });
        self.boot.cur = None;
        self.sess.state = SessionState::Rejoining;
        self.next_candidate();
    }

    /// Steps 5 and 7: the log reached the relay's head (ENG-BOOT-002, ENG-UI-003).
    pub(crate) fn rejoin_at_head(&mut self) {
        let Some(c) = self.boot.cur.as_ref() else { return };
        if !self.pipe.batches.is_empty() || self.pipe.flight.is_some() {
            return;
        }
        let usable = c.announced.is_some_and(|seq| {
            self.registry.snapshots.iter().any(|r| r.entry_seq == seq && super::duty::usable(r, &self.own))
        });
        let s = c.meta.seq;
        self.boot.cur = None;
        if usable {
            self.boot.candidates.clear();
            self.boot.confirmed = false;
            self.meta.continuity_unchecked = Some(s);
            self.touch_meta();
        } else {
            self.sess.state = SessionState::Rejoining;
            self.next_candidate();
        }
    }

    // ------------------------------------------------------------------ rebootstrap (§19.9)

    /// Looks for a usable snapshot at an aligned seq above the entry the board could not apply.
    pub(crate) fn reboot_step(&mut self) {
        let Some(after) = self.meta.stops.iter().find_map(|s| match s.kind {
            StopKind::Rebootstrap { after } => Some(after),
            _ => None,
        }) else {
            return;
        };
        if self.boot.busy || self.boot.reboot.is_some() || self.sess.state == SessionState::Offline {
            return;
        }
        // The classified entries wait in the queue: the `snap` entries and the verdicts on them.
        let mut found: Option<(u64, Dev, SnapInfo)> = None;
        for e in self.pipe.queue.iter().filter(|e| e.class == Class::Accept) {
            let Some(o) = e.opened.as_ref() else { continue };
            let Some(info) = o.snap.as_ref().filter(|_| o.kind == Kind::Snap) else { continue };
            if info.seq <= after || info.seq > self.k || self.boot.bad.contains(&info.blob) {
                continue;
            }
            let mut ack = false;
            let mut mismatch = false;
            for b in self.pipe.queue.iter().filter(|b| b.class == Class::Accept && b.author != e.author) {
                if let Some(beat) = b.opened.as_ref().and_then(|o| o.beat.as_ref()) {
                    for (seq, v) in &beat.acks {
                        if *seq == e.seq {
                            match v {
                                Verdict::Ack => ack = true,
                                Verdict::Mismatch => mismatch = true,
                            }
                        }
                    }
                }
            }
            if ack && !mismatch {
                found = Some((e.seq, e.author, info.clone()));
            }
        }
        let Some((entry_seq, author, info)) = found else { return };
        let (id, parts) = (info.blob, info.parts);
        self.boot.reboot = Some((entry_seq, author, info));
        self.boot.busy = true;
        let req = self.want(Want::RebootFetch);
        self.fx(Effect::Blob { req, op: BlobRequest::Fetch { id, parts } });
    }

    pub(crate) fn reboot_blob(&mut self, result: Result<BlobOk, RelayRefusal>) {
        self.boot.busy = false;
        let Some((_, author, info)) = self.boot.reboot.take() else { return };
        let pass = |me: &mut Self, code: &str| {
            me.boot.bad.push(info.blob);
            me.alarm(AlarmKind::BlobFailure, None, Some(author), Some(code), Evidence::Blob { blob: info.blob });
        };
        let parts = match result {
            Ok(BlobOk::Parts(p)) => p,
            _ => return, // looked at again on a later call
        };
        let refs: Vec<&[u8]> = parts.iter().map(Vec::as_slice).collect();
        let sealer = &self.sealer;
        let material = match guard(|| sealer.open_snapshot(&refs, &info, &author, true)) {
            Some(Ok((m, _, _))) => m,
            Some(Err(e)) => return pass(self, e.code),
            None => return pass(self, "engine/local_fault"),
        };
        let s = material.seq;
        // The membership bytes must equal this device's own M_S.
        let own_m = self.pipe.queue.iter().find(|e| e.seq == s).map(|e| self.sealer.membership_bytes(&e.m));
        if own_m.as_deref() != Some(material.membership.as_slice()) {
            return pass(self, "engine/adopt_membership");
        }
        let sm = &self.sm;
        let state = match guard(|| sm.load(&material.board)) {
            Some(Ok(st)) => st,
            Some(Err(e)) => return pass(self, e.code),
            None => return pass(self, "engine/local_fault"),
        };
        let rules = self.pipe.queue.iter().find(|e| e.seq == s).map(|e| self.sealer.view(&e.m).rules);
        if guard(|| sm.rules(&state)) != rules {
            return pass(self, "engine/adopt_membership");
        }
        // B = S: the entries up to S are not applied; their records and the registry follow.
        self.state = Some(state);
        self.board_jump_to(s);
        self.end_stops(|x| matches!(x.kind, StopKind::Rebootstrap { .. }));
        self.pipe.pre = self.state.clone();
        self.pipe.redo.clear();
        self.write_checkpoint();
        self.rebase_due = true;
    }

    pub(crate) fn reboot_lost(&mut self) {
        self.boot.busy = false;
        self.boot.reboot = None;
    }
}
