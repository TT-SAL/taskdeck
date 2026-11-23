//! Slots delivered twice (task S7b, ruling Q60, seal.md §17.10, §17.11): a
//! relay answers a repeated read with the stored slot, and a runtime that
//! polls and listens can deliver one twice. A slot that is byte for byte one
//! the machine has already accepted is ignored in every later state, and the
//! pairing completes; any other slot its state does not await, a repeat
//! with one byte changed included, aborts with `seal/slot_mac`. A machine
//! that is `Aborted` still ignores everything.

use taskdeck_seal::pair::*;
use taskdeck_seal::*;

use crate::board::*;
use crate::pairing::*;

fn names_n(fx: &[NewEffect]) -> Vec<&'static str> {
    fx.iter().map(n_name).collect()
}

fn names_e(fx: &[ExistingEffect]) -> Vec<&'static str> {
    fx.iter().map(e_name).collect()
}

/// The slot with one byte changed: a byte of its body, or its MAC's last.
fn changed(slot: &[u8], at_end: bool) -> Vec<u8> {
    let mut s = slot.to_vec();
    let i = if at_end { s.len() - 1 } else { 4 };
    s[i] ^= 1;
    s
}

/// N's states after `Idle` and `AwaitA`, in order.
const N_STATES: [&str; 5] = ["AwaitC", "AwaitSas", "AwaitD", "AwaitSnapshot", "Joining"];

/// A pairing run to N's state `N_STATES[k]`, and the slots N has accepted
/// there (A, then C, then D).
fn n_at(seed: u64, k: usize) -> (Run, Vec<Vec<u8>>) {
    let mut r = Run::new(Sim::new(seed, 1), 0);
    r.step_b();
    let mut acc = vec![r.slot_a.clone()];
    if k >= 1 {
        r.step_c().step_sas();
        acc.push(r.slot_c.clone());
    }
    if k >= 2 {
        r.step_confirmed();
    }
    if k >= 3 {
        r.step_d();
        let fx = r.send_d();
        assert_eq!(names_n(&fx), ["FetchSnapshot"], "{:?}", n_abort(&fx));
        acc.push(r.slot_d.clone());
    }
    if k >= 4 {
        let fx = r.send_snapshot(None);
        assert_eq!(names_n(&fx), ["ReadLog"], "{:?}", n_abort(&fx));
    }
    (r, acc)
}

/// Delivers each slot to N again: nothing happens.
fn ignored_by_n(r: &mut Run, slots: &[Vec<u8>], at: &str) {
    for (i, s) in slots.iter().enumerate() {
        let fx = r.n_on(NewEvent::Slot { bytes: s.clone() });
        assert!(fx.is_empty(), "{at}: accepted slot {i} again: {:?} {:?}", names_n(&fx), n_abort(&fx));
    }
}

// E ignores slot B, byte for byte as it accepted it, while showing,
// registering and done; the pairing then completes. In `Showing` and
// `Registering`, slot B with one byte changed aborts with `seal/slot_mac`
// (`DeleteTicket`, `Abort`), and the aborted machine ignores slot B again.
// spec: SEAL-PAIR-003
// spec: SEAL-PAIR-014
#[test]
fn e_ignores_slot_b_again() {
    let mut r = Run::new(Sim::new(720, 1), 0);
    r.step_b().step_c();
    let b = r.slot_b.clone();
    assert!(r.e_on(ExistingEvent::SlotB { bytes: b.clone() }).is_empty(), "showing");
    r.step_sas().step_confirmed();
    assert!(r.e_on(ExistingEvent::SlotB { bytes: b.clone() }).is_empty(), "registering");
    r.step_d();
    assert!(r.e_on(ExistingEvent::SlotB { bytes: b.clone() }).is_empty(), "done");
    r.step_joining();
    let fx = r.send_log();
    assert_eq!(names_n(&fx), ["Joined"], "{:?}", n_abort(&fx));

    // One byte changed, in `Showing` and in `Registering`.
    for registering in [false, true] {
        for at_end in [false, true] {
            let mut r = Run::new(Sim::new(721, 1), 0);
            r.step_b().step_c();
            if registering {
                r.step_sas().step_confirmed();
            }
            let fx = r.e_on(ExistingEvent::SlotB { bytes: changed(&r.slot_b, at_end) });
            assert_eq!(names_e(&fx), ["DeleteTicket", "Abort"], "registering {registering}, at_end {at_end}");
            assert_eq!(e_abort(&fx), Some(SealError::SlotMac));
            let b = r.slot_b.clone();
            assert!(r.e_on(ExistingEvent::SlotB { bytes: b }).is_empty(), "aborted");
            assert!(r.e_on(ExistingEvent::UserConfirmed).is_empty(), "aborted");
        }
    }
}

// N ignores each of slots A, C and D, byte for byte as it accepted them, in
// every state after it accepted them (A in `AwaitC` to `Joining`, C in
// `AwaitSas` to `Joining`, D in `AwaitSnapshot` and `Joining`) and once
// done; the pairing then completes, so nothing changed.
// spec: SEAL-PAIR-003
#[test]
fn n_ignores_accepted_slots_again() {
    let (mut r, a) = n_at(730, 0);
    let a = a[0].clone();
    ignored_by_n(&mut r, std::slice::from_ref(&a), "AwaitC");
    r.step_c().step_sas();
    let c = r.slot_c.clone();
    ignored_by_n(&mut r, &[a.clone(), c.clone()], "AwaitSas");
    r.step_confirmed();
    ignored_by_n(&mut r, &[a.clone(), c.clone()], "AwaitD");
    r.step_d();
    let fx = r.send_d();
    assert_eq!(names_n(&fx), ["FetchSnapshot"]);
    let d = r.slot_d.clone();
    ignored_by_n(&mut r, &[a.clone(), c.clone(), d.clone()], "AwaitSnapshot");
    let fx = r.send_snapshot(None);
    assert_eq!(names_n(&fx), ["ReadLog"]);
    ignored_by_n(&mut r, &[a.clone(), c.clone(), d.clone()], "Joining");
    let fx = r.send_log();
    assert_eq!(names_n(&fx), ["Joined"], "{:?}", n_abort(&fx));
    ignored_by_n(&mut r, &[a, c, d], "Done");
    // The same in every state on a run where each repeat comes twice in a
    // row, every accepted slot in each state.
    for (k, state) in N_STATES.iter().enumerate() {
        let (mut r, acc) = n_at(731, k);
        ignored_by_n(&mut r, &acc, state);
        ignored_by_n(&mut r, &acc, state);
    }
}

// A repeat of an accepted slot with one byte changed (in its body, or in
// its MAC) is not the slot accepted: in every later state it aborts with
// `seal/slot_mac` and nothing else, and the aborted machine ignores the
// slot as accepted, and anything else, afterwards.
// spec: SEAL-PAIR-003
// spec: SEAL-PAIR-014
#[test]
fn n_aborts_on_a_changed_repeat() {
    for (k, state) in N_STATES.iter().enumerate() {
        let (_, acc) = n_at(740, k);
        for i in 0..acc.len() {
            for at_end in [false, true] {
                let (mut r, acc) = n_at(740, k);
                let fx = r.n_on(NewEvent::Slot { bytes: changed(&acc[i], at_end) });
                assert_eq!(
                    (names_n(&fx), n_abort(&fx)),
                    (vec!["Abort"], Some(SealError::SlotMac)),
                    "{state}: slot {i} changed (at_end {at_end})"
                );
                ignored_by_n(&mut r, &acc, "Aborted");
                assert!(r.n_on(NewEvent::Cancel).is_empty());
            }
        }
    }
}
