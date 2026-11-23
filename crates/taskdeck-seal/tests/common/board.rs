//! A board simulator over the public API only (task S3), for the `tamper`,
//! `parsers` and `vectors` targets: a recovery key, a genesis, devices,
//! roots and the log. Every classification goes through `replay`, which
//! rebuilds the inputs of §14.1 from a JSON record of them, so that each
//! published case (`vectors`) is exactly what `classify` was given.
//!
//! Randomness: with the feature `test-entropy` every drawn byte comes from a
//! counter (`Rng`), so the vectors are deterministic; without it, `OsEntropy`.
#![allow(dead_code)]

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use taskdeck_seal::*;

use crate::common::{hex, unhex, unhex_n};

/// Bytes for the simulator: deterministic under `test-entropy`.
pub struct Rng {
    counter: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng { counter: seed << 32 }
    }
    /// `n` bytes: SHA-256 of a counter, block after block.
    pub fn bytes(&mut self, n: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(n + 32);
        while out.len() < n {
            self.counter += 1;
            let d: [u8; 32] = <sha2::Sha256 as sha2::Digest>::digest(self.counter.to_be_bytes()).into();
            out.extend_from_slice(&d);
        }
        out.truncate(n);
        out
    }
    pub fn arr<const N: usize>(&mut self) -> [u8; N] {
        self.bytes(N).try_into().unwrap()
    }
}

/// An entropy source replaying `bytes` (with `test-entropy`), or the
/// operating system's.
#[cfg(feature = "test-entropy")]
pub fn source(bytes: Vec<u8>) -> Box<dyn Entropy> {
    Box::new(FixedEntropy::new(bytes))
}

#[cfg(not(feature = "test-entropy"))]
pub fn source(_bytes: Vec<u8>) -> Box<dyn Entropy> {
    Box::new(OsEntropy)
}

/// The recovery keys of a known seed, through its sheet (§18.1): the 52
/// characters of the seed, then the first 20 bits of
/// `H(ctx("sheet-check") ‖ seed)` as 4 characters.
pub fn recovery_from_seed(seed: &[u8; 32]) -> RecoveryKeys {
    let d: [u8; 32] = <sha2::Sha256 as sha2::Digest>::digest([b"taskdeck/v1/sheet-check\0".as_slice(), seed].concat()).into();
    let check = base32::encode(&d[..3]);
    let sheet = format!("{}{}", base32::encode(seed), &check[..4]);
    RecoverySeed::from_sheet(&sheet).unwrap().keys()
}

/// The canonical bytes of the empty state `M_0` (every field zero or empty).
pub fn empty_membership() -> Vec<u8> {
    let mut v = b"TDM1".to_vec();
    v.extend(1u32.to_be_bytes());
    v.extend([0u8; 76]); // board_id .. home_seq
    v.push(0); // home_host
    v.extend([0u8; 5 + 8 + 2]); // door, purge_seq, next_device_no
    v.extend([0u8; 4 + 2 + 2]); // n_commit, n_recovery, n_dev
    v.extend([0u8; 2]); // two flags: pending_override, last_reset
    v.extend([0u8; 4 + 4]); // n_skip, n_ctl
    v
}

pub fn copy_membership(m: &Membership) -> Membership {
    Membership::from_canonical_bytes(&m.to_canonical_bytes()).unwrap()
}

#[derive(Clone, Copy, Debug)]
pub enum Who {
    D(usize),
    R,
    OldR(usize),
}

#[derive(Default, Clone)]
pub struct Spec {
    pub epoch: Option<u32>,
    pub ctr: Option<u64>,
    pub at_ms: Option<i64>,
    pub based_on: Option<Head>,
    pub root: Option<[u8; 32]>,
}

#[derive(Default, Clone)]
pub struct Opts {
    pub own: Option<usize>,
    pub supported: Option<Vec<u32>>,
    pub retained_from: Option<u64>,
    pub hide_frames: Vec<u64>,
    pub hide_roots: Vec<u32>,
    pub seq: Option<u64>,
    /// The Skip targets whose frames the receiver is given (minus `hide_frames`).
    pub targets: Vec<u64>,
    /// The anchor for seq 1 (set by `Sim::new` for the genesis).
    pub genesis: Option<(Vec<u8>, [u8; 16], [u8; 32])>,
}

pub const T0: i64 = 1_767_225_600_000;

pub struct Sim {
    pub rng: Rng,
    pub rec: Vec<RecoveryKeys>,
    /// The seed of each of `rec` (task S4: a backup reader types it).
    pub seeds: Vec<[u8; 32]>,
    pub salt: [u8; 16],
    pub board: BoardId,
    pub devs: Vec<DeviceKeys>,
    pub roots: BTreeMap<u32, [u8; 32]>,
    pub m: Membership,
    pub chains: Vec<Chain>,
    pub frames: Vec<Vec<u8>>,
    pub now: i64,
    pub sealed: BTreeMap<(DevId, u64), FrameHash>,
    pub staged: Option<(u32, [u8; 32])>,
    /// The genesis inputs and receipt, for the vectors.
    pub genesis_case: Option<(Value, Value)>,
    /// A history record for every accepted control entry (task S4).
    pub history: Vec<HistoryRecord>,
}

impl Sim {
    /// A board with `n` current devices.
    pub fn new(seed: u64, n: usize) -> Sim {
        let mut rng = Rng::new(seed);
        let seed0 = rng.arr::<32>();
        let rk = recovery_from_seed(&seed0);
        let salt = rng.arr::<16>();
        let board = board_id(&rk.public().sign_pk, &salt);
        let d0 = DeviceKeys::generate(&mut *source(rng.bytes(80))).unwrap();
        let r1 = rng.arr::<32>();
        let mut s = Sim {
            rng,
            rec: vec![rk],
            seeds: vec![seed0],
            salt,
            board,
            devs: vec![d0],
            roots: BTreeMap::from([(1, r1)]),
            m: Membership::from_canonical_bytes(&empty_membership()).unwrap(),
            chains: vec![chain_zero(&board)],
            frames: Vec::new(),
            now: T0,
            sealed: BTreeMap::new(),
            staged: None,
            genesis_case: None,
            history: Vec::new(),
        };
        let g = s.genesis_item();
        let f = s.ctl(Who::R, vec![CtlItem::Genesis(g)], &Spec::default());
        let o = Opts { genesis: Some(s.anchor_parts()), ..Opts::default() };
        let inputs = s.inputs("genesis", &f, &o);
        let r = replay(&inputs).unwrap();
        assert!(matches!(r.class, Class::Accept), "{:?}", r.code);
        s.genesis_case = Some((inputs, outputs(&r)));
        s.commit(f, r);
        for _ in 1..n {
            s.add_device();
        }
        s
    }

    pub fn anchor_parts(&self) -> (Vec<u8>, [u8; 16], [u8; 32]) {
        (self.rec[0].public().sign_pk.to_vec(), self.salt, self.roots[&1])
    }

    pub fn genesis_item(&mut self) -> Genesis {
        let root = EpochRoot::from_bytes(self.roots[&1]);
        let commit = root.commitment(&self.board, 1);
        let rp = self.rec[0].public();
        let d = self.devs[0].public();
        let mut rc = [(d.dev, d.hpke_pk), (rp.dev(&self.board), rp.hpke_pk)];
        rc.sort();
        let board = self.board;
        let wraps = rc
            .iter()
            .map(|(id, pk)| wrap_root(&root, &board, 1, &commit, *id, pk, &mut *source(self.rng.bytes(32))).unwrap())
            .collect();
        Genesis {
            salt: self.salt,
            recovery: rp,
            device: GenesisDevice { dev: d.dev, sign_pk: d.sign_pk, hpke_pk: d.hpke_pk, name: "Laptop".into() },
            epoch: 1,
            commit,
            wraps,
            rules: 1,
            hash_version: 1,
            state_hash: [0x33; 32],
            snapshot: None,
        }
    }

    pub fn seq(&self) -> u64 {
        self.frames.len() as u64
    }

    pub fn head(&self) -> Head {
        Head { seq: self.seq(), chain: *self.chains.last().unwrap() }
    }

    pub fn signer(&self, who: Who) -> Signer<'_> {
        match who {
            Who::D(i) => Signer::Device(&self.devs[i]),
            Who::R => Signer::Recovery(self.rec.last().unwrap(), self.board),
            Who::OldR(i) => Signer::Recovery(&self.rec[i], self.board),
        }
    }

    pub fn dev(&self, who: Who) -> DevId {
        match who {
            Who::D(i) => self.devs[i].public().dev,
            Who::R => self.rec.last().unwrap().public().dev(&self.board),
            Who::OldR(i) => self.rec[i].public().dev(&self.board),
        }
    }

    pub fn next_ctr(&self, who: Who) -> u64 {
        let dev = self.dev(who);
        if let Some(d) = self.m.devices().iter().find(|d| d.dev == dev) {
            return d.last_ctr + 1;
        }
        // Only the current recovery record is reachable through §26; an
        // earlier one's counter is tracked here.
        let r = self.m.current_recovery();
        if r.dev == dev { r.last_ctr + 1 } else { 1 }
    }

    pub fn epoch(&self) -> u32 {
        self.m.epoch().max(1)
    }

    fn fill(&self, who: Who, sp: &Spec) -> (u64, u32, [u8; 32], i64, Head) {
        let epoch = sp.epoch.unwrap_or(self.epoch());
        (
            sp.ctr.unwrap_or_else(|| self.next_ctr(who)),
            epoch,
            sp.root.unwrap_or_else(|| self.roots[&epoch]),
            sp.at_ms.unwrap_or(self.now),
            sp.based_on.unwrap_or(self.head()),
        )
    }

    pub fn seal(&mut self, who: Who, plain: &Plain, ctr: u64, epoch: u32, root: [u8; 32]) -> Vec<u8> {
        let key = derive_epoch_key(&EpochRoot::from_bytes(root), &self.board, epoch, KeyKind::Entry);
        let nonce = self.rng.bytes(24);
        let f = seal_frame(&self.signer(who), &self.board, ctr, epoch, &key, plain, &mut *source(nonce)).unwrap();
        self.sealed.insert((self.dev(who), ctr), frame_hash(&f));
        f
    }

    pub fn plain(&mut self, who: Who, kind: Kind, payload: Vec<u8>, sp: &Spec) -> Vec<u8> {
        let (ctr, epoch, root, at_ms, based_on) = self.fill(who, sp);
        let plain = Plain { entry_id: EntryId(self.rng.arr()), kind, at_ms, based_on, payload };
        self.seal(who, &plain, ctr, epoch, root)
    }

    pub fn cmd(&mut self, who: Who, sp: &Spec) -> Vec<u8> {
        self.plain(who, Kind::Cmd, b"{\"op\":\"x\"}".to_vec(), sp)
    }

    pub fn beat(&mut self, who: Who, rules: Vec<u32>, sp: &Spec) -> Vec<u8> {
        let beat = Beat { rules, envelopes: vec![(1, 1)], hash_versions: vec![1], state: None, acks: vec![] };
        self.plain(who, Kind::Beat, beat_payload(&beat).unwrap(), sp)
    }

    /// A control frame; `inner` signs the payload (normally the author).
    pub fn ctl_by(&mut self, who: Who, inner: Who, items: Vec<CtlItem>, sp: &Spec) -> Vec<u8> {
        let (ctr, epoch, root, at_ms, based_on) = self.fill(who, sp);
        let entry_id = EntryId(self.rng.arr());
        let payload = ctl_payload(&items, &self.signer(inner), &self.board, ctr, &entry_id, at_ms).unwrap();
        let plain = Plain { entry_id, kind: Kind::Ctl, at_ms, based_on, payload };
        self.seal(who, &plain, ctr, epoch, root)
    }

    pub fn ctl(&mut self, who: Who, items: Vec<CtlItem>, sp: &Spec) -> Vec<u8> {
        self.ctl_by(who, who, items, sp)
    }

    /// The JSON record of the inputs of §14.1 for `frame` at the next seq.
    pub fn inputs(&self, name: &str, frame: &[u8], o: &Opts) -> Value {
        let seq = o.seq.unwrap_or(self.seq() + 1);
        let mut m = Map::new();
        m.insert("name".into(), json!(name));
        m.insert("seq".into(), json!(seq));
        m.insert("frame".into(), json!(hex(frame)));
        let prev = self.chains.get(seq.saturating_sub(1) as usize).copied().unwrap_or(*self.chains.last().unwrap());
        m.insert("prev_chain".into(), json!(hex(&prev.0)));
        m.insert("membership".into(), json!(hex(&self.m.to_canonical_bytes())));
        let e = self.m.epoch();
        let mut roots = Map::new();
        for x in [e.saturating_sub(1), e] {
            if let Some(r) = self.roots.get(&x)
                && !o.hide_roots.contains(&x)
            {
                roots.insert(x.to_string(), json!(hex(r)));
            }
        }
        m.insert("roots".into(), Value::Object(roots));
        let retained = o.retained_from.unwrap_or(1);
        let mut chains = Map::new();
        for b in retained..seq.min(self.chains.len() as u64) {
            chains.insert(b.to_string(), json!(hex(&self.chains[b as usize].0)));
        }
        m.insert("chains".into(), Value::Object(chains));
        m.insert("retained_from".into(), json!(retained));
        m.insert(
            "own".into(),
            match o.own {
                None => Value::Null,
                Some(i) => {
                    let dev = self.devs[i].public().dev;
                    let mut sealed = Map::new();
                    for ((d, c), h) in &self.sealed {
                        if *d == dev {
                            sealed.insert(c.to_string(), json!(hex(&h.0)));
                        }
                    }
                    json!({ "identity": hex(&self.devs[i].to_identity_bytes()), "sealed": sealed })
                }
            },
        );
        m.insert("supported_rules".into(), json!(o.supported.clone().unwrap_or_else(|| vec![1])));
        m.insert(
            "genesis".into(),
            match &o.genesis {
                None => Value::Null,
                Some((pk, salt, root)) => json!({ "rsign_pk": hex(pk), "salt": hex(salt), "root1": hex(root) }),
            },
        );
        // The frames a Skip item of this frame names, when held.
        let mut targets = Map::new();
        for &t in &o.targets {
            if !o.hide_frames.contains(&t)
                && let Some(f) = self.frames.get(t as usize - 1)
            {
                targets.insert(t.to_string(), json!(hex(f)));
            }
        }
        m.insert("skip_targets".into(), Value::Object(targets));
        Value::Object(m)
    }

    /// Classifies through `replay` (the vectors' path).
    pub fn classify(&self, frame: &[u8], o: &Opts) -> Result<Receipt, SealError> {
        replay(&self.inputs("", frame, o))
    }

    pub fn commit(&mut self, frame: Vec<u8>, r: Receipt) {
        if matches!(r.class, Class::Accept)
            && let Some(o) = &r.opened
            && o.kind == Kind::Ctl
        {
            let seq = self.frames.len() as u64 + 1;
            self.history.push(HistoryRecord::from_accepted(seq, &frame, o).unwrap());
        }
        let old = self.m.epoch();
        self.chains.push(r.chain);
        self.frames.push(frame);
        self.m = r.membership;
        if self.m.epoch() > old
            && let Some((e, root)) = self.staged.take()
        {
            self.roots.insert(e, root);
        }
        self.now += 1000;
    }

    pub fn push(&mut self, frame: Vec<u8>) -> Receipt {
        let r = self.classify(&frame, &Opts::default()).unwrap();
        let out = Receipt {
            class: match &r.class {
                Class::Skip => Class::Skip,
                Class::Tamper => Class::Tamper,
                Class::Refuse => Class::Refuse,
                Class::Accept => Class::Accept,
                Class::NeedFrames(v) => Class::NeedFrames(v.clone()),
            },
            code: r.code,
            chain: r.chain,
            own_dev_foreign: r.own_dev_foreign,
            based_on_unchecked: r.based_on_unchecked,
            opened: None,
            membership: copy_membership(&r.membership),
            learned_root: None,
            halt: None,
        };
        self.commit(frame, r);
        out
    }

    pub fn accept(&mut self, frame: Vec<u8>) {
        let r = self.push(frame);
        assert!(matches!(r.class, Class::Accept) && r.code.is_none(), "not accepted: {:?}", r.code);
    }

    pub fn added_item(&self, k: &DeviceKeys) -> CtlItem {
        let p = k.public();
        CtlItem::DeviceAdded(DeviceAdded {
            dev: p.dev,
            sign_pk: p.sign_pk,
            hpke_pk: p.hpke_pk,
            name: "Phone".into(),
            transcript: [7; 32],
            bootstrap_seq: self.seq().max(1),
        })
    }

    pub fn new_device(&mut self) -> DeviceKeys {
        DeviceKeys::generate(&mut *source(self.rng.bytes(80))).unwrap()
    }

    pub fn new_recovery(&mut self) -> RecoveryKeys {
        RecoverySeed::generate(&mut *source(self.rng.bytes(32))).unwrap().keys()
    }

    /// New recovery keys and their seed, from the simulator's bytes.
    pub fn new_recovery_seed(&mut self) -> ([u8; 32], RecoveryKeys) {
        let seed = self.rng.arr::<32>();
        (seed, recovery_from_seed(&seed))
    }

    /// A direct recovery-key change by the current recovery identity, with
    /// its rotation (task S4); the new keys become `Who::R`.
    pub fn change_recovery(&mut self) {
        let (seed, nk) = self.new_recovery_seed();
        let p = nk.public();
        let k = CtlItem::RecoveryKeyChange(RecoveryKeyChange { sign_pk: p.sign_pk, hpke_pk: p.hpke_pk });
        let items = self.rotate_items(vec![k]);
        let f = self.ctl(Who::R, items, &Spec::default());
        self.accept(f);
        self.rec.push(nk);
        self.seeds.push(seed);
    }

    pub fn add_device(&mut self) -> usize {
        let k = self.new_device();
        let item = self.added_item(&k);
        self.devs.push(k);
        let f = self.ctl(Who::D(0), vec![item], &Spec::default());
        self.accept(f);
        self.devs.len() - 1
    }

    /// The recipients of a rotation after `prefix`, as in the unit tests.
    pub fn recipients_after(&self, prefix: &[CtlItem]) -> Vec<(DevId, [u8; 32])> {
        let mut out = Vec::new();
        for d in self.m.devices() {
            let revoked = prefix.iter().any(|i| matches!(i, CtlItem::DeviceRevoked(t) if *t == d.dev));
            let reinstated = prefix.iter().any(|i| matches!(i, CtlItem::DeviceReinstated(t) if *t == d.dev));
            if (matches!(d.status, DeviceStatus::Current) && !revoked) || reinstated {
                out.push((d.dev, d.hpke_pk));
            }
        }
        let new_rec = prefix.iter().find_map(|i| match i {
            CtlItem::RecoveryKeyChange(k) => {
                let p = RecoveryPublic { sign_pk: k.sign_pk, hpke_pk: k.hpke_pk };
                Some((p.dev(&self.board), p.hpke_pk))
            }
            _ => None,
        });
        out.push(new_rec.unwrap_or_else(|| {
            let r = self.m.current_recovery();
            (r.dev, r.hpke_pk)
        }));
        out.sort();
        out
    }

    pub fn rotation_to(&mut self, epoch: u32, recipients: &[(DevId, [u8; 32])], purge: Option<u64>) -> EpochRotated {
        let bytes = self.rng.arr::<32>();
        let root = EpochRoot::from_bytes(bytes);
        let commit = root.commitment(&self.board, epoch);
        let mut rc = recipients.to_vec();
        rc.sort();
        let board = self.board;
        let wraps = rc
            .iter()
            .map(|(id, pk)| wrap_root(&root, &board, epoch, &commit, *id, pk, &mut *source(self.rng.bytes(32))).unwrap())
            .collect();
        self.staged = Some((epoch, bytes));
        EpochRotated { epoch, commit, wraps, purge_before: purge }
    }

    pub fn rotate_items(&mut self, prefix: Vec<CtlItem>) -> Vec<CtlItem> {
        let rc = self.recipients_after(&prefix);
        let r = self.rotation_to(self.m.epoch() + 1, &rc, None);
        let mut items = prefix;
        items.push(CtlItem::EpochRotated(r));
        items
    }

    pub fn revoke(&mut self, who: Who, t: usize) {
        let items = self.rotate_items(vec![CtlItem::DeviceRevoked(self.dev(Who::D(t)))]);
        let f = self.ctl(who, items, &Spec::default());
        self.accept(f);
    }

    pub fn rotate(&mut self, who: Who) {
        let items = self.rotate_items(vec![]);
        let f = self.ctl(who, items, &Spec::default());
        self.accept(f);
    }
}

/// Rebuilds the inputs of §14.1 from their JSON record and classifies.
pub fn replay(c: &Value) -> Result<Receipt, SealError> {
    let frame = unhex(c["frame"].as_str().unwrap());
    let membership = Membership::from_canonical_bytes(&unhex(c["membership"].as_str().unwrap())).unwrap();
    let mut roots: BTreeMap<u32, EpochRoot> = BTreeMap::new();
    for (k, v) in c["roots"].as_object().unwrap() {
        roots.insert(k.parse().unwrap(), EpochRoot::from_bytes(unhex_n(v.as_str().unwrap())));
    }
    let mut chains: BTreeMap<u64, Chain> = BTreeMap::new();
    for (k, v) in c["chains"].as_object().unwrap() {
        chains.insert(k.parse().unwrap(), Chain(unhex_n(v.as_str().unwrap())));
    }
    let mut targets: BTreeMap<u64, Vec<u8>> = BTreeMap::new();
    for (k, v) in c["skip_targets"].as_object().unwrap() {
        targets.insert(k.parse().unwrap(), unhex(v.as_str().unwrap()));
    }
    let own_keys = c["own"].get("identity").map(|i| DeviceKeys::from_identity_bytes(&unhex(i.as_str().unwrap())).unwrap());
    let mut sealed: BTreeMap<u64, FrameHash> = BTreeMap::new();
    if let Some(m) = c["own"].get("sealed").and_then(|s| s.as_object()) {
        for (k, v) in m {
            sealed.insert(k.parse().unwrap(), FrameHash(unhex_n(v.as_str().unwrap())));
        }
    }
    let anchor = c["genesis"].as_object().map(|g| GenesisAnchor {
        rsign_pk: unhex_n(g["rsign_pk"].as_str().unwrap()),
        salt: unhex_n(g["salt"].as_str().unwrap()),
        root1: EpochRoot::from_bytes(unhex_n(g["root1"].as_str().unwrap())),
    });
    let supported: Vec<u32> = c["supported_rules"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u32).collect();
    let roots_f = |e: u32| roots.get(&e);
    let chain_at = |b: u64| chains.get(&b).copied();
    let skip = |t: u64| targets.get(&t).map(|v| v.as_slice());
    let sealed_f = |ctr: u64| sealed.get(&ctr).copied();
    let input = ReceiptInput {
        seq: c["seq"].as_u64().unwrap(),
        frame: &frame,
        prev_chain: Chain(unhex_n(c["prev_chain"].as_str().unwrap())),
        membership: &membership,
        roots: &roots_f,
        chain_at: &chain_at,
        retained_from: c["retained_from"].as_u64().unwrap(),
        own: own_keys.as_ref().map(|k| OwnIdentity { keys: k, sealed: &sealed_f }),
        supported_rules: &supported,
        genesis: anchor.as_ref(),
        skip_targets: &skip,
    };
    classify(&input)
}

pub fn class_name(c: &Class) -> &'static str {
    match c {
        Class::Skip => "Skip",
        Class::Tamper => "Tamper",
        Class::Refuse => "Refuse",
        Class::Accept => "Accept",
        Class::NeedFrames(_) => "NeedFrames",
    }
}

/// The JSON record of a receipt's outputs.
pub fn outputs(r: &Receipt) -> Value {
    let need: Vec<Value> = match &r.class {
        Class::NeedFrames(v) => v.iter().map(|(t, h)| json!([t, hex(&h.0)])).collect(),
        _ => vec![],
    };
    let mut m = Map::new();
    m.insert("class".into(), json!(class_name(&r.class)));
    m.insert("code".into(), json!(r.code));
    if !need.is_empty() {
        m.insert("need_frames".into(), Value::Array(need));
    }
    m.insert("own_dev_foreign".into(), json!(r.own_dev_foreign));
    m.insert("based_on_unchecked".into(), json!(r.based_on_unchecked));
    m.insert("chain_after".into(), json!(hex(&r.chain.0)));
    m.insert("membership_after".into(), json!(hex(&r.membership.to_canonical_bytes())));
    m.insert(
        "learned_root".into(),
        match &r.learned_root {
            None => Value::Null,
            Some((e, root)) => json!({ "epoch": e, "root": hex(&root.to_bytes()[..]) }),
        },
    );
    m.insert(
        "halt".into(),
        match &r.halt {
            None => Value::Null,
            Some(Halt::Commitment) => json!("Commitment"),
            Some(Halt::UnknownRules(v)) => json!(format!("UnknownRules({v})")),
        },
    );
    Value::Object(m)
}

/// The record of a classification: its inputs, then its outputs; asserts
/// the class and code the case is meant to show.
pub fn case(inputs: Value, r: &Receipt, class: &str, code: Option<&str>) -> Value {
    assert_eq!((class_name(&r.class), r.code), (class, code), "{}", inputs["name"]);
    let mut m = inputs.as_object().unwrap().clone();
    for (k, v) in outputs(r).as_object().unwrap() {
        m.insert(k.clone(), v.clone());
    }
    Value::Object(m)
}

/// Replays a published case and compares every output (the reader side of
/// the vectors).
pub fn check_case(c: &Value) {
    let r = replay(c);
    match r {
        Err(e) => assert_eq!(c["error"].as_str(), Some(e.code()), "{}", c["name"]),
        Ok(r) => {
            for (k, v) in outputs(&r).as_object().unwrap() {
                assert_eq!(&c[k.as_str()], v, "{}: {k}", c["name"]);
            }
        }
    }
}
