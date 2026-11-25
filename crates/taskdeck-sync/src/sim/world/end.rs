//! The end of a run: the final facts, the six invariants (ENG-SIM-002), the
//! `check` trace lines and the report.

use super::*;

impl World<'_> {
    pub(super) fn finish_run(mut self) -> (SimReport, Vec<u8>) {
        let toy = ToyMachine::new(true);
        let end = self.t;
        // §31.7 invariant 6: no request outstanding longer than 120,000 ms.
        for id in 0..self.idents.len() {
            let Some(p) = self.idents[id].proc.as_ref() else { continue };
            if let Some((t0, what)) = p.outstanding.values().find(|(t0, _)| end > t0 + 120_000) {
                let detail = format!("dev {id}: a {what} request outstanding since {t0} ms");
                let line = self.trace.lines();
                self.violations.push(Violation { invariant: 6, line, detail });
            }
        }
        for id in 0..self.idents.len() {
            self.check_pair_drops(id, true);
            // A restore tail still owed at the end (task D6).
            self.owe_end(id);
        }
        let home_head = self.relays[self.home].head();
        let mut facts_idents = Vec::new();
        let last_line = self.trace.lines();
        for id in 0..self.idents.len() {
            let ident = &self.idents[id];
            let (status, final_b, device_no, current, applied) = match ident.proc.as_ref() {
                Some(p) => {
                    let e = &p.engine;
                    let (st, head) = e.confirmed();
                    let bh = toy.board_hash(st);
                    let sh = crate::state_hash(1, &bh, &e.membership_bytes());
                    let me = e.sealer().own_dev();
                    let current = e.membership().devices.iter().any(|d| d.dev == me && d.status == DeviceStatus::Current);
                    (Some(e.status()), Some((head, bh, sh)), e.device_no(), current, st.applied())
                }
                None => (ident.last_status.clone(), None, None, false, Vec::new()),
            };
            let meta = ident.store.meta.as_ref();
            let persisted = meta.is_some_and(|m| m.halt.is_some() || !m.stops.is_empty());
            let halted = status.as_ref().is_some_and(|s| s.halt.is_some() || !s.stops.is_empty());
            let revoked = status.as_ref().is_some_and(|s| s.stops.iter().any(|x| matches!(x.kind, StopKind::Revoked { .. })));
            let live = status.as_ref().is_some_and(|s| s.session == SessionState::Live);
            let caught_up = status.as_ref().is_some_and(|s| Some(s.head) == home_head);
            if halted {
                if status.as_ref().is_some_and(|s| s.halt.is_some()) {
                    self.stats.halted_at_end += 1;
                } else {
                    self.stats.stopped_at_end += 1;
                }
            }
            let converges = !ident.retired && live && device_no.is_some() && current && !halted;
            let outbox: Vec<([u8; 16], u64)> = ident
                .store
                .live_outbox()
                .iter()
                .map(|r| (r.entry_id, self.first_put.get(&r.entry_id).copied().unwrap_or(0)))
                .collect();
            let refusal_seqs = {
                let mut s = BTreeSet::new();
                for n in &ident.store.notices {
                    if let crate::NoticeOp::Add { kind: crate::NoticeKind::Refusal, seq, .. } = n {
                        s.insert(*seq);
                    }
                }
                s
            };
            let desc = match &status {
                Some(s) => format!(
                    "{} session {:?}, L {}, B {}, relay head {}, halt {:?}, stops {:?}, queued {}",
                    if ident.proc.is_some() { "running" } else { "not running" },
                    s.session,
                    s.head.seq,
                    s.board_seq,
                    home_head.map_or(0, |h| h.seq),
                    s.halt.as_ref().map(|h| (&h.kind, h.seq, &h.code)),
                    s.stops.iter().map(|x| (&x.kind, x.action)).collect::<Vec<_>>(),
                    s.queued
                ),
                None => "never started".to_owned(),
            };
            facts_idents.push(check::IdentFacts {
                dev: ident.dev,
                desc,
                retired: ident.retired,
                retired_at: ident.retired_at,
                met_reference: ident.met_reference,
                log_reach: ident.log_reach,
                converges,
                live,
                halted_or_stopped_persisted: halted && persisted,
                revoked,
                final_b,
                caught_up,
                joined_at: ident.joined_at,
                samples: ident.samples.clone(),
                crossings: ident.crossings.clone(),
                crash_halts: ident.crash_halts.clone(),
                applied,
                alarms: ident.alarms.clone(),
                took: ident.took.clone(),
                evidence: ident.evidence.clone(),
                outbox,
                refusal_seqs,
                pair_failed: ident.pair_failed_log.clone(),
                op_done: ident.op_done.clone(),
                expect_alarm: ident.expect_alarm.as_ref().map(|(l, w, _)| (*l, w.clone())),
                last_line,
            });
        }
        #[cfg(test)]
        if let Some(f) = &self.tamper_facts {
            f(&mut facts_idents);
        }
        let (board, relay) = match self.idents.first().map(|i| &i.genesis) {
            Some(Some((b, r))) => (b.clone(), r.clone()),
            _ => (Vec::new(), RelayTarget::default()),
        };
        let roots: Vec<(u32, [u8; 32])> = self.roots.iter().copied().collect();
        let found = {
            let facts = Facts {
                reference: &self.archive,
                branches: &self.branches,
                template: self.template.as_ref(),
                roots,
                board,
                relay,
                idents: facts_idents,
                acked: &self.acked,
                acks: &self.acks,
                enqueued: &self.enqueued,
                registered: &self.registered,
                entry_ops: &self.entry_ops,
                cancelled: &self.cancelled,
                injected: &self.injected_seqs,
                made_up: &self.made_up,
                restore_tails: &self.restore_tails,
                owed: &self.owed,
                deletions: &self.deletions,
                t_f: self.t_f,
            };
            check::check(&facts)
        };
        for (inv, line, detail) in found {
            self.violations.push(Violation { invariant: inv, line, detail });
        }
        self.violations.sort_by_key(|v| (v.invariant, v.line));
        for inv in 1..=6u8 {
            let bad: Vec<&Violation> = self.violations.iter().filter(|v| v.invariant == inv).collect();
            let detail = match bad.first() {
                Some(v) => format!("{} violation(s); first at line {}: {}", bad.len(), v.line, v.detail),
                None => String::new(),
            };
            self.trace.line(end, "check", &[("inv", V::I(i128::from(inv))), ("ok", V::I(i128::from(bad.is_empty()))), ("detail", V::S(&detail))]);
        }
        self.stats.entries = self.archive.len() as u64;
        self.stats.trace_lines = self.trace.lines();
        let stats = std::mem::take(&mut self.stats);
        let violations = std::mem::take(&mut self.violations);
        let (sha, bytes) = self.trace.finish();
        (SimReport { seed: self.seed, trace_sha256: sha, violations, stats }, bytes)
    }
}
