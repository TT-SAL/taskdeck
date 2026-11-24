//! The vectors of engine.md §35 for SplitMix64 (§31.2), the toy state machine
//! (§5.3) and the fake sealer (§6.3): `tests/vectors/engine-splitmix.json`,
//! `engine-toy.json` and `engine-fake.json`, loaded and replayed through the
//! crate's public API (the test builds have the feature `sim`).
//!
//! The splitmix and toy files were written by a model of the spec text that
//! is not the crate's code; the fake file's independent values are listed in
//! its `notes` (docs/sync/status/engine/api-fake-toy.md).

use std::collections::BTreeMap;

use serde_json::Value;
use taskdeck_sync::fake::{FakeBundle, FakeMembership, FakeSealer};
use taskdeck_sync::sim::SplitMix64;
use taskdeck_sync::toy::{ToyCommand, ToyMachine, ToyNotice, ToyState};
use taskdeck_sync::{
    Applied, Class, ClassifyInput, Coalesce, Ctx, Dev, Hash32, Receipt, Sealer, SmRefusal, SnapInfo, SnapshotMaterial,
    StateMachine,
};

const SPLITMIX: &str = include_str!("vectors/engine-splitmix.json");
const TOY: &str = include_str!("vectors/engine-toy.json");
const FAKE: &str = include_str!("vectors/engine-fake.json");

// ---------- JSON helpers

fn parse(text: &str) -> Value {
    serde_json::from_str(text).expect("a vector file is JSON")
}

fn unhex(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2), "odd hex");
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex")).collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn bytes(v: &Value) -> Vec<u8> {
    unhex(v.as_str().expect("a hex string"))
}

fn arr<const N: usize>(v: &Value) -> [u8; N] {
    bytes(v).try_into().expect("a fixed-width hex string")
}

fn u64_hex(v: &Value) -> u64 {
    u64::from_be_bytes(arr::<8>(v))
}

fn int(v: &Value) -> u64 {
    v.as_u64().expect("an unsigned integer")
}

fn cases(file: &Value) -> &Vec<Value> {
    assert!(file["spec"].as_str().is_some(), "a `spec` field (README §7)");
    file["cases"].as_array().expect("a `cases` array")
}

// ---------- SplitMix64 (engine.md §31.2)

/// The outputs of `next`, `below` and `chance` for several seeds.
// spec: ENG-FAKE-002
#[test]
fn splitmix_vectors() {
    let file = parse(SPLITMIX);
    for case in cases(&file) {
        let seed = u64_hex(&case["seed"]);
        let mut g = SplitMix64 { state: seed };
        for (i, want) in case["next"].as_array().expect("next").iter().enumerate() {
            assert_eq!(g.next(), u64_hex(want), "{} next #{i}", case["name"]);
        }
        let mut g = SplitMix64 { state: seed };
        for b in case["below"].as_array().expect("below") {
            assert_eq!(g.below(u64_hex(&b["n"])), u64_hex(&b["value"]), "{} below", case["name"]);
        }
        let mut g = SplitMix64 { state: seed };
        for c in case["chance"].as_array().expect("chance") {
            let ppm = u32::try_from(int(&c["ppm"])).expect("ppm");
            assert_eq!(g.chance(ppm), c["value"].as_bool().expect("bool"), "{} chance", case["name"]);
        }
        assert_eq!(g.state, u64_hex(&case["state_after_chance"]), "{}", case["name"]);
    }
}

// ---------- the toy (engine.md §5.3)

fn command(v: &Value) -> ToyCommand {
    let key = || u8::try_from(int(&v["key"])).expect("key");
    let i64_of = |f: &str| v[f].as_i64().expect("i64");
    let name = || u32::try_from(int(&v["name"])).expect("name");
    match v["op"].as_str().expect("op") {
        "set" => ToyCommand::Set { key: key(), value: i64_of("value") },
        "add" => ToyCommand::Add { key: key(), delta: i64_of("delta") },
        "mul" => ToyCommand::Mul { key: key(), factor: i64_of("factor") },
        "del" => ToyCommand::Del { key: key() },
        "create" => ToyCommand::Create { id: int(&v["id"]), name: name() },
        "name" => ToyCommand::Name { id: int(&v["id"]), name: name() },
        "forget" => ToyCommand::Forget { id: int(&v["id"]) },
        "fail" => ToyCommand::Fail { code: u8::try_from(int(&v["code"])).expect("code") },
        "panic" => ToyCommand::Panic,
        other => panic!("unknown op {other}"),
    }
}

fn opt_u64(v: &Value) -> Option<u64> {
    v.as_u64()
}

fn load(toy: &ToyMachine, b: &[u8]) -> ToyState {
    match toy.load(b) {
        Ok(s) => s,
        Err(e) => panic!("load refused with {}", e.code),
    }
}

/// `count` items of `device_no`, numbered 1 to `count`, named 0, with its mark at `count`.
fn state_with_items(v: &Value) -> ToyState {
    let device_no = u16::try_from(int(&v["device_no"])).expect("device_no");
    let count = int(&v["count"]);
    let base = u64::from(device_no) << 40;
    ToyState {
        rules: 1,
        keys: BTreeMap::new(),
        items: (1..=count).map(|n| (base + n, 0)).collect(),
        marks: BTreeMap::from([(device_no, count)]),
    }
}

fn check_apply(toy: &ToyMachine, case: &Value) {
    let name = case["name"].as_str().expect("name");
    let state = match case.get("state") {
        Some(s) => load(toy, &bytes(s)),
        None => state_with_items(&case["state_items"]),
    };
    let c = command(&case["command"]);
    let cx = &case["ctx"];
    let ctx = Ctx {
        seq: int(&cx["seq"]),
        device_no: u16::try_from(int(&cx["device_no"])).expect("device_no"),
        at_ms: cx["at_ms"].as_i64().expect("at_ms"),
        based_on: int(&cx["based_on"]),
    };
    let expect = &case["expect"];
    if expect.get("panics").is_some() {
        let mut s = state.clone();
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| toy.apply(&mut s, &c, &ctx))).is_err();
        assert!(panicked, "{name}: Panic panics");
        return;
    }
    let mut after = state.clone();
    let got: Result<Applied<ToyNotice>, SmRefusal> = toy.apply(&mut after, &c, &ctx);
    // trial_apply never changes the state and returns the same result.
    let trial = toy.trial_apply(&state, &c, &ctx);
    assert_eq!(trial, got, "{name}: trial_apply");
    if let Some(ok) = expect.get("ok") {
        let applied = got.unwrap_or_else(|r| panic!("{name}: refused with {}", r.code));
        assert_eq!(applied.benign, ok["benign"].as_bool().expect("benign"), "{name}");
        let notices: Vec<String> = applied.notices.iter().map(|n| hex(&toy.notice_bytes(n))).collect();
        let want: Vec<String> = ok["notices"].as_array().expect("notices").iter().map(|n| n.as_str().expect("hex").to_owned()).collect();
        assert_eq!(notices, want, "{name}");
        assert_eq!(hex(&toy.canonical_bytes(&after)), ok["state"].as_str().expect("state"), "{name}");
    } else {
        let r = &expect["refused"];
        let refusal = match got {
            Ok(_) => panic!("{name}: accepted"),
            Err(refusal) => refusal,
        };
        assert_eq!(refusal.code, r["code"].as_str().expect("code"), "{name}");
        assert_eq!(refusal.op, r["op"].as_str(), "{name}");
        assert_eq!(refusal.target, opt_u64(&r["target"]), "{name}");
        assert_eq!((refusal.base, refusal.current), (None, None), "{name}");
        assert_eq!(after, state, "{name}: a refusal leaves the state unchanged");
        let rb = toy.refusal_bytes(ctx.seq, ctx.device_no, &refusal);
        assert_eq!(hex(&rb), r["refusal_bytes"].as_str().expect("refusal_bytes"), "{name}");
    }
}

fn command_json(toy: &ToyMachine, c: &ToyCommand) -> Value {
    // Back to the value form, through the bytes (`parse(encode(c)) = c`).
    let b = toy.encode(c);
    let mut v = serde_json::Map::new();
    v.insert("op".into(), Value::from(toy.op_name(c)));
    match *c {
        ToyCommand::Set { key, value } => {
            v.insert("key".into(), key.into());
            v.insert("value".into(), value.into());
        }
        ToyCommand::Add { key, delta } => {
            v.insert("key".into(), key.into());
            v.insert("delta".into(), delta.into());
        }
        ToyCommand::Mul { key, factor } => {
            v.insert("key".into(), key.into());
            v.insert("factor".into(), factor.into());
        }
        ToyCommand::Del { key } => {
            v.insert("key".into(), key.into());
        }
        ToyCommand::Create { id, name } | ToyCommand::Name { id, name } => {
            v.insert("id".into(), id.into());
            v.insert("name".into(), name.into());
        }
        ToyCommand::Forget { id } => {
            v.insert("id".into(), id.into());
        }
        ToyCommand::Fail { code } => {
            v.insert("code".into(), code.into());
        }
        ToyCommand::Panic => {}
    }
    assert_eq!(toy.parse(&b).ok().as_ref(), Some(c));
    Value::Object(v)
}

/// One case per toy operation and per refusal code: state bytes, command
/// bytes, `apply`, `upgrade`, `reset`, `coalesce`, `rebind`, notices and
/// refusal records.
// spec: ENG-TOY-001
#[test]
fn toy_vectors() {
    let file = parse(TOY);
    let mut kinds = BTreeMap::new();
    for case in cases(&file) {
        let name = case["name"].as_str().expect("name");
        let kind = case["kind"].as_str().expect("kind");
        *kinds.entry(kind.to_owned()).or_insert(0) += 1;
        let toy = ToyMachine::new(case["rules2"].as_bool().unwrap_or(false));
        let expect = &case["expect"];
        match kind {
            "load" => {
                let b = bytes(&case["bytes"]);
                match toy.load(&b) {
                    Ok(s) => {
                        assert_eq!(expect["ok"], Value::Bool(true), "{name}");
                        assert_eq!(toy.canonical_bytes(&s), b, "{name}: canonical bytes round-trip");
                        let want: [u8; 32] = sha2_of(&b);
                        assert_eq!(toy.board_hash(&s), want, "{name}: board_hash");
                    }
                    Err(e) => assert_eq!(Some(e.code), expect["error"].as_str(), "{name}"),
                }
            }
            "command" => {
                let c = command(&case["command"]);
                assert_eq!(hex(&toy.encode(&c)), expect["bytes"].as_str().expect("bytes"), "{name}");
                assert_eq!(toy.op_name(&c), expect["op_name"].as_str().expect("op_name"), "{name}");
                assert_eq!(toy.target(&c), opt_u64(&expect["target"]), "{name}");
                assert_eq!(toy.hold(&c), opt_u64(&expect["hold"]), "{name}");
                assert_eq!(toy.parse(&toy.encode(&c)).ok(), Some(c.clone()), "{name}");
                assert_eq!(command_json(&toy, &c), case["command"], "{name}");
                // `parse` accepts op 9 under any rules (the rules check is apply's).
                assert!(ToyMachine::new(false).parse(&toy.encode(&c)).is_ok(), "{name}");
            }
            "parse" => {
                let e = toy.parse(&bytes(&case["bytes"])).err().map(|e| e.code);
                assert_eq!(e, expect["error"].as_str(), "{name}");
            }
            "apply" => check_apply(&toy, case),
            "upgrade" => {
                let mut s = load(&toy, &bytes(&case["state"]));
                let before = s.clone();
                let v = u32::try_from(int(&case["v"])).expect("v");
                match toy.upgrade(&mut s, v) {
                    Ok(()) => assert_eq!(hex(&toy.canonical_bytes(&s)), expect["state"].as_str().expect("state"), "{name}"),
                    Err(e) => {
                        assert_eq!(Some(e.code), expect["error"].as_str(), "{name}");
                        assert_eq!(s, before, "{name}: unchanged on error");
                    }
                }
            }
            "reset" => {
                let t = load(&toy, &bytes(&case["target"]));
                let c = load(&toy, &bytes(&case["current"]));
                let out = toy.reset(&t, &c).unwrap_or_else(|e| panic!("{name}: {}", e.code));
                assert_eq!(hex(&toy.canonical_bytes(&out)), expect["state"].as_str().expect("state"), "{name}");
            }
            "coalesce" => {
                let got = toy.coalesce(&command(&case["held"]), &command(&case["next"]));
                let want = match expect {
                    Value::String(s) if s == "keep" => Coalesce::Keep,
                    Value::String(s) if s == "cancel_both" => Coalesce::CancelBoth,
                    v => Coalesce::Merge(command(&v["merge"])),
                };
                assert_eq!(got, want, "{name}");
            }
            "rebind" => {
                let cmds: Vec<ToyCommand> = case["commands"].as_array().expect("commands").iter().map(command).collect();
                let view = load(&toy, &bytes(&case["view"]));
                let from = u16::try_from(int(&case["from"])).expect("from");
                let to = u16::try_from(int(&case["to"])).expect("to");
                match toy.rebind(&cmds, from, to, &view, &bytes(&case["origin"])) {
                    Ok((out, origin)) => {
                        let want: Vec<ToyCommand> = expect["commands"].as_array().expect("commands").iter().map(command).collect();
                        assert_eq!(out, want, "{name}");
                        assert_eq!(hex(&origin), expect["origin"].as_str().expect("origin"), "{name}");
                    }
                    Err(e) => assert_eq!(Some(e.code), expect["error"].as_str(), "{name}"),
                }
            }
            "notice" => {
                let n = &case["notice"];
                let notice = ToyNotice {
                    seq: int(&n["seq"]),
                    author: u16::try_from(int(&n["author"])).expect("author"),
                    other: u16::try_from(int(&n["other"])).expect("other"),
                    other_seq: int(&n["other_seq"]),
                    key: u8::try_from(int(&n["key"])).expect("key"),
                };
                assert_eq!(hex(&toy.notice_bytes(&notice)), expect["bytes"].as_str().expect("bytes"), "{name}");
                for d in expect["concerns"].as_array().expect("concerns") {
                    assert!(toy.notice_concerns(&notice, u16::try_from(int(d)).expect("d")), "{name}");
                }
                for d in expect["does_not_concern"].as_array().expect("does_not_concern") {
                    assert!(!toy.notice_concerns(&notice, u16::try_from(int(d)).expect("d")), "{name}");
                }
            }
            "refusal_bytes" => {
                let r = &case["refusal"];
                let code: &'static str = Box::leak(r["code"].as_str().expect("code").to_owned().into_boxed_str());
                let op: Option<&'static str> = r["op"].as_str().map(|s| &*Box::leak(s.to_owned().into_boxed_str()));
                let refusal = SmRefusal { code, op, target: opt_u64(&r["target"]), base: None, current: None };
                let author = u16::try_from(int(&case["author"])).expect("author");
                let got = toy.refusal_bytes(int(&case["seq"]), author, &refusal);
                assert_eq!(hex(&got), expect["bytes"].as_str().expect("bytes"), "{name}");
            }
            other => panic!("unknown kind {other}"),
        }
    }
    // Every op and every refusal code of §5.3 has a case.
    for k in ["load", "command", "parse", "apply", "upgrade", "reset", "coalesce", "rebind", "notice", "refusal_bytes"] {
        assert!(kinds.contains_key(k), "no {k} case");
    }
    let codes: Vec<&str> = cases(&file).iter().filter_map(|c| c["expect"]["refused"]["code"].as_str()).collect();
    for code in ["engine/toy_bad_ctx", "engine/toy_rules", "engine/toy_gone", "engine/toy_overflow", "engine/toy_bad_id", "engine/toy_cap", "engine/toy_fail"] {
        assert!(codes.contains(&code), "no apply case for {code}");
    }
    let ops: Vec<&str> = cases(&file).iter().filter(|c| c["kind"] == "apply").filter_map(|c| c["command"]["op"].as_str()).collect();
    for op in ["set", "add", "del", "create", "name", "forget", "fail", "panic", "mul"] {
        assert!(ops.contains(&op), "no apply case for {op}");
    }
}

/// The toy and the fake exist only with the feature `sim`, which no default
/// feature enables: these test builds have it through the crate's
/// dev-dependency on itself, and `Cargo.toml` lists `sim = []` with no
/// `default`.
// spec: ENG-TOY-002
#[test]
fn the_toy_and_the_fake_are_behind_the_feature_sim_only() {
    let manifest = include_str!("../Cargo.toml");
    let features = manifest.split("[features]").nth(1).expect("a [features] table");
    let features = features.split("\n[").next().expect("the table");
    let entries: Vec<&str> = features.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
    assert_eq!(entries, vec!["sim = []"]);
    let lib = include_str!("../src/lib.rs");
    for module in ["fake", "toy", "sim"] {
        assert!(lib.contains(&format!("#[cfg(feature = \"sim\")]\npub mod {module};")), "{module} is behind `sim`");
    }
}

// ---------- the fake sealer (engine.md §6.3)

fn class_name(c: &Class) -> &'static str {
    match c {
        Class::Skip => "skip",
        Class::Tamper => "tamper",
        Class::Refuse => "refuse",
        Class::Accept => "accept",
        Class::NeedFrames(_) => "need",
    }
}

fn kind_name(k: taskdeck_sync::Kind) -> &'static str {
    match k {
        taskdeck_sync::Kind::Cmd => "cmd",
        taskdeck_sync::Kind::Ctl => "ctl",
        taskdeck_sync::Kind::Beat => "beat",
        taskdeck_sync::Kind::Snap => "snap",
    }
}

/// A receipt in the file's form, through the public API only.
fn receipt_json(fake: &FakeSealer, r: &Result<Receipt<FakeMembership>, taskdeck_sync::SealerError>) -> Value {
    let r = match r {
        Err(e) => return serde_json::json!({ "error": e.code }),
        Ok(r) => r,
    };
    let need: Vec<Value> = match &r.class {
        Class::NeedFrames(v) => v.iter().map(|(t, h)| serde_json::json!([t, hex(h)])).collect(),
        _ => Vec::new(),
    };
    let halt = match r.halt {
        None => Value::Null,
        Some(taskdeck_sync::Halt::Commitment) => serde_json::json!("commitment"),
        Some(taskdeck_sync::Halt::UnknownRules(v)) => serde_json::json!({ "unknown_rules": v }),
    };
    serde_json::json!({
        "class": class_name(&r.class),
        "code": r.code,
        "author": hex(&r.author),
        "ctr": r.ctr,
        "chain": hex(&r.chain),
        "frame_hash": hex(&r.frame_hash),
        "own_dev_foreign": r.own_dev_foreign,
        "based_on_unchecked": r.based_on_unchecked,
        "kind": r.opened.as_ref().map(|o| kind_name(o.kind)),
        "need": need,
        "halt": halt,
        "keys_changed": r.keys_changed,
        "membership": hex(&fake.membership_bytes(&r.membership)),
    })
}

/// How a receipt case is classified (the file's `notes`).
struct Replay {
    after: usize,
    retained_from: u64,
    targets: bool,
    forget_below: Option<u32>,
}

/// A fresh `create(seed, name)` classifies the log's first `after` frames from
/// `M_0`, then the case's frame at `after + 1`. Returns the fake, the membership
/// and chain values it reached, and the case's receipt.
#[allow(clippy::type_complexity)]
fn replay(
    seed: u64,
    name: &str,
    log: &[Vec<u8>],
    frame: Option<&[u8]>,
    how: &Replay,
) -> (FakeSealer, FakeMembership, Vec<Hash32>, Option<Result<Receipt<FakeMembership>, taskdeck_sync::SealerError>>) {
    let (mut fake, _, _) = FakeSealer::create(seed, name);
    let own_dev = fake.own_dev();
    let own: BTreeMap<u64, Hash32> = log
        .iter()
        .filter_map(|f| fake.frame_author(f).ok().filter(|(d, _)| *d == own_dev).map(|(_, c)| (c, fake.frame_hash(f))))
        .collect();
    // `M_0` is the default membership, and its canonical bytes parse (`seal.md` §13.3).
    let mut m = FakeMembership::default();
    assert_eq!(fake.membership_from_bytes(&fake.membership_bytes(&m)).as_ref(), Ok(&m));
    let mut chains = vec![fake.chain_zero()];
    let one = |fake: &mut FakeSealer, m: &FakeMembership, chains: &[Hash32], s: u64, f: &[u8], retained_from: u64, targets: bool| {
        let chain_at = |b: u64| if b >= retained_from && b < s { chains.get(b as usize).copied() } else { None };
        let own_sealed = |c: u64| own.get(&c).copied();
        let skip_targets = |t: u64| if targets && t >= 1 { log.get(t as usize - 1).map(Vec::as_slice) } else { None };
        fake.classify(&ClassifyInput {
            seq: s,
            frame: f,
            prev_chain: chains[(s - 1) as usize],
            membership: m,
            chain_at: &chain_at,
            retained_from,
            own_sealed: &own_sealed,
            skip_targets: &skip_targets,
        })
    };
    for (i, f) in log[..how.after].iter().enumerate() {
        let s = i as u64 + 1;
        let r = one(&mut fake, &m, &chains, s, f, 1, true).expect("the log classifies");
        // The chain is computed independently of classification (SEAL-CHAIN-002).
        assert_eq!(r.chain, fake.chain_step(&chains[i], s, f));
        m = r.membership;
        chains.push(r.chain);
    }
    if let Some(e) = how.forget_below {
        fake.forget_roots_below(e);
    }
    let receipt = frame.map(|f| one(&mut fake, &m, &chains, how.after as u64 + 1, f, how.retained_from, how.targets));
    (fake, m, chains, receipt)
}

/// The fields of `export_identity` (docs/sync/status/engine/api-fake-toy.md):
/// `dev`, the key, `sign_pk`, `hpke_pk`, `board_id`, the anchor, the roots.
struct Identity {
    dev: Dev,
    sk: [u8; 32],
    sign_pk: Hash32,
    hpke_pk: Hash32,
    board_id: [u8; 16],
    anchor: Option<(Hash32, [u8; 16], [u8; 32])>,
    roots: Vec<(u32, [u8; 32])>,
}

fn identity(fake: &FakeSealer) -> Identity {
    let b = fake.export_identity().to_vec();
    assert_eq!(&b[..8], b"FKID\x00\x00\x00\x01");
    let mut at = 8;
    let mut take = |n: usize| {
        let out = b[at..at + n].to_vec();
        at += n;
        out
    };
    let dev = take(16).try_into().expect("dev");
    let sk = take(32).try_into().expect("sk");
    let sign_pk = take(32).try_into().expect("sign_pk");
    let hpke_pk = take(32).try_into().expect("hpke_pk");
    let board_id = take(16).try_into().expect("board_id");
    let anchor = match take(1)[0] {
        0 => None,
        _ => Some((take(32).try_into().expect("rsign_pk"), take(16).try_into().expect("salt"), take(32).try_into().expect("root"))),
    };
    let n = u32::from_be_bytes(take(4).try_into().expect("n"));
    let roots = (0..n)
        .map(|_| (u32::from_be_bytes(take(4).try_into().expect("e")), take(32).try_into().expect("root")))
        .collect();
    Identity { dev, sk, sign_pk, hpke_pk, board_id, anchor, roots }
}

fn empty_bundle() -> FakeBundle {
    FakeBundle {
        roots: Vec::new(),
        material: SnapshotMaterial { seq: 0, chain: [0; 32], hash_version: 1, state_hash: [0; 32], board: Vec::new(), membership: Vec::new() },
        history: Vec::new(),
        added_seq: 0,
    }
}

/// Keys, the genesis frame, the scenario's log, one failing frame per step of
/// `seal.md` §14.2 with its receipt, and a snapshot, replayed through the
/// public API (FakeSealer::create, join and the Sealer trait).
// spec: ENG-FAKE-001
// spec: ENG-FAKE-002
// spec: SEAL-WRAP-003
#[test]
fn fake_vectors() {
    let file = parse(FAKE);
    let all = cases(&file);
    let log_case = all.iter().find(|c| c["kind"] == "log").expect("a log case");
    let log_seed = u64_hex(&log_case["seed"]);
    let log_name = log_case["device_name"].as_str().expect("device_name").to_owned();
    let log: Vec<Vec<u8>> = log_case["frames"].as_array().expect("frames").iter().map(bytes).collect();
    let mut kinds = BTreeMap::new();
    for case in all {
        let name = case["name"].as_str().expect("name");
        let kind = case["kind"].as_str().expect("kind");
        *kinds.entry(kind.to_owned()).or_insert(0) += 1;
        let expect = &case["expect"];
        match kind {
            "keys" => {
                // A joining device's keys come first from its seed: the key, then `dev`.
                let fake = FakeSealer::join(u64_hex(&case["seed"]), "any", empty_bundle());
                let id = identity(&fake);
                assert_eq!(hex(&fake.own_dev()), expect["dev"].as_str().expect("dev"), "{name}");
                assert_eq!((hex(&id.sk), hex(&id.sign_pk), hex(&id.hpke_pk)), (
                    expect["sk"].as_str().expect("sk").to_owned(),
                    expect["sign_pk"].as_str().expect("sign_pk").to_owned(),
                    expect["hpke_pk"].as_str().expect("hpke_pk").to_owned(),
                ), "{name}");
                assert_eq!(id.dev, fake.own_dev());
            }
            "genesis" => {
                let (fake, frame, board) = FakeSealer::create(u64_hex(&case["seed"]), case["device_name"].as_str().expect("name"));
                assert_eq!(hex(&frame), expect["frame"].as_str().expect("frame"), "{name}");
                assert_eq!(hex(&board), expect["board"].as_str().expect("board"), "{name}");
                let id = identity(&fake);
                assert_eq!(hex(&id.dev), expect["dev"].as_str().expect("dev"));
                assert_eq!(hex(&id.sk), expect["sk"].as_str().expect("sk"));
                assert_eq!(hex(&id.sign_pk), expect["sign_pk"].as_str().expect("sign_pk"));
                assert_eq!(hex(&id.hpke_pk), expect["hpke_pk"].as_str().expect("hpke_pk"));
                assert_eq!(hex(&id.board_id), expect["board_id"].as_str().expect("board_id"));
                let (rsign_pk, salt, root) = id.anchor.expect("the creating device holds the genesis anchor");
                assert_eq!(hex(&rsign_pk), expect["rsign_pk"].as_str().expect("rsign_pk"));
                assert_eq!(hex(&salt), expect["salt"].as_str().expect("salt"));
                assert_eq!(hex(&root), expect["root_1"].as_str().expect("root_1"));
                assert_eq!(id.roots, vec![(1, root)]);
                assert_eq!(fake.export_identity().len() as u64, int(&expect["export_identity_len"]));
                assert_eq!(hex(&fake.frame_hash(&frame)), expect["frame_hash"].as_str().expect("frame_hash"));
                assert_eq!(hex(&fake.chain_zero()), expect["chain_0"].as_str().expect("chain_0"));
                let (_, m, chains, r) = replay(u64_hex(&case["seed"]), case["device_name"].as_str().expect("name"), &[], Some(&frame), &Replay { after: 0, retained_from: 1, targets: true, forget_below: None });
                let r = r.expect("classified").expect("no local fault");
                assert_eq!(class_name(&r.class), "accept", "{name}");
                assert_eq!(hex(&r.chain), expect["chain_1"].as_str().expect("chain_1"));
                assert_eq!(hex(&fake.membership_bytes(&r.membership)), expect["membership"].as_str().expect("membership"));
                assert_eq!(m, FakeMembership::default());
                assert_eq!(chains.len(), 1);
                let v = fake.view(&r.membership);
                assert_eq!(hex(&v.board_id), expect["board_id"].as_str().expect("board_id"));
                assert_eq!(hex(&v.recovery_dev), expect["dev_r"].as_str().expect("dev_r"));
                assert_eq!(v.recovery_fp, expect["recovery_fp"].as_str().expect("recovery_fp"));
                assert_eq!(v.devices[0].key_fp, expect["key_fp"].as_str().expect("key_fp"));
                let Some(taskdeck_sync::Opened { entry_id, payload, items, .. }) = &r.opened else { panic!("opened") };
                assert_eq!(hex(entry_id), expect["entry_id"].as_str().expect("entry_id"));
                assert_eq!(hex(payload), expect["payload"].as_str().expect("payload"));
                let [taskdeck_sync::CtlItem::Genesis { hash_version: 1, state_hash }] = items.as_slice() else { panic!("Genesis") };
                assert_eq!(hex(state_hash), expect["state_hash"].as_str().expect("state_hash"));
                // Seq 1 is the recovery identity's frame, not this device's (engine.md §6.1 `open_own`).
                assert!(fake.open_own(&r.membership, &frame).is_err());
            }
            // The encodings and primitives are crate-private: src/fake/tests/vector_tests.rs checks them.
            "primitives" | "item" | "beat" | "snap" => {}
            "log" => {
                let (fake, _, chains, _) = replay(log_seed, &log_name, &log, None, &Replay { after: log.len(), retained_from: 1, targets: true, forget_below: None });
                let receipts = expect["receipts"].as_array().expect("receipts");
                assert_eq!(receipts.len(), log.len());
                for k in 0..log.len() {
                    let (f, _, _, r) = replay(log_seed, &log_name, &log, Some(&log[k]), &Replay { after: k, retained_from: 1, targets: true, forget_below: None });
                    assert_eq!(receipt_json(&f, &r.expect("classified")), receipts[k], "{name}: seq {}", k + 1);
                    assert_eq!(hex(&chains[k + 1]), receipts[k]["chain"].as_str().expect("chain"));
                }
                let roots: Vec<Value> = identity(&fake).roots.iter().map(|(e, r)| serde_json::json!([e, hex(r)])).collect();
                assert_eq!(Value::from(roots), expect["roots"], "{name}: the roots the log gave Desk");
            }
            "receipt" => {
                let how = Replay {
                    after: usize::try_from(int(&case["after"])).expect("after"),
                    retained_from: int(&case["retained_from"]),
                    targets: case["targets_supplied"].as_bool().expect("targets_supplied"),
                    forget_below: case["forget_roots_below"].as_u64().map(|e| u32::try_from(e).expect("epoch")),
                };
                let frame = bytes(&case["frame"]);
                let (fake, _, _, r) = replay(log_seed, &log_name, &log, Some(&frame), &how);
                assert_eq!(receipt_json(&fake, &r.expect("classified")), *expect, "{name}");
            }
            "snapshot" => {
                let (mut fake, m, chains, _) = replay(u64_hex(&case["seed"]), &log_name, &log, None, &Replay { after: log.len(), retained_from: 1, targets: true, forget_below: None });
                let board = bytes(&case["board"]);
                let s = int(&expect["seq"]);
                let membership = fake.membership_bytes(&m);
                assert_eq!(hex(&membership), expect["membership"].as_str().expect("membership"));
                let material = SnapshotMaterial {
                    seq: s,
                    chain: chains[s as usize],
                    hash_version: 1,
                    state_hash: arr(&expect["state_hash"]),
                    board,
                    membership,
                };
                let blob = fake.seal_snapshot(&m, &material).expect("seals");
                assert_eq!(hex(&blob.bytes), expect["bytes"].as_str().expect("bytes"), "{name}");
                assert_eq!(hex(&blob.id), expect["blob_id"].as_str().expect("blob_id"));
                assert_eq!(hex(&blob.sha256), expect["sha256"].as_str().expect("sha256"));
                assert_eq!((u64::from(blob.parts), blob.inflated, u64::from(blob.epoch)), (int(&expect["parts"]), int(&expect["inflated"]), int(&expect["epoch"])));
                let info = SnapInfo {
                    seq: s,
                    hash: material.state_hash,
                    hash_version: 1,
                    blob: blob.id,
                    sha256: blob.sha256,
                    size: int(&expect["size"]),
                    parts: blob.parts,
                    inflated: blob.inflated,
                    epoch: blob.epoch,
                };
                let writer = fake.own_dev();
                let (opened, epoch, inflated) = fake.open_snapshot(&[&blob.bytes], &info, &writer, true).expect("opens");
                assert_eq!((opened, epoch, inflated), (material, blob.epoch, blob.inflated));
            }
            other => panic!("unknown kind {other}"),
        }
    }
    for k in ["keys", "genesis", "primitives", "item", "beat", "snap", "log", "receipt", "snapshot"] {
        assert!(kinds.contains_key(k), "no {k} case");
    }
}

fn sha2_of(b: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(b).into()
}
