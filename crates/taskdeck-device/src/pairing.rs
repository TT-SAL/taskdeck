//! Pairing (device.md §18.3; seal.md §17): the runtime of both of seal's
//! machines on the sync thread. The machines are sans-IO; this performs
//! their effects through the transport (`Job::Raw`), feeds them their
//! answers, monotonic time and ticks (SEAL-PAIR-013), and hands the engine
//! its part (`PairRegister`, engine.md §28.4).
//!
//! The six points `docs/sync/spec/CHANGES.md` left to the pairing runtime are
//! decided here (findings f3-*, `docs/sync/status/device/fit.md`):
//! 1. a `PairFailed` after E's machine is `Done` shows `Aborted { code }`;
//! 2. the new device reads the log in pages to the head and hands the
//!    machine one `Entries`, and `FetchFrames` as one `Frames`;
//! 3. a ticket is deleted only when the machine emits `DeleteTicket`;
//! 4. after an abort that follows `Registered`, nothing more is done: the
//!    device that never joins shows in the device list, and the user
//!    revokes it, as after a crash while registering;
//! 5. each machine gets `Tick { now }` every 1,000 ms of monotonic time
//!    while it is neither `Done` nor `Aborted`;
//! 6. `join_sas` strips spaces and hyphens and hands on exactly six ASCII
//!    digits; anything else is refused with `device/sas_format` and costs
//!    no attempt.

use std::sync::mpsc::Sender;

use taskdeck_seal::pair::{Existing, ExistingEffect, ExistingEvent, New, NewEffect, NewEvent};
use taskdeck_seal::{BoardId, Chain, DevId, Membership, OsEntropy};
use taskdeck_sync::{Event, Head, NewDevice, SnapInfo};
use zeroize::Zeroizing;

use crate::net::{Auth, Fail, Job, Method, Req, Resp, wire};
use crate::runtime::{Msg, Runtime, err};
use crate::strict_json::hex;
use crate::{DeviceError, PairSheet, PairState};

/// How often a waiting side reads a slot that was empty (device.md §18.3).
const POLL_MS: u64 = 2_000;
/// How often a machine gets `Tick` (decision 5).
const TICK_MS: u64 = 1_000;

/// A pairing call of the facade (§17.10).
pub(crate) enum PairCall {
    Start { reply: Sender<Result<PairSheet, DeviceError>> },
    Confirm { reply: Sender<Result<(), DeviceError>> },
    Cancel { reply: Sender<Result<(), DeviceError>> },
    Join { host: String, board: [u8; 16], door: [u8; 5], code: Zeroizing<String>, name: String, reply: Sender<Result<(), DeviceError>> },
    Sas { digits: Zeroizing<String>, reply: Sender<Result<(), DeviceError>> },
}

impl PairCall {
    pub(crate) fn refuse(self, e: DeviceError) {
        match self {
            PairCall::Start { reply } => {
                let _ = reply.send(Err(e));
            }
            PairCall::Confirm { reply } | PairCall::Cancel { reply } | PairCall::Join { reply, .. } | PairCall::Sas { reply, .. } => {
                let _ = reply.send(Err(e));
            }
        }
    }
}

/// What a pairing request was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum What {
    Open,
    SlotB,
    Post(u8),
    Delete,
    Slot(u8),
    Meta,
    Part(u32),
    Log,
    Frame(u64),
}

/// The answer of a pairing request, for attempt `attempt`.
pub(crate) struct PairNet {
    attempt: u64,
    what: What,
    result: Result<Resp, Fail>,
}

/// The existing device's side.
pub(crate) struct EPair {
    m: Existing,
    attempt: u64,
    ticket: u64,
    tid: [u8; 16],
    host: String,
    door: [u8; 5],
    address_display: String,
    state: PairState,
    code: Option<Zeroizing<String>>,
    start_reply: Option<Sender<Result<PairSheet, DeviceError>>>,
    polling: bool,
    poll_at: u64,
    in_flight: bool,
    tick_at: u64,
    register_token: Option<([u8; 16], Zeroizing<String>)>,
    dev_n: Option<[u8; 16]>,
}

/// The new device's side.
pub(crate) struct NPair {
    m: New,
    attempt: u64,
    host: String,
    board: [u8; 16],
    door: [u8; 5],
    name: String,
    state: PairState,
    tid: [u8; 16],
    ticket_key: Option<Zeroizing<[u8; 32]>>,
    /// The label of the slot awaited, while one is.
    fetch: Option<u8>,
    poll_at: u64,
    in_flight: bool,
    tick_at: u64,
    token: Option<Zeroizing<String>>,
    blob: [u8; 16],
    parts_n: u32,
    parts: Vec<Vec<u8>>,
    log: Vec<(u64, Vec<u8>)>,
    frames_wanted: Vec<u64>,
    frames: Vec<(u64, Vec<u8>)>,
    sas_reply: Option<Sender<Result<(), DeviceError>>>,
}

pub(crate) enum Pairing {
    Idle,
    E(Box<EPair>),
    N(Box<NPair>),
    Ended(PairState),
}

static ATTEMPTS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn ended(s: &PairState) -> bool {
    matches!(s, PairState::Done | PairState::Aborted { .. })
}

impl Pairing {
    pub(crate) fn state(&self) -> PairState {
        match self {
            Pairing::Idle => PairState::Idle,
            Pairing::E(e) => e.state.clone(),
            Pairing::N(n) => n.state.clone(),
            Pairing::Ended(s) => s.clone(),
        }
    }

    fn busy(&self) -> bool {
        match self {
            Pairing::E(e) => !ended(&e.state),
            Pairing::N(n) => !ended(&n.state),
            _ => false,
        }
    }

    /// The next monotonic time the runtime must act on.
    pub(crate) fn next_due(&self) -> Option<u64> {
        match self {
            Pairing::E(e) if !ended(&e.state) => {
                let poll = (e.polling && !e.in_flight).then_some(e.poll_at);
                Some(poll.map_or(e.tick_at, |p| p.min(e.tick_at)))
            }
            Pairing::N(n) if !ended(&n.state) => {
                let poll = (n.fetch.is_some() && !n.in_flight).then_some(n.poll_at);
                Some(poll.map_or(n.tick_at, |p| p.min(n.tick_at)))
            }
            _ => None,
        }
    }

    pub(crate) fn register_token(&mut self, dev: [u8; 16], token: Zeroizing<String>) {
        if let Pairing::E(e) = self {
            e.register_token = Some((dev, token));
        }
    }

    /// At shutdown: the machines end with the process (engine.md §28.4).
    pub(crate) fn cancel_all(&mut self) {
        match self {
            Pairing::E(e) if !ended(&e.state) => {
                let _ = e.m.on(ExistingEvent::Cancel, &mut OsEntropy);
            }
            Pairing::N(n) if !ended(&n.state) => {
                let _ = n.m.on(NewEvent::Cancel, &mut OsEntropy);
            }
            _ => {}
        }
    }
}

fn code_of(r: &Result<Resp, Fail>) -> &'static str {
    match r {
        Err(Fail::Insecure) => "device/insecure_url",
        Err(Fail::Unreachable) => "device/relay_unreachable",
        Ok(resp) => {
            let parsed = wire::parse_refusal(resp.status, &resp.body);
            match parsed.map(|r| r.code) {
                Some(c) => intern(&c),
                None => "device/bad_answer",
            }
        }
    }
}

/// A relay code as a `&'static str`: the codes a pairing can meet.
fn intern(c: &str) -> &'static str {
    const KNOWN: &[&str] = &[
        "relay/no_ticket",
        "relay/slot_empty",
        "relay/slot_order",
        "relay/tries",
        "relay/bad_slot",
        "relay/ticket_cap",
        "relay/ticket_exists",
        "relay/forbidden",
        "relay/unauthorized",
        "relay/no_blob",
        "relay/blob_incomplete",
        "relay/bad_blob",
        "relay/fenced",
        "relay/no_board",
        "relay/revoked",
        "relay/token_stale",
    ];
    KNOWN.iter().find(|k| **k == c).copied().unwrap_or("device/relay_refused")
}

fn is_empty_slot(r: &Result<Resp, Fail>) -> bool {
    matches!(r, Ok(resp) if resp.status == 404 && code_of(r) == "relay/slot_empty")
}

fn ok_body(r: &Result<Resp, Fail>) -> Option<&[u8]> {
    match r {
        Ok(resp) if resp.status == 200 => Some(&resp.body),
        _ => None,
    }
}

impl Runtime {
    fn pair_send(&self, attempt: u64, what: What, req: Req) {
        let Some(tx) = self.sync_sender() else { return };
        let reply = Box::new(move |result: Result<Resp, Fail>| {
            let _ = tx.send(Msg::Pair(PairNet { attempt, what, result }));
        });
        self.net.submit(Job::Raw { req, reply });
    }

    fn board_path(board: &[u8; 16]) -> String {
        format!("/v1/b/{}", hex(board))
    }

    // ------------------------------------------------------------ calls

    pub(crate) fn pair_call(&mut self, c: PairCall) {
        match c {
            PairCall::Start { reply } => self.pair_start(reply),
            PairCall::Confirm { reply } => {
                let r = match &mut self.pair {
                    Pairing::E(e) if matches!(e.state, PairState::ShowSas { .. }) => {
                        let fx = e.m.on(ExistingEvent::UserConfirmed, &mut OsEntropy);
                        self.e_effects(fx);
                        Ok(())
                    }
                    _ => Err(err("device/pair_state")),
                };
                let _ = reply.send(r);
            }
            PairCall::Cancel { reply } => {
                match &mut self.pair {
                    Pairing::E(e) if !ended(&e.state) => {
                        let fx = e.m.on(ExistingEvent::Cancel, &mut OsEntropy);
                        self.e_effects(fx);
                    }
                    Pairing::N(n) if !ended(&n.state) => {
                        let fx = n.m.on(NewEvent::Cancel, &mut OsEntropy);
                        self.n_effects(fx);
                    }
                    _ => {}
                }
                let _ = reply.send(Ok(()));
            }
            PairCall::Join { host, board, door, code, name, reply } => self.join(host, board, door, code, name, reply),
            PairCall::Sas { digits, reply } => self.join_sas(&digits, reply),
        }
    }

    /// `pair_start` (§18.3, E): the engine must be able to add a device
    /// (engine.md §28.4's refusals, checked here first), then
    /// `Existing::start`; the answer waits for the ticket's registration.
    fn pair_start(&mut self, reply: Sender<Result<PairSheet, DeviceError>>) {
        if self.pair.busy() {
            let _ = reply.send(Err(err("engine/pair_busy")));
            return;
        }
        let mono = self.mono_ms();
        let Some(s) = self.synced.as_ref() else {
            let _ = reply.send(Err(err("device/wrong_mode")));
            return;
        };
        let st = s.engine.status();
        if st.halt.is_some() {
            let _ = reply.send(Err(err("engine/halted")));
            return;
        }
        if s.engine.device_no().is_none() || !st.stops.is_empty() || st.session != taskdeck_sync::SessionState::Live {
            let _ = reply.send(Err(err("engine/not_eligible")));
            return;
        }
        let Some(relay) = s.relay.clone() else {
            let _ = reply.send(Err(err("engine/not_eligible")));
            return;
        };
        let Ok(membership) = Membership::from_canonical_bytes(&s.engine.membership_bytes()) else {
            let _ = reply.send(Err(err("device/store_corrupt")));
            return;
        };
        let (_, head) = s.engine.confirmed();
        let Ok(origin) = taskdeck_seal::relay_origin(&relay.host) else {
            let _ = reply.send(Err(err("relay/address")));
            return;
        };
        let (board, dev) = (s.board, s.dev);
        let head = taskdeck_seal::Head { seq: head.seq, chain: Chain(head.chain) };
        let (m, fx) = Existing::start(&membership, head, &origin, &DevId(dev), mono, &mut OsEntropy);
        let attempt = ATTEMPTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let ticket = self.ticket();
        self.pair = Pairing::E(Box::new(EPair {
            m,
            attempt,
            ticket,
            tid: [0; 16],
            host: relay.host.clone(),
            door: relay.door,
            address_display: wire::address_display(&relay.host, &board, &relay.door),
            state: PairState::Idle,
            code: None,
            start_reply: Some(reply),
            polling: false,
            poll_at: 0,
            in_flight: false,
            tick_at: mono + TICK_MS,
            register_token: None,
            dev_n: None,
        }));
        self.e_effects(fx);
    }

    /// `join` (§18.3, N): the address and the code were checked by the facade
    /// before any request (DEV-PAIR-003); then `CodeEntered`.
    fn join(&mut self, host: String, board: [u8; 16], door: [u8; 5], code: Zeroizing<String>, name: String, reply: Sender<Result<(), DeviceError>>) {
        if self.synced.is_some() || self.pending_create.is_some() {
            let _ = reply.send(Err(err("device/wrong_mode")));
            return;
        }
        if self.pair.busy() {
            let _ = reply.send(Err(err("device/pair_state")));
            return;
        }
        let now = self.mono_ms();
        let mut m = New::new();
        let fx = m.on(
            NewEvent::CodeEntered {
                code: code.to_string(),
                host: host.clone(),
                board_id: BoardId(board),
                door,
                name: name.clone(),
                now,
            },
            &mut OsEntropy,
        );
        if let Some(NewEffect::AskAgain { code }) = fx.first() {
            let _ = reply.send(Err(err(code.code())));
            return;
        }
        let attempt = ATTEMPTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.pair = Pairing::N(Box::new(NPair {
            m,
            attempt,
            host,
            board,
            door,
            name,
            state: PairState::Joining,
            tid: [0; 16],
            ticket_key: None,
            fetch: None,
            poll_at: 0,
            in_flight: false,
            tick_at: now + TICK_MS,
            token: None,
            blob: [0; 16],
            parts_n: 0,
            parts: Vec::new(),
            log: Vec::new(),
            frames_wanted: Vec::new(),
            frames: Vec::new(),
            sas_reply: None,
        }));
        self.mark_pairing();
        self.n_effects(fx);
        let _ = reply.send(Ok(()));
    }

    /// `join_sas` (decision 6): exactly six ASCII digits reach the machine.
    fn join_sas(&mut self, typed: &str, reply: Sender<Result<(), DeviceError>>) {
        let digits: Zeroizing<String> = Zeroizing::new(typed.chars().filter(|c| *c != ' ' && *c != '-').collect());
        if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_digit()) {
            let _ = reply.send(Err(err("device/sas_format")));
            return;
        }
        match &mut self.pair {
            Pairing::N(n) if n.state == PairState::AskSas => {
                n.sas_reply = Some(reply);
                let fx = n.m.on(NewEvent::SasEntered { digits: digits.to_string() }, &mut OsEntropy);
                self.n_effects(fx);
                if let Pairing::N(n) = &mut self.pair
                    && let Some(r) = n.sas_reply.take() {
                        let _ = r.send(Ok(()));
                    }
            }
            _ => {
                let _ = reply.send(Err(err("device/pair_state")));
            }
        }
    }

    // ------------------------------------------------------------ timers

    pub(crate) fn pairing_tick(&mut self) {
        let now = self.mono_ms();
        let board = self.synced_board();
        match &mut self.pair {
            Pairing::E(e) if !ended(&e.state) => {
                if now >= e.tick_at {
                    e.tick_at = now + TICK_MS;
                    let fx = e.m.on(ExistingEvent::Tick { now }, &mut OsEntropy);
                    self.e_effects(fx);
                }
                if let Pairing::E(e) = &mut self.pair
                    && e.polling && !e.in_flight && now >= e.poll_at && !ended(&e.state) {
                        e.in_flight = true;
                        let req = Req {
                            method: Method::Get,
                            host: e.host.clone(),
                            path: format!("{}/pair/{}/b", Self::board_path(&board), hex(&e.tid)),
                            auth: Auth::Stored,
                            proof: None,
                            body: None,
                            limit: wire::SLOT_MAX,
                        };
                        let attempt = e.attempt;
                        self.pair_send(attempt, What::SlotB, req);
                    }
            }
            Pairing::N(n) if !ended(&n.state) => {
                if now >= n.tick_at {
                    n.tick_at = now + TICK_MS;
                    let fx = n.m.on(NewEvent::Tick { now }, &mut OsEntropy);
                    self.n_effects(fx);
                }
                if let Pairing::N(n) = &mut self.pair
                    && let (Some(label), false, true) = (n.fetch, n.in_flight, now >= n.poll_at)
                        && !ended(&n.state) {
                            n.in_flight = true;
                            let req = n_slot_get(n, label);
                            let attempt = n.attempt;
                            self.pair_send(attempt, What::Slot(label), req);
                        }
            }
            _ => {}
        }
    }

    fn synced_board(&self) -> [u8; 16] {
        self.synced.as_ref().map_or([0; 16], |s| s.board)
    }

    // ------------------------------------------------------------ E's effects

    fn e_effects(&mut self, fx: Vec<ExistingEffect>) {
        let board = self.synced_board();
        for f in fx {
            let Pairing::E(e) = &mut self.pair else { return };
            let attempt = e.attempt;
            match f {
                ExistingEffect::RegisterTicket { tid, slot_a } => {
                    e.tid = tid;
                    let mut body = tid.to_vec();
                    body.extend_from_slice(&slot_a);
                    let req = Req {
                        method: Method::Post,
                        host: e.host.clone(),
                        path: format!("{}/pair", Self::board_path(&board)),
                        auth: Auth::Stored,
                        proof: None,
                        body: Some(body),
                        limit: wire::JSON_MAX,
                    };
                    self.pair_send(attempt, What::Open, req);
                }
                ExistingEffect::ShowCode { code } => e.code = Some(Zeroizing::new(code)),
                ExistingEffect::PostSlot { slot } => {
                    let label = slot.first().copied().unwrap_or(0);
                    let l = char::from(label.to_ascii_lowercase());
                    let req = Req {
                        method: Method::Post,
                        host: e.host.clone(),
                        path: format!("{}/pair/{}/{l}", Self::board_path(&board), hex(&e.tid)),
                        auth: Auth::Stored,
                        proof: None,
                        body: Some(slot),
                        limit: wire::JSON_MAX,
                    };
                    self.pair_send(attempt, What::Post(label), req);
                }
                ExistingEffect::ShowSas { digits } => {
                    e.polling = false;
                    e.state = PairState::ShowSas { digits };
                    self.mark_pairing();
                }
                ExistingEffect::NeedSnapshotAndRegister { device } => {
                    e.state = PairState::Registering;
                    e.dev_n = Some(device.dev.0);
                    let ticket = e.ticket;
                    let nd = NewDevice {
                        dev: device.dev.0,
                        sign_pk: device.sign_pk,
                        hpke_pk: device.hpke_pk,
                        name: device.name.clone(),
                        transcript: device.transcript,
                    };
                    self.mark_pairing();
                    self.feed(Event::PairRegister { ticket, device: nd });
                }
                ExistingEffect::Done { .. } => {
                    e.state = PairState::Done;
                    self.mark_pairing();
                }
                ExistingEffect::DeleteTicket { tid } => {
                    let req = Req {
                        method: Method::Delete,
                        host: e.host.clone(),
                        path: format!("{}/pair/{}", Self::board_path(&board), hex(&tid)),
                        auth: Auth::Stored,
                        proof: None,
                        body: None,
                        limit: wire::JSON_MAX,
                    };
                    self.pair_send(attempt, What::Delete, req);
                }
                ExistingEffect::Abort { code } => {
                    e.polling = false;
                    e.code = None;
                    e.state = PairState::Aborted { code: code.code() };
                    if let Some(r) = e.start_reply.take() {
                        let _ = r.send(Err(err(code.code())));
                    }
                    self.mark_pairing();
                }
            }
        }
    }

    /// `PairRegistered` (engine.md §28.4 step 3): `Registered` with the
    /// token of the `register` answer and the bundle's roots.
    pub(crate) fn pair_registered(&mut self, ticket: u64, head: Head, snapshot: SnapInfo, added_seq: u64) {
        let roots = match self.synced.as_ref() {
            Some(s) => s.engine.sealer().bundle_roots(snapshot.epoch),
            None => return,
        };
        let Pairing::E(e) = &mut self.pair else { return };
        if e.ticket != ticket || ended(&e.state) {
            return;
        }
        let token = match e.register_token.take() {
            Some((dev, t)) if Some(dev) == e.dev_n => t,
            _ => {
                let fx = e.m.on(ExistingEvent::Cancel, &mut OsEntropy);
                self.e_effects(fx);
                return;
            }
        };
        let ev = ExistingEvent::Registered {
            h: head.seq,
            chain: Chain(head.chain),
            snapshot: crate::adapt::snap_to_seal(&snapshot),
            added_seq,
            token: token.to_string(),
            roots,
            host: e.host.clone(),
            door: e.door,
        };
        let fx = e.m.on(ev, &mut OsEntropy);
        self.e_effects(fx);
    }

    /// `PairFailed` (decision 1): the pairing shows aborted with the
    /// engine's code, also after the machine reached `Done`.
    pub(crate) fn pair_failed(&mut self, ticket: u64, code: &'static str) {
        let Pairing::E(e) = &mut self.pair else { return };
        if e.ticket != ticket {
            return;
        }
        if !ended(&e.state) {
            let fx = e.m.on(ExistingEvent::Cancel, &mut OsEntropy);
            self.e_effects(fx);
        }
        if let Pairing::E(e) = &mut self.pair {
            e.state = PairState::Aborted { code };
            if let Some(r) = e.start_reply.take() {
                let _ = r.send(Err(err(code)));
            }
        }
        self.mark_pairing();
    }

    // ------------------------------------------------------------ N's effects

    fn n_effects(&mut self, fx: Vec<NewEffect>) {
        for f in fx {
            let Pairing::N(n) = &mut self.pair else { return };
            let attempt = n.attempt;
            match f {
                NewEffect::FetchSlot { tid, ticket_key, label } => {
                    n.tid = tid;
                    n.ticket_key = Some(ticket_key);
                    n.fetch = Some(label);
                    n.in_flight = true;
                    let req = n_slot_get(n, label);
                    self.pair_send(attempt, What::Slot(label), req);
                }
                NewEffect::AskAgain { code } => {
                    n.state = PairState::Aborted { code: code.code() };
                    self.mark_pairing();
                }
                NewEffect::PostSlot { slot } => {
                    let label = slot.first().copied().unwrap_or(0);
                    let req = Req {
                        method: Method::Post,
                        host: n.host.clone(),
                        path: format!("{}/pair/{}/b", Self::board_path(&n.board), hex(&n.tid)),
                        auth: Auth::Door(n.door),
                        proof: n.ticket_key.clone(),
                        body: Some(slot),
                        limit: wire::JSON_MAX,
                    };
                    self.pair_send(attempt, What::Post(label), req);
                }
                NewEffect::AskSas => {
                    n.state = PairState::AskSas;
                    self.mark_pairing();
                }
                NewEffect::ShowAccepted => {
                    n.state = PairState::Accepted;
                    self.mark_pairing();
                }
                NewEffect::FetchSnapshot { token, blob, parts } => {
                    n.token = Some(Zeroizing::new(token));
                    n.blob = blob.0;
                    n.parts_n = parts;
                    n.parts.clear();
                    n.state = PairState::Joining;
                    let req = n_get(n, format!("/blobs/{}", hex(&blob.0)), wire::JSON_MAX);
                    self.mark_pairing();
                    self.pair_send(attempt, What::Meta, req);
                }
                NewEffect::ReadLog { token, from } => {
                    n.token = Some(Zeroizing::new(token));
                    n.log.clear();
                    let after = from.saturating_sub(1);
                    let req = n_get(n, format!("/log?after={after}&limit=256"), wire::PAGE_MAX);
                    self.pair_send(attempt, What::Log, req);
                }
                NewEffect::FetchFrames { token, seqs } => {
                    n.token = Some(Zeroizing::new(token));
                    n.frames.clear();
                    n.frames_wanted = seqs;
                    if let Some(&t) = n.frames_wanted.first() {
                        let req = n_get(n, format!("/log?after={}&limit=1", t.saturating_sub(1)), wire::PAGE_MAX);
                        self.pair_send(attempt, What::Frame(t), req);
                    }
                }
                NewEffect::Joined { device_no, keys, bundle, snapshot } => {
                    let name = n.name.clone();
                    let r = crate::create::persist_join(self, device_no, keys, bundle, snapshot, &name);
                    if let Pairing::N(n) = &mut self.pair {
                        n.state = match r {
                            Ok(()) => PairState::Joining,
                            Err(e) => PairState::Aborted { code: e.code },
                        };
                    }
                    self.mark_pairing();
                }
                NewEffect::Abort { code } => {
                    n.fetch = None;
                    n.token = None;
                    n.ticket_key = None;
                    n.state = PairState::Aborted { code: code.code() };
                    if let Some(r) = n.sas_reply.take() {
                        let _ = r.send(Err(err(code.code())));
                    }
                    self.mark_pairing();
                }
            }
        }
    }

    /// N's pairing state moves to `Done` once its engine has applied the
    /// `DeviceAdded` and has a `device_no` (engine.md §19.8).
    pub(crate) fn pair_progress(&mut self) {
        let joined = self.synced.as_ref().is_some_and(|s| s.engine.device_no().is_some());
        if let Pairing::N(n) = &mut self.pair
            && joined && n.state == PairState::Joining && n.token.is_some() {
                n.state = PairState::Done;
                n.token = None;
                self.mark_pairing();
            }
    }

    // ------------------------------------------------------------ answers

    pub(crate) fn pair_net(&mut self, p: PairNet) {
        match &mut self.pair {
            Pairing::E(e) if e.attempt == p.attempt => self.e_answer(p),
            Pairing::N(n) if n.attempt == p.attempt => self.n_answer(p),
            _ => {}
        }
    }

    fn e_answer(&mut self, p: PairNet) {
        let now = self.mono_ms();
        let Pairing::E(e) = &mut self.pair else { return };
        match p.what {
            What::Open => {
                if ok_body(&p.result).is_some() {
                    e.state = PairState::ShowingCode;
                    e.polling = true;
                    e.poll_at = now + POLL_MS;
                    let sheet = PairSheet {
                        address_display: e.address_display.clone(),
                        code_display: e.code.take().unwrap_or_default(),
                    };
                    if let Some(r) = e.start_reply.take() {
                        let _ = r.send(Ok(sheet));
                    }
                    self.mark_pairing();
                } else {
                    let code = code_of(&p.result);
                    if let Some(r) = e.start_reply.take() {
                        let _ = r.send(Err(err(code)));
                    }
                    let fx = e.m.on(ExistingEvent::Cancel, &mut OsEntropy);
                    self.e_effects(fx);
                    if let Pairing::E(e) = &mut self.pair {
                        e.state = PairState::Aborted { code };
                    }
                }
            }
            What::SlotB => {
                e.in_flight = false;
                if let Some(b) = ok_body(&p.result) {
                    let fx = e.m.on(ExistingEvent::SlotB { bytes: b.to_vec() }, &mut OsEntropy);
                    self.e_effects(fx);
                } else if is_empty_slot(&p.result) || matches!(p.result, Err(Fail::Unreachable)) {
                    e.poll_at = now + POLL_MS;
                } else {
                    let code = code_of(&p.result);
                    let fx = e.m.on(ExistingEvent::Cancel, &mut OsEntropy);
                    self.e_effects(fx);
                    if let Pairing::E(e) = &mut self.pair {
                        e.state = PairState::Aborted { code };
                    }
                }
            }
            What::Post(_)
                if ok_body(&p.result).is_none() && !ended(&e.state) => {
                    let code = code_of(&p.result);
                    let fx = e.m.on(ExistingEvent::Cancel, &mut OsEntropy);
                    self.e_effects(fx);
                    if let Pairing::E(e) = &mut self.pair {
                        e.state = PairState::Aborted { code };
                    }
                }
            _ => {}
        }
        self.mark_pairing();
    }

    fn n_abort(&mut self, code: &'static str) {
        if let Pairing::N(n) = &mut self.pair
            && !ended(&n.state) {
                let fx = n.m.on(NewEvent::Cancel, &mut OsEntropy);
                self.n_effects(fx);
            }
        if let Pairing::N(n) = &mut self.pair {
            n.state = PairState::Aborted { code };
        }
        self.mark_pairing();
    }

    fn n_answer(&mut self, p: PairNet) {
        let now = self.mono_ms();
        let Pairing::N(n) = &mut self.pair else { return };
        let attempt = n.attempt;
        match p.what {
            What::Slot(label) => {
                n.in_flight = false;
                if n.fetch != Some(label) {
                    return;
                }
                if let Some(b) = ok_body(&p.result) {
                    n.fetch = None;
                    let fx = n.m.on(NewEvent::Slot { bytes: b.to_vec() }, &mut OsEntropy);
                    self.n_effects(fx);
                } else if is_empty_slot(&p.result) || matches!(p.result, Err(Fail::Unreachable)) {
                    n.poll_at = now + POLL_MS;
                } else {
                    let code = code_of(&p.result);
                    self.n_abort(code);
                }
            }
            What::Post(_) => {
                if ok_body(&p.result).is_none() {
                    let code = code_of(&p.result);
                    self.n_abort(code);
                }
            }
            What::Meta => {
                let ok = ok_body(&p.result)
                    .and_then(crate::net::parse_blob_meta)
                    .is_some_and(|m| m.id == n.blob && m.parts == n.parts_n && m.complete);
                if !ok {
                    return self.n_abort(if p.result.is_ok() { "seal/bundle_invalid" } else { code_of(&p.result) });
                }
                if n.parts_n == 0 {
                    return self.n_abort("seal/bundle_invalid");
                }
                let req = n_get(n, format!("/blobs/{}/0", hex(&n.blob)), wire::PART_MAX);
                self.pair_send(attempt, What::Part(0), req);
            }
            What::Part(i) => {
                let Some(b) = ok_body(&p.result) else { return self.n_abort(code_of(&p.result)) };
                if i as usize != n.parts.len() {
                    return;
                }
                n.parts.push(b.to_vec());
                if n.parts.len() < n.parts_n as usize {
                    let req = n_get(n, format!("/blobs/{}/{}", hex(&n.blob), i + 1), wire::PART_MAX);
                    self.pair_send(attempt, What::Part(i + 1), req);
                } else {
                    let parts = std::mem::take(&mut n.parts);
                    let fx = n.m.on(NewEvent::Snapshot { parts }, &mut OsEntropy);
                    self.n_effects(fx);
                }
            }
            What::Log => {
                let page = ok_body(&p.result).and_then(wire::parse_page);
                let Some(page) = page else {
                    return self.n_abort(if p.result.is_ok() { "seal/bundle_invalid" } else { code_of(&p.result) });
                };
                for e in page.entries {
                    n.log.push((e.seq, e.frame));
                }
                if page.more {
                    let after = n.log.last().map_or(0, |(s, _)| *s);
                    let req = n_get(n, format!("/log?after={after}&limit=256"), wire::PAGE_MAX);
                    self.pair_send(attempt, What::Log, req);
                } else {
                    let frames = std::mem::take(&mut n.log);
                    let fx = n.m.on(NewEvent::Entries { frames }, &mut OsEntropy);
                    self.n_effects(fx);
                }
            }
            What::Frame(t) => {
                let page = ok_body(&p.result).and_then(wire::parse_page);
                let Some(e) = page.and_then(|pg| pg.entries.into_iter().next()) else {
                    return self.n_abort("seal/bundle_invalid");
                };
                n.frames.push((e.seq, e.frame));
                let next = n.frames_wanted.iter().copied().find(|s| *s > t);
                match next {
                    Some(s) => {
                        let req = n_get(n, format!("/log?after={}&limit=1", s.saturating_sub(1)), wire::PAGE_MAX);
                        self.pair_send(attempt, What::Frame(s), req);
                    }
                    None => {
                        let frames = std::mem::take(&mut n.frames);
                        let fx = n.m.on(NewEvent::Frames { frames }, &mut OsEntropy);
                        self.n_effects(fx);
                    }
                }
            }
            What::Open | What::SlotB | What::Delete => {}
        }
        self.mark_pairing();
    }
}

/// N's read of a slot: the door and the ticket proof (relay.md §10.8).
fn n_slot_get(n: &NPair, label: u8) -> Req {
    let l = char::from(label.to_ascii_lowercase());
    Req {
        method: Method::Get,
        host: n.host.clone(),
        path: format!("/v1/b/{}/pair/{}/{l}", hex(&n.board), hex(&n.tid)),
        auth: Auth::Door(n.door),
        proof: n.ticket_key.clone(),
        body: None,
        limit: wire::SLOT_MAX,
    }
}

/// N's reads with the bundle's token (device.md §18.3).
fn n_get(n: &NPair, rest: String, limit: usize) -> Req {
    Req {
        method: Method::Get,
        host: n.host.clone(),
        path: format!("/v1/b/{}{rest}", hex(&n.board)),
        auth: Auth::Token(n.token.clone().unwrap_or_default()),
        proof: None,
        body: None,
        limit,
    }
}
