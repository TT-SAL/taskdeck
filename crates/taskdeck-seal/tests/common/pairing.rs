//! Pairing over the public API (task S6) for the `pairing_mitm`, `tamper`,
//! `contexts`, `parsers` and `vectors` targets: E and N driven through a
//! relay simulated here (the slots pass through the test, which may replace
//! them), against a `Sim` board; and the primitives of §17 computed here
//! with the pinned crates, for an attacker that knows the code (a relay in
//! the middle), or a test that plays E or N itself. Include after
//! `board.rs`, `backup.rs` and `blob.rs`:
//! `#[path = "../common/pairing.rs"] mod pairing;`.
#![allow(dead_code)]

use chacha20poly1305::aead::{Aead as _, KeyInit as _, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::hmac::{Hmac, Mac as _};
use sha2::Sha256;
use taskdeck_seal::pair::*;
use taskdeck_seal::*;

use crate::backup::sha;
use crate::blob::*;
use crate::board::*;

pub const ORIGIN: &str = "https://relay.example.org";
pub const HOST: &str = "relay.example.org";
pub const DOOR: [u8; 5] = [0xd0, 0x0d, 1, 2, 3];
pub const NAME: &str = "Phone";
pub const TOKEN: &str = "n-token-1";
pub const PT0: u64 = 1_000_000;
pub const BASE: [u8; 32] = x25519_dalek::X25519_BASEPOINT_BYTES;
pub const A: u8 = 0x41;
pub const B: u8 = 0x42;
pub const C: u8 = 0x43;
pub const D: u8 = 0x44;
pub const ROLE_E: u8 = 0x45;
pub const ROLE_N: u8 = 0x4E;

/// `ctx(name)`.
pub fn cx(name: &str) -> Vec<u8> {
    [b"taskdeck/v1/", name.as_bytes(), b"\0"].concat()
}

pub fn hk(salt: &[u8], ikm: &[u8], info: &[u8], n: usize) -> Vec<u8> {
    let mut out = vec![0u8; n];
    hkdf::Hkdf::<Sha256>::new(Some(salt), ikm).expand(info, &mut out).unwrap();
    out
}

pub fn mac(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut m = <Hmac<Sha256> as hkdf::hmac::KeyInit>::new_from_slice(key).unwrap();
    m.update(data);
    m.finalize().into_bytes().into()
}

/// RFC 7748 X25519 of a raw scalar and a u-coordinate.
pub fn x(k: [u8; 32], u: [u8; 32]) -> [u8; 32] {
    x25519_dalek::x25519(k, u)
}

/// What a code gives whoever knows it (§17.3).
#[derive(Clone)]
pub struct Code {
    pub s: [u8; 10],
    pub ticket_key: [u8; 32],
    pub tid: [u8; 16],
    pub mac_key: [u8; 32],
}

impl Code {
    pub fn of(text: &str, board: &BoardId) -> Code {
        let plain: String = text.chars().filter(|c| *c != '-').collect();
        let s: [u8; 10] = base32::decode_typed(&plain[..16], 10).unwrap().try_into().unwrap();
        Code::from_secret(s, board)
    }
    pub fn from_secret(s: [u8; 10], board: &BoardId) -> Code {
        let ticket_key: [u8; 32] = hk(&board.0, &s, &cx("pair-ticket"), 32).try_into().unwrap();
        let tid: [u8; 16] = sha(&[&cx("pair-tid"), &ticket_key])[..16].try_into().unwrap();
        let mac_key: [u8; 32] = hk(&board.0, &s, &cx("pair-mac"), 32).try_into().unwrap();
        Code { s, ticket_key, tid, mac_key }
    }
    /// A slot of §17.4 under this code.
    pub fn slot(&self, label: u8, role: u8, body: &[u8]) -> Vec<u8> {
        let len = (body.len() as u16).to_be_bytes();
        let m = mac(&self.mac_key, &[&cx("pair")[..], &[label, role], &self.tid, &len, body].concat());
        [&[label, role][..], &len, body, &m].concat()
    }
    /// The slot with its MAC made again over its own label, role and body.
    pub fn remac(&self, slot: &[u8]) -> Vec<u8> {
        self.slot(slot[0], slot[1], body(slot))
    }
}

/// The body of a slot.
pub fn body(slot: &[u8]) -> &[u8] {
    &slot[4..slot.len() - 32]
}

pub fn commit(epk: &[u8; 32], r: &[u8; 32]) -> [u8; 32] {
    sha(&[&cx("pair-commit"), epk, r])
}

pub fn transcript(board: &BoardId, tid: &[u8; 16], dev_e: &DevId, a: &[u8], b: &[u8], c: &[u8]) -> [u8; 32] {
    sha(&[&cx("pair-transcript"), &(ORIGIN.len() as u16).to_be_bytes(), ORIGIN.as_bytes(), &board.0, tid, &dev_e.0, a, b, c])
}

pub fn sas(s: &[u8; 10], dh: &[u8; 32], t: &[u8; 32]) -> String {
    let okm = hk(s, dh, &[&cx("sas")[..], t].concat(), 8);
    format!("{:06}", u64::from_be_bytes(okm.try_into().unwrap()) % 1_000_000)
}

pub fn bundle_key(s: &[u8; 10], dh: &[u8; 32], t: &[u8; 32]) -> [u8; 32] {
    hk(s, dh, &[&cx("pair-bundle-key")[..], t].concat(), 32).try_into().unwrap()
}

pub fn bundle_ad(board: &BoardId, tid: &[u8; 16], t: &[u8; 32]) -> Vec<u8> {
    [&cx("pair-bundle")[..], &board.0, tid, t].concat()
}

/// `nonce ‖ XChaCha20-Poly1305(key, nonce, ad, pt)`.
pub fn aead_seal(key: &[u8; 32], nonce: [u8; 24], ad: &[u8], pt: &[u8]) -> Vec<u8> {
    let ct = XChaCha20Poly1305::new_from_slice(key).unwrap().encrypt(&XNonce::from(nonce), Payload { msg: pt, aad: ad }).unwrap();
    [&nonce[..], &ct].concat()
}

pub fn aead_open(key: &[u8; 32], ad: &[u8], body: &[u8]) -> Option<Vec<u8>> {
    let nonce: [u8; 24] = body.get(..24)?.try_into().ok()?;
    XChaCha20Poly1305::new_from_slice(key).unwrap().decrypt(&XNonce::from(nonce), Payload { msg: &body[24..], aad: ad }).ok()
}

/// Slot B's body (§17.4).
pub fn b_body(p: &DevicePublic, epk: &[u8; 32], rules: &[u32], name: &str) -> Vec<u8> {
    let mut v = [&p.dev.0[..], &p.sign_pk, &p.hpke_pk, epk].concat();
    v.push(rules.len() as u8);
    for r in rules {
        v.extend(r.to_be_bytes());
    }
    v.push(name.len() as u8);
    v.extend(name.as_bytes());
    v
}

/// The bundle JSON of §17.8 for the given fields, in canonical form
/// (`serde_json` sorts the keys of its maps and writes no whitespace).
#[allow(clippy::too_many_arguments)]
pub fn bundle_json(board: &BoardId, host: &str, door: &[u8; 5], roots: &[(u32, [u8; 32])], head: Head, s: &SnapAnnounce, added_seq: u64, token: &str) -> Vec<u8> {
    use crate::common::hex;
    let v = serde_json::json!({
        "board_id": hex(&board.0),
        "host": host,
        "door": hex(door),
        "roots": roots.iter().map(|(e, r)| serde_json::json!({"epoch": e, "root": hex(r)})).collect::<Vec<_>>(),
        "head": {"seq": head.seq, "chain": hex(&head.chain.0)},
        "snapshot": {"blob": hex(&s.blob.0), "sha256": hex(&s.sha256), "size": s.size, "parts": s.parts,
                     "inflated": s.inflated, "epoch": s.epoch, "hash": hex(&s.hash), "hash_version": s.hash_version},
        "added_seq": added_seq,
        "token": token,
    });
    serde_json::to_vec(&v).unwrap()
}

pub fn e_abort(fx: &[ExistingEffect]) -> Option<SealError> {
    fx.iter().find_map(|f| match f {
        ExistingEffect::Abort { code } => Some(*code),
        _ => None,
    })
}

pub fn n_abort(fx: &[NewEffect]) -> Option<SealError> {
    fx.iter().find_map(|f| match f {
        NewEffect::Abort { code } => Some(*code),
        _ => None,
    })
}

/// The name of an effect, for assertions.
pub fn n_name(f: &NewEffect) -> &'static str {
    match f {
        NewEffect::FetchSlot { .. } => "FetchSlot",
        NewEffect::AskAgain { .. } => "AskAgain",
        NewEffect::PostSlot { .. } => "PostSlot",
        NewEffect::AskSas => "AskSas",
        NewEffect::ShowAccepted => "ShowAccepted",
        NewEffect::FetchSnapshot { .. } => "FetchSnapshot",
        NewEffect::ReadLog { .. } => "ReadLog",
        NewEffect::FetchFrames { .. } => "FetchFrames",
        NewEffect::Joined { .. } => "Joined",
        NewEffect::Abort { .. } => "Abort",
    }
}

pub fn e_name(f: &ExistingEffect) -> &'static str {
    match f {
        ExistingEffect::RegisterTicket { .. } => "RegisterTicket",
        ExistingEffect::ShowCode { .. } => "ShowCode",
        ExistingEffect::PostSlot { .. } => "PostSlot",
        ExistingEffect::ShowSas { .. } => "ShowSas",
        ExistingEffect::NeedSnapshotAndRegister { .. } => "NeedSnapshotAndRegister",
        ExistingEffect::Done { .. } => "Done",
        ExistingEffect::DeleteTicket { .. } => "DeleteTicket",
        ExistingEffect::Abort { .. } => "Abort",
    }
}

/// A snapshot E's engine took at the board's head.
pub struct Snap {
    pub h: u64,
    pub chain: Chain,
    pub bytes: Vec<u8>,
    pub ann: SnapAnnounce,
    /// The current and previous roots at `h`.
    pub roots: Vec<(u32, [u8; 32])>,
}

/// One pairing: E (device `ei` of `sim`) and N, with what passed between them.
pub struct Run {
    pub sim: Sim,
    pub ei: usize,
    pub e: Existing,
    pub n: New,
    pub erng: Box<dyn Entropy>,
    pub nrng: Box<dyn Entropy>,
    pub code: String,
    pub k: Code,
    pub slot_a: Vec<u8>,
    pub slot_b: Vec<u8>,
    pub slot_c: Vec<u8>,
    pub slot_d: Vec<u8>,
    pub digits: String,
    pub device: Option<DeviceAdded>,
    pub snap: Option<Snap>,
    pub added_seq: u64,
    pub start_effects: Vec<&'static str>,
}

impl Run {
    /// E starts on `sim` as device `ei`, with entropy from the simulator.
    pub fn new(mut sim: Sim, ei: usize) -> Run {
        let erng = source(sim.rng.bytes(74 + 24));
        let nrng = source(sim.rng.bytes(112));
        Run::with(sim, ei, erng, nrng)
    }

    /// E starts on `sim` as device `ei` with the entropy sources given.
    pub fn with(sim: Sim, ei: usize, mut erng: Box<dyn Entropy>, nrng: Box<dyn Entropy>) -> Run {
        let own = sim.devs[ei].public().dev;
        let (e, fx) = Existing::start(&sim.m, sim.head(), ORIGIN, &own, PT0, &mut *erng);
        let start_effects = fx.iter().map(e_name).collect();
        let (ExistingEffect::RegisterTicket { tid, slot_a }, ExistingEffect::ShowCode { code }) = (&fx[0], &fx[1]) else {
            panic!("start: {start_effects:?}")
        };
        let k = Code::of(code, &sim.board);
        assert_eq!(*tid, k.tid);
        Run {
            ei,
            e,
            n: New::new(),
            erng,
            nrng,
            code: code.clone(),
            k,
            slot_a: slot_a.clone(),
            slot_b: vec![],
            slot_c: vec![],
            slot_d: vec![],
            digits: String::new(),
            device: None,
            snap: None,
            added_seq: 0,
            start_effects,
            sim,
        }
    }

    pub fn dev_e(&self) -> DevId {
        self.sim.devs[self.ei].public().dev
    }

    pub fn n_on(&mut self, ev: NewEvent) -> Vec<NewEffect> {
        self.n.on(ev, &mut *self.nrng)
    }

    pub fn e_on(&mut self, ev: ExistingEvent) -> Vec<ExistingEffect> {
        self.e.on(ev, &mut *self.erng)
    }

    pub fn code_event(&self) -> NewEvent {
        NewEvent::CodeEntered {
            code: self.code.clone(),
            host: HOST.into(),
            board_id: self.sim.board,
            door: DOOR,
            name: NAME.into(),
            now: PT0 + 1,
        }
    }

    /// N takes the code and slot A, and posts slot B.
    pub fn step_b(&mut self) -> &mut Self {
        let fx = self.n_on(self.code_event());
        assert_eq!(fx.iter().map(n_name).collect::<Vec<_>>(), ["FetchSlot"]);
        let fx = self.n_on(NewEvent::Slot { bytes: self.slot_a.clone() });
        let [NewEffect::PostSlot { slot }, NewEffect::FetchSlot { label: C, .. }] = &fx[..] else {
            panic!("slot A: {:?}", fx.iter().map(n_name).collect::<Vec<_>>())
        };
        self.slot_b = slot.clone();
        self
    }

    /// E takes slot B and posts slot C; its digits.
    pub fn step_c(&mut self) -> &mut Self {
        let fx = self.e_on(ExistingEvent::SlotB { bytes: self.slot_b.clone() });
        let [ExistingEffect::PostSlot { slot }, ExistingEffect::ShowSas { digits }] = &fx[..] else {
            panic!("slot B: {:?} {:?}", fx.iter().map(e_name).collect::<Vec<_>>(), e_abort(&fx))
        };
        self.slot_c = slot.clone();
        self.digits = digits.clone();
        self
    }

    /// N takes slot C and asks for the digits.
    pub fn step_sas(&mut self) -> &mut Self {
        let fx = self.n_on(NewEvent::Slot { bytes: self.slot_c.clone() });
        assert_eq!(fx.iter().map(n_name).collect::<Vec<_>>(), ["AskSas"], "{:?}", n_abort(&fx));
        self
    }

    /// The user types E's digits into N, then confirms on E.
    pub fn step_confirmed(&mut self) -> &mut Self {
        let fx = self.n_on(NewEvent::SasEntered { digits: self.digits.clone() });
        assert_eq!(fx.iter().map(n_name).collect::<Vec<_>>(), ["ShowAccepted", "FetchSlot"]);
        let fx = self.e_on(ExistingEvent::UserConfirmed);
        let [ExistingEffect::NeedSnapshotAndRegister { device }] = &fx[..] else { panic!("confirm") };
        self.device = Some(device.clone());
        self
    }

    /// E's engine takes a snapshot at the board's head (§20).
    pub fn take_snapshot(&mut self) -> &mut Self {
        let s = &mut self.sim;
        let body = snap_body(s);
        let inflated = body_bytes(&body).len() as u64;
        let e = s.m.epoch();
        let writer = s.devs[self.ei].public().dev;
        let sb = seal_snapshot(&body, &snap_key(s), &s.board, e, &writer, &mut *source(s.rng.bytes(40))).unwrap();
        let ann = SnapAnnounce {
            seq: s.seq(),
            hash: body.state_hash,
            hash_version: 1,
            blob: sb.id,
            sha256: sb.sha256,
            size: sb.bytes.len() as u64,
            parts: sb.parts,
            inflated,
            epoch: e,
        };
        let roots = s.roots.iter().filter(|(x, _)| **x + 1 >= e && **x <= e).map(|(x, r)| (*x, *r)).collect();
        self.snap = Some(Snap { h: s.seq(), chain: s.head().chain, bytes: sb.bytes, ann, roots });
        self
    }

    /// E's engine seals the `DeviceAdded` with `bootstrap_seq = h` and
    /// appends it; its receipt on the board.
    pub fn append_added(&mut self, edit: impl FnOnce(&mut DeviceAdded), sp: &Spec) -> Receipt {
        let h = self.snap.as_ref().unwrap().h;
        let mut item = self.device.clone().unwrap();
        item.bootstrap_seq = h;
        edit(&mut item);
        let based_on = Head { seq: h, chain: self.snap.as_ref().unwrap().chain };
        let sp = Spec { based_on: Some(sp.based_on.unwrap_or(based_on)), ..sp.clone() };
        let f = self.sim.ctl(Who::D(self.ei), vec![CtlItem::DeviceAdded(item)], &sp);
        let r = self.sim.push(f);
        self.added_seq = self.sim.seq();
        r
    }

    /// The `Registered` event from the snapshot and `added_seq`.
    pub fn registered(&self) -> ExistingEvent {
        let s = self.snap.as_ref().unwrap();
        ExistingEvent::Registered {
            h: s.h,
            chain: s.chain,
            snapshot: s.ann.clone(),
            added_seq: self.added_seq,
            token: TOKEN.into(),
            roots: s.roots.iter().map(|(e, r)| (*e, EpochRoot::from_bytes(*r))).collect(),
            host: HOST.into(),
            door: DOOR,
        }
    }

    /// E posts slot D for `ev`.
    pub fn post_d(&mut self, ev: ExistingEvent) -> &mut Self {
        let fx = self.e_on(ev);
        let [ExistingEffect::PostSlot { slot }, ExistingEffect::Done { dev }] = &fx[..] else {
            panic!("registered: {:?} {:?}", fx.iter().map(e_name).collect::<Vec<_>>(), e_abort(&fx))
        };
        assert_eq!(*dev, self.device.as_ref().unwrap().dev);
        self.slot_d = slot.clone();
        self
    }

    /// The usual registration: snapshot, `DeviceAdded`, slot D.
    pub fn step_d(&mut self) -> &mut Self {
        self.take_snapshot();
        let r = self.append_added(|_| {}, &Spec::default());
        assert!(matches!(r.class, Class::Accept), "{:?}", r.code);
        let ev = self.registered();
        self.post_d(ev)
    }

    /// N takes slot D; its effects.
    pub fn send_d(&mut self) -> Vec<NewEffect> {
        self.n_on(NewEvent::Slot { bytes: self.slot_d.clone() })
    }

    /// N takes the snapshot's parts; its effects.
    pub fn send_snapshot(&mut self, bytes: Option<Vec<u8>>) -> Vec<NewEffect> {
        let b = bytes.unwrap_or_else(|| self.snap.as_ref().unwrap().bytes.clone());
        let parts = split_parts(&b).into_iter().map(|p| p.to_vec()).collect();
        self.n_on(NewEvent::Snapshot { parts })
    }

    /// The board's entries from `h + 1` to its head.
    pub fn log(&self) -> Vec<(u64, Vec<u8>)> {
        let h = self.snap.as_ref().unwrap().h;
        (h + 1..=self.sim.seq()).map(|t| (t, self.sim.frames[t as usize - 1].clone())).collect()
    }

    /// N takes the log; its effects.
    pub fn send_log(&mut self) -> Vec<NewEffect> {
        let frames = self.log();
        self.n_on(NewEvent::Entries { frames })
    }

    /// Everything up to slot D posted.
    pub fn through_d(&mut self) -> &mut Self {
        self.step_b().step_c().step_sas().step_confirmed().step_d()
    }

    /// N takes slot D and the snapshot: it then asks for the log.
    pub fn step_joining(&mut self) -> &mut Self {
        let fx = self.send_d();
        assert_eq!(fx.iter().map(n_name).collect::<Vec<_>>(), ["FetchSnapshot"], "{:?}", n_abort(&fx));
        let fx = self.send_snapshot(None);
        assert_eq!(fx.iter().map(n_name).collect::<Vec<_>>(), ["ReadLog"], "{:?}", n_abort(&fx));
        self
    }
}

/// A test that plays E towards N (or a relay in the middle that knows the
/// code and replaces E): its own ephemeral key and `r`, fixed before it sees
/// N's slot B.
pub struct FakeE {
    pub k: Code,
    pub board: BoardId,
    pub dev_e: DevId,
    pub esk: [u8; 32],
    pub epk: [u8; 32],
    pub r: [u8; 32],
}

impl FakeE {
    pub fn new(k: &Code, board: BoardId, dev_e: DevId, esk: [u8; 32], r: [u8; 32]) -> FakeE {
        FakeE { k: k.clone(), board, dev_e, esk, epk: x(esk, BASE), r }
    }
    pub fn slot_a(&self) -> Vec<u8> {
        self.k.slot(A, ROLE_E, &[&self.dev_e.0[..], &commit(&self.epk, &self.r)].concat())
    }
    pub fn slot_c(&self) -> Vec<u8> {
        self.k.slot(C, ROLE_E, &[self.epk, self.r].concat())
    }
    /// `dh`, `T` and the digits for a slot B received, with slots A and C as posted.
    pub fn view(&self, a: &[u8], b: &[u8], c: &[u8]) -> ([u8; 32], [u8; 32], String) {
        let epk_n: [u8; 32] = body(b)[80..112].try_into().unwrap();
        let dh = x(self.esk, epk_n);
        let t = transcript(&self.board, &self.k.tid, &self.dev_e, a, b, c);
        (dh, t, sas(&self.k.s, &dh, &t))
    }
}

/// A test that plays N towards E (or a relay in the middle that replaces N):
/// its own long-term and ephemeral keys.
pub struct FakeN {
    pub k: Code,
    pub keys: DeviceKeys,
    pub esk: [u8; 32],
    pub epk: [u8; 32],
}

impl FakeN {
    pub fn new(k: &Code, esk: [u8; 32]) -> FakeN {
        FakeN { k: k.clone(), keys: DeviceKeys::generate(&mut OsEntropy).unwrap(), esk, epk: x(esk, BASE) }
    }
    pub fn slot_b(&self, rules: &[u32], name: &str) -> Vec<u8> {
        self.k.slot(B, ROLE_N, &b_body(&self.keys.public(), &self.epk, rules, name))
    }
    /// `dh`, `T` and the digits once slot C is read.
    pub fn view(&self, board: &BoardId, dev_e: &DevId, a: &[u8], b: &[u8], c: &[u8]) -> ([u8; 32], [u8; 32], String) {
        let epk_e: [u8; 32] = body(c)[..32].try_into().unwrap();
        let dh = x(self.esk, epk_e);
        let t = transcript(board, &self.k.tid, dev_e, a, b, c);
        (dh, t, sas(&self.k.s, &dh, &t))
    }
}

/// 32 random bytes.
pub fn rand32() -> [u8; 32] {
    let mut b = [0u8; 32];
    OsEntropy.fill(&mut b).unwrap();
    b
}

/// The 18 characters of a code for `s` (§17.2), computed here.
pub fn code_text(s: &[u8; 10]) -> String {
    const ALPHA: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let d = sha(&[&cx("pair-code"), s]);
    let v = (usize::from(d[0]) << 2) | (usize::from(d[1]) >> 6);
    format!("{}{}{}", base32::encode(s), ALPHA[v >> 5] as char, ALPHA[v & 31] as char)
}

/// N driven by a test that plays E (`FakeE`) up to `AwaitD`: what both know.
pub struct WithFakeE {
    pub n: New,
    pub fe: FakeE,
    pub a: Vec<u8>,
    pub b: Vec<u8>,
    pub c: Vec<u8>,
    pub dh: [u8; 32],
    pub t: [u8; 32],
    pub digits: String,
}

impl WithFakeE {
    /// `dev_e` is the id the fake E names in slot A.
    pub fn new(board: BoardId, dev_e: DevId) -> WithFakeE {
        let mut s = [0u8; 10];
        OsEntropy.fill(&mut s).unwrap();
        let k = Code::from_secret(s, &board);
        let fe = FakeE::new(&k, board, dev_e, rand32(), rand32());
        let mut n = New::new();
        let ev = NewEvent::CodeEntered { code: code_text(&s), host: HOST.into(), board_id: board, door: DOOR, name: NAME.into(), now: PT0 };
        n.on(ev, &mut OsEntropy);
        let a = fe.slot_a();
        let fx = n.on(NewEvent::Slot { bytes: a.clone() }, &mut OsEntropy);
        let NewEffect::PostSlot { slot: b } = &fx[0] else { panic!("slot B") };
        let b = b.clone();
        let c = fe.slot_c();
        let fx = n.on(NewEvent::Slot { bytes: c.clone() }, &mut OsEntropy);
        assert_eq!(fx.iter().map(n_name).collect::<Vec<_>>(), ["AskSas"]);
        let (dh, t, digits) = fe.view(&a, &b, &c);
        WithFakeE { n, fe, a, b, c, dh, t, digits }
    }

    /// N accepts the digits.
    pub fn accept(mut self) -> WithFakeE {
        let fx = self.n.on(NewEvent::SasEntered { digits: self.digits.clone() }, &mut OsEntropy);
        assert_eq!(fx.iter().map(n_name).collect::<Vec<_>>(), ["ShowAccepted", "FetchSlot"]);
        self
    }

    /// Slot D holding `pt` sealed under the bundle key and the AD of §17.8.
    pub fn slot_d(&self, pt: &[u8]) -> Vec<u8> {
        let key = bundle_key(&self.fe.k.s, &self.dh, &self.t);
        let mut nonce = [0u8; 24];
        OsEntropy.fill(&mut nonce).unwrap();
        self.fe.k.slot(D, ROLE_E, &aead_seal(&key, nonce, &bundle_ad(&self.fe.board, &self.fe.k.tid, &self.t), pt))
    }

    /// N's public keys, from its slot B.
    pub fn n_dev(&self) -> DevId {
        DevId(body(&self.b)[..16].try_into().unwrap())
    }
}
