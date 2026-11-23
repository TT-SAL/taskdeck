use super::*;
use crate::{DeviceKeys, OsEntropy, RecoverySeed};
use proptest::prelude::*;
use serde_json::json;

// ---------------------------------------------------------------- helpers

/// `v` with the member at JSON pointer `ptr` set to `new`.
fn set(v: &Value, ptr: &str, new: Value) -> Value {
    let mut v = v.clone();
    let (parent, key) = ptr.rsplit_once('/').unwrap();
    match v.pointer_mut(parent).unwrap() {
        Value::Object(m) => {
            m.insert(key.into(), new);
        }
        Value::Array(a) => a[key.parse::<usize>().unwrap()] = new,
        _ => panic!("{ptr}"),
    }
    v
}

/// `v` without the member at `ptr`.
fn del(v: &Value, ptr: &str) -> Value {
    let mut v = v.clone();
    let (parent, key) = ptr.rsplit_once('/').unwrap();
    v.pointer_mut(parent).unwrap().as_object_mut().unwrap().remove(key).unwrap();
    v
}

fn hx(b: &[u8]) -> Value {
    hex_value(b)
}

/// A wrap whose recipient id is `[id; 16]`.
fn w(id: u8) -> [u8; 96] {
    let mut x = [0x77u8; 96];
    x[..16].copy_from_slice(&[id; 16]);
    x
}

fn genesis() -> CtlItem {
    CtlItem::Genesis(Genesis {
        salt: [1; 16],
        recovery: RecoveryPublic { sign_pk: [2; 32], hpke_pk: [3; 32] },
        device: GenesisDevice { dev: DevId([4; 16]), sign_pk: [5; 32], hpke_pk: [6; 32], name: "Laptop \"é\"".into() },
        epoch: 1,
        commit: [7; 32],
        wraps: vec![w(0x04), w(0x09)],
        rules: 1,
        hash_version: 1,
        state_hash: [8; 32],
        snapshot: Some(GenesisSnapshot { blob: BlobId([9; 16]), sha256: [10; 32], size: 1_048_577, parts: 2, inflated: 5 }),
    })
}

fn device_added() -> CtlItem {
    CtlItem::DeviceAdded(DeviceAdded {
        dev: DevId([0x11; 16]),
        sign_pk: [0x12; 32],
        hpke_pk: [0x13; 32],
        name: "Phone".into(),
        transcript: [0x14; 32],
        bootstrap_seq: 64,
    })
}

fn rotated(ids: &[u8]) -> CtlItem {
    CtlItem::EpochRotated(EpochRotated {
        epoch: 3,
        commit: [0x21; 32],
        wraps: ids.iter().map(|&i| w(i)).collect(),
        purge_before: Some(10),
    })
}

fn rkc(mode: RecoveryKeyMode) -> CtlItem {
    let keys = !matches!(mode, RecoveryKeyMode::Cancel);
    CtlItem::RecoveryKeyChange(RecoveryKeyChange {
        mode,
        sign_pk: keys.then_some([0x31; 32]),
        hpke_pk: keys.then_some([0x32; 32]),
    })
}

fn fence() -> CtlItem {
    CtlItem::Fence(Fence { host: "relay.example.org:8443".into(), door: [0x41; 5], home_gen: 2 })
}

fn reset() -> CtlItem {
    CtlItem::ResetTo(ResetTo { seq: 5, state_hash: [0x51; 32], hash_version: 1 })
}

fn skip(seq: u64) -> CtlItem {
    CtlItem::Skip(SkipItem { seq, frame_hash: FrameHash([0x61; 32]) })
}

fn upgrade() -> CtlItem {
    CtlItem::RulesUpgrade(RulesUpgrade { rules: 2, mode: RulesUpgradeMode::Override })
}

fn every_item() -> Vec<CtlItem> {
    vec![
        genesis(),
        device_added(),
        CtlItem::DeviceRevoked(DevId([0x15; 16])),
        CtlItem::DeviceReinstated(DevId([0x16; 16])),
        rotated(&[1, 2, 3]),
        rkc(RecoveryKeyMode::Direct),
        rkc(RecoveryKeyMode::Propose),
        rkc(RecoveryKeyMode::Confirm),
        rkc(RecoveryKeyMode::Cancel),
        fence(),
        reset(),
        skip(7),
        upgrade(),
        CtlItem::RulesUpgrade(RulesUpgrade { rules: VERSION_MAX, mode: RulesUpgradeMode::Normal }),
    ]
}

fn item_err(v: &Value) -> Option<JsonError> {
    item_from_value(v).err()
}

fn check_table(base: &Value, cases: Vec<(&str, Value, JsonError)>, parse: impl Fn(&Value) -> Option<JsonError>) {
    assert_eq!(parse(base), None, "the base value is valid");
    for (what, v, want) in cases {
        assert_eq!(parse(&v), Some(want), "{what}: {v}");
    }
}

// ------------------------------------------------------------ items (§12)

// Every item's value parses back to the same item, and its JSON is the
// object of its table with the fields named as there.
// spec: SEAL-CTL-003
// spec: SEAL-ENC-003
#[test]
fn every_item_round_trips() {
    for item in every_item() {
        let v = checked_item_value(&item).unwrap();
        assert_eq!(item_from_value(&v).unwrap(), item);
        let bytes = canon::encode(&v).unwrap();
        assert_eq!(canon::parse(&bytes).unwrap(), v);
    }
    let v = item_value(&skip(7)).unwrap();
    assert_eq!(
        String::from_utf8(canon::encode(&v).unwrap()).unwrap(),
        format!(r#"{{"frame_hash":"{}","seq":7,"type":"Skip"}}"#, "61".repeat(32))
    );
    let v = item_value(&rkc(RecoveryKeyMode::Cancel)).unwrap();
    assert_eq!(canon::encode(&v).unwrap(), br#"{"mode":"cancel","type":"RecoveryKeyChange"}"#);
    let v = item_value(&CtlItem::DeviceRevoked(DevId([0xab; 16]))).unwrap();
    assert_eq!(
        String::from_utf8(canon::encode(&v).unwrap()).unwrap(),
        format!(r#"{{"dev":"{}","type":"DeviceRevoked"}}"#, "ab".repeat(16))
    );
    // Genesis without a snapshot, and a rotation without purge_before, omit the field.
    let mut g = genesis();
    if let CtlItem::Genesis(x) = &mut g {
        x.snapshot = None;
    }
    let v = checked_item_value(&g).unwrap();
    assert!(v.get("snapshot").is_none());
    assert_eq!(item_from_value(&v).unwrap(), g);
    let mut r = rotated(&[1, 2]);
    if let CtlItem::EpochRotated(x) = &mut r {
        x.purge_before = None;
    }
    let v = checked_item_value(&r).unwrap();
    assert!(v.get("purge_before").is_none());
    assert_eq!(item_from_value(&v).unwrap(), r);
}

// §12.2, each constraint violated once.
// spec: SEAL-CTL-003
// spec: SEAL-ENC-004
// spec: SEAL-ENC-007
#[test]
fn genesis_table() {
    let g = item_value(&genesis()).unwrap();
    use JsonError::*;
    check_table(
        &g,
        vec![
            ("unknown type", set(&g, "/type", json!("Genesys")), UnknownItem),
            ("type not a string", set(&g, "/type", json!(1)), WrongType),
            ("no type", del(&g, "/type"), MissingField),
            ("unknown field", set(&g, "/extra", json!(1)), UnknownField),
            ("no salt", del(&g, "/salt"), MissingField),
            ("salt of 15 bytes", set(&g, "/salt", hx(&[1; 15])), BadHex),
            ("salt uppercase", set(&g, "/salt", json!("AB".repeat(16))), BadHex),
            ("recovery not an object", set(&g, "/recovery", json!("x")), WrongType),
            ("recovery without sign_pk", del(&g, "/recovery/sign_pk"), MissingField),
            ("recovery without hpke_pk", del(&g, "/recovery/hpke_pk"), MissingField),
            ("recovery with an unknown field", set(&g, "/recovery/x", hx(&[0; 32])), UnknownField),
            ("recovery hpke_pk of 31 bytes", set(&g, "/recovery/hpke_pk", hx(&[0; 31])), BadHex),
            ("device without dev", del(&g, "/device/dev"), MissingField),
            ("device dev of 17 bytes", set(&g, "/device/dev", hx(&[0; 17])), BadHex),
            ("device sign_pk bad", set(&g, "/device/sign_pk", json!("zz")), BadHex),
            ("device hpke_pk missing", del(&g, "/device/hpke_pk"), MissingField),
            ("device with an unknown field", set(&g, "/device/no", json!(1)), UnknownField),
            ("device name empty", set(&g, "/device/name", json!("")), StringLimit),
            ("device name 65 bytes", set(&g, "/device/name", json!("x".repeat(65))), StringLimit),
            ("device name with U+0007", set(&g, "/device/name", json!("a\u{7}")), StringLimit),
            ("device name with U+0085", set(&g, "/device/name", json!("a\u{85}")), StringLimit),
            ("device name not a string", set(&g, "/device/name", json!(3)), WrongType),
            ("epoch 2", set(&g, "/epoch", json!(2)), IntRange),
            ("epoch 0", set(&g, "/epoch", json!(0)), IntRange),
            ("commit of 33 bytes", set(&g, "/commit", hx(&[0; 33])), BadHex),
            ("one wrap", set(&g, "/wraps", json!([hx(&w(4))])), ArrayCount),
            ("three wraps", set(&g, "/wraps", json!([hx(&w(1)), hx(&w(4)), hx(&w(9))])), ArrayCount),
            ("wraps descending", set(&g, "/wraps", json!([hx(&w(9)), hx(&w(4))])), ArrayOrder),
            ("wraps to one recipient", set(&g, "/wraps", json!([hx(&w(4)), hx(&w(4))])), ArrayOrder),
            ("a wrap of 95 bytes", set(&g, "/wraps/1", hx(&[9; 95])), BadHex),
            ("wraps not an array", set(&g, "/wraps", hx(&w(4))), WrongType),
            ("rules 2", set(&g, "/rules", json!(2)), IntRange),
            ("hash_version 0", set(&g, "/hash_version", json!(0)), IntRange),
            ("no state_hash", del(&g, "/state_hash"), MissingField),
            ("snapshot size 66", set(&g, "/snapshot/size", json!(66)), IntRange),
            ("snapshot size 67108865", set(&g, "/snapshot/size", json!(67_108_865)), IntRange),
            ("snapshot parts not ceil(size / 2^20)", set(&g, "/snapshot/parts", json!(1)), Constraint),
            ("snapshot inflated 0", set(&g, "/snapshot/inflated", json!(0)), IntRange),
            ("snapshot inflated 67108865", set(&g, "/snapshot/inflated", json!(67_108_865)), IntRange),
            ("snapshot blob of 15 bytes", set(&g, "/snapshot/blob", hx(&[0; 15])), BadHex),
            ("snapshot without sha256", del(&g, "/snapshot/sha256"), MissingField),
            ("snapshot with an unknown field", set(&g, "/snapshot/x", json!(1)), UnknownField),
        ],
        item_err,
    );
    // The bounds: size 67 with 1 part, size 2^26 with 64 parts.
    let ok = set(&set(&g, "/snapshot/size", json!(67)), "/snapshot/parts", json!(1));
    assert_eq!(item_err(&ok), None);
    let ok = set(&set(&g, "/snapshot/size", json!(67_108_864)), "/snapshot/parts", json!(64));
    assert_eq!(item_err(&ok), None);
    assert_eq!(item_err(&set(&g, "/device/name", json!("x".repeat(64)))), None);
}

// §12.3 to §12.10, each constraint violated once.
// spec: SEAL-CTL-003
// spec: SEAL-ENC-004
// spec: SEAL-ENC-006
// spec: SEAL-ENC-007
#[test]
fn the_other_item_tables() {
    use JsonError::*;
    let a = item_value(&device_added()).unwrap();
    check_table(
        &a,
        vec![
            ("unknown field", set(&a, "/x", json!(1)), UnknownField),
            ("no dev", del(&a, "/dev"), MissingField),
            ("dev of 15 bytes", set(&a, "/dev", hx(&[0; 15])), BadHex),
            ("sign_pk bad", set(&a, "/sign_pk", hx(&[0; 16])), BadHex),
            ("no hpke_pk", del(&a, "/hpke_pk"), MissingField),
            ("name of 65 bytes", set(&a, "/name", json!("é".repeat(33))), StringLimit),
            ("name with U+001F", set(&a, "/name", json!("\u{1f}")), StringLimit),
            ("transcript bad", set(&a, "/transcript", json!("00")), BadHex),
            ("bootstrap_seq 0", set(&a, "/bootstrap_seq", json!(0)), IntRange),
            ("bootstrap_seq 2^53", set(&a, "/bootstrap_seq", json!(1u64 << 53)), IntRange),
            ("bootstrap_seq a string", set(&a, "/bootstrap_seq", json!("1")), WrongType),
        ],
        item_err,
    );
    for target in [CtlItem::DeviceRevoked(DevId([1; 16])), CtlItem::DeviceReinstated(DevId([1; 16]))] {
        let t = item_value(&target).unwrap();
        check_table(
            &t,
            vec![
                ("no dev", del(&t, "/dev"), MissingField),
                ("dev of 16 uppercase bytes", set(&t, "/dev", json!("AB".repeat(16))), BadHex),
                ("unknown field", set(&t, "/epoch", json!(2)), UnknownField),
            ],
            item_err,
        );
    }
    let r = item_value(&rotated(&[1, 2, 3])).unwrap();
    let many: Vec<Value> = (1..=18).map(|i| hx(&w(i))).collect();
    check_table(
        &r,
        vec![
            ("epoch 1", set(&r, "/epoch", json!(1)), IntRange),
            ("epoch 2^32", set(&r, "/epoch", json!(1u64 << 32)), IntRange),
            ("no commit", del(&r, "/commit"), MissingField),
            ("one wrap", set(&r, "/wraps", json!([hx(&w(1))])), ArrayCount),
            ("18 wraps", set(&r, "/wraps", Value::Array(many.clone())), ArrayCount),
            ("wraps out of order", set(&r, "/wraps", json!([hx(&w(1)), hx(&w(3)), hx(&w(2))])), ArrayOrder),
            ("a recipient twice", set(&r, "/wraps", json!([hx(&w(1)), hx(&w(1))])), ArrayOrder),
            ("purge_before 0", set(&r, "/purge_before", json!(0)), IntRange),
            ("purge_before a float", set(&r, "/purge_before", json!(1.5)), Float),
            ("unknown field", set(&r, "/x", json!(1)), UnknownField),
        ],
        item_err,
    );
    assert_eq!(item_err(&set(&r, "/wraps", Value::Array(many[..17].to_vec()))), None);
    assert_eq!(item_err(&set(&r, "/epoch", json!(u32::MAX))), None);

    let k = item_value(&rkc(RecoveryKeyMode::Propose)).unwrap();
    let c = item_value(&rkc(RecoveryKeyMode::Cancel)).unwrap();
    check_table(
        &k,
        vec![
            ("mode unknown", set(&k, "/mode", json!("Propose")), Constraint),
            ("no mode", del(&k, "/mode"), MissingField),
            ("propose without sign_pk", del(&k, "/sign_pk"), MissingField),
            ("propose without hpke_pk", del(&k, "/hpke_pk"), MissingField),
            ("direct without sign_pk", set(&del(&k, "/sign_pk"), "/mode", json!("direct")), MissingField),
            ("confirm without hpke_pk", set(&del(&k, "/hpke_pk"), "/mode", json!("confirm")), MissingField),
            ("sign_pk bad", set(&k, "/sign_pk", hx(&[0; 33])), BadHex),
            ("cancel with keys", set(&k, "/mode", json!("cancel")), UnknownField),
            ("cancel with sign_pk", set(&c, "/sign_pk", hx(&[0; 32])), UnknownField),
        ],
        item_err,
    );
    assert_eq!(item_err(&c), None);

    let f = item_value(&fence()).unwrap();
    check_table(
        &f,
        vec![
            ("host with uppercase", set(&f, "/host", json!("Relay.example.org")), Host),
            ("host with port 443", set(&f, "/host", json!("relay.example.org:443")), Host),
            ("host with a leading zero port", set(&f, "/host", json!("relay.example.org:0443")), Host),
            ("host with a path", set(&f, "/host", json!("relay.example.org/x")), Host),
            ("host name of 254 bytes", set(&f, "/host", json!(format!("{}.{}.{}.{}", "a".repeat(63), "b".repeat(63), "c".repeat(63), "d".repeat(62)))), Host),
            ("no host", del(&f, "/host"), MissingField),
            ("door of 4 bytes", set(&f, "/door", hx(&[0; 4])), BadHex),
            ("home_gen 1", set(&f, "/home_gen", json!(1)), IntRange),
            ("home_gen 2^32", set(&f, "/home_gen", json!(1u64 << 32)), IntRange),
            ("unknown field", set(&f, "/port", json!(1)), UnknownField),
        ],
        item_err,
    );
    let t = item_value(&reset()).unwrap();
    check_table(
        &t,
        vec![
            ("seq 0", set(&t, "/seq", json!(0)), IntRange),
            ("no state_hash", del(&t, "/state_hash"), MissingField),
            ("hash_version 0", set(&t, "/hash_version", json!(0)), IntRange),
            ("hash_version 2^31", set(&t, "/hash_version", json!(1u64 << 31)), IntRange),
            ("unknown field", set(&t, "/x", json!(1)), UnknownField),
        ],
        item_err,
    );
    let s = item_value(&skip(3)).unwrap();
    check_table(
        &s,
        vec![
            ("seq 0", set(&s, "/seq", json!(0)), IntRange),
            ("frame_hash of 31 bytes", set(&s, "/frame_hash", hx(&[0; 31])), BadHex),
            ("no frame_hash", del(&s, "/frame_hash"), MissingField),
            ("unknown field", set(&s, "/x", json!(1)), UnknownField),
        ],
        item_err,
    );
    let u = item_value(&upgrade()).unwrap();
    check_table(
        &u,
        vec![
            ("rules 1", set(&u, "/rules", json!(1)), IntRange),
            ("rules 2^31", set(&u, "/rules", json!(1u64 << 31)), IntRange),
            ("mode unknown", set(&u, "/mode", json!("force")), Constraint),
            ("no mode", del(&u, "/mode"), MissingField),
            ("unknown field", set(&u, "/x", json!(1)), UnknownField),
        ],
        item_err,
    );
}

// An encoder refuses every item its parser rejects: the public
// `ctl_payload` with an invalid item fails, a host with `seal/host_format`
// (SEAL-ENC-006) and anything else with `seal/bad_payload`.
// spec: SEAL-CTL-003
// spec: SEAL-ENC-006
// spec: SEAL-ENC-007
#[test]
fn ctl_payload_refuses_invalid_items() {
    let d = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let board = BoardId([0xB0; 16]);
    let call = |items: &[CtlItem]| {
        crate::ctl_payload(items, &crate::Signer::Device(&d), &board, 4, &EntryId([1; 16]), 1000).err()
    };
    let bad_host = CtlItem::Fence(Fence { host: "relay.example.org:443".into(), door: [0; 5], home_gen: 2 });
    assert_eq!(call(&[bad_host]), Some(SealError::HostFormat));
    let bad_gen = CtlItem::Fence(Fence { host: "relay.example.org".into(), door: [0; 5], home_gen: 1 });
    assert_eq!(call(&[bad_gen]), Some(SealError::BadPayload));
    let mut g = genesis();
    if let CtlItem::Genesis(x) = &mut g {
        x.wraps.pop();
    }
    assert_eq!(call(&[g]), Some(SealError::BadPayload));
    let mut a = device_added();
    if let CtlItem::DeviceAdded(x) = &mut a {
        x.name = String::new();
    }
    assert_eq!(call(&[a]), Some(SealError::BadPayload));
    let mut a = device_added();
    if let CtlItem::DeviceAdded(x) = &mut a {
        x.bootstrap_seq = 1 << 53;
    }
    assert_eq!(call(&[a]), Some(SealError::BadPayload));
    let half = CtlItem::RecoveryKeyChange(RecoveryKeyChange { mode: RecoveryKeyMode::Direct, sign_pk: Some([1; 32]), hpke_pk: None });
    assert_eq!(call(&[half, rotated(&[1, 2])]), Some(SealError::BadPayload));
    let cancel_keys = CtlItem::RecoveryKeyChange(RecoveryKeyChange { mode: RecoveryKeyMode::Cancel, sign_pk: Some([1; 32]), hpke_pk: None });
    assert_eq!(call(&[cancel_keys]), Some(SealError::BadPayload));
    assert_eq!(call(&[rotated(&[2, 1])]), Some(SealError::BadPayload));
    assert_eq!(call(&[]), Some(SealError::BadPayload));
    let seventeen: Vec<CtlItem> = (1..=17).map(skip).collect();
    assert_eq!(call(&seventeen), Some(SealError::BadPayload));
    assert_eq!(call(&seventeen[..16]), None);
}

// ------------------------------------------------------- shapes (§12.1)

// Each shape is accepted; the near misses are `seal/ctl_shape`.
// spec: SEAL-CTL-001
#[test]
fn the_shapes_of_12_1() {
    let rev = |i: u8| CtlItem::DeviceRevoked(DevId([i; 16]));
    let rei = |i: u8| CtlItem::DeviceReinstated(DevId([i; 16]));
    let ok: Vec<(Vec<CtlItem>, Shape)> = vec![
        (vec![genesis()], Shape::G),
        (vec![device_added()], Shape::A),
        (vec![rotated(&[1, 2])], Shape::R),
        (vec![rev(1), rotated(&[2, 3])], Shape::R),
        (vec![rev(1), rei(2), rev(3), rotated(&[2, 3])], Shape::R),
        ((1..=15).map(rev).chain([rotated(&[1, 2])]).collect(), Shape::R),
        (vec![rkc(RecoveryKeyMode::Direct), rotated(&[1, 2])], Shape::K),
        (vec![rkc(RecoveryKeyMode::Confirm), rotated(&[1, 2])], Shape::K),
        (vec![rkc(RecoveryKeyMode::Propose)], Shape::P),
        (vec![rkc(RecoveryKeyMode::Cancel)], Shape::P),
        (vec![fence()], Shape::F),
        (vec![reset()], Shape::T),
        (vec![skip(1)], Shape::S),
        ((1..=16).map(skip).collect(), Shape::S),
        (vec![upgrade()], Shape::U),
    ];
    for (items, want) in ok {
        assert_eq!(ctl_shape(&items), Ok(want), "{items:?}");
    }
    let bad: Vec<Vec<CtlItem>> = vec![
        vec![],
        vec![genesis(), genesis()],
        vec![genesis(), fence()],
        vec![device_added(), device_added()],
        vec![device_added(), rotated(&[1, 2])],
        vec![rev(1)],
        vec![rotated(&[1, 2]), rev(1)],
        vec![rev(1), rev(1), rotated(&[1, 2])],
        vec![rev(1), rei(1), rotated(&[1, 2])],
        (1..=16).map(rev).chain([rotated(&[1, 2])]).collect(),
        vec![rotated(&[1, 2]), rotated(&[1, 2])],
        vec![rev(1), fence(), rotated(&[1, 2])],
        vec![rkc(RecoveryKeyMode::Direct)],
        vec![rkc(RecoveryKeyMode::Confirm)],
        vec![rkc(RecoveryKeyMode::Propose), rotated(&[1, 2])],
        vec![rkc(RecoveryKeyMode::Cancel), rotated(&[1, 2])],
        vec![rev(1), rkc(RecoveryKeyMode::Direct), rotated(&[1, 2])],
        vec![rkc(RecoveryKeyMode::Direct), rotated(&[1, 2]), rev(1)],
        vec![fence(), fence()],
        vec![reset(), skip(1)],
        vec![skip(1), skip(1)],
        (1..=17).map(skip).collect(),
        vec![skip(1), fence()],
        vec![upgrade(), upgrade()],
    ];
    for items in bad {
        assert_eq!(ctl_shape(&items), Err(SealError::CtlShape), "{items:?}");
    }
}

// `ctl_payload` refuses a list of items that forms no shape.
// spec: SEAL-CTL-001
#[test]
fn ctl_payload_refuses_a_list_with_no_shape() {
    let d = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let board = BoardId([0xB0; 16]);
    let r = crate::ctl_payload(&[fence(), reset()], &crate::Signer::Device(&d), &board, 1, &EntryId([0; 16]), 0);
    assert_eq!(r.err(), Some(SealError::CtlShape));
}

// ------------------------------------------------------------ ctl (§11.3)

struct Signed {
    payload: Vec<u8>,
    board: BoardId,
    dev: DevId,
    sign_pk: [u8; 32],
}

fn signed(items: &[CtlItem], d: &DeviceKeys) -> Signed {
    let board = BoardId([0xB0; 16]);
    let payload = crate::ctl_payload(items, &crate::Signer::Device(d), &board, 9, &EntryId([0xE1; 16]), 1234).unwrap();
    Signed { payload, board, dev: d.public().dev, sign_pk: d.public().sign_pk }
}

fn cx(s: &Signed) -> CtlContext<'_> {
    CtlContext { board: &s.board, dev: &s.dev, ctr: 9, entry_id: &EntryId([0xE1; 16]), at_ms: 1234, sign_pk: &s.sign_pk }
}

// The payload is `{"items": …, "sig": …}` and `sig` the author's signature
// over `ctx("ctl") ‖ board_id ‖ dev ‖ u64 ctr ‖ entry_id ‖ i64 at_ms ‖
// H(enc(items))`, laid out here by hand.
// spec: SEAL-KIND-003
// spec: SEAL-CTX-001
#[test]
fn the_inner_signature_layout() {
    let d = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let s = signed(&[fence()], &d);
    let v: Value = serde_json::from_slice(&s.payload).unwrap();
    let items_bytes = canon::encode(&v["items"]).unwrap();
    let text = String::from_utf8(s.payload.clone()).unwrap();
    let sig_hex = v["sig"].as_str().unwrap();
    assert_eq!(text, format!("{{\"items\":{},\"sig\":\"{sig_hex}\"}}", String::from_utf8(items_bytes.clone()).unwrap()));
    let mut inner = b"taskdeck/v1/ctl\0".to_vec();
    inner.extend_from_slice(&[0xB0; 16]);
    inner.extend_from_slice(&s.dev.0);
    inner.extend_from_slice(&9u64.to_be_bytes());
    inner.extend_from_slice(&[0xE1; 16]);
    inner.extend_from_slice(&1234i64.to_be_bytes());
    inner.extend_from_slice(&<sha2::Sha256 as sha2::Digest>::digest(&items_bytes));
    let sig: [u8; 64] = hex::<64>(&v["sig"]).unwrap();
    let vk = ed25519_dalek::VerifyingKey::from_bytes(&s.sign_pk).unwrap();
    vk.verify_strict(&inner, &ed25519_dalek::Signature::from_bytes(&sig)).unwrap();
    assert_eq!(parse_ctl(&s.payload, &cx(&s)).unwrap(), vec![fence()]);
}

// The receiver verifies the inner signature with the author's key over the
// frame's board, dev, ctr and the plaintext's entry_id and at_ms; any other
// value fails with `seal/bad_ctl_sig`.
// spec: SEAL-KIND-004
// spec: SEAL-KIND-003
#[test]
fn the_inner_signature_is_verified() {
    let d = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let other = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let s = signed(&[rev_item(1), rotated(&[1, 2])], &d);
    let good = cx(&s);
    assert!(parse_ctl(&s.payload, &good).is_ok());
    let b2 = BoardId([0xB1; 16]);
    let d2 = DevId([0; 16]);
    let pk2 = other.public().sign_pk;
    let e2 = EntryId([0xE2; 16]);
    let cases = [
        CtlContext { board: &b2, ..cx(&s) },
        CtlContext { dev: &d2, ..cx(&s) },
        CtlContext { ctr: 10, ..cx(&s) },
        CtlContext { entry_id: &e2, ..cx(&s) },
        CtlContext { at_ms: 1235, ..cx(&s) },
        CtlContext { sign_pk: &pk2, ..cx(&s) },
        CtlContext { sign_pk: &[0; 32], ..cx(&s) },
    ];
    for c in &cases {
        assert_eq!(parse_ctl(&s.payload, c).err(), Some(SealError::BadCtlSig));
    }
    // Another valid item list under the same signature.
    let v: Value = serde_json::from_slice(&s.payload).unwrap();
    let swapped = set(&v, "/items/0/dev", hx(&[2; 16]));
    assert_eq!(parse_ctl(&canon::encode(&swapped).unwrap(), &good).err(), Some(SealError::BadCtlSig));
    // A signature by the recovery identity verifies under its key and dev_R.
    let seed = RecoverySeed::generate(&mut OsEntropy).unwrap();
    let rk = seed.keys();
    let dev_r = rk.public().dev(&s.board);
    let p = crate::ctl_payload(&[skip(3)], &crate::Signer::Recovery(&rk, s.board), &s.board, 2, &EntryId([1; 16]), 5).unwrap();
    let rpk = rk.public().sign_pk;
    let rc = CtlContext { board: &s.board, dev: &dev_r, ctr: 2, entry_id: &EntryId([1; 16]), at_ms: 5, sign_pk: &rpk };
    assert_eq!(parse_ctl(&p, &rc).unwrap(), vec![skip(3)]);
    let other_board = crate::ctl_payload(&[skip(3)], &crate::Signer::Recovery(&rk, b2), &s.board, 2, &EntryId([1; 16]), 5);
    assert_eq!(other_board.err(), Some(SealError::UnknownAuthor));
}

fn rev_item(i: u8) -> CtlItem {
    CtlItem::DeviceRevoked(DevId([i; 16]))
}

// §11.3's table: 1 to 16 items, a 64-byte `sig`, nothing else; canonical
// bytes only. Each is `seal/bad_payload` at step 15.
// spec: SEAL-KIND-003
// spec: SEAL-ENC-003
// spec: SEAL-ENC-004
#[test]
fn the_ctl_table() {
    let d = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let s = signed(&[skip(1), skip(2)], &d);
    let v: Value = serde_json::from_slice(&s.payload).unwrap();
    let un = |v: &Value| parse_ctl_unsigned(&canon::encode(v).unwrap()).err();
    use JsonError::*;
    let item = v["items"][0].clone();
    check_table(
        &v,
        vec![
            ("no items", set(&v, "/items", json!([])), ArrayCount),
            ("17 items", set(&v, "/items", Value::Array(vec![item.clone(); 17])), ArrayCount),
            ("items not an array", set(&v, "/items", item.clone()), WrongType),
            ("an invalid item", set(&v, "/items/1/seq", json!(0)), IntRange),
            ("an item of no type", set(&v, "/items/0", json!({"seq": 1})), MissingField),
            ("sig of 63 bytes", set(&v, "/sig", hx(&[0; 63])), BadHex),
            ("no sig", del(&v, "/sig"), MissingField),
            ("no items field", del(&v, "/items"), MissingField),
            ("unknown field", set(&v, "/at_ms", json!(1)), UnknownField),
        ],
        un,
    );
    for bad in [
        set(&v, "/items", Value::Array(vec![item.clone(); 17])),
        set(&v, "/sig", hx(&[0; 63])),
        set(&v, "/x", json!(1)),
    ] {
        assert_eq!(parse_ctl(&canon::encode(&bad).unwrap(), &cx(&s)).err(), Some(SealError::BadPayload));
    }
    // Not canonical: whitespace, and the members in another order.
    let text = String::from_utf8(s.payload.clone()).unwrap();
    let spaced = text.replacen(':', ": ", 1);
    assert_eq!(parse_ctl_unsigned(spaced.as_bytes()).err(), Some(NotCanonical));
    let sig = v["sig"].as_str().unwrap();
    let items = String::from_utf8(canon::encode(&v["items"]).unwrap()).unwrap();
    let reordered = format!("{{\"sig\":\"{sig}\",\"items\":{items}}}");
    assert_eq!(parse_ctl_unsigned(reordered.as_bytes()).err(), Some(NotCanonical));
    assert_eq!(parse_ctl(reordered.as_bytes(), &cx(&s)).err(), Some(SealError::BadPayload));
    // A duplicate key and a null are refused before the schema.
    let dup = text.replacen("{\"items\":", "{\"sig\":\"00\",\"items\":", 1);
    assert_eq!(parse_ctl_unsigned(dup.as_bytes()).err(), Some(DuplicateKey));
    let dup = text.replacen("\"seq\":1,", "\"seq\":1,\"seq\":1,", 1);
    assert_eq!(parse_ctl_unsigned(dup.as_bytes()).err(), Some(DuplicateKey));
    let null = text.replacen("\"seq\":1", "\"seq\":null", 1);
    assert_eq!(parse_ctl_unsigned(null.as_bytes()).err(), Some(Null));
    let float = text.replacen("\"seq\":1", "\"seq\":1.0", 1);
    assert_eq!(parse_ctl_unsigned(float.as_bytes()).err(), Some(Float));
}

// ----------------------------------------------------------- beat (§11.4)

fn beat() -> Beat {
    Beat {
        rules: vec![1, 2],
        envelopes: vec![(1, 1), (1, 2), (2, 1)],
        hash_versions: vec![1],
        state: Some(BeatState { seq: 128, hash: [0xAA; 32], hash_version: 1 }),
        acks: vec![
            BeatAck { entry: 100, verdict: AckVerdict::Ack },
            BeatAck { entry: 140, verdict: AckVerdict::Mismatch },
        ],
    }
}

const BEAT_CX: BeatContext = BeatContext { seq: 150, based_on_seq: 149 };

fn beat_err(v: &Value) -> Option<JsonError> {
    parse_beat_json(&canon::encode(v).unwrap(), Some(BEAT_CX)).err()
}

// The beat's JSON round-trips; `acks` at its default `[]` is omitted and
// `state` when absent.
// spec: SEAL-KIND-005
// spec: SEAL-ENC-003
#[test]
fn a_beat_round_trips() {
    let b = beat();
    let bytes = crate::beat_payload(&b).unwrap();
    assert_eq!(parse_beat(&bytes, BEAT_CX).unwrap(), b);
    let text = String::from_utf8(bytes).unwrap();
    assert_eq!(
        text,
        format!(
            r#"{{"acks":[{{"entry":100,"verdict":"ack"}},{{"entry":140,"verdict":"mismatch"}}],"envelopes":[[1,1],[1,2],[2,1]],"hash_versions":[1],"rules":[1,2],"state":{{"hash":"{}","hash_version":1,"seq":128}}}}"#,
            "aa".repeat(32)
        )
    );
    let bare = Beat { rules: vec![1], envelopes: vec![(1, 1)], hash_versions: vec![1], state: None, acks: vec![] };
    let bytes = crate::beat_payload(&bare).unwrap();
    assert_eq!(bytes, br#"{"envelopes":[[1,1]],"hash_versions":[1],"rules":[1]}"#);
    assert_eq!(parse_beat(&bytes, BeatContext { seq: 1, based_on_seq: 0 }).unwrap(), bare);
}

// §11.4, each constraint violated once, with the entry's seq and
// based_on_seq where the table names them.
// spec: SEAL-KIND-005
// spec: SEAL-ENC-004
#[test]
fn the_beat_table() {
    let v: Value = serde_json::from_slice(&crate::beat_payload(&beat()).unwrap()).unwrap();
    let seventeen: Vec<Value> = (1..=17).map(|i| json!(i)).collect();
    let pairs17: Vec<Value> = (1..=17).map(|i| json!([1, i])).collect();
    let acks65: Vec<Value> = (0..65).map(|i| json!({"entry": i, "verdict": "ack"})).collect();
    use JsonError::*;
    check_table(
        &v,
        vec![
            ("unknown field", set(&v, "/head", json!(1)), UnknownField),
            ("no rules", del(&v, "/rules"), MissingField),
            ("rules empty", set(&v, "/rules", json!([])), ArrayCount),
            ("17 rules", set(&v, "/rules", Value::Array(seventeen.clone())), ArrayCount),
            ("rules 0", set(&v, "/rules", json!([0, 1])), IntRange),
            ("rules 2^31", set(&v, "/rules", json!([1, 1u64 << 31])), IntRange),
            ("rules descending", set(&v, "/rules", json!([2, 1])), ArrayOrder),
            ("rules twice", set(&v, "/rules", json!([1, 1])), ArrayOrder),
            ("no envelopes", del(&v, "/envelopes"), MissingField),
            ("envelopes empty", set(&v, "/envelopes", json!([])), ArrayCount),
            ("17 envelopes", set(&v, "/envelopes", Value::Array(pairs17)), ArrayCount),
            ("a triple", set(&v, "/envelopes/0", json!([1, 1, 1])), ArrayCount),
            ("a single", set(&v, "/envelopes/0", json!([1])), ArrayCount),
            ("a pair not an array", set(&v, "/envelopes/0", json!(1)), WrongType),
            ("ver 0", set(&v, "/envelopes/0", json!([0, 1])), IntRange),
            ("suite 256", set(&v, "/envelopes/2", json!([2, 256])), IntRange),
            ("envelopes out of order", set(&v, "/envelopes", json!([[1, 2], [1, 1]])), ArrayOrder),
            ("envelopes by suite first", set(&v, "/envelopes", json!([[2, 1], [1, 2]])), ArrayOrder),
            ("an envelope twice", set(&v, "/envelopes", json!([[1, 1], [1, 1]])), ArrayOrder),
            ("no hash_versions", del(&v, "/hash_versions"), MissingField),
            ("hash_versions empty", set(&v, "/hash_versions", json!([])), ArrayCount),
            ("17 hash_versions", set(&v, "/hash_versions", Value::Array(seventeen)), ArrayCount),
            ("hash_versions descending", set(&v, "/hash_versions", json!([3, 2])), ArrayOrder),
            ("hash_versions 0", set(&v, "/hash_versions", json!([0])), IntRange),
            ("state.seq 0", set(&v, "/state/seq", json!(0)), IntRange),
            ("state.seq 63", set(&v, "/state/seq", json!(63)), IntRange),
            ("state.seq 130", set(&v, "/state/seq", json!(130)), Constraint),
            ("state.seq above based_on_seq", set(&v, "/state/seq", json!(192)), Constraint),
            ("state.hash bad", set(&v, "/state/hash", hx(&[0; 31])), BadHex),
            ("state.hash_version 0", set(&v, "/state/hash_version", json!(0)), IntRange),
            ("state without hash", del(&v, "/state/hash"), MissingField),
            ("state with an unknown field", set(&v, "/state/x", json!(1)), UnknownField),
            ("state not an object", set(&v, "/state", json!([])), WrongType),
            ("65 acks", set(&v, "/acks", Value::Array(acks65)), ArrayCount),
            ("acks out of order", set(&v, "/acks/1/entry", json!(99)), ArrayOrder),
            ("an ack twice", set(&v, "/acks/1/entry", json!(100)), ArrayOrder),
            ("an ack at the beat's seq", set(&v, "/acks/1/entry", json!(150)), Constraint),
            ("an ack after the beat's seq", set(&v, "/acks/1/entry", json!(151)), Constraint),
            ("a verdict unknown", set(&v, "/acks/0/verdict", json!("nack")), Constraint),
            ("an ack without verdict", del(&v, "/acks/0/verdict"), MissingField),
            ("an ack with an unknown field", set(&v, "/acks/0/seq", json!(1)), UnknownField),
            ("acks written at their default", set(&v, "/acks", json!([])), NotCanonical),
        ],
        beat_err,
    );
    // The bounds: state.seq equal to based_on_seq; an ack just below the seq; 64 acks.
    let cx = BeatContext { seq: 300, based_on_seq: 256 };
    let at = |v: &Value| parse_beat_json(&canon::encode(v).unwrap(), Some(cx)).err();
    assert_eq!(at(&set(&v, "/state/seq", json!(256))), None);
    assert_eq!(at(&set(&v, "/acks/1/entry", json!(299))), None);
    let acks64: Vec<Value> = (0..64).map(|i| json!({"entry": i, "verdict": "ack"})).collect();
    assert_eq!(at(&set(&v, "/acks", Value::Array(acks64))), None);
}

// `beat_payload` refuses what its parser rejects without the entry's
// context.
// spec: SEAL-KIND-005
#[test]
fn beat_payload_refuses_invalid_beats() {
    let mut cases = Vec::new();
    let mut b = beat();
    b.rules = vec![];
    cases.push(b);
    let mut b = beat();
    b.rules = vec![2, 1];
    cases.push(b);
    let mut b = beat();
    b.envelopes = vec![(0, 1)];
    cases.push(b);
    let mut b = beat();
    b.hash_versions = vec![VERSION_MAX + 1];
    cases.push(b);
    let mut b = beat();
    b.state = Some(BeatState { seq: 65, hash: [0; 32], hash_version: 1 });
    cases.push(b);
    let mut b = beat();
    b.acks = (0..65).map(|e| BeatAck { entry: e, verdict: AckVerdict::Ack }).collect();
    cases.push(b);
    let mut b = beat();
    b.acks.reverse();
    cases.push(b);
    let mut b = beat();
    b.acks[0].entry = 1 << 53;
    cases.push(b);
    for b in cases {
        assert_eq!(crate::beat_payload(&b).err(), Some(SealError::BadPayload), "{b:?}");
    }
}

// ----------------------------------------------------------- snap (§11.5)

fn snap() -> SnapAnnounce {
    SnapAnnounce {
        seq: 192,
        hash: [0xC1; 32],
        hash_version: 1,
        blob: BlobId([0xC2; 16]),
        sha256: [0xC3; 32],
        size: 2_097_152,
        parts: 2,
        inflated: 9_000_000,
        epoch: 4,
    }
}

const SNAP_CX: SnapContext = SnapContext { based_on_seq: 200, epoch: 4 };

fn snap_err(v: &Value) -> Option<JsonError> {
    parse_snap_json(&canon::encode(v).unwrap(), Some(SNAP_CX)).err()
}

// §11.5, each constraint violated once, with the entry's based_on_seq and
// the frame's epoch.
// spec: SEAL-KIND-006
// spec: SEAL-ENC-003
// spec: SEAL-ENC-004
#[test]
fn the_snap_table() {
    let bytes = crate::snap_payload(&snap()).unwrap();
    assert_eq!(parse_snap(&bytes, SNAP_CX).unwrap(), snap());
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    use JsonError::*;
    check_table(
        &v,
        vec![
            ("unknown field", set(&v, "/kind", json!(1)), UnknownField),
            ("no seq", del(&v, "/seq"), MissingField),
            ("seq 0", set(&v, "/seq", json!(0)), IntRange),
            ("seq 100", set(&v, "/seq", json!(100)), Constraint),
            ("seq above based_on_seq", set(&v, "/seq", json!(256)), Constraint),
            ("hash of 31 bytes", set(&v, "/hash", hx(&[0; 31])), BadHex),
            ("hash_version 0", set(&v, "/hash_version", json!(0)), IntRange),
            ("hash_version 2^31", set(&v, "/hash_version", json!(1u64 << 31)), IntRange),
            ("blob of 17 bytes", set(&v, "/blob", hx(&[0; 17])), BadHex),
            ("no sha256", del(&v, "/sha256"), MissingField),
            ("size 66", set(&v, "/size", json!(66)), IntRange),
            ("size 67108865", set(&v, "/size", json!(67_108_865)), IntRange),
            ("parts 1 for 2 MiB", set(&v, "/parts", json!(1)), Constraint),
            ("parts 3 for 2 MiB", set(&v, "/parts", json!(3)), Constraint),
            ("inflated 0", set(&v, "/inflated", json!(0)), IntRange),
            ("inflated 67108865", set(&v, "/inflated", json!(67_108_865)), IntRange),
            ("epoch not the frame's", set(&v, "/epoch", json!(3)), Constraint),
            ("epoch 0", set(&v, "/epoch", json!(0)), IntRange),
            ("epoch a string", set(&v, "/epoch", json!("4")), WrongType),
        ],
        snap_err,
    );
    // Boundaries of `parts`: 1,048,576 bytes is one part, one more is two.
    let one = set(&set(&v, "/size", json!(1_048_576)), "/parts", json!(1));
    assert_eq!(snap_err(&one), None);
    let two = set(&set(&v, "/size", json!(1_048_577)), "/parts", json!(2));
    assert_eq!(snap_err(&two), None);
    assert_eq!(snap_err(&set(&v, "/seq", json!(64))), None);
    // `snap_payload` refuses what its parser rejects.
    for s in [
        SnapAnnounce { seq: 65, ..snap() },
        SnapAnnounce { parts: 3, ..snap() },
        SnapAnnounce { size: 66, parts: 1, ..snap() },
        SnapAnnounce { inflated: 0, ..snap() },
        SnapAnnounce { hash_version: 0, ..snap() },
        SnapAnnounce { epoch: 0, ..snap() },
        SnapAnnounce { seq: 1 << 54, ..snap() },
    ] {
        assert_eq!(crate::snap_payload(&s).err(), Some(SealError::BadPayload), "{s:?}");
    }
}

// --------------------------------------------------------- property tests

fn mutate(mut b: Vec<u8>, at: usize, op: u8, x: u8) -> Vec<u8> {
    if b.is_empty() {
        return vec![x];
    }
    let i = at % b.len();
    match op % 3 {
        0 => b[i] = x,
        1 => b.insert(i, x),
        _ => {
            b.remove(i);
        }
    }
    b
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    // The private payload parsers return, never panic, on random and
    // mutated input, and what they accept is canonical.
    // spec: SEAL-GEN-011
    // spec: SEAL-ENC-003
    #[test]
    fn payload_parsers_never_panic(
        random in proptest::collection::vec(any::<u8>(), 0..200),
        which in 0usize..4, at in any::<usize>(), op in any::<u8>(), x in any::<u8>(), n in 1usize..4,
    ) {
        let d = DeviceKeys::generate(&mut OsEntropy).unwrap();
        let s = signed(&[skip(1), skip(2)], &d);
        let bases = [
            s.payload.clone(),
            crate::beat_payload(&beat()).unwrap(),
            crate::snap_payload(&snap()).unwrap(),
            signed(&[genesis()], &d).payload,
        ];
        let mut m = bases[which].clone();
        for k in 0..n {
            m = mutate(m, at.wrapping_add(k * 7919), op.wrapping_add(k as u8), x);
        }
        for input in [&random, &m] {
            if let Ok(u) = parse_ctl_unsigned(input) {
                let mut re = Vec::new();
                for i in &u.items { re.push(item_value(i).unwrap()); }
                prop_assert_eq!(&canon::encode(&ctl_value(Value::Array(re), &u.sig)).unwrap(), input);
                prop_assert!(!u.items_bytes.is_empty());
            }
            let _ = parse_ctl(input, &cx(&s));
            if let Ok(b) = parse_beat_json(input, None) {
                prop_assert_eq!(&crate::beat_payload(&b).unwrap(), input);
            }
            let _ = parse_beat(input, BEAT_CX);
            if let Ok(sn) = parse_snap_json(input, None) {
                prop_assert_eq!(&crate::snap_payload(&sn).unwrap(), input);
            }
            let _ = parse_snap(input, SNAP_CX);
        }
    }
}
