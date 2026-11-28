//! Flipping any byte of any pairing slot aborts the machine that reads it
//! (seal.md §17.4, §17.9): slots A to D as E and N post them, and the
//! bundle inside slot D, re-MACed by a relay that knows the code.

use taskdeck_seal::pair::*;
use taskdeck_seal::*;

use crate::board::*;
use crate::pairing::*;

/// One attempt on `sim` with fresh entropy, driven as far as needed.
struct Flow {
    e: Existing,
    n: New,
    k: Code,
    a: Vec<u8>,
}

fn start(sim: &Sim) -> Flow {
    let own = sim.devs[0].public().dev;
    let (e, fx) = Existing::start(&sim.m, sim.head(), ORIGIN, &own, PT0, &mut OsEntropy);
    let (ExistingEffect::RegisterTicket { slot_a, .. }, ExistingEffect::ShowCode { code }) = (&fx[0], &fx[1]) else {
        panic!("start")
    };
    let mut n = New::new();
    let ev = NewEvent::CodeEntered { code: code.clone(), host: HOST.into(), board_id: sim.board, door: DOOR, name: NAME.into(), now: PT0 };
    n.on(ev, &mut OsEntropy);
    Flow { e, n, k: Code::of(code, &sim.board), a: slot_a.clone() }
}

impl Flow {
    fn b(&mut self) -> Vec<u8> {
        let fx = self.n.on(NewEvent::Slot { bytes: self.a.clone() }, &mut OsEntropy);
        let NewEffect::PostSlot { slot } = &fx[0] else { panic!("slot B") };
        slot.clone()
    }
    /// Slot C and E's digits.
    fn c(&mut self) -> (Vec<u8>, String) {
        let b = self.b();
        let fx = self.e.on(ExistingEvent::SlotB { bytes: b }, &mut OsEntropy);
        let (ExistingEffect::PostSlot { slot }, ExistingEffect::ShowSas { digits }) = (&fx[0], &fx[1]) else { panic!("slot C") };
        (slot.clone(), digits.clone())
    }
    fn d(&mut self, sim: &Sim) -> Vec<u8> {
        let (c, digits) = self.c();
        assert_eq!(self.n.on(NewEvent::Slot { bytes: c }, &mut OsEntropy).len(), 1);
        assert_eq!(self.n.on(NewEvent::SasEntered { digits }, &mut OsEntropy).len(), 2);
        self.e.on(ExistingEvent::UserConfirmed, &mut OsEntropy);
        let h = sim.seq();
        let ev = ExistingEvent::Registered {
            h,
            chain: sim.head().chain,
            snapshot: SnapAnnounce {
                seq: h,
                hash: [1; 32],
                hash_version: 1,
                blob: BlobId([2; 16]),
                sha256: [3; 32],
                size: 4_000,
                parts: 1,
                inflated: 9_000,
                epoch: 1,
            },
            added_seq: h + 1,
            token: TOKEN.into(),
            roots: vec![(1, EpochRoot::from_bytes(sim.roots[&1]))],
            host: HOST.into(),
            door: DOOR,
        };
        let fx = self.e.on(ev, &mut OsEntropy);
        let ExistingEffect::PostSlot { slot } = &fx[0] else { panic!("slot D") };
        slot.clone()
    }
}

fn flip(v: &[u8], i: usize) -> Vec<u8> {
    let mut v = v.to_vec();
    v[i] ^= 0x01;
    v
}

// Every byte of slot A, as N reads it: `seal/slot_mac` (or the layout).
// spec: SEAL-PAIR-003
#[test]
fn every_byte_of_slot_a() {
    let sim = Sim::new(700, 1);
    let f = start(&sim);
    let fresh_n = || {
        let mut n = New::new();
        let code = code_text(&f.k.s);
        n.on(NewEvent::CodeEntered { code, host: HOST.into(), board_id: sim.board, door: DOOR, name: NAME.into(), now: PT0 }, &mut OsEntropy);
        n
    };
    let mut n = fresh_n();
    assert_eq!(n_name(&n.on(NewEvent::Slot { bytes: f.a.clone() }, &mut OsEntropy)[0]), "PostSlot");
    for i in 0..f.a.len() {
        let mut n = fresh_n();
        let fx = n.on(NewEvent::Slot { bytes: flip(&f.a, i) }, &mut OsEntropy);
        assert_eq!(n_abort(&fx), Some(SealError::SlotMac), "byte {i}");
    }
}

// Every byte of slot B, as E reads it.
// spec: SEAL-PAIR-003
#[test]
fn every_byte_of_slot_b() {
    let sim = Sim::new(701, 1);
    let len = start(&sim).b().len();
    for i in 0..len {
        let mut f = start(&sim);
        let b = f.b();
        let fx = f.e.on(ExistingEvent::SlotB { bytes: flip(&b, i) }, &mut OsEntropy);
        assert_eq!(e_abort(&fx), Some(SealError::SlotMac), "byte {i}");
        assert!(!fx.iter().any(|x| matches!(x, ExistingEffect::PostSlot { .. })));
    }
}

// Every byte of slot C, as N reads it.
// spec: SEAL-PAIR-003
// spec: SEAL-PAIR-004
#[test]
fn every_byte_of_slot_c() {
    let sim = Sim::new(702, 1);
    for i in 0..(36 + 64) {
        let mut f = start(&sim);
        let (c, _) = f.c();
        let fx = f.n.on(NewEvent::Slot { bytes: flip(&c, i) }, &mut OsEntropy);
        assert_eq!(n_abort(&fx), Some(SealError::SlotMac), "byte {i}");
        // Re-MACed by a relay that knows the code: the commitment fails.
        if (4..68).contains(&i) {
            let mut f = start(&sim);
            let (c, _) = f.c();
            let fx = f.n.on(NewEvent::Slot { bytes: f.k.remac(&flip(&c, i)) }, &mut OsEntropy);
            assert_eq!(n_abort(&fx), Some(SealError::CommitMismatch), "byte {i}");
        }
    }
}

// Every byte of slot D, as N reads it (`seal/slot_mac`), and every byte of
// its body re-MACed by a relay that knows the code (`seal/bundle_open`):
// the bundle's nonce, ciphertext and tag.
// spec: SEAL-PAIR-003
// spec: SEAL-PAIR-009
#[test]
fn every_byte_of_slot_d_and_the_bundle() {
    let sim = Sim::new(703, 1);
    let len = start(&sim).d(&sim).len();
    for i in 0..len {
        let mut f = start(&sim);
        let d = f.d(&sim);
        assert_eq!(d.len(), len);
        let fx = f.n.on(NewEvent::Slot { bytes: flip(&d, i) }, &mut OsEntropy);
        assert_eq!(n_abort(&fx), Some(SealError::SlotMac), "byte {i}");
        if (4..len - 32).contains(&i) {
            let mut f = start(&sim);
            let d = f.d(&sim);
            let fx = f.n.on(NewEvent::Slot { bytes: f.k.remac(&flip(&d, i)) }, &mut OsEntropy);
            assert_eq!(n_abort(&fx), Some(SealError::BundleOpen), "byte {i}");
        }
    }
    let mut f = start(&sim);
    let d = f.d(&sim);
    assert_eq!(n_name(&f.n.on(NewEvent::Slot { bytes: d }, &mut OsEntropy)[0]), "FetchSnapshot");
}
