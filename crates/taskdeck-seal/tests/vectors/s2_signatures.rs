//! `seal-signatures.json` (seal.md §16.1, §16.2): login messages and
//! signatures, every operation of the admin registry with its `args`,
//! `args_hash`, message and signature, and the refusals: 31- and 33-byte
//! relay nonces, an origin or host outside §5.4, arguments the registry
//! rules out. Each signature is checked here with `ed25519-dalek`.

use crate::common::hex;
use crate::{check_or_write, count};
use serde_json::{Value, json};
use taskdeck_seal::{
    AdminOp, BlobId, BoardId, Chain, CompactArgs, DevId, DeviceKeys, FixedEntropy, FrameHash, RecoveryKeys, RecoverySeed,
    SeedArgs, SeedEntry, Signer, admin_message, login_message, sign_admin, sign_login,
};

const BOARD: BoardId = BoardId([0x42; 16]);

fn verify(pk: &[u8; 32], msg: &[u8], sig: &[u8; 64]) {
    ed25519_dalek::VerifyingKey::from_bytes(pk).unwrap().verify_strict(msg, &ed25519_dalek::Signature::from_bytes(sig)).unwrap();
}

fn sha(b: &[u8]) -> [u8; 32] {
    <sha2::Sha256 as sha2::Digest>::digest(b).into()
}

fn login(name: &str, d: &DeviceKeys, entropy: &[u8], origin: &str, generation: u32, nonce: &[u8], client: &[u8]) -> Value {
    let mut c = json!({
        "name": name,
        "device_entropy": hex(entropy),
        "dev": hex(&d.public().dev.0),
        "sign_pk": hex(&d.public().sign_pk),
        "origin": origin,
        "board_id": hex(&BOARD.0),
        "gen": generation,
        "relay_nonce": hex(nonce),
        "entropy": hex(client),
    });
    match sign_login(d, origin, &BOARD, generation, nonce, &mut FixedEntropy::new(client.to_vec())) {
        Ok((cn, sig)) => {
            let m = login_message(origin, &BOARD, &d.public().dev, generation, nonce, &cn).unwrap();
            verify(&d.public().sign_pk, &m, &sig);
            assert_eq!(cn.as_slice(), client);
            c["client_nonce"] = json!(hex(&cn));
            c["message"] = json!(hex(&m));
            c["sig"] = json!(hex(&sig));
        }
        Err(e) => c["error"] = json!(e.code()),
    }
    c
}

/// The operation's fields as JSON, for another implementation to rebuild it.
fn op_fields(op: &AdminOp) -> Value {
    let wraps = |w: &[[u8; 96]]| w.iter().map(|x| hex(x)).collect::<Vec<_>>();
    match op {
        AdminOp::Register { dev, sign_pk, frame_hash } | AdminOp::RecoveryRegister { dev, sign_pk, frame_hash } => {
            json!({ "dev": hex(&dev.0), "sign_pk": hex(sign_pk), "frame_hash": hex(&frame_hash.0) })
        }
        AdminOp::Revoke(dev) | AdminOp::RevokeReadToken(dev) => json!({ "dev": hex(&dev.0) }),
        AdminOp::Reinstate { dev, first_epoch, wraps: w } => {
            json!({ "dev": hex(&dev.0), "first_epoch": first_epoch, "wraps": wraps(w) })
        }
        AdminOp::Fence { host, door, home_gen } => json!({ "host": host, "door": hex(door), "home_gen": home_gen }),
        AdminOp::Seed(s) => json!({
            "home_gen": s.home_gen, "first_seq": s.first_seq, "chain_before": hex(&s.chain_before.0),
            "last_seq": s.last_seq, "chain_last": hex(&s.chain_last.0), "blob_id": hex(&s.blob_id.0),
            "entries": s.entries.iter().map(|e| json!({
                "dev": hex(&e.dev.0), "sign_pk": hex(&e.sign_pk), "role": e.role, "status": e.status, "last_ctr": e.last_ctr,
            })).collect::<Vec<_>>(),
        }),
        AdminOp::Compact(c) => json!({
            "below_seq": c.below_seq, "snap_entry_seq": c.snap_entry_seq, "blob_id": hex(&c.blob_id.0), "keep_seqs": c.keep_seqs,
        }),
        AdminOp::RestoreTail { first_seq, frames } => {
            json!({ "first_seq": first_seq, "frames": frames.iter().map(|f| hex(&f.0)).collect::<Vec<_>>() })
        }
        AdminOp::Purge { before_seq, epoch } => json!({ "before_seq": before_seq, "epoch": epoch }),
        AdminOp::MintReadToken { dev, epoch } => json!({ "dev": hex(&dev.0), "epoch": epoch }),
    }
}

enum Who<'a> {
    Device(&'a DeviceKeys, &'a [u8]),
    Recovery(&'a RecoveryKeys, &'a [u8]),
}

fn admin(name: &str, who: &Who, op: &AdminOp, nonce: &[u8]) -> Value {
    let (signer, dev, pk) = match who {
        Who::Device(d, _) => (Signer::Device(d), d.public().dev, d.public().sign_pk),
        Who::Recovery(r, _) => (Signer::Recovery(r, BOARD), r.public().dev(&BOARD), r.public().sign_pk),
    };
    let mut c = json!({
        "name": name,
        "op": op.name(),
        "fields": op_fields(op),
        "board_id": hex(&BOARD.0),
        "signer": hex(&dev.0),
        "sign_pk": hex(&pk),
        "relay_nonce": hex(nonce),
    });
    match who {
        Who::Device(_, e) => c["signer_entropy"] = json!(hex(e)),
        Who::Recovery(_, s) => c["signer_seed"] = json!(hex(s)),
    }
    match sign_admin(&signer, op, &BOARD, nonce) {
        Ok(sig) => {
            let args = op.args();
            let m = admin_message(op, &BOARD, &dev, nonce).unwrap();
            verify(&pk, &m, &sig);
            c["args"] = json!(hex(&args));
            c["args_hash"] = json!(hex(&sha(&args)));
            c["message"] = json!(hex(&m));
            c["sig"] = json!(hex(&sig));
        }
        Err(e) => c["error"] = json!(e.code()),
    }
    c
}

fn wrap_to(dev: &DevId, fill: u8) -> [u8; 96] {
    let mut w = [fill; 96];
    w[..16].copy_from_slice(&dev.0);
    w
}

#[test]
fn seal_signature_vectors() {
    let de = count(0x80, 80);
    let d = DeviceKeys::generate(&mut FixedEntropy::new(de.clone())).unwrap();
    let seed = count(0, 32);
    let rk = RecoverySeed::generate(&mut FixedEntropy::new(seed.clone())).unwrap().keys();
    let nonce = count(0x40, 32);
    let client = count(0x60, 32);
    let mut cases = vec![
        login("login", &d, &de, "https://relay.example.org", 1, &nonce, &client),
        login("login, a port and the largest gen", &d, &de, "https://relay.example.org:8443", u32::MAX, &nonce, &client),
        login("login, relay nonce of 31 bytes", &d, &de, "https://relay.example.org", 1, &nonce[..31], &client),
        login("login, relay nonce of 33 bytes", &d, &de, "https://relay.example.org", 1, &[nonce.as_slice(), &[0]].concat(), &client),
        login("login, origin with the default port", &d, &de, "https://relay.example.org:443", 1, &nonce, &client),
        login("login, origin with a trailing slash", &d, &de, "https://relay.example.org/", 1, &nonce, &client),
        login("login, client nonce entropy one byte short", &d, &de, "https://relay.example.org", 1, &nonce, &client[..31]),
    ];

    let dev_n = DevId(count(0xd0, 16).try_into().unwrap());
    let pk_n: [u8; 32] = count(0x10, 32).try_into().unwrap();
    let fh = FrameHash(count(0xf0, 16).repeat(2).try_into().unwrap());
    let dw = Who::Device(&d, &de);
    let rw = Who::Recovery(&rk, &seed);
    let ops: Vec<(&str, &Who, AdminOp)> = vec![
        ("register", &dw, AdminOp::Register { dev: dev_n, sign_pk: pk_n, frame_hash: fh }),
        ("revoke", &dw, AdminOp::Revoke(dev_n)),
        (
            "reinstate, by the recovery identity",
            &rw,
            AdminOp::Reinstate { dev: dev_n, first_epoch: 4, wraps: vec![wrap_to(&dev_n, 0x01), wrap_to(&dev_n, 0x02)] },
        ),
        ("recovery_register", &rw, AdminOp::RecoveryRegister { dev: rk.public().dev(&BOARD), sign_pk: rk.public().sign_pk, frame_hash: fh }),
        ("fence", &dw, AdminOp::Fence { host: "new-relay.example.net".into(), door: [1, 2, 3, 4, 5], home_gen: 2 }),
        (
            "seed",
            &dw,
            AdminOp::Seed(SeedArgs {
                home_gen: 2,
                first_seq: 1,
                chain_before: Chain(count(0xc0, 32).try_into().unwrap()),
                last_seq: 40,
                chain_last: Chain(count(0xc8, 32).try_into().unwrap()),
                blob_id: BlobId([0; 16]),
                entries: vec![
                    SeedEntry { dev: d.public().dev, sign_pk: d.public().sign_pk, role: 1, status: 1, last_ctr: 30 },
                    SeedEntry { dev: rk.public().dev(&BOARD), sign_pk: rk.public().sign_pk, role: 2, status: 1, last_ctr: 1 },
                    SeedEntry { dev: dev_n, sign_pk: pk_n, role: 1, status: 2, last_ctr: 9 },
                ],
            }),
        ),
        (
            "compact",
            &dw,
            AdminOp::Compact(CompactArgs { below_seq: 20_000, snap_entry_seq: 19_968, blob_id: BlobId(count(0xb0, 16).try_into().unwrap()), keep_seqs: vec![1, 2, 5000] }),
        ),
        ("restore_tail", &dw, AdminOp::RestoreTail { first_seq: 41, frames: vec![fh, FrameHash([0xee; 32])] }),
        ("purge", &dw, AdminOp::Purge { before_seq: 100, epoch: 5 }),
        ("mint_read_token", &dw, AdminOp::MintReadToken { dev: d.public().dev, epoch: 3 }),
        ("mint_read_token, by the recovery tool", &rw, AdminOp::MintReadToken { dev: rk.public().dev(&BOARD), epoch: 1 }),
        ("revoke_read_token", &dw, AdminOp::RevokeReadToken(d.public().dev)),
    ];
    for (name, who, op) in &ops {
        cases.push(admin(name, who, op, &nonce));
    }
    let revoke = AdminOp::Revoke(dev_n);
    cases.push(admin("admin, relay nonce of 31 bytes", &dw, &revoke, &nonce[..31]));
    cases.push(admin("admin, relay nonce of 33 bytes", &dw, &revoke, &[nonce.as_slice(), &[0]].concat()));
    let fence443 = AdminOp::Fence { host: "new-relay.example.net:443".into(), door: [0; 5], home_gen: 2 };
    cases.push(admin("fence, host with the default port", &dw, &fence443, &nonce));
    let no_wraps = AdminOp::Reinstate { dev: dev_n, first_epoch: 4, wraps: vec![] };
    cases.push(admin("reinstate, no wraps", &rw, &no_wraps, &nonce));
    check_or_write("seal-signatures.json", json!({ "spec": "seal.md §16", "cases": cases }));
}
