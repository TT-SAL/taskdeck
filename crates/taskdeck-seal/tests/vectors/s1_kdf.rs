//! `seal-kdf.json` (seal.md §6): recovery keys, `board_id`, `dev_R`, `c_e`,
//! `K_entry,e`, `K_snap,e`, `K_blob,e`, device keys and key fingerprints.
//! `K_file(k)` is task S4's and is added there.

use crate::common::hex;
use crate::{check_or_write, count};
use serde_json::{Value, json};
use taskdeck_seal::{
    BoardId, DeviceKeys, EpochRoot, FixedEntropy, KeyKind, RecoverySeed, board_id, derive_epoch_key,
};

fn recovery(name: &str, seed: Vec<u8>, salt: [u8; 16]) -> Value {
    let s = RecoverySeed::generate(&mut FixedEntropy::new(seed.clone())).unwrap();
    let p = s.keys().public();
    let b = board_id(&p.sign_pk, &salt);
    json!({
        "name": name,
        "entropy": hex(&seed),
        "salt": hex(&salt),
        "rsign_pk": hex(&p.sign_pk),
        "rhpke_pk": hex(&p.hpke_pk),
        "board_id": hex(&b.0),
        "dev_r": hex(&p.dev(&b).0),
        "fingerprint": p.fingerprint(),
    })
}

fn epoch(name: &str, board: [u8; 16], e: u32, root: [u8; 32]) -> Value {
    let b = BoardId(board);
    let r = EpochRoot::from_bytes(root);
    let k = |kind| hex(&*derive_epoch_key(&r, &b, e, kind).expose_for_vectors());
    json!({
        "name": name,
        "board_id": hex(&board),
        "epoch": e,
        "root": hex(&root),
        "commitment": hex(&r.commitment(&b, e)),
        "k_entry": k(KeyKind::Entry),
        "k_snap": k(KeyKind::Snap),
        "k_blob": k(KeyKind::Blob),
    })
}

fn device(name: &str, entropy: Vec<u8>) -> Value {
    let d = DeviceKeys::generate(&mut FixedEntropy::new(entropy.clone())).unwrap();
    let p = d.public();
    json!({
        "name": name,
        "entropy": hex(&entropy),
        "dev": hex(&p.dev.0),
        "sign_pk": hex(&p.sign_pk),
        "hpke_pk": hex(&p.hpke_pk),
        "fingerprint": p.fingerprint(),
        "identity_bytes": hex(&d.to_identity_bytes()),
    })
}

#[test]
fn seal_kdf() {
    let mut cases = vec![
        recovery("recovery keys, seed 00..1f", count(0, 32), [0x5a; 16]),
        recovery("recovery keys, seed of ff", vec![0xff; 32], [0; 16]),
        recovery("recovery keys, seed a0..bf", count(0xa0, 32), *b"taskdeck salt 16"),
        epoch("epoch 1", [0x42; 16], 1, [0x11; 32]),
        epoch("epoch 2", [0x42; 16], 2, [0x11; 32]),
        epoch("epoch 65535, another board", count(0x10, 16).try_into().unwrap(), 65535, count(0x60, 32).try_into().unwrap()),
        epoch("epoch 4294967295", [0x42; 16], u32::MAX, [0xee; 32]),
        device("device keys, entropy 00..4f", count(0, 80)),
        device("device keys, entropy 80..cf", count(0x80, 80)),
    ];
    let err = |e: taskdeck_seal::SealError| e.code();
    let short_seed = count(0, 31);
    cases.push(json!({
        "name": "recovery seed, entropy one byte short",
        "entropy": hex(&short_seed),
        "error": err(RecoverySeed::generate(&mut FixedEntropy::new(short_seed.clone())).err().unwrap()),
    }));
    let short_dev = count(0, 79);
    cases.push(json!({
        "name": "device keys, entropy one byte short",
        "entropy": hex(&short_dev),
        "error": err(DeviceKeys::generate(&mut FixedEntropy::new(short_dev.clone())).err().unwrap()),
    }));
    let ids = DeviceKeys::generate(&mut FixedEntropy::new(count(0, 80))).unwrap().to_identity_bytes().to_vec();
    for (name, bytes) in [
        ("identity bytes, 79 bytes", ids[..79].to_vec()),
        ("identity bytes, 81 bytes", [ids.as_slice(), &[0]].concat()),
        ("identity bytes, X25519 key not clamped", {
            let mut v = ids.clone();
            v[48] |= 0x01;
            v
        }),
    ] {
        cases.push(json!({
            "name": name,
            "identity_bytes": hex(&bytes),
            "error": err(DeviceKeys::from_identity_bytes(&bytes).err().unwrap()),
        }));
    }
    check_or_write("seal-kdf.json", json!({ "spec": "seal.md §6", "cases": cases }));
}
