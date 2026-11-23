//! The padding check of a blob (§20.3, §20.5 step 5) over random and mutated
//! padded bodies: each is sealed here, properly, under the blob key, so that
//! it passes steps 1 to 4 and the padding check runs on bytes the input
//! chose. `open_blob` returns, never panics, and returns the body exactly
//! when the padded bytes are valid as §10.3 checks them (`U = 4 + body_len`),
//! `seal/blob_open` otherwise.

use std::sync::OnceLock;

use proptest::prelude::*;
use taskdeck_seal::*;

use crate::backup::sha;
use crate::blob::*;
use crate::board::*;

struct Fixture {
    sim: Sim,
    writer: DevId,
    epoch: u32,
}

fn fixture() -> &'static Fixture {
    static F: OnceLock<Fixture> = OnceLock::new();
    F.get_or_init(|| {
        let sim = Sim::new(91, 2);
        let writer = sim.devs[0].public().dev;
        let epoch = sim.m.epoch();
        Fixture { sim, writer, epoch }
    })
}

/// The checks of §10.3 for a padded body of §20.3, written here from the
/// spec: `L ≥ 4`, `U = 4 + body_len ≤ L`, `P(U) = L`, zeros from `U` to `L`.
fn valid_body(padded: &[u8]) -> Option<&[u8]> {
    let len = u32::from_be_bytes(padded.get(..4)?.try_into().ok()?);
    let u = 4 + u64::from(len);
    let l = padded.len() as u64;
    if u > l || padded_len(u) != l {
        return None;
    }
    let u = u as usize;
    padded[u..].iter().all(|&b| b == 0).then(|| &padded[4..u])
}

/// A valid padded body changed at `at` by `op`: a byte set, inserted or
/// removed, a cut, the length field set to `x` repeated or moved by one, or
/// left as it is.
fn mutate(mut b: Vec<u8>, at: usize, op: u8, x: u8) -> Vec<u8> {
    let i = at % b.len().max(1);
    match op % 8 {
        0 if !b.is_empty() => b[i] = x,
        1 => b.insert(i.min(b.len()), x),
        2 if !b.is_empty() => {
            b.remove(i);
        }
        3 => b.truncate(i),
        4 => b[..4].copy_from_slice(&[x; 4]),
        5 => {
            let n = u32::from_be_bytes(b[..4].try_into().unwrap());
            b[..4].copy_from_slice(&n.wrapping_add(1).to_be_bytes());
        }
        6 => {
            let n = u32::from_be_bytes(b[..4].try_into().unwrap());
            b[..4].copy_from_slice(&n.wrapping_sub(1).to_be_bytes());
        }
        _ => {}
    }
    b
}

/// Seals `padded` as a blob of kind 2 under the fixture's blob key, opens it
/// with `open_blob`, and checks the result against `valid_body`.
fn open_padded(padded: &[u8]) -> Result<(), TestCaseError> {
    let fx = fixture();
    let s = &fx.sim;
    let raw = Raw::new(2, fx.epoch, s.board, 0, fx.writer, [3; 16], [4; 24]);
    let sealed = raw.seal(&kind_key(&s.board, &s.roots[&fx.epoch], fx.epoch, "blob"), padded);
    let r = BlobRef { sha256: sha(&[&sealed]), ..blob_ref(&sealed, 0, fx.epoch) };
    let got = open_blob(&split_parts(&sealed), &r, &fx.writer, &blob_key(s), &s.board);
    match valid_body(padded) {
        Some(body) => prop_assert_eq!(got, Ok(body.to_vec())),
        None => prop_assert_eq!(got, Err(SealError::BlobOpen)),
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    // Random padded bodies and valid ones mutated (bytes changed, inserted,
    // removed, cut, the length field at its extremes or off by one), each
    // sealed properly: the padding check returns, never panics, refuses
    // every invalid one with `seal/blob_open` and returns the body of every
    // valid one.
    // spec: SEAL-GEN-011
    #[test]
    fn open_blob_never_panics_on_padded_bodies(
        random in proptest::collection::vec(any::<u8>(), 0..2100),
        body in proptest::collection::vec(any::<u8>(), 0..1500),
        at in any::<usize>(), op in any::<u8>(), x in any::<u8>(),
    ) {
        open_padded(&random)?;
        open_padded(&mutate(pad(&body), at, op, x))?;
    }
}
