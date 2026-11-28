//! `Start` (§7.10): rebuilding the replica from its records, the bootstrap at
//! creation and at pairing (§19.8), what a `Start` clears (§16.1, §21.4), and
//! handing the tail to the board under the marker rules (§15.2).
//!
//! Rules: a `Start` forgets everything the process held and reads only
//! `Loaded`. When the records cannot be loaded the engine stays inert
//! (`on = false`) with a local fault in its status. The tail above the
//! checkpoint goes through the same dry run as a page (recv.rs, `from_log`),
//! so `K` runs ahead of `B` until the board has applied it.

use super::board::Pipe;
use super::outbox::Rec;
use super::recv::Stash;
use super::{After, Engine, NoticeRow, guard};
use crate::{
    Body, Bootstrap, Checkpoint, Class, ClassifyInput, CtlItem, HaltKind, Hash32, Kind, LastEntry, Loaded,
    LogRecord, Meta, NoticeOp, OutboxOp, PersistOp, RelayTarget, Sealer, SnapshotMaterial, StateMachine, StopKind,
    state_hash,
};

impl<SM: StateMachine, S: Sealer> Engine<SM, S> {
    /// Forgets everything a process holds (the next `Start` reads it all again).
    fn reset_process(&mut self) {
        self.on = false;
        self.dead = false;
        self.next_id = 1;
        self.notes.clear();
        self.durable_notes.clear();
        self.pq.clear();
        self.durable = 0;
        self.last_pid = 0;
        self.meta = Meta::default();
        self.meta_dirty = false;
        self.meta_pid = 0;
        self.registry = Default::default();
        self.outbox.clear();
        self.outbox_ops = 0;
        self.notices.clear();
        self.notice_ops = 0;
        self.notices_through = 0;
        self.hashes.clear();
        self.bases.clear();
        self.floor = 1;
        self.retained = 1;
        self.l = 0;
        self.chain_first = 1;
        self.chains.clear();
        self.k = 0;
        self.mk = None;
        self.vk = Default::default();
        self.b = 0;
        self.cb = [0; 32];
        self.state = None;
        self.mb = None;
        self.no_b = None;
        self.view = None;
        self.view_version = 0;
        self.rebase_due = false;
        self.sess = Default::default();
        self.pipe = Pipe::default();
        self.jobs = Default::default();
        self.duty = Default::default();
        self.evid = Default::default();
        self.boot = Default::default();
        self.opx = Default::default();
        self.send = Default::default();
        self.epochs = Default::default();
        self.wants.clear();
        self.prev_session = None;
        self.protocol_hits.clear();
        self.tick_at = None;
        self.status_dirty = true;
        self.banners_dirty = false;
        self.view_notified = 0;
        self.exiting = false;
    }

    /// The records could not be loaded: the engine stays inert and shows why (§21.4).
    fn fail_start(&mut self, code: &'static str) {
        self.on = false;
        self.meta.stops.push(crate::StopRecord {
            kind: StopKind::LocalFault { code: code.into() },
            since_ms: self.wall,
            alarm: None,
        });
        self.status_dirty = true;
    }

    pub(crate) fn start(&mut self, loaded: Loaded) {
        self.reset_process();
        let Loaded {
            checkpoint,
            bases,
            chains,
            log_floor,
            floor_chain,
            tail,
            hashes,
            outbox,
            notices,
            meta,
            marker,
            build_id,
            session,
            bootstrap,
        } = loaded;
        self.session_id = session;
        let fresh = meta.is_none();
        let mut meta = meta.unwrap_or_default();
        // §16.1: what a Start clears.
        if let Some(h) = &meta.halt {
            let clear = match h.kind {
                HaltKind::Crash { .. } => meta.build_id != build_id,
                HaltKind::Rules { version } => self.sm.supported_rules().contains(&version),
                HaltKind::Commitment => false,
            };
            if clear {
                meta.halt = None;
                meta.last_failed = None;
            }
        }
        // §21.4: local faults are looked at again; §11.9: `Closed` ends at the next Start.
        meta.stops.retain(|s| !matches!(s.kind, StopKind::LocalFault { .. } | StopKind::Closed | StopKind::Rejoin));
        meta.attempt += 1;
        meta.build_id = build_id;
        // §11.6: the previous process's session id, and this one's for the next process.
        self.prev_session = meta.prev_session;
        meta.prev_session = Some(session);
        if fresh {
            meta.last_beat_ms = 0; // ENG-BEAT-003
        }
        self.meta = meta;
        self.meta_dirty = true;

        // ENG-STATE-003: the outbox, the newest Put of each entry_id winning.
        self.outbox_ops = outbox.len();
        for op in outbox {
            match op {
                OutboxOp::Put(mut r) => {
                    r.hold = false; // ENG-HOLD-004: holds end at every Start
                    let cmd = match &r.body {
                        Body::Cmd(p) => {
                            let sm = &self.sm;
                            guard(|| sm.parse(p)).and_then(Result::ok)
                        }
                        _ => None,
                    };
                    match self.rec_index(&r.entry_id) {
                        Some(i) => self.outbox[i] = Rec::new(r, cmd),
                        None => self.outbox.push(Rec::new(r, cmd)),
                    }
                }
                OutboxOp::Drop(id) => self.outbox.retain(|x| x.r.entry_id != id),
            }
        }
        self.outbox.sort_by_key(|r| r.r.order);
        // §28.4: a pairing does not go on after a `Start`; its record still ends with a report.
        self.opx.orphans = self
            .outbox
            .iter()
            .filter(|x| matches!(x.r.via, crate::Via::Register { .. }) && x.r.ack.is_none())
            .map(|x| x.r.entry_id)
            .collect();

        // The notice and refusal records with their dismissed flags (§7.7).
        self.notice_ops = notices.len();
        for op in notices {
            match op {
                NoticeOp::Add { kind, seq, record } => {
                    self.notices.push(NoticeRow { kind, seq, record, dismissed: false })
                }
                NoticeOp::Dismiss { seq, index } => {
                    if let Some(n) = self.notices.iter_mut().filter(|n| n.seq == seq).nth(index as usize) {
                        n.dismissed = true;
                    }
                }
            }
        }
        self.notices_through = self.notices.iter().map(|n| n.seq).max().unwrap_or(0);
        for h in hashes {
            self.hashes.insert(h.seq, h);
        }
        self.bases = bases;
        self.bases.sort();

        let ok = match (checkpoint, bootstrap) {
            (Some(c), _) => self.load_checkpoint(c, chains, log_floor, floor_chain, tail, marker),
            (None, Some(Bootstrap::Genesis { board, frame_1, relay })) => self.genesis(board, frame_1, relay),
            (None, Some(Bootstrap::Join { material, added_seq: _, relay })) => self.join(material, relay),
            (None, None) => Err("engine/local_fault"),
        };
        if let Err(code) = ok {
            self.fail_start(code);
            return;
        }
        self.on = true;
        self.pipe.ckpt_mono = self.mono;
        self.view_version = 0;
        self.rebase();
        // The tail (if any) is classified now and applied by the steps that follow.
        self.run_stash();
        if self.pipe.stash.is_none() && self.pipe.flight.is_none() && self.pipe.batches.is_empty() {
            self.pipe.tail = false;
        }
    }

    /// ENG-STATE-002, ENG-STATE-005: the confirmed state, `M_B`, `C_B` and the log's index.
    fn load_checkpoint(
        &mut self,
        c: Checkpoint,
        chains: Vec<(u64, Hash32, i64)>,
        loaded_floor: u64,
        floor_chain: Option<Hash32>,
        tail: Vec<LogRecord>,
        marker: Option<crate::Marker>,
    ) -> Result<(), &'static str> {
        let sm = &self.sm;
        let state = match guard(|| sm.load(&c.board)) {
            Some(Ok(s)) => s,
            _ => return Err("engine/local_fault"),
        };
        let m = self.sealer.membership_from_bytes(&c.membership).map_err(|_| "engine/local_fault")?;
        self.b = c.board_seq;
        self.cb = c.board_chain;
        self.registry = c.registry;
        self.floor = c.log_floor.max(loaded_floor).max(1);
        // `C_{floor-1}`, when the device holds it (§7.10).
        let before = self.floor - 1;
        let floor_c = if loaded_floor == self.floor && floor_chain.is_some() {
            floor_chain
        } else if self.b == before {
            Some(self.cb)
        } else {
            chains.iter().find(|x| x.0 == before).map(|x| x.1)
        };
        let mut next = self.floor;
        let first_tau = chains.iter().find(|x| x.0 >= self.floor).map_or(0, |x| x.2);
        if let Some(fc) = floor_c {
            self.retained = before;
            self.chain_first = before;
            self.chains.push_back((fc, first_tau));
        } else {
            self.retained = self.floor;
            self.chain_first = self.floor;
        }
        for (seq, chain, tau) in chains {
            if seq < self.floor {
                continue;
            }
            if seq != next {
                return Err("engine/local_fault"); // the log is not contiguous
            }
            self.chains.push_back((chain, tau));
            next += 1;
        }
        self.l = next - 1;
        if self.l < self.b {
            if self.b == before || self.chains.is_empty() {
                // A bootstrap from a snapshot: the log starts after the board.
                self.l = self.b;
                if self.chains.is_empty() {
                    self.chain_first = self.b;
                    self.retained = self.b;
                    self.chains.push_back((self.cb, 0));
                }
            } else {
                return Err("engine/local_fault");
            }
        }
        self.k = self.b;
        self.vk = self.sealer.view(&m);
        self.no_b = self.vk.devices.iter().find(|d| d.dev == self.own).map(|d| d.device_no);
        self.mk = Some(m.clone());
        self.mb = Some(m);
        self.state = Some(state);
        self.pipe.ckpt_b = self.b;
        // The tail above B, in seq order, with the chain values the log recorded.
        let mut entries = Vec::new();
        let mut expect = self.b + 1;
        for r in tail {
            if r.seq <= self.b || r.seq < self.floor {
                continue;
            }
            if r.seq != expect || self.chain_at(r.seq) != Some(r.chain) {
                return Err("engine/local_fault");
            }
            expect += 1;
            entries.push((r.seq, r.frame));
        }
        if expect - 1 != self.l {
            return Err("engine/local_fault"); // a log record above the checkpoint is missing from the tail
        }
        if !entries.is_empty() {
            self.pipe.tail = true;
            self.pipe.start_marker = marker;
            self.pipe.stash = Some(Stash { entries, head: None, from_log: true, keys_changed: false, quiet: false });
        }
        Ok(())
    }

    /// §19.8 Genesis (ENG-BOOT-004).
    fn genesis(&mut self, board: Vec<u8>, frame_1: Vec<u8>, relay: RelayTarget) -> Result<(), &'static str> {
        const BAD: &str = "engine/bad_genesis";
        let sm = &self.sm;
        let state = match guard(|| sm.load(&board)) {
            Some(Ok(s)) => s,
            _ => return Err(BAD),
        };
        let m0 = self.sealer.membership_zero();
        let c0 = self.sealer.chain_zero();
        let c1 = self.sealer.chain_step(&c0, 1, &frame_1);
        let receipt = {
            let chain_at = |b: u64| if b == 0 { Some(c0) } else { None };
            let none = |_: u64| -> Option<Hash32> { None };
            let frames = |_: u64| -> Option<&[u8]> { None };
            let input = ClassifyInput {
                seq: 1,
                frame: &frame_1,
                prev_chain: c0,
                membership: &m0,
                chain_at: &chain_at,
                retained_from: 0,
                own_sealed: &none,
                skip_targets: &frames,
            };
            let sealer = &mut self.sealer;
            guard(|| sealer.classify(&input))
        };
        let r = match receipt {
            Some(Ok(r)) if r.class == Class::Accept && r.halt.is_none() => r,
            _ => return Err(BAD),
        };
        let o = r.opened.as_ref().ok_or(BAD)?;
        let item = o.items.iter().find_map(|i| match i {
            CtlItem::Genesis { hash_version, state_hash } => Some((*hash_version, *state_hash)),
            _ => None,
        });
        let (hv, expected) = item.ok_or(BAD)?;
        let view = self.sealer.view(&r.membership);
        let sm = &self.sm;
        let rules = guard(|| sm.rules(&state)).ok_or(BAD)?;
        let bh = guard(|| sm.board_hash(&state)).ok_or(BAD)?;
        let mbytes = self.sealer.membership_bytes(&r.membership);
        if o.kind != Kind::Ctl || rules != view.rules || hv != 1 || state_hash(hv, &bh, &mbytes) != expected {
            return Err(BAD);
        }
        let tau = view.clock_ms;
        self.meta.relay = relay;
        self.state = Some(state);
        self.b = 1;
        self.k = 1;
        self.l = 1;
        self.cb = c1;
        self.floor = 1;
        self.retained = 0;
        self.chain_first = 0;
        self.chains.push_back((c0, 0));
        self.chains.push_back((c1, tau));
        self.no_b = view.devices.iter().find(|d| d.dev == self.own).map(|d| d.device_no);
        self.registry.last_entry.push(LastEntry { dev: r.author, at_ms: o.at_ms, seq: 1, based_on_seq: 0 });
        self.vk = view;
        self.mk = Some(r.membership.clone());
        self.mb = Some(r.membership);
        let c = self.checkpoint().ok_or(BAD)?;
        // The first `Meta` (the relay target) travels with the first checkpoint: a crash must
        // not leave a replica that has a checkpoint and no relay to go to.
        self.meta_dirty = false;
        let mut ops = vec![
            PersistOp::WriteMeta(self.meta.clone()),
            PersistOp::AppendLog(vec![LogRecord { seq: 1, chain: c1, tau, frame: frame_1 }]),
            PersistOp::WriteCheckpoint(c.clone()),
            PersistOp::WriteBase(c),
        ];
        if r.keys_changed {
            ops.push(PersistOp::WriteIdentity(self.sealer.export_identity()));
        }
        let pid = self.persist(ops);
        self.meta_pid = pid;
        self.bases = vec![(1, c1)];
        self.pipe.ckpt_b = 1;
        self.pipe.ckpt_pid = pid;
        Ok(())
    }

    /// §19.8 Join, and §19.7 step 4: the replica starts from a snapshot at `S`
    /// (ENG-BOOT-006, ENG-HASH-003).
    fn join(&mut self, material: SnapshotMaterial, relay: RelayTarget) -> Result<(), &'static str> {
        // Set before the snapshot is installed: its write carries the first `Meta`.
        self.meta.relay = relay;
        self.meta.last_beat_ms = 0;
        self.install_snapshot(&material)?;
        Ok(())
    }

    /// Installs a snapshot's state as the confirmed state at `S`, with
    /// `B = K = L = S`, `log_floor = S + 1`, `retained_from = S`; writes the
    /// checkpoint and the base at `S` in one `Persist`, and the truncation after it.
    pub(crate) fn install_snapshot(&mut self, material: &SnapshotMaterial) -> Result<(), &'static str> {
        const BAD: &str = "engine/local_fault";
        let sm = &self.sm;
        let state = match guard(|| sm.load(&material.board)) {
            Some(Ok(s)) => s,
            _ => return Err(BAD),
        };
        let m = self.sealer.membership_from_bytes(&material.membership).map_err(|_| BAD)?;
        let view = self.sealer.view(&m);
        let rules = guard(|| sm.rules(&state)).ok_or(BAD)?;
        if rules != view.rules {
            return Err(BAD);
        }
        let s = material.seq;
        self.state = Some(state);
        self.b = s;
        self.k = s;
        self.l = s;
        self.cb = material.chain;
        self.floor = s + 1;
        self.retained = s;
        self.chain_first = s;
        self.chains.clear();
        self.chains.push_back((material.chain, view.clock_ms));
        self.no_b = view.devices.iter().find(|d| d.dev == self.own).map(|d| d.device_no);
        self.vk = view;
        self.mk = Some(m.clone());
        self.mb = Some(m);
        self.pipe.queue.clear();
        self.pipe.skip_at.clear();
        self.pipe.batches.clear();
        self.pipe.skipped.clear();
        self.pipe.pre = None;
        self.pipe.redo.clear();
        self.hashes.retain(|x, _| *x > s);
        let c = self.checkpoint().ok_or(BAD)?;
        // `Meta` in the same write (for a join it is the first one, with the relay target).
        self.meta_dirty = false;
        let pid = self.persist(vec![
            PersistOp::WriteMeta(self.meta.clone()),
            PersistOp::WriteCheckpoint(c.clone()),
            PersistOp::WriteBase(c),
        ]);
        self.meta_pid = pid;
        if !self.bases.iter().any(|b| b.0 == s) {
            self.bases.push((s, material.chain));
            self.bases.sort();
        }
        self.pipe.ckpt_b = s;
        self.pipe.ckpt_pid = pid;
        self.pipe.since_ckpt = 0;
        self.after(pid, After::Truncate { below: s + 1, floor_chain: Some(material.chain) });
        self.rebase_due = true;
        Ok(())
    }
}
