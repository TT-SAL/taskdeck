use super::*;
use crate::{
    BoardId, CtlItem, DeviceAdded, EntryId, EpochRoot, Head, KeyKind, Kind, OsEntropy, Plain, RecoverySeed, chain_zero,
    ctl_payload, derive_epoch_key, frame_hash, seal_frame,
};

const BOARD: BoardId = BoardId([0xB0; 16]);
const NONCE: [u8; 32] = [0x4E; 32];

fn sha(b: &[u8]) -> [u8; 32] {
    <sha2::Sha256 as sha2::Digest>::digest(b).into()
}

fn verify(pk: &[u8; 32], msg: &[u8], sig: &[u8; 64]) -> bool {
    ed25519_dalek::VerifyingKey::from_bytes(pk)
        .unwrap()
        .verify_strict(msg, &ed25519_dalek::Signature::from_bytes(sig))
        .is_ok()
}

// ------------------------------------------------------------------ login

// The login message is exactly the table of §16.1, written out here, and
// `sign_login` signs it with the device's key over its own `dev`.
// spec: SEAL-SIG-001
// spec: SEAL-CTX-001
#[test]
fn the_login_message() {
    let d = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let dev = d.public().dev;
    let origin = "https://relay.example.org:8443";
    let m = login_message(origin, &BOARD, &dev, 0x0102_0304, &NONCE, &[0xC1; 32]).unwrap();
    let mut want = b"taskdeck/v1/login\0".to_vec();
    want.extend_from_slice(&[0, 30]);
    want.extend_from_slice(origin.as_bytes());
    want.extend_from_slice(&[0xB0; 16]);
    want.extend_from_slice(&dev.0);
    want.extend_from_slice(&[1, 2, 3, 4]);
    want.extend_from_slice(&[0x4E; 32]);
    want.extend_from_slice(&[0xC1; 32]);
    assert_eq!(m, want);
    assert_eq!(m.len(), 120 + origin.len());
    let (client_nonce, sig) = sign_login(&d, origin, &BOARD, 0x0102_0304, &NONCE, &mut OsEntropy).unwrap();
    let m = login_message(origin, &BOARD, &dev, 0x0102_0304, &NONCE, &client_nonce).unwrap();
    assert!(verify(&d.public().sign_pk, &m, &sig));
    // A fresh client nonce each time.
    let (other, _) = sign_login(&d, origin, &BOARD, 0x0102_0304, &NONCE, &mut OsEntropy).unwrap();
    assert_ne!(other, client_nonce);
    // The largest origin: "https://" and a host of 255 bytes, 263 in all.
    let host = format!("{}.{}.{}.{}:9999", "a".repeat(63), "b".repeat(63), "c".repeat(63), "d".repeat(58));
    assert_eq!(host.len(), 255);
    let big = format!("https://{host}");
    assert_eq!(login_message(&big, &BOARD, &dev, 0, &NONCE, &[0; 32]).unwrap().len(), 120 + 263);
}

// A relay nonce of any length other than 32 is `seal/challenge_length`,
// for login and admin, and nothing is drawn or signed.
// spec: SEAL-SIG-002
#[test]
fn a_nonce_not_of_32_bytes_is_refused() {
    let d = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let dev = d.public().dev;
    let origin = "https://relay.example.org";
    for n in [0usize, 1, 31, 33, 64] {
        let nonce = vec![7u8; n];
        let want = Some(SealError::ChallengeLength);
        assert_eq!(login_message(origin, &BOARD, &dev, 1, &nonce, &[0; 32]).err(), want, "{n}");
        assert_eq!(sign_login(&d, origin, &BOARD, 1, &nonce, &mut OsEntropy).err(), want, "{n}");
        assert_eq!(admin_message(&AdminOp::Revoke(dev), &BOARD, &dev, &nonce).err(), want, "{n}");
        assert_eq!(sign_admin(&Signer::Device(&d), &AdminOp::Revoke(dev), &BOARD, &nonce).err(), want, "{n}");
    }
}

// The origin comes from the caller's configuration and must be a relay
// origin of §5.4; anything else is `seal/host_format`.
// spec: SEAL-SIG-002
// spec: SEAL-ENC-005
#[test]
fn the_origin_must_be_a_relay_origin() {
    let d = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let dev = d.public().dev;
    for bad in [
        "",
        "relay.example.org",
        "http://relay.example.org",
        "https://relay.example.org/",
        "https://relay.example.org:443",
        "https://Relay.example.org",
        "HTTPS://relay.example.org",
        "https://relay.example.org:0",
        "https://relay.example.org:65536",
        "https://",
        "https://-a.example.org",
        "https://user@relay.example.org",
    ] {
        assert_eq!(login_message(bad, &BOARD, &dev, 1, &NONCE, &[0; 32]).err(), Some(SealError::HostFormat), "{bad}");
        assert_eq!(sign_login(&d, bad, &BOARD, 1, &NONCE, &mut OsEntropy).err(), Some(SealError::HostFormat), "{bad}");
    }
    let long = format!("https://{}.{}.{}.{}", "a".repeat(63), "b".repeat(63), "c".repeat(63), "d".repeat(62));
    assert_eq!(login_message(&long, &BOARD, &dev, 1, &NONCE, &[0; 32]).err(), Some(SealError::HostFormat));
    assert!(login_message("https://127.0.0.1:8080", &BOARD, &dev, 1, &NONCE, &[0; 32]).is_ok());
}

// `sign_login` draws exactly the 32-byte client nonce, and a refused call
// draws nothing.
// spec: SEAL-SIG-001
// spec: SEAL-SIG-002
#[cfg(feature = "test-entropy")]
#[test]
fn sign_login_draws_the_client_nonce() {
    use crate::FixedEntropy;
    let d = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let mut rng = FixedEntropy::new((0..64).collect());
    let origin = "https://relay.example.org";
    assert!(sign_login(&d, origin, &BOARD, 1, &[0; 31], &mut rng).is_err());
    assert!(sign_login(&d, "https://x:443", &BOARD, 1, &NONCE, &mut rng).is_err());
    let (a, _) = sign_login(&d, origin, &BOARD, 1, &NONCE, &mut rng).unwrap();
    let (b, _) = sign_login(&d, origin, &BOARD, 1, &NONCE, &mut rng).unwrap();
    assert_eq!(a, core::array::from_fn(|i| i as u8));
    assert_eq!(b, core::array::from_fn(|i| 32 + i as u8));
    assert_eq!(sign_login(&d, origin, &BOARD, 1, &NONCE, &mut rng).err(), Some(SealError::Entropy));
}

// ------------------------------------------------------------------ admin

fn wrap_to(dev: &DevId, fill: u8) -> [u8; 96] {
    let mut w = [fill; 96];
    w[..16].copy_from_slice(&dev.0);
    w
}

/// Every operation of the registry with its `args` written out by hand.
fn registry() -> Vec<(AdminOp, &'static str, Vec<u8>)> {
    let dev = DevId([0xD1; 16]);
    let pk = [0x5B; 32];
    let fh = FrameHash([0xF1; 32]);
    let cat = |parts: &[&[u8]]| parts.concat();
    vec![
        (AdminOp::Register { dev, sign_pk: pk, frame_hash: fh }, "register", cat(&[&dev.0, &pk, &fh.0])),
        (AdminOp::Revoke(dev), "revoke", dev.0.to_vec()),
        (
            AdminOp::Reinstate { dev, first_epoch: 5, wraps: vec![wrap_to(&dev, 1), wrap_to(&dev, 2)] },
            "reinstate",
            cat(&[&dev.0, &[0, 0, 0, 5], &[0, 2], &wrap_to(&dev, 1), &wrap_to(&dev, 2)]),
        ),
        (
            AdminOp::RecoveryRegister { dev, sign_pk: pk, frame_hash: fh },
            "recovery_register",
            cat(&[&dev.0, &pk, &fh.0]),
        ),
        (
            AdminOp::Fence { host: "new.example.net:8443".into(), door: [1, 2, 3, 4, 5], home_gen: 3 },
            "fence",
            cat(&[&[20], b"new.example.net:8443", &[1, 2, 3, 4, 5], &[0, 0, 0, 3]]),
        ),
        (
            AdminOp::Seed(SeedArgs {
                home_gen: 2,
                first_seq: 65,
                chain_before: Chain([0xC0; 32]),
                last_seq: 70,
                chain_last: Chain([0xC1; 32]),
                blob_id: BlobId([0xB1; 16]),
                entries: vec![
                    SeedEntry { dev, sign_pk: pk, role: 1, status: 1, last_ctr: 9 },
                    SeedEntry { dev: DevId([0xD2; 16]), sign_pk: [0x5C; 32], role: 2, status: 2, last_ctr: 1 },
                ],
            }),
            "seed",
            cat(&[
                &[0, 0, 0, 2],
                &65u64.to_be_bytes(),
                &[0xC0; 32],
                &70u64.to_be_bytes(),
                &[0xC1; 32],
                &[0xB1; 16],
                &[0, 2],
                &dev.0,
                &pk,
                &[1, 1],
                &9u64.to_be_bytes(),
                &[0xD2; 16],
                &[0x5C; 32],
                &[2, 2],
                &1u64.to_be_bytes(),
            ]),
        ),
        (
            AdminOp::Compact(CompactArgs { below_seq: 20_000, snap_entry_seq: 19_000, blob_id: BlobId([0xB2; 16]), keep_seqs: vec![1, 5, 64] }),
            "compact",
            cat(&[
                &20_000u64.to_be_bytes(),
                &19_000u64.to_be_bytes(),
                &[0xB2; 16],
                &[0, 0, 0, 3],
                &1u64.to_be_bytes(),
                &5u64.to_be_bytes(),
                &64u64.to_be_bytes(),
            ]),
        ),
        (
            AdminOp::RestoreTail { first_seq: 100, frames: vec![FrameHash([1; 32]), FrameHash([2; 32])] },
            "restore_tail",
            cat(&[&100u64.to_be_bytes(), &[0, 0, 0, 2], &[1; 32], &[2; 32]]),
        ),
        (AdminOp::Purge { before_seq: 300, epoch: 4 }, "purge", cat(&[&300u64.to_be_bytes(), &[0, 0, 0, 4]])),
        (AdminOp::MintReadToken { dev, epoch: 1 }, "mint_read_token", cat(&[&dev.0, &[0, 0, 0, 1]])),
        (AdminOp::RevokeReadToken(dev), "revoke_read_token", dev.0.to_vec()),
    ]
}

// Each operation's name and `args` bytes against the registry's layout,
// and the widths the registry states (80, 16, 22 + 96n, 80, 10 + n,
// 102 + 58n, 36 + 8n, 12 + 32n, 12, 20, 16).
// spec: SEAL-SIG-004
#[test]
fn every_registry_operation() {
    let ops = registry();
    assert_eq!(ops.len(), 11);
    let widths = [80, 16, 22 + 96 * 2, 80, 10 + 20, 102 + 58 * 2, 36 + 8 * 3, 12 + 32 * 2, 12, 20, 16];
    for ((op, name, args), width) in ops.iter().zip(widths) {
        assert_eq!(op.name(), *name);
        assert_eq!(&op.args(), args, "{name}");
        assert_eq!(args.len(), width, "{name}");
    }
    // Every name is distinct.
    let mut names: Vec<&str> = ops.iter().map(|(op, _, _)| op.name()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), 11);
}

// The admin message is exactly the first table of §16.2 with
// `args_hash = H(args)`, signed by the signer: a device over its `dev`, the
// recovery identity over `dev_R`.
// spec: SEAL-SIG-003
// spec: SEAL-CTX-001
#[test]
fn the_admin_message() {
    let d = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let rk = RecoverySeed::generate(&mut OsEntropy).unwrap().keys();
    let dev_r = rk.public().dev(&BOARD);
    for (op, name, args) in registry() {
        let m = admin_message(&op, &BOARD, &d.public().dev, &NONCE).unwrap();
        let mut want = b"taskdeck/v1/admin\0".to_vec();
        want.push(name.len() as u8);
        want.extend_from_slice(name.as_bytes());
        want.extend_from_slice(&[0xB0; 16]);
        want.extend_from_slice(&d.public().dev.0);
        want.extend_from_slice(&sha(&args));
        want.extend_from_slice(&NONCE);
        assert_eq!(m, want, "{name}");
        assert_eq!(m.len(), 115 + name.len());
        let sig = sign_admin(&Signer::Device(&d), &op, &BOARD, &NONCE).unwrap();
        assert!(verify(&d.public().sign_pk, &m, &sig), "{name}");
        let sig = sign_admin(&Signer::Recovery(&rk, BOARD), &op, &BOARD, &NONCE).unwrap();
        let m = admin_message(&op, &BOARD, &dev_r, &NONCE).unwrap();
        assert!(verify(&rk.public().sign_pk, &m, &sig), "{name}");
    }
    let r = sign_admin(&Signer::Recovery(&rk, BoardId([1; 16])), &AdminOp::Revoke(dev_r), &BOARD, &NONCE);
    assert_eq!(r.err(), Some(SealError::UnknownAuthor));
}

// What the registry's arguments cannot be is refused before signing: a
// host outside §5.4 with `seal/host_format`, everything else with
// `seal/bad_payload`.
// spec: SEAL-SIG-004
// spec: SEAL-ENC-006
#[test]
fn invalid_arguments_are_refused() {
    let d = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let dev = DevId([0xD1; 16]);
    let ops = registry();
    let seed = || match &ops[5].0 {
        AdminOp::Seed(s) => s.clone(),
        _ => unreachable!(),
    };
    let compact = || match &ops[6].0 {
        AdminOp::Compact(c) => c.clone(),
        _ => unreachable!(),
    };
    let fence = |host: &str, home_gen| AdminOp::Fence { host: host.into(), door: [0; 5], home_gen };
    for (op, want) in [
        (fence("new.example.net:443", 3), SealError::HostFormat),
        (fence("New.example.net", 3), SealError::HostFormat),
        (fence(&"a".repeat(256), 3), SealError::HostFormat),
        (fence("new.example.net", 1), SealError::BadPayload),
        (AdminOp::Reinstate { dev, first_epoch: 5, wraps: vec![] }, SealError::BadPayload),
        (AdminOp::Reinstate { dev, first_epoch: 5, wraps: vec![wrap_to(&dev, 0); 257] }, SealError::BadPayload),
        (AdminOp::Reinstate { dev, first_epoch: 0, wraps: vec![wrap_to(&dev, 0)] }, SealError::BadPayload),
        (AdminOp::Reinstate { dev, first_epoch: u32::MAX, wraps: vec![wrap_to(&dev, 0); 2] }, SealError::BadPayload),
        (AdminOp::Reinstate { dev, first_epoch: 5, wraps: vec![wrap_to(&DevId([9; 16]), 0)] }, SealError::BadPayload),
        (AdminOp::Seed(SeedArgs { first_seq: 0, last_seq: 0, ..seed() }), SealError::BadPayload),
        (AdminOp::Seed(SeedArgs { first_seq: 10, last_seq: 8, ..seed() }), SealError::BadPayload),
        (AdminOp::Seed(SeedArgs { entries: vec![SeedEntry { dev, sign_pk: [0; 32], role: 3, status: 1, last_ctr: 0 }], ..seed() }), SealError::BadPayload),
        (AdminOp::Seed(SeedArgs { entries: vec![SeedEntry { dev, sign_pk: [0; 32], role: 1, status: 0, last_ctr: 0 }], ..seed() }), SealError::BadPayload),
        (AdminOp::Compact(CompactArgs { keep_seqs: vec![5, 1], ..compact() }), SealError::BadPayload),
        (AdminOp::Compact(CompactArgs { keep_seqs: vec![5, 5], ..compact() }), SealError::BadPayload),
        (AdminOp::Compact(CompactArgs { keep_seqs: vec![20_000], ..compact() }), SealError::BadPayload),
        (AdminOp::RestoreTail { first_seq: 0, frames: vec![] }, SealError::BadPayload),
        (AdminOp::Purge { before_seq: 0, epoch: 4 }, SealError::BadPayload),
        (AdminOp::Purge { before_seq: 5, epoch: 1 }, SealError::BadPayload),
        (AdminOp::MintReadToken { dev, epoch: 0 }, SealError::BadPayload),
    ] {
        assert_eq!(admin_message(&op, &BOARD, &dev, &NONCE).err(), Some(want), "{op:?}");
        assert_eq!(sign_admin(&Signer::Device(&d), &op, &BOARD, &NONCE).err(), Some(want), "{op:?}");
        // `args` itself never panics on them.
        let _ = op.args();
    }
    // The bounds that pass: 256 wraps ending at epoch 2^32 − 1, no tail, empty lists.
    for op in [
        AdminOp::Reinstate { dev, first_epoch: u32::MAX - 255, wraps: vec![wrap_to(&dev, 0); 256] },
        AdminOp::Seed(SeedArgs { first_seq: 10, last_seq: 9, entries: vec![], ..seed() }),
        AdminOp::Compact(CompactArgs { keep_seqs: vec![], ..compact() }),
        AdminOp::RestoreTail { first_seq: 1, frames: vec![] },
    ] {
        assert!(admin_message(&op, &BOARD, &dev, &NONCE).is_ok(), "{op:?}");
    }
}

// `register` names the hash of the frame that carries the DeviceAdded, and
// its keys are the ones that DeviceAdded names: built here from a sealed
// DeviceAdded frame through the crate's functions, and read back from it.
// spec: SEAL-SIG-005
#[test]
fn register_names_the_device_added_frame() {
    let e = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let n = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let np = n.public();
    let item = CtlItem::DeviceAdded(DeviceAdded {
        dev: np.dev,
        sign_pk: np.sign_pk,
        hpke_pk: np.hpke_pk,
        name: "N".into(),
        transcript: [3; 32],
        bootstrap_seq: 1,
    });
    let entry_id = EntryId([4; 16]);
    let payload = ctl_payload(&[item], &Signer::Device(&e), &BOARD, 2, &entry_id, 10).unwrap();
    let key = derive_epoch_key(&EpochRoot::from_bytes([1; 32]), &BOARD, 1, KeyKind::Entry);
    let plain = Plain { entry_id, kind: Kind::Ctl, at_ms: 10, based_on: Head { seq: 0, chain: chain_zero(&BOARD) }, payload };
    let frame = seal_frame(&Signer::Device(&e), &BOARD, 2, 1, &key, &plain, &mut OsEntropy).unwrap();
    let op = AdminOp::Register { dev: np.dev, sign_pk: np.sign_pk, frame_hash: frame_hash(&frame) };
    let args = op.args();
    assert_eq!(&args[48..80], &sha(&frame));
    // The frame opens to the DeviceAdded whose dev and sign_pk the args name.
    let opened = crate::entries::unpad(&crate::entries::open_envelope(&frame, &key).unwrap()).unwrap();
    let ep = e.public();
    let cx = crate::payloads::CtlContext { board: &BOARD, dev: &ep.dev, ctr: 2, entry_id: &entry_id, at_ms: 10, sign_pk: &ep.sign_pk };
    let items = crate::payloads::parse_ctl(&opened.payload, &cx).unwrap();
    let [CtlItem::DeviceAdded(a)] = items.as_slice() else { panic!("{items:?}") };
    assert_eq!(&args[..16], &a.dev.0);
    assert_eq!(&args[16..48], &a.sign_pk);
    let sig = sign_admin(&Signer::Device(&e), &op, &BOARD, &NONCE).unwrap();
    assert!(verify(&ep.sign_pk, &admin_message(&op, &BOARD, &ep.dev, &NONCE).unwrap(), &sig));
}
