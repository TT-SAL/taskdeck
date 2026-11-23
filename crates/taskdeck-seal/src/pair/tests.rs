//! Unit tests of pairing (seal.md §17): the code, the keys, the slots, the
//! transcript and SAS, the bundle's JSON, the draws, and the machines'
//! aborts and erasure. The whole pairing against a board and a relay is in
//! the integration target `pairing_mitm`.

use hkdf::hmac::{Hmac, KeyInit as _, Mac as _};
use sha2::{Digest as _, Sha256};

use super::*;
use crate::receipt::tests::board_tests::Board;
use crate::{Entropy, OsEntropy};

// ------------------------------------------------------------ helpers

/// Entropy that records every draw (its length and bytes); bytes are the
/// SHA-256 of a counter. With `fail_at`, that draw (0-based) fails.
struct Rec {
    n: u64,
    draws: Vec<Vec<u8>>,
    fail_at: Option<usize>,
}

impl Rec {
    fn new(seed: u64) -> Rec {
        Rec { n: seed << 32, draws: Vec::new(), fail_at: None }
    }
    fn lens(&self) -> Vec<usize> {
        self.draws.iter().map(Vec::len).collect()
    }
}

impl crate::entropy::private::Sealed for Rec {}

impl Entropy for Rec {
    fn fill(&mut self, out: &mut [u8]) -> Result<(), SealError> {
        if self.fail_at == Some(self.draws.len()) {
            return Err(SealError::Entropy);
        }
        for chunk in out.chunks_mut(32) {
            self.n += 1;
            let d = Sha256::digest(self.n.to_be_bytes());
            chunk.copy_from_slice(&d[..chunk.len()]);
        }
        self.draws.push(out.to_vec());
        Ok(())
    }
}

fn sha(parts: &[&[u8]]) -> [u8; 32] {
    Sha256::digest(parts.concat()).into()
}

fn hk(salt: &[u8], ikm: &[u8], info: &[u8], n: usize) -> Vec<u8> {
    let mut out = vec![0u8; n];
    hkdf::Hkdf::<Sha256>::new(Some(salt), ikm).expand(info, &mut out).unwrap();
    out
}

fn mac(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut m = Hmac::<Sha256>::new_from_slice(key).unwrap();
    m.update(data);
    m.finalize().into_bytes().into()
}

fn c(name: &str) -> Vec<u8> {
    [b"taskdeck/v1/", name.as_bytes(), b"\0"].concat()
}

const ALPHA: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const ORIGIN: &str = "https://relay.example.org";
const HOST: &str = "relay.example.org";
const DOOR: [u8; 5] = [1, 2, 3, 4, 5];
const T0: u64 = 5_000_000;

/// The code text of `s`, computed here from §17.2.
fn code_of(s: &[u8; 10]) -> String {
    let d = sha(&[&c("pair-code"), s]);
    let v = (u32::from(d[0]) << 2) | (u32::from(d[1]) >> 6);
    let mut t = base32::encode(s);
    t.push(char::from(ALPHA[(v >> 5) as usize]));
    t.push(char::from(ALPHA[(v & 31) as usize]));
    t
}

/// `ticket_key`, `tid`, `mac_key` computed here from §17.3.
fn keys_of(s: &[u8; 10], board: &BoardId) -> ([u8; 32], [u8; 16], [u8; 32]) {
    let tk: [u8; 32] = hk(&board.0, s, &c("pair-ticket"), 32).try_into().unwrap();
    let tid: [u8; 16] = sha(&[&c("pair-tid"), &tk])[..16].try_into().unwrap();
    let mk: [u8; 32] = hk(&board.0, s, &c("pair-mac"), 32).try_into().unwrap();
    (tk, tid, mk)
}

/// A slot built here from §17.4.
fn slot_here(label: u8, role: u8, tid: &[u8; 16], mk: &[u8; 32], body: &[u8]) -> Vec<u8> {
    let len = (body.len() as u16).to_be_bytes();
    let m = mac(mk, &[&c("pair")[..], &[label, role], tid, &len, body].concat());
    [&[label, role][..], &len, body, &m].concat()
}

fn secret_of(code: &str) -> [u8; 10] {
    let plain: String = code.chars().filter(|c| *c != '-').collect();
    base32::decode_typed(&plain[..16], 10).unwrap().try_into().unwrap()
}

/// E started on a one-device board, and N given its code.
struct Pair {
    b: Board,
    e: Existing,
    n: New,
    code: String,
    slot_a: Vec<u8>,
    ern: Rec,
    nrn: Rec,
}

fn started() -> Pair {
    let b = Board::new(2);
    let own = b.devs[0].public().dev;
    let mut ern = Rec::new(1);
    let (e, fx) = Existing::start(&b.m, b.head(), ORIGIN, &own, T0, &mut ern);
    let (ExistingEffect::RegisterTicket { slot_a, .. }, ExistingEffect::ShowCode { code }) = (&fx[0], &fx[1]) else {
        panic!("start")
    };
    let (slot_a, code) = (slot_a.clone(), code.clone());
    Pair { b, e, n: New::new(), code, slot_a, ern, nrn: Rec::new(2) }
}

fn code_entered(code: &str, board: BoardId) -> NewEvent {
    NewEvent::CodeEntered {
        code: code.into(),
        host: HOST.into(),
        board_id: board,
        door: DOOR,
        name: "Phone".into(),
        now: T0 + 10,
    }
}

impl Pair {
    /// Runs both machines to `Showing` and `AwaitSas`: E's digits, slots B and C.
    fn run_to_sas(&mut self) -> (String, Vec<u8>, Vec<u8>) {
        let fx = self.n.on(code_entered(&self.code, self.b.board), &mut self.nrn);
        assert!(matches!(fx[..], [NewEffect::FetchSlot { label: LABEL_A, .. }]));
        let fx = self.n.on(NewEvent::Slot { bytes: self.slot_a.clone() }, &mut self.nrn);
        let NewEffect::PostSlot { slot: slot_b } = &fx[0] else { panic!("slot B") };
        let slot_b = slot_b.clone();
        let fx = self.e.on(ExistingEvent::SlotB { bytes: slot_b.clone() }, &mut self.ern);
        let (ExistingEffect::PostSlot { slot: slot_c }, ExistingEffect::ShowSas { digits }) = (&fx[0], &fx[1]) else {
            panic!("slot C")
        };
        let (slot_c, digits) = (slot_c.clone(), digits.clone());
        let fx = self.n.on(NewEvent::Slot { bytes: slot_c.clone() }, &mut self.nrn);
        assert!(matches!(fx[..], [NewEffect::AskSas]));
        (digits, slot_b, slot_c)
    }
}

fn abort_code_e(fx: &[ExistingEffect]) -> Option<SealError> {
    fx.iter().find_map(|f| match f {
        ExistingEffect::Abort { code } => Some(*code),
        _ => None,
    })
}

fn abort_code_n(fx: &[NewEffect]) -> Option<SealError> {
    match fx {
        [NewEffect::Abort { code }] => Some(*code),
        _ => None,
    }
}

fn snap(h: u64, epoch: u32) -> SnapAnnounce {
    SnapAnnounce {
        seq: h,
        hash: [3; 32],
        hash_version: 1,
        blob: BlobId([4; 16]),
        sha256: [5; 32],
        size: 2_000_000,
        parts: 2,
        inflated: 9_000,
        epoch,
    }
}

fn registered(h: u64, epoch: u32) -> ExistingEvent {
    let roots = if epoch == 1 {
        vec![(1, EpochRoot::from_bytes([9; 32]))]
    } else {
        vec![(epoch - 1, EpochRoot::from_bytes([8; 32])), (epoch, EpochRoot::from_bytes([9; 32]))]
    };
    ExistingEvent::Registered {
        h,
        chain: Chain([6; 32]),
        snapshot: snap(h, epoch),
        added_seq: h + 1,
        token: "tok-\"1\"\n".into(),
        roots,
        host: HOST.into(),
        door: DOOR,
    }
}

// ------------------------------------------------------------- the code

// `new_code` draws exactly the 10 bytes of `s`, and its text is the
// display form of the 16 characters of `s` and the 2 of the first 10 bits
// of `H(ctx("pair-code") ‖ s)`, computed here.
// spec: SEAL-CODE-001
#[test]
fn a_new_code() {
    for seed in 0..50 {
        let mut r = Rec::new(seed);
        let (p, text) = new_code(&mut r).unwrap();
        assert_eq!(r.lens(), vec![10]);
        let s: [u8; 10] = r.draws[0].clone().try_into().unwrap();
        assert_eq!(p.s, s);
        let want = code_of(&s);
        let grouped = format!("{}-{}-{}-{}-{}", &want[..4], &want[4..8], &want[8..12], &want[12..16], &want[16..]);
        assert_eq!(text, grouped);
        assert_eq!(parse_code(&text).unwrap().s, s);
    }
    let mut r = Rec::new(0);
    r.fail_at = Some(0);
    assert_eq!(new_code(&mut r).err(), Some(SealError::Entropy));
}

// Typed codes: normalised as §5.2 (spaces, hyphens, tabs removed; case
// folded; O, I, L read as 0, 1, 1); exactly 18 alphabet characters, else
// `seal/code_format`; the check bits compared, else `seal/code_check`.
// spec: SEAL-CODE-002
#[test]
fn a_typed_code() {
    let s = *b"\x01\x23\x45\x67\x89\xab\xcd\xef\x10\x32";
    let code = code_of(&s);
    assert_eq!(parse_code(&code).unwrap().s, s);
    let looks: String = code
        .chars()
        .map(|c| match c {
            '0' => 'o',
            '1' => 'l',
            c => c.to_ascii_lowercase(),
        })
        .collect();
    assert_eq!(parse_code(&format!(" {}\t{} - {} ", &looks[..5], &looks[5..9], &looks[9..])).unwrap().s, s);
    assert_eq!(parse_code(&code.replace('1', "I")).unwrap().s, s);
    for bad in [&code[..17], &format!("{code}0"), "", "----"] {
        assert_eq!(parse_code(bad).err(), Some(SealError::CodeFormat), "{bad}");
    }
    for at in [0, 9, 17] {
        let mut b = code.clone().into_bytes();
        b[at] = b'U';
        assert_eq!(parse_code(std::str::from_utf8(&b).unwrap()).err(), Some(SealError::CodeFormat));
    }
    assert_eq!(parse_code(&format!("{}\n{}", &code[..9], &code[9..])).err(), Some(SealError::CodeFormat));
    assert_eq!(parse_code(&format!("{}é{}", &code[..9], &code[10..])).err(), Some(SealError::CodeFormat));
    // Every other check, and a changed secret character, fail the check.
    let mut fails = 0;
    for v in 0..1024u32 {
        let typed = format!("{}{}{}", &code[..16], ALPHA[(v >> 5) as usize] as char, ALPHA[(v & 31) as usize] as char);
        match parse_code(&typed) {
            Ok(p) => assert_eq!((p.s, typed.as_str()), (s, code.as_str())),
            Err(e) => {
                assert_eq!(e, SealError::CodeCheck);
                fails += 1;
            }
        }
    }
    assert_eq!(fails, 1023);
    let mut b = code.clone().into_bytes();
    b[3] = if b[3] == b'Z' { b'Y' } else { b'Z' };
    let typed = String::from_utf8(b).unwrap();
    let want = code_of(&secret_of(&typed));
    assert_ne!(&want[16..], &code[16..], "pick another character");
    assert_eq!(parse_code(&typed).err(), Some(SealError::CodeCheck));
}

// A code that fails its check asks again: nothing is derived, no slot is
// fetched, N stays in `Idle`, and a good code then works.
// spec: SEAL-CODE-002
#[test]
fn a_failed_check_asks_again_and_derives_nothing() {
    let b = Board::new(1);
    let s = [7u8; 10];
    let code = code_of(&s);
    let mut wrong = code.clone().into_bytes();
    wrong[17] = if wrong[17] == b'0' { b'1' } else { b'0' };
    let mut n = New::new();
    let mut r = Rec::new(3);
    for (typed, err) in [(String::from_utf8(wrong).unwrap(), SealError::CodeCheck), ("ABC".into(), SealError::CodeFormat)] {
        let fx = n.on(code_entered(&typed, b.board), &mut r);
        assert!(matches!(fx[..], [NewEffect::AskAgain { code }] if code == err));
        assert_eq!(n.state, NewState::Idle);
        assert!(n.sec.is_none());
    }
    // A host outside §5.4 and a name outside §5.5 are asked again too.
    let ev = NewEvent::CodeEntered {
        code: code.clone(),
        host: "Relay.example".into(),
        board_id: b.board,
        door: DOOR,
        name: "x".into(),
        now: 0,
    };
    assert!(matches!(n.on(ev, &mut r)[..], [NewEffect::AskAgain { code: SealError::HostFormat }]));
    let ev =
        NewEvent::CodeEntered { code: code.clone(), host: HOST.into(), board_id: b.board, door: DOOR, name: String::new(), now: 0 };
    assert!(matches!(n.on(ev, &mut r)[..], [NewEffect::AskAgain { code: SealError::SlotFormat }]));
    assert!(n.sec.is_none());
    let fx = n.on(code_entered(&code, b.board), &mut r);
    assert!(matches!(fx[..], [NewEffect::FetchSlot { label: LABEL_A, .. }]));
    assert!(r.draws.is_empty());
}

// `ticket_key`, `tid` and `mac_key` from the decoded code and `board_id`,
// as computed here; the ticket id is `H(ctx("pair-tid") ‖ ticket_key)[0..16]`.
// spec: SEAL-PAIR-001
// spec: SEAL-PAIR-015
#[test]
fn the_keys_of_the_code() {
    let mut p = started();
    let s = secret_of(&p.code);
    let (tk, tid, mk) = keys_of(&s, &p.b.board);
    assert_eq!(p.e.tid, tid);
    assert_eq!(p.e.sec.as_ref().unwrap().mac_key, mk);
    let fx = p.n.on(code_entered(&p.code, p.b.board), &mut p.nrn);
    let [NewEffect::FetchSlot { tid: t, ticket_key, label: LABEL_A }] = &fx[..] else { panic!() };
    assert_eq!((*t, **ticket_key), (tid, tk));
    assert_eq!(p.n.sec.as_ref().unwrap().mac_key, mk);
    // Another board gives other keys from the same code.
    let (tk2, tid2, mk2) = keys_of(&s, &BoardId([0xEE; 16]));
    assert!(tk2 != tk && tid2 != tid && mk2 != mk);
}

// ----------------------------------------------------------- the slots

// Slot A is `0x41 ‖ 0x45 ‖ u16 48 ‖ dev_E ‖ commit ‖ mac`, its MAC over
// `ctx("pair") ‖ label ‖ role ‖ tid ‖ body_len ‖ body`, and `commit =
// H(ctx("pair-commit") ‖ epk_E ‖ r_E)` with `epk_E = X25519(esk_E, 9)`;
// E's start draws `s`, `esk_E`, `r_E` (10, 32, 32 bytes) and nothing else.
// spec: SEAL-PAIR-002
// spec: SEAL-PAIR-016
// spec: SEAL-PAIR-017
#[test]
fn slot_a_and_e_s_draws() {
    let p = started();
    assert_eq!(p.ern.lens(), vec![10, 32, 32]);
    let s: [u8; 10] = p.ern.draws[0].clone().try_into().unwrap();
    let esk: [u8; 32] = p.ern.draws[1].clone().try_into().unwrap();
    let r = &p.ern.draws[2];
    assert_eq!(secret_of(&p.code), s);
    let (_, tid, mk) = keys_of(&s, &p.b.board);
    let epk = x25519_dalek::x25519(esk, x25519_dalek::X25519_BASEPOINT_BYTES);
    // The RFC 7748 function of the raw scalar, not HPKE's DeriveKeyPair.
    assert_ne!(epk, crate::prim::hpke_derive(&esk).1);
    let commit = sha(&[&c("pair-commit"), &epk, r]);
    let body = [&p.b.devs[0].public().dev.0[..], &commit].concat();
    assert_eq!(p.slot_a, slot_here(0x41, 0x45, &tid, &mk, &body));
    assert_eq!(p.slot_a.len(), 36 + 48);
    assert_eq!(make_slot(LABEL_A, ROLE_E, &tid, &mk, &body).unwrap(), p.slot_a);
}

// N draws nothing before slot A; on slot A it draws `dev_N`, the signing
// key, the X25519 `ikm` and `esk_N` in that order (16, 32, 32, 32), its
// keys are those §6.4 makes of them, and slot B lays them out with its
// rules and name; `epk_N = X25519(esk_N, 9)`.
// spec: SEAL-PAIR-016
// spec: SEAL-PAIR-017
// spec: SEAL-KEY-008
// spec: SEAL-PAIR-002
#[test]
fn slot_b_and_n_s_draws() {
    let mut p = started();
    let fx = p.n.on(code_entered(&p.code, p.b.board), &mut p.nrn);
    assert_eq!(fx.len(), 1);
    assert!(p.nrn.draws.is_empty());
    let fx = p.n.on(NewEvent::Slot { bytes: p.slot_a.clone() }, &mut p.nrn);
    assert_eq!(p.nrn.lens(), vec![16, 32, 32, 32]);
    let d = &p.nrn.draws;
    let sign_pk = ed25519_dalek::SigningKey::from_bytes(&d[1].clone().try_into().unwrap()).verifying_key().to_bytes();
    let hpke_pk = crate::prim::hpke_derive(&d[2]).1;
    let epk = x25519_dalek::x25519(d[3].clone().try_into().unwrap(), x25519_dalek::X25519_BASEPOINT_BYTES);
    let pubk = p.n.keys.as_ref().unwrap().public();
    assert_eq!((pubk.dev.0.to_vec(), pubk.sign_pk, pubk.hpke_pk), (d[0].clone(), sign_pk, hpke_pk));
    let body = [&d[0][..], &sign_pk, &hpke_pk, &epk, &[1], &1u32.to_be_bytes(), &[5], b"Phone"].concat();
    let (_, tid, mk) = keys_of(&secret_of(&p.code), &p.b.board);
    let [NewEffect::PostSlot { slot }, NewEffect::FetchSlot { label: LABEL_C, .. }] = &fx[..] else { panic!() };
    assert_eq!(*slot, slot_here(0x42, 0x4E, &tid, &mk, &body));
    // Other entropy, other keys: nothing is derived from the code.
    let mut q = started();
    q.nrn = Rec::new(99);
    q.n.on(code_entered(&q.code, q.b.board), &mut q.nrn);
    q.n.on(NewEvent::Slot { bytes: q.slot_a.clone() }, &mut q.nrn);
    assert_ne!(q.n.keys.as_ref().unwrap().public().sign_pk, sign_pk);
}

// A slot is checked by its MAC, label and role before its body is read:
// a slot with a wrong MAC is `seal/slot_mac` even when its body's layout
// is also wrong; a slot replayed under another label, or reflected to its
// sender (the other role), fails, even with a MAC made for its own label
// and role; a short or overlong framing fails.
// spec: SEAL-PAIR-003
// spec: SEAL-GEN-009
// spec: SEAL-GEN-008
#[test]
fn a_slot_is_checked_before_its_body() {
    let tid = [1u8; 16];
    let mk = [2u8; 32];
    let body = [0u8; 48];
    let a = slot_here(0x41, 0x45, &tid, &mk, &body);
    assert_eq!(open_slot(&a, LABEL_A, ROLE_E, &tid, &mk).unwrap(), &body[..]);
    // Replayed as C, reflected as N's, under another ticket or key.
    assert_eq!(open_slot(&a, LABEL_C, ROLE_E, &tid, &mk).err(), Some(SealError::SlotMac));
    assert_eq!(open_slot(&a, LABEL_A, ROLE_N, &tid, &mk).err(), Some(SealError::SlotMac));
    assert_eq!(open_slot(&a, LABEL_A, ROLE_E, &[9; 16], &mk).err(), Some(SealError::SlotMac));
    assert_eq!(open_slot(&a, LABEL_A, ROLE_E, &tid, &[9; 32]).err(), Some(SealError::SlotMac));
    let relabelled = slot_here(0x43, 0x45, &tid, &mk, &body);
    assert_eq!(open_slot(&relabelled, LABEL_A, ROLE_E, &tid, &mk).err(), Some(SealError::SlotMac));
    let reflected = slot_here(0x41, 0x4E, &tid, &mk, &body);
    assert_eq!(open_slot(&reflected, LABEL_A, ROLE_E, &tid, &mk).err(), Some(SealError::SlotMac));
    // Framing: shorter, longer, a body over 16,040 bytes.
    for bad in [&a[..a.len() - 1], &[&a[..], &[0]].concat()[..], &a[..3], &[][..]] {
        assert_eq!(open_slot(bad, LABEL_A, ROLE_E, &tid, &mk).err(), Some(SealError::SlotMac));
    }
    let big = slot_here(0x44, 0x45, &tid, &mk, &vec![0u8; 16_041]);
    assert_eq!(open_slot(&big, LABEL_D, ROLE_E, &tid, &mk).err(), Some(SealError::SlotMac));
    assert!(make_slot(LABEL_D, ROLE_E, &tid, &mk, &[0u8; 16_041]).is_none());
    let max = slot_here(0x44, 0x45, &tid, &mk, &vec![0u8; 16_040]);
    assert!(open_slot(&max, LABEL_D, ROLE_E, &tid, &mk).is_ok());
    // A slot B whose body breaks the layout: `seal/slot_mac` with a bad MAC
    // (nothing of the body was read), `seal/slot_format` with a good one.
    let mut p = started();
    let (_, tid, mk) = keys_of(&secret_of(&p.code), &p.b.board);
    let bad_body = [0u8; 10];
    let mut wrong = slot_here(0x42, 0x4E, &tid, &mk, &bad_body);
    let n = wrong.len();
    wrong[n - 1] ^= 1;
    let fx = p.e.on(ExistingEvent::SlotB { bytes: wrong }, &mut p.ern);
    assert_eq!(abort_code_e(&fx), Some(SealError::SlotMac));
    let mut p = started();
    let (_, tid, mk) = keys_of(&secret_of(&p.code), &p.b.board);
    let fx = p.e.on(ExistingEvent::SlotB { bytes: slot_here(0x42, 0x4E, &tid, &mk, &bad_body) }, &mut p.ern);
    assert_eq!(abort_code_e(&fx), Some(SealError::SlotFormat));
}

// Slot B's layout and limits: 119 to 242 bytes, 1 to 16 ascending rules
// versions, a name of §5.5, nothing left over.
// spec: SEAL-PAIR-002
#[test]
fn slot_b_layout() {
    let keys = [[1u8; 16].to_vec(), vec![2; 32], vec![3; 32], vec![4; 32]].concat();
    let body = |rules: &[u32], name: &[u8], extra: &[u8]| {
        let mut b = keys.clone();
        b.push(rules.len() as u8);
        for r in rules {
            b.extend(r.to_be_bytes());
        }
        b.push(name.len() as u8);
        b.extend(name);
        b.extend(extra);
        b
    };
    let ok = body(&[1], b"P", &[]);
    assert_eq!(ok.len(), 119);
    let b = parse_b(&ok).unwrap();
    assert_eq!((b.dev, b.rules, b.name.as_str()), (DevId([1; 16]), vec![1], "P"));
    let sixteen: Vec<u32> = (1..=16).collect();
    let long = body(&sixteen, &[b'n'; 64], &[]);
    assert_eq!(long.len(), 242);
    assert!(parse_b(&long).is_some());
    for bad in [
        body(&[], b"Phone", &[]),
        body(&(1..=17).collect::<Vec<_>>(), b"P", &[]),
        body(&[2, 1], b"Phone", &[]),
        body(&[1, 1], b"Phone", &[]),
        body(&[0], b"Phone", &[]),
        body(&[0x8000_0000], b"Phone", &[]),
        body(&[1], b"", &[]),
        body(&[1], b"P\x07", &[]),
        body(&[1], &[0xff, 0xfe], &[]),
        body(&[1], b"Phone", &[0]),
        body(&[1], &[b'n'; 65], &[]),
        ok[..118].to_vec(),
    ] {
        assert!(parse_b(&bad).is_none(), "{bad:?}");
    }
    // A name length past the end.
    let mut short_name = ok.clone();
    let n = short_name.len();
    short_name[n - 2] = 2;
    assert!(parse_b(&short_name).is_none());
}

// ------------------------------------------------------ transcript, SAS

// `T` over the relay origin, the board, `tid`, `dev_E` and slots A to C
// verbatim, and the six digits of `HKDF(s, dh, ctx("sas") ‖ T, 8)` as a
// big-endian u64 mod 10^6, computed here: E shows them, and N accepts them.
// spec: SEAL-PAIR-006
#[test]
fn transcript_and_sas() {
    let mut p = started();
    let (digits, slot_b, slot_c) = p.run_to_sas();
    let s = secret_of(&p.code);
    let (_, tid, _) = keys_of(&s, &p.b.board);
    let esk_e: [u8; 32] = p.ern.draws[1].clone().try_into().unwrap();
    let epk_n: [u8; 32] = slot_b[4 + 80..4 + 112].try_into().unwrap();
    let dh = x25519_dalek::x25519(esk_e, epk_n);
    let dev_e = p.b.devs[0].public().dev;
    let t = sha(&[
        &c("pair-transcript"),
        &(ORIGIN.len() as u16).to_be_bytes(),
        ORIGIN.as_bytes(),
        &p.b.board.0,
        &tid,
        &dev_e.0,
        &p.slot_a,
        &slot_b,
        &slot_c,
    ]);
    assert_eq!(p.e.transcript, t);
    assert_eq!(p.n.transcript, t);
    let okm = hk(&s, &dh, &[&c("sas")[..], &t].concat(), 8);
    let v = u64::from_be_bytes(okm.try_into().unwrap()) % 1_000_000;
    assert_eq!(digits, format!("{v:06}"));
    assert_eq!(&p.n.sec.as_ref().unwrap().sas, digits.as_bytes());
    assert_eq!(p.e.sec.as_ref().unwrap().dh, dh);
    let fx = p.n.on(NewEvent::SasEntered { digits }, &mut p.nrn);
    assert!(matches!(fx[..], [NewEffect::ShowAccepted, NewEffect::FetchSlot { label: LABEL_D, .. }]));
}

// At most three entries: two wrong ones ask again, a third aborts with
// `seal/sas_mismatch` and erases; the right digits after two wrong ones are
// accepted. A digit string of another length or with spaces is wrong.
// spec: SEAL-PAIR-007
#[test]
fn three_sas_entries() {
    let mut p = started();
    let (digits, _, _) = p.run_to_sas();
    let other = format!("{:06}", (digits.parse::<u32>().unwrap() + 1) % 1_000_000);
    for typed in [format!(" {digits}"), other.clone()] {
        assert!(matches!(p.n.on(NewEvent::SasEntered { digits: typed }, &mut p.nrn)[..], [NewEffect::AskSas]));
    }
    let fx = p.n.on(NewEvent::SasEntered { digits: digits[..5].into() }, &mut p.nrn);
    assert_eq!(abort_code_n(&fx), Some(SealError::SasMismatch));
    assert!(p.n.sec.is_none() && p.n.keys.is_none());
    assert_eq!(p.n.state, NewState::Aborted);
    // The right digits now change nothing.
    assert!(p.n.on(NewEvent::SasEntered { digits }, &mut p.nrn).is_empty());

    let mut p = started();
    let (digits, _, _) = p.run_to_sas();
    for _ in 0..2 {
        assert!(matches!(p.n.on(NewEvent::SasEntered { digits: other.clone() }, &mut p.nrn)[..], [NewEffect::AskSas]));
    }
    let fx = p.n.on(NewEvent::SasEntered { digits }, &mut p.nrn);
    assert!(matches!(fx[..], [NewEffect::ShowAccepted, NewEffect::FetchSlot { .. }]));
}

// An all-zero X25519 output aborts with `seal/dh_zero`: on E for a slot B
// whose `epk_N` is a small-order point, on N for a slot C whose `epk_E` is
// one (with the commitment made for it).
// spec: SEAL-GEN-013
#[test]
fn an_all_zero_dh_aborts() {
    let mut p = started();
    let (_, tid, mk) = keys_of(&secret_of(&p.code), &p.b.board);
    let body = [&[0x77u8; 16][..], &[0x55; 32], &[0x66; 32], &[0; 32], &[1], &1u32.to_be_bytes(), &[1], b"P"].concat();
    let fx = p.e.on(ExistingEvent::SlotB { bytes: slot_here(0x42, 0x4E, &tid, &mk, &body) }, &mut p.ern);
    assert!(matches!(fx[..], [ExistingEffect::DeleteTicket { .. }, ExistingEffect::Abort { code: SealError::DhZero }]));

    let mut p = started();
    let (_, tid, mk) = keys_of(&secret_of(&p.code), &p.b.board);
    // N gets a slot A committing to the order-1 point `u = 1` (X25519 of
    // any scalar with it is all zero), then its opening.
    let mut one = [0u8; 32];
    one[0] = 1;
    let r = [5u8; 32];
    let a = slot_here(0x41, 0x45, &tid, &mk, &[&[0x11u8; 16][..], &sha(&[&c("pair-commit"), &one, &r])].concat());
    p.n.on(code_entered(&p.code, p.b.board), &mut p.nrn);
    p.n.on(NewEvent::Slot { bytes: a }, &mut p.nrn);
    let fx = p.n.on(NewEvent::Slot { bytes: slot_here(0x43, 0x45, &tid, &mk, &[one, r].concat()) }, &mut p.nrn);
    assert_eq!(abort_code_n(&fx), Some(SealError::DhZero));
}

// ------------------------------------------------------------ the bundle

fn a_bundle(epoch: u32) -> Bundle {
    let roots = if epoch == 1 {
        vec![(1, EpochRoot::from_bytes([9; 32]))]
    } else {
        vec![(epoch - 1, EpochRoot::from_bytes([8; 32])), (epoch, EpochRoot::from_bytes([9; 32]))]
    };
    Bundle {
        board_id: BoardId([0xB0; 16]),
        host: "relay.example.org:8443".into(),
        door: DOOR,
        roots,
        head: Head { seq: 70, chain: Chain([6; 32]) },
        snapshot: snap(70, epoch),
        added_seq: 72,
        token: "t\u{1}\"\\/\u{7f}".into(),
    }
}

fn hexs(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The bundle as a JSON value, encoded by the crate's canonical encoder.
fn bundle_value(b: &Bundle) -> serde_json::Value {
    let s = &b.snapshot;
    serde_json::json!({
        "board_id": hexs(&b.board_id.0),
        "host": b.host,
        "door": hexs(&b.door),
        "roots": b.roots.iter().map(|(e, r)| serde_json::json!({"epoch": e, "root": hexs(&r.to_bytes()[..])})).collect::<Vec<_>>(),
        "head": {"seq": b.head.seq, "chain": hexs(&b.head.chain.0)},
        "snapshot": {"blob": hexs(&s.blob.0), "sha256": hexs(&s.sha256), "size": s.size, "parts": s.parts,
                     "inflated": s.inflated, "epoch": s.epoch, "hash": hexs(&s.hash), "hash_version": s.hash_version},
        "added_seq": b.added_seq,
        "token": b.token,
    })
}

// The bundle's JSON is the canonical encoding of exactly its table's
// fields (the crate's canonical encoder, `core.md` §4.2, gives the same
// bytes) and reads back; the parser takes nothing else: whitespace, another
// key order, a field more or less, a non-canonical number, string or hex,
// and every constraint of the table.
// spec: SEAL-PAIR-009
// spec: SEAL-KEY-015
#[test]
fn the_bundle_json() {
    for epoch in [1, 2, 7] {
        let b = a_bundle(epoch);
        let json = encode_bundle(&b);
        assert_eq!(&json[..], &crate::canon::encode(&bundle_value(&b)).unwrap()[..]);
        let back = parse_bundle(&json).unwrap();
        assert_eq!(&encode_bundle(&back)[..], &json[..]);
        assert_eq!(back.snapshot.seq, 70);
    }
    let b = a_bundle(2);
    let json = String::from_utf8(encode_bundle(&b).to_vec()).unwrap();
    let edits: Vec<(&str, String)> = vec![
        ("whitespace", json.replacen(",", ", ", 1)),
        ("a field more", json.replacen("{\"added_seq\"", "{\"a\":1,\"added_seq\"", 1)),
        ("a field less", json.replacen("\"added_seq\":72,", "", 1)),
        ("key order", json.replacen("\"chain\":\"0606", "\"seq\":70,\"chain\":\"0606", 1).replacen(",\"seq\":70}", "}", 1)),
        ("leading zero", json.replacen("\"added_seq\":72", "\"added_seq\":072", 1)),
        ("a fraction", json.replacen("\"added_seq\":72", "\"added_seq\":72.0", 1)),
        ("negative", json.replacen("\"added_seq\":72", "\"added_seq\":-72", 1)),
        ("uppercase hex", json.replacen("b0b0", "B0B0", 1)),
        ("short hex", json.replacen("\"0102030405\"", "\"01020304\"", 1)),
        ("escaped slash", json.replacen("/", "\\/", 1)),
        ("unicode escape", json.replacen("\\u0001", "\\u0041", 1)),
        ("a raw control byte", json.replacen("\\u0001", "\u{1}", 1)),
        ("added_seq not above h", json.replacen("\"added_seq\":72", "\"added_seq\":70", 1)),
        ("an empty token", json.replacen("\"token\":\"t\\u0001\\\"\\\\/\u{7f}\"", "\"token\":\"\"", 1)),
        ("a token not ASCII", json.replacen("\"token\":\"t", "\"token\":\"\u{e9}", 1)),
        ("a token over 512 bytes", json.replacen("\"token\":\"t", &format!("\"token\":\"{}", "x".repeat(512)), 1)),
        ("a host outside §5.4", json.replacen("relay.example.org", "Relay.example.org", 1)),
        ("one root at epoch 2", json.replacen("{\"epoch\":1,\"root\":\"0808080808080808080808080808080808080808080808080808080808080808\"},", "", 1)),
        ("a root of a later epoch", json.replacen("{\"epoch\":1,", "{\"epoch\":3,", 1)),
        ("three roots", json.replacen("\"roots\":[", "\"roots\":[{\"epoch\":0,\"root\":\"0808080808080808080808080808080808080808080808080808080808080808\"},", 1)),
        ("no roots", json.replacen(&json[json.find("\"roots\":[").unwrap()..json.find("],\"snapshot\"").unwrap()], "\"roots\":[", 1)),
        ("parts not ceil(size / 2^20)", json.replacen("\"parts\":2", "\"parts\":3", 1)),
        ("size under 67", json.replacen("\"size\":2000000", "\"size\":66", 1)),
        ("inflated 0", json.replacen("\"inflated\":9000", "\"inflated\":0", 1)),
        ("hash_version 0", json.replacen("\"hash_version\":1", "\"hash_version\":0", 1)),
        ("an integer over 2^53 - 1", json.replacen("\"added_seq\":72", "\"added_seq\":9007199254740992", 1)),
        ("trailing", format!("{json} ")),
    ];
    for (what, j) in edits {
        assert_ne!(j, json, "{what}: the edit did not apply");
        assert!(parse_bundle(j.as_bytes()).is_none(), "{what}");
    }
    let mut big = a_bundle(2);
    big.token = "x".repeat(512);
    assert!(parse_bundle(&encode_bundle(&big)).is_some());
    assert!(bundle_is_valid(&big));
    big.snapshot.seq = 69;
    assert!(!bundle_is_valid(&big));
}

// E's slot D: `nonce ‖ XChaCha20-Poly1305(bundle_key, nonce, AD, JSON)` with
// `bundle_key = HKDF(s, dh, ctx("pair-bundle-key") ‖ T, 32)` and `AD =
// ctx("pair-bundle") ‖ board_id ‖ tid ‖ T`, computed here; the nonce is
// E's only draw after slot A; N opens it.
// spec: SEAL-PAIR-009
// spec: SEAL-GEN-012
#[test]
fn the_bundle_is_sealed_with_bundle_key_and_ad() {
    let mut p = started();
    let (digits, _, _) = p.run_to_sas();
    let s = secret_of(&p.code);
    let dh = p.e.sec.as_ref().unwrap().dh;
    let t = p.e.transcript;
    let (_, tid, _) = keys_of(&s, &p.b.board);
    p.n.on(NewEvent::SasEntered { digits }, &mut p.nrn);
    let fx = p.e.on(ExistingEvent::UserConfirmed, &mut p.ern);
    let [ExistingEffect::NeedSnapshotAndRegister { device }] = &fx[..] else { panic!() };
    assert_eq!(device.transcript, t);
    let before = p.ern.draws.len();
    let fx = p.e.on(registered(70, 2), &mut p.ern);
    assert_eq!(p.ern.lens()[before..], [24]);
    let [ExistingEffect::PostSlot { slot }, ExistingEffect::Done { dev }] = &fx[..] else { panic!() };
    assert_eq!(*dev, p.n.keys.as_ref().unwrap().public().dev);
    let body = &slot[4..slot.len() - 32];
    assert_eq!(&body[..24], &p.ern.draws[before][..]);
    let key = hk(&s, &dh, &[&c("pair-bundle-key")[..], &t].concat(), 32);
    let ad = [&c("pair-bundle")[..], &p.b.board.0, &tid, &t].concat();
    let pt = XChaCha20Poly1305::new_from_slice(&key)
        .unwrap()
        .decrypt(&XNonce::from(<[u8; 24]>::try_from(&body[..24]).unwrap()), Payload { msg: &body[24..], aad: &ad })
        .unwrap();
    let b = parse_bundle(&pt).unwrap();
    assert_eq!((b.board_id, b.head.seq, b.added_seq, b.token.as_str()), (p.b.board, 70, 71, "tok-\"1\"\n"));
    // N: steps 1 to 3 pass.
    let fx = p.n.on(NewEvent::Slot { bytes: slot.clone() }, &mut p.nrn);
    assert!(matches!(&fx[..], [NewEffect::FetchSnapshot { blob, parts: 2, .. }] if *blob == BlobId([4; 16])));
}

// Values E is given that the bundle's table excludes abort with
// `seal/bundle_invalid` and post nothing.
// spec: SEAL-PAIR-009
#[test]
fn e_refuses_a_bundle_outside_its_table() {
    type Edit = Box<dyn Fn(&mut ExistingEvent)>;
    let bad: Vec<Edit> = vec![
        Box::new(|e| if let ExistingEvent::Registered { added_seq, .. } = e { *added_seq = 70 }),
        Box::new(|e| if let ExistingEvent::Registered { token, .. } = e { token.clear() }),
        Box::new(|e| if let ExistingEvent::Registered { token, .. } = e { *token = "x".repeat(513) }),
        Box::new(|e| if let ExistingEvent::Registered { host, .. } = e { *host = "relay.example.org:443".into() }),
        Box::new(|e| if let ExistingEvent::Registered { roots, .. } = e { roots.remove(0); }),
        Box::new(|e| if let ExistingEvent::Registered { snapshot, .. } = e { snapshot.parts = 1 }),
        Box::new(|e| if let ExistingEvent::Registered { snapshot, .. } = e { snapshot.seq = 64 }),
    ];
    for f in bad {
        let mut p = started();
        p.run_to_sas();
        p.e.on(ExistingEvent::UserConfirmed, &mut p.ern);
        let mut ev = registered(70, 2);
        f(&mut ev);
        let fx = p.e.on(ev, &mut p.ern);
        assert!(matches!(fx[..], [ExistingEffect::DeleteTicket { .. }, ExistingEffect::Abort { code: SealError::BundleInvalid }]));
    }
}

// ------------------------------------------------------------- aborts

// Every abort erases the attempt's secrets (and N's long-term keys and
// bundle), leaves the machine `Aborted`, and no later event does anything.
// spec: SEAL-PAIR-014
#[test]
fn an_abort_erases_and_is_final() {
    let mut p = started();
    p.run_to_sas();
    assert!(p.e.sec.is_some() && p.n.sec.is_some() && p.n.keys.is_some());
    let fx = p.e.on(ExistingEvent::Cancel, &mut p.ern);
    assert!(matches!(fx[..], [ExistingEffect::DeleteTicket { tid }, ExistingEffect::Abort { code: SealError::PairCancelled }] if tid == p.e.tid));
    assert!(p.e.sec.is_none());
    assert_eq!(p.e.state, ExistingState::Aborted);
    for ev in [ExistingEvent::UserConfirmed, ExistingEvent::Cancel, ExistingEvent::Tick { now: u64::MAX }, registered(70, 2)] {
        assert!(p.e.on(ev, &mut p.ern).is_empty());
        assert_eq!(p.e.state, ExistingState::Aborted);
    }
    let fx = p.n.on(NewEvent::Cancel, &mut p.nrn);
    assert_eq!(abort_code_n(&fx), Some(SealError::PairCancelled));
    assert!(p.n.sec.is_none() && p.n.keys.is_none() && p.n.bundle.is_none() && p.n.join.is_none());
    for ev in [NewEvent::Cancel, NewEvent::SasEntered { digits: "000000".into() }, NewEvent::Tick { now: u64::MAX }] {
        assert!(p.n.on(ev, &mut p.nrn).is_empty());
        assert_eq!(p.n.state, NewState::Aborted);
    }
    // N in `AwaitSnapshot` holds the bundle; an abort drops it too.
    let mut p = started();
    let (digits, _, _) = p.run_to_sas();
    p.n.on(NewEvent::SasEntered { digits }, &mut p.nrn);
    p.e.on(ExistingEvent::UserConfirmed, &mut p.ern);
    let fx = p.e.on(registered(70, 2), &mut p.ern);
    let ExistingEffect::PostSlot { slot } = &fx[0] else { panic!() };
    assert!(p.e.sec.is_none(), "E erases at Done");
    p.n.on(NewEvent::Slot { bytes: slot.clone() }, &mut p.nrn);
    assert!(p.n.bundle.is_some());
    let fx = p.n.on(NewEvent::Snapshot { parts: vec![vec![0; 10]] }, &mut p.nrn);
    assert_eq!(abort_code_n(&fx), Some(SealError::BundleInvalid));
    assert!(p.n.sec.is_none() && p.n.keys.is_none() && p.n.bundle.is_none());
}

// 600,000 ms of the supplied time after the start abort, on both sides;
// 599,999 do not. N's start is its accepted `CodeEntered`; in `Idle` a
// tick does nothing.
// spec: SEAL-PAIR-013
#[test]
fn the_time_limit() {
    let mut p = started();
    assert!(p.e.on(ExistingEvent::Tick { now: T0 + 599_999 }, &mut p.ern).is_empty());
    let fx = p.e.on(ExistingEvent::Tick { now: T0 + 600_000 }, &mut p.ern);
    assert!(matches!(fx[..], [ExistingEffect::DeleteTicket { .. }, ExistingEffect::Abort { code: SealError::PairTimeout }]));
    assert!(p.e.sec.is_none());
    assert!(p.n.on(NewEvent::Tick { now: u64::MAX }, &mut p.nrn).is_empty());
    p.n.on(code_entered(&p.code, p.b.board), &mut p.nrn);
    assert!(p.n.on(NewEvent::Tick { now: T0 + 10 + 599_999 }, &mut p.nrn).is_empty());
    let fx = p.n.on(NewEvent::Tick { now: T0 + 10 + 600_000 }, &mut p.nrn);
    assert_eq!(abort_code_n(&fx), Some(SealError::PairTimeout));
    assert!(p.n.sec.is_none());
    // The limit holds in every state, `Showing` and `AwaitSas` included.
    let mut p = started();
    p.run_to_sas();
    assert_eq!(abort_code_e(&p.e.on(ExistingEvent::Tick { now: T0 + 600_000 }, &mut p.ern)), Some(SealError::PairTimeout));
    assert_eq!(abort_code_n(&p.n.on(NewEvent::Tick { now: T0 + 600_010 }, &mut p.nrn)), Some(SealError::PairTimeout));
}

// E's guard and draws: a device that is not current, an origin that is
// not `https://` and a host, and entropy that fails each abort `start`
// without a ticket.
// spec: SEAL-GEN-004
#[test]
fn e_s_start_guard() {
    let b = Board::new(1);
    let own = b.devs[0].public().dev;
    let (e, fx) = Existing::start(&b.m, b.head(), ORIGIN, &DevId([0x99; 16]), 0, &mut OsEntropy);
    assert!(matches!(fx[..], [ExistingEffect::Abort { code: SealError::CtlAuthor }]));
    assert_eq!(e.state, ExistingState::Aborted);
    for origin in ["relay.example.org", "http://relay.example.org", "https://Relay", "https://relay.example.org:443"] {
        let (_, fx) = Existing::start(&b.m, b.head(), origin, &own, 0, &mut OsEntropy);
        assert!(matches!(fx[..], [ExistingEffect::Abort { code: SealError::HostFormat }]), "{origin}");
    }
    for at in 0..3 {
        let mut r = Rec::new(4);
        r.fail_at = Some(at);
        let (e, fx) = Existing::start(&b.m, b.head(), ORIGIN, &own, 0, &mut r);
        assert!(matches!(fx[..], [ExistingEffect::Abort { code: SealError::Entropy }]));
        assert!(e.sec.is_none());
    }
    // N's draws on slot A, and E's nonce at `Registered`.
    for at in 0..4 {
        let mut p = started();
        p.n.on(code_entered(&p.code, p.b.board), &mut p.nrn);
        p.nrn.fail_at = Some(at);
        let fx = p.n.on(NewEvent::Slot { bytes: p.slot_a.clone() }, &mut p.nrn);
        assert_eq!(abort_code_n(&fx), Some(SealError::Entropy));
        assert!(p.n.keys.is_none());
    }
    let mut p = started();
    p.run_to_sas();
    p.e.on(ExistingEvent::UserConfirmed, &mut p.ern);
    p.ern.fail_at = Some(p.ern.draws.len());
    assert_eq!(abort_code_e(&p.e.on(registered(70, 2), &mut p.ern)), Some(SealError::Entropy));
}

// Events a state does not expect abort (lead ruling): a slot with
// `seal/slot_mac`, N's join data with `seal/bundle_invalid`, E's
// `RegisterFailed` with its own code, anything else with
// `seal/pair_cancelled`. Slot C is never posted before slot B verified,
// and nothing is sealed for N before the user confirmed.
// spec: SEAL-PAIR-005
// spec: SEAL-PAIR-008
#[test]
fn unexpected_events_abort() {
    let ev_e: Vec<(fn() -> ExistingEvent, SealError)> = vec![
        (|| ExistingEvent::UserConfirmed, SealError::PairCancelled),
        (|| registered(70, 2), SealError::PairCancelled),
        (|| ExistingEvent::RegisterFailed { code: SealError::PairRaced }, SealError::PairRaced),
    ];
    for (ev, code) in ev_e {
        let mut p = started();
        let fx = p.e.on(ev(), &mut p.ern);
        assert!(matches!(fx[..], [ExistingEffect::DeleteTicket { .. }, ExistingEffect::Abort { code: c }] if c == code));
        assert!(!fx.iter().any(|f| matches!(f, ExistingEffect::PostSlot { .. })));
    }
    // In `Showing`: a second slot B that is not the one accepted (the same
    // slot B again is ignored, §17.10), or `Registered` before
    // `UserConfirmed`.
    let mut p = started();
    let (_, slot_b, _) = p.run_to_sas();
    assert!(p.e.on(ExistingEvent::SlotB { bytes: slot_b.clone() }, &mut p.ern).is_empty());
    assert_eq!(p.e.state, ExistingState::Showing);
    let mut other = slot_b;
    other[5] ^= 1;
    let fx = p.e.on(ExistingEvent::SlotB { bytes: other }, &mut p.ern);
    assert_eq!(abort_code_e(&fx), Some(SealError::SlotMac));
    let mut p = started();
    p.run_to_sas();
    let fx = p.e.on(registered(70, 2), &mut p.ern);
    assert_eq!(abort_code_e(&fx), Some(SealError::PairCancelled));
    assert!(!fx.iter().any(|f| matches!(f, ExistingEffect::PostSlot { .. })));
    // `RegisterFailed` in `Registering`: only `Abort{code}` (§17.10).
    let mut p = started();
    p.run_to_sas();
    p.e.on(ExistingEvent::UserConfirmed, &mut p.ern);
    let fx = p.e.on(ExistingEvent::RegisterFailed { code: SealError::PairRaced }, &mut p.ern);
    assert!(matches!(fx[..], [ExistingEffect::Abort { code: SealError::PairRaced }]));
    assert!(p.e.sec.is_none());

    let ev_n: Vec<(fn() -> NewEvent, SealError)> = vec![
        (|| NewEvent::Slot { bytes: vec![0; 84] }, SealError::SlotMac),
        (|| NewEvent::SasEntered { digits: "123456".into() }, SealError::PairCancelled),
        (|| NewEvent::Snapshot { parts: vec![] }, SealError::BundleInvalid),
        (|| NewEvent::Entries { frames: vec![] }, SealError::BundleInvalid),
        (|| NewEvent::Frames { frames: vec![] }, SealError::BundleInvalid),
    ];
    for (ev, code) in ev_n {
        let mut n = New::new();
        assert_eq!(abort_code_n(&n.on(ev(), &mut OsEntropy)), Some(code));
        let mut p = started();
        p.n.on(code_entered(&p.code, p.b.board), &mut p.nrn);
        assert_eq!(abort_code_n(&p.n.on(ev(), &mut p.nrn)), Some(code));
    }
    let mut p = started();
    p.n.on(code_entered(&p.code, p.b.board), &mut p.nrn);
    let fx = p.n.on(code_entered(&p.code, p.b.board), &mut p.nrn);
    assert_eq!(abort_code_n(&fx), Some(SealError::PairCancelled));
}

// ---------------------------------------- §17.6 check 5, E's two other limits

/// E (started on a two-device board) with its copy of the membership edited
/// by `edit`, given N's slot B: E's effects.
fn e_on_b_with(edit: impl FnOnce(&mut Membership)) -> (Existing, Vec<ExistingEffect>) {
    let mut p = started();
    edit(p.e.membership.as_mut().unwrap());
    p.n.on(code_entered(&p.code, p.b.board), &mut p.nrn);
    let fx = p.n.on(NewEvent::Slot { bytes: p.slot_a.clone() }, &mut p.nrn);
    let NewEffect::PostSlot { slot } = &fx[0] else { panic!("slot B") };
    let fx = p.e.on(ExistingEvent::SlotB { bytes: slot.clone() }, &mut p.ern);
    (p.e, fx)
}

/// E refused slot B with `seal/ctl_device_cap`: `DeleteTicket`, `Abort`,
/// nothing posted, and the machine aborted.
fn refused_for_cap(e: &Existing, fx: &[ExistingEffect]) {
    assert!(
        matches!(fx[..], [ExistingEffect::DeleteTicket { .. }, ExistingEffect::Abort { code: SealError::CtlDeviceCap }]),
        "{:?}",
        abort_code_e(fx)
    );
    assert!(matches!(e.state, ExistingState::Aborted));
    assert!(e.sec.is_none());
}

fn accepted(fx: &[ExistingEffect]) {
    assert!(matches!(fx[..], [ExistingEffect::PostSlot { .. }, ExistingEffect::ShowSas { .. }]), "{:?}", abort_code_e(fx));
}

/// Fills the membership with revoked device records up to `total` records;
/// the current ones stay as they were.
fn revoked_up_to(m: &mut Membership, total: usize) {
    let base = m.devices[0].dup();
    let mut i: u16 = 0;
    while m.devices.len() < total {
        i += 1;
        let mut d = base.dup();
        let mut id = [0xA0u8; 16];
        id[14..].copy_from_slice(&i.to_be_bytes());
        d.dev = DevId(id);
        d.sign_pk = [0xB0; 32];
        d.sign_pk[30..].copy_from_slice(&i.to_be_bytes());
        d.hpke_pk = [0xC0; 32];
        d.hpke_pk[30..].copy_from_slice(&i.to_be_bytes());
        d.device_no = 100 + i;
        d.status = crate::DeviceStatus::Revoked;
        m.devices.push(d);
    }
    m.devices.sort_by_key(|d| d.dev);
    m.next_device_no = 101 + i;
}

// §17.6 check 5 (ruling Q66): a membership that already holds 256 device
// records, with fewer than 16 current, has no room for N: E refuses slot B
// with `seal/ctl_device_cap` and posts nothing (slot C is never posted).
// With 255 records, E goes on.
// spec: SEAL-PAIR-005
#[test]
fn e_refuses_slot_b_at_256_device_records() {
    let (e, fx) = e_on_b_with(|m| {
        revoked_up_to(m, 256);
        assert!(m.current_count() < 16);
    });
    refused_for_cap(&e, &fx);
    let (_, fx) = e_on_b_with(|m| revoked_up_to(m, 255));
    accepted(&fx);
}

// §17.6 check 5: a membership whose `next_device_no` is above 8191 has no
// `device_no` left for N (the `DeviceAdded` would be refused): E refuses
// slot B with `seal/ctl_device_cap` and posts nothing. At 8191, E goes on.
// spec: SEAL-PAIR-005
// spec: SEAL-MEM-014
#[test]
fn e_refuses_slot_b_when_no_device_no_is_left() {
    let (e, fx) = e_on_b_with(|m| m.next_device_no = 8192);
    refused_for_cap(&e, &fx);
    let (_, fx) = e_on_b_with(|m| m.next_device_no = 8191);
    accepted(&fx);
}

// The comparisons of pairing's secret-derived values are made with `ct_eq`
// (SEAL-GEN-007): the code's check characters (`parse_code`); the slot MAC
// with its label and role (`open_slot`); slot C's opening with the
// commitment (N's `slot_c`); the typed digits with the SAS (`sas_entered`);
// the transcript a `DeviceAdded` carries with N's own (§17.9 step 7,
// `check_added`); the repeat check of an accepted slot (`same_slot`, and
// the two machines' use of it); the all-zero check of the Diffie-Hellman
// output (`diffie_hellman`, SEAL-GEN-013); and each bundle root's commitment
// with the one `M_h` records (§17.9 step 5, N's `snapshot`). None of these
// values is compared with `==` or `!=`: no line of those functions that holds
// `==` or `!=` names one.
//
// Left out, because the values are public, not derived from a secret: in
// N's `snapshot`, the snapshot's chain value with the bundle head's
// (`body.chain != b.head.chain`) and the recomputed state hash and the
// body's `state_hash` with the bundle's (`hash_ok`: the state hash a signed
// entry carries; `ct_eq` there all the same); in N's `frames`, a frame's
// `frame_hash` with the one its Skip item names; in N's `slot_d`, the
// bundle's `board_id`, `host` and `door` with those entered with the code
// (the relay address); in N's `check_added` and E's `slot_b`, device ids,
// public keys, names and seqs; and the lengths, counts, epochs and seqs
// compared everywhere, and the JSON literals of the bundle's parser
// (`Cur::lit`).
// spec: SEAL-GEN-007
#[test]
fn pairing_secret_comparisons_are_constant_time() {
    let src = include_str!("../pair.rs");
    // The one function whose head is `head`, to the line `}` at the
    // indentation of its first line.
    let body = |head: &str| -> &str {
        assert_eq!(src.matches(head).count(), 1, "{head}");
        let start = src.find(head).unwrap();
        let line = src[..start].rfind('\n').map_or(0, |i| i + 1);
        let indent: String = src[line..].chars().take_while(|c| *c == ' ').collect();
        let end = src[start..].find(&format!("\n{indent}}}\n")).unwrap();
        &src[start..start + end]
    };
    let words = |line: &str| -> Vec<String> {
        line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).filter(|w| !w.is_empty()).map(str::to_owned).collect()
    };
    let cases: [(&str, &[&str], &[&str]); 8] = [
        ("pub fn parse_code(", &["typed.as_slice().ct_eq(&want[..])"], &["typed", "want", "check"]),
        ("fn open_slot<", &["want.ct_eq(mac)", "l.ct_eq(&label)", "r.ct_eq(&role)"], &["want", "mac", "l", "r", "label", "role"]),
        ("fn slot_c(", &["commitment(epk_e, r_e).ct_eq(&self.commit)"], &["commitment", "commit", "epk_e", "r_e"]),
        ("fn sas_entered(", &["digits.as_bytes().ct_eq(&sec.sas[..])"], &["digits", "sas"]),
        ("fn check_added(", &["a.transcript.ct_eq(t)"], &["transcript", "t"]),
        ("fn same_slot(", &["accepted.ct_eq(bytes)"], &["accepted", "bytes"]),
        ("fn diffie_hellman(", &["out.ct_eq(&[0u8; 32])"], &["out"]),
        ("fn snapshot(", &["r.commitment(&self.board, *e).ct_eq(recorded)"], &["commitment", "recorded"]),
    ];
    for (head, ct, values) in cases {
        let b = body(head);
        for c in ct {
            assert!(b.contains(c), "{head}: {c}");
        }
        for line in b.lines().filter(|l| l.contains("==") || l.contains("!=")) {
            let w = words(line);
            assert!(!values.iter().any(|v| w.iter().any(|x| x == v)), "{head}: `==` or `!=` on a secret value: {line}");
        }
    }
    // The repeat checks go through `same_slot`, on both machines.
    assert!(body("fn accepted(").contains("same_slot(s, bytes)"));
    assert!(src.contains("if same_slot(&self.accepted_b, &bytes) =>"));
    for line in src.lines().filter(|l| l.contains("==") || l.contains("!=")) {
        let w = words(line);
        assert!(!w.iter().any(|x| x == "accepted_b" || x == "slot_a" || x == "slot_c" || x == "slot_d"), "{line}");
    }
}
