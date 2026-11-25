//! Receive (§12): pages, the dry run (classification), the frames a Skip needs
//! (§17.2), the page's `Persist`, the commit and the membership effects of
//! control entries with their banners (§14).
//!
//! Rules: a page is dry-run when it arrives (`dry_run`), which touches no
//! engine state but the sealer. Its `Persist` (AppendLog, SetMarker, the
//! identity when a key changed, pending acknowledgements) is emitted, and the
//! page waits in `pipe.flight` until that write is durable (`page_durable`,
//! D3). Only then do `L`, `K`, `M_K` move, the membership effects run and the
//! entries enter `pipe.queue` for the board (board.rs). While a page is in
//! flight, or waits for a Skip's frames (`pipe.stash`), other pages are
//! dropped and read again. The tail of a `Start` goes the same way with
//! `from_log` set: nothing is appended, and its batches follow §15.2.

use super::{After, Engine, guard, lim};
use crate::{
    AlarmKind, Banner, BannerDetail, BannerKind, Class, ClassifyInput, CtlItem, Dev, Effect, Evidence, ForkSource,
    Halt, HaltKind, Hash32, Kind, LocalReadResult, LogRecord, Marker, Notification, Opened, Page, PersistOp, Sealer,
    StateMachine, StopKind,
};

/// A banner's subject: its device, name and key fingerprint.
type Subject = (Option<Dev>, Option<String>, Option<String>);

/// A classified entry on its way to the board.
pub(crate) struct Entry<M> {
    pub seq: u64,
    pub chain: Hash32,
    pub tau: i64,
    pub frame_hash: Hash32,
    pub author: Dev,
    /// The author's `device_no` in `M_{s-1}` (ENG-SM-001).
    pub author_no: Option<u16>,
    pub ctr: u64,
    /// The author is this device and the frame is one of its own.
    pub own: bool,
    pub class: Class,
    pub code: Option<&'static str>,
    pub opened: Option<Opened>,
    pub foreign: bool,
    pub unchecked: bool,
    pub halt: Option<Halt>,
    /// Classified again from the device's own log (a `Start`): not a first receipt.
    pub from_log: bool,
    /// `M_s`.
    pub m: M,
}

/// A page whose `Persist` is not yet durable.
pub(crate) struct Flight<M> {
    pub entries: Vec<Entry<M>>,
    pub from_log: bool,
}

/// A page (or a `Start`'s tail) waiting to be classified, or classified again
/// once a Skip's frames arrived.
pub(crate) struct Stash {
    pub entries: Vec<(u64, Vec<u8>)>,
    pub head: Option<crate::Head>,
    pub from_log: bool,
    pub keys_changed: bool,
    /// A candidate relay's page (§28.3 step 4): evidence is not raised, nothing is taken.
    pub quiet: bool,
}

enum Dry<M> {
    Done { entries: Vec<Entry<M>>, keys_changed: bool },
    Need(Vec<(u64, Hash32)>),
    Fault(&'static str),
    Paused,
}

impl<SM: StateMachine, S: Sealer> Engine<SM, S> {
    /// May a page be taken now?
    pub(crate) fn can_take_page(&self) -> bool {
        self.pipe.flight.is_none()
            && self.pipe.stash.is_none()
            && !self.no_apply()
            && !self.classify_held()
            && !self.jobs.holds_log()
    }

    /// Under a rules or commitment halt nothing above the halting seq is classified
    /// (ENG-HALT-005); a revoked device pauses at the first missing root (§24.4).
    pub(crate) fn classify_held(&self) -> bool {
        matches!(
            self.meta.halt.as_ref().map(|h| &h.kind),
            Some(HaltKind::Rules { .. }) | Some(HaltKind::Commitment)
        ) || self.sess.paused_root
    }

    /// §12.3 for a page from the relay whose head, clock and flags the session
    /// has already looked at (sess.rs). Steps 1 and 5 to 8.
    pub(crate) fn take_page(&mut self, page: Page) {
        // Step 1: limits (ENG-GEN-009).
        let total: usize = page.entries.iter().map(|e| e.frame.len()).sum();
        if page.entries.len() > lim::PAGE_ENTRIES
            || page.entries.iter().any(|e| e.frame.len() < lim::FRAME_MIN || e.frame.len() > lim::FRAME_MAX)
            || (page.entries.len() > 1 && total > lim::PAGE_BYTES)
        {
            self.protocol("page");
            return;
        }
        // Step 5: contiguity.
        let mut entries: Vec<(u64, Vec<u8>)> = Vec::with_capacity(page.entries.len());
        let mut next = self.l + 1;
        for e in page.entries {
            if e.seq <= self.l {
                // A repeated push: the frame must be the one the log holds.
                if let (Some(prev), Some(own)) = (self.chain_at(e.seq.wrapping_sub(1)), self.chain_at(e.seq)) {
                    let c = self.sealer.chain_step(&prev, e.seq, &e.frame);
                    if c != own {
                        self.raise_evidence(
                            AlarmKind::Fork,
                            Some(e.seq),
                            None,
                            None,
                            Evidence::Fork { local_chain: own, relay_chain: c, source: ForkSource::RepeatedEntry },
                        );
                        return;
                    }
                }
                continue;
            }
            if e.seq != next {
                self.raise_evidence(
                    AlarmKind::Withheld,
                    None,
                    None,
                    None,
                    Evidence::Withheld {
                        relay_head: page.head.seq,
                        claimed_ctr: None,
                        cursor: self.l,
                        missing_from: next,
                        missing_to: e.seq.saturating_sub(1).max(next),
                        counters: false,
                    },
                );
                return;
            }
            next += 1;
            entries.push((e.seq, e.frame));
        }
        if entries.is_empty() {
            return;
        }
        self.pipe.stash =
            Some(Stash { entries, head: Some(page.head), from_log: false, keys_changed: false, quiet: false });
        self.run_stash();
    }

    /// Classifies the stashed page, or pauses it for a Skip's frames.
    pub(crate) fn run_stash(&mut self) {
        let Some(mut stash) = self.pipe.stash.take() else { return };
        match self.dry_run(&stash) {
            Dry::Need(wants) => {
                stash.keys_changed = true; // conservative: the next pass cannot tell what the first one learned
                self.pipe.stash = Some(stash);
                self.pipe.need = wants;
                self.fetch_skip_frames();
            }
            Dry::Paused => {
                // A missing root on a revoked device: nothing is taken (§24.4).
                self.sess.paused_root = true;
            }
            Dry::Fault(code) => self.fault(code),
            Dry::Done { entries, keys_changed } => {
                self.note_skip_authors();
                self.pipe.skip_frames.clear();
                let keys_changed = keys_changed || stash.keys_changed;
                self.finish_page(entries, stash.head, stash.from_log, keys_changed, &stash.entries);
            }
        }
    }

    /// Steps 6 and 7 of §12.3 produced `entries` (those before the cut): step 8.
    fn finish_page(
        &mut self,
        mut entries: Vec<Entry<S::Membership>>,
        head: Option<crate::Head>,
        from_log: bool,
        keys_changed: bool,
        frames: &[(u64, Vec<u8>)],
    ) {
        // Step 7: the page's head chain.
        if let (Some(head), Some(last)) = (head, entries.last())
            && last.seq == head.seq && last.chain != head.chain {
                self.raise_evidence(
                    AlarmKind::Fork,
                    Some(head.seq),
                    None,
                    None,
                    Evidence::Fork { local_chain: last.chain, relay_chain: head.chain, source: ForkSource::PageHead },
                );
                entries.clear();
            }
        if entries.is_empty() {
            return;
        }
        if from_log {
            // The tail of a Start: already in the log. Only a changed key is written.
            if keys_changed {
                let id = self.sealer.export_identity();
                self.persist(vec![PersistOp::WriteIdentity(id)]);
            }
            self.pipe.flight = Some(Flight { entries, from_log: true });
            self.page_durable();
            return;
        }
        // Alarms the dry run raised are written ahead of the page, so that a crash
        // between the two cannot lose them (the frames are then read again).
        let notes = self.take_durable_notes();
        if self.meta_dirty {
            self.meta_dirty = false;
            let m = self.meta.clone();
            self.meta_pid = self.persist(vec![PersistOp::WriteMeta(m)]);
        }
        let first = entries[0].seq;
        let last = entries[entries.len() - 1].seq;
        let mut records = Vec::with_capacity(entries.len());
        for (e, (seq, frame)) in entries.iter().zip(frames.iter()) {
            debug_assert_eq!(e.seq, *seq);
            records.push(LogRecord { seq: e.seq, chain: e.chain, tau: e.tau, frame: frame.clone() });
        }
        let mut ops = vec![
            PersistOp::AppendLog(records),
            PersistOp::SetMarker(Marker { first, last, attempt: self.meta.attempt }),
        ];
        if keys_changed {
            ops.push(PersistOp::WriteIdentity(self.sealer.export_identity()));
        }
        if let Some(acks) = self.take_ack_puts() {
            ops.push(acks);
        }
        let pid = self.persist(ops);
        if pid == 0 {
            return;
        }
        for n in notes {
            self.after(pid, After::Notify(n));
        }
        self.pipe.flight = Some(Flight { entries, from_log: false });
        self.after(pid, After::PageDurable);
    }

    /// Step 6 of §12.3: the dry run. Touches nothing but the sealer; evidence it
    /// finds is raised, and the entries from the cut on are left out.
    fn dry_run(&mut self, stash: &Stash) -> Dry<S::Membership> {
        let Some(mut m) = self.mk.clone() else { return Dry::Fault("engine/local_fault") };
        let mut view = self.vk.clone();
        // From the class head: `C_K` and `M_K` (K = L except for the tail of a Start).
        let mut chain = match self.chain_at(self.k) {
            Some(c) => c,
            None if self.k == 0 => self.sealer.chain_zero(),
            None => self.cb,
        };
        let mut out: Vec<Entry<S::Membership>> = Vec::with_capacity(stash.entries.len());
        let mut dry: Vec<(u64, Hash32)> = Vec::with_capacity(stash.entries.len());
        let mut keys_changed = false;
        let revoked_here = self.me().is_some_and(|d| d.status == crate::DeviceStatus::Revoked);
        for (seq, frame) in &stash.entries {
            let seq = *seq;
            let c = self.sealer.chain_step(&chain, seq, frame);
            if stash.from_log && self.chain_at(seq) != Some(c) {
                // Two of the device's own records disagree (§21.4).
                return Dry::Fault("engine/local_fault");
            }
            let receipt = {
                let Self { sealer, chains, chain_first, retained, l, outbox, registry, pipe, .. } = self;
                let (retained, l, chain_first) = (*retained, *l, *chain_first);
                let dry_ref = &dry;
                let chain_at = |b: u64| -> Option<Hash32> {
                    if b < retained {
                        None
                    } else if b <= l {
                        if b < chain_first { None } else { chains.get((b - chain_first) as usize).map(|x| x.0) }
                    } else {
                        dry_ref.iter().find(|d| d.0 == b).map(|d| d.1)
                    }
                };
                let fh = sealer.frame_hash(frame);
                let restored = registry.restored;
                let from_log = stash.from_log;
                let outbox_ref = &*outbox;
                // ENG-SEALER-002: the frames this device sealed, by counter; during a
                // restored-copy reconcile the frames of §11.3 step 3; and a frame read
                // back from the device's own log, which was vetted when it was first
                // classified (its record may be gone since).
                let own_sealed = |ctr: u64| -> Option<Hash32> {
                    if let Some(r) = outbox_ref.iter().find(|r| r.r.ctr == Some(ctr) && r.r.frame_hash.is_some())
                        && (!from_log || r.r.frame_hash == Some(fh)) {
                            return r.r.frame_hash;
                        }
                    if from_log || restored.is_some_and(|(a, z)| a <= ctr && ctr <= z) {
                        return Some(fh);
                    }
                    None
                };
                let frames = &pipe.skip_frames;
                let skip_targets =
                    |t: u64| -> Option<&[u8]> { frames.iter().find(|f| f.0 == t).map(|f| f.1.as_slice()) };
                let input = ClassifyInput {
                    seq,
                    frame,
                    prev_chain: chain,
                    membership: &m,
                    chain_at: &chain_at,
                    retained_from: retained,
                    own_sealed: &own_sealed,
                    skip_targets: &skip_targets,
                };
                guard(|| sealer.classify(&input))
            };
            let r = match receipt {
                None => return Dry::Fault("engine/local_fault"),
                Some(Err(e)) if e.code == "seal/missing_root" => {
                    if revoked_here || view.devices.iter().any(|d| d.dev == self.own && d.status == crate::DeviceStatus::Revoked)
                    {
                        // §24.4 case 3: pause here; what was classified before it goes on.
                        if out.is_empty() {
                            return Dry::Paused;
                        }
                        self.sess.paused_root = true;
                        break;
                    }
                    return Dry::Fault("engine/missing_root");
                }
                Some(Err(_)) => return Dry::Fault("engine/local_fault"),
                Some(Ok(r)) => r,
            };
            keys_changed |= r.keys_changed;
            match &r.class {
                Class::NeedFrames(wants) => return Dry::Need(wants.clone()),
                Class::Tamper => {
                    // ENG-CLASS-003: evidence; this entry is the cut.
                    let code = r.code.unwrap_or("seal/tamper");
                    if stash.quiet {
                        break;
                    }
                    if stash.from_log {
                        return Dry::Fault("engine/local_fault");
                    }
                    if code.contains("counter") {
                        let expected = view
                            .devices
                            .iter()
                            .find(|d| d.dev == r.author)
                            .map(|d| d.last_ctr + 1)
                            .or_else(|| view.recovery.iter().find(|x| x.dev_r == r.author).map(|x| x.last_ctr + 1))
                            .unwrap_or(1);
                        self.raise_evidence(
                            AlarmKind::Counter,
                            Some(seq),
                            Some(r.author),
                            Some(code),
                            Evidence::Counter { ctr: r.ctr, expected },
                        );
                    } else {
                        let (at, relay_chain) =
                            r.opened.as_ref().map(|o| (o.based_on.seq, o.based_on.chain)).unwrap_or((seq, c));
                        let local = self.chain_at(at).or_else(|| dry.iter().find(|d| d.0 == at).map(|d| d.1));
                        self.raise_evidence(
                            AlarmKind::Fork,
                            Some(seq),
                            None,
                            Some(code),
                            Evidence::Fork {
                                local_chain: local.unwrap_or([0; 32]),
                                relay_chain,
                                source: ForkSource::BasedOn,
                            },
                        );
                    }
                    break;
                }
                _ => {}
            }
            let author_no = view.devices.iter().find(|d| d.dev == r.author).map(|d| d.device_no);
            let tau = match &r.class {
                Class::Accept => r.opened.as_ref().map_or(view.clock_ms, |o| o.at_ms.max(view.clock_ms)),
                _ => view.clock_ms,
            };
            let is_ctl = r.class == Class::Accept && r.opened.as_ref().is_some_and(|o| o.kind == Kind::Ctl);
            let halt = r.halt;
            let own = r.author == self.own && !r.own_dev_foreign;
            let mut entry = Entry {
                seq,
                chain: c,
                tau,
                frame_hash: r.frame_hash,
                author: r.author,
                author_no,
                ctr: r.ctr,
                own,
                class: r.class,
                code: r.code,
                opened: r.opened,
                foreign: r.own_dev_foreign,
                unchecked: r.based_on_unchecked,
                halt,
                from_log: stash.from_log,
                m: r.membership,
            };
            if is_ctl || seq == 1 {
                // Control entries change who the devices are; the view follows.
                view = self.sealer.view(&entry.m);
            } else {
                view.clock_ms = tau;
            }
            entry.tau = view.clock_ms.max(tau);
            chain = c;
            dry.push((seq, c));
            m = entry.m.clone();
            out.push(entry);
            if halt.is_some() {
                // §16.3: the dry run ends after the halting entry; nothing after it is evidence.
                break;
            }
        }
        Dry::Done { entries: out, keys_changed }
    }

    /// The entries a dry run of `stash` classifies, for a scan that takes nothing (movement.rs).
    pub(crate) fn dry_entries(&mut self, stash: &Stash) -> Vec<Entry<S::Membership>> {
        match self.dry_run(stash) {
            Dry::Done { entries, .. } => entries,
            _ => Vec::new(),
        }
    }

    // ------------------------------------------------------------------ frames a Skip needs (§17.2)

    /// Asks for the next frame a `NeedFrames` named: from the log when it holds
    /// the seq, else from the relay (ENG-SKIP-001).
    pub(crate) fn fetch_skip_frames(&mut self) {
        if self.pipe.need_req.is_some() {
            return;
        }
        // Frames of the stashed page itself (a Skip and its target in one page, or in the tail of a Start).
        while let Some(&(t, hash)) = self.pipe.need.first() {
            let local = self
                .pipe
                .stash
                .as_ref()
                .and_then(|s| s.entries.iter().find(|e| e.0 == t).map(|e| e.1.clone()));
            match local {
                Some(frame) if self.sealer.frame_hash(&frame) == hash => {
                    self.pipe.skip_frames.push((t, frame));
                    self.pipe.need.remove(0);
                }
                _ => break,
            }
        }
        let Some(&(t, _)) = self.pipe.need.first() else {
            self.run_stash();
            return;
        };
        if t >= self.floor && t <= self.l {
            let req = self.id();
            self.pipe.need_req = Some((req, t, false));
            self.fx(Effect::ReadLog { req, from: t, to: t });
        } else if self.sess.can_read() && self.sess.read.is_none() {
            // Below this device's log: the relay keeps the targets of accepted Skips (§17.4).
            // An answer without the frame means it is held nowhere (`engine/skip_unreachable`).
            let req = self.id();
            self.pipe.need_req = Some((req, t, true));
            self.sess.read = Some(req);
            self.fx(Effect::Read { req, after: t - 1, limit: 1 });
        }
    }

    /// The answer to a single-frame fetch: used only here, never by §12.3.
    pub(crate) fn skip_frame_arrived(&mut self, t: u64, frame: Option<Vec<u8>>) {
        self.pipe.need_req = None;
        let Some(&(want, hash)) = self.pipe.need.first() else { return };
        if want != t {
            return;
        }
        match frame {
            Some(f) if self.sealer.frame_hash(&f) == hash => {
                self.pipe.skip_frames.push((t, f));
                self.pipe.need.remove(0);
                self.fetch_skip_frames();
            }
            _ => {
                self.pipe.stash = None;
                self.pipe.need.clear();
                self.fault("engine/skip_unreachable");
            }
        }
    }

    /// A `LocalRead` answer for a Skip's frame.
    pub(crate) fn skip_local_read(&mut self, t: u64, result: LocalReadResult) {
        let frame = match result {
            LocalReadResult::Records(mut v) if v.len() == 1 && v[0].seq == t => Some(v.remove(0).frame),
            _ => None,
        };
        if frame.is_none() {
            self.pipe.need_req = None;
            self.pipe.stash = None;
            self.pipe.need.clear();
            self.fault("engine/local_fault");
            return;
        }
        self.skip_frame_arrived(t, frame);
    }

    // ------------------------------------------------------------------ the commit (§12.3 step 9)

    /// The page in flight is durable: `L`, `K` and `M_K` move, the membership
    /// effects run, and the entries wait for the board (ENG-RECV-002).
    pub(crate) fn page_durable(&mut self) {
        let Some(flight) = self.pipe.flight.take() else { return };
        let Some(first) = flight.entries.first().map(|e| e.seq) else { return };
        let mut last = first;
        let b = self.b;
        self.pipe.skip_at.retain(|x| x.1 > b);
        for e in flight.entries {
            last = e.seq;
            if e.class == Class::Accept
                && let Some(o) = e.opened.as_ref()
            {
                // An accepted Skip: what is reported below it for its target's seq or a later one
                // is the state before the Skip (§17.3).
                for item in &o.items {
                    if let crate::CtlItem::Skip { seq: t, .. } = item
                        && !self.pipe.skip_at.contains(&(*t, e.seq))
                    {
                        self.pipe.skip_at.push((*t, e.seq));
                    }
                }
            }
            if !flight.from_log || e.seq > self.l {
                self.push_chain(e.seq, e.chain, e.tau);
                self.l = e.seq;
            }
            self.k = e.seq;
            self.commit_entry(&e);
            self.mk = Some(e.m.clone());
            self.pipe.queue.push_back(e);
        }
        self.refresh_vk();
        if flight.from_log {
            self.plan_tail(first, last);
        } else {
            self.pipe.batches.push_back((first, last));
            self.pipe.pages_since_rebase += 1;
        }
        self.after_commit();
        // What the commit recorded (the alarm of a `Skip`-class frame, the membership's effects)
        // is written before the board applies these entries: a checkpoint above them never
        // becomes durable without it, and below a checkpoint nothing is classified again.
        self.flush_meta();
    }

    /// What one classified entry does when its membership is committed: alarms of
    /// its class (§13.1), `seen` on an own record, the membership effects of §14.1.
    fn commit_entry(&mut self, e: &Entry<S::Membership>) {
        let seq = e.seq;
        match e.class {
            Class::Skip => {
                // ENG-CLASS-002.
                self.alarm(
                    AlarmKind::SkippedFrame,
                    Some(seq),
                    None,
                    e.code,
                    Evidence::Frame { frame_hash: Some(e.frame_hash) },
                );
            }
            Class::Refuse
                if e.code.is_some_and(names_author) => {
                    self.alarm(
                        AlarmKind::RefusedFrame,
                        Some(seq),
                        Some(e.author),
                        e.code,
                        Evidence::Frame { frame_hash: None },
                    );
                }
            _ => {}
        }
        if e.foreign && !e.from_log {
            // §13.1: stop sending; applying goes on.
            let id = self.alarm(
                AlarmKind::ForeignSigner,
                Some(seq),
                Some(self.own),
                None,
                Evidence::ForeignSigner { ctr: e.ctr },
            );
            self.add_stop(StopKind::ForeignSigner, Some(id));
        }
        if e.own {
            if let Some(o) = &e.opened
                && let Some(i) = self.rec_index(&o.entry_id) {
                    let rec = &mut self.outbox[i];
                    if rec.r.seen.is_none() {
                        rec.r.seen = Some(seq);
                        self.restored_match(seq, e.ctr);
                    }
                    self.status_dirty = true;
                }
            self.restored_clear(e);
        }
        if !e.from_log {
            self.echo_entry(seq, e.chain, e.frame_hash);
        }
        match e.halt {
            Some(Halt::Commitment) => self.halt(HaltKind::Commitment, seq, e.author, "seal/commitment"),
            Some(Halt::UnknownRules(v)) => self.halt(HaltKind::Rules { version: v }, seq, e.author, "core/unknown_rules"),
            None => {}
        }
        if e.class != Class::Accept {
            return;
        }
        let Some(o) = &e.opened else { return };
        if o.kind != Kind::Ctl {
            return;
        }
        // ENG-CTL-001: membership effects, also while the board is crash-halted.
        let view = self.sealer.view(&e.m);
        let author_row = view.devices.iter().find(|d| d.dev == e.author);
        let (author_name, author_fp) = match author_row {
            Some(d) => (d.name.clone(), d.key_fp.clone()),
            None => ("recovery key".to_owned(), view.recovery_fp.clone()),
        };
        let has_membership_item = o
            .items
            .iter()
            .any(|i| matches!(i, CtlItem::DeviceRevoked { .. } | CtlItem::DeviceReinstated { .. }));
        for (index, item) in o.items.iter().enumerate() {
            let subject = |dev: &Dev| {
                let d = view.devices.iter().find(|d| &d.dev == dev);
                (Some(*dev), d.map(|d| d.name.clone()), d.map(|d| d.key_fp.clone()))
            };
            let none = (None, None, None);
            let mut banners: Vec<(BannerKind, Subject, BannerDetail)> = Vec::new();
            match item {
                CtlItem::Genesis { .. } => {}
                CtlItem::DeviceAdded { dev, .. } => banners.push((BannerKind::DeviceAdded, subject(dev), BannerDetail::None)),
                CtlItem::DeviceRevoked { dev } => {
                    banners.push((BannerKind::DeviceRevoked, subject(dev), BannerDetail::None));
                    if *dev == self.own {
                        self.set_revoked(true);
                    }
                }
                CtlItem::DeviceReinstated { dev } => {
                    banners.push((BannerKind::DeviceReinstated, subject(dev), BannerDetail::None));
                    if *dev == self.own {
                        self.clear_revoked();
                    }
                }
                CtlItem::EpochRotated { purge_before, .. } => {
                    if !has_membership_item {
                        banners.push((BannerKind::KeysRotated, none.clone(), BannerDetail::None));
                    }
                    if let Some(p) = purge_before {
                        banners.push((BannerKind::HistoryPurged, none.clone(), BannerDetail::Purge { before_seq: *p }));
                    }
                }
                CtlItem::RecoveryKeyChange { key_fp } => {
                    banners.push((
                        BannerKind::RecoveryChanged,
                        none.clone(),
                        BannerDetail::Recovery { key_fp: key_fp.clone() },
                    ));
                    if !e.from_log {
                        self.notify_durable(Notification::RecoveryKeyChanged { key_fp: key_fp.clone() });
                    }
                }
                CtlItem::Fence { host, home_gen, .. } => {
                    banners.push((
                        BannerKind::Moved,
                        none.clone(),
                        BannerDetail::Moved { host: host.clone(), home_gen: *home_gen },
                    ));
                    self.fence_accepted(seq, e.frame_hash, e.own);
                }
                CtlItem::ResetTo { seq: target, .. } => {
                    banners.push((BannerKind::Reset, none.clone(), BannerDetail::Reset { target: *target }))
                }
                CtlItem::Skip { seq: target, .. } => {
                    let target_author = self
                        .pipe
                        .skip_authors
                        .iter()
                        .find(|a| a.0 == *target)
                        .map(|a| a.1)
                        .or_else(|| self.meta.halt.as_ref().filter(|h| h.seq == *target).map(|h| h.author))
                        .unwrap_or([0; 16]);
                    banners.push((
                        BannerKind::Skipped,
                        none.clone(),
                        BannerDetail::Skipped { target: *target, target_author },
                    ));
                    if self.crash_halt_seq() == Some(*target) {
                        // ENG-HALT-004.
                        self.meta.halt = None;
                        self.meta.last_failed = None;
                        self.touch_meta();
                    }
                }
                CtlItem::RulesUpgrade { rules, .. } => {
                    let kind = if view.rules == *rules { BannerKind::RulesUpgraded } else { BannerKind::RulesVote };
                    banners.push((kind, none.clone(), BannerDetail::Rules { version: *rules }));
                }
            }
            // ENG-BANNER-001: on every device but the author's.
            if e.author == self.own {
                continue;
            }
            for (kind, (subject, subject_name, subject_fp), detail) in banners {
                let id = (seq, index as u8);
                if self.meta.banners.iter().any(|b| b.id == id && b.kind == kind) {
                    continue;
                }
                if self.meta.banners.len() >= lim::BANNERS {
                    let at = self.meta.banners.iter().position(|b| b.acked).unwrap_or(0);
                    self.meta.banners.remove(at);
                }
                self.meta.banners.push(Banner {
                    id,
                    kind,
                    seq,
                    at_ms: o.at_ms,
                    author: e.author,
                    author_name: author_name.clone(),
                    author_fp: author_fp.clone(),
                    subject,
                    subject_name,
                    subject_fp,
                    detail,
                    acked: false,
                });
                self.touch_meta();
                self.banners_dirty = true;
            }
        }
    }

    /// The frame of a Skip's target, as far as the engine saw it: its author for the banner.
    pub(crate) fn note_skip_authors(&mut self) {
        let frames: Vec<(u64, Dev)> = self
            .pipe
            .skip_frames
            .iter()
            .filter_map(|(t, f)| self.sealer.frame_author(f).ok().map(|(d, _)| (*t, d)))
            .collect();
        for f in frames {
            if !self.pipe.skip_authors.iter().any(|a| a.0 == f.0) {
                self.pipe.skip_authors.push(f);
            }
        }
    }
}

/// §13.2: refusals that name their author in an alarm.
pub(crate) fn names_author(code: &str) -> bool {
    matches!(
        code,
        "seal/revoked_author"
            | "seal/bad_version"
            | "seal/bad_epoch"
            | "seal/aead"
            | "seal/bad_plaintext"
            | "seal/not_authorized"
            | "seal/bad_payload"
            | "seal/bad_ctl_sig"
            | "seal/ctl_shape"
            | "seal/ctl_author"
    )
}
