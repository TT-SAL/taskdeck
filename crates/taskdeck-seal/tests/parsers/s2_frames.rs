//! Task S2's public parser (`frame_header`) and builders over random and
//! mutated inputs: each returns, never panics, and what it accepts holds
//! its contract. The private receive-side parsers have their own property
//! tests in `src/entries/tests.rs` and `src/payloads/tests.rs`.

use crate::common::BOARD;
use proptest::prelude::*;
use taskdeck_seal::{
    AckVerdict, AdminOp, Beat, BeatAck, BeatState, BlobId, Chain, CompactArgs, CtlItem, DevId, DeviceKeys, EntryId,
    EpochRoot, Fence, FrameHash, Head, KeyKind, Kind, OsEntropy, Plain, ResetTo, RulesUpgrade, RulesUpgradeMode,
    SealError, SeedArgs, SeedEntry, Signer, SkipItem, SnapAnnounce, admin_message, beat_payload, chain_zero, ctl_payload,
    derive_epoch_key, frame_header, login_message, padded_len, seal_frame, snap_payload,
};

fn mutate(mut b: Vec<u8>, at: usize, op: u8, x: u8) -> Vec<u8> {
    if b.is_empty() {
        return vec![x];
    }
    let i = at % b.len();
    match op % 4 {
        0 => b[i] = x,
        1 => b.insert(i, x),
        2 => {
            b.remove(i);
        }
        _ => b.truncate(i),
    }
    b
}

fn sealed() -> Vec<u8> {
    let d = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let key = derive_epoch_key(&EpochRoot::from_bytes([1; 32]), &BOARD, 1, KeyKind::Entry);
    let p = Plain {
        entry_id: EntryId([2; 16]),
        kind: Kind::Cmd,
        at_ms: 5,
        based_on: Head { seq: 0, chain: chain_zero(&BOARD) },
        payload: vec![3; 20],
    };
    seal_frame(&Signer::Device(&d), &BOARD, 1, 1, &key, &p, &mut OsEntropy).unwrap()
}

fn item() -> impl Strategy<Value = CtlItem> {
    prop_oneof![
        (any::<u64>(), any::<[u8; 32]>()).prop_map(|(seq, h)| CtlItem::Skip(SkipItem { seq, frame_hash: FrameHash(h) })),
        (any::<u64>(), any::<u32>()).prop_map(|(seq, hv)| CtlItem::ResetTo(ResetTo { seq, state_hash: [1; 32], hash_version: hv })),
        ("[a-zA-Z0-9.:\\-]{0,20}", any::<u32>()).prop_map(|(host, g)| CtlItem::Fence(Fence { host, door: [0; 5], home_gen: g })),
        (any::<u32>(), any::<bool>()).prop_map(|(r, o)| CtlItem::RulesUpgrade(RulesUpgrade {
            rules: r,
            mode: if o { RulesUpgradeMode::Override } else { RulesUpgradeMode::Normal },
        })),
        any::<[u8; 16]>().prop_map(|d| CtlItem::DeviceRevoked(DevId(d))),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]

    // `frame_header` on random bytes and on mutated sealed frames returns;
    // when it accepts, the layout of step 1 holds.
    // spec: SEAL-GEN-011
    // spec: SEAL-FRAME-003
    #[test]
    fn frame_header_never_panics(
        random in proptest::collection::vec(any::<u8>(), 0..1024),
        at in any::<usize>(), op in any::<u8>(), x in any::<u8>(), len_field in any::<u32>(),
    ) {
        let f = sealed();
        let mut lf = f.clone();
        lf[44..48].copy_from_slice(&len_field.to_be_bytes());
        for input in [random, mutate(f, at, op, x), lf] {
            if let Ok(h) = frame_header(&input) {
                prop_assert!((670..=262_144).contains(&input.len()));
                prop_assert_eq!(&input[..4], b"TDF1");
                prop_assert_eq!(h.len as usize, input.len() - 112);
            } else {
                prop_assert_eq!(frame_header(&input).err(), Some(SealError::FrameFormat));
            }
        }
    }

    // `padded_len` returns for every u64, never below its input.
    // spec: SEAL-GEN-011
    // spec: SEAL-PAD-001
    #[test]
    fn padded_len_never_panics(u in any::<u64>()) {
        prop_assert!(padded_len(u) >= u);
    }

    // The payload builders return on any field values.
    // spec: SEAL-GEN-011
    #[test]
    fn payload_builders_never_panic(
        rules in proptest::collection::vec(any::<u32>(), 0..20),
        envelopes in proptest::collection::vec(any::<(u8, u8)>(), 0..20),
        state in proptest::option::of((any::<u64>(), any::<u32>())),
        acks in proptest::collection::vec((any::<u64>(), any::<bool>()), 0..70),
        snap in (any::<u64>(), any::<u32>(), any::<u64>(), any::<u32>(), any::<u64>(), any::<u32>()),
        items in proptest::collection::vec(item(), 0..18),
        ctr in any::<u64>(), at_ms in any::<i64>(),
    ) {
        let beat = Beat {
            rules: rules.clone(),
            envelopes,
            hash_versions: rules,
            state: state.map(|(seq, hv)| BeatState { seq, hash: [0; 32], hash_version: hv }),
            acks: acks.into_iter().map(|(entry, a)| BeatAck { entry, verdict: if a { AckVerdict::Ack } else { AckVerdict::Mismatch } }).collect(),
        };
        let _ = beat_payload(&beat);
        let (seq, hv, size, parts, inflated, epoch) = snap;
        let s = SnapAnnounce { seq, hash: [0; 32], hash_version: hv, blob: BlobId([0; 16]), sha256: [0; 32], size, parts, inflated, epoch };
        let _ = snap_payload(&s);
        let d = DeviceKeys::generate(&mut OsEntropy).unwrap();
        let _ = ctl_payload(&items, &Signer::Device(&d), &BOARD, ctr, &EntryId([0; 16]), at_ms);
    }

    // `seal_frame`, `login_message` and `admin_message` return on any inputs.
    // spec: SEAL-GEN-011
    #[test]
    fn frame_and_signature_builders_never_panic(
        ctr in any::<u64>(), epoch in any::<u32>(), at_ms in any::<i64>(), seq in any::<u64>(), kind in 1u8..5,
        payload_len in prop_oneof![0usize..600, 257_970usize..257_990],
        origin in "(https://)?[a-zA-Z0-9.:/\\-]{0,30}", nonce in proptest::collection::vec(any::<u8>(), 0..40),
        host in "[a-z0-9.:\\-]{0,30}", n in 0usize..300, first in any::<u32>(), keep in proptest::collection::vec(any::<u64>(), 0..5),
        role in any::<u8>(), status in any::<u8>(),
    ) {
        let d = DeviceKeys::generate(&mut OsEntropy).unwrap();
        let key = derive_epoch_key(&EpochRoot::from_bytes([1; 32]), &BOARD, epoch, KeyKind::Entry);
        let kind = match kind { 1 => Kind::Cmd, 2 => Kind::Ctl, 3 => Kind::Beat, _ => Kind::Snap };
        let chain = if seq == 0 && ctr % 2 == 0 { chain_zero(&BOARD) } else { Chain([7; 32]) };
        let p = Plain { entry_id: EntryId([1; 16]), kind, at_ms, based_on: Head { seq, chain }, payload: vec![0; payload_len] };
        if let Ok(f) = seal_frame(&Signer::Device(&d), &BOARD, ctr, epoch, &key, &p, &mut OsEntropy) {
            prop_assert!(frame_header(&f).is_ok());
            prop_assert_eq!(f.len() as u64, 112 + 30 + padded_len(69 + payload_len as u64) + 16);
        }
        let dev = d.public().dev;
        let _ = login_message(&origin, &BOARD, &dev, epoch, &nonce, &[0; 32]);
        let dw = DevId([5; 16]);
        let mut w = [0u8; 96];
        w[..16].copy_from_slice(&dw.0);
        for op in [
            AdminOp::Fence { host: host.clone(), door: [0; 5], home_gen: epoch },
            AdminOp::Reinstate { dev: dw, first_epoch: first, wraps: vec![w; n] },
            AdminOp::Compact(CompactArgs { below_seq: seq, snap_entry_seq: 0, blob_id: BlobId([0; 16]), keep_seqs: keep.clone() }),
            AdminOp::Seed(SeedArgs {
                home_gen: epoch, first_seq: seq, chain_before: Chain([0; 32]), last_seq: ctr, chain_last: Chain([0; 32]),
                blob_id: BlobId([0; 16]), entries: vec![SeedEntry { dev: dw, sign_pk: [0; 32], role, status, last_ctr: 0 }; n % 4],
            }),
            AdminOp::RestoreTail { first_seq: seq, frames: vec![FrameHash([0; 32]); n % 4] },
            AdminOp::Purge { before_seq: seq, epoch },
            AdminOp::MintReadToken { dev: dw, epoch },
        ] {
            let _ = op.args();
            if let Ok(m) = admin_message(&op, &BOARD, &dev, &nonce) {
                prop_assert_eq!(m.len(), 115 + op.name().len());
            }
        }
    }
}
