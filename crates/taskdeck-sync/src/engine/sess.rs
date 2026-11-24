//! Sessions (§11.1), the checks on a welcome (§11.2), the reconcile on hello
//! (§11.3), second and displaced (§11.6), whole-request refusals and pauses
//! (§11.9), and when the engine reads (§12.2).
//!
//! Rules: `sess.state` follows the table of §11.1. `sess.relay_head` is the
//! latest head the relay named in this session; a read goes out whenever it is
//! above `L`, no read is outstanding and a page may be taken. `sess.recon`
//! holds the welcome's `last_ctr` and `last_seq` until the log reaches
//! `last_seq`; nothing new is sealed before (send.rs asks `recon_done`).
//! Every answer of a request that a `Disconnected` cut off is ignored:
//! the request ids are forgotten (ENG-SEND-007).

use super::{Engine, lim};
use crate::{
    AlarmKind, Effect, Evidence, ForkSource, Head, Page, RelayRefusal, Sealer, SessionState, StateMachine, StopKind,
    Welcome,
};

pub(crate) struct Recon {
    pub r: u64,
    pub q: u64,
    /// A restored-copy catch-up (§11.3 step 3): the records matched so far.
    pub restored: bool,
    pub matched: u32,
}

pub(crate) struct Sess {
    pub state: SessionState,
    pub push: bool,
    pub relay_head: Option<Head>,
    /// `mono_ms` when a welcome or page last named a head equal to `L` (§29.1).
    pub caught_mono: Option<u64>,
    pub floor: u64,
    pub purge_floor: u64,
    pub read: Option<u64>,
    pub hello_mono: Option<u64>,
    pub connect_mono: Option<u64>,
    pub refused_login: bool,
    pub no_board: bool,
    pub recon: Option<Recon>,
    /// The relay's `last_ctr` for this device once a reconcile verified it (§10.3).
    pub w: Option<u64>,
    /// A conflict, or a refused request, asks for a new welcome before anything is sent or read.
    pub need_hello: bool,
    pub wait_welcome: bool,
    pub pause_append_until: Option<i64>,
    pub pause_all_until: Option<i64>,
    pub pause_writes_until: Option<i64>,
    pub seeding_until: Option<u64>,
    pub last_synced_ms: Option<i64>,
    pub unreachable_since_ms: Option<i64>,
    pub fenced: bool,
    pub fenced_alarmed: bool,
    /// A revoked device paused at the first entry whose root it lacks (§24.4).
    pub paused_root: bool,
    /// `wall_ms` when the session entered `Rejoining` (§19.7 step 1).
    pub rejoin_since: Option<i64>,
}

impl Default for Sess {
    fn default() -> Self {
        Sess {
            state: SessionState::Offline,
            push: false,
            relay_head: None,
            caught_mono: None,
            floor: 0,
            purge_floor: 0,
            read: None,
            hello_mono: None,
            connect_mono: None,
            refused_login: false,
            no_board: false,
            recon: None,
            w: None,
            need_hello: false,
            wait_welcome: false,
            pause_append_until: None,
            pause_all_until: None,
            pause_writes_until: None,
            seeding_until: None,
            last_synced_ms: None,
            unreachable_since_ms: None,
            fenced: false,
            fenced_alarmed: false,
            paused_root: false,
            rejoin_since: None,
        }
    }
}

impl Sess {
    pub(crate) fn can_read(&self) -> bool {
        matches!(self.state, SessionState::Reconciling | SessionState::Live)
    }
}

impl<SM: StateMachine, S: Sealer> Engine<SM, S> {
    fn set_session(&mut self, s: SessionState) {
        if self.sess.state != s {
            self.sess.state = s;
            self.status_dirty = true;
        }
    }

    pub(crate) fn evidence_stop(&self) -> bool {
        self.has_stop(|k| matches!(k, StopKind::Evidence))
    }

    pub(crate) fn revoked(&self) -> bool {
        self.has_stop(|k| matches!(k, StopKind::Revoked { .. }))
    }

    /// The guard of the `Offline` row of §11.1.
    fn may_connect(&self) -> bool {
        if self.exiting || self.meta.relay.host.is_empty() {
            return false;
        }
        if self.has_stop(|k| matches!(k, StopKind::IdentityClone | StopKind::Closed)) {
            return false;
        }
        if self.sess.pause_all_until.is_some_and(|t| self.wall < t) {
            return false;
        }
        if self.sess.seeding_until.is_some_and(|t| self.mono < t) {
            return false;
        }
        if self.revoked() || self.sess.refused_login || self.sess.no_board {
            // ENG-EPOCH-004, §11.9: at most once per 3,600,000 ms.
            if self.sess.connect_mono.is_some_and(|t| self.mono.saturating_sub(t) < lim::SLOW_CONNECT_MS) {
                return false;
            }
        }
        true
    }

    pub(crate) fn say_hello(&mut self) {
        self.sess.hello_mono = Some(self.mono);
        self.fx(Effect::Hello);
    }

    /// Step 1 of §4.5: the session table.
    pub(crate) fn step_session(&mut self) {
        match self.sess.state {
            SessionState::Offline => {
                if self.may_connect() {
                    self.sess.connect_mono = Some(self.mono);
                    self.sess.hello_mono = None;
                    let relay = self.meta.relay.clone();
                    self.fx(Effect::Connect { relay });
                    self.set_session(SessionState::Connecting);
                }
            }
            SessionState::Connecting => {
                // A welcome whose check failed left the session here; under a stop the engine
                // looks again every 900,000 ms (§12.2), and at once when the stop ended (ENG-SESS-010).
                if let Some(t) = self.sess.hello_mono {
                    let stopped = self.evidence_stop();
                    if (stopped && self.mono.saturating_sub(t) >= lim::STOP_HELLO_MS)
                        || (!stopped && self.sess.need_hello)
                    {
                        self.sess.need_hello = false;
                        self.say_hello();
                    }
                }
            }
            SessionState::Reconciling | SessionState::Live => {
                let since = self.sess.hello_mono.map(|t| self.mono.saturating_sub(t));
                if self.evidence_stop() {
                    if since.is_none_or(|d| d >= lim::STOP_HELLO_MS) {
                        self.say_hello();
                    }
                } else if (self.sess.need_hello || self.sess.wait_welcome) && since.is_none_or(|d| d >= 1) {
                    // A new reconcile before anything is sealed again (§11.4), or the welcome a
                    // refused request waits for (§11.9); never twice for one reason.
                    if self.sess.need_hello || since.is_none_or(|d| d >= lim::POLL_MS) {
                        self.sess.need_hello = false;
                        self.say_hello();
                    }
                } else if self.sess.state == SessionState::Live
                    && !self.sess.push
                    && since.is_none_or(|d| d >= lim::POLL_MS)
                {
                    self.say_hello();
                }
            }
            SessionState::Rejoining => {}
        }
    }

    /// Step 2 of §4.5, the session's part: reads and the move to `Live` (§12.2, §12.3 step 10).
    pub(crate) fn step_reads(&mut self) {
        if !self.sess.can_read() {
            return;
        }
        if !self.pipe.need.is_empty() {
            self.fetch_skip_frames();
        }
        let head = self.sess.relay_head.map_or(0, |h| h.seq);
        let idle = self.pipe.flight.is_none() && self.pipe.stash.is_none();
        if head > self.l {
            if self.sess.read.is_none() && self.can_take_page() && !self.sess.wait_welcome && !self.follow_holds_reads() {
                let req = self.id();
                self.sess.read = Some(req);
                let after = self.l;
                self.fx(Effect::Read { req, after, limit: lim::PAGE_ENTRIES as u16 });
            }
        } else if idle && self.sess.read.is_none() {
            // The run has ended.
            if self.sess.relay_head.is_some() {
                self.sess.caught_mono.get_or_insert(self.mono);
            }
            if self.pipe.view_stale && self.pipe.batches.is_empty() {
                self.pipe.view_stale = false;
                self.pipe.pages_since_rebase = 0;
                self.rebase_due = true;
            }
            if self.sess.state == SessionState::Reconciling && self.recon_done() && !self.evidence_stop() {
                self.set_session(SessionState::Live);
            }
            self.fenced_check();
            self.rejoin_at_head();
        }
        if self.pipe.pages_since_rebase >= 16 && self.pipe.batches.is_empty() {
            // §8.4 case 2.
            self.pipe.pages_since_rebase = 0;
            self.pipe.view_stale = false;
            self.rebase_due = true;
        }
    }

    /// Is the reconcile of the latest welcome complete (§11.3)?
    pub(crate) fn recon_done(&self) -> bool {
        self.sess.recon.is_none()
    }

    pub(crate) fn on_connected(&mut self, push: bool) {
        if self.sess.state != SessionState::Connecting {
            return;
        }
        self.sess.push = push;
        self.sess.refused_login = false;
        self.sess.no_board = false;
        if !push {
            self.say_hello();
        } else {
            self.sess.hello_mono = Some(self.mono);
        }
        self.status_dirty = true;
    }

    pub(crate) fn on_disconnected(&mut self, unreachable: bool, paused_until_ms: Option<i64>) {
        self.drop_requests();
        self.set_session(SessionState::Offline);
        self.sess.relay_head = None;
        self.sess.caught_mono = None;
        self.sess.recon = None;
        self.sess.need_hello = false;
        self.sess.wait_welcome = false;
        self.sess.hello_mono = None;
        self.sess.fenced = false;
        self.sess.rejoin_since = None;
        if unreachable && self.sess.unreachable_since_ms.is_none() {
            self.sess.unreachable_since_ms = Some(self.wall);
        }
        if paused_until_ms.is_some() {
            self.sess.pause_all_until = paused_until_ms;
        }
        self.status_dirty = true;
    }

    /// Forgets every outstanding relay request: its answer, if one comes, is ignored.
    pub(crate) fn drop_requests(&mut self) {
        self.sess.read = None;
        if self.pipe.need_req.is_some_and(|r| r.2) {
            self.pipe.need_req = None;
        }
        self.send.append = None;
        self.send.admin = None;
        self.lose_wants();
    }

    // ------------------------------------------------------------------ the welcome (§11.2, §11.3)

    /// §11.2 steps 2 and 3 with a head from a welcome or a page. True: evidence was raised.
    fn check_head(&mut self, head: Head, last_ctr: Option<u64>, source: ForkSource) -> bool {
        let mut found = false;
        let ack_above = self
            .outbox
            .iter()
            .filter_map(|r| r.r.ack)
            .filter(|a| a.0 > head.seq)
            .max_by_key(|a| a.0);
        if head.seq < self.l || ack_above.is_some() {
            found = true;
            if !self.has_unresolved(AlarmKind::RelayBehind) {
                let local_head = self.log_head();
                self.raise_evidence(
                    AlarmKind::RelayBehind,
                    None,
                    None,
                    None,
                    Evidence::RelayBehind { relay_head: head, relay_last_ctr: last_ctr, local_head, ack: ack_above },
                );
            }
        }
        if head.seq <= self.l
            && let Some(own) = self.chain_at(head.seq)
                && own != head.chain {
                    found = true;
                    if !self.has_unresolved(AlarmKind::Fork) {
                        self.raise_evidence(
                            AlarmKind::Fork,
                            Some(head.seq),
                            None,
                            None,
                            Evidence::Fork { local_chain: own, relay_chain: head.chain, source },
                        );
                    }
                }
        found
    }

    pub(crate) fn on_welcome(&mut self, w: Welcome) {
        if self.sess.state == SessionState::Offline {
            return;
        }
        // Step 1.
        self.measure(w.relay_now);
        self.sess.push = w.push;
        self.sess.floor = w.floor;
        self.sess.purge_floor = w.purge_floor;
        self.sess.wait_welcome = false;
        self.sess.unreachable_since_ms = None;
        self.sess.hello_mono.get_or_insert(self.mono);
        // Steps 2 and 3.
        let mut evidence = self.check_head(w.head, w.last_ctr, ForkSource::Welcome);
        // Step 4.
        self.sess.fenced = w.fenced;
        // Step 5: the reconcile (§11.3).
        let a = self.me().map_or(0, |d| d.last_ctr);
        let h = self.sealed_high().max(a);
        let r = w.last_ctr.unwrap_or(0);
        let q = w.last_seq.unwrap_or(0);
        if r < a {
            evidence = true;
            if !self.has_unresolved(AlarmKind::RelayBehind) {
                let local_head = self.log_head();
                self.raise_evidence(
                    AlarmKind::RelayBehind,
                    None,
                    None,
                    None,
                    Evidence::RelayBehind { relay_head: w.head, relay_last_ctr: w.last_ctr, local_head, ack: None },
                );
            }
        }
        if self.follow_welcome(w.head.seq) {
            return;
        }
        if evidence || self.evidence_stop() {
            // ENG-SESS-008: only step 1; nothing is read, no `restored`, no `w`.
            self.repair_on_welcome(w.head, r);
            return;
        }
        self.sess.relay_head = Some(w.head);
        self.send.await_welcome = false;
        self.send.relay_ctr = None;
        self.relay_answered();
        let restored = r > h;
        if restored {
            self.registry.restored = Some((h + 1, r));
        }
        self.sess.recon = Some(Recon { r, q, restored, matched: 0 });
        self.status_dirty = true;
        if w.head.seq > self.l {
            if self.sess.state == SessionState::Connecting {
                self.set_session(SessionState::Reconciling);
            }
        } else {
            self.sess.caught_mono = Some(self.mono);
            self.sess.last_synced_ms = Some(self.wall);
            self.recon_progress();
            if self.sess.state == SessionState::Connecting {
                // ENG-SESS-009.
                self.set_session(if self.recon_done() { SessionState::Live } else { SessionState::Reconciling });
            }
        }
    }

    /// The log moved or a welcome arrived: is the reconcile complete (§11.3 steps 2 and 3)?
    pub(crate) fn recon_progress(&mut self) {
        let Some(rc) = self.sess.recon.as_ref() else { return };
        if self.l < rc.q {
            return;
        }
        let (r, restored, matched) = (rc.r, rc.restored, rc.matched);
        self.sess.recon = None;
        if restored {
            let a = self.me().map_or(0, |d| d.last_ctr);
            if a < r {
                // ENG-RECON-004: the relay claimed counters it cannot produce.
                let relay_head = self.sess.relay_head.map_or(self.l, |h| h.seq);
                self.raise_evidence(
                    AlarmKind::Withheld,
                    None,
                    None,
                    None,
                    Evidence::Withheld {
                        relay_head,
                        claimed_ctr: Some(r),
                        cursor: self.l,
                        missing_from: a + 1,
                        missing_to: r,
                        counters: true,
                    },
                );
                return;
            }
            self.registry.restored = None;
            let n = self.meta.restored_copy.unwrap_or(0);
            self.meta.restored_copy = Some(n.saturating_add(matched));
            self.touch_meta();
        }
        self.sess.w = Some(r);
    }

    /// A record matched, by `entry_id`, an own frame sequenced at a counter of the restored range.
    pub(crate) fn restored_match(&mut self, _seq: u64, ctr: u64) {
        if let (Some((a, z)), Some(rc)) = (self.registry.restored, self.sess.recon.as_mut())
            && a <= ctr && ctr <= z {
                rc.matched = rc.matched.saturating_add(1);
            }
    }

    // ------------------------------------------------------------------ pages, as the session sees them

    pub(crate) fn on_page(&mut self, req: Option<u64>, page: Page) {
        if let Some(r) = req {
            if let Some((nr, t, true)) = self.pipe.need_req
                && nr == r {
                    // §17.2: the single-frame fetch; nothing else of the page is used.
                    self.sess.read = None;
                    let frame = page.entries.into_iter().find(|e| e.seq == t).map(|e| e.frame);
                    self.skip_frame_arrived(t, frame);
                    return;
                }
            if self.sess.read != Some(r) {
                return; // an id not issued, or already answered (ENG-EFFECT-002)
            }
            self.sess.read = None;
        }
        if !self.sess.can_read() && self.sess.state != SessionState::Rejoining {
            return;
        }
        if self.follow_page(&page) {
            return;
        }
        // §12.3 steps 2 and 3.
        self.measure(page.relay_now);
        if self.check_head(page.head, None, ForkSource::PageHead) {
            return;
        }
        self.sess.floor = page.floor;
        self.sess.purge_floor = page.purge_floor;
        if page.fenced {
            self.sess.fenced = true;
        }
        if self.sess.relay_head.is_none_or(|h| h.seq < page.head.seq) {
            self.sess.relay_head = Some(page.head);
        }
        if page.head.seq == self.l {
            self.sess.caught_mono = Some(self.mono);
            self.sess.last_synced_ms = Some(self.wall);
        }
        if self.sess.state == SessionState::Rejoining {
            return;
        }
        // Step 4: the log this device needs is gone from the relay.
        if page.truncated && self.l + 1 < page.floor {
            self.enter_rejoin();
            return;
        }
        let first = page.entries.first().map(|e| e.seq);
        if req.is_none() && first.is_some_and(|s| s > self.l + 1) {
            return; // a push above L + 1 only raises the known head (§12.2)
        }
        if !self.can_take_page() {
            return;
        }
        self.take_page(page);
    }

    /// The commit moved `L`: what the session derives from it.
    pub(crate) fn after_commit(&mut self) {
        if self.sess.relay_head.is_some_and(|h| h.seq <= self.l) {
            self.sess.caught_mono = Some(self.mono);
            self.sess.last_synced_ms = Some(self.wall);
        }
        self.recon_progress();
        self.status_dirty = true;
    }

    // ------------------------------------------------------------------ second, displaced (§11.6)

    pub(crate) fn on_second(&mut self, session: [u8; 16]) {
        if self.prev_session == Some(session) {
            // The previous process of this device was still connected: ignored, once.
            self.prev_session = None;
            return;
        }
        self.identity_clone(Some(session), Vec::new());
    }

    pub(crate) fn on_displaced(&mut self, session: [u8; 16]) {
        self.identity_clone(Some(session), Vec::new());
    }

    /// ENG-SESS-003, ENG-SESS-004: stop with `identity_clone`; no `Connect` until `UseThisCopy`.
    pub(crate) fn identity_clone(&mut self, session: Option<[u8; 16]>, ctrs: Vec<u64>) {
        let id = self.alarm(AlarmKind::IdentityClone, None, None, None, Evidence::IdentityClone { session, ctrs });
        self.add_stop(StopKind::IdentityClone, Some(id));
        if self.sess.state != SessionState::Offline {
            self.fx(Effect::Disconnect);
            self.on_disconnected(false, None);
        }
    }

    // ------------------------------------------------------------------ whole-request refusals (§11.9)

    pub(crate) fn on_relay_refused(&mut self, req: Option<u64>, refusal: RelayRefusal) {
        let what = match req {
            None => {
                if self.sess.state == SessionState::Connecting && self.sess.hello_mono.is_none() {
                    "login"
                } else {
                    "hello"
                }
            }
            Some(r) => match self.forget_request(r) {
                Some(name) => name,
                None => return, // not issued, or already answered
            },
        };
        let connecting = req.is_none() && self.sess.state == SessionState::Connecting;
        match (refusal.status, refusal.code.as_str()) {
            (403, "relay/fenced") => {
                self.sess.fenced = true;
                self.status_dirty = true;
            }
            (403, "relay/revoked") => self.set_revoked(false),
            (429, "relay/budget") => {
                let until = refusal.until.unwrap_or_else(|| next_utc_midnight(self.wall));
                self.sess.pause_append_until = Some(until);
                self.status_dirty = true;
            }
            (404, "relay/no_board") => {
                self.sess.no_board = true;
                self.status_dirty = true;
            }
            (409, "relay/seeding") => self.sess.seeding_until = Some(self.mono + 60_000),
            (507, "relay/storage_refused") => {
                self.sess.pause_writes_until = Some(refusal.until.unwrap_or(self.wall));
                self.status_dirty = true;
            }
            (401, "relay/unauthorized")
                if connecting
                    && self.vk.home_seq > 0
                    && self.meta.relay.host == self.vk.home_host
                    && self.meta.relay.door == self.vk.home_door
                    && self.meta.relay.home_gen == self.vk.home_gen =>
            {
                // ENG-SESS-007: no member of the board there.
                self.add_stop(StopKind::Closed, None);
            }
            (_, code) => {
                let request = what.to_owned();
                self.alarm(AlarmKind::RelayProtocol, None, None, Some(code), Evidence::Request { request });
                if connecting {
                    self.sess.refused_login = true;
                } else {
                    self.sess.wait_welcome = true;
                }
            }
        }
    }

    /// A fenced relay whose log holds no accepted `Fence` that leads elsewhere (§11.9).
    fn fenced_check(&mut self) {
        if !self.sess.fenced || self.sess.fenced_alarmed {
            return;
        }
        let leads_elsewhere = self.vk.home_seq > 0 && self.vk.home_gen > self.meta.relay.home_gen;
        if !leads_elsewhere {
            self.sess.fenced_alarmed = true;
            self.alarm(
                AlarmKind::RelayProtocol,
                None,
                None,
                Some("relay/fenced"),
                Evidence::Request { request: "hello".to_owned() },
            );
        }
    }
}

/// The next 00:00 UTC after `wall_ms` (§10.4).
pub(crate) fn next_utc_midnight(wall_ms: i64) -> i64 {
    (wall_ms.div_euclid(lim::DAY) + 1) * lim::DAY
}
