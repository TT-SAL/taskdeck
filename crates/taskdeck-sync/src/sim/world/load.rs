//! The workload (engine.md §31.5), the simulated user, pairing, the
//! operations, and the faults (§31.6).

use super::*;

impl World<'_> {
    pub(super) fn step_load(&mut self, ev: Ev) {
        match ev {
            Ev::Work { machine } => {
                if self.workload_on && self.t < self.t_f {
                    self.work_action(machine);
                    let next = self.t + exp_draw(&mut self.work, 600_000);
                    if next < self.t_f {
                        self.at(next, Ev::Work { machine });
                    }
                }
            }
            Ev::Sleep { machine } => {
                if !self.workload_on || !self.machines[machine].awake {
                    return;
                }
                self.machines[machine].awake = false;
                let id = self.machines[machine].ident;
                if id != usize::MAX
                    && let Some(proc) = self.proc_no(id) {
                        if let Some(p) = self.idents[id].proc.as_mut() {
                            p.exiting = true;
                        }
                        self.deliver(id, Event::Shutdown);
                        self.at(self.t + 10_000, Ev::Exit { id, proc });
                    }
            }
            Ev::Wake { machine } => self.wake(machine),
            Ev::Poll { machine } => {
                self.poll(machine);
                let next = self.t + 300_000 + self.work.below(60_000);
                if next < self.t_end {
                    self.at(next, Ev::Poll { machine });
                }
            }
            Ev::Pair { machine, replaces } => self.pair(machine, replaces),
            Ev::Repair { id } => self.begin_repair(id),
            Ev::PairCheck { ticket } => self.pair_check(ticket),
            Ev::PairRetry { pp } => self.pair_retry(*pp),
            Ev::Fault(f) => self.fault(f),
            Ev::OpAt(k) => self.op_at(k),
            Ev::Settle => self.settle(),
            _ => {}
        }
    }

    fn wake(&mut self, machine: usize) {
        if self.machines[machine].awake {
            return;
        }
        self.machines[machine].awake = true;
        let id = self.machines[machine].ident;
        if id != usize::MAX && self.idents[id].proc.is_none() {
            // `device.md` §10.7: the backoff is skipped once after a resume.
            self.idents[id].skip_backoff = true;
            self.start_proc(id);
        }
    }

    fn settle(&mut self) {
        // §31.6: faults stop at T_f; the settle runs with duties on and no workload.
        self.faults_on = false;
        self.workload_on = false;
        self.withheld = None;
        if self.fork.is_some() {
            self.fault(FaultEv::ForkEnd);
        }
        for s in &mut self.skew {
            *s = 0;
        }
        for m in 0..self.machines.len() {
            self.wake(m);
        }
    }

    /// A running, joined identity of the machine, awake.
    fn running(&self, machine: usize) -> Option<usize> {
        let id = self.machines[machine].ident;
        (id != usize::MAX && self.machines[machine].awake && self.idents[id].proc.is_some()).then_some(id)
    }

    fn engine(&self, id: usize) -> Option<&Eng> {
        self.idents[id].proc.as_ref().filter(|_| !self.idents[id].unloaded).map(|p| &p.engine)
    }

    // ---------- toy commands

    fn work_action(&mut self, machine: usize) {
        let Some(id) = self.running(machine) else { return };
        let Some(e) = self.engine(id) else { return };
        let Some(device_no) = e.device_no() else { return };
        let view = e.view().clone();
        let m = self.machine_of(id);
        let wall = self.wall(m);
        // A held create is followed by a rename or a forget (§9.3), or released.
        if let Some((_, item, _)) = self.idents[id].held {
            match self.work.below(10) {
                0..=4 => {
                    let name = self.work.below(1_000) as u32;
                    self.enqueue(id, ToyCommand::Name { id: item, name }, wall, false);
                }
                5..=7 => {
                    let t = self.enqueue(id, ToyCommand::Forget { id: item }, wall, false);
                    self.idents[id].forget_ticket = Some(t);
                }
                _ => {
                    self.idents[id].held = None;
                    self.deliver(id, Event::ReleaseHold);
                }
            }
            return;
        }
        let keys: Vec<u8> = view.keys.keys().copied().collect();
        let pick_key = |w: &mut Self| -> u8 {
            if !keys.is_empty() && w.work.chance(600_000) { keys[w.work.below(keys.len() as u64) as usize] } else { w.work.below(8) as u8 }
        };
        let r = self.work.below(100);
        let cmd = match r {
            0..=34 => {
                let key = pick_key(self);
                ToyCommand::Set { key, value: self.work.below(1_000) as i64 }
            }
            35..=59 => {
                let key = pick_key(self);
                ToyCommand::Add { key, delta: self.work.below(21) as i64 - 10 }
            }
            60..=69 => ToyCommand::Del { key: pick_key(self) },
            70..=84 => {
                let n = self.idents[id].next_n;
                self.idents[id].next_n = n + 1;
                let item = (u64::from(device_no) << 40) + n;
                let hold = self.work.chance(700_000);
                let name = self.work.below(1_000) as u32;
                let ticket = self.enqueue(id, ToyCommand::Create { id: item, name }, wall, hold);
                if hold {
                    self.idents[id].held = Some((ticket, item, None));
                }
                return;
            }
            85..=89 => ToyCommand::Fail { code: self.work.below(4) as u8 },
            _ if view.rules >= 2 => ToyCommand::Mul { key: pick_key(self), factor: 2 },
            _ => {
                let key = pick_key(self);
                ToyCommand::Set { key, value: -(self.work.below(1_000) as i64) }
            }
        };
        self.enqueue(id, cmd, wall, false);
    }

    fn machine_of(&self, id: usize) -> usize {
        self.idents[id].machine
    }

    fn enqueue(&mut self, id: usize, command: ToyCommand, at_ms: i64, hold: bool) -> u64 {
        self.tickets += 1;
        let ticket = self.tickets;
        let origin_state = self.idents[id].next_n.to_be_bytes().to_vec();
        self.stats.enqueued += 1;
        self.deliver(id, Event::Enqueue { ticket, command, at_ms, origin_state, hold });
        ticket
    }

    // ---------- notifications

    pub(super) fn on_notify(&mut self, id: usize, n: Notification, line: u64) {
        match n {
            Notification::Enqueued { ticket, entry_id, .. } => {
                self.stats.enqueue_ok += 1;
                let ident = &mut self.idents[id];
                match entry_id {
                    Some(eid) => {
                        self.enqueued.push((eid, id, self.t, line));
                        if let Some(h) = ident.held.as_mut()
                            && h.0 == ticket {
                                h.2 = Some(eid);
                            }
                    }
                    None => {
                        if ident.forget_ticket == Some(ticket) {
                            ident.held = None;
                        }
                    }
                }
                if ident.forget_ticket == Some(ticket) {
                    ident.forget_ticket = None;
                    ident.held = None;
                }
            }
            Notification::EnqueueRefused { ticket, .. } => {
                self.stats.enqueue_refused += 1;
                let ident = &mut self.idents[id];
                if ident.held.is_some_and(|h| h.0 == ticket) {
                    ident.held = None;
                }
                if ident.forget_ticket == Some(ticket) {
                    ident.forget_ticket = None;
                }
            }
            Notification::PairRegistered { ticket, snapshot, added_seq, .. } => self.pair_done(id, ticket, snapshot, added_seq),
            Notification::PairFailed { ticket, .. } => {
                if let Some(dev) = self.pair_devs.get(&ticket).copied() {
                    let call = self.idents[id].calls;
                    self.idents[id].pair_failed.push((dev, call));
                    self.idents[id].pair_failed_log.push((dev, line));
                }
                if let Some(pp) = self.pairs.remove(&ticket) {
                    self.retry_pair(pp);
                } else if let Some(z) = self.paired.remove(&ticket) {
                    // §28.4 step 4: the `DeviceAdded` was registered (so the
                    // pairing machine started the new device) and then refused;
                    // the relay drops its row, and the user pairs again.
                    self.stop_proc(z);
                    self.idents[z].retired = true;
                    let m = self.idents[z].machine;
                    self.machines[m].repairing = true;
                    if self.t + 300_000 < self.t_end {
                        self.at(self.t + 300_000, Ev::Pair { machine: m, replaces: Some(z) });
                    }
                }
            }
            Notification::OpDone { ticket, result } => {
                self.idents[id].op_done.push((ticket, line));
                if let (Some(k), Ok(())) = (self.op_tickets.get(&ticket), result) {
                    *self.stats.ops_ok.entry(k).or_default() += 1;
                }
            }
            Notification::ReplaceReady { dev } => {
                if let Some(o) = self.idents.iter().position(|i| i.dev == dev && !i.retired) {
                    self.idents[o].replace_ready = true;
                    self.at(self.t + 1, Ev::Repair { id: o });
                }
            }
            Notification::MoveCode { text } => self.move_code = Some(text),
            // A refused code (`engine/move_no_fence` before the `Fence` is applied,
            // a lockout): the user types it again at a later prompt.
            Notification::MoveCodeResult { result: Err(_) } => self.idents[id].asked.retain(|k| !k.starts_with("move ")),
            Notification::Unsent { commands, from_device_no, .. } => self.idents[id].unsent = Some((commands, from_device_no)),
            Notification::ReadyToExit
                if self.idents[id].proc.as_ref().is_some_and(|p| p.exiting) => {
                    let proc = self.proc_no(id).unwrap_or(0);
                    self.at(self.t, Ev::Exit { id, proc });
                }
            _ => {}
        }
    }

    // ---------- the simulated user (§31.5: user actions after a drawn delay)

    fn user(&mut self, id: usize, ev: Event<ToyCommand>) {
        let Some(proc) = self.proc_no(id) else { return };
        let delay = 30_000 + self.work.below(300_000);
        self.at(self.t + delay, Ev::User { id, proc, ev });
    }

    fn op(&mut self, id: usize, op: Operation) {
        self.tickets += 1;
        let ticket = self.tickets;
        let k = op_name(&op);
        *self.stats.ops_started.entry(k).or_default() += 1;
        self.op_tickets.insert(ticket, k);
        self.deliver(id, Event::Op { ticket, op });
    }

    fn poll(&mut self, machine: usize) {
        let Some(id) = self.running(machine) else { return };
        let Some(e) = self.engine(id) else { return };
        let st = e.status();
        let joined = e.device_no().is_some();
        if st.clock.blocked && !self.idents[id].asked.contains("clock") {
            self.idents[id].asked.insert("clock".into());
            self.user(id, Event::User(UserAction::ConfirmClock));
        } else if !st.clock.blocked {
            self.idents[id].asked.remove("clock");
        }
        if let Some(h) = &st.halt {
            if let Some(o) = h.skip_offer {
                let key = format!("skip {}", o.seq);
                if o.here && self.cfg.ops.skip && !self.idents[id].asked.contains(&key) {
                    self.idents[id].asked.insert(key);
                    self.tickets += 0;
                    self.op(id, Operation::Skip { seq: o.seq });
                }
            }
            if matches!(h.kind, crate::HaltKind::Commitment) {
                self.at(self.t + 1, Ev::Repair { id });
            }
        }
        for s in &st.stops {
            let key = format!("{:?}", s.action);
            match s.action {
                StopAction::ConfirmRejoin | StopAction::UseThisCopy if !self.idents[id].asked.contains(&key) => {
                    self.idents[id].asked.insert(key);
                    let a = if s.action == StopAction::ConfirmRejoin { UserAction::ConfirmRejoin } else { UserAction::UseThisCopy };
                    self.user(id, Event::User(a));
                }
                _ => {}
            }
            if matches!(s.kind, StopKind::Revoked { .. } | StopKind::Closed) || s.action == StopAction::RePair {
                self.at(self.t + 1, Ev::Repair { id });
            }
        }
        if st.moving.is_some()
            && let Some(code) = self.move_code.clone() {
                let key = format!("move {code}");
                if !self.idents[id].asked.contains(&key) {
                    self.idents[id].asked.insert(key);
                    self.user(id, Event::User(UserAction::MoveCode { text: code }));
                }
            }
        if joined && st.session == SessionState::Live
            && let Some((cmds, from)) = self.idents[id].resubmit.take() {
                let toy = ToyMachine::new(true);
                let commands: Vec<ToyCommand> = cmds.iter().filter_map(|c| toy.parse(c).ok()).collect();
                self.tickets += 1;
                let ticket = self.tickets;
                let origin_state = self.idents[id].next_n.to_be_bytes().to_vec();
                self.deliver(id, Event::Resubmit { ticket, commands, from_device_no: from, origin_state });
            }
    }

    /// Re-pair and resubmit (§24.5): export the unsent edits, retire the old
    /// identity, pair the machine again as a new device. After a
    /// `ReplaceReady` that named it (§28.5 step 3), the retirement is the one
    /// §31.7 invariant 2's last outcome names (`retired_at`, task D7).
    pub(super) fn begin_repair(&mut self, o: usize) {
        if self.idents[o].repair_started || self.idents[o].retired || !self.workload_on {
            return;
        }
        self.idents[o].repair_started = true;
        if self.idents[o].proc.is_some() {
            self.deliver(o, Event::User(UserAction::ExportUnsent));
        }
        self.stop_proc(o);
        self.idents[o].retired = true;
        if self.idents[o].replace_ready {
            self.idents[o].retired_at = Some(self.trace.lines());
        }
        let m = self.idents[o].machine;
        self.machines[m].repairing = true;
        self.at(self.t + 60_000, Ev::Pair { machine: m, replaces: Some(o) });
    }

    // ---------- pairing (§28.4, api-fake-toy.md "A second device")

    fn pair(&mut self, machine: usize, replaces: Option<usize>) {
        let (seed, name) = match (machine, replaces) {
            (FAULTY, _) => (self.work.next(), "Faulty".to_owned()),
            (m, None) => match self.pairs_seed.get(&m) {
                Some(x) => x.clone(),
                None => return,
            },
            (m, Some(o)) => (self.work.next(), format!("{} again", self.idents[o].name.clone()).chars().take(32).collect::<String>() + &m.to_string()),
        };
        let pp = PendingPair { machine, seed, name, via: usize::MAX, replaces };
        self.pair_via(pp);
    }

    fn pair_via(&mut self, mut pp: PendingPair) {
        // The existing device: joined, live, unhalted, unstopped.
        let via = (0..self.idents.len()).find(|&i| {
            let Some(e) = self.engine(i) else { return false };
            if self.idents[i].retired || !self.machines[self.idents[i].machine].awake || e.device_no().is_none() {
                return false;
            }
            let st = e.status();
            st.session == SessionState::Live && st.halt.is_none() && st.stops.is_empty()
        });
        let Some(via) = via else {
            self.retry_pair(pp);
            return;
        };
        pp.via = via;
        let (dev, sign_pk, hpke_pk) = FakeSealer::join_keys(pp.seed);
        self.tickets += 1;
        let ticket = self.tickets;
        let transcript = h(&pp.seed.to_be_bytes());
        let device = NewDevice { dev, sign_pk, hpke_pk, name: pp.name.clone(), transcript };
        self.pairs.insert(ticket, pp);
        self.pair_devs.insert(ticket, dev);
        self.deliver(via, Event::PairRegister { ticket, device });
        // A pairing whose answer never comes is started again.
        self.at(self.t + 3_600_000, Ev::PairCheck { ticket });
    }

    pub(super) fn pair_check(&mut self, ticket: u64) {
        if let Some(pp) = self.pairs.remove(&ticket) {
            self.retry_pair(pp);
        }
    }

    fn retry_pair(&mut self, pp: PendingPair) {
        if self.t + 300_000 >= self.t_end {
            return;
        }
        self.at(self.t + 300_000, Ev::PairRetry { pp: Box::new(pp) });
    }

    pub(super) fn pair_retry(&mut self, mut pp: PendingPair) {
        // Pairing again is a new attempt with new keys (`seal.md` §17): the
        // keys of a failed attempt may already be registered at the relay.
        pp.seed = self.work.next();
        self.pair_via(pp);
    }

    fn pair_done(&mut self, via: usize, ticket: u64, snapshot: crate::SnapInfo, added_seq: u64) {
        let Some(pp) = self.pairs.remove(&ticket) else { return };
        let sel = self.idents[via].proc.as_ref().and_then(|p| p.conn.as_ref().map(|c| c.sel)).unwrap_or(Sel::Main(self.home));
        let parts: Option<Vec<Vec<u8>>> =
            (0..snapshot.parts).map(|i| self.relay(sel).part(&snapshot.blob, i).cloned()).collect();
        let via_dev = self.idents[via].dev;
        let opened = parts.and_then(|parts| {
            let refs: Vec<&[u8]> = parts.iter().map(Vec::as_slice).collect();
            let e = self.engine(via)?;
            let (material, _, _) = e.sealer().open_snapshot(&refs, &snapshot, &via_dev, false).ok()?;
            let bundle = e.sealer().bundle(material.clone(), added_seq);
            let relay = self.idents[via].store.meta.as_ref().map(|m| m.relay.clone())?;
            Some((material, bundle, relay))
        });
        let Some((material, bundle, relay)) = opened else {
            self.retry_pair(pp);
            return;
        };
        let mut fake = FakeSealer::join(pp.seed, &pp.name, bundle);
        let (dev, _, _) = FakeSealer::join_keys(pp.seed);
        let token = self.reg_tokens.remove(&dev);
        if pp.machine == FAULTY {
            if let Some((r, g)) = token {
                self.faulty.push(Faulty { dev, fake, relay: r, generation: g, added_seq, ctr: 0 });
                let t = self.t + 600_000;
                self.at(t, Ev::OpAt(OpKind::PoisonSend));
            }
            return;
        }
        let m = pp.machine;
        // Ruling 9 of the D2 brief: a machine whose build lacks the board's
        // active rules is given the newer build before it pairs (`seal.md`
        // §17.9 checks the bundle; the engine answers `engine/local_fault`
        // for material it cannot load, §19.8).
        let board_rules = self.engine(via).map_or(1, |e| e.membership().rules);
        let newer = board_rules > 1 && !self.rules2(m) && !self.idents.iter().any(|i| i.machine == m && i.build > 1);
        if self.rules2(m) || newer || self.idents.iter().any(|i| i.machine == m && i.build > 1) {
            fake.set_supported_rules(&[1, 2]);
        }
        let identity = fake.export_identity().to_vec();
        let history = fake.history();
        let store = Store::new(identity, history, Bootstrap::Join { material: material.clone(), added_seq, relay });
        let seed = pp.seed;
        let id = self.new_ident(m, pp.name.clone(), seed, fake, store, material.seq);
        if let Some((r, g)) = token {
            self.idents[id].tokens.insert(r, g);
        }
        if let Some(o) = pp.replaces {
            self.idents[id].resubmit = self.idents[o].unsent.take();
            self.idents[id].build = self.idents[o].build;
        }
        if newer {
            self.idents[id].build = 2;
            *self.stats.ops_started.entry("rules_build").or_default() += 1;
        }
        self.machines[m].repairing = false;
        self.paired.insert(ticket, id);
        self.stats.paired += 1;
        self.at(self.t + 1_000, Ev::Start { id });
    }

    // ---------- operations (§31.5)

    /// A desktop device that may start an operation (§28: joined, current, not halted, not stopped).
    fn actor(&self, desktop: bool) -> Option<usize> {
        let n = self.machines.len();
        let start = (self.t / 1000) as usize % n;
        (0..n).map(|k| (start + k) % n).filter(|&m| !(desktop && self.machines[m].phone)).find_map(|m| {
            let id = self.running(m)?;
            let e = self.engine(id)?;
            e.device_no()?;
            let st = e.status();
            (st.session == SessionState::Live && st.halt.is_none() && st.stops.is_empty() && st.ops.is_empty()).then_some(id)
        })
    }

    fn op_at(&mut self, k: OpKind) {
        if !self.workload_on {
            return;
        }
        let retry = |w: &mut Self, k: OpKind| {
            if w.t + 1_800_000 < w.t_f {
                w.at(w.t + 1_800_000, Ev::OpAt(k));
            }
        };
        match k {
            OpKind::Revoke => {
                let Some(a) = self.actor(true) else { return retry(self, k) };
                let a_dev = self.idents[a].dev;
                let current: Vec<Dev> = self
                    .engine(a)
                    .map(|e| e.membership().devices.iter().filter(|d| d.status == DeviceStatus::Current).map(|d| d.dev).collect())
                    .unwrap_or_default();
                let targets: Vec<usize> = (0..self.idents.len())
                    .filter(|&i| !self.idents[i].retired && self.idents[i].dev != a_dev && current.contains(&self.idents[i].dev))
                    .collect();
                if targets.is_empty() || current.len() < 2 {
                    return retry(self, k);
                }
                let tg = targets[self.work.below(targets.len() as u64) as usize];
                let dev = self.idents[tg].dev;
                self.op(a, Operation::Revoke { dev, replace: true });
            }
            OpKind::Purge => {
                let Some(a) = self.actor(true) else { return retry(self, k) };
                self.op(a, Operation::Purge);
            }
            OpKind::Restore => {
                let Some(a) = self.actor(true) else { return retry(self, k) };
                let targets = self.engine(a).map(|e| e.restore_targets()).unwrap_or_default();
                if targets.is_empty() {
                    return retry(self, k);
                }
                let (seq, _) = targets[self.work.below(targets.len() as u64) as usize];
                self.op(a, Operation::RestoreTo { seq });
            }
            OpKind::Move => {
                if self.moved || self.fork.is_some() {
                    return retry(self, k);
                }
                let Some(a) = self.actor(true) else { return retry(self, k) };
                if self.relays.len() < 2 {
                    let board = self.relays[0].board_id();
                    self.relays.push(ModelRelay::new(board, [2; 32]));
                }
                self.op(a, Operation::Move { host: HOSTS[1].to_owned() });
            }
            OpKind::RulesBuild => {
                // A new build with rules 2 on every machine that lacks it (§25):
                // a restart with another build id.
                for id in 0..self.idents.len() {
                    if self.idents[id].retired || self.idents[id].rules2 || self.idents[id].build > 1 {
                        continue;
                    }
                    self.idents[id].build = 2;
                    if let Some(proc) = self.proc_no(id) {
                        self.deliver(id, Event::Shutdown);
                        self.at(self.t + 5_000, Ev::Exit { id, proc });
                        self.at(self.t + 6_000, Ev::Start { id });
                    }
                }
                // A workload step, not a fault of §31.6: counted, no `fault` line (§31.9).
                *self.stats.ops_started.entry("rules_build").or_default() += 1;
            }
            OpKind::Recovery => self.recovery_change(),
            OpKind::Poison => {
                let pp = PendingPair { machine: FAULTY, seed: self.work.next(), name: "Faulty".to_owned(), via: usize::MAX, replaces: None };
                self.pair_via(pp);
            }
            OpKind::PoisonSend => self.poison_send(),
        }
    }

    /// A caught-up live device and its confirmed membership.
    fn caught_up(&self) -> Option<usize> {
        let head = self.relays[self.home].head()?;
        (0..self.idents.len()).find(|&i| {
            self.engine(i).is_some_and(|e| e.confirmed().1 == head && e.device_no().is_some() && e.status().halt.is_none())
        })
    }

    /// The recovery tool (§26.1): a K-shape entry sealed as the current
    /// recovery identity with the current sheet, appended with the door, then
    /// `recovery_register` once the entry is classified `Accept`.
    fn recovery_change(&mut self) {
        let Some(d) = self.caught_up() else {
            if self.t + 1_800_000 < self.t_f {
                self.at(self.t + 1_800_000, Ev::OpAt(OpKind::Recovery));
            }
            return;
        };
        let all: Vec<(u32, [u8; 32])> = self.roots.iter().copied().collect();
        let roots = check::pick_roots(&self.archive, self.template.as_ref(), &all);
        let seed = self.work.next();
        let Some(mut tool) = self.template.as_ref().and_then(|t| t.observer(seed, &roots)) else { return };
        let e = self.engine(d).expect("running");
        let head = e.confirmed().1;
        let Ok(m) = tool.membership_from_bytes(&e.membership_bytes()) else { return };
        let view = tool.view(&m);
        let Some(rec) = view.recovery.last() else { return };
        let ctr = rec.last_ctr + 1;
        let (_, spk, hpk) = FakeSealer::join_keys(self.work.next());
        let mut entry_id = [0u8; 16];
        entry_id[..8].copy_from_slice(&self.work.next().to_be_bytes());
        entry_id[8..].copy_from_slice(&self.work.next().to_be_bytes());
        let wall = self.wall(self.idents[d].machine);
        let entry = PlainEntry {
            entry_id,
            at_ms: wall,
            based_on: head,
            body: Body::Ctl(CtlIntent::Rotate { revoke: vec![], reinstate: vec![], purge_before: None }),
        };
        let Ok(sealed) = tool.seal_recovery_key_change(&m, ctr, &entry, (spk, hpk)) else { return };
        let sel = Sel::Main(self.home);
        let now = self.sel_now(sel);
        let reply = self.relay(sel).append(now, &Cred::Door, std::slice::from_ref(&sealed.frame));
        self.route(sel, reply.out);
        self.relay_line(sel, "recovery_append");
        *self.stats.ops_started.entry("recovery_key").or_default() += 1;
        let Ok(results) = reply.result else { return };
        let Some(AppendResult::New { seq, .. }) = results.first().cloned() else { return };
        // The tool classifies the entry before it registers the key (ENG-OPS-005's rule for the engine).
        let ledger = check::ledger(&self.archive, self.template.as_ref(), &roots);
        if ledger.get(seq as usize - 1).is_some_and(|x| x.accept) {
            let rsign = spk;
            let board = self.relays[self.home].board_id();
            let dev_r = crate::sim::relay::fake_dev_r(&board, &rsign);
            let op = crate::sim::relay::AdminOp::RecoveryRegister { dev_r, sign_pk: spk, frame_hash: h(&sealed.frame), seq };
            let signer = rec.dev_r;
            let reply = self.relay(sel).op(now, &Cred::Door, &signer, &op);
            self.route(sel, reply.out);
        }
    }

    /// The faulty build (§15, §31.5 poison): a member's bare fake seals the
    /// toy's `Panic` without the trial apply and appends it.
    fn poison_send(&mut self) {
        let Some(d) = self.caught_up() else {
            if self.t + 1_800_000 < self.t_f {
                self.at(self.t + 1_800_000, Ev::OpAt(OpKind::PoisonSend));
            }
            return;
        };
        let Some(mut f) = self.faulty.pop() else { return };
        let e = self.engine(d).expect("running");
        let head = e.confirmed().1;
        let bytes = e.membership_bytes();
        let wall = self.wall(self.idents[d].machine);
        let Ok(m) = f.fake.membership_from_bytes(&bytes) else { return };
        let last = f.fake.view(&m).devices.iter().find(|x| x.dev == f.dev).map(|x| x.last_ctr);
        let Some(last) = last else {
            self.faulty.push(f);
            return;
        };
        f.ctr = last + 1;
        let Ok(entry_id) = f.fake.new_entry_id() else { return };
        let payload = ToyMachine::new(true).encode(&ToyCommand::Panic);
        let entry = PlainEntry { entry_id, at_ms: wall, based_on: head, body: Body::Cmd(payload) };
        let Ok(sealed) = f.fake.seal(&m, f.ctr, &entry) else { return };
        let sel = Sel::Main(f.relay);
        let now = self.sel_now(sel);
        let cred = Cred::Token { dev: f.dev, generation: f.generation };
        let reply = self.relay(sel).append(now, &cred, std::slice::from_ref(&sealed.frame));
        self.route(sel, reply.out);
        self.relay_line(sel, "poison");
        *self.stats.ops_started.entry("poison").or_default() += 1;
    }

    // ---------- faults (§31.6)

    pub(super) fn fault(&mut self, f: FaultEv) {
        if !self.faults_on && f != FaultEv::ForkEnd {
            return;
        }
        match f {
            FaultEv::Disconnect => {
                let open: Vec<usize> = (0..self.idents.len())
                    .filter(|&i| self.idents[i].proc.as_ref().is_some_and(|p| p.conn.as_ref().is_some_and(|c| c.socket.is_some())))
                    .collect();
                if !open.is_empty() {
                    let id = open[self.fault.below(open.len() as u64) as usize];
                    self.fault_line("disconnection", ("dev", id as i128), &[]);
                    self.close_conn(id, true);
                    self.deliver(id, Event::Disconnected { unreachable: true, paused_until_ms: None });
                }
                let rate = u64::from(self.cfg.faults.disconnects_per_hour.clamp(1, 4));
                let next = self.t + exp_draw(&mut self.fault, HOUR / rate);
                if next < self.t_f {
                    self.at(next, Ev::Fault(FaultEv::Disconnect));
                }
            }
            FaultEv::SkewJump => {
                let m = self.fault.below(self.machines.len() as u64) as usize;
                let s = self.draw_skew();
                self.skew[m] = s;
                self.fault_line("clock_skew", ("dev", m as i128), &[s]);
                let next = self.t + exp_draw(&mut self.fault, 24 * HOUR);
                if next < self.t_f {
                    self.at(next, Ev::Fault(FaultEv::SkewJump));
                }
            }
            FaultEv::RollbackMark(i) => {
                if !self.moved {
                    let snap = self.relays[self.home].clone();
                    self.rollback_marks.insert(i, snap);
                }
            }
            FaultEv::Rollback(i) => {
                let Some(snap) = self.rollback_marks.remove(&i) else { return };
                if self.moved || self.fork.is_some() {
                    return;
                }
                let to = snap.head().map_or(0, |h| h.seq);
                let from = self.relays[0].head().map_or(0, |h| h.seq);
                self.contra_since = self.t;
                let line = self.fault_line("relay_rollback", ("r", 0), &[from as i64, to as i64]);
                self.branches.push(BranchLog { rows: self.archive.clone(), why: check::WHY_ROLLBACK, line, removed_from: to + 1 });
                self.relays[0].rollback_to(&snap);
                self.archive.truncate(to as usize);
                self.relay_line(Sel::Main(0), "rollback");
                // The relay restarts from the earlier state: its sessions end.
                let open: Vec<usize> = (0..self.idents.len())
                    .filter(|&i| self.idents[i].proc.as_ref().is_some_and(|p| p.conn.as_ref().is_some_and(|c| c.sel == Sel::Main(0))))
                    .collect();
                for id in open {
                    self.close_conn(id, true);
                    self.deliver(id, Event::Disconnected { unreachable: true, paused_until_ms: None });
                }
            }
            FaultEv::ForkStart => {
                if self.moved || self.fork.is_some() {
                    return;
                }
                let cands: Vec<usize> = (0..self.idents.len()).filter(|&i| !self.idents[i].retired && self.idents[i].proc.is_some()).collect();
                if cands.len() < 2 {
                    // Too few processes to split: the fork starts later (FaultSet.fork means one fork).
                    if self.t + 600_000 < self.t_f {
                        self.at(self.t + 600_000, Ev::Fault(FaultEv::ForkStart));
                    }
                    return;
                }
                let k = 1 + self.fault.below(cands.len() as u64 - 1) as usize;
                let mut set = BTreeSet::new();
                let mut pool = cands.clone();
                for _ in 0..k {
                    let x = pool.remove(self.fault.below(pool.len() as u64) as usize);
                    set.insert(x);
                }
                let at = self.relays[0].head().map_or(0, |h| h.seq);
                let p: Vec<i64> = std::iter::once(at as i64).chain(set.iter().map(|x| *x as i64)).collect();
                self.fork_line = self.fault_line("relay_fork", ("r", 0), &p);
                self.fork_at = at;
                self.fork = Some((self.relays[0].clone(), set.clone()));
                for id in set {
                    if self.idents[id].proc.as_ref().is_some_and(|p| p.conn.is_some()) {
                        self.close_conn(id, true);
                        self.deliver(id, Event::Disconnected { unreachable: true, paused_until_ms: None });
                    }
                }
            }
            FaultEv::ForkEnd => {
                let Some((copy, set)) = self.fork.take() else { return };
                let mut rows: Vec<LogRow> = Vec::new();
                for r in copy.log().values() {
                    rows.push(r.clone());
                }
                if let Some(first) = rows.first().map(|r| r.seq) {
                    let mut all: Vec<LogRow> = self.archive.iter().take(first.saturating_sub(1) as usize).cloned().collect();
                    all.extend(rows);
                    self.branches.push(BranchLog { rows: all, why: check::WHY_FORK, line: self.fork_line, removed_from: self.fork_at + 1 });
                }
                self.contra_since = self.t;
                self.fault_line("relay_fork_end", ("r", 0), &[]);
                for id in set {
                    if self.idents[id].proc.as_ref().is_some_and(|p| p.conn.is_some()) {
                        let sel = self.idents[id].proc.as_ref().and_then(|p| p.conn.as_ref()).map(|c| c.sel);
                        if sel == Some(Sel::Fork) {
                            if let Some(p) = self.idents[id].proc.as_mut()
                                && let Some(c) = p.conn.as_mut() {
                                    c.socket = None;
                                }
                            self.close_conn(id, true);
                            self.deliver(id, Event::Disconnected { unreachable: true, paused_until_ms: None });
                        }
                    }
                }
            }
            FaultEv::WithholdStart => {
                let cands: Vec<usize> = (0..self.idents.len()).filter(|&i| !self.idents[i].retired && self.idents[i].proc.is_some()).collect();
                if cands.is_empty() || self.moved {
                    return;
                }
                let id = cands[self.fault.below(cands.len() as u64) as usize];
                let w = self.relays[self.home].head().map_or(0, |h| h.seq);
                self.withheld = Some((id, w));
                self.fault_line("relay_withholding", ("dev", id as i128), &[w as i64]);
            }
            FaultEv::WithholdEnd => {
                if self.withheld.take().is_some() {
                    self.fault_line("relay_withholding_end", ("r", 0), &[]);
                }
            }
            FaultEv::Inject(i) => {
                let sel = Sel::Main(self.home);
                let (frame, how) = if i % 2 == 0 {
                    // A frame with a bad signature: a stored frame with its last byte flipped.
                    let Some(mut f) = self.archive.last().map(|r| r.frame.clone()) else { return };
                    if let Some(b) = f.last_mut() {
                        *b ^= 0x01;
                    }
                    (f, MadeUp::BadSignature)
                } else {
                    // Another board's genesis frame.
                    let wall = self.relay_now(0);
                    (FakeSealer::create_at(self.fault.next(), "Other", wall).1, MadeUp::OtherBoard)
                };
                let now = self.sel_now(sel);
                let made_up = h(&frame);
                let Some((head, outs)) = self.relay(sel).inject_frame(now, frame) else { return };
                self.made_up.insert(made_up, how);
                let line = self.fault_line("injected_frame", ("r", self.home as i128), &[head.seq as i64, i64::from(i % 2)]);
                self.injected_seqs.push(check::Injected { seq: head.seq, chain: head.chain, line, frame: made_up, host: HOSTS[self.home].to_owned() });
                self.route(sel, outs);
                self.relay_line(sel, "inject");
            }
        }
    }
}
