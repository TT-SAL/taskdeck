//! `seal-wrap.json` (seal.md §7): the 96-byte wrap of a root to a device or
//! to the recovery identity, its opening, and the failures of tampered
//! wraps and of wraps opened in the wrong place.

use crate::common::hex;
use crate::{check_or_write, count};
use serde_json::{Map, Value, json};
use taskdeck_seal::{
    BoardId, DevId, DeviceKeys, EpochRoot, FixedEntropy, Recipient, RecoveryKeys, RecoverySeed, SealError, unwrap_root,
    wrap_root,
};

const BOARD: BoardId = BoardId([0x42; 16]);

enum Who {
    Device(DeviceKeys, Vec<u8>),
    Recovery(RecoveryKeys, Vec<u8>),
}

impl Who {
    fn device(entropy: Vec<u8>) -> Who {
        Who::Device(DeviceKeys::generate(&mut FixedEntropy::new(entropy.clone())).unwrap(), entropy)
    }
    fn recovery(seed: Vec<u8>) -> Who {
        Who::Recovery(RecoverySeed::generate(&mut FixedEntropy::new(seed.clone())).unwrap().keys(), seed)
    }
    fn id_pk(&self) -> (DevId, [u8; 32]) {
        match self {
            Who::Device(k, _) => (k.public().dev, k.public().hpke_pk),
            Who::Recovery(k, _) => (k.public().dev(&BOARD), k.public().hpke_pk),
        }
    }
    fn recipient(&self) -> Recipient<'_> {
        match self {
            Who::Device(k, _) => Recipient::Device(k),
            Who::Recovery(k, _) => Recipient::Recovery(k, BOARD),
        }
    }
    fn inputs(&self, m: &mut Map<String, Value>) {
        let (id, pk) = self.id_pk();
        match self {
            Who::Device(_, e) => {
                m.insert("recipient".into(), json!("device"));
                m.insert("recipient_entropy".into(), json!(hex(e)));
            }
            Who::Recovery(_, s) => {
                m.insert("recipient".into(), json!("recovery"));
                m.insert("recipient_seed".into(), json!(hex(s)));
            }
        }
        m.insert("recipient_id".into(), json!(hex(&id.0)));
        m.insert("recipient_pk".into(), json!(hex(&pk)));
    }
}

/// A case: wrap `root` for `who` under (board, e, commit of `committed`),
/// then open `tamper(wrap)` under the `open_*` values.
struct Case<'a> {
    name: String,
    who: &'a Who,
    epoch: u32,
    root: [u8; 32],
    committed: [u8; 32],
    entropy: Vec<u8>,
    open_board: BoardId,
    open_epoch: u32,
    flip: Option<(usize, u8)>,
}

fn build(c: &Case) -> Value {
    let root = EpochRoot::from_bytes(c.root);
    let commit = EpochRoot::from_bytes(c.committed).commitment(&BOARD, c.epoch);
    let (id, pk) = c.who.id_pk();
    let mut m = Map::new();
    m.insert("name".into(), json!(&c.name));
    m.insert("board_id".into(), json!(hex(&BOARD.0)));
    m.insert("epoch".into(), json!(c.epoch));
    m.insert("root".into(), json!(hex(&c.root)));
    m.insert("commitment".into(), json!(hex(&commit)));
    c.who.inputs(&mut m);
    m.insert("entropy".into(), json!(hex(&c.entropy)));
    let wrap = match wrap_root(&root, &BOARD, c.epoch, &commit, id, &pk, &mut FixedEntropy::new(c.entropy.clone())) {
        Ok(w) => w,
        Err(e) => {
            m.insert("error".into(), json!(e.code()));
            return Value::Object(m);
        }
    };
    m.insert("ephemeral_pk".into(), json!(hex(&wrap[16..48])));
    m.insert("wrap".into(), json!(hex(&wrap)));
    let mut opened = wrap;
    if let Some((i, mask)) = c.flip {
        opened[i] ^= mask;
        m.insert("opened_wrap".into(), json!(hex(&opened)));
    }
    m.insert("open_board_id".into(), json!(hex(&c.open_board.0)));
    m.insert("open_epoch".into(), json!(c.open_epoch));
    match unwrap_root(&opened, &c.open_board, c.open_epoch, &commit, &c.who.recipient()) {
        Ok(r) => m.insert("opened_root".into(), json!(hex(&*r.to_bytes()))),
        Err(e) => m.insert("error".into(), json!(e.code())),
    };
    Value::Object(m)
}

#[test]
fn seal_wrap() {
    let dev = Who::device(count(0, 80));
    let rec = Who::recovery(count(0xa0, 32));
    let root = [0x11; 32];
    let base = |name: &str, who| Case {
        name: name.to_string(),
        who,
        epoch: 3,
        root,
        committed: root,
        entropy: count(0x20, 32),
        open_board: BOARD,
        open_epoch: 3,
        flip: None,
    };
    let mut cases = vec![
        build(&base("wrap to a device", &dev)),
        build(&base("wrap to the recovery identity", &rec)),
        build(&Case { entropy: count(0xc0, 32), root: count(1, 32).try_into().unwrap(), committed: count(1, 32).try_into().unwrap(), epoch: 1, open_epoch: 1, ..base("wrap to a device, epoch 1, another root", &dev) }),
    ];
    for (i, mask) in [(0usize, 0x01u8), (15, 0x80), (16, 0x01), (47, 0x40), (48, 0x01), (79, 0x10), (80, 0x01), (95, 0x80)] {
        let name = format!("byte {i} flipped with {mask:#04x}");
        cases.push(build(&Case { flip: Some((i, mask)), ..base(&name, &dev) }));
    }
    cases.push(build(&Case { open_epoch: 4, ..base("opened as another epoch", &dev) }));
    cases.push(build(&Case { open_board: BoardId([0x43; 16]), ..base("opened on another board", &dev) }));
    cases.push(build(&Case { committed: [0x12; 32], ..base("root does not match the commitment", &dev) }));
    cases.push(build(&Case { entropy: count(0x20, 31), ..base("entropy one byte short", &dev) }));
    // A wrap opened by a recipient it is not for.
    let other = Who::device(count(0x80, 80));
    let mut m = build(&base("opened by another device", &dev));
    let w: [u8; 96] = crate::common::unhex_n(m["wrap"].as_str().unwrap());
    let commit = EpochRoot::from_bytes(root).commitment(&BOARD, 3);
    let got = unwrap_root(&w, &BOARD, 3, &commit, &other.recipient()).err().unwrap();
    assert_eq!(got, SealError::Commitment);
    let obj = m.as_object_mut().unwrap();
    obj.remove("opened_root");
    if let Who::Device(_, e) = &other {
        obj.insert("opener_entropy".into(), json!(hex(e)));
    }
    obj.insert("error".into(), json!(got.code()));
    cases.push(m);
    // A small-order recipient key: the X25519 output is all zero.
    let root_e = EpochRoot::from_bytes(root);
    let commit3 = root_e.commitment(&BOARD, 3);
    let e = wrap_root(&root_e, &BOARD, 3, &commit3, DevId([0x77; 16]), &[0; 32], &mut FixedEntropy::new(count(0x20, 32))).err().unwrap();
    cases.push(json!({
        "name": "small-order recipient key",
        "board_id": hex(&BOARD.0),
        "epoch": 3,
        "root": hex(&root),
        "commitment": hex(&commit3),
        "recipient_id": hex(&[0x77; 16]),
        "recipient_pk": hex(&[0; 32]),
        "entropy": hex(&count(0x20, 32)),
        "error": e.code(),
    }));
    check_or_write("seal-wrap.json", json!({ "spec": "seal.md §7", "cases": cases }));
}
