//! A relay in the middle that knows the code and holds every slot (seal.md
//! §17.5, §17.7, V1-6): it substitutes its own keys towards E, towards N or
//! both, re-computing every MAC. Commit-then-reveal fixes its keys before it
//! sees either side's, so E's and N's digits differ unless it guessed them;
//! N aborts after the third wrong entry; the attack goes through exactly
//! when the digits typed into N are N's own, so the six digits are the whole
//! barrier. E never posts slot C before slot B verified, nor slot D before
//! the user confirmed.

use taskdeck_seal::pair::*;
use taskdeck_seal::*;

use crate::board::*;
use crate::pairing::*;

/// The user types `typed` into N up to three times; N's effects each time.
fn type_three(r: &mut Run, typed: &str) -> Vec<Vec<&'static str>> {
    let mut out = Vec::new();
    for _ in 0..3 {
        let fx = r.n_on(NewEvent::SasEntered { digits: typed.into() });
        out.push(fx.iter().map(n_name).collect());
        if n_abort(&fx).is_some() {
            assert_eq!(n_abort(&fx), Some(SealError::SasMismatch));
        }
    }
    out
}

/// Both sides replaced: towards N the relay plays E (its slot A, committed
/// before it sees N's slot B, then its slot C), towards E it plays N (its own
/// slot B). E's digits are those of E's transcript, N's those of N's: they
/// differ; typed into N, E's digits fail three times and N aborts; N's own
/// digits (which the relay can compute) are the only ones N accepts, and
/// then the relay's bundle opens on N.
// spec: SEAL-PAIR-006
// spec: SEAL-PAIR-007
// spec: SEAL-PAIR-004
#[test]
fn both_sides_replaced() {
    for accept_own in [false, true] {
        let mut r = Run::new(Sim::new(601, 2), 0);
        let board = r.sim.board;
        let dev_e = r.dev_e();
        // Towards N: the relay's slot A, its key fixed now.
        let fe = FakeE::new(&r.k, board, dev_e, rand32(), rand32());
        let a_n = fe.slot_a();
        let a_e = std::mem::replace(&mut r.slot_a, a_n.clone());
        r.step_b();
        let b_n = r.slot_b.clone();
        // Towards E: the relay's slot B, with its own long-term and ephemeral keys.
        let fake_n = FakeN::new(&r.k, rand32());
        r.slot_b = fake_n.slot_b(&[1], NAME);
        r.step_c();
        let digits_e = r.digits.clone();
        let c_e = r.slot_c.clone();
        let (_, _, relay_e) = fake_n.view(&board, &dev_e, &a_e, &r.slot_b, &c_e);
        assert_eq!(relay_e, digits_e, "the relay knows E's digits");
        // Towards N: the opening of the relay's commitment.
        r.slot_c = fe.slot_c();
        r.step_sas();
        let (dh_n, t_n, digits_n) = fe.view(&a_n, &b_n, &r.slot_c);
        assert_ne!(digits_e, digits_n, "the relay guessed (probability 10^-6)");
        if !accept_own {
            let seen = type_three(&mut r, &digits_e);
            assert_eq!(seen, [vec!["AskSas"], vec!["AskSas"], vec!["Abort"]]);
            assert!(r.n_on(NewEvent::SasEntered { digits: digits_n.clone() }).is_empty(), "aborted for good");
        } else {
            let fx = r.n_on(NewEvent::SasEntered { digits: digits_n.clone() });
            assert_eq!(fx.iter().map(n_name).collect::<Vec<_>>(), ["ShowAccepted", "FetchSlot"]);
            // The attack goes through: the relay's own bundle opens on N.
            let snap = SnapAnnounce {
                seq: 9,
                hash: [1; 32],
                hash_version: 1,
                blob: BlobId([2; 16]),
                sha256: [3; 32],
                size: 100,
                parts: 1,
                inflated: 100,
                epoch: 1,
            };
            let json = bundle_json(&board, HOST, &DOOR, &[(1, [4; 32])], Head { seq: 9, chain: Chain([5; 32]) }, &snap, 10, "t");
            let key = bundle_key(&r.k.s, &dh_n, &t_n);
            let d = r.k.slot(D, ROLE_E, &aead_seal(&key, [6; 24], &bundle_ad(&board, &r.k.tid, &t_n), &json));
            let fx = r.n_on(NewEvent::Slot { bytes: d });
            assert_eq!(fx.iter().map(n_name).collect::<Vec<_>>(), ["FetchSnapshot"]);
        }
    }
}

/// Only one side replaced: the digits still differ, because `T` covers the
/// slots each side saw and `dh` the keys each side used.
// spec: SEAL-PAIR-006
// spec: SEAL-PAIR-007
#[test]
fn one_side_replaced() {
    // Towards E only: N's slots A and C are E's; E gets the relay's slot B.
    let mut r = Run::new(Sim::new(602, 1), 0);
    r.step_b();
    let fake_n = FakeN::new(&r.k, rand32());
    let b_n = std::mem::replace(&mut r.slot_b, fake_n.slot_b(&[1], NAME));
    r.step_c();
    r.slot_b = b_n;
    r.step_sas();
    let typed = r.digits.clone();
    let seen = type_three(&mut r, &typed);
    assert_eq!(seen, [vec!["AskSas"], vec!["AskSas"], vec!["Abort"]]);

    // Towards N only: E's slot B is N's, N gets the relay's slots A and C.
    let mut r = Run::new(Sim::new(603, 1), 0);
    let fe = FakeE::new(&r.k, r.sim.board, r.dev_e(), rand32(), rand32());
    let a_e = std::mem::replace(&mut r.slot_a, fe.slot_a());
    r.step_b();
    r.step_c();
    r.slot_c = fe.slot_c();
    r.step_sas();
    let (_, _, digits_n) = fe.view(&r.slot_a, &r.slot_b, &r.slot_c);
    assert_ne!(r.digits, digits_n);
    assert_ne!(a_e, r.slot_a);
    let typed = r.digits.clone();
    let seen = type_three(&mut r, &typed);
    assert_eq!(seen, [vec!["AskSas"], vec!["AskSas"], vec!["Abort"]]);
}

/// Commit, then reveal: a relay that committed to one key in slot A and,
/// having seen N's slot B, opens another in slot C (to steer the digits)
/// fails the commitment, MAC or no MAC; N aborts with
/// `seal/commit_mismatch` and the digits are never asked for.
// spec: SEAL-PAIR-004
#[test]
fn the_relay_cannot_change_its_key_after_slot_b() {
    let (esk, r0) = (rand32(), rand32());
    for (epk, rr) in [(x(rand32(), BASE), r0), (x(esk, BASE), rand32())] {
        let mut q = Run::new(Sim::new(605, 1), 0);
        let fe = FakeE::new(&q.k, q.sim.board, q.dev_e(), esk, r0);
        q.slot_a = fe.slot_a();
        q.step_b();
        let fx = q.n_on(NewEvent::Slot { bytes: q.k.slot(C, ROLE_E, &[epk, rr].concat()) });
        assert_eq!(n_abort(&fx), Some(SealError::CommitMismatch));
        assert!(!fx.iter().any(|f| matches!(f, NewEffect::AskSas)));
    }
    // E's own slot C opens E's own commitment of slot A, which E posted
    // before anything of N's was read.
    let mut r = Run::new(Sim::new(606, 1), 0);
    r.step_b().step_c();
    let epk: [u8; 32] = body(&r.slot_c)[..32].try_into().unwrap();
    let rr: [u8; 32] = body(&r.slot_c)[32..].try_into().unwrap();
    assert_eq!(&body(&r.slot_a)[16..], &commit(&epk, &rr));
}

/// E posts slot C only as the answer to a slot B that verified, and once:
/// a slot B with a bad MAC, another label, the other role, or a ticket of
/// another code gets no slot C (an abort), and after slot C a second slot B
/// (the relay trying another key once it has seen `epk_E`) aborts too.
// spec: SEAL-PAIR-005
// spec: SEAL-PAIR-003
#[test]
fn e_posts_slot_c_only_after_slot_b_verified() {
    type Bad = fn(&Run, Vec<u8>) -> Vec<u8>;
    let bads: Vec<Bad> = vec![
        |_, mut b| {
            let n = b.len();
            b[n - 1] ^= 1;
            b
        },
        |r, b| r.k.slot(C, ROLE_N, body(&b)),
        |r, b| r.k.slot(B, ROLE_E, body(&b)),
        |r, b| Code::from_secret([9; 10], &r.sim.board).slot(B, ROLE_N, body(&b)),
    ];
    for bad in bads {
        let mut r = Run::new(Sim::new(607, 1), 0);
        r.step_b();
        let b = bad(&r, r.slot_b.clone());
        let fx = r.e_on(ExistingEvent::SlotB { bytes: b });
        assert_eq!(fx.iter().map(e_name).collect::<Vec<_>>(), ["DeleteTicket", "Abort"]);
        assert_eq!(e_abort(&fx), Some(SealError::SlotMac));
    }
    let mut r = Run::new(Sim::new(608, 1), 0);
    r.step_b().step_c();
    let fake_n = FakeN::new(&r.k, rand32());
    let fx = r.e_on(ExistingEvent::SlotB { bytes: fake_n.slot_b(&[1], NAME) });
    assert_eq!(fx.iter().map(e_name).collect::<Vec<_>>(), ["DeleteTicket", "Abort"]);
}

/// E seals nothing for N before the user confirmed on E: `Registered`
/// before `UserConfirmed` aborts with no slot D, and E asks its engine for
/// the `DeviceAdded` only after `UserConfirmed`.
// spec: SEAL-PAIR-008
#[test]
fn e_posts_slot_d_only_after_the_user_confirmed() {
    let mut r = Run::new(Sim::new(609, 1), 0);
    r.step_b().step_c().step_sas();
    let fx = r.n_on(NewEvent::SasEntered { digits: r.digits.clone() });
    assert_eq!(fx.iter().map(n_name).collect::<Vec<_>>(), ["ShowAccepted", "FetchSlot"]);
    r.device = Some(DeviceAdded {
        dev: DevId([0; 16]),
        sign_pk: [0; 32],
        hpke_pk: [0; 32],
        name: NAME.into(),
        transcript: [0; 32],
        bootstrap_seq: 0,
    });
    r.take_snapshot();
    r.added_seq = r.sim.seq() + 1;
    let fx = r.e_on(r.registered());
    assert_eq!(fx.iter().map(e_name).collect::<Vec<_>>(), ["DeleteTicket", "Abort"]);
    assert!(r.e_on(ExistingEvent::UserConfirmed).is_empty());

    let mut r = Run::new(Sim::new(609, 1), 0);
    r.step_b().step_c();
    let effects: Vec<&str> = r.start_effects.to_vec();
    assert_eq!(effects, ["RegisterTicket", "ShowCode"]);
    let fx = r.e_on(ExistingEvent::UserConfirmed);
    assert_eq!(fx.iter().map(e_name).collect::<Vec<_>>(), ["NeedSnapshotAndRegister"]);
}
