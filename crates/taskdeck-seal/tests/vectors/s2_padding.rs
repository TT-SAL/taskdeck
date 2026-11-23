//! `seal-padding.json` (seal.md §10.2, §10.3): `P(U)` for the payload
//! lengths §24 names and the examples of §10.2, the frame length
//! `seal_frame` gives for each and its refusal over the cap; then
//! plaintexts, accepted and malformed, with the unpadding check each fails.
//!
//! The accepted plaintexts are opened here (with `hkdf` and
//! `chacha20poly1305`) from frames `seal_frame` sealed; the malformed ones
//! are those with one field changed. The unpadding error is private, so
//! `src/entries/tests.rs` checks every committed plaintext against the
//! crate's unpadding.

use crate::common::hex;
use crate::{check_or_write, count};
use chacha20poly1305::aead::{Aead as _, KeyInit as _, Payload};
use serde_json::{Value, json};
use taskdeck_seal::{
    BoardId, DeviceKeys, EntryId, EpochRoot, FixedEntropy, Head, KeyKind, Kind, OsEntropy, Plain, Signer, chain_zero,
    derive_epoch_key, padded_len, seal_frame,
};

const BOARD: BoardId = BoardId([0x42; 16]);
const ROOT: [u8; 32] = [0x11; 32];
const AT_MS_MAX: i64 = 253_402_300_799_999;

fn plain(kind: Kind, at_ms: i64, payload: Vec<u8>) -> Plain {
    Plain {
        entry_id: EntryId(count(0xe0, 16).try_into().unwrap()),
        kind,
        at_ms,
        based_on: Head { seq: 0, chain: chain_zero(&BOARD) },
        payload,
    }
}

fn seal(p: &Plain) -> Result<Vec<u8>, taskdeck_seal::SealError> {
    let d = DeviceKeys::generate(&mut FixedEntropy::new(count(0x80, 80))).unwrap();
    let key = derive_epoch_key(&EpochRoot::from_bytes(ROOT), &BOARD, 1, KeyKind::Entry);
    seal_frame(&Signer::Device(&d), &BOARD, 1, 1, &key, p, &mut OsEntropy)
}

/// The padded plaintext of a sealed frame, opened with the pinned crates.
fn open(f: &[u8]) -> Vec<u8> {
    let info = [b"taskdeck/v1/key-entry\0".as_slice(), &1u32.to_be_bytes()].concat();
    let mut k = [0u8; 32];
    hkdf::Hkdf::<sha2::Sha256>::new(Some(&BOARD.0), &ROOT).expand(&info, &mut k).unwrap();
    let ad = [b"taskdeck/v1/entry\0".as_slice(), &f[..48], &f[48..54]].concat();
    let c = chacha20poly1305::XChaCha20Poly1305::new_from_slice(&k).unwrap();
    let n = chacha20poly1305::XNonce::try_from(&f[54..78]).unwrap();
    c.decrypt(&n, Payload { msg: &f[78..f.len() - 64], aad: &ad }).unwrap()
}

fn accepted(name: &str, p: &Plain) -> Value {
    let pt = open(&seal(p).unwrap());
    json!({
        "name": name,
        "plaintext": hex(&pt),
        "entry_id": hex(&p.entry_id.0),
        "kind": p.kind as u8,
        "at_ms": p.at_ms,
        "based_on_seq": p.based_on.seq,
        "based_on_chain": hex(&p.based_on.chain.0),
        "payload": hex(&p.payload),
    })
}

fn malformed(name: &str, pt: Vec<u8>, check: u8) -> Value {
    json!({ "name": name, "plaintext": hex(&pt), "class": "Refuse", "error": "seal/bad_plaintext", "check": check })
}

#[test]
fn seal_padding_vectors() {
    let mut cases = Vec::new();
    for n in [0u64, 1, 443, 444, 8123, 8124, 257_979, 257_980] {
        let u = 69 + n;
        let mut c = json!({ "name": format!("payload_len {n}"), "payload_len": n, "unpadded": u, "padded": padded_len(u) });
        if n > 0 {
            // A `cmd` of n bytes 0x61 through seal_frame.
            match seal(&plain(Kind::Cmd, 0, vec![0x61; n as usize])) {
                Ok(f) => c["frame_len"] = json!(f.len()),
                Err(e) => {
                    c["error"] = json!(e.code());
                }
            }
        }
        cases.push(c);
    }
    for u in [600u64, 8192, 8193, 16384, 16385, 20000, 258_048, 258_049] {
        cases.push(json!({ "name": format!("P({u})"), "unpadded": u, "padded": padded_len(u) }));
    }

    let base = plain(Kind::Cmd, 1_767_225_600_000, b"payload".to_vec());
    cases.push(accepted("accepted: a 7-byte cmd", &base));
    cases.push(accepted("accepted: at_ms 253402300799999", &Plain { at_ms: AT_MS_MAX, ..base.clone() }));
    cases.push(accepted("accepted: at_ms 0, kind snap", &Plain { at_ms: 0, kind: Kind::Snap, ..base.clone() }));
    cases.push(accepted("accepted: 443 bytes fill the bucket", &plain(Kind::Beat, 5, vec![0x62; 443])));
    cases.push(accepted("accepted: 444 bytes, 1024 padded", &plain(Kind::Beat, 5, vec![0x62; 444])));

    let pt = open(&seal(&base).unwrap());
    let with = |at: usize, bytes: &[u8]| {
        let mut m = pt.clone();
        m[at..at + bytes.len()].copy_from_slice(bytes);
        m
    };
    cases.push(malformed("check 1: 68 bytes", pt[..68].to_vec(), 1));
    cases.push(malformed("check 1: empty", vec![], 1));
    cases.push(malformed("check 2: payload_len one past the plaintext", with(65, &(512u32 - 68).to_be_bytes()), 2));
    cases.push(malformed("check 2: payload_len 4294967295", with(65, &u32::MAX.to_be_bytes()), 2));
    cases.push(malformed("check 3: 1024 bytes for a 7-byte payload", [pt.as_slice(), &[0; 512]].concat(), 3));
    cases.push(malformed("check 3: 511 bytes", pt[..511].to_vec(), 3));
    cases.push(malformed("check 4: the last byte not zero", with(511, &[1]), 4));
    cases.push(malformed("check 4: the first padding byte not zero", with(76, &[0x80]), 4));
    cases.push(malformed("check 5: kind 0", with(16, &[0]), 5));
    cases.push(malformed("check 5: kind 5", with(16, &[5]), 5));
    cases.push(malformed("check 5: at_ms 253402300800000", with(17, &(AT_MS_MAX + 1).to_be_bytes()), 5));
    cases.push(malformed("check 5: at_ms -1", with(17, &(-1i64).to_be_bytes()), 5));
    let mut both = with(16, &[0]);
    both[65..69].copy_from_slice(&u32::MAX.to_be_bytes());
    cases.push(malformed("checks 2 and 5 fail: 2 is reported", both, 2));
    check_or_write("seal-padding.json", json!({ "spec": "seal.md §10.2", "cases": cases }));
}
