//! The transport (device.md §10): the thread `taskdeck-https`, which
//! performs relay requests one at a time in the order they were queued
//! (§4.2), and turns the engine's relay effects into exactly the requests of
//! §10.5 and their answers into exactly the events of §10.6 (DEV-NET-007,
//! DEV-NET-011). It does no file I/O: a token it learns is made durable by
//! the sync thread before it is used (DEV-LOGIN-001).
//!
//! Not built yet (stage plan, noted in `fit.md`): the socket thread, backoff
//! with jitter (a `Connect` after a failed one waits a fixed second instead),
//! the request budget, and admin operations other than `register` and
//! `revoke`.

pub(crate) mod http;
pub(crate) mod wire;

use std::collections::BTreeMap;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use taskdeck_core::command::LogCommand;
use taskdeck_seal::{AdminOp, DevId, DeviceKeys, FrameHash, OsEntropy, Signer};
use taskdeck_sync::{
    AdminOk, AdminRequest, BlobMeta, BlobOk, BlobRequest, Effect, Event, RelayRefusal, RelayTarget,
};
use zeroize::Zeroizing;

pub(crate) use http::{Auth, Fail, Method, Req, Resp};

use crate::store::RelayToken;
use crate::strict_json::{Obj, hex};

/// What the transport needs of the active identity.
pub(crate) struct NetConfig {
    pub(crate) board: [u8; 16],
    pub(crate) dev: [u8; 16],
    pub(crate) keys: DeviceKeys,
    pub(crate) tokens: Vec<RelayToken>,
}

/// A reply to a request that is not an engine effect.
pub(crate) type Reply = Box<dyn FnOnce(Result<Resp, Fail>) + Send>;

/// A job for the thread `taskdeck-https`.
pub(crate) enum Job {
    /// The active identity (after an open, a create or a join); `None` drops it.
    Configure(Option<Box<NetConfig>>),
    /// An engine effect of connection generation `conn`.
    Effect { conn: u64, effect: Effect },
    /// A request of a flow (create, pairing). `Auth::Stored` takes the token
    /// stored for the request's host, with one login on `token_stale`.
    Raw { req: Req, reply: Reply },
}

/// What the transport hands the sync thread.
pub(crate) enum Back {
    /// An event for the engine, of connection generation `conn`.
    Event { conn: u64, event: Box<Event<LogCommand>> },
    /// A token to make durable in `device.tdk` before it is used; the
    /// transport waits for `ack` (DEV-LOGIN-001).
    StoreToken { token: RelayToken, ack: Sender<bool> },
    /// The token of a `register` answer, for the pairing machine (§18.3).
    RegisterToken { dev: [u8; 16], token: Zeroizing<String> },
}

/// The sink the transport sends to; `false` when the sync thread is gone.
pub(crate) type Sink = Box<dyn Fn(Back) -> bool + Send>;

/// The device clock, for "next 00:00 UTC" (device.md §10.6).
pub(crate) type WallClock = Box<dyn Fn() -> i64 + Send>;

/// The thread's body: performs jobs until the channel closes.
pub(crate) fn run(rx: Receiver<Job>, sink: Sink, wall: WallClock) {
    let mut w = Worker {
        client: http::Client::new(),
        cfg: None,
        tokens: BTreeMap::new(),
        sink,
        wall,
        dead_gen: None,
        connect_failed: false,
        target: None,
    };
    while let Ok(job) = rx.recv() {
        match job {
            Job::Configure(cfg) => {
                w.tokens = cfg.as_ref().map(|c| c.tokens.iter().map(|t| (t.host.clone(), t.clone())).collect()).unwrap_or_default();
                w.cfg = cfg;
                w.dead_gen = None;
            }
            Job::Effect { conn, effect } => {
                if w.dead_gen == Some(conn) {
                    // The session of this generation ended unreachable; its
                    // later requests are not sent (their answers would be
                    // discarded, device.md §10.5 `Disconnect`).
                    continue;
                }
                w.effect(conn, effect);
            }
            Job::Raw { req, reply } => {
                let r = w.raw(req);
                reply(r);
            }
        }
    }
}

/// The outcome of one exchange as §10.6 classifies it before its code.
enum Outcome {
    /// A relay answer with a 2xx status, or a refusal body the caller reads.
    Answer(Resp),
    /// A JSON refusal of the relay (`X-TD-Relay` present).
    Refused(RelayRefusal),
    /// Delivered as `Disconnected { unreachable, paused_until_ms }`.
    Disconnect { unreachable: bool, paused_until_ms: Option<i64> },
    /// `507 relay/storage_refused`: `RelayRefused` with `until` the next 00:00 UTC.
    Paused(RelayRefusal),
    /// The body failed its layout (DEV-NET-009).
    Bad,
    /// A host this build may not reach.
    Insecure,
}

const BAD_ANSWER: &str = "device/bad_answer";
const INSECURE: &str = "device/insecure_url";
const TOKEN_STALE: &str = "relay/token_stale";
const NOT_BUILT: &str = "device/not_built";

fn refusal(status: u16, code: &str) -> RelayRefusal {
    RelayRefusal { status, code: code.to_string(), until: None, head: None, index: None }
}

fn next_midnight(wall_ms: i64) -> i64 {
    (wall_ms.div_euclid(86_400_000) + 1) * 86_400_000
}

struct Worker {
    client: http::Client,
    cfg: Option<Box<NetConfig>>,
    tokens: BTreeMap<String, RelayToken>,
    sink: Sink,
    wall: WallClock,
    dead_gen: Option<u64>,
    connect_failed: bool,
    target: Option<RelayTarget>,
}

impl Worker {
    fn send(&self, conn: u64, event: Event<LogCommand>) {
        let _ = (self.sink)(Back::Event { conn, event: Box::new(event) });
    }

    fn board_path(&self) -> Option<String> {
        self.cfg.as_ref().map(|c| format!("/v1/b/{}", hex(&c.board)))
    }

    /// One exchange, classified by relay.md §14.5 first (DEV-NET-011).
    fn exchange(&self, req: &Req) -> Outcome {
        let resp = match self.client.perform(req) {
            Ok(r) => r,
            Err(Fail::Unreachable) => return Outcome::Disconnect { unreachable: true, paused_until_ms: None },
            Err(Fail::Insecure) => return Outcome::Insecure,
        };
        if !resp.relay {
            if resp.status == 429 {
                return Outcome::Disconnect { unreachable: false, paused_until_ms: Some(next_midnight((self.wall)())) };
            }
            return Outcome::Disconnect { unreachable: true, paused_until_ms: None };
        }
        if (200..300).contains(&resp.status) || !resp.json {
            return Outcome::Answer(resp);
        }
        let Some(mut r) = wire::parse_refusal(resp.status, &resp.body) else { return Outcome::Bad };
        match (r.status, r.code.as_str()) {
            (503, "relay/object_unavailable" | "relay/misconfigured") | (408, "relay/slow") => {
                Outcome::Disconnect { unreachable: true, paused_until_ms: None }
            }
            (507, _) => {
                r.until = Some(next_midnight((self.wall)()));
                Outcome::Paused(r)
            }
            _ => Outcome::Refused(r),
        }
    }

    /// An exchange with the stored token of `host`: a `401 relay/token_stale`
    /// logs in and sends the request once more (§10.3, DEV-LOGIN-002).
    fn with_token(&mut self, host: &str, build: &dyn Fn(Auth) -> Req) -> Result<Outcome, Outcome> {
        let Some(t) = self.tokens.get(host) else {
            return Err(Outcome::Refused(refusal(401, "relay/unauthorized")));
        };
        let out = self.exchange(&build(Auth::Token(t.token.clone())));
        match &out {
            Outcome::Refused(r) if r.code == TOKEN_STALE => {
                let door = None;
                match self.login(host, door) {
                    Ok(()) => {}
                    Err(o) => return Err(o),
                }
                let t = self.tokens.get(host).map(|t| t.token.clone()).unwrap_or_default();
                Ok(self.exchange(&build(Auth::Token(t))))
            }
            _ => Ok(out),
        }
    }

    /// Login (device.md §10.3, relay.md §10.3): `login-nonce` with the stored
    /// token or the door, `sign_login` with the origin of this device's own
    /// target, `login`; a first `no_nonce` or `bad_gen` restarts it once
    /// (DEV-LOGIN-003). The new token is durable before this returns
    /// (DEV-LOGIN-001). `MissedRoots` is not sent here: no reinstatement is
    /// built (the caller ignores `wraps`).
    fn login(&mut self, host: &str, door: Option<[u8; 5]>) -> Result<(), Outcome> {
        let Some(cfg) = self.cfg.as_ref() else { return Err(Outcome::Bad) };
        let (board, dev) = (cfg.board, cfg.dev);
        let base = format!("/v1/b/{}", hex(&board));
        let origin = taskdeck_seal::relay_origin(host).map_err(|_| Outcome::Insecure)?;
        for attempt in 0..2 {
            let auth = || match (self.tokens.get(host), door) {
                (Some(t), _) => Some(Auth::Token(t.token.clone())),
                (None, Some(d)) => Some(Auth::Door(d)),
                (None, None) => None,
            };
            let Some(a) = auth() else { return Err(Outcome::Refused(refusal(401, "relay/unauthorized"))) };
            let req = Req {
                method: Method::Post,
                host: host.to_string(),
                path: format!("{base}/login/nonce"),
                auth: a,
                proof: None,
                body: Some(dev.to_vec()),
                limit: wire::JSON_MAX,
            };
            let resp = match self.exchange(&req) {
                Outcome::Answer(r) if r.status == 200 => r,
                Outcome::Answer(_) => return Err(Outcome::Bad),
                other => return Err(other),
            };
            let (nonce, generation) = {
                let mut o = wire::json_obj(&resp.body).ok_or(Outcome::Bad)?;
                let nonce: [u8; 32] = o.hex("nonce").ok_or(Outcome::Bad)?;
                let generation = u32::try_from(o.u53("gen").ok_or(Outcome::Bad)?).map_err(|_| Outcome::Bad)?;
                o.u53("expires").ok_or(Outcome::Bad)?;
                o.finish().ok_or(Outcome::Bad)?;
                (nonce, generation)
            };
            let Some(cfg) = self.cfg.as_ref() else { return Err(Outcome::Bad) };
            let (client_nonce, sig) = taskdeck_seal::sign_login(
                &cfg.keys,
                &origin,
                &taskdeck_seal::BoardId(board),
                generation,
                &nonce,
                &mut OsEntropy,
            )
            .map_err(|_| Outcome::Bad)?;
            let mut body = Vec::with_capacity(148);
            body.extend_from_slice(&dev);
            body.extend_from_slice(&generation.to_be_bytes());
            body.extend_from_slice(&nonce);
            body.extend_from_slice(&client_nonce);
            body.extend_from_slice(&sig);
            let Some(a) = auth() else { return Err(Outcome::Bad) };
            let req = Req {
                method: Method::Post,
                host: host.to_string(),
                path: format!("{base}/login"),
                auth: a,
                proof: None,
                body: Some(body),
                limit: wire::JSON_MAX,
            };
            match self.exchange(&req) {
                Outcome::Answer(r) if r.status == 200 => {
                    let mut o = wire::json_obj(&r.body).ok_or(Outcome::Bad)?;
                    let token = Zeroizing::new(o.string("token").ok_or(Outcome::Bad)?);
                    let gen2 = u32::try_from(o.u53("gen").ok_or(Outcome::Bad)?).map_err(|_| Outcome::Bad)?;
                    let push = o.boolean("push").ok_or(Outcome::Bad)?;
                    // `wraps` is parsed by presence only: no reinstatement yet.
                    o.take("wraps");
                    o.finish().ok_or(Outcome::Bad)?;
                    if !wire::is_token(&token) || gen2 != generation {
                        return Err(Outcome::Bad);
                    }
                    let t = RelayToken { host: host.to_string(), generation: gen2, push: Some(push), token };
                    return self.store_token(t);
                }
                Outcome::Refused(r) if attempt == 0 && matches!(r.code.as_str(), "relay/no_nonce" | "relay/bad_gen") => {
                    continue;
                }
                Outcome::Answer(_) => return Err(Outcome::Bad),
                other => return Err(other),
            }
        }
        Err(Outcome::Bad)
    }

    /// Hands the token to the sync thread and waits until it is durable.
    fn store_token(&mut self, t: RelayToken) -> Result<(), Outcome> {
        let (ack, wait) = std::sync::mpsc::channel();
        if !(self.sink)(Back::StoreToken { token: t.clone(), ack }) {
            return Err(Outcome::Bad);
        }
        match wait.recv_timeout(Duration::from_secs(30)) {
            Ok(true) => {
                self.tokens.insert(t.host.clone(), t);
                Ok(())
            }
            _ => Err(Outcome::Bad),
        }
    }

    // ------------------------------------------------------------ effects (§10.5)

    fn effect(&mut self, conn: u64, effect: Effect) {
        match effect {
            Effect::Connect { relay } => self.connect(conn, &relay),
            Effect::Disconnect => {}
            Effect::Hello => self.hello(conn),
            Effect::Read { req, after, limit } => self.read(conn, req, after, limit),
            Effect::Append { req, frames } => self.append(conn, req, frames),
            Effect::Admin { req, op } => self.admin(conn, req, op),
            Effect::Blob { req, op } => self.blob(conn, req, op),
            _ => {}
        }
    }

    /// The host of the current relay target: the last `Connect`'s.
    fn host(&self) -> Option<String> {
        self.target.as_ref().map(|t| t.host.clone())
    }

    /// Delivers an outcome that is not an answer for a request of `req`.
    fn fail(&mut self, conn: u64, req: Option<u64>, o: Outcome) {
        match o {
            Outcome::Disconnect { unreachable, paused_until_ms } => {
                self.dead_gen = Some(conn);
                self.send(conn, Event::Disconnected { unreachable, paused_until_ms });
            }
            Outcome::Refused(r) | Outcome::Paused(r) => self.send(conn, Event::RelayRefused { req, refusal: r }),
            Outcome::Bad | Outcome::Answer(_) => {
                self.send(conn, Event::RelayRefused { req, refusal: refusal(0, BAD_ANSWER) })
            }
            Outcome::Insecure => self.send(conn, Event::RelayRefused { req, refusal: refusal(0, INSECURE) }),
        }
    }

    /// `Connect` (§10.5): the fixed retry delay after a failed one, login
    /// when no token is stored for the host (with the target's door), `caps`
    /// when the token has no `push`; then `Connected { push: false }`: no
    /// socket is built yet, so every relay is polled over HTTPS.
    fn connect(&mut self, conn: u64, relay: &RelayTarget) {
        self.target = Some(relay.clone());
        if self.connect_failed {
            std::thread::sleep(Duration::from_millis(1_000));
        }
        let host = relay.host.clone();
        let end = |w: &mut Worker, o: Outcome| {
            w.connect_failed = true;
            match o {
                Outcome::Disconnect { unreachable, paused_until_ms } => {
                    w.dead_gen = Some(conn);
                    w.send(conn, Event::Disconnected { unreachable, paused_until_ms });
                }
                other => {
                    let r = match other {
                        Outcome::Refused(r) | Outcome::Paused(r) => r,
                        Outcome::Insecure => refusal(0, INSECURE),
                        _ => refusal(0, BAD_ANSWER),
                    };
                    w.send(conn, Event::RelayRefused { req: None, refusal: r });
                    w.dead_gen = Some(conn);
                    w.send(conn, Event::Disconnected { unreachable: false, paused_until_ms: None });
                }
            }
        };
        if !self.tokens.contains_key(&host)
            && let Err(o) = self.login(&host, Some(relay.door)) {
                return end(self, o);
            }
        let needs_caps = self.tokens.get(&host).is_some_and(|t| t.push.is_none());
        if needs_caps {
            let Some(base) = self.board_path() else { return end(self, Outcome::Bad) };
            let h = host.clone();
            let build = move |a: Auth| Req {
                method: Method::Get,
                host: h.clone(),
                path: format!("{base}/caps"),
                auth: a,
                proof: None,
                body: None,
                limit: wire::JSON_MAX,
            };
            let push = match self.with_token(&host, &build) {
                Ok(Outcome::Answer(r)) if r.status == 200 => parse_caps(&r.body),
                Ok(Outcome::Answer(_)) => None,
                Ok(o) | Err(o) => return end(self, o),
            };
            let Some(push) = push else { return end(self, Outcome::Bad) };
            let Some(mut t) = self.tokens.get(&host).cloned() else { return end(self, Outcome::Bad) };
            t.push = Some(push);
            if let Err(o) = self.store_token(t) {
                return end(self, o);
            }
        }
        self.connect_failed = false;
        self.send(conn, Event::Connected { push: false });
    }

    fn get_with_token(&mut self, path: String, limit: usize) -> Result<Outcome, Outcome> {
        let Some(host) = self.host() else { return Err(Outcome::Bad) };
        let h = host.clone();
        let build = move |a: Auth| Req { method: Method::Get, host: h.clone(), path: path.clone(), auth: a, proof: None, body: None, limit };
        self.with_token(&host, &build)
    }

    fn post_with_token(&mut self, method: Method, path: String, body: Vec<u8>, limit: usize) -> Result<Outcome, Outcome> {
        let Some(host) = self.host() else { return Err(Outcome::Bad) };
        let h = host.clone();
        let build = move |a: Auth| Req {
            method,
            host: h.clone(),
            path: path.clone(),
            auth: a,
            proof: None,
            body: Some(body.clone()),
            limit,
        };
        self.with_token(&host, &build)
    }

    fn hello(&mut self, conn: u64) {
        let Some(base) = self.board_path() else { return };
        match self.get_with_token(format!("{base}/hello"), wire::JSON_MAX) {
            Ok(Outcome::Answer(r)) if r.status == 200 && r.json => match wire::parse_hello(&r.body) {
                Some(w) => self.send(conn, Event::Welcome(w)),
                None => self.fail(conn, None, Outcome::Bad),
            },
            Ok(Outcome::Answer(_)) => self.fail(conn, None, Outcome::Bad),
            Ok(o) | Err(o) => self.fail(conn, None, o),
        }
    }

    fn read(&mut self, conn: u64, req: u64, after: u64, limit: u16) {
        let Some(base) = self.board_path() else { return };
        match self.get_with_token(format!("{base}/log?after={after}&limit={limit}"), wire::PAGE_MAX) {
            Ok(Outcome::Answer(r)) if r.status == 200 && !r.json => match wire::parse_page(&r.body) {
                Some(page) => self.send(conn, Event::Page { req: Some(req), page }),
                None => self.fail(conn, Some(req), Outcome::Bad),
            },
            Ok(Outcome::Answer(_)) => self.fail(conn, Some(req), Outcome::Bad),
            Ok(o) | Err(o) => self.fail(conn, Some(req), o),
        }
    }

    fn append(&mut self, conn: u64, req: u64, frames: Vec<Vec<u8>>) {
        let Some(base) = self.board_path() else { return };
        let Some(body) = wire::batch(&frames) else { return self.fail(conn, Some(req), Outcome::Bad) };
        match self.post_with_token(Method::Post, format!("{base}/append"), body, wire::RESULTS_MAX) {
            Ok(Outcome::Answer(r)) if !r.json => match wire::parse_results(&r.body, frames.len()) {
                Some(results) => self.send(conn, Event::AppendResults { req, results }),
                None => self.fail(conn, Some(req), Outcome::Bad),
            },
            Ok(Outcome::Answer(_)) => self.fail(conn, Some(req), Outcome::Bad),
            Ok(o) | Err(o) => self.fail(conn, Some(req), o),
        }
    }

    /// An admin request (§10.5): `admin-nonce` with this device as signer,
    /// then the envelope of relay.md §12.1 signed by this device.
    fn admin(&mut self, conn: u64, req: u64, op: AdminRequest) {
        let deliver = |w: &Worker, result: Result<AdminOk, RelayRefusal>| {
            w.send(conn, Event::AdminResult { req, result });
        };
        let (aop, extra, register_dev) = match &op {
            AdminRequest::Register { frame, dev, sign_pk } => (
                AdminOp::Register { dev: DevId(*dev), sign_pk: *sign_pk, frame_hash: taskdeck_seal::frame_hash(frame) },
                frame.clone(),
                Some(*dev),
            ),
            AdminRequest::RegisterSeq { seq, dev, sign_pk, frame_hash } => (
                AdminOp::Register { dev: DevId(*dev), sign_pk: *sign_pk, frame_hash: FrameHash(*frame_hash) },
                seq.to_be_bytes().to_vec(),
                Some(*dev),
            ),
            AdminRequest::Revoke { dev } => (AdminOp::Revoke(DevId(*dev)), Vec::new(), None),
            _ => return deliver(self, Err(refusal(0, NOT_BUILT))),
        };
        let Some(base) = self.board_path() else { return };
        let Some(cfg) = self.cfg.as_ref() else { return };
        let signer = cfg.dev;
        // The nonce.
        let nonce = match self.post_with_token(Method::Post, format!("{base}/admin/nonce"), signer.to_vec(), wire::JSON_MAX) {
            Ok(Outcome::Answer(r)) if r.status == 200 => {
                let parsed = wire::json_obj(&r.body).and_then(|mut o| {
                    let n: [u8; 32] = o.hex("nonce")?;
                    o.u53("expires")?;
                    o.finish()?;
                    Some(n)
                });
                match parsed {
                    Some(n) => n,
                    None => return deliver(self, Err(refusal(0, BAD_ANSWER))),
                }
            }
            Ok(Outcome::Refused(r)) | Err(Outcome::Refused(r)) => return deliver(self, Err(r)),
            Ok(o) | Err(o) => return self.fail(conn, Some(req), o),
        };
        let Some(cfg) = self.cfg.as_ref() else { return };
        let sig = match taskdeck_seal::sign_admin(&Signer::Device(&cfg.keys), &aop, &taskdeck_seal::BoardId(cfg.board), &nonce) {
            Ok(s) => s,
            Err(e) => return deliver(self, Err(refusal(0, e.code()))),
        };
        let body = wire::admin_envelope(&signer, &nonce, &sig, &aop.args(), &extra);
        match self.post_with_token(Method::Post, format!("{base}/admin/{}", aop.name()), body, wire::JSON_MAX) {
            Ok(Outcome::Answer(r)) if r.status == 200 => {
                let ok = match &op {
                    AdminRequest::Register { .. } | AdminRequest::RegisterSeq { .. } => {
                        wire::json_obj(&r.body).and_then(|mut o| {
                            let seq = o.u53("seq")?;
                            let chain: [u8; 32] = o.hex("chain")?;
                            let token = Zeroizing::new(o.string("token")?);
                            o.finish()?;
                            wire::is_token(&token).then_some((seq, chain, token))
                        })
                        .map(|(seq, chain, token)| {
                            if let Some(dev) = register_dev {
                                let _ = (self.sink)(Back::RegisterToken { dev, token });
                            }
                            AdminOk { seq: Some(seq), chain: Some(chain), token: true, ..admin_ok() }
                        })
                    }
                    _ => wire::json_obj(&r.body).and_then(Obj::finish).map(|()| admin_ok()),
                };
                match ok {
                    Some(ok) => deliver(self, Ok(ok)),
                    None => deliver(self, Err(refusal(0, BAD_ANSWER))),
                }
            }
            Ok(Outcome::Answer(_)) => deliver(self, Err(refusal(0, BAD_ANSWER))),
            Ok(Outcome::Refused(r) | Outcome::Paused(r)) | Err(Outcome::Refused(r) | Outcome::Paused(r)) => {
                deliver(self, Err(r))
            }
            Ok(o) | Err(o) => self.fail(conn, Some(req), o),
        }
    }

    /// A blob request (§10.5).
    fn blob(&mut self, conn: u64, req: u64, op: BlobRequest) {
        let deliver = |w: &Worker, result: Result<BlobOk, RelayRefusal>| {
            w.send(conn, Event::BlobResult { req, result });
        };
        let Some(base) = self.board_path() else { return };
        macro_rules! answer {
            ($e:expr) => {
                match $e {
                    Ok(Outcome::Answer(r)) if r.status == 200 => r,
                    Ok(Outcome::Answer(_)) => return deliver(self, Err(refusal(0, BAD_ANSWER))),
                    Ok(Outcome::Refused(r) | Outcome::Paused(r)) | Err(Outcome::Refused(r) | Outcome::Paused(r)) => {
                        return deliver(self, Err(r))
                    }
                    Ok(o) | Err(o) => return self.fail(conn, Some(req), o),
                }
            };
        }
        match op {
            BlobRequest::Upload { purpose, seq, blob } => {
                let mut body = Vec::with_capacity(65);
                body.extend_from_slice(&blob.id);
                body.push(purpose);
                body.extend_from_slice(&seq.to_be_bytes());
                body.extend_from_slice(&(blob.bytes.len() as u64).to_be_bytes());
                body.extend_from_slice(&blob.sha256);
                let r = answer!(self.post_with_token(Method::Post, format!("{base}/blobs"), body, wire::JSON_MAX));
                let parts = wire::json_obj(&r.body).and_then(|mut o| {
                    let _id: [u8; 16] = o.hex("id")?;
                    let p = o.u53("parts")?;
                    o.finish()?;
                    Some(p)
                });
                let Some(parts) = parts else { return deliver(self, Err(refusal(0, BAD_ANSWER))) };
                let chunks: Vec<&[u8]> = blob.bytes.chunks(wire::PART_MAX).collect();
                if parts != chunks.len() as u64 {
                    return deliver(self, Err(refusal(0, BAD_ANSWER)));
                }
                for (i, c) in chunks.iter().enumerate() {
                    let path = format!("{base}/blobs/{}/{i}", hex(&blob.id));
                    let r = answer!(self.post_with_token(Method::Put, path, c.to_vec(), wire::JSON_MAX));
                    let ok = wire::json_obj(&r.body).and_then(|mut o| {
                        o.u53("received")?;
                        o.boolean("complete")?;
                        o.finish()
                    });
                    if ok.is_none() {
                        return deliver(self, Err(refusal(0, BAD_ANSWER)));
                    }
                }
                deliver(self, Ok(BlobOk::Uploaded));
            }
            BlobRequest::Fetch { id, parts } => {
                let r = answer!(self.get_with_token(format!("{base}/blobs/{}", hex(&id)), wire::JSON_MAX));
                let Some(meta) = parse_blob_meta(&r.body) else { return deliver(self, Err(refusal(0, BAD_ANSWER))) };
                if meta.parts != parts || !meta.complete {
                    return deliver(self, Err(refusal(0, BAD_ANSWER)));
                }
                let mut out = Vec::with_capacity(parts as usize);
                for i in 0..parts {
                    let r = answer!(self.get_with_token(format!("{base}/blobs/{}/{i}", hex(&id)), wire::PART_MAX));
                    out.push(r.body);
                }
                deliver(self, Ok(BlobOk::Parts(out)));
            }
            BlobRequest::Meta { id } => {
                let r = answer!(self.get_with_token(format!("{base}/blobs/{}", hex(&id)), wire::JSON_MAX));
                match parse_blob_meta(&r.body) {
                    Some(m) => deliver(self, Ok(BlobOk::Meta(m))),
                    None => deliver(self, Err(refusal(0, BAD_ANSWER))),
                }
            }
            BlobRequest::List => {
                let r = answer!(self.get_with_token(format!("{base}/blobs"), wire::JSON_MAX));
                let list = wire::json_obj(&r.body).and_then(|mut o| {
                    let v = o.take("blobs")?;
                    o.finish()?;
                    let arr = v.as_array()?;
                    arr.iter()
                        .map(|m| parse_blob_meta(serde_json::to_string(m).ok()?.as_bytes()))
                        .collect::<Option<Vec<_>>>()
                });
                match list {
                    Some(l) => deliver(self, Ok(BlobOk::List(l))),
                    None => deliver(self, Err(refusal(0, BAD_ANSWER))),
                }
            }
        }
    }

    // ------------------------------------------------------------ flows

    /// A request of a flow; `Auth::Stored` takes the stored token of the
    /// request's host, logging in once on `token_stale`.
    fn raw(&mut self, req: Req) -> Result<Resp, Fail> {
        let out = if matches!(req.auth, Auth::Stored) {
            let Req { method, host, path, proof, body, limit, .. } = req;
            let h = host.clone();
            let build = move |a: Auth| Req {
                method,
                host: h.clone(),
                path: path.clone(),
                auth: a,
                proof: proof.clone(),
                body: body.clone(),
                limit,
            };
            match self.with_token(&host, &build) {
                Ok(o) | Err(o) => o,
            }
        } else {
            {
                let r = self.client.perform(&req)?;
                return Ok(r)
            }
        };
        match out {
            Outcome::Answer(r) => Ok(r),
            Outcome::Refused(r) | Outcome::Paused(r) => {
                let body = serde_json::json!({ "error": r.code }).to_string().into_bytes();
                Ok(Resp { status: r.status, relay: true, json: true, body })
            }
            Outcome::Insecure => Err(Fail::Insecure),
            Outcome::Disconnect { .. } | Outcome::Bad => Err(Fail::Unreachable),
        }
    }
}

fn admin_ok() -> AdminOk {
    AdminOk {
        seq: None,
        chain: None,
        token: false,
        deleted: None,
        floor: None,
        more: None,
        purge_floor: None,
        done: None,
        head: None,
        door: None,
    }
}

/// `caps` (relay.md §10.9): `push`, the rest checked.
fn parse_caps(body: &[u8]) -> Option<bool> {
    let mut o = wire::json_obj(body)?;
    (o.u53("v")? == 1).then_some(())?;
    let push = o.boolean("push")?;
    for k in ["max_batch", "max_frame", "page_entries", "page_bytes", "part_bytes"] {
        o.u53(k)?;
    }
    o.finish()?;
    Some(push)
}

/// `blob-meta` (relay.md §10.7).
pub(crate) fn parse_blob_meta(body: &[u8]) -> Option<BlobMeta> {
    let mut o = wire::json_obj(body)?;
    let m = BlobMeta {
        id: o.hex("id")?,
        purpose: u8::try_from(o.u53("purpose")?).ok().filter(|p| (1..=4).contains(p))?,
        seq: o.u53("seq")?,
        author: o.hex("author")?,
        size: o.u53("size")?,
        parts: u32::try_from(o.u53("parts")?).ok()?,
        sha256: o.hex("sha256")?,
        complete: {
            o.u53("received")?;
            o.boolean("complete")?
        },
    };
    o.finish()?;
    Some(m)
}
