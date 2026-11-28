//! `write_backup` and the epoch roots it is handed (S5, security-1's
//! finding 2): no root it is given is left in a freed heap block.

use taskdeck_seal::*;

use crate::backup::{copy_history, input, nightly};
use crate::board::*;
use crate::freed_with;

/// The roots of `s` and `extra` more of epochs the membership has no
/// commitment for (left out of the file, ruling Q38), each with its bytes.
/// Both vectors get their full capacity up front: a vector that grows frees
/// its old buffer unwiped, and a later allocation of the code under test
/// could reuse those bytes and be reported for them.
fn many_roots(s: &Sim, extra: u32) -> (Vec<(u32, EpochRoot)>, Vec<[u8; 32]>) {
    let n = s.roots.len() + extra as usize;
    let mut v = Vec::with_capacity(n);
    let mut pats = Vec::with_capacity(n);
    for (e, r) in &s.roots {
        v.push((*e, EpochRoot::from_bytes(*r)));
        pats.push(*r);
    }
    for e in 0..extra {
        let mut b = [0u8; 32];
        b[..4].copy_from_slice(&e.to_be_bytes());
        b[4..].copy_from_slice(&s.roots[&1][4..]);
        // Handed in out of order, so that the writer sorts them.
        v.push((1_000_000 - e, EpochRoot::from_bytes(b)));
        pats.push(b);
    }
    (v, pats)
}

// A Vec of roots dropped whole leaves no root behind (the control), and
// neither does `write_backup`: it neither moves a root out of the Vec it is
// handed (whose buffer is then freed without the moved-out elements being
// wiped) nor copies roots into a scratch buffer while sorting them, for the
// roots it keeps and those it leaves out, with one root and with 151 (more than a stable sort sorts on the stack).
// spec: SEAL-GEN-006
#[test]
fn write_backup_frees_no_root_unwiped() {
    let s = Sim::new(5, 2);
    for extra in [0u32, 150] {
        let (v, pats) = many_roots(&s, extra);
        assert!(freed_with(pats, || drop(v)).is_empty(), "control");
        let h = copy_history(&s.history);
        let (v, pats) = many_roots(&s, extra);
        let mut inp = input(&s, &h, nightly(&s));
        inp.roots = v;
        let mut file = None;
        let hits = freed_with(pats, || file = Some(write_backup(&s.devs[0], inp, &mut OsEntropy)));
        assert!(file.unwrap().is_ok());
        assert!(hits.is_empty(), "{} extra roots: freed heap blocks still held roots: (block size, root) {hits:?}", extra);
    }
}
