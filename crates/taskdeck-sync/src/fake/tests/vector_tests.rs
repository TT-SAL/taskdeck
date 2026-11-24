//! `tests/vectors/engine-fake.json` (engine.md §35): built here with the
//! crate-private parts of the fake, and compared with the published file, so
//! that the fake produces exactly its bytes (ENG-FAKE-001). The integration
//! test `tests/vectors_toy_fake.rs` replays the same file through the public
//! API, and an independent script checked the values its `notes` name.

use super::*;

use serde_json::{Value, json};

const FILE: &str = include_str!("../../../tests/vectors/engine-fake.json");

/// The three devices of the scenario and the replacement recovery key.
pub(super) const DESK: u64 = 0x0000_0000_D35C_0001;
pub(super) const LAPTOP: u64 = 0x0000_0000_1A97_0002;
pub(super) const PHONE: u64 = 0x0000_0000_9403_0003;
pub(super) const NEW_RECOVERY: u64 = 0x0000_0000_5EE7_0004;
pub(super) const ELSEWHERE: u64 = 0x0000_0000_E15E_0005;

fn hx(b: &[u8]) -> String {
    hex(b)
}

fn u64x(v: u64) -> String {
    hex(&v.to_be_bytes())
}

fn kind_name(k: Kind) -> &'static str {
    match k {
        Kind::Cmd => "cmd",
        Kind::Ctl => "ctl",
        Kind::Beat => "beat",
        Kind::Snap => "snap",
    }
}

/// A receipt as the file states it.
fn receipt_json(r: &Result<Receipt<FakeMembership>, SealerError>) -> Value {
    let r = match r {
        Err(e) => return json!({ "error": e.code }),
        Ok(r) => r,
    };
    let need: Vec<Value> = match &r.class {
        Class::NeedFrames(v) => v.iter().map(|(t, h)| json!([t, hx(h)])).collect(),
        _ => Vec::new(),
    };
    let halt = match r.halt {
        None => Value::Null,
        Some(Halt::Commitment) => json!("commitment"),
        Some(Halt::UnknownRules(v)) => json!({ "unknown_rules": v }),
    };
    json!({
        "class": class_name(&r.class),
        "code": r.code,
        "author": hx(&r.author),
        "ctr": r.ctr,
        "chain": hx(&r.chain),
        "frame_hash": hx(&r.frame_hash),
        "own_dev_foreign": r.own_dev_foreign,
        "based_on_unchecked": r.based_on_unchecked,
        "kind": r.opened.as_ref().map(|o| kind_name(o.kind)),
        "need": need,
        "halt": halt,
        "keys_changed": r.keys_changed,
        "membership": hx(&r.membership.to_bytes()),
    })
}

/// How a case is classified: after the first `k` frames of the log, by a
/// fresh `FakeSealer::create(DESK, "Desk")` that classified them in order.
#[derive(Clone, Copy)]
struct At {
    k: usize,
    retained_from: u64,
    targets: bool,
    forget_below: Option<u32>,
}

fn at(k: usize) -> At {
    At { k, retained_from: 1, targets: true, forget_below: None }
}

/// The replay the integration test repeats through the public API.
fn replay(log: &[Vec<u8>], frame: &[u8], a: At) -> Result<Receipt<FakeMembership>, SealerError> {
    let (mut fake, _, _) = FakeSealer::create(DESK, "Desk");
    let own: BTreeMap<u64, Hash32> = log
        .iter()
        .filter_map(|f| fake.frame_author(f).ok().filter(|(d, _)| *d == fake.own_dev()).map(|(_, c)| (c, h(&[f]))))
        .collect();
    let mut m = FakeMembership::default();
    let mut chains = vec![fake.chain_zero()];
    let classify = |fake: &mut FakeSealer, m: &FakeMembership, chains: &[Hash32], s: u64, f: &[u8], a: At| {
        let chain_at = |b: u64| if b >= a.retained_from && b < s { chains.get(b as usize).copied() } else { None };
        let own_sealed = |c: u64| own.get(&c).copied();
        let skip_targets = |t: u64| if a.targets && t >= 1 { log.get(t as usize - 1).map(Vec::as_slice) } else { None };
        fake.classify(&ClassifyInput {
            seq: s,
            frame: f,
            prev_chain: chains[(s - 1) as usize],
            membership: m,
            chain_at: &chain_at,
            retained_from: a.retained_from,
            own_sealed: &own_sealed,
            skip_targets: &skip_targets,
        })
    };
    for (i, f) in log[..a.k].iter().enumerate() {
        let r = classify(&mut fake, &m, &chains, i as u64 + 1, f, at(a.k))?;
        assert!(!matches!(r.class, Class::NeedFrames(_)));
        m = r.membership;
        chains.push(r.chain);
    }
    if let Some(e) = a.forget_below {
        fake.forget_roots_below(e);
    }
    classify(&mut fake, &m, &chains, a.k as u64 + 1, frame, a)
}

/// Delivers the log's frame at `s` to every node.
fn deliver_all(net: &Net, nodes: &mut [&mut Node], s: u64) {
    for n in nodes.iter_mut() {
        let r = n.deliver(net, s);
        assert_eq!(outcome(&r).0, "accept", "seq {s}");
    }
}

fn set(key: u8, value: i64) -> Body {
    Body::Cmd(ToyMachine::new(false).encode(&crate::toy::ToyCommand::Set { key, value }))
}

/// The scenario's log and the frames of the cases.
struct Scenario {
    log: Vec<Vec<u8>>,
    cases: Vec<(String, u8, Vec<u8>, At)>,
    roots: BTreeMap<u32, [u8; 32]>,
}

fn scenario() -> Scenario {
    let (fake_a, f1, _board) = FakeSealer::create(DESK, "Desk");
    let mut net = Net::new(&fake_a.board_id);
    let mut a = Node::new(fake_a);
    let s = net.push(f1);
    deliver_all(&net, &mut [&mut a], s);
    // seq 2: Desk adds Laptop from a snapshot at seq 1.
    let m1 = a.m.clone();
    let material1 = material_at(&a, &net);
    let (dev_b, spk_b, hpk_b) = FakeSealer::join_keys(LAPTOP);
    let add = |dev, sign_pk, hpke_pk, name: &str, bootstrap_seq| {
        Body::Ctl(CtlIntent::AddDevice { dev, sign_pk, hpke_pk, name: name.into(), transcript: [0x7A; 32], bootstrap_seq })
    };
    let f = a.seal(&net, 1_000, add(dev_b, spk_b, hpk_b, "Laptop", 1)).frame;
    let s2 = net.push(f);
    deliver_all(&net, &mut [&mut a], s2);
    let mut b = Node::new(FakeSealer::join(LAPTOP, "Laptop", a.fake.bundle(material1, s2)));
    b.m = m1;
    deliver_all(&net, &mut [&mut b], s2);
    // seq 3: Desk sets key 1; seq 4: Laptop sets key 2.
    let f = a.seal(&net, 2_000, set(1, 10)).frame;
    let s = net.push(f);
    deliver_all(&net, &mut [&mut a, &mut b], s);
    let f = b.seal(&net, 3_000, set(2, 20)).frame;
    let s = net.push(f);
    deliver_all(&net, &mut [&mut a, &mut b], s);
    // seq 5: Desk adds Phone from a snapshot at seq 4; seq 6: Phone sets key 3.
    let m4 = a.m.clone();
    let material4 = material_at(&a, &net);
    let (dev_c, spk_c, hpk_c) = FakeSealer::join_keys(PHONE);
    let f = a.seal(&net, 4_000, add(dev_c, spk_c, hpk_c, "Phone", 4)).frame;
    let s5 = net.push(f);
    deliver_all(&net, &mut [&mut a, &mut b], s5);
    let mut c = Node::new(FakeSealer::join(PHONE, "Phone", a.fake.bundle(material4, s5)));
    c.m = m4;
    deliver_all(&net, &mut [&mut c], s5);
    let f = c.seal(&net, 5_000, set(3, 30)).frame;
    let s = net.push(f);
    deliver_all(&net, &mut [&mut a, &mut b, &mut c], s);
    // seq 7: Desk's beat.
    let f = a.seal(&net, 6_000, Body::Beat(beat_info(vec![1]))).frame;
    let s = net.push(f);
    deliver_all(&net, &mut [&mut a, &mut b, &mut c], s);
    // Phone's frame after its revocation, sealed now (it still holds epoch 1).
    let phone_late = c.seal(&net, 6_500, set(3, 31)).frame;
    // seq 8: Desk revokes Phone (epoch 2); Laptop composed a cmd before it (seq 9, under epoch 1).
    let f8 = a.seal(&net, 7_000, Body::Ctl(CtlIntent::Rotate { revoke: vec![dev_c], reinstate: vec![], purge_before: None })).frame;
    let f9 = b.seal(&net, 8_000, set(2, 21)).frame;
    let s = net.push(f8);
    deliver_all(&net, &mut [&mut a, &mut b], s);
    let s = net.push(f9);
    deliver_all(&net, &mut [&mut a, &mut b], s);
    // seq 10: the recovery identity moves the board (a Fence).
    let rec = a.m.current_recovery().expect("recovery").clone();
    let entry = PlainEntry {
        entry_id: ok(a.fake.new_entry_id()),
        at_ms: 9_000,
        based_on: net.head(),
        body: Body::Ctl(CtlIntent::Fence { host: "relay.example".into(), door: [1, 2, 3, 4, 5], home_gen: 2 }),
    };
    let f = ok(a.fake.seal_as_recovery(&a.m, rec.last_ctr + 1, &entry)).frame;
    let s = net.push(f);
    deliver_all(&net, &mut [&mut a, &mut b], s);
    // seq 11: 31 days later; seq 12: Desk rotates (epoch 3); seq 13: Laptop sets key 2.
    let f = a.seal(&net, 9_000 + 31 * DAY, set(1, 11)).frame;
    let s = net.push(f);
    deliver_all(&net, &mut [&mut a, &mut b], s);
    let m11_b = b.m.clone();
    let b_under_2 = b.seal(&net, 9_050 + 31 * DAY, set(2, 22));
    b.ctr -= 1;
    b.own.remove(&3);
    b.fake.sealed.remove(&3);
    let f = a.seal(&net, 9_100 + 31 * DAY, Body::Ctl(CtlIntent::Rotate { revoke: vec![], reinstate: vec![], purge_before: None }))
        .frame;
    let s = net.push(f);
    deliver_all(&net, &mut [&mut a, &mut b], s);
    let f13 = b.seal(&net, 9_200 + 31 * DAY, set(2, 23)).frame;
    let s13 = net.push(f13.clone());
    deliver_all(&net, &mut [&mut a, &mut b], s13);
    // seq 14: the recovery identity changes the recovery key (a K-shape entry, epoch 4).
    let rec = a.m.current_recovery().expect("recovery").clone();
    let (_, new_spk, new_hpk) = FakeSealer::join_keys(NEW_RECOVERY);
    let entry = PlainEntry { entry_id: ok(a.fake.new_entry_id()), at_ms: 9_300 + 31 * DAY, based_on: net.head(), body: Body::Cmd(vec![]) };
    let f = ok(a.fake.seal_recovery_key_change(&a.m, rec.last_ctr + 1, &entry, (new_spk, new_hpk))).frame;
    let s = net.push(f);
    deliver_all(&net, &mut [&mut a, &mut b], s);
    // seq 15: Desk skips Laptop's frame at seq 13.
    let f15 = a.seal(&net, 9_400 + 31 * DAY, Body::Ctl(CtlIntent::Skip { targets: vec![(s13, h(&[&f13]))] })).frame;
    let s = net.push(f15.clone());
    deliver_all(&net, &mut [&mut a, &mut b], s);
    let log = net.frames.clone();
    assert_eq!(log.len(), 15);

    // ---------- the cases: one failing frame per step of `seal.md` §14.2, and accepted ones.
    let roots = a.fake.roots.clone();
    let chains = net.chains.clone();
    let head = |k: usize| Head { seq: k as u64, chain: chains[k] };
    let (dev_a, spk_a) = (a.dev(), a.fake.sign_pk);
    let cmd_plain = |k: usize| plain(1, 3_500, &head(k), &ToyMachine::new(false).encode(&crate::toy::ToyCommand::Set { key: 2, value: 24 }));
    let raw = |ver, epoch: u32, root: [u8; 32], plain: Vec<u8>| Raw { ver, suite: 1, epoch, root, plain };
    let by_b = |ctr, r: &Raw| raw_frame(&b, &dev_b, &spk_b, ctr, r);
    let r1 = roots[&1];
    let mut cases: Vec<(String, u8, Vec<u8>, At)> = Vec::new();
    let mut add_case = |name: &str, step: u8, frame: Vec<u8>, a: At| cases.push((name.to_owned(), step, frame, a));

    let f4 = log[3].clone();
    add_case("step 1: a frame shorter than 670 bytes", 1, f4[..669].to_vec(), at(4));
    let mut x = f4.clone();
    x[0] = b'X';
    add_case("step 1: magic other than TDF1", 1, x, at(4));
    let mut x = f4.clone();
    x[47] ^= 1;
    add_case("step 1: len not the total minus 112", 1, x, at(4));
    let (_, other_genesis, _) = FakeSealer::create(ELSEWHERE, "Desk");
    add_case("step 2: a frame of another board", 2, other_genesis, at(4));
    add_case("step 3: an author the membership does not know (Phone before its DeviceAdded)", 3, log[5].clone(), at(4));
    let mut x = f4.clone();
    let last = x.len() - 1;
    x[last] ^= 1;
    add_case("step 4: a signature that does not verify", 4, x, at(3));
    add_case("step 5: a replayed counter", 5, log[2].clone(), at(3));
    add_case("step 5: a skipped counter", 5, log[6].clone(), at(3));
    let own_plain = plain(1, 2_500, &head(3), &ToyMachine::new(false).encode(&crate::toy::ToyCommand::Del { key: 1 }));
    let foreign = raw_frame(&a, &dev_a, &spk_a, 3, &raw(1, 1, r1, own_plain));
    add_case("step 6: a frame under this device's dev that it did not seal (flag, then accepted)", 6, foreign, at(3));
    add_case("step 7: a revoked author (Phone after its revocation)", 7, phone_late, at(9));
    add_case("step 8: an envelope version the rules do not allow", 8, by_b(2, &raw(2, 1, r1, cmd_plain(4))), at(4));
    add_case("step 9: an epoch above the current one", 9, by_b(2, &raw(1, 2, r1, cmd_plain(4))), at(4));
    add_case("step 9: an epoch older than the previous one", 9, by_b(3, &raw(1, 1, r1, cmd_plain(7))), at(12));
    add_case(
        "step 9: the previous epoch more than 30 days of log clock after the rotation",
        9,
        by_b(3, &raw(1, 1, r1, cmd_plain(7))),
        at(11),
    );
    add_case(
        "step 9: the same, with a ciphertext that would not open (no decryption is tried)",
        9,
        by_b(3, &raw(1, 1, [0xEE; 32], cmd_plain(7))),
        at(11),
    );
    add_case("step 10: a ciphertext that does not open", 10, by_b(2, &raw(1, 1, [0xEE; 32], cmd_plain(4))), at(4));
    let mut bad_pad = cmd_plain(4);
    let end = bad_pad.len() - 1;
    bad_pad[end] = 1;
    add_case("step 11: padding that is not zero", 11, by_b(2, &raw(1, 1, r1, bad_pad)), at(4));
    let future = plain(1, 3_500, &Head { seq: 5, chain: [0; 32] }, &[1]);
    add_case("step 12: based_on at or after the entry's own seq", 12, by_b(2, &raw(1, 1, r1, future)), at(4));
    let fork = plain(1, 3_500, &Head { seq: 2, chain: chains[3] }, &[1]);
    add_case("step 12: based_on a chain value the receiver does not hold", 12, by_b(2, &raw(1, 1, r1, fork)), at(4));
    let unchecked = plain(1, 3_500, &head(2), &[1]);
    add_case(
        "step 12: based_on below retained_from (flag, then accepted)",
        12,
        by_b(2, &raw(1, 1, r1, unchecked)),
        At { retained_from: 3, ..at(4) },
    );
    let rec1 = m_at(&log, 4).current_recovery().expect("recovery").clone();
    let rec_cmd = raw_frame(&a, &rec1.dev_r, &rec1.sign_pk, 2, &raw(1, 1, r1, cmd_plain(4)));
    add_case("step 13: a cmd by the recovery identity", 13, rec_cmd, at(4));
    let fence_items = encode_items(&[Item::Fence { host: "b.example".into(), door: [0; 5], home_gen: 2 }]).expect("items");
    let ctl_payload = |entry_id: &[u8; 16], at_ms: i64, ctr: u64, items: &[u8], sig: Option<[u8; 64]>| {
        let mut p = items.to_vec();
        let inner = ctl_inner(&a.fake.board_id, &dev_b, ctr, entry_id, at_ms, items);
        p.extend_from_slice(&sig.unwrap_or_else(|| sign(&spk_b, &inner)));
        p
    };
    let stale_ctl = build_plain(&[0x42; 16], 2, 7_500, &head(7), &ctl_payload(&[0x42; 16], 7_500, 2, &fence_items, None)).expect("fits");
    add_case("step 13: a ctl under the previous epoch", 13, by_b(2, &raw(1, 1, r1, stale_ctl)), at(8));
    let late = plain(1, 7_500, &head(8), &[1]);
    add_case("step 14: a cmd under the previous epoch composed after the rotation", 14, by_b(2, &raw(1, 1, r1, late)), at(8));
    let beat = encode_beat(&beat_info(vec![2, 1])).expect("beat");
    add_case("step 15: a beat payload out of order", 15, by_b(2, &raw(1, 1, r1, plain(3, 3_500, &head(4), &beat))), at(4));
    let bad_sig = build_plain(&[0x42; 16], 2, 3_500, &head(4), &ctl_payload(&[0x42; 16], 3_500, 2, &fence_items, Some([0; 64])))
        .expect("fits");
    add_case("step 15: an inner control signature that does not verify", 15, by_b(2, &raw(1, 1, r1, bad_sig)), at(4));
    let two = encode_items(&[
        Item::Fence { host: "b.example".into(), door: [0; 5], home_gen: 2 },
        Item::Fence { host: "c.example".into(), door: [0; 5], home_gen: 3 },
    ])
    .expect("items");
    let no_shape = build_plain(&[0x42; 16], 2, 3_500, &head(4), &ctl_payload(&[0x42; 16], 3_500, 2, &two, None)).expect("fits");
    add_case("step 16: items that form no shape", 16, by_b(2, &raw(1, 1, r1, no_shape)), at(4));
    let gen3 = encode_items(&[Item::Fence { host: "b.example".into(), door: [0; 5], home_gen: 3 }]).expect("items");
    let wrong_gen = build_plain(&[0x42; 16], 2, 3_500, &head(4), &ctl_payload(&[0x42; 16], 3_500, 2, &gen3, None)).expect("fits");
    add_case("step 16: an item whose precondition fails (home_gen)", 16, by_b(2, &raw(1, 1, r1, wrong_gen)), at(4));
    add_case("step 16: a Skip whose target frame is not supplied (NeedFrames)", 16, f15, At { targets: false, ..at(14) });
    add_case("step 17: a valid cmd", 17, f4.clone(), at(3));
    // A rotation by Laptop whose wrap to Desk does not open: accepted, with Desk's commitment halt.
    let m4 = m_at(&log, 4);
    let recipients = recipients_of(&m4);
    let root2 = [0x52; 32];
    let Item::Rotated { epoch, commit, mut wraps, purge_before } = rotation_item(&m4, 2, &root2, &recipients) else {
        panic!("a rotation")
    };
    let pos = wraps.iter().position(|w| first16_of(w) == dev_a).expect("a wrap to Desk");
    wraps[pos] = wrap(&dev_a, &hpk_b, 2, &root2);
    let items = encode_items(&[Item::Rotated { epoch, commit, wraps, purge_before }]).expect("items");
    let halting = build_plain(&[0x43; 16], 2, 3_500, &head(4), &ctl_payload(&[0x43; 16], 3_500, 2, &items, None)).expect("fits");
    add_case("step 17: a rotation whose wrap to this device does not open (the commitment halt)", 17, by_b(2, &raw(1, 1, r1, halting)), at(4));
    let under2 = b_under_2.frame.clone();
    assert_eq!(m11_b.epoch, 2);
    add_case("step 14: a cmd under the previous epoch composed before the rotation, within 30 days (accepted)", 14, under2.clone(), at(12));
    add_case(
        "local fault: the root of an accepted epoch is not held (seal/missing_root)",
        9,
        under2,
        At { forget_below: Some(3), ..at(12) },
    );
    Scenario { log, cases, roots }
}

/// `M_k` as the replay derives it.
fn m_at(log: &[Vec<u8>], k: usize) -> FakeMembership {
    let (mut fake, _, _) = FakeSealer::create(DESK, "Desk");
    let mut m = FakeMembership::default();
    let mut chains = vec![fake.chain_zero()];
    for (i, f) in log[..k].iter().enumerate() {
        let s = i as u64 + 1;
        let chain_at = |b: u64| if b < s { chains.get(b as usize).copied() } else { None };
        let none = |_: u64| None;
        let targets = |t: u64| if t >= 1 { log.get(t as usize - 1).map(Vec::as_slice) } else { None };
        let r = ok(fake.classify(&ClassifyInput {
            seq: s,
            frame: f,
            prev_chain: chains[i],
            membership: &m,
            chain_at: &chain_at,
            retained_from: 1,
            own_sealed: &none,
            skip_targets: &targets,
        }));
        m = r.membership;
        chains.push(r.chain);
    }
    m
}

fn item_json(item: &Item) -> Value {
    match item {
        Item::Genesis(_) => json!({ "type": "Genesis" }),
        Item::DeviceAdded { dev, sign_pk, hpke_pk, name, transcript, bootstrap_seq } => json!({
            "type": "DeviceAdded", "dev": hx(dev), "sign_pk": hx(sign_pk), "hpke_pk": hx(hpke_pk),
            "name": name, "transcript": hx(transcript), "bootstrap_seq": bootstrap_seq,
        }),
        Item::Revoked(d) => json!({ "type": "DeviceRevoked", "dev": hx(d) }),
        Item::Reinstated(d) => json!({ "type": "DeviceReinstated", "dev": hx(d) }),
        Item::Rotated { epoch, commit, wraps, purge_before } => json!({
            "type": "EpochRotated", "epoch": epoch, "commit": hx(commit),
            "wraps": wraps.iter().map(|w| hx(w)).collect::<Vec<_>>(), "purge_before": purge_before,
        }),
        Item::Recovery { sign_pk, hpke_pk } => json!({ "type": "RecoveryKeyChange", "sign_pk": hx(sign_pk), "hpke_pk": hx(hpke_pk) }),
        Item::Fence { host, door, home_gen } => json!({ "type": "Fence", "host": host, "door": hx(door), "home_gen": home_gen }),
        Item::ResetTo { seq, state_hash, hash_version } => {
            json!({ "type": "ResetTo", "seq": seq, "state_hash": hx(state_hash), "hash_version": hash_version })
        }
        Item::Skip { seq, frame_hash } => json!({ "type": "Skip", "seq": seq, "frame_hash": hx(frame_hash) }),
        Item::Upgrade { rules, mode } => json!({ "type": "RulesUpgrade", "rules": rules, "mode": if *mode == 1 { "normal" } else { "override" } }),
    }
}

/// Builds the whole file.
fn build() -> Value {
    let mut cases = Vec::new();

    // ---------- keys (engine.md §6.3 table) and the genesis helper.
    for (name, seed) in [("Laptop", LAPTOP), ("Phone", PHONE)] {
        let mut g = SplitMix64 { state: seed };
        let sk = draw::<32>(&mut g);
        let dev = draw::<16>(&mut g);
        let (sign_pk, hpke_pk) = public_keys(&sk);
        assert_eq!(FakeSealer::join_keys(seed), (dev, sign_pk, hpke_pk));
        cases.push(json!({
            "name": format!("keys: the device of seed {} ({name})", u64x(seed)),
            "kind": "keys",
            "seed": u64x(seed),
            "expect": { "sk": hx(&sk), "dev": hx(&dev), "sign_pk": hx(&sign_pk), "hpke_pk": hx(&hpke_pk), "key_fp": key_fp(&sign_pk, &hpke_pk) },
        }));
    }
    let (fake, frame, board) = FakeSealer::create(DESK, "Desk");
    let mut g = SplitMix64 { state: DESK };
    let salt = draw::<16>(&mut g);
    let rsk = draw::<32>(&mut g);
    let sk = draw::<32>(&mut g);
    let dev = draw::<16>(&mut g);
    let root = draw::<32>(&mut g);
    let entry_id = draw::<16>(&mut g);
    let (rsign_pk, rhpke_pk) = public_keys(&rsk);
    let (sign_pk, hpke_pk) = public_keys(&sk);
    let board_id = board_id_of(&rsign_pk, &salt);
    let dev_r = dev_r_of(&board_id, &rsign_pk);
    let mut net = Net::new(&board_id);
    let mut n = Node::new(fake);
    let s = net.push(frame.clone());
    let r = n.deliver(&net, s);
    let p = parse_frame(&frame).expect("frame");
    let plain = aead_open(&entry_key(&board_id, 1, &root), &entry_ad(p.header, 1, 1, 1), &p.body[BODY_PREFIX..]).expect("opens");
    let payload = parse_plain(plain).expect("plain").payload.to_vec();
    let c = parse_ctl(&payload).expect("ctl");
    let Item::Genesis(gi) = &c.items[0] else { panic!("Genesis") };
    cases.push(json!({
        "name": "genesis: FakeSealer::create(seed, \"Desk\")",
        "kind": "genesis",
        "seed": u64x(DESK),
        "device_name": "Desk",
        "expect": {
            "salt": hx(&salt), "rsk": hx(&rsk), "rsign_pk": hx(&rsign_pk), "rhpke_pk": hx(&rhpke_pk),
            "sk": hx(&sk), "dev": hx(&dev), "sign_pk": hx(&sign_pk), "hpke_pk": hx(&hpke_pk),
            "root_1": hx(&root), "entry_id": hx(&entry_id), "board_id": hx(&board_id), "dev_r": hx(&dev_r),
            "commit_1": hx(&commitment(&board_id, 1, &root)), "wraps": gi.wraps.iter().map(|w| hx(w)).collect::<Vec<_>>(),
            "nonce": hx(&p.body[6..30]), "state_hash": hx(&gi.state_hash), "board": hx(&board),
            "membership": hx(&r.membership.to_bytes()), "items": hx(c.items_bytes), "payload": hx(&payload),
            "frame": hx(&frame), "frame_hash": hx(&h(&[&frame])), "chain_0": hx(&net.chains[0]), "chain_1": hx(&net.chains[1]),
            "key_fp": key_fp(&sign_pk, &hpke_pk), "recovery_fp": key_fp(&rsign_pk, &rhpke_pk),
            "export_identity_len": n.fake.export_identity().len(),
        },
    }));

    // ---------- primitives (engine.md §6.3 table) on fixed inputs.
    let k = [0x4B; 32];
    let pk = [0x50; 32];
    cases.push(json!({
        "name": "primitives: signature, AEAD, wrap, commitment, keys, chain, move code and fingerprint",
        "kind": "primitives",
        "inputs": { "sign_pk": hx(&pk), "message": hx(b"taskdeck"), "key": hx(&k), "ad": hx(b"ad"), "plain": hx(b"plain"),
                    "board_id": hx(&[0x0B; 16]), "epoch": 2, "root": hx(&[0x5A; 32]), "recipient": hx(&[0x10; 16]),
                    "seq": 4096, "chain": hx(&[0xC4; 32]), "fence_hash": hx(&[0xF0; 32]) },
        "expect": {
            "signature": hx(&sign(&pk, b"taskdeck")),
            "aead": hx(&aead_seal(&k, b"ad", b"plain")),
            "wrap": hx(&wrap(&[0x10; 16], &pk, 2, &[0x5A; 32])),
            "commitment": hx(&commitment(&[0x0B; 16], 2, &[0x5A; 32])),
            "entry_key": hx(&entry_key(&[0x0B; 16], 2, &[0x5A; 32])),
            "snap_key": hx(&snap_key(&[0x0B; 16], 2, &[0x5A; 32])),
            "chain_zero": hx(&chain_zero_of(&[0x0B; 16])),
            "sync_fingerprint": fp_text(&[0x0B; 16], 4096, &[0xC4; 32]),
            "move_code": mc_text(&[0x0B; 16], 4096, &[0xF0; 32]),
            "key_fp": key_fp(&pk, &k),
        },
    }));

    // ---------- payload encodings (engine.md §6.3 "Fake payload encodings").
    let items = [
        Item::DeviceAdded { dev: [0x0D; 16], sign_pk: [0x0E; 32], hpke_pk: [0x0F; 32], name: "Laptop".into(), transcript: [0x10; 32], bootstrap_seq: 9 },
        Item::Revoked([0x66; 16]),
        Item::Reinstated([0x66; 16]),
        Item::Rotated { epoch: 2, commit: [0x77; 32], wraps: vec![[0x01; 96], [0x02; 96]], purge_before: 0 },
        Item::Rotated { epoch: 3, commit: [0x78; 32], wraps: vec![[0x01; 96], [0x02; 96], [0x03; 96]], purge_before: 40 },
        Item::Recovery { sign_pk: [0x01; 32], hpke_pk: [0x02; 32] },
        Item::Fence { host: "relay.example:8443".into(), door: [1, 2, 3, 4, 5], home_gen: 2 },
        Item::ResetTo { seq: 5, state_hash: [0x44; 32], hash_version: 1 },
        Item::Skip { seq: 3, frame_hash: [0x55; 32] },
        Item::Upgrade { rules: 2, mode: 1 },
        Item::Upgrade { rules: 2, mode: 2 },
    ];
    for item in &items {
        let bytes = encode_items(std::slice::from_ref(item)).expect("encodes");
        cases.push(json!({
            "name": format!("item: {}", item_json(item)["type"].as_str().unwrap_or("?")),
            "kind": "item",
            "item": item_json(item),
            "expect": { "items_bytes": hx(&bytes) },
        }));
    }
    cases.push(json!({
        "name": "item: Genesis (the genesis case's items, field by field)",
        "kind": "item",
        "item": {
            "type": "Genesis", "salt": hx(&gi.salt), "rsign_pk": hx(&gi.rsign_pk), "rhpke_pk": hx(&gi.rhpke_pk), "dev": hx(&gi.dev),
            "sign_pk": hx(&gi.sign_pk), "hpke_pk": hx(&gi.hpke_pk), "name": gi.name, "commit": hx(&gi.commit),
            "wraps": gi.wraps.iter().map(|w| hx(w)).collect::<Vec<_>>(), "rules": gi.rules, "hash_version": gi.hash_version,
            "state_hash": hx(&gi.state_hash),
        },
        "expect": { "items_bytes": hx(c.items_bytes) },
    }));
    let beat = BeatInfo {
        rules: vec![1, 2],
        envelopes: vec![(1, 1)],
        hash_versions: vec![1],
        state: Some((64, [0xAB; 32], 1)),
        acks: vec![(70, Verdict::Ack), (71, Verdict::Mismatch)],
    };
    cases.push(json!({
        "name": "beat payload",
        "kind": "beat",
        "beat": { "rules": [1, 2], "envelopes": [[1, 1]], "hash_versions": [1], "state": { "seq": 64, "hash": hx(&[0xAB; 32]), "hash_version": 1 },
                  "acks": [{ "entry": 70, "verdict": "ack" }, { "entry": 71, "verdict": "mismatch" }] },
        "expect": { "payload": hx(&encode_beat(&beat).expect("beat")) },
    }));
    let snap = SnapInfo { seq: 64, hash: [0x11; 32], hash_version: 1, blob: [0x22; 16], sha256: [0x33; 32], size: 1_048_577, parts: 2, inflated: 5_000, epoch: 3 };
    cases.push(json!({
        "name": "snap payload",
        "kind": "snap",
        "snap": { "seq": 64, "hash": hx(&[0x11; 32]), "hash_version": 1, "blob": hx(&[0x22; 16]), "sha256": hx(&[0x33; 32]),
                  "size": 1_048_577, "parts": 2, "inflated": 5_000, "epoch": 3 },
        "expect": { "payload": hx(&encode_snap(&snap)) },
    }));

    // ---------- the scenario: its log, its receipts, and one failing case per step of §14.2.
    let sc = scenario();
    let receipts: Vec<Value> = (0..sc.log.len()).map(|k| receipt_json(&replay(&sc.log, &sc.log[k], at(k)))).collect();
    cases.push(json!({
        "name": "log: Desk creates the board, adds Laptop and Phone, revokes Phone, moves, rotates, changes the recovery key, skips",
        "kind": "log",
        "seed": u64x(DESK),
        "device_name": "Desk",
        "devices": { "Desk": u64x(DESK), "Laptop": u64x(LAPTOP), "Phone": u64x(PHONE), "new recovery key": u64x(NEW_RECOVERY) },
        "frames": sc.log.iter().map(|f| hx(f)).collect::<Vec<_>>(),
        "expect": { "receipts": receipts, "roots": sc.roots.iter().map(|(e, r)| json!([e, hx(r)])).collect::<Vec<_>>() },
    }));
    for (name, step, frame, a) in &sc.cases {
        cases.push(json!({
            "name": name,
            "kind": "receipt",
            "step": step,
            "after": a.k,
            "retained_from": a.retained_from,
            "targets_supplied": a.targets,
            "forget_roots_below": a.forget_below,
            "frame": hx(frame),
            "expect": receipt_json(&replay(&sc.log, frame, *a)),
        }));
    }

    // ---------- a snapshot of the log's state at seq 15, by a fresh replay.
    let snap_case = snapshot_case(&sc.log);
    cases.push(snap_case);

    // ---------- the identity layout, with a pending root and without.
    cases.push(identity_case());

    json!({
        "spec": "engine.md §6.3",
        "notes": NOTES,
        "cases": cases,
    })
}

const NOTES: &str = "Byte strings are lowercase hex; u64 seeds are 16 hex digits, big-endian. Every random byte comes from \
SplitMix64 (engine.md §31.2) seeded with the case's seed. `receipt` cases classify `frame` at seq `after + 1` with a fresh \
FakeSealer::create(seed of the log case, \"Desk\") that first classified the log's first `after` frames in order from M_0 \
(retained_from 1, every Skip target supplied, own_sealed the log's frames under Desk's dev by counter); the case then uses its \
`retained_from` for `chain_at` (C_b for retained_from <= b < seq), supplies the log's frames as Skip targets only when \
`targets_supplied`, and calls forget_roots_below(n) first when `forget_roots_below` is n. `expect.error` is a SealerError. \
Values checked by an independent script written from the spec text (not the crate's code): every value of `keys`, \
`genesis` (frame bytes included), `primitives`, `item`, `beat` and `snap`; for the log, every frame's layout, signature \
(fake) and AEAD tag under the epoch roots its rotations wrap, its frame hash and the chain values. Recorded from the crate's \
own output: the bytes of the later log frames and of the case frames (built by the crate; their `step` and expected class and \
code were chosen by hand from seal.md §14.2), the membership bytes after each receipt, and the snapshot blob. The `identity` \
case is recorded from the crate's own output too: export_identity (the layout of docs/sync/status/engine/api-fake-toy.md) of \
a fresh FakeSealer::create(seed, device_name) after it classified its genesis frame at seq 1 from membership_zero(), after it \
sealed `Rotate` (no revoke, no reinstate, no purge) at counter 1 with at_ms `at_ms`, based_on the head at seq 1 and the entry id \
of new_entry_id() (a pending root), and after it classified that frame at seq 2.";

fn fp_text(board_id: &[u8; 16], seq: u64, chain: &Hash32) -> String {
    let x = h(&[&ctx("fingerprint"), board_id, &seq.to_be_bytes(), chain]);
    format!("{} {}", seq, hex(&x[..6]))
}

fn mc_text(board_id: &[u8; 16], seq: u64, fence_hash: &Hash32) -> String {
    hex(&h(&[&ctx("move"), board_id, &seq.to_be_bytes(), fence_hash])[..5])
}

/// A snapshot at the log's head by Desk, sealed by a fresh replay (its first draw).
fn snapshot_case(log: &[Vec<u8>]) -> Value {
    let (mut fake, _, board) = FakeSealer::create(DESK, "Desk");
    let mut m = FakeMembership::default();
    let mut chains = vec![fake.chain_zero()];
    for (i, f) in log.iter().enumerate() {
        let s = i as u64 + 1;
        let chain_at = |b: u64| if b < s { chains.get(b as usize).copied() } else { None };
        let none = |_: u64| None;
        let targets = |t: u64| if t >= 1 { log.get(t as usize - 1).map(Vec::as_slice) } else { None };
        let r = ok(fake.classify(&ClassifyInput {
            seq: s,
            frame: f,
            prev_chain: chains[i],
            membership: &m,
            chain_at: &chain_at,
            retained_from: 1,
            own_sealed: &none,
            skip_targets: &targets,
        }));
        m = r.membership;
        chains.push(r.chain);
    }
    let s = log.len() as u64;
    let membership = m.to_bytes();
    let material = SnapshotMaterial {
        seq: s,
        chain: chains[log.len()],
        hash_version: 1,
        state_hash: state_hash_of(&h(&[&board]), &membership),
        board: board.clone(),
        membership: membership.clone(),
    };
    let blob = ok(fake.seal_snapshot(&m, &material));
    json!({
        "name": "snapshot: Desk's state at the log's head, sealed by a fresh replay of the log (seal_snapshot is its first draw)",
        "kind": "snapshot",
        "seed": u64x(DESK),
        "board": hx(&board),
        "expect": {
            "seq": s, "chain": hx(&material.chain), "state_hash": hx(&material.state_hash), "membership": hx(&membership),
            "blob_id": hx(&blob.id), "bytes": hx(&blob.bytes), "sha256": hx(&blob.sha256), "size": blob.bytes.len(),
            "parts": blob.parts, "inflated": blob.inflated, "epoch": blob.epoch,
        },
    })
}

/// The fake produces exactly the bytes of `engine-fake.json` (engine.md §35).
// spec: ENG-FAKE-001
// spec: ENG-FAKE-002
#[test]
fn the_fake_produces_the_bytes_of_its_vectors() {
    let built = build();
    let file: Value = serde_json::from_str(FILE).expect("engine-fake.json is JSON");
    let (built_cases, file_cases) = (built["cases"].as_array().expect("cases"), file["cases"].as_array().expect("cases"));
    assert_eq!(built_cases.len(), file_cases.len(), "number of cases");
    for (b, f) in built_cases.iter().zip(file_cases) {
        assert_eq!(b, f, "case {}", f["name"]);
    }
    assert_eq!(built, file);
}

/// One failing case per step of `seal.md` §14.2 that can fail, and a valid frame.
#[test]
fn the_vectors_cover_every_step_of_14_2() {
    let file: Value = serde_json::from_str(FILE).expect("engine-fake.json is JSON");
    let receipts: Vec<&Value> = file["cases"].as_array().expect("cases").iter().filter(|c| c["kind"] == "receipt").collect();
    for step in 1..=17u64 {
        assert!(receipts.iter().any(|c| c["step"] == step), "no case for step {step}");
    }
    assert!(receipts.iter().any(|c| c["expect"]["class"] == "accept" && c["expect"]["code"].is_null()));
}

/// The identity layout (api-fake-toy.md): Desk after its genesis, after sealing
/// a rotation (one pending root), and after accepting it (two roots). A fake
/// restored from each export writes the same bytes again (engine.md §31.4).
fn identity_case() -> Value {
    let (fake, frame, _) = FakeSealer::create(DESK, "Desk");
    let mut net = Net::new(&fake.board_id);
    let mut n = Node::new(fake);
    n.m = n.fake.membership_zero();
    let s = net.push(frame);
    assert!(matches!(n.deliver(&net, s).class, Class::Accept));
    let at_genesis = n.fake.export_identity().to_vec();
    let rotate = Body::Ctl(CtlIntent::Rotate { revoke: vec![], reinstate: vec![], purge_before: None });
    let sealed = n.seal(&net, 1_000, rotate);
    let entry_id = ok(n.fake.open_own(&n.m, &sealed.frame));
    let pending = n.fake.export_identity().to_vec();
    let s = net.push(sealed.frame.clone());
    let r = n.deliver(&net, s);
    assert!(matches!(r.class, Class::Accept) && r.keys_changed && n.fake.pending.is_empty());
    let accepted = n.fake.export_identity().to_vec();
    for id in [&at_genesis, &pending, &accepted] {
        let restored = ok(FakeSealer::restore(id, n.fake.history(), 7, &[1]));
        assert_eq!(restored.export_identity().to_vec(), *id);
    }
    json!({
        "name": "identity: export_identity of Desk after its genesis, after sealing a rotation (a pending root), after accepting it",
        "kind": "identity",
        "seed": u64x(DESK),
        "device_name": "Desk",
        "at_ms": 1_000,
        "expect": {
            "genesis": hx(&at_genesis), "entry_id": hx(&entry_id), "frame": hx(&sealed.frame),
            "sealed": hx(&pending), "accepted": hx(&accepted),
        },
    })
}
