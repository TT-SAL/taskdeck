//! `seal-frame.json` (seal.md §8, §9): sealed frames from fixed inputs, with
//! the frame bytes, `frame_hash`, the signed bytes, the AD, `K_entry,e` and
//! the padded plaintext; for `ctl` frames also `enc(items)`, the inner
//! signed bytes and the inner signature; then the sealing refusals and the
//! frames step 1 and step 4 of §14.2 reject.
//!
//! `K_entry,e`, the padded plaintext and the signature checks are computed
//! here with the pinned crates (`hkdf`, `chacha20poly1305`, `ed25519-dalek`)
//! from bytes the public API gives; `src/entries/tests.rs` opens every
//! committed frame with the crate's private receive side.

use crate::common::hex;
use crate::{check_or_write, count};
use chacha20poly1305::aead::{Aead as _, KeyInit as _, Payload};
use serde_json::{Map, Value, json};
use taskdeck_seal::{
    Beat, BeatAck, AckVerdict, BeatState, BlobId, BoardId, Chain, CtlItem, DeviceKeys, EntryId, EpochRoot, FixedEntropy,
    Genesis, GenesisDevice, Head, KeyKind, Kind, Plain, RecoveryKeys, RecoverySeed, SealError, Signer, SnapAnnounce,
    beat_payload, board_id, chain_zero, ctl_payload, derive_epoch_key, frame_hash, frame_header, seal_frame, snap_payload,
    wrap_root,
};

const BOARD: BoardId = BoardId([0x42; 16]);

/// `K_entry,e` with the `hkdf` crate (seal.md §6.5).
fn k_entry(root: &EpochRoot, board: &BoardId, e: u32) -> [u8; 32] {
    let info = [b"taskdeck/v1/key-entry\0".as_slice(), &e.to_be_bytes()].concat();
    let mut okm = [0u8; 32];
    hkdf::Hkdf::<sha2::Sha256>::new(Some(&board.0), &root.to_bytes()[..]).expand(&info, &mut okm).unwrap();
    okm
}

fn sha(b: &[u8]) -> [u8; 32] {
    <sha2::Sha256 as sha2::Digest>::digest(b).into()
}

enum Author {
    Device(DeviceKeys, Vec<u8>),
    Recovery(RecoveryKeys, Vec<u8>, BoardId),
}

impl Author {
    fn device(entropy: Vec<u8>) -> Author {
        Author::Device(DeviceKeys::generate(&mut FixedEntropy::new(entropy.clone())).unwrap(), entropy)
    }
    fn signer(&self) -> Signer<'_> {
        match self {
            Author::Device(k, _) => Signer::Device(k),
            Author::Recovery(k, _, b) => Signer::Recovery(k, *b),
        }
    }
    fn sign_pk(&self) -> [u8; 32] {
        match self {
            Author::Device(k, _) => k.public().sign_pk,
            Author::Recovery(k, _, _) => k.public().sign_pk,
        }
    }
    fn inputs(&self, m: &mut Map<String, Value>) {
        match self {
            Author::Device(k, e) => {
                m.insert("author".into(), json!("device"));
                m.insert("author_entropy".into(), json!(hex(e)));
                m.insert("dev".into(), json!(hex(&k.public().dev.0)));
            }
            Author::Recovery(k, s, b) => {
                m.insert("author".into(), json!("recovery"));
                m.insert("author_seed".into(), json!(hex(s)));
                m.insert("dev".into(), json!(hex(&k.public().dev(b).0)));
            }
        }
        m.insert("sign_pk".into(), json!(hex(&self.sign_pk())));
    }
}

struct Case<'a> {
    name: &'a str,
    author: &'a Author,
    board: BoardId,
    ctr: u64,
    epoch: u32,
    root: [u8; 32],
    plain: Plain,
    entropy: Vec<u8>,
}

fn kind_name(k: Kind) -> &'static str {
    match k {
        Kind::Cmd => "cmd",
        Kind::Ctl => "ctl",
        Kind::Beat => "beat",
        Kind::Snap => "snap",
    }
}

fn inputs(c: &Case) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("name".into(), json!(c.name));
    m.insert("board_id".into(), json!(hex(&c.board.0)));
    c.author.inputs(&mut m);
    m.insert("ctr".into(), json!(c.ctr));
    m.insert("epoch".into(), json!(c.epoch));
    m.insert("root".into(), json!(hex(&c.root)));
    m.insert("entry_id".into(), json!(hex(&c.plain.entry_id.0)));
    m.insert("kind".into(), json!(c.plain.kind as u8));
    m.insert("kind_name".into(), json!(kind_name(c.plain.kind)));
    m.insert("at_ms".into(), json!(c.plain.at_ms));
    m.insert("based_on_seq".into(), json!(c.plain.based_on.seq));
    m.insert("based_on_chain".into(), json!(hex(&c.plain.based_on.chain.0)));
    m.insert("payload".into(), json!(hex(&c.plain.payload)));
    m.insert("entropy".into(), json!(hex(&c.entropy)));
    m
}

fn seal(c: &Case) -> Result<Vec<u8>, SealError> {
    let r = EpochRoot::from_bytes(c.root);
    let key = derive_epoch_key(&r, &c.board, c.epoch, KeyKind::Entry);
    seal_frame(&c.author.signer(), &c.board, c.ctr, c.epoch, &key, &c.plain, &mut FixedEntropy::new(c.entropy.clone()))
}

fn valid(c: &Case) -> Value {
    let mut m = inputs(c);
    let f = seal(c).unwrap();
    let r = EpochRoot::from_bytes(c.root);
    let k = k_entry(&r, &c.board, c.epoch);
    let len = f.len() - 112;
    let ad = [b"taskdeck/v1/entry\0".as_slice(), &f[..48], &f[48..54]].concat();
    let cipher = chacha20poly1305::XChaCha20Poly1305::new_from_slice(&k).unwrap();
    let nonce = chacha20poly1305::XNonce::try_from(&f[54..78]).unwrap();
    let pt = cipher.decrypt(&nonce, Payload { msg: &f[78..48 + len], aad: &ad }).unwrap();
    let signed = [b"taskdeck/v1/frame\0".as_slice(), &f[..48 + len]].concat();
    let vk = ed25519_dalek::VerifyingKey::from_bytes(&c.author.sign_pk()).unwrap();
    vk.verify_strict(&signed, &ed25519_dalek::Signature::from_bytes(f[48 + len..].try_into().unwrap())).unwrap();
    assert_eq!(&f[54..78], c.entropy.as_slice());
    m.insert("k_entry".into(), json!(hex(&k)));
    m.insert("len".into(), json!(len));
    m.insert("ad".into(), json!(hex(&ad)));
    m.insert("padded_plaintext".into(), json!(hex(&pt)));
    m.insert("signed".into(), json!(hex(&signed)));
    m.insert("frame".into(), json!(hex(&f)));
    m.insert("frame_hash".into(), json!(hex(&frame_hash(&f).0)));
    if c.plain.kind == Kind::Ctl {
        // {"items":<enc(items)>,"sig":"<128 hex>"}: the items' bytes are the
        // payload between the first member's colon and the second member.
        let p = &c.plain.payload;
        let items_bytes = &p[b"{\"items\":".len()..p.len() - b",\"sig\":\"\"}".len() - 128];
        let v: Value = serde_json::from_slice(p).unwrap();
        let sig: [u8; 64] = crate::common::unhex(v["sig"].as_str().unwrap()).try_into().unwrap();
        let hdr = frame_header(&f).unwrap();
        let inner = [
            b"taskdeck/v1/ctl\0".as_slice(),
            &c.board.0,
            &hdr.dev.0,
            &c.ctr.to_be_bytes(),
            &c.plain.entry_id.0,
            &c.plain.at_ms.to_be_bytes(),
            &sha(items_bytes),
        ]
        .concat();
        vk.verify_strict(&inner, &ed25519_dalek::Signature::from_bytes(&sig)).unwrap();
        m.insert("items_bytes".into(), json!(String::from_utf8(items_bytes.to_vec()).unwrap()));
        m.insert("ctl_inner".into(), json!(hex(&inner)));
        m.insert("ctl_sig".into(), json!(hex(&sig)));
    }
    Value::Object(m)
}

fn refused(c: &Case) -> Value {
    let mut m = inputs(c);
    m.insert("error".into(), json!(seal(c).err().unwrap().code()));
    Value::Object(m)
}

fn plain(kind: Kind, at_ms: i64, based_on: Head, payload: Vec<u8>) -> Plain {
    Plain { entry_id: EntryId(count(0xe0, 16).try_into().unwrap()), kind, at_ms, based_on, payload }
}

#[test]
fn seal_frame_vectors() {
    let dev = Author::device(count(0x80, 80));
    let c0 = Head { seq: 0, chain: chain_zero(&BOARD) };
    let head130 = Head { seq: 130, chain: Chain(count(0xa0, 32).try_into().unwrap()) };
    let head200 = Head { seq: 200, chain: Chain(count(0x70, 32).try_into().unwrap()) };

    // The genesis: the recovery identity of seed 00..1f on its own board.
    let seed = count(0, 32);
    let rk = RecoverySeed::generate(&mut FixedEntropy::new(seed.clone())).unwrap().keys();
    let rp = rk.public();
    let salt = [0x5a; 16];
    let gboard = board_id(&rp.sign_pk, &salt);
    let groot = [0x22; 32];
    let gr = EpochRoot::from_bytes(groot);
    let commit = gr.commitment(&gboard, 1);
    let first = match &dev {
        Author::Device(k, _) => k.public(),
        Author::Recovery(..) => unreachable!(),
    };
    let mut recipients = [(first.dev, first.hpke_pk, count(0x30, 32)), (rp.dev(&gboard), rp.hpke_pk, count(0x50, 32))];
    recipients.sort_by_key(|r| r.0);
    let wraps: Vec<[u8; 96]> = recipients
        .iter()
        .map(|(id, pk, e)| wrap_root(&gr, &gboard, 1, &commit, *id, pk, &mut FixedEntropy::new(e.clone())).unwrap())
        .collect();
    let genesis = CtlItem::Genesis(Genesis {
        salt,
        recovery: rp,
        device: GenesisDevice { dev: first.dev, sign_pk: first.sign_pk, hpke_pk: first.hpke_pk, name: "Laptop".into() },
        epoch: 1,
        commit,
        wraps,
        rules: 1,
        hash_version: 1,
        state_hash: [0x33; 32],
        snapshot: None,
    });
    let gentry = EntryId(count(0xe0, 16).try_into().unwrap());
    let gat = 1_767_225_600_000;
    let rauthor = Author::Recovery(rk, seed, gboard);
    let gpayload = ctl_payload(&[genesis], &rauthor.signer(), &gboard, 1, &gentry, gat).unwrap();

    let beat = Beat {
        rules: vec![1],
        envelopes: vec![(1, 1)],
        hash_versions: vec![1],
        state: Some(BeatState { seq: 128, hash: [0x44; 32], hash_version: 1 }),
        acks: vec![BeatAck { entry: 129, verdict: AckVerdict::Ack }],
    };
    let snap = SnapAnnounce {
        seq: 192,
        hash: [0x55; 32],
        hash_version: 1,
        blob: BlobId(count(0x10, 16).try_into().unwrap()),
        sha256: [0x66; 32],
        size: 1_500_000,
        parts: 2,
        inflated: 4_000_000,
        epoch: 3,
    };
    let at = 1_767_225_600_123;
    let case = |name, author, board, ctr, epoch, root, plain, entropy| Case { name, author, board, ctr, epoch, root, plain, entropy };
    let nonce = count(0x20, 24);
    let cases = vec![
        valid(&case("cmd by a device", &dev, BOARD, 1, 3, [0x11; 32], plain(Kind::Cmd, at, c0, b"{\"op\":\"x\"}".to_vec()), nonce.clone())),
        valid(&case("cmd of 444 bytes: two buckets", &dev, BOARD, 2, 3, [0x11; 32], plain(Kind::Cmd, at, head130, vec![0x61; 444]), nonce.clone())),
        valid(&case("beat by a device", &dev, BOARD, 3, 3, [0x11; 32], plain(Kind::Beat, at, head130, beat_payload(&beat).unwrap()), count(0x90, 24))),
        valid(&case("snap by a device", &dev, BOARD, 4, 3, [0x11; 32], plain(Kind::Snap, at, head200, snap_payload(&snap).unwrap()), count(0xb0, 24))),
        valid(&case(
            "Genesis by the recovery identity",
            &rauthor,
            gboard,
            1,
            1,
            groot,
            Plain { entry_id: gentry, kind: Kind::Ctl, at_ms: gat, based_on: Head { seq: 0, chain: chain_zero(&gboard) }, payload: gpayload },
            count(0xc0, 24),
        )),
        refused(&case("ctr 0", &dev, BOARD, 0, 3, [0x11; 32], plain(Kind::Cmd, at, c0, b"x".to_vec()), nonce.clone())),
        refused(&case(
            "based_on seq 0 with a chain other than C_0",
            &dev,
            BOARD,
            1,
            3,
            [0x11; 32],
            plain(Kind::Cmd, at, Head { seq: 0, chain: Chain([0; 32]) }, b"x".to_vec()),
            nonce.clone(),
        )),
        refused(&case("at_ms 253402300800000", &dev, BOARD, 1, 3, [0x11; 32], plain(Kind::Cmd, 253_402_300_800_000, c0, b"x".to_vec()), nonce.clone())),
        refused(&case("entropy one byte short", &dev, BOARD, 1, 3, [0x11; 32], plain(Kind::Cmd, at, c0, b"x".to_vec()), count(0x20, 23))),
    ];
    let mut cases = cases;

    // Frames the receiver rejects at step 1 (layout) and step 4 (signature).
    let good = seal(&case("", &dev, BOARD, 1, 3, [0x11; 32], plain(Kind::Cmd, at, c0, b"{\"op\":\"x\"}".to_vec()), nonce.clone())).unwrap();
    let mut layout = |name: &str, f: Vec<u8>| {
        let code = frame_header(&f).err().unwrap().code();
        cases.push(json!({ "name": name, "frame_bytes": hex(&f), "class": "Skip", "error": code, "step": 1 }));
    };
    layout("frame of 669 bytes", good[..669].to_vec());
    let mut m = good.clone();
    m[3] = b'2';
    layout("magic TDF2", m);
    let mut m = good.clone();
    let len = u32::from_be_bytes(m[44..48].try_into().unwrap());
    m[44..48].copy_from_slice(&(len + 1).to_be_bytes());
    layout("len one more than total − 112", m);
    let mut m = good.clone();
    m.push(0);
    layout("one byte appended", m);
    let sign_pk = dev.sign_pk();
    let vk = ed25519_dalek::VerifyingKey::from_bytes(&sign_pk).unwrap();
    for (name, i) in [("ciphertext byte flipped", 100), ("ctr flipped", 43), ("signature byte flipped", good.len() - 1)] {
        let mut m = good.clone();
        m[i] ^= 1;
        assert!(frame_header(&m).is_ok());
        let n = m.len();
        let signed = [b"taskdeck/v1/frame\0".as_slice(), &m[..n - 64]].concat();
        assert!(vk.verify_strict(&signed, &ed25519_dalek::Signature::from_bytes(m[n - 64..].try_into().unwrap())).is_err());
        cases.push(json!({
            "name": name, "frame_bytes": hex(&m), "sign_pk": hex(&sign_pk), "class": "Skip", "error": "seal/bad_signature", "step": 4,
        }));
    }
    check_or_write("seal-frame.json", json!({ "spec": "seal.md §8", "cases": cases }));
}
