//! The model runtime (engine.md §31.4): processes, the persist queue, local
//! reads, ticks, sessions, the network and the model relays' answers.

use super::*;

/// What a delivered event tells the invariants before the engine handles it.
struct Contra {
    what: String,
    l: u64,
}

impl World<'_> {
    pub(super) fn step(&mut self, ev: Ev) {
        if let Ev::Tick { id, proc, .. } = &ev {
            let (id, proc) = (*id, *proc);
            if let Some(p) = self.idents[id].proc.as_mut().filter(|p| p.no == proc) {
                p.last_tick = self.t;
            }
        }
        match ev {
            Ev::ToRelay { id, proc, conn, sel, call } => self.at_relay(id, proc, conn, sel, call),
            Ev::Down { id, proc, conn, msg } => self.down(id, proc, conn, msg),
            Ev::PersistDone { id, proc, crash } => self.persist_done(id, proc, crash),
            Ev::Local { id, proc, ev } => {
                if self.proc_is(id, proc) {
                    let s = &self.idents[id].store;
                    let (req, result) = match ev {
                        LocalAsk::Log { req, from, to } => {
                            let recs: Vec<_> = (from..=to).filter_map(|q| s.log.get(&q).cloned()).collect();
                            if from <= to && recs.len() as u64 == to - from + 1 {
                                (req, LocalReadResult::Records(recs))
                            } else {
                                (req, LocalReadResult::Missing { code: "device/log_missing" })
                            }
                        }
                        LocalAsk::Base { req, seq } => match s.bases.get(&seq) {
                            Some(c) => (req, LocalReadResult::Base(c.clone())),
                            None => (req, LocalReadResult::Missing { code: "device/base_missing" }),
                        },
                    };
                    self.deliver(id, Event::LocalRead { req, result });
                }
            }
            Ev::Tick { id, proc, tgen } => {
                if self.idents[id].proc.as_ref().is_some_and(|p| p.no == proc && p.tick_gen == tgen) {
                    self.deliver(id, Event::Tick);
                }
            }
            Ev::Connect { id, proc, tgen } => self.connect_now(id, proc, tgen),
            Ev::Timeout { id, proc, conn, key } => self.timeout(id, proc, conn, key),
            Ev::Start { id } => self.start_proc(id),
            Ev::Exit { id, proc } => {
                if self.proc_is(id, proc) {
                    self.stop_proc(id);
                    // Woken again before the old process exited: a new one starts.
                    if self.machines[self.idents[id].machine].awake && self.machines[self.idents[id].machine].ident == id {
                        self.idents[id].skip_backoff = true;
                        self.start_proc(id);
                    }
                }
            }
            Ev::User { id, proc, ev } => {
                if self.proc_is(id, proc) {
                    self.deliver(id, ev);
                }
            }
            other => self.step_load(other),
        }
    }

    pub(super) fn proc_is(&self, id: usize, proc: u32) -> bool {
        self.idents[id].proc.as_ref().is_some_and(|p| p.no == proc)
    }

    pub(super) fn proc_no(&self, id: usize) -> Option<u32> {
        self.idents[id].proc.as_ref().map(|p| p.no)
    }

    // ---------- processes

    pub(super) fn start_proc(&mut self, id: usize) {
        let m = self.idents[id].machine;
        if self.idents[id].retired || self.idents[id].proc.is_some() || !self.machines[m].awake {
            return;
        }
        let rules = self.rules_of(id);
        let fake = match self.idents[id].first.take() {
            Some(f) => f,
            None => {
                let reseed = self.idents[id].rng.next();
                let st = &self.idents[id].store;
                match FakeSealer::restore(&st.identity, st.history.clone(), reseed, &rules) {
                    Ok(f) => f,
                    Err(e) => {
                        let line = self.trace.line(self.t, "check", &[("inv", V::I(0)), ("ok", V::I(0)), ("detail", V::S(e.code))]);
                        self.violations.push(Violation { invariant: 0, line, detail: format!("dev {id}: restore failed: {}", e.code) });
                        return;
                    }
                }
            }
        };
        let mut session = [0u8; 16];
        let a = self.idents[id].rng.next().to_be_bytes();
        let b = self.idents[id].rng.next().to_be_bytes();
        session[..8].copy_from_slice(&a);
        session[8..].copy_from_slice(&b);
        let engine = Engine::new(ToyMachine::new(rules.len() > 1), fake, Config {});
        let ident = &mut self.idents[id];
        ident.unloaded = false;
        // The outbox as the new process will load it: the durable one.
        ident.live = ident.store.live_outbox().into_iter().map(|r| (r.entry_id, r)).collect();
        if let Some((roots, pending)) = identity_keys(&ident.store.identity) {
            ident.last_roots = roots;
            ident.last_pending = pending;
        }
        ident.nproc += 1;
        let no = ident.nproc;
        ident.proc = Some(Proc {
            no,
            engine,
            start_t: self.t,
            session,
            queue: VecDeque::new(),
            writing: false,
            tick_gen: 0,
            connect_gen: 0,
            conn: None,
            conn_no: 0,
            connecting: false,
            target: None,
            outstanding: BTreeMap::new(),
            exiting: false,
            hello_no: 0,
            last_tick: 0,
        });
        self.stats.starts += 1;
        self.trace.line(self.t, "start", &[("dev", V::I(id as i128)), ("attempt", V::I(i128::from(no)))]);
        let build = format!("sim-{}", self.idents[id].build);
        let loaded = self.idents[id].store.loaded(&build, session);
        self.deliver(id, Event::Start(loaded));
    }

    pub(super) fn rules_of(&self, id: usize) -> Vec<u32> {
        let i = &self.idents[id];
        if i.rules2 || i.build > 1 { vec![1, 2] } else { vec![1] }
    }

    /// The process ends: a crash, a kill after `Shutdown`, a retirement.
    pub(super) fn stop_proc(&mut self, id: usize) {
        let Some(p) = self.idents[id].proc.take() else { return };
        for (e, r) in p.engine.sealer().roots() {
            self.roots.insert(e, r);
        }
        if let Some(c) = &p.conn
            && let Some(s) = c.socket {
                self.relay(c.sel).socket_close(s);
            }
        self.idents[id].last_status = Some(p.engine.status());
    }

    fn crash(&mut self, id: usize, at: CrashAt) {
        let name = match at {
            CrashAt::Before => "before",
            CrashAt::During => "during",
            CrashAt::After => "after",
        };
        self.stats.crashes += 1;
        *self.stats.faults.entry("crash").or_default() += 1;
        self.trace.line(self.t, "crash", &[("dev", V::I(id as i128)), ("at", V::S(name))]);
        self.stop_proc(id);
        let delay = 1_000 + self.fault.below(30_000);
        self.at(self.t + delay, Ev::Start { id });
    }

    // ---------- delivering an event

    pub(super) fn deliver(&mut self, id: usize, ev: Event<ToyCommand>) {
        let m = self.idents[id].machine;
        let wall = self.wall(m);
        let contra = self.contradiction(id, &ev);
        let Some(p) = self.idents[id].proc.as_mut() else { return };
        let mono = self.t - p.start_t + 1;
        let name = ev_name(&ev);
        *self.stats.events.entry(name).or_default() += 1;
        let (seq, n): (Option<u64>, Option<u64>) = match &ev {
            Event::Page { page, .. } => (Some(page.head.seq), Some(page.entries.len() as u64)),
            Event::Welcome(w) => (Some(w.head.seq), None),
            Event::AppendResults { results, .. } => (None, Some(results.len() as u64)),
            Event::LocalRead { result: LocalReadResult::Records(r), .. } => (None, Some(r.len() as u64)),
            Event::Start(l) => (Some(l.checkpoint.as_ref().map_or(0, |c| c.board_seq)), Some(l.tail.len() as u64)),
            Event::Persisted { id } => (None, Some(*id)),
            _ => (None, None),
        };
        let mut f: Vec<(&str, V<'_>)> = vec![("dev", V::I(id as i128)), ("ev", V::S(name))];
        if let Some(s) = seq {
            f.push(("seq", V::I(i128::from(s))));
        }
        if let Some(x) = n {
            f.push(("n", V::I(i128::from(x))));
        }
        let line = self.trace.line(self.t, "in", &f);
        let is_rejoin = matches!(ev, Event::User(UserAction::ConfirmRejoin));
        let effects = p.engine.handle(Input { wall_ms: wall, mono_ms: mono, event: ev });
        if is_rejoin {
            self.idents[id].rejoin_confirmed = true;
        }
        if let Some(c) = contra {
            let ident = &mut self.idents[id];
            if ident.expect_alarm.is_none() && !ident.alarms.iter().any(|a| a.3 >= self.contra_since) {
                ident.expect_alarm = Some((line, c.what, c.l));
            }
        }
        for e in effects {
            self.effect(id, e);
            if self.idents[id].proc.is_none() {
                return;
            }
        }
        self.sample(id);
    }

    /// Records the confirmed state when `B` moved (invariant 4).
    fn sample(&mut self, id: usize) {
        let Some(p) = self.idents[id].proc.as_ref() else { return };
        // `confirmed` panics while no replica is loaded (a refused `Start`).
        let e = &p.engine;
        let Ok((bh, head)) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let (st, head) = e.confirmed();
            (ToyMachine::new(true).board_hash(st), head)
        })) else {
            if !self.idents[id].unloaded {
                self.idents[id].unloaded = true;
                // The process cannot go on without a replica: it ends here.
                self.idents[id].proc = None;
                let line = self.trace.line(self.t, "check", &[("inv", V::I(6)), ("ok", V::I(0)), ("detail", V::S("no replica loaded after Start"))]);
                self.violations.push(Violation { invariant: 6, line, detail: format!("dev {id}: the engine loaded no replica at Start") });
            }
            return;
        };
        if (head.seq, head.chain) == self.idents[id].last_b {
            return;
        }
        let line = self.trace.lines();
        let ident = &mut self.idents[id];
        ident.last_b = (head.seq, head.chain);
        ident.samples.push((head.seq, head.chain, bh, line));
        if let Some((l, what, at_l)) = &ident.expect_alarm
            && head.seq > *at_l {
                let detail = format!("dev {id}: applied seq {} after {what} without an alarm", head.seq);
                self.violations.push(Violation { invariant: 4, line: *l, detail });
                ident.expect_alarm = None;
            }
    }

    /// Whether a welcome or page contradicts what the device holds (invariant 4).
    fn contradiction(&self, id: usize, ev: &Event<ToyCommand>) -> Option<Contra> {
        let s = &self.idents[id].store;
        let (l, _) = s.head()?;
        let (head, entries): (Head, &[crate::PageEntry]) = match ev {
            Event::Welcome(w) => (w.head, &[]),
            Event::Page { page, .. } if !page.truncated => (page.head, &page.entries),
            _ => return None,
        };
        if head.seq < l {
            return Some(Contra { what: format!("a relay head {} below its log head {l}", head.seq), l });
        }
        if let Some(c) = s.chain_at(head.seq)
            && c != head.chain {
                return Some(Contra { what: format!("a relay chain at {} that differs from its log", head.seq), l });
            }
        for e in entries {
            if let Some(r) = s.log.get(&e.seq)
                && r.frame != e.frame {
                    return Some(Contra { what: format!("a page entry at {} that differs from its log", e.seq), l });
                }
        }
        None
    }

    // ---------- effects

    fn effect(&mut self, id: usize, e: Effect) {
        let name = fx_name(&e);
        *self.stats.effects.entry(name).or_default() += 1;
        let (fid, n, frames): (Option<u64>, Option<u64>, Vec<Hash32>) = match &e {
            Effect::Persist { id, ops } => (Some(*id), Some(ops.len() as u64), Vec::new()),
            Effect::ReadLog { req, from, to } => (Some(*req), Some(to.saturating_sub(*from) + 1), Vec::new()),
            Effect::ReadBase { req, .. } | Effect::Read { req, .. } => (Some(*req), None, Vec::new()),
            Effect::Append { req, frames } => (Some(*req), Some(frames.len() as u64), frames.iter().map(|f| h(f)).collect()),
            Effect::Admin { req, op } => {
                let fr: Vec<Hash32> = match op {
                    AdminRequest::Register { frame, .. } => vec![h(frame)],
                    AdminRequest::Fence { frame: Some(f), .. } => vec![h(f)],
                    AdminRequest::SeedTail { frames, .. } | AdminRequest::RestoreTail { frames, .. } => frames.iter().map(|f| h(f)).collect(),
                    _ => Vec::new(),
                };
                (Some(*req), Some(fr.len() as u64), fr)
            }
            Effect::Blob { req, .. } => (Some(*req), None, Vec::new()),
            Effect::ScheduleTick { at_mono_ms } => (Some(*at_mono_ms), None, Vec::new()),
            _ => (None, None, Vec::new()),
        };
        let note = if let Effect::Notify(nt) = &e { Some(note_name(nt)) } else { None };
        let mut f: Vec<(&str, V<'_>)> = vec![("dev", V::I(id as i128)), ("fx", V::S(note.unwrap_or(name)))];
        if let Some(x) = fid {
            f.push(("id", V::I(i128::from(x))));
        }
        if let Some(x) = n {
            f.push(("n", V::I(i128::from(x))));
        }
        if !frames.is_empty() {
            f.push(("frames", V::Hexes(&frames)));
        }
        let line = self.trace.line(self.t, "out", &f);
        if let Some(nn) = note {
            *self.stats.notifications.entry(nn).or_default() += 1;
        }
        match e {
            Effect::Persist { id: pid, ops } => {
                self.note_persist(id, &ops, line);
                let p = self.idents[id].proc.as_mut().expect("running");
                let hist = ops
                    .iter()
                    .any(|o| matches!(o, PersistOp::WriteIdentity(_) | PersistOp::WriteCheckpoint(_) | PersistOp::WriteBase(_)))
                    .then(|| p.engine.sealer().history());
                p.queue.push_back((pid, ops, hist));
                if !p.writing {
                    self.start_write(id);
                }
            }
            Effect::ReadLog { req, from, to } => {
                let proc = self.proc_no(id).unwrap_or(0);
                self.at(self.t + 1, Ev::Local { id, proc, ev: LocalAsk::Log { req, from, to } });
            }
            Effect::ReadBase { req, seq } => {
                let proc = self.proc_no(id).unwrap_or(0);
                self.at(self.t + 1, Ev::Local { id, proc, ev: LocalAsk::Base { req, seq } });
            }
            Effect::Connect { relay } => {
                let sel = self.sel_of_host(id, &relay.host);
                let ident = &mut self.idents[id];
                let k = ident.failures;
                let wait = if k == 0 || ident.skip_backoff {
                    0
                } else {
                    let hi = 60_000u64.min(1_000u64 << (k - 1).min(6));
                    1_000 + ident.rng.below(hi - 1_000 + 1)
                };
                ident.skip_backoff = false;
                let p = ident.proc.as_mut().expect("running");
                p.target = sel;
                p.connect_gen += 1;
                p.connecting = true;
                let (proc, tgen) = (p.no, p.connect_gen);
                self.at(self.t + wait, Ev::Connect { id, proc, tgen });
            }
            Effect::Disconnect => self.close_conn(id, false),
            Effect::Hello => {
                let p = self.idents[id].proc.as_mut().expect("running");
                p.hello_no += 1;
                let key = KEY_HELLO | p.hello_no;
                p.outstanding.insert(key, (self.t, "hello"));
                self.send_up(id, Call::Hello, key, 0);
            }
            Effect::Read { req, after, limit } => {
                self.outstanding(id, req, "read");
                self.send_up(id, Call::Read { req, after, limit }, req, 0);
            }
            Effect::Append { req, frames } => {
                self.outstanding(id, req, "append");
                let bytes: usize = frames.iter().map(Vec::len).sum();
                self.send_up(id, Call::Append { req, frames }, req, bytes);
            }
            Effect::Admin { req, op } => {
                self.outstanding(id, req, "admin");
                self.send_up(id, Call::Admin { req, op }, req, 0);
            }
            Effect::Blob { req, op } => {
                self.outstanding(id, req, "blob");
                let bytes = if let BlobRequest::Upload { blob, .. } = &op { blob.bytes.len() } else { 0 };
                self.send_up(id, Call::Blob { req, op }, req, bytes);
            }
            Effect::ScheduleTick { at_mono_ms } => {
                let p = self.idents[id].proc.as_mut().expect("running");
                p.tick_gen += 1;
                let mono = self.t - p.start_t + 1;
                if at_mono_ms <= mono + 1 {
                    self.stats.ticks_due_now += 1;
                }
                // A tick comes at or after the time asked (§4.3), never sooner than
                // TICK_MIN after the last one: the runtime's timer granularity.
                let at = (p.start_t + at_mono_ms).saturating_sub(1).max(self.t).max(p.last_tick + TICK_MIN);
                let (proc, tgen) = (p.no, p.tick_gen);
                self.at(at, Ev::Tick { id, proc, tgen });
            }
            Effect::Notify(nt) => self.on_notify(id, nt, line),
        }
    }

    fn outstanding(&mut self, id: usize, req: u64, what: &'static str) {
        if let Some(p) = self.idents[id].proc.as_mut() {
            p.outstanding.insert(req, (self.t, what));
        }
    }

    // ---------- persistence (ENG-PERSIST-001, ENG-PERSIST-004)

    fn start_write(&mut self, id: usize) {
        let crash_ppm = if self.faults_on { self.cfg.faults.crash_ppm.min(20_000) } else { 0 };
        let crash = (crash_ppm > 0 && self.fault.chance(crash_ppm)).then(|| match self.fault.below(3) {
            0 => CrashAt::Before,
            1 => CrashAt::During,
            _ => CrashAt::After,
        });
        let ident = &mut self.idents[id];
        let lat = 1 + ident.rng.below(20);
        let p = ident.proc.as_mut().expect("running");
        if p.queue.is_empty() {
            return;
        }
        p.writing = true;
        let proc = p.no;
        if let Some(CrashAt::Before) = crash {
            self.crash(id, CrashAt::Before);
            return;
        }
        self.at(self.t + lat, Ev::PersistDone { id, proc, crash });
    }

    fn persist_done(&mut self, id: usize, proc: u32, crash: Option<CrashAt>) {
        if !self.proc_is(id, proc) {
            return;
        }
        let ident = &mut self.idents[id];
        let p = ident.proc.as_mut().expect("running");
        let Some((pid, ops, hist)) = p.queue.pop_front() else { return };
        p.writing = false;
        match crash {
            Some(CrashAt::During) => {
                if self.fault.chance(500_000) {
                    self.idents[id].store.apply(&ops, hist.as_ref());
                }
                self.crash(id, CrashAt::During);
            }
            Some(_) => {
                ident.store.apply(&ops, hist.as_ref());
                self.crash(id, CrashAt::After);
            }
            None => {
                ident.store.apply(&ops, hist.as_ref());
                self.deliver(id, Event::Persisted { id: pid });
                if let Some(p) = self.idents[id].proc.as_ref()
                    && !p.writing && !p.queue.is_empty() {
                        self.start_write(id);
                    }
            }
        }
    }

    /// What a `Persist` tells the invariants (2, 3, 5, alarms).
    fn note_persist(&mut self, id: usize, ops: &[PersistOp], line: u64) {
        let b = self.idents[id].proc.as_ref().and_then(|p| board_head(&p.engine)).map_or(0, |h| h.seq);
        for op in ops {
            match op {
                PersistOp::Outbox(xs) => {
                    for x in xs {
                        match x {
                            OutboxOp::Put(r) => self.note_put(id, r),
                            OutboxOp::Drop(eid) => self.explain_drop(id, eid, b, line),
                        }
                    }
                }
                PersistOp::RewriteOutbox(recs) => {
                    let keep: BTreeSet<[u8; 16]> = recs.iter().map(|r| r.entry_id).collect();
                    let gone: Vec<[u8; 16]> = self.idents[id].live.keys().filter(|k| !keep.contains(*k)).copied().collect();
                    for g in gone {
                        self.explain_drop(id, &g, b, line);
                    }
                    self.idents[id].live.clear();
                    for r in recs {
                        self.note_put(id, r);
                    }
                }
                PersistOp::WriteMeta(meta) => {
                    for a in &meta.alarms {
                        if a.id > self.idents[id].max_alarm {
                            self.idents[id].max_alarm = a.id;
                            let k = alarm_name(a.kind);
                            *self.stats.alarms.entry(k).or_default() += 1;
                            let mut f: Vec<(&str, V<'_>)> = vec![("dev", V::I(id as i128)), ("kind", V::S(k))];
                            if let Some(s) = a.seq {
                                f.push(("seq", V::I(i128::from(s))));
                            }
                            let l = self.trace.line(self.t, "alarm", &f);
                            let t = self.t;
                            let ident = &mut self.idents[id];
                            ident.alarms.push((a.id, a.kind, a.seq, t));
                            ident.alarm_lines.push(l);
                            ident.expect_alarm = None;
                        }
                    }
                }
                PersistOp::TruncateLogBelow(p, _) => {
                    let ctx = self.deletion_ctx(id) || self.join_floor(id, *p);
                    self.deletions.push((id, Deletion::Truncate(*p, ctx), b, line));
                }
                PersistOp::DeleteBases(seqs) => {
                    let ctx = self.deletion_ctx(id);
                    self.deletions.push((id, Deletion::Bases(seqs.clone(), ctx), b, line));
                }
                PersistOp::WriteIdentity(bytes) => {
                    if let Some((roots, pending)) = identity_keys(bytes) {
                        let ident = &mut self.idents[id];
                        let lost: Vec<u32> = ident.last_roots.difference(&roots).copied().collect();
                        let gone: Vec<[u8; 16]> = ident.last_pending.difference(&pending).copied().collect();
                        ident.last_roots = roots;
                        ident.last_pending = pending;
                        if !lost.is_empty() {
                            self.deletions.push((id, Deletion::Roots(lost), b, line));
                        }
                        if !gone.is_empty() {
                            self.deletions.push((id, Deletion::Pending(gone), b, line));
                        }
                    }
                    if let Some(p) = self.idents[id].proc.as_ref() {
                        for (e, r) in p.engine.sealer().roots() {
                            self.roots.insert(e, r);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// A deletion during a confirmed rejoin or a join is explained (§19.7, §19.8).
    fn deletion_ctx(&self, id: usize) -> bool {
        let i = &self.idents[id];
        i.rejoin_confirmed || i.store.checkpoint.is_none()
    }

    /// A truncation to the floor of a join (§19.8: the log is empty, its floor `h + 1`).
    fn join_floor(&self, id: usize, p: u64) -> bool {
        let i = &self.idents[id];
        i.joined_at > 0 && p <= i.joined_at + 1
    }

    fn note_put(&mut self, id: usize, r: &OutboxRecord) {
        self.first_put.entry(r.entry_id).or_insert(self.t);
        if let Some(fr) = &r.frame {
            let duty = matches!(
                (&r.kind, &r.body),
                (Kind::Beat | Kind::Snap, _) | (Kind::Ctl, Body::Ctl(CtlIntent::RulesUpgrade { mode: crate::UpgradeMode::Normal, .. }))
            );
            self.frames.insert(h(fr), (r.entry_id, r.kind, duty));
        }
        self.idents[id].live.insert(r.entry_id, r.clone());
    }

    /// Invariant 5 for an outbox `Drop`: the entry reached the board head, a
    /// hold was cancelled, a confirmed rejoin or a join.
    fn explain_drop(&mut self, id: usize, eid: &[u8; 16], b: u64, line: u64) {
        let ctx = self.deletion_ctx(id);
        let Some(r) = self.idents[id].live.remove(eid) else { return };
        let st = &self.idents[id].store;
        let in_log = |fh: &Hash32| st.by_hash.get(fh).is_some_and(|s| *s <= b);
        let reached = r.seen.or(r.ack.map(|a| a.0)).is_some_and(|s| s <= b)
            || r.frame.as_ref().is_some_and(|f| in_log(&h(f)))
            || r.frame_hash.as_ref().is_some_and(in_log);
        let cancelled_hold = r.hold && r.frame.is_none();
        // §28.4 step 3: a pairing's DeviceAdded dropped unsent when the epoch raced it.
        let pair_raced = matches!(r.via, crate::Via::Register { .. }) && r.frame.is_none();
        if !(reached || cancelled_hold || pair_raced || ctx) {
            let detail = format!(
                "dev {id}: outbox Drop of {} (kind {:?}, seen {:?}, ack {:?}) with B = {b}",
                hex(eid),
                r.kind,
                r.seen,
                r.ack.map(|a| a.0)
            );
            self.violations.push(Violation { invariant: 5, line, detail });
        }
    }

    // ---------- sessions

    fn connect_now(&mut self, id: usize, proc: u32, tgen: u64) {
        let Some(p) = self.idents[id].proc.as_mut() else { return };
        if p.no != proc || p.connect_gen != tgen || !p.connecting {
            return;
        }
        let Some(sel) = p.target else {
            p.connecting = false;
            self.idents[id].failures += 1;
            self.deliver(id, Event::Disconnected { unreachable: true, paused_until_ms: None });
            return;
        };
        p.conn_no += 1;
        p.conn = Some(Conn { no: p.conn_no, sel, socket: None, welcomed_t: None, sock_t: 0 });
        let key = KEY_CONNECT | p.conn_no;
        self.send_up(id, Call::Login, key, 0);
    }

    /// Ends the current session: closes the socket at the relay; answers still
    /// in flight are discarded (`device.md` §10.5 `Disconnect`).
    pub(super) fn close_conn(&mut self, id: usize, failure: bool) {
        let t = self.t;
        let ident = &mut self.idents[id];
        let Some(p) = ident.proc.as_mut() else { return };
        p.connecting = false;
        p.outstanding.clear();
        let Some(c) = p.conn.take() else { return };
        p.conn_no += 1;
        if c.welcomed_t.is_some_and(|w| t >= w + 60_000) {
            ident.failures = 0;
        }
        if failure {
            ident.failures += 1;
        }
        if let Some(s) = c.socket {
            self.relay(c.sel).socket_close(s);
        }
    }

    fn timeout(&mut self, id: usize, proc: u32, conn: u64, key: u64) {
        let Some(p) = self.idents[id].proc.as_ref() else { return };
        if p.no != proc || p.conn_no != conn {
            return;
        }
        let live = if key & KEY_CONNECT != 0 && key & KEY_HELLO == 0 {
            p.connecting && p.conn.as_ref().is_some_and(|c| c.welcomed_t.is_none())
        } else {
            p.outstanding.contains_key(&key)
        };
        if !live {
            return;
        }
        self.close_conn(id, true);
        self.deliver(id, Event::Disconnected { unreachable: true, paused_until_ms: None });
    }

    // ---------- the network

    fn net_delay(&mut self) -> u64 {
        let d = if self.faults_on { u64::from(self.cfg.faults.delay_ms.min(5_000)) } else { 0 };
        LATENCY + if d > 0 { self.net.below(d + 1) } else { 0 }
    }

    fn lost(&mut self, id: usize) -> bool {
        let p = if self.faults_on { self.cfg.faults.loss_ppm.min(200_000) } else { 0 };
        if p > 0 && self.net.chance(p) {
            self.fault_line("loss", ("dev", id as i128), &[]);
            return true;
        }
        false
    }

    fn dup(&mut self, id: usize) -> bool {
        let p = if self.faults_on { self.cfg.faults.dup_ppm.min(50_000) } else { 0 };
        if p > 0 && self.net.chance(p) {
            self.fault_line("duplication", ("dev", id as i128), &[]);
            return true;
        }
        false
    }

    fn send_up(&mut self, id: usize, call: Call, key: u64, bytes: usize) {
        let Some(p) = self.idents[id].proc.as_ref() else { return };
        let (proc, conn) = (p.no, p.conn_no);
        let host_sel = match &call {
            Call::Admin { op: AdminRequest::Door { host } | AdminRequest::Seed { host, .. } | AdminRequest::SeedTail { host, .. }, .. } => {
                Some(self.sel_of_host(id, host))
            }
            _ => None,
        };
        let sel = match host_sel {
            Some(s) => s,
            None => p.conn.as_ref().map(|c| c.sel).or(p.target),
        };
        let timeout = TIMEOUT + (bytes as u64 * 1_000).div_ceil(65_536);
        self.at(self.t + timeout, Ev::Timeout { id, proc, conn, key });
        let Some(sel) = sel else { return };
        if self.lost(id) {
            return;
        }
        let d = self.net_delay();
        if self.dup(id) {
            let d2 = self.net_delay();
            self.at(self.t + d2, Ev::ToRelay { id, proc, conn, sel, call: call.clone() });
        }
        self.at(self.t + d, Ev::ToRelay { id, proc, conn, sel, call });
    }

    /// Sends a message down to a device; socket messages keep their order.
    fn send_down(&mut self, id: usize, proc: u32, conn: u64, msg: Down, socket: bool) {
        if self.lost(id) {
            return;
        }
        let copies = if self.dup(id) { 2 } else { 1 };
        for _ in 0..copies {
            let mut at = self.t + self.net_delay();
            if socket
                && let Some(c) = self.idents[id].proc.as_mut().and_then(|p| p.conn.as_mut()).filter(|c| c.no == conn) {
                    at = at.max(c.sock_t + 1);
                    c.sock_t = at;
                }
            self.at(at, Ev::Down { id, proc, conn, msg: msg.clone() });
        }
    }

    fn down(&mut self, id: usize, proc: u32, conn: u64, msg: Down) {
        let Some(p) = self.idents[id].proc.as_mut() else { return };
        if p.no != proc || p.conn_no != conn {
            return;
        }
        match msg {
            Down::Events(evs) => {
                if !p.connecting {
                    return;
                }
                p.connecting = false;
                let failed = evs.iter().any(|e| matches!(e, Event::Disconnected { .. }));
                if failed {
                    p.conn = None;
                    p.conn_no += 1;
                    self.idents[id].failures += 1;
                } else if let Some(c) = p.conn.as_mut() {
                    c.welcomed_t = Some(self.t);
                }
                for e in evs {
                    if !self.proc_is(id, proc) {
                        return;
                    }
                    self.deliver(id, e);
                }
            }
            Down::Answer(key, ev) => {
                let first = p.outstanding.remove(&key).is_some();
                if first
                    && let Some(acks) = self.pending_acks.remove(&(id, proc, key)) {
                        for (eid, ref_branch) in acks {
                            let line = self.trace.lines() + 1;
                            self.acked.push((eid, id, line, ref_branch));
                        }
                    }
                if let Event::Disconnected { unreachable, .. } = ev {
                    self.close_conn(id, unreachable);
                }
                self.deliver(id, ev);
            }
            Down::Sock(m) => {
                let ev = match m {
                    SockMsg::Push(page) => Event::Page { req: None, page },
                    SockMsg::Second(s) => Event::Second { session: s },
                    SockMsg::Displaced(s) => Event::Displaced { session: s },
                    SockMsg::State { .. } => Event::RelayRefused { req: None, refusal: refusal(403, "relay/fenced") },
                    SockMsg::Close(code) => {
                        if let Some(c) = p.conn.as_mut() {
                            c.socket = None;
                        }
                        if code == 4001 || code == 4002 {
                            return;
                        }
                        let unreachable = !(code == 4003 || code == 4004);
                        self.close_conn(id, unreachable);
                        Event::Disconnected { unreachable, paused_until_ms: None }
                    }
                    SockMsg::Welcome(_) | SockMsg::Result { .. } | SockMsg::Page { .. } | SockMsg::Error { .. } => return,
                };
                self.deliver(id, ev);
            }
        }
    }

    // ---------- the relays

    fn cred(&self, id: usize, sel: Sel) -> Cred {
        let r = match sel {
            Sel::Main(r) => r,
            Sel::Fork => 0,
        };
        let i = &self.idents[id];
        Cred::Token { dev: i.dev, generation: i.tokens.get(&r).copied().unwrap_or(0) }
    }

    fn sel_index(sel: Sel) -> usize {
        match sel {
            Sel::Main(r) => r,
            Sel::Fork => 0,
        }
    }

    /// Routes socket messages the relay sent to the sockets' devices.
    pub(super) fn route(&mut self, sel: Sel, outs: Vec<Out>) {
        for o in outs {
            let found = self.idents.iter().position(|i| {
                i.proc.as_ref().and_then(|p| p.conn.as_ref()).is_some_and(|c| c.sel == sel && c.socket == Some(o.socket))
            });
            let Some(id) = found else { continue };
            let mut msg = o.msg;
            if let SockMsg::Push(page) = &mut msg
                && !self.withhold(id, page) {
                    continue;
                }
            let p = self.idents[id].proc.as_ref().expect("running");
            let (proc, conn) = (p.no, p.conn_no);
            self.send_down(id, proc, conn, Down::Sock(msg), true);
        }
    }

    /// Withholding (§31.6): entries above the seq are not served to the
    /// device while the others progress. The relay shows it a stale view: the
    /// head at that seq, no entry above it (an empty page with a head above
    /// the device's log is not evidence by §12.3, and the engine would read
    /// again at once; see task D's report). Returns false when nothing is left
    /// of a push.
    fn withhold(&self, id: usize, page: &mut Page) -> bool {
        let Some(head) = self.withheld_head(id) else { return true };
        page.entries.retain(|e| e.seq <= head.seq);
        page.head = head;
        page.more = page.entries.last().is_some_and(|e| e.seq < head.seq);
        !page.entries.is_empty()
    }

    fn withheld_head(&self, id: usize) -> Option<Head> {
        let (wid, w) = self.withheld?;
        if wid != id {
            return None;
        }
        let r = self.archive.get((w as usize).checked_sub(1)?)?;
        Some(Head { seq: r.seq, chain: r.chain })
    }

    fn at_relay(&mut self, id: usize, proc: u32, conn: u64, sel: Sel, call: Call) {
        if sel == Sel::Fork && self.fork.is_none() {
            return;
        }
        let now = self.sel_now(sel);
        let cred = self.cred(id, sel);
        let dev = self.idents[id].dev;
        let before = self.relay(sel).head();
        match call {
            Call::Login => {
                let reply = self.relay(sel).login(now, &cred, &dev);
                self.route(sel, reply.out);
                let evs = match reply.result {
                    Ok(login) => {
                        self.idents[id].tokens.insert(Self::sel_index(sel), login.generation);
                        let cred = Cred::Token { dev, generation: login.generation };
                        match self.relay(sel).socket_open(&cred) {
                            Ok(sid) => {
                                let session = self.idents[id].proc.as_ref().filter(|p| p.no == proc).map(|p| p.session);
                                let Some(session) = session else {
                                    self.relay(sel).socket_close(sid);
                                    return;
                                };
                                let outs = self.relay(sel).socket_hello(now, sid, session, 0);
                                let mut welcome = None;
                                let mut rest = Vec::new();
                                for o in outs {
                                    match o.msg {
                                        SockMsg::Welcome(mut w) if o.socket == sid => {
                                            if let Some(hd) = self.withheld_head(id) {
                                                w.head = hd;
                                            }
                                            welcome = Some(w)
                                        }
                                        _ => rest.push(o),
                                    }
                                }
                                if let Some(c) = self.idents[id].proc.as_mut().and_then(|p| p.conn.as_mut()).filter(|c| c.no == conn) {
                                    c.socket = Some(sid);
                                } else {
                                    self.relay(sel).socket_close(sid);
                                }
                                self.route(sel, rest);
                                let mut evs = Vec::new();
                                if let Some((first_epoch, wraps)) = login.wraps.filter(|w| !w.1.is_empty()) {
                                    evs.push(Event::MissedRoots { first_epoch, wraps });
                                }
                                evs.push(Event::Connected { push: true });
                                match welcome {
                                    Some(w) => evs.push(Event::Welcome(w)),
                                    None => evs.push(Event::Disconnected { unreachable: false, paused_until_ms: None }),
                                }
                                evs
                            }
                            Err(r) => vec![
                                Event::RelayRefused { req: None, refusal: r },
                                Event::Disconnected { unreachable: false, paused_until_ms: None },
                            ],
                        }
                    }
                    Err(r) => vec![
                        Event::RelayRefused { req: None, refusal: r },
                        Event::Disconnected { unreachable: false, paused_until_ms: None },
                    ],
                };
                self.send_down(id, proc, conn, Down::Events(evs), true);
            }
            Call::Hello => {
                let reply = self.relay(sel).hello(now, &cred);
                let key = self.idents[id].proc.as_ref().map_or(0, |p| KEY_HELLO | p.hello_no);
                let ev = match reply.result {
                    Ok(mut w) => {
                        if let Some(hd) = self.withheld_head(id) {
                            w.head = hd;
                        }
                        Event::Welcome(w)
                    }
                    Err(r) => whole(None, r),
                };
                self.send_down(id, proc, conn, Down::Answer(key, ev), false);
            }
            Call::Read { req, after, limit } => {
                let reply = self.relay(sel).read(now, &cred, after, limit);
                let ev = match reply.result {
                    Ok(mut page) => {
                        self.withhold(id, &mut page);
                        Event::Page { req: Some(req), page }
                    }
                    Err(r) => whole(Some(req), r),
                };
                self.send_down(id, proc, conn, Down::Answer(req, ev), false);
            }
            Call::Append { req, frames } => {
                let reply = self.relay(sel).append(now, &cred, &frames);
                self.route(sel, reply.out);
                let ev = match reply.result {
                    Ok(results) => {
                        let mut acks = Vec::new();
                        for (f, r) in frames.iter().zip(&results) {
                            if matches!(r, AppendResult::New { .. } | AppendResult::Dup { .. })
                                && let Some((eid, _, _)) = self.frames.get(&h(f)) {
                                    acks.push((*eid, sel == Sel::Main(self.home)));
                                }
                        }
                        if !acks.is_empty() {
                            self.pending_acks.insert((id, proc, req), acks);
                        }
                        Event::AppendResults { req, results }
                    }
                    Err(r) => whole(Some(req), r),
                };
                self.send_down(id, proc, conn, Down::Answer(req, ev), false);
            }
            Call::Admin { req, op } => {
                let (cred, ridx) = match &op {
                    AdminRequest::Door { .. } | AdminRequest::Seed { .. } => (Cred::Create, Self::sel_index(sel)),
                    _ => (cred, Self::sel_index(sel)),
                };
                let reply = self.relay(sel).admin(now, &cred, &dev, &op);
                self.route(sel, reply.out);
                if reply.result.is_ok() {
                    match &op {
                        AdminRequest::Register { dev: d, .. } => {
                            if let Some(g) = reply.token_gen {
                                self.reg_tokens.insert(*d, (ridx, g));
                            }
                        }
                        AdminRequest::Seed { first_seq, last_seq, .. } => {
                            if let Some(g) = reply.token_gen {
                                self.idents[id].tokens.insert(ridx, g);
                            }
                            if ridx != self.home {
                                self.seeding = Some((ridx, *first_seq, *last_seq));
                            }
                        }
                        _ => {}
                    }
                }
                let carried = match &op {
                    AdminRequest::Register { frame, .. } => Some(frame.clone()),
                    AdminRequest::Fence { frame: Some(f), .. } => Some(f.clone()),
                    _ => None,
                };
                if let (Some(f), Ok(ok)) = (carried, &reply.result)
                    && ok.seq.is_some()
                        && let Some((eid, _, _)) = self.frames.get(&h(&f)) {
                            self.pending_acks.insert((id, proc, req), vec![(*eid, sel == Sel::Main(self.home))]);
                        }
                let ev = match reply.result {
                    Ok(ok) => Event::AdminResult { req, result: Ok(ok) },
                    Err(r) if is_whole_admin(&r) => Event::RelayRefused { req: Some(req), refusal: r },
                    Err(r) => Event::AdminResult { req, result: Err(r) },
                };
                self.send_down(id, proc, conn, Down::Answer(req, ev), false);
            }
            Call::Blob { req, op } => {
                let reply = self.relay(sel).blob(now, &cred, &op);
                self.route(sel, reply.out);
                let ev = match reply.result {
                    Ok(ok) => Event::BlobResult { req, result: Ok(ok) },
                    Err(r) if is_whole_admin(&r) => Event::RelayRefused { req: Some(req), refusal: r },
                    Err(r) => Event::BlobResult { req, result: Err(r) },
                };
                self.send_down(id, proc, conn, Down::Answer(req, ev), false);
            }
        }
        if self.relay(sel).head() != before {
            self.relay_line(sel, "store");
        }
        // The board's home moves once the new relay holds the whole seeded tail.
        if let Some((r, first, last)) = self.seeding
            && sel == Sel::Main(r) && self.relays[r].head().is_some_and(|h| h.seq >= last) {
                self.seeding = None;
                self.rehome(r, first);
            }
    }

    /// The board's home moves to relay `r` once it is seeded (§28.3 stage 3):
    /// the reference log continues there from `first_seq`.
    fn rehome(&mut self, r: usize, first_seq: u64) {
        let old: Vec<LogRow> = self.archive.iter().filter(|x| x.seq >= first_seq).cloned().collect();
        if !old.is_empty() {
            self.branches.push(BranchLog { rows: self.archive.clone(), why: "the old relay after a move" });
        }
        self.archive.truncate(first_seq.saturating_sub(1) as usize);
        self.home = r;
        self.moved = true;
        self.sync_archive();
    }
}

/// A whole-request refusal of `Hello`, `Read` or `Append` (`device.md` §10.6).
fn whole(req: Option<u64>, r: RelayRefusal) -> Event<ToyCommand> {
    if r.status == 408 || r.status == 503 {
        Event::Disconnected { unreachable: true, paused_until_ms: None }
    } else {
        Event::RelayRefused { req, refusal: r }
    }
}

/// The refusals of an `Admin` or `Blob` that come as `RelayRefused` (`device.md` §10.6).
fn is_whole_admin(r: &RelayRefusal) -> bool {
    matches!(r.code.as_str(), "relay/fenced" | "relay/revoked" | "relay/no_board" | "relay/seeding")
}

/// The confirmed head, or `None` while the engine holds no replica
/// (`Engine::confirmed` panics then).
pub(crate) fn board_head(e: &Eng) -> Option<Head> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| e.confirmed().1)).ok()
}

pub(crate) fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
