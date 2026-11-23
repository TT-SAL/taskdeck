//! `seal-rotation.json` (seal.md §7, §12.5): one rotation's sealing call,
//! with one entropy source in the draw order of §7: the root's 32 bytes,
//! then the 32 bytes `ikm` of each wrap in ascending recipient-id order,
//! then the frame's 24-byte nonce. Outputs: the root, each wrap with its
//! `ikm` and the ephemeral public key `DeriveKeyPair(ikm)` gives (computed
//! here with `hpke`, and equal to the wrap's `enc`), the item, the frame;
//! and the same call with one byte short, which fails with `seal/entropy`.
//! The frame is classified `Accept` on the board it was made for.

use crate::board::*;
use crate::check_or_write;
use crate::common::hex;
use serde_json::{Map, Value, json};
use taskdeck_seal::*;

struct Call<'a> {
    signer: Signer<'a>,
    board: BoardId,
    ctr: u64,
    epoch: u32,
    current_root: [u8; 32],
    recipients: Vec<(DevId, [u8; 32])>,
    entry_id: EntryId,
    at_ms: i64,
    based_on: Head,
}

/// The sealing call: root, wraps, frame, all from one `Entropy`.
fn seal_rotation(c: &Call, entropy: Vec<u8>) -> Result<(EpochRoot, EpochRotated, Vec<u8>), SealError> {
    let mut rng = FixedEntropy::new(entropy);
    let root = EpochRoot::generate(&mut rng)?;
    let commit = root.commitment(&c.board, c.epoch);
    let mut wraps = Vec::new();
    for (id, pk) in &c.recipients {
        wraps.push(wrap_root(&root, &c.board, c.epoch, &commit, *id, pk, &mut rng)?);
    }
    let item = EpochRotated { epoch: c.epoch, commit, wraps, purge_before: None };
    let payload = ctl_payload(&[CtlItem::EpochRotated(item.clone())], &c.signer, &c.board, c.ctr, &c.entry_id, c.at_ms)?;
    let plain = Plain { entry_id: c.entry_id, kind: Kind::Ctl, at_ms: c.at_ms, based_on: c.based_on, payload };
    let e = c.epoch - 1;
    let key = derive_epoch_key(&EpochRoot::from_bytes(c.current_root), &c.board, e, KeyKind::Entry);
    let frame = seal_frame(&c.signer, &c.board, c.ctr, e, &key, &plain, &mut rng)?;
    Ok((root, item, frame))
}

fn ephemeral(ikm: &[u8]) -> [u8; 32] {
    use hpke::{Kem as _, Serializable as _};
    let (_, pk) = hpke::kem::X25519HkdfSha256::derive_keypair(ikm);
    let mut out = [0u8; 32];
    pk.write_exact(&mut out);
    out
}

fn inputs(name: &str, s: &Sim, author: usize, c: &Call, entropy: &[u8]) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("name".into(), json!(name));
    m.insert("board_id".into(), json!(hex(&c.board.0)));
    m.insert("author_identity".into(), json!(hex(&s.devs[author].to_identity_bytes())));
    m.insert("author_dev".into(), json!(hex(&s.devs[author].public().dev.0)));
    m.insert("author_sign_pk".into(), json!(hex(&s.devs[author].public().sign_pk)));
    m.insert("ctr".into(), json!(c.ctr));
    m.insert("epoch".into(), json!(c.epoch));
    m.insert("sealed_under_epoch".into(), json!(c.epoch - 1));
    m.insert("sealed_under_root".into(), json!(hex(&c.current_root)));
    m.insert(
        "recipients".into(),
        Value::Array(c.recipients.iter().map(|(id, pk)| json!({ "id": hex(&id.0), "hpke_pk": hex(pk) })).collect()),
    );
    m.insert("entry_id".into(), json!(hex(&c.entry_id.0)));
    m.insert("at_ms".into(), json!(c.at_ms));
    m.insert("based_on_seq".into(), json!(c.based_on.seq));
    m.insert("based_on_chain".into(), json!(hex(&c.based_on.chain.0)));
    m.insert("entropy".into(), json!(hex(entropy)));
    m
}

#[test]
fn seal_rotation_vectors() {
    let mut cases = Vec::new();
    for (seed, n) in [(81u64, 2usize), (82, 4)] {
        let mut s = Sim::new(seed, n);
        let mut recipients = s.recipients_after(&[]);
        recipients.sort();
        let call = Call {
            signer: Signer::Device(&s.devs[0]),
            board: s.board,
            ctr: s.next_ctr(Who::D(0)),
            epoch: s.m.epoch() + 1,
            current_root: s.roots[&s.m.epoch()],
            recipients: recipients.clone(),
            entry_id: EntryId(crate::count(0xe0, 16).try_into().unwrap()),
            at_ms: s.now,
            based_on: s.head(),
        };
        let mut entropy = crate::count(0x10, 32);
        for i in 0..recipients.len() {
            entropy.extend(crate::count(0x40 + 0x20 * i as u8, 32));
        }
        entropy.extend(crate::count(0xa0, 24));
        let (root, item, frame) = seal_rotation(&call, entropy.clone()).unwrap();
        let mut m = inputs(&format!("a rotation to {} recipients", recipients.len()), &s, 0, &call, &entropy);
        assert_eq!(&root.to_bytes()[..], &entropy[..32]);
        m.insert("root".into(), json!(hex(&root.to_bytes()[..])));
        let mut wraps = Vec::new();
        for (i, w) in item.wraps.iter().enumerate() {
            let ikm = &entropy[32 + 32 * i..64 + 32 * i];
            let enc = ephemeral(ikm);
            assert_eq!(&w[16..48], &enc);
            assert_eq!(&w[..16], &recipients[i].0.0);
            wraps.push(json!({ "ikm": hex(ikm), "ephemeral_pk": hex(&enc), "wrap": hex(w) }));
        }
        m.insert("wraps".into(), Value::Array(wraps));
        m.insert(
            "item".into(),
            json!({ "type": "EpochRotated", "epoch": item.epoch, "commit": hex(&item.commit), "wraps": item.wraps.iter().map(|w| hex(w)).collect::<Vec<_>>() }),
        );
        assert_eq!(&frame[54..78], &entropy[entropy.len() - 24..]);
        m.insert("frame".into(), json!(hex(&frame)));
        m.insert("frame_hash".into(), json!(hex(&frame_hash(&frame).0)));
        // The frame is what the board accepts at the next seq.
        let r = s.classify(&frame, &Opts::default()).unwrap();
        assert!(matches!(r.class, Class::Accept) && r.code.is_none());
        assert_eq!(r.membership.epoch(), call.epoch);
        cases.push(Value::Object(m));

        let short = entropy[..entropy.len() - 1].to_vec();
        let err = seal_rotation(&call, short.clone()).err().unwrap();
        let mut m = inputs(&format!("a rotation to {} recipients, entropy one byte short", recipients.len()), &s, 0, &call, &short);
        m.insert("error".into(), json!(err.code()));
        assert_eq!(err.code(), "seal/entropy");
        cases.push(Value::Object(m));
        s.staged = None;
    }
    check_or_write("seal-rotation.json", json!({ "spec": "seal.md §7", "cases": cases }));
}
