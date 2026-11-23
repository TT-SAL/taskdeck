//! The pairing code and the SAS digits the crate is handed as text (S6,
//! security-2's finding): `NewEvent` takes them by value, so once `New::on`
//! has them the crate holds the only copy, and it erases that copy before
//! freeing it (seal.md §17.11), on every path; so does `Existing::start`
//! with the code it made when it aborts before handing it out.

use taskdeck_seal::pair::*;
use taskdeck_seal::*;

use crate::board::*;
use crate::freed_with_bytes;
use crate::pairing::*;

/// N handles `ev` with the scan armed for `typed`: its effects and the
/// freed blocks that still held `typed`.
fn n_on(r: &mut Run, ev: NewEvent, typed: &str) -> (Vec<&'static str>, Option<SealError>, Vec<(usize, usize)>) {
    let pat = typed.as_bytes().to_vec();
    let mut fx = None;
    let hits = freed_with_bytes(vec![pat], || fx = Some(r.n_on(ev)));
    let fx = fx.unwrap();
    (fx.iter().map(n_name).collect(), n_abort(&fx), hits)
}

fn code_event(r: &Run, code: &str, host: &str, name: &str) -> NewEvent {
    NewEvent::CodeEntered {
        code: code.into(),
        host: host.into(),
        board_id: r.sim.board,
        door: DOOR,
        name: name.into(),
        now: PT0 + 1,
    }
}

// The typed code and the typed digits leave no freed heap block holding
// them, whatever N does with them: the code accepted (`FetchSlot`), asked
// again for its check, its host or its name, or arriving in a state that
// aborts or ignores it; the digits accepted, refused, refused a third time
// (the abort), or arriving in a state that aborts. A String holding the code
// dropped as it is (the control) is found.
// spec: SEAL-PAIR-014
// spec: SEAL-GEN-006
#[test]
fn n_frees_no_typed_code_or_digits_unwiped() {
    let mut r = Run::new(Sim::new(601, 2), 0);
    let code = r.code.clone();
    // The control.
    let s = code.clone();
    assert!(!freed_with_bytes(vec![code.as_bytes().to_vec()], || drop(s)).is_empty(), "control");

    // Asked again: the check, the host, the name.
    let mut bad = code.clone();
    let last = if bad.ends_with('0') { '1' } else { '0' };
    bad.pop();
    bad.push(last);
    let cases = [
        (bad.as_str(), HOST, NAME, SealError::CodeCheck),
        (code.as_str(), "Bad Host", NAME, SealError::HostFormat),
        (code.as_str(), HOST, "", SealError::SlotFormat),
    ];
    for (typed, host, name, want) in cases {
        let ev = code_event(&r, typed, host, name);
        let (names, _, hits) = n_on(&mut r, ev, typed);
        assert_eq!(names, ["AskAgain"], "{want:?}");
        assert!(hits.is_empty(), "AskAgain {want:?}: freed blocks held the code: {hits:?}");
    }
    // Accepted.
    let ev = code_event(&r, &code, HOST, NAME);
    let (names, _, hits) = n_on(&mut r, ev, &code);
    assert_eq!(names, ["FetchSlot"]);
    assert!(hits.is_empty(), "FetchSlot: freed blocks held the code: {hits:?}");

    // The digits: E's, and two wrong entries before them.
    r.n = New::new();
    r.step_b().step_c().step_sas();
    let digits = r.digits.clone();
    for typed in ["wrong-digits-typed-1", "wrong-digits-typed-2"] {
        let (names, _, hits) = n_on(&mut r, NewEvent::SasEntered { digits: typed.into() }, typed);
        assert_eq!(names, ["AskSas"]);
        assert!(hits.is_empty(), "AskSas: freed blocks held the digits: {hits:?}");
    }
    let (names, _, hits) = n_on(&mut r, NewEvent::SasEntered { digits: digits.clone() }, &digits);
    assert_eq!(names, ["ShowAccepted", "FetchSlot"]);
    assert!(hits.is_empty(), "accepted: freed blocks held the digits: {hits:?}");

    // A code in a state with no row for it: the abort; then in `Aborted`.
    let ev = code_event(&r, &code, HOST, NAME);
    let (_, abort, hits) = n_on(&mut r, ev, &code);
    assert_eq!(abort, Some(SealError::PairCancelled));
    assert!(hits.is_empty(), "abort: freed blocks held the code: {hits:?}");
    let ev = code_event(&r, &code, HOST, NAME);
    let (names, _, hits) = n_on(&mut r, ev, &code);
    assert!(names.is_empty());
    assert!(hits.is_empty(), "Aborted: freed blocks held the code: {hits:?}");
    let typed = "digits-typed-when-aborted";
    let (_, _, hits) = n_on(&mut r, NewEvent::SasEntered { digits: typed.into() }, typed);
    assert!(hits.is_empty(), "Aborted: freed blocks held the digits: {hits:?}");

    // The third wrong entry (the abort), and digits in `Idle`.
    let mut r = Run::new(Sim::new(602, 2), 0);
    r.step_b().step_c().step_sas();
    for (i, typed) in ["wrong-digits-typed-a", "wrong-digits-typed-b", "wrong-digits-typed-c"].into_iter().enumerate() {
        let (_, abort, hits) = n_on(&mut r, NewEvent::SasEntered { digits: typed.into() }, typed);
        assert_eq!(abort, (i == 2).then_some(SealError::SasMismatch));
        assert!(hits.is_empty(), "entry {i}: freed blocks held the digits: {hits:?}");
    }
    r.n = New::new();
    let typed = "digits-typed-in-idle";
    let (_, abort, hits) = n_on(&mut r, NewEvent::SasEntered { digits: typed.into() }, typed);
    assert_eq!(abort, Some(SealError::PairCancelled));
    assert!(hits.is_empty(), "Idle: freed blocks held the digits: {hits:?}");
}

// `Existing::start` whose entropy fails after `s` (no `esk_E`): it aborts
// with `seal/entropy`, and the code text it had made from `s` is erased
// before it is freed, as is `s`.
// spec: SEAL-PAIR-014
// spec: SEAL-GEN-006
#[cfg(feature = "test-entropy")]
#[test]
fn e_start_frees_no_code_unwiped_when_it_aborts() {
    let sim = Sim::new(603, 2);
    let own = sim.devs[0].public().dev;
    let s = [0x5au8, 1, 2, 3, 4, 5, 6, 7, 8, 9];
    let (_, code) = new_code(&mut FixedEntropy::new(s.to_vec())).unwrap();
    let mut rng = FixedEntropy::new(s.to_vec());
    let mut out = None;
    let hits = freed_with_bytes(vec![code.as_bytes().to_vec()], || {
        out = Some(Existing::start(&sim.m, sim.head(), ORIGIN, &own, PT0, &mut rng))
    });
    let (_, fx) = out.unwrap();
    assert!(matches!(fx[..], [ExistingEffect::Abort { code: SealError::Entropy }]));
    assert!(hits.is_empty(), "freed blocks held the code: {hits:?}");
}
