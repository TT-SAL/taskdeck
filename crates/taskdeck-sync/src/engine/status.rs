//! What the user interfaces are told (§30): `status()`, `pending()`,
//! `notices()`, the other read accessors of §37, and the user actions (§30.4).

use super::{Engine, lim};
use crate::{
    AlarmKind, Blocker, ClockStatus, DeviceRow, HaltKind, HaltView, Head, MembershipView,
    MovingView, NoticeEntry, NoticeKind, NoticeOp, OpStatus, PendingMark, PersistOp, RulesStatus, Sealer,
    SessionState, SkipOffer, StateMachine, Status, StatusWarning, StopAction, StopKind, StopView, UserAction,
    VersionStatus,
};

impl<SM: StateMachine, S: Sealer> Engine<SM, S> {
    /// The pending view (§8.2). Panics when the engine holds no replica: before
    /// `Start`, or after a `Start` whose records did not load (the status then
    /// shows the local fault).
    pub fn view(&self) -> &SM::State {
        match self.view.as_ref().or(self.state.as_ref()) {
            Some(s) => s,
            None => panic!("Engine::view: no replica is loaded (before Start, or the Start failed)"),
        }
    }

    pub fn view_version(&self) -> u64 {
        self.view_version
    }

    /// The confirmed state at `B`. Panics as `view` does.
    pub fn confirmed(&self) -> (&SM::State, Head) {
        match self.state.as_ref() {
            Some(s) => (s, Head { seq: self.b, chain: self.cb }),
            None => panic!("Engine::confirmed: no replica is loaded (before Start, or the Start failed)"),
        }
    }

    /// `M_K`.
    pub fn membership(&self) -> MembershipView {
        self.vk.clone()
    }

    /// The canonical bytes of `M_B`, for backups; empty when no replica is loaded.
    pub fn membership_bytes(&self) -> Vec<u8> {
        self.mb.as_ref().map(|m| self.sealer.membership_bytes(m)).unwrap_or_default()
    }

    pub fn device_no(&self) -> Option<u16> {
        self.no_b
    }

    pub fn sealer(&self) -> &S {
        &self.sealer
    }

    /// The marks on pending rows (§30.1, V3-6).
    pub fn pending(&self) -> Vec<PendingMark> {
        let mut out = Vec::new();
        for r in &self.outbox {
            let (Some(prov), Some(cmd)) = (r.prov, r.cmd.as_ref()) else { continue };
            out.push(PendingMark {
                entry_id: r.r.entry_id,
                provisional_seq: prov,
                op: self.sm.op_name(cmd),
                target: self.sm.target(cmd),
                doomed: r.doomed,
                acknowledged: r.r.ack.is_some(),
            });
        }
        out
    }

    /// The notice and refusal records with their dismissed flag.
    pub fn notices(&self) -> Vec<NoticeEntry> {
        let mut out = Vec::with_capacity(self.notices.len());
        let mut last: Option<(u64, u32)> = None;
        for n in &self.notices {
            let index = match last {
                Some((seq, i)) if seq == n.seq => i + 1,
                _ => 0,
            };
            last = Some((n.seq, index));
            out.push(NoticeEntry { kind: n.kind, seq: n.seq, index, record: n.record.clone(), dismissed: n.dismissed });
        }
        out
    }

    /// The action a stop offers (§30.1 `stops`).
    fn stop_action(&self, kind: &StopKind, alarm: Option<u64>) -> StopAction {
        match kind {
            StopKind::Evidence => {
                let a = alarm.and_then(|id| self.meta.alarms.iter().find(|a| a.id == id));
                match a.map(|a| (a.kind, a.raised_ms)) {
                    Some((AlarmKind::RelayBehind, _)) => StopAction::RestoreTail,
                    Some((AlarmKind::Withheld, raised)) => {
                        // §21.3: withholding that lasts more than a day is answered by a re-home.
                        if self.wall.saturating_sub(raised) > lim::DAY { StopAction::ReHome } else { StopAction::Wait }
                    }
                    _ => StopAction::ReHome,
                }
            }
            StopKind::Divergence => StopAction::Adopt,
            StopKind::ForeignSigner => StopAction::ReplaceDevice,
            StopKind::IdentityClone => StopAction::UseThisCopy,
            StopKind::Revoked { .. } | StopKind::Stranded | StopKind::Closed => StopAction::RePair,
            StopKind::LocalFault { code } if code == "engine/missing_root" => StopAction::RePair,
            StopKind::LocalFault { .. } => StopAction::Restart,
            StopKind::Rebootstrap { .. } => StopAction::Wait,
            StopKind::Rejoin => StopAction::ConfirmRejoin,
        }
    }

    /// §25.2: the versions this build supports above the active one, with their blockers.
    pub(crate) fn version_status(&self) -> Vec<VersionStatus> {
        let active = self.vk.rules;
        self.sm
            .supported_rules()
            .iter()
            .copied()
            .filter(|v| *v > active)
            .map(|v| VersionStatus {
                version: v,
                blockers: self
                    .active_members()
                    .into_iter()
                    .filter(|d| !d.rules.contains(&v))
                    .map(|d| Blocker {
                        dev: d.dev,
                        name: d.name.clone(),
                        key_fp: d.key_fp.clone(),
                        last_beat_ms: d.beat_ms,
                        rules: d.rules.clone(),
                    })
                    .collect(),
                override_vote: self.vk.pending_override.filter(|p| p.0 == v),
            })
            .collect()
    }

    /// Every field of §30.1 from the engine's current state (ENG-UI-001).
    pub fn status(&self) -> Status {
        let v: &MembershipView = &self.vk;
        let queued: Vec<&crate::OutboxRecord> =
            self.outbox.iter().map(|r| &r.r).filter(|r| r.ack.is_none() && r.seen.is_none()).collect();
        let (skew, warning, blocked) = self.clock();
        let confirmed_until_ms = self.meta.clock.map(|c| c.0.saturating_add(lim::WEEK));
        let name_of = |dev: &crate::Dev| match v.devices.iter().find(|d| &d.dev == dev) {
            Some(d) => (d.name.clone(), d.key_fp.clone()),
            None => ("recovery key".to_owned(), v.recovery_fp.clone()),
        };
        let halt = self.meta.halt.as_ref().map(|h| {
            let (author_name, author_fp) = name_of(&h.author);
            let skip_offer = match h.kind {
                HaltKind::Crash { .. } => Some(SkipOffer { seq: h.seq, here: self.skip_allowed(h.seq).is_ok() }),
                _ => None,
            };
            HaltView {
                kind: h.kind.clone(),
                seq: h.seq,
                author: h.author,
                author_name,
                author_fp,
                since_ms: h.since_ms,
                code: h.code.clone(),
                skip_offer,
            }
        });
        let mut stops: Vec<StopView> = self
            .meta
            .stops
            .iter()
            .map(|s| StopView {
                kind: s.kind.clone(),
                since_ms: s.since_ms,
                alarm: s.alarm,
                action: self.stop_action(&s.kind, s.alarm),
            })
            .collect();
        if self.sess.state == SessionState::Rejoining {
            // ENG-BOOT-007: derived from the session, never persisted.
            stops.push(StopView {
                kind: StopKind::Rejoin,
                since_ms: self.sess.rejoin_since.unwrap_or(self.wall),
                alarm: None,
                action: StopAction::ConfirmRejoin,
            });
        }
        let devices = v
            .devices
            .iter()
            .map(|d| DeviceRow {
                dev: d.dev,
                name: d.name.clone(),
                key_fp: d.key_fp.clone(),
                device_no: d.device_no,
                status: d.status,
                is_self: d.dev == self.own,
                last_beat_ms: d.beat_ms,
                last_entry_ms: self.registry.last_entry.iter().find(|e| e.dev == d.dev).map(|e| e.at_ms),
                rules: d.rules.clone(),
                bootstrap_seq: d.bootstrap_seq,
            })
            .collect();
        let moving = self.fence_pending().then(|| {
            let locked = self.meta.move_tries.1;
            MovingView {
                host: v.home_host.clone(),
                home_gen: v.home_gen,
                fence_seq: v.home_seq,
                tries_left: 5u32.saturating_sub(self.meta.move_tries.0),
                locked_until_ms: (self.wall < locked).then_some(locked),
            }
        });
        let ops = self
            .meta
            .ops
            .iter()
            .map(|o| OpStatus { ticket: o.ticket, kind: super::ops::op_kind(&o.op).to_owned(), stage: o.stage, started_ms: o.started_ms })
            .collect();
        let mut warnings = Vec::new();
        if let Some(entries) = self.meta.restored_copy {
            warnings.push(StatusWarning::RestoredCopy { entries });
        }
        if let Some(from) = self.meta.continuity_unchecked {
            warnings.push(StatusWarning::ContinuityUnchecked { from });
        }
        if self.meta.based_on_unchecked > 0 {
            warnings.push(StatusWarning::BasedOnUnchecked { count: self.meta.based_on_unchecked });
        }
        let pause = [self.sess.pause_append_until, self.sess.pause_all_until, self.sess.pause_writes_until]
            .into_iter()
            .flatten()
            .filter(|t| self.wall < *t)
            .max();
        Status {
            session: self.sess.state,
            relay_host: self.meta.relay.host.clone(),
            push: self.sess.push,
            last_synced_ms: self.sess.last_synced_ms,
            unreachable_since_ms: self.sess.unreachable_since_ms,
            paused_until_ms: pause,
            queued: queued.len() as u32,
            oldest_queued_ms: queued.iter().map(|r| r.at_ms).min(),
            doomed: self.outbox.iter().filter(|r| r.doomed.is_some()).count() as u32,
            queue_large: queued.len() >= 10_000,
            head: self.log_head(),
            board_seq: self.b,
            fingerprint: self.sealer.sync_fingerprint(&Head { seq: self.b, chain: self.cb }),
            clock: ClockStatus { skew_ms: skew, warning, blocked, confirmed_until_ms },
            halt,
            stops,
            alarms: self.meta.alarms.iter().filter(|a| !a.resolved).cloned().collect(),
            banners: self.meta.banners.iter().filter(|b| !b.acked).cloned().collect(),
            devices,
            rules: RulesStatus { active: v.rules, versions: self.version_status() },
            moving,
            ops,
            notices_unread: self.notices.iter().filter(|n| !n.dismissed && n.kind == NoticeKind::Notice).count() as u32,
            refusals_unread: self.notices.iter().filter(|n| !n.dismissed && n.kind == NoticeKind::Refusal).count()
                as u32,
            warnings,
        }
    }

    // ------------------------------------------------------------------ user actions (§30.4)

    pub(crate) fn on_user(&mut self, action: UserAction) {
        match action {
            UserAction::AckBanner { id } => {
                // ENG-BANNER-002.
                if let Some(b) = self.meta.banners.iter_mut().find(|b| b.id == id && !b.acked) {
                    b.acked = true;
                    self.touch_meta();
                    self.banners_dirty = true;
                }
            }
            UserAction::DismissNotice { seq, index } => {
                if let Some(n) = self.notices.iter_mut().filter(|n| n.seq == seq).nth(index as usize)
                    && !n.dismissed {
                        n.dismissed = true;
                        self.persist(vec![PersistOp::Notices(vec![NoticeOp::Dismiss { seq, index }])]);
                        self.status_dirty = true;
                    }
            }
            UserAction::DismissAlarm { id } => {
                // ENG-UI-002: an alarm behind a stop is resolved only by its repair.
                let behind_stop = self.meta.stops.iter().any(|s| s.alarm == Some(id));
                if !behind_stop {
                    self.resolve_alarms(|a| a.id == id);
                }
            }
            UserAction::ConfirmClock => self.confirm_clock(),
            UserAction::MoveCode { text } => self.on_move_code(text),
            UserAction::OldRelayGone => self.on_old_relay_gone(),
            UserAction::ConfirmRejoin => self.confirm_rejoin(),
            UserAction::UseThisCopy => {
                self.end_stops(|s| s.kind == StopKind::IdentityClone);
                self.resolve_alarms(|a| a.kind == AlarmKind::IdentityClone);
                self.meta.conflicts = 0;
                self.touch_meta();
            }
            UserAction::ExportUnsent => self.export_unsent(),
            UserAction::AckContinuity => {
                self.meta.continuity_unchecked = None;
                self.touch_meta();
            }
            UserAction::AckRestoredCopy => {
                self.meta.restored_copy = None;
                self.touch_meta();
            }
            UserAction::AckBasedOnUnchecked => {
                self.meta.based_on_unchecked = 0;
                self.touch_meta();
            }
        }
    }


}
