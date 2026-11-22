//! Tests of the fake sealer (engine.md §6.3). Hex strings are the layouts the
//! spec fixes (engine.md §6.3 payload encodings, `seal.md` §8.1, §13.2, §7 as
//! substituted); stage `engine` turns them into `engine-fake.json` (ENG-FAKE-001).

use super::*;

use proptest::prelude::*;

// ---------- harness

/// The relay's log as the tests model it: the frames and `C_0 .. C_n`.
struct Net {
    frames: Vec<Vec<u8>>,
    chains: Vec<Hash32>,
}

impl Net {
    fn new(board_id: &[u8; 16]) -> Self {
        Net { frames: Vec::new(), chains: vec![chain_zero_of(board_id)] }
    }

    fn next_seq(&self) -> u64 {
        self.frames.len() as u64 + 1
    }

    fn head(&self) -> Head {
        Head { seq: self.frames.len() as u64, chain: self.chains[self.frames.len()] }
    }

    fn push(&mut self, frame: Vec<u8>) -> u64 {
        let s = self.next_seq();
        let c = chain_next(&self.chains[self.frames.len()], s, &h(&[&frame]));
        self.chains.push(c);
        self.frames.push(frame);
        s
    }
}

/// One device: its fake, the membership it derived, its counter and the frames it sealed.
struct Node {
    fake: FakeSealer,
    m: FakeMembership,
    ctr: u64,
    own: BTreeMap<u64, Hash32>,
    retained_from: u64,
}

fn ok<T>(r: Result<T, SealerError>) -> T {
    match r {
        Ok(v) => v,
        Err(e) => panic!("sealer error {}", e.code),
    }
}

fn err_code<T>(r: Result<T, SealerError>) -> &'static str {
    match r {
        Ok(_) => "ok",
        Err(e) => e.code,
    }
}

impl Node {
    fn new(fake: FakeSealer) -> Self {
        Node { fake, m: FakeMembership::default(), ctr: 0, own: BTreeMap::new(), retained_from: 1 }
    }

    fn try_classify(
        &mut self,
        net: &Net,
        s: u64,
        frame: &[u8],
        with_targets: bool,
    ) -> Result<Receipt<FakeMembership>, SealerError> {
        let chains = &net.chains;
        let chain_at = |b: u64| if b < s { chains.get(b as usize).copied() } else { None };
        let own = &self.own;
        let own_sealed = |c: u64| own.get(&c).copied();
        let frames = &net.frames;
        let skip_targets =
            move |t: u64| if with_targets && t >= 1 { frames.get(t as usize - 1).map(Vec::as_slice) } else { None };
        let input = ClassifyInput {
            seq: s,
            frame,
            prev_chain: net.chains[(s - 1) as usize],
            membership: &self.m,
            chain_at: &chain_at,
            retained_from: self.retained_from,
            own_sealed: &own_sealed,
            skip_targets: &skip_targets,
        };
        self.fake.classify(&input)
    }

    /// Classifies a candidate frame at `s` without keeping anything.
    fn classify(&mut self, net: &Net, s: u64, frame: &[u8]) -> Receipt<FakeMembership> {
        ok(self.try_classify(net, s, frame, true))
    }

    /// Classifies the log's frame at `s` and keeps the membership it gives.
    fn deliver(&mut self, net: &Net, s: u64) -> Receipt<FakeMembership> {
        let f = net.frames[(s - 1) as usize].clone();
        let r = self.classify(net, s, &f);
        if !matches!(r.class, Class::NeedFrames(_)) {
            self.m = r.membership.clone();
        }
        r
    }

    /// Seals a body as this device under its membership and the log's head.
    fn seal(&mut self, net: &Net, at_ms: i64, body: Body) -> Sealed {
        let entry_id = ok(self.fake.new_entry_id());
        self.ctr += 1;
        let entry = PlainEntry { entry_id, at_ms, based_on: net.head(), body };
        let sealed = ok(self.fake.seal(&self.m, self.ctr, &entry));
        self.own.insert(self.ctr, sealed.frame_hash);
        sealed
    }

    fn dev(&self) -> Dev {
        self.fake.dev
    }
}

/// Appends a frame and delivers it to every node.
fn submit(net: &mut Net, nodes: &mut [&mut Node], frame: Vec<u8>) -> Vec<Receipt<FakeMembership>> {
    let s = net.push(frame);
    nodes.iter_mut().map(|n| n.deliver(net, s)).collect()
}

fn class_name(c: &Class) -> &'static str {
    match c {
        Class::Skip => "skip",
        Class::Tamper => "tamper",
        Class::Refuse => "refuse",
        Class::Accept => "accept",
        Class::NeedFrames(_) => "need",
    }
}

fn outcome(r: &Receipt<FakeMembership>) -> (&'static str, Option<&'static str>) {
    (class_name(&r.class), r.code)
}

const ACCEPT: (&str, Option<&str>) = ("accept", None);

fn refused(c: &'static str) -> (&'static str, Option<&'static str>) {
    ("refuse", Some(c))
}

fn assert_all(rs: &[Receipt<FakeMembership>], want: (&'static str, Option<&'static str>)) {
    for r in rs {
        assert_eq!(outcome(r), want);
    }
}

const SEED1: u64 = 0x5EED_0000_0000_0001;
const SEED2: u64 = 0x5EED_0000_0000_0002;
const SEED3: u64 = 0x5EED_0000_0000_0003;
const SEED_OTHER: u64 = 0x5EED_0000_0000_0099;
const DAY: i64 = 86_400_000;

fn toy_board() -> Vec<u8> {
    let toy = ToyMachine::new(false);
    toy.canonical_bytes(&ToyState { rules: 1, keys: BTreeMap::new(), items: BTreeMap::new(), marks: BTreeMap::new() })
}

/// Snapshot material of `n`'s state at the log's head.
fn material_at(n: &Node, net: &Net) -> SnapshotMaterial {
    let board = toy_board();
    let membership = n.m.to_bytes();
    let head = net.head();
    SnapshotMaterial {
        seq: head.seq,
        chain: head.chain,
        hash_version: 1,
        state_hash: state_hash_of(&h(&[&board]), &membership),
        board,
        membership,
    }
}

/// A board created by node 1, genesis classified.
fn board() -> (Net, Node) {
    let (fake, frame, _board) = FakeSealer::create(SEED1, "alpha");
    let mut net = Net::new(&fake.board_id);
    let mut n1 = Node::new(fake);
    let r = submit(&mut net, &mut [&mut n1], frame);
    assert_eq!(outcome(&r[0]), ACCEPT);
    (net, n1)
}

/// `adder` adds the device of `seed` and pairs it from a snapshot at the head.
fn add_device(net: &mut Net, adder: &mut Node, others: &mut [&mut Node], seed: u64, at_ms: i64) -> Node {
    let material = material_at(adder, net);
    let bootstrap = material.seq;
    let m_bootstrap = adder.m.clone();
    let (dev, sign_pk, hpke_pk) = FakeSealer::join_keys(seed);
    let body = Body::Ctl(CtlIntent::AddDevice {
        dev,
        sign_pk,
        hpke_pk,
        name: format!("device-{}", seed % 1000),
        transcript: [7; 32],
        bootstrap_seq: bootstrap,
    });
    let sealed = adder.seal(net, at_ms, body);
    assert!(!sealed.keys_changed);
    let s = net.push(sealed.frame);
    assert_eq!(outcome(&adder.deliver(net, s)), ACCEPT);
    for o in others.iter_mut() {
        assert_eq!(outcome(&o.deliver(net, s)), ACCEPT);
    }
    let bundle = adder.fake.bundle(material, s);
    let mut node = Node::new(FakeSealer::join(seed, "joined", bundle));
    assert_eq!(node.dev(), dev);
    node.m = m_bootstrap;
    node.retained_from = bootstrap;
    assert_eq!(outcome(&node.deliver(net, s)), ACCEPT);
    node
}

fn two_devices() -> (Net, Node, Node) {
    let (mut net, mut n1) = board();
    let n2 = add_device(&mut net, &mut n1, &mut [], SEED2, 1_000);
    (net, n1, n2)
}

/// A recovery identity the tests author as.
struct Rk {
    dev: Dev,
    sign_pk: Hash32,
    ctr: u64,
}

fn recovery_of(n: &Node) -> Rk {
    let r = n.m.current_recovery().expect("a current recovery key");
    Rk { dev: r.dev_r, sign_pk: r.sign_pk, ctr: r.last_ctr }
}

/// Seals a body as the recovery identity, with `n`'s fake and roots.
fn seal_as_recovery(n: &mut Node, rk: &mut Rk, net: &Net, at_ms: i64, body: Body) -> Vec<u8> {
    rk.ctr += 1;
    let entry_id = ok(n.fake.new_entry_id());
    let entry = PlainEntry { entry_id, at_ms, based_on: net.head(), body };
    ok(n.fake.seal_as(&rk.dev, &rk.sign_pk, &n.m, rk.ctr, &entry)).frame
}

/// A `ctl` frame of hand-made items by any author, under `n`'s current epoch.
fn raw_ctl(n: &mut Node, dev: &Dev, pk: &Hash32, ctr: u64, net: &Net, at_ms: i64, items: &[Item]) -> Vec<u8> {
    let epoch = n.m.epoch;
    let root = n.fake.roots[&epoch];
    let entry_id = ok(n.fake.new_entry_id());
    ok(n.fake.ctl_frame(dev, pk, ctr, epoch, &root, &entry_id, at_ms, &net.head(), items))
}

/// A `ctl` frame of hand-made items by `n` itself.
fn raw_own(n: &mut Node, net: &Net, at_ms: i64, items: &[Item]) -> Vec<u8> {
    n.ctr += 1;
    let (dev, pk, ctr) = (n.fake.dev, n.fake.sign_pk, n.ctr);
    let f = raw_ctl(n, &dev, &pk, ctr, net, at_ms, items);
    n.own.insert(ctr, h(&[&f]));
    f
}

/// A frame with every envelope field chosen by the test.
struct Raw {
    ver: u8,
    suite: u8,
    epoch: u32,
    root: [u8; 32],
    plain: Vec<u8>,
}

fn raw_frame(n: &Node, dev: &Dev, pk: &Hash32, ctr: u64, raw: &Raw) -> Vec<u8> {
    let env = Envelope {
        board_id: &n.fake.board_id,
        dev,
        sign_pk: pk,
        ctr,
        ver: raw.ver,
        suite: raw.suite,
        epoch: raw.epoch,
        key: entry_key(&n.fake.board_id, raw.epoch, &raw.root),
    };
    build_frame(&env, &[0; 24], &raw.plain)
}

fn plain(kind: u8, at_ms: i64, based_on: &Head, payload: &[u8]) -> Vec<u8> {
    build_plain(&[0x42; 16], kind, at_ms, based_on, payload).expect("fits")
}

fn raw_cmd(n: &Node, net: &Net) -> Raw {
    Raw { ver: 1, suite: 1, epoch: n.m.epoch, root: n.fake.roots[&n.m.epoch], plain: plain(1, 5, &net.head(), b"x") }
}

/// The current devices and recovery key of `m` as wrap recipients.
fn recipients_of(m: &FakeMembership) -> BTreeMap<Dev, Hash32> {
    let mut r: BTreeMap<Dev, Hash32> = m.devices.iter().filter(|d| d.current).map(|d| (d.dev, d.hpke_pk)).collect();
    if let Some(x) = m.current_recovery() {
        r.insert(x.dev_r, x.hpke_pk);
    }
    r
}

fn rotation_item(m: &FakeMembership, epoch: u32, root: &[u8; 32], recipients: &BTreeMap<Dev, Hash32>) -> Item {
    Item::Rotated {
        epoch,
        commit: commitment(&m.board_id, epoch, root),
        wraps: recipients.iter().map(|(id, pk)| wrap(id, pk, epoch, root)).collect(),
        purge_before: 0,
    }
}

fn cmd(n: &mut Node, net: &Net, at_ms: i64) -> Vec<u8> {
    n.seal(net, at_ms, Body::Cmd(vec![1, 0x2A])).frame
}

fn beat_info(rules: Vec<u32>) -> BeatInfo {
    BeatInfo { rules, envelopes: vec![(1, 1)], hash_versions: vec![1], state: None, acks: Vec::new() }
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex")).collect()
}

/// SHA-256 straight from `sha2`, independent of the module's helper.
fn sha(parts: &[&[u8]]) -> Hash32 {
    let mut d = Sha256::new();
    for p in parts {
        d.update(p);
    }
    d.finalize().into()
}

// ---------- primitives and layouts

#[test]
fn padding_matches_the_examples_of_seal_10_2() {
    for (u, p) in [(69, 512), (600, 1024), (8192, 8192), (8193, 8704), (20000, 20480), (258_048, 258_048), (258_049, 262_144)]
    {
        assert_eq!(pad_len(u), p, "P({u})");
    }
}

#[test]
fn draws_are_consecutive_big_endian_outputs() {
    let mut a = SplitMix64 { state: 9 };
    let mut b = SplitMix64 { state: 9 };
    let x = draw::<24>(&mut a);
    let mut want = Vec::new();
    for _ in 0..3 {
        want.extend_from_slice(&b.next().to_be_bytes());
    }
    assert_eq!(x.to_vec(), want);
    // The last output truncated: 5 bytes take one output.
    let y = draw::<5>(&mut a);
    assert_eq!(y.to_vec(), b.next().to_be_bytes()[..5].to_vec());
    assert_eq!(a.state, b.state);
}

#[test]
fn crockford_and_key_fingerprints() {
    assert_eq!(crockford(&[0xFF; 5]), "ZZZZZZZZ");
    assert_eq!(crockford(&[0; 10]), "0000000000000000");
    assert_eq!(crockford(&[0x08]), "10");
    let fp = key_fp(&[1; 32], &[2; 32]);
    let x = sha(&[b"taskdeck/v1/key-fp\0", &[1; 32], &[2; 32]]);
    let want = crockford(&x[..10]);
    assert_eq!(fp, format!("{}-{}-{}-{}", &want[0..4], &want[4..8], &want[8..12], &want[12..16]));
    assert_eq!(fp.len(), 19);
}

#[test]
fn fake_keys_signatures_wraps_and_aead_follow_the_table() {
    let sk = [3u8; 32];
    let (s, p) = public_keys(&sk);
    assert_eq!(s, sha(&[b"FAKE-PK", &sk]));
    assert_eq!(p, sha(&[b"FAKE-HPKE", &sk]));
    let sig = sign(&s, b"m");
    assert_eq!(sig[..32], sha(&[b"FAKE-SIG", &s, b"m"]));
    assert_eq!(sig[32..], sha(&[b"FAKE-SIG2", &s, b"m"]));
    assert!(verify(&s, b"m", &sig));
    assert!(!verify(&p, b"m", &sig));

    let board_id = [0x0B; 16];
    let root = [0x5A; 32];
    let w = wrap(&[0x10; 16], &p, 2, &root);
    let mask = sha(&[b"FAKE-WRAP", &p, &[0, 0, 0, 2]]);
    let tag = sha(&[b"FAKE-WRAPTAG", &root, &[0, 0, 0, 2]]);
    assert_eq!(w[..16], [0x10; 16]);
    assert_eq!(w[16..48], [0; 32]);
    let xored: Vec<u8> = root.iter().zip(&mask).map(|(a, b)| a ^ b).collect();
    assert_eq!(w[48..80].to_vec(), xored);
    assert_eq!(w[80..], tag[..16]);
    let commit = commitment(&board_id, 2, &root);
    assert_eq!(commit, sha(&[b"FAKE-EPOCH", &board_id, &[0, 0, 0, 2], &root]));
    assert_eq!(unwrap(&w, &p, 2, &commit, &board_id), Some(root));
    assert_eq!(unwrap(&w, &s, 2, &commit, &board_id), None);
    assert_eq!(unwrap(&w, &p, 2, &[0; 32], &board_id), None);

    let k = entry_key(&board_id, 1, &root);
    assert_eq!(k, sha(&[b"FAKE-KEY", &board_id, &[0, 0, 0, 1], &root]));
    assert_eq!(snap_key(&board_id, 1, &root), sha(&[b"FAKE-SNAPKEY", &board_id, &[0, 0, 0, 1], &root]));
    let c = aead_seal(&k, b"ad", b"plain");
    assert_eq!(c[..5], *b"plain");
    assert_eq!(c[5..], sha(&[b"FAKE-AEAD", &k, b"ad", b"plain"])[..16]);
    assert_eq!(aead_open(&k, b"ad", &c), Some(&b"plain"[..]));
    assert_eq!(aead_open(&k, b"other", &c), None);
    assert_eq!(chain_zero_of(&board_id), sha(&[b"taskdeck/v1/chain\0", &board_id]));
}

#[test]
fn payload_encodings_have_the_fixed_layouts() {
    let beat = BeatInfo {
        rules: vec![1, 2],
        envelopes: vec![(1, 1)],
        hash_versions: vec![1],
        state: Some((64, [0xAB; 32], 1)),
        acks: vec![(70, Verdict::Ack), (71, Verdict::Mismatch)],
    };
    let want = format!(
        "02000000010000000201010101000000010100000000000000{}{}00000001020000000000000046010000000000000047 02",
        "40",
        "ab".repeat(32)
    )
    .replace(' ', "");
    let enc = encode_beat(&beat).expect("encodes");
    assert_eq!(hex(&enc), want);
    let back = parse_beat(&enc, 64, 72).expect("parses");
    assert_eq!(encode_beat(&back), Some(enc.clone()));
    // §11.4: `state.seq` at most `based_on_seq`, acks below the beat's own seq.
    assert!(parse_beat(&enc, 63, 72).is_none());
    assert!(parse_beat(&enc, 64, 71).is_none());

    let snap = SnapInfo {
        seq: 64,
        hash: [0x11; 32],
        hash_version: 1,
        blob: [0x22; 16],
        sha256: [0x33; 32],
        size: 1_048_577,
        parts: 2,
        inflated: 5_000,
        epoch: 3,
    };
    let want = format!(
        "0000000000000040{}00000001{}{}00000000001000010000000200000000000013880000 0003",
        "11".repeat(32),
        "22".repeat(16),
        "33".repeat(32)
    )
    .replace(' ', "");
    let enc = encode_snap(&snap);
    assert_eq!(enc.len(), 116);
    assert_eq!(hex(&enc), want);
    assert!(parse_snap(&enc, 64, 3).is_some());
    assert!(parse_snap(&enc, 64, 2).is_none());
    assert!(parse_snap(&enc, 63, 3).is_none());

    let cases: Vec<(Item, String)> = vec![
        (
            Item::Fence { host: "relay.example".into(), door: [1, 2, 3, 4, 5], home_gen: 2 },
            "070d72656c61792e6578616d706c65010203040500000002".into(),
        ),
        (
            Item::ResetTo { seq: 5, state_hash: [0x44; 32], hash_version: 1 },
            format!("080000000000000005{}00000001", "44".repeat(32)),
        ),
        (Item::Skip { seq: 3, frame_hash: [0x55; 32] }, format!("090000000000000003{}", "55".repeat(32))),
        (Item::Upgrade { rules: 2, mode: 2 }, "0a0000000202".into()),
        (Item::Revoked([0x66; 16]), format!("03{}", "66".repeat(16))),
        (Item::Reinstated([0x66; 16]), format!("04{}", "66".repeat(16))),
        (Item::Recovery { mode: 4, keys: None }, "0604".into()),
        (
            Item::Recovery { mode: 2, keys: Some(([0x01; 32], [0x02; 32])) },
            format!("0602{}{}", "01".repeat(32), "02".repeat(32)),
        ),
        (
            Item::DeviceAdded {
                dev: [0x0D; 16],
                sign_pk: [0x0E; 32],
                hpke_pk: [0x0F; 32],
                name: "beta".into(),
                transcript: [0x10; 32],
                bootstrap_seq: 9,
            },
            format!(
                "02{}{}{}0462657461{}0000000000000009",
                "0d".repeat(16),
                "0e".repeat(32),
                "0f".repeat(32),
                "10".repeat(32)
            ),
        ),
        (
            Item::Rotated { epoch: 2, commit: [0x77; 32], wraps: vec![[0x01; 96], [0x02; 96]], purge_before: 0 },
            format!("0500000002{}02{}{}0000000000000000", "77".repeat(32), "01".repeat(96), "02".repeat(96)),
        ),
    ];
    for (item, want) in cases {
        let bytes = encode_items(std::slice::from_ref(&item)).expect("encodes");
        assert_eq!(hex(&bytes), format!("01{want}"));
        let back = parse_items(&bytes).expect("parses");
        assert_eq!(encode_items(&back), Some(bytes));
    }
    // Strict: no trailing bytes, counts from 1 to 16.
    assert!(parse_items(&unhex("0a000000020200")).is_none());
    assert!(parse_items(&unhex("00")).is_none());
    // Schema constraints (§12): a host of §5.4, rules above 1.
    assert!(parse_items(&unhex("01070d52454c41592e6578616d706c65010203040500000002")).is_none());
    assert!(parse_items(&unhex("010a0000000101")).is_none());
}

#[test]
fn hosts_and_names_follow_seal_5_4_and_5_5() {
    for good in ["relay.example", "a", "a-b.c1", "relay.example:8443", "x.y:1"] {
        assert!(host_ok(good), "{good}");
    }
    for bad in ["", "Relay.example", "-a.b", "a-.b", "a..b", "a.b:443", "a.b:0", "a.b:080", "a.b:65536", "a.b:", "a_b"] {
        assert!(!host_ok(bad), "{bad}");
    }
    assert!(name_ok("phone"));
    assert!(!name_ok(""));
    assert!(!name_ok(&"x".repeat(65)));
    assert!(!name_ok("a\u{7}b"));
    assert!(!name_ok("a\u{85}b"));
}

// ---------- the genesis helper

#[test]
fn the_genesis_helper_draws_in_order_and_builds_seq_1() {
    let (fake, frame, board) = FakeSealer::create(SEED1, "alpha");
    assert_eq!(hex(&board), "544f5931000000010000000000000000");
    assert_eq!(board, toy_board());

    // The draws of §6.3, in order: salt, recovery key, device key, dev, R_1, entry id.
    let mut g = SplitMix64 { state: SEED1 };
    let salt = draw::<16>(&mut g);
    let rsk = draw::<32>(&mut g);
    let sk = draw::<32>(&mut g);
    let dev = draw::<16>(&mut g);
    let root = draw::<32>(&mut g);
    let entry_id = draw::<16>(&mut g);
    let _wraps = draw::<64>(&mut g);
    let nonce = draw::<24>(&mut g);
    let (rsign_pk, rhpke_pk) = public_keys(&rsk);
    let (sign_pk, hpke_pk) = public_keys(&sk);
    let board_id = first16(&sha(&[b"FAKE-BOARD", &rsign_pk, &salt]));
    let dev_r = first16(&sha(&[b"FAKE-REC", &board_id, &rsign_pk]));
    assert_eq!(fake.dev, dev);
    assert_eq!(fake.board_id, board_id);
    assert_eq!(fake.rng.state, g.state);

    // §8.1 and §9.1: the header and the body prefix.
    assert_eq!(frame.len(), 1182);
    assert_eq!(&frame[..4], b"TDF1");
    assert_eq!(frame[4..20], board_id);
    assert_eq!(frame[20..36], dev_r);
    assert_eq!(hex(&frame[36..48]), "00000000000000010000042e");
    assert_eq!(hex(&frame[48..54]), "010100000001");
    assert_eq!(frame[54..78], nonce);

    // The plaintext (§10.1) and the Genesis item (§6.3 type 1).
    let key = entry_key(&board_id, 1, &root);
    let ad = entry_ad(&frame[..48], 1, 1, 1);
    let padded = aead_open(&key, &ad, &frame[78..frame.len() - 64]).expect("opens under K_entry,1");
    let p = parse_plain(padded).expect("unpads");
    assert_eq!(p.entry_id, entry_id);
    assert_eq!((p.kind, p.at_ms, p.based_on_seq), (2, 0, 0));
    assert_eq!(p.based_on_chain, chain_zero_of(&board_id));
    assert_eq!(p.payload.len(), 496);
    let c = parse_ctl(p.payload).expect("parses");
    let [Item::Genesis(gi)] = c.items.as_slice() else { panic!("one Genesis item") };
    assert_eq!((gi.salt, gi.rsign_pk, gi.rhpke_pk), (salt, rsign_pk, rhpke_pk));
    assert_eq!((gi.dev, gi.sign_pk, gi.hpke_pk, gi.name.as_str()), (dev, sign_pk, hpke_pk, "alpha"));
    assert_eq!(gi.commit, commitment(&board_id, 1, &root));
    let mut ids = [dev, dev_r];
    ids.sort_unstable();
    assert_eq!(first16_of(&gi.wraps[0]), ids[0]);
    assert_eq!(first16_of(&gi.wraps[1]), ids[1]);
    assert!(verify(&rsign_pk, &ctl_inner(&board_id, &dev_r, 1, &entry_id, 0, c.items_bytes), c.sig));
    assert!(verify(&rsign_pk, &frame_signed(&frame[..frame.len() - 64]), &frame[frame.len() - 64..]));

    // Classifying seq 1 gives M_1 of §13.3, and the item's state hash is engine.md §18.3's.
    let mut net = Net::new(&board_id);
    let mut n1 = Node::new(fake);
    let r = submit(&mut net, &mut [&mut n1], frame.clone());
    assert_eq!(outcome(&r[0]), ACCEPT);
    assert_eq!((r[0].author, r[0].ctr), (dev_r, 1));
    let m1 = n1.m.to_bytes();
    let z8 = "0000000000000000";
    let one8 = "0000000000000001";
    let mut want = Vec::new();
    want.extend_from_slice(b"TDM1");
    want.extend_from_slice(&unhex("00000001"));
    want.extend_from_slice(&board_id);
    want.extend_from_slice(&salt);
    // rules, epoch, epoch_seq, epoch_clock_ms, clock_ms, home_gen, home_seq.
    want.extend_from_slice(&unhex(&format!("0000000100000001{one8}{z8}{z8}00000001{z8}")));
    // home_host (empty), home_door, purge_seq, next_device_no.
    want.extend_from_slice(&unhex(&format!("000000000000{z8}0002")));
    // One commitment.
    want.extend_from_slice(&unhex(&format!("0000000100000001{one8}")));
    want.extend_from_slice(&gi.commit);
    // One recovery record.
    want.extend_from_slice(&unhex("0001"));
    want.extend_from_slice(&dev_r);
    want.extend_from_slice(&rsign_pk);
    want.extend_from_slice(&rhpke_pk);
    want.extend_from_slice(&unhex(&format!("{one8}{z8}{one8}")));
    // One device record.
    want.extend_from_slice(&unhex("0001"));
    want.extend_from_slice(&dev);
    want.extend_from_slice(&sign_pk);
    want.extend_from_slice(&hpke_pk);
    want.extend_from_slice(&unhex(&format!("000101{one8}")));
    want.extend_from_slice(&dev_r);
    want.extend_from_slice(&unhex(&format!("{z8}{one8}")));
    want.extend_from_slice(&dev_r);
    want.extend_from_slice(&unhex(&format!("{z8}{z8}")));
    want.extend_from_slice(&unhex("01000000010101010100000001"));
    want.extend_from_slice(&unhex("05616c706861"));
    // No pending recovery, override or reset; no skips; ctl_seqs = [1].
    want.extend_from_slice(&unhex(&format!("000000{}{}{one8}", "00000000", "00000001")));
    assert_eq!(hex(&m1), hex(&want));
    assert_eq!(ok(n1.fake.membership_from_bytes(&m1)).to_bytes(), m1);
    let Some(Opened { items, .. }) = &r[0].opened else { panic!("opened") };
    let [CtlItem::Genesis { hash_version: 1, state_hash }] = items.as_slice() else { panic!("Genesis") };
    let board_hash = sha(&[&board]);
    assert_eq!(*state_hash, sha(&[b"taskdeck/v1/state\0", &[0, 0, 0, 1], &board_hash, &sha(&[&m1])]));

    // The view (engine.md §6.2).
    let v = n1.fake.view(&n1.m);
    assert_eq!((v.rules, v.epoch, v.epoch_seq, v.home_gen, v.ctl_seqs.clone()), (1, 1, 1, 1, vec![1]));
    assert_eq!(v.devices.len(), 1);
    assert_eq!((v.devices[0].device_no, v.devices[0].name.as_str()), (1, "alpha"));
    assert_eq!(v.recovery_dev, dev_r);
    assert_eq!(v.recovery_fp, key_fp(&rsign_pk, &rhpke_pk));

    // Equal seeds give equal bytes; another seed gives another board.
    let (again, frame2, _) = FakeSealer::create(SEED1, "alpha");
    assert_eq!(frame2, frame);
    assert_eq!(*again.export_identity(), *n1.fake.export_identity());
    let (_, other, _) = FakeSealer::create(SEED2, "alpha");
    assert_ne!(other[4..20], frame[4..20]);
}

#[test]
fn seq_1_refusals_and_the_anchor() {
    let (mut fake, frame, _) = FakeSealer::create(SEED1, "alpha");
    let a = fake.anchor.as_ref().expect("anchor");
    let (rsign_pk, root) = (a.rsign_pk, a.root);
    let board_id = fake.board_id;
    let dev_r = dev_r_of(&board_id, &rsign_pk);
    let f = parse_frame(&frame).expect("frame");
    let ad = entry_ad(f.header, 1, 1, 1);
    let padded = aead_open(&entry_key(&board_id, 1, &root), &ad, &f.body[BODY_PREFIX..]).expect("opens");
    let p = parse_plain(padded).expect("plain");
    let items = parse_ctl(p.payload).expect("ctl").items;
    let entry_id = p.entry_id;
    let head0 = Head { seq: 0, chain: chain_zero_of(&board_id) };

    let mut wrong_commit = items.clone();
    if let Item::Genesis(g) = &mut wrong_commit[0] {
        g.commit = [0; 32];
    }
    let bad = ok(fake.ctl_frame(&dev_r, &rsign_pk, 1, 1, &root, &entry_id, 0, &head0, &wrong_commit));
    let fence = Item::Fence { host: "relay.example".into(), door: [0; 5], home_gen: 2 };
    let two = ok(fake.ctl_frame(&dev_r, &rsign_pk, 1, 1, &root, &entry_id, 0, &head0, &[items[0].clone(), fence]));
    let at_2 = ok(fake.ctl_frame(&dev_r, &rsign_pk, 1, 2, &root, &entry_id, 0, &head0, &items));
    let net = Net::new(&board_id);
    let mut n = Node::new(fake);
    assert_eq!(outcome(&n.classify(&net, 1, &bad)), refused(code::CTL_GENESIS));
    assert_eq!(outcome(&n.classify(&net, 1, &two)), refused(code::CTL_SHAPE));
    assert_eq!(outcome(&n.classify(&net, 1, &at_2)), refused(code::BAD_EPOCH));

    // A paired device has no anchor: seq 1 has no author it knows.
    let (_net2, _n1, mut n2) = two_devices();
    n2.m = FakeMembership::default();
    let net1 = Net::new(&n2.fake.board_id);
    let (_, frame1, _) = FakeSealer::create(SEED1, "alpha");
    assert_eq!(outcome(&n2.classify(&net1, 1, &frame1)), ("skip", Some(code::UNKNOWN_AUTHOR)));
}

// ---------- seal, classify and open each kind

#[test]
fn each_kind_seals_classifies_and_opens() {
    let (mut net, mut n1, mut n2) = two_devices();

    // cmd
    let sealed = n1.seal(&net, 2_000, Body::Cmd(vec![9, 8, 7]));
    assert_eq!(sealed.epoch, 1);
    assert_eq!(ok(n1.fake.open_own(&n1.m, &sealed.frame)).len(), 16);
    let entry_id = ok(n1.fake.open_own(&n1.m, &sealed.frame));
    assert_eq!(err_code(n2.fake.open_own(&n2.m, &sealed.frame)), code::UNKNOWN_AUTHOR);
    let rs = submit(&mut net, &mut [&mut n1, &mut n2], sealed.frame);
    assert_all(&rs, ACCEPT);
    for r in &rs {
        let o = r.opened.as_ref().expect("opened");
        assert!(matches!(o.kind, Kind::Cmd));
        assert_eq!((o.entry_id, o.payload.clone(), o.at_ms), (entry_id, vec![9, 8, 7], 2_000));
    }
    assert!(!rs[0].own_dev_foreign);

    // beat
    let frame = n2.seal(&net, 3_000, Body::Beat(beat_info(vec![1, 2]))).frame;
    let rs = submit(&mut net, &mut [&mut n1, &mut n2], frame);
    assert_all(&rs, ACCEPT);
    let o = rs[0].opened.as_ref().expect("opened");
    assert!(matches!(o.kind, Kind::Beat));
    assert_eq!(o.beat.as_ref().map(|b| b.rules.clone()), Some(vec![1, 2]));
    let d2 = n1.m.device(&n2.dev()).expect("device 2");
    assert_eq!((d2.rules.clone(), d2.beat_ms, d2.device_no), (vec![1, 2], 3_000, 2));
    assert_eq!(n1.m.clock_ms, 3_000);

    // ctl: a Fence, then its move code.
    let fence = CtlIntent::Fence { host: "relay.example".into(), door: [1, 2, 3, 4, 5], home_gen: 2 };
    let sealed = n1.seal(&net, 4_000, Body::Ctl(fence));
    let fence_hash = sealed.frame_hash;
    let rs = submit(&mut net, &mut [&mut n1, &mut n2], sealed.frame);
    assert_all(&rs, ACCEPT);
    let fence_seq = net.head().seq;
    assert_eq!((n2.m.home_gen, n2.m.home_seq, n2.m.home_host.as_str()), (2, fence_seq, "relay.example"));
    let mc = n1.fake.move_code(fence_seq, &fence_hash);
    assert_eq!(mc.len(), 10);
    assert_eq!(mc, hex(&sha(&[b"taskdeck/v1/move\0", &n1.fake.board_id, &fence_seq.to_be_bytes(), &fence_hash])[..5]));
    assert!(n2.fake.check_move_code(fence_seq, &fence_hash, &mc).is_ok());
    let typed = format!("{}-{} ", mc[..5].to_uppercase(), &mc[5..]);
    assert!(n2.fake.check_move_code(fence_seq, &fence_hash, &typed).is_ok());
    assert_eq!(err_code(n2.fake.check_move_code(fence_seq, &[0; 32], &mc)), code::MOVE_CODE);
    assert_eq!(err_code(n2.fake.check_move_code(fence_seq, &fence_hash, "00000")), code::MOVE_CODE);
    assert_eq!(err_code(n1.fake.seal(&n1.m, 99, &PlainEntry {
        entry_id: [0; 16],
        at_ms: 0,
        based_on: net.head(),
        body: Body::Ctl(CtlIntent::Fence { host: "Bad_Host".into(), door: [0; 5], home_gen: 3 }),
    })), code::HOST_FORMAT);

    // The fingerprint (§15.3 with §6.3's text).
    let head = net.head();
    let fp = n1.fake.sync_fingerprint(&head);
    let x = sha(&[b"taskdeck/v1/fingerprint\0", &n1.fake.board_id, &head.seq.to_be_bytes(), &head.chain]);
    assert_eq!(fp, format!("{} {}", head.seq, hex(&x[..6])));
    assert_eq!(fp, n2.fake.sync_fingerprint(&head));

    // snap: fill the log to seq 64, snapshot it, announce it.
    while net.head().seq < 64 {
        let f = cmd(&mut n1, &net, 5_000);
        assert_all(&submit(&mut net, &mut [&mut n1, &mut n2], f), ACCEPT);
    }
    let material = material_at(&n1, &net);
    let blob = ok(n1.fake.seal_snapshot(&n1.m, &material));
    let info = SnapInfo {
        seq: 64,
        hash: material.state_hash,
        hash_version: 1,
        blob: blob.id,
        sha256: blob.sha256,
        size: blob.bytes.len() as u64,
        parts: blob.parts,
        inflated: blob.inflated,
        epoch: blob.epoch,
    };
    let frame = n1.seal(&net, 6_000, Body::Snap(info_copy(&info))).frame;
    let rs = submit(&mut net, &mut [&mut n1, &mut n2], frame);
    assert_all(&rs, ACCEPT);
    let got = rs[1].opened.as_ref().and_then(|o| o.snap.as_ref()).expect("snap");
    assert_eq!(encode_snap(got), encode_snap(&info));
    let (opened, epoch, inflated) = ok(n2.fake.open_snapshot(&[&blob.bytes], &info, &n1.dev(), true));
    assert_eq!((epoch, inflated), (1, blob.inflated));
    assert_eq!((opened.seq, opened.chain, opened.state_hash), (64, material.chain, material.state_hash));
    assert_eq!((opened.board, opened.membership), (material.board.clone(), material.membership.clone()));
    assert!(n2.fake.open_snapshot(&[&blob.bytes], &info, &n1.dev(), false).is_ok());
}

fn info_copy(i: &SnapInfo) -> SnapInfo {
    parse_snap(&encode_snap(i), u64::MAX, i.epoch).expect("valid")
}

// ---------- determinism (ENG-FAKE-002)

/// A scripted run: every frame, a snapshot blob and the identity exports.
fn script(seed1: u64, seed2: u64) -> Vec<Vec<u8>> {
    let (fake, frame, _) = FakeSealer::create(seed1, "alpha");
    let mut net = Net::new(&fake.board_id);
    let mut n1 = Node::new(fake);
    submit(&mut net, &mut [&mut n1], frame);
    let mut n2 = add_device(&mut net, &mut n1, &mut [], seed2, 10);
    let f = cmd(&mut n2, &net, 20);
    submit(&mut net, &mut [&mut n1, &mut n2], f);
    let f = n1.seal(&net, 30, Body::Ctl(CtlIntent::Rotate { revoke: vec![], reinstate: vec![], purge_before: None })).frame;
    submit(&mut net, &mut [&mut n1, &mut n2], f);
    let f = n2.seal(&net, 40, Body::Beat(beat_info(vec![1]))).frame;
    submit(&mut net, &mut [&mut n1, &mut n2], f);
    let blob = ok(n1.fake.seal_snapshot(&n1.m, &material_at(&n1, &net)));
    let mut out = net.frames.clone();
    out.push(blob.bytes);
    out.push(n1.fake.export_identity().to_vec());
    out.push(n2.fake.export_identity().to_vec());
    out
}

#[test]
fn equal_seeds_and_equal_calls_give_equal_bytes() {
    let a = script(SEED1, SEED2);
    assert_eq!(a, script(SEED1, SEED2));
    assert_ne!(a, script(SEED1, SEED3));
    assert_ne!(a[0], script(SEED3, SEED2)[0]);
    // The sealed frames are padded to 512-byte buckets: 670, 1182, ... bytes.
    assert!(a[..a.len() - 3].iter().all(|f| (f.len() - 158) % 512 == 0));
}

// ---------- membership changes

#[test]
fn add_revoke_with_rotation_and_reinstate_with_missed_wraps() {
    let (mut net, mut n1, mut n2) = two_devices();
    let v = n1.fake.view(&n1.m);
    assert_eq!(v.devices.len(), 2);
    let d2 = v.devices.iter().find(|d| d.dev == n2.dev()).expect("added");
    assert_eq!((d2.device_no, d2.added_by, d2.bootstrap_seq, d2.added_seq), (2, n1.dev(), 1, 2));
    assert!(matches!(d2.status, DeviceStatus::Current));
    assert_eq!(n2.fake.view(&n2.m).devices.len(), 2);

    // A rotation with no revocation: both learn R_2; the author's pending root goes.
    let sealed = n1.seal(&net, 2_000, Body::Ctl(CtlIntent::Rotate { revoke: vec![], reinstate: vec![], purge_before: None }));
    assert!(sealed.keys_changed);
    assert_eq!(n1.fake.pending.len(), 1);
    let rs = submit(&mut net, &mut [&mut n1, &mut n2], sealed.frame);
    assert_all(&rs, ACCEPT);
    assert!(rs[0].keys_changed && rs[1].keys_changed);
    assert!(n1.fake.pending.is_empty());
    assert_eq!(n1.fake.roots.get(&2), n2.fake.roots.get(&2));
    assert_eq!(n2.m.epoch, 2);
    let f = cmd(&mut n2, &net, 2_100);
    assert_all(&submit(&mut net, &mut [&mut n1, &mut n2], f), ACCEPT);

    // Revoke device 2 with a rotation: it gets no wrap and no root.
    let before = n2.m.clone();
    let body = Body::Ctl(CtlIntent::Rotate { revoke: vec![n2.dev()], reinstate: vec![], purge_before: None });
    let f = n1.seal(&net, 3_000, body).frame;
    let rs = submit(&mut net, &mut [&mut n1, &mut n2], f);
    assert_all(&rs, ACCEPT);
    assert!(!rs[1].keys_changed && rs[1].halt.is_none());
    assert!(!n2.fake.roots.contains_key(&3));
    let d2 = n1.m.device(&n2.dev()).expect("device 2");
    assert!(!d2.current);
    assert_eq!(d2.status_by, n1.dev());
    // Its later frames are refused, and consume its counter.
    n2.ctr += 1;
    let entry = PlainEntry { entry_id: [5; 16], at_ms: 3_100, based_on: net.head(), body: Body::Cmd(vec![1]) };
    let late = ok(n2.fake.seal(&before, n2.ctr, &entry)).frame;
    let r = n1.classify(&net, net.next_seq(), &late);
    assert_eq!(outcome(&r), refused(code::REVOKED_AUTHOR));
    assert_eq!(r.membership.device(&n2.dev()).map(|d| d.last_ctr), Some(n2.ctr));
    n2.fake.sealed.remove(&n2.ctr);
    n2.ctr -= 1;
    // Without R_3 it cannot classify what follows.
    let f = cmd(&mut n1, &net, 3_200);
    let s = net.push(f);
    assert_eq!(outcome(&n1.deliver(&net, s)), ACCEPT);
    assert_eq!(err_code(n2.try_classify(&net, s, &net.frames[(s - 1) as usize].clone(), true)), code::MISSING_ROOT);
    // n2 catches up later through the missed wraps; it keeps M as of the revocation.

    // n1 rotates again (epoch 4, no wrap to device 2), then the recovery identity reinstates it (epoch 5).
    let f = n1.seal(&net, 4_000, Body::Ctl(CtlIntent::Rotate { revoke: vec![], reinstate: vec![], purge_before: None })).frame;
    assert_all(&submit(&mut net, &mut [&mut n1], f), ACCEPT);
    let mut rk = recovery_of(&n1);
    let body = Body::Ctl(CtlIntent::Rotate { revoke: vec![], reinstate: vec![n2.dev()], purge_before: None });
    let f = seal_as_recovery(&mut n1, &mut rk, &net, 5_000, body);
    assert_all(&submit(&mut net, &mut [&mut n1], f), ACCEPT);
    assert!(n1.m.is_current_device(&n2.dev()));
    assert_eq!(n1.m.epoch, 5);

    // The login answer's wraps of epochs 3 and 4 (ruling S24): 3 opens at once,
    // 4 when device 2 classifies the rotation that set it.
    let w3 = wrap(&n2.dev(), &n2.fake.hpke_pk, 3, &n1.fake.roots[&3]);
    let w4 = wrap(&n2.dev(), &n2.fake.hpke_pk, 4, &n1.fake.roots[&4]);
    assert_eq!(err_code(n2.fake.take_missed_wraps(&n2.m, 3, &[[0; 96], w4])), code::COMMITMENT);
    assert_eq!(n2.fake.take_missed_wraps(&n2.m, 3, &[w3, w4]).ok(), Some(true));
    assert!(n2.fake.roots.contains_key(&3) && !n2.fake.roots.contains_key(&4));
    for s in s..=net.head().seq {
        let r = n2.deliver(&net, s);
        assert_eq!(outcome(&r), ACCEPT, "seq {s}");
    }
    assert_eq!(n2.fake.roots.get(&4), n1.fake.roots.get(&4));
    assert_eq!(n2.fake.roots.get(&5), n1.fake.roots.get(&5));
    assert_eq!(n2.m.to_bytes(), n1.m.to_bytes());
    let f = cmd(&mut n2, &net, 5_100);
    assert_all(&submit(&mut net, &mut [&mut n1, &mut n2], f), ACCEPT);

    // forget_roots_below (a purge).
    assert!(n1.fake.forget_roots_below(5));
    assert!(!n1.fake.forget_roots_below(5));
    assert_eq!(n1.fake.roots.keys().copied().collect::<Vec<_>>(), vec![5]);
}

#[test]
fn a_missed_wrap_that_does_not_open_halts_on_the_rotation() {
    let (mut net, mut n1, mut n2) = two_devices();
    let body = Body::Ctl(CtlIntent::Rotate { revoke: vec![n2.dev()], reinstate: vec![], purge_before: None });
    let f = n1.seal(&net, 1_000, body).frame;
    assert_all(&submit(&mut net, &mut [&mut n1, &mut n2], f), ACCEPT);
    let f = n1.seal(&net, 2_000, Body::Ctl(CtlIntent::Rotate { revoke: vec![], reinstate: vec![], purge_before: None })).frame;
    let s = net.push(f);
    assert_eq!(outcome(&n1.deliver(&net, s)), ACCEPT);
    let w2 = wrap(&n2.dev(), &n2.fake.hpke_pk, 2, &n1.fake.roots[&2]);
    let bad3 = wrap(&n2.dev(), &n2.fake.hpke_pk, 3, &[0xAA; 32]);
    assert_eq!(n2.fake.take_missed_wraps(&n2.m, 2, &[w2, bad3]).ok(), Some(true));
    let r = n2.deliver(&net, s);
    assert_eq!(outcome(&r), ("accept", Some(code::COMMITMENT)));
    assert!(matches!(r.halt, Some(Halt::Commitment)));
}

#[test]
fn an_own_wrap_that_does_not_match_the_commitment_halts_only_its_recipient() {
    let (mut net, mut n1, mut n2) = two_devices();
    let good = [0x61; 32];
    let recipients = recipients_of(&n1.m);
    let Item::Rotated { epoch, commit, mut wraps, purge_before } = rotation_item(&n1.m, 2, &good, &recipients) else {
        panic!("a rotation")
    };
    let at = wraps.iter().position(|w| first16_of(w) == n2.dev()).expect("a wrap to device 2");
    wraps[at] = wrap(&n2.dev(), &n2.fake.hpke_pk, 2, &[0x62; 32]);
    let f = raw_own(&mut n1, &net, 100, &[Item::Rotated { epoch, commit, wraps, purge_before }]);
    let rs = submit(&mut net, &mut [&mut n1, &mut n2], f);
    assert_eq!(outcome(&rs[0]), ACCEPT);
    assert!(rs[0].keys_changed);
    assert_eq!(n1.fake.roots.get(&2), Some(&good));
    assert_eq!(outcome(&rs[1]), ("accept", Some(code::COMMITMENT)));
    assert!(matches!(rs[1].halt, Some(Halt::Commitment)));
    assert!(!rs[1].keys_changed && !n2.fake.roots.contains_key(&2));
    assert_eq!(n2.m.to_bytes(), n1.m.to_bytes());
}

#[test]
fn recovery_key_changes_direct_propose_confirm_and_cancel() {
    let (mut net, mut n1, mut n2) = two_devices();
    let mut rk = recovery_of(&n1);
    let old_rk_dev = rk.dev;

    // direct, by the current recovery identity: a K-shape entry wrapping to the new key.
    let (s2, p2) = public_keys(&[0x21; 32]);
    let body = Body::Ctl(CtlIntent::RecoveryChange { mode: RecoveryMode::Direct, sign_pk: Some(s2), hpke_pk: Some(p2) });
    let f = seal_as_recovery(&mut n1, &mut rk, &net, 1_500, body);
    let rs = submit(&mut net, &mut [&mut n1, &mut n2], f);
    assert_all(&rs, ACCEPT);
    let s_direct = net.head().seq;
    let v = n2.fake.view(&n2.m);
    assert_eq!(v.recovery.len(), 2);
    assert_eq!(v.recovery[0].until_seq, s_direct);
    assert_eq!(v.recovery_dev, dev_r_of(&n2.fake.board_id, &s2));
    assert_eq!(v.recovery_fp, key_fp(&s2, &p2));
    assert_eq!(n2.m.epoch, 2);
    let Some(Opened { items, .. }) = &rs[1].opened else { panic!("opened") };
    assert!(matches!(items.as_slice(), [CtlItem::RecoveryKeyChange { mode: RecoveryMode::Direct, key_fp: Some(_) }, CtlItem::EpochRotated { epoch: 2, purge_before: None }]));
    // The retired key is refused from now on; the new one starts at counter 1.
    let f = seal_as_recovery(&mut n1, &mut rk, &net, 1_600, Body::Ctl(CtlIntent::Fence { host: "a.example".into(), door: [0; 5], home_gen: 2 }));
    assert_eq!(outcome(&n2.classify(&net, net.next_seq(), &f)), refused(code::REVOKED_AUTHOR));
    let mut rk2 = Rk { dev: dev_r_of(&n2.fake.board_id, &s2), sign_pk: s2, ctr: 0 };
    let f = seal_as_recovery(&mut n1, &mut rk2, &net, 1_700, Body::Ctl(CtlIntent::Fence { host: "a.example".into(), door: [0; 5], home_gen: 2 }));
    assert_all(&submit(&mut net, &mut [&mut n1, &mut n2], f), ACCEPT);
    assert_ne!(rk2.dev, old_rk_dev);
    // The recovery identity never authors a cmd.
    let f = seal_as_recovery(&mut n1, &mut rk2, &net, 1_800, Body::Cmd(vec![1]));
    assert_eq!(outcome(&n2.classify(&net, net.next_seq(), &f)), refused(code::NOT_AUTHORIZED));

    // propose by device 1; a second proposal; a confirm too early, by the proposer, then by device 2.
    let (s3, p3) = public_keys(&[0x31; 32]);
    let propose = |s, p| Body::Ctl(CtlIntent::RecoveryChange { mode: RecoveryMode::Propose, sign_pk: Some(s), hpke_pk: Some(p) });
    let confirm = |s, p| Body::Ctl(CtlIntent::RecoveryChange { mode: RecoveryMode::Confirm, sign_pk: Some(s), hpke_pk: Some(p) });
    let f = n1.seal(&net, 10 * DAY, propose(s3, p3)).frame;
    assert_all(&submit(&mut net, &mut [&mut n1, &mut n2], f), ACCEPT);
    let pending = n2.fake.view(&n2.m).pending_recovery.expect("pending");
    assert_eq!((pending.proposer, pending.start_ms, pending.key_fp), (n1.dev(), 10 * DAY, key_fp(&s3, &p3)));
    let f = n2.seal(&net, 10 * DAY, propose(s3, p3)).frame;
    assert_all(&submit(&mut net, &mut [&mut n1, &mut n2], f), refused(code::CTL_PENDING));
    let early = n2.seal(&net, 12 * DAY, confirm(s3, p3));
    assert!(early.keys_changed);
    let entry_id = ok(n2.fake.open_own(&n2.m, &early.frame));
    assert_all(&submit(&mut net, &mut [&mut n1, &mut n2], early.frame), refused(code::CTL_WINDOW));
    assert!(n2.fake.discard_pending(&entry_id));
    assert!(!n2.fake.discard_pending(&entry_id));
    let f = n1.seal(&net, 14 * DAY, confirm(s3, p3)).frame;
    assert_all(&submit(&mut net, &mut [&mut n1, &mut n2], f), refused(code::CTL_AUTHOR));
    let (s4, p4) = public_keys(&[0x41; 32]);
    let f = n2.seal(&net, 14 * DAY, confirm(s4, p4)).frame;
    assert_all(&submit(&mut net, &mut [&mut n1, &mut n2], f), refused(code::CTL_TARGET));
    let f = n2.seal(&net, 13 * DAY + 1, confirm(s3, p3)).frame;
    assert_all(&submit(&mut net, &mut [&mut n1, &mut n2], f), ACCEPT);
    let v = n1.fake.view(&n1.m);
    assert_eq!(v.recovery.len(), 3);
    assert_eq!(v.recovery_dev, dev_r_of(&n1.fake.board_id, &s3));
    assert!(v.pending_recovery.is_none());

    // cancel: without a proposal, then after one; keys already recorded.
    let cancel = || Body::Ctl(CtlIntent::RecoveryChange { mode: RecoveryMode::Cancel, sign_pk: None, hpke_pk: None });
    let f = n1.seal(&net, 15 * DAY, cancel()).frame;
    assert_all(&submit(&mut net, &mut [&mut n1, &mut n2], f), refused(code::CTL_NO_PENDING));
    let f = n1.seal(&net, 15 * DAY, propose(s3, p3)).frame;
    assert_all(&submit(&mut net, &mut [&mut n1, &mut n2], f), refused(code::CTL_DUPLICATE_KEY));
    let f = n1.seal(&net, 15 * DAY, propose(s4, p4)).frame;
    assert_all(&submit(&mut net, &mut [&mut n1, &mut n2], f), ACCEPT);
    let f = n2.seal(&net, 15 * DAY, cancel()).frame;
    assert_all(&submit(&mut net, &mut [&mut n1, &mut n2], f), ACCEPT);
    assert!(n1.m.pending_recovery.is_none());
    // Revoking the proposer clears its proposal (SEAL-MEM-025).
    let f = n1.seal(&net, 16 * DAY, propose(s4, p4)).frame;
    assert_all(&submit(&mut net, &mut [&mut n1, &mut n2], f), ACCEPT);
    let body = Body::Ctl(CtlIntent::Rotate { revoke: vec![n1.dev()], reinstate: vec![], purge_before: None });
    let f = n2.seal(&net, 16 * DAY, body).frame;
    assert_all(&submit(&mut net, &mut [&mut n1, &mut n2], f), ACCEPT);
    assert!(n2.m.pending_recovery.is_none());
    assert_eq!(err_code(n2.fake.seal(&n2.m, 99, &PlainEntry {
        entry_id: [0; 16],
        at_ms: 0,
        based_on: net.head(),
        body: Body::Ctl(CtlIntent::RecoveryChange { mode: RecoveryMode::Direct, sign_pk: None, hpke_pk: None }),
    })), code::BAD_PAYLOAD);
}

// ---------- refusals of the checks of §14.2, in order

#[test]
fn every_step_of_14_2_refuses_with_its_code() {
    let (net, mut n1, mut n2) = two_devices();
    let s = net.next_seq();
    let n1dev = n1.dev();
    let n1pk = n1.fake.sign_pk;
    let good = n1.seal(&net, 100, Body::Cmd(vec![1])).frame;
    n1.ctr -= 1;
    n1.own.clear();

    // Step 1.
    assert_eq!(outcome(&n2.classify(&net, s, &good[..669])), ("skip", Some(code::FRAME_FORMAT)));
    let mut bad = good.clone();
    bad[0] = b'X';
    assert_eq!(outcome(&n2.classify(&net, s, &bad)), ("skip", Some(code::FRAME_FORMAT)));
    let mut bad = good.clone();
    bad.push(0);
    assert_eq!(outcome(&n2.classify(&net, s, &bad)), ("skip", Some(code::FRAME_FORMAT)));
    // Step 2.
    let (_, other, _) = FakeSealer::create(SEED_OTHER, "other");
    assert_eq!(outcome(&n2.classify(&net, s, &other)), ("skip", Some(code::WRONG_BOARD)));
    // Step 3.
    let (stranger, stranger_pk, _) = FakeSealer::join_keys(SEED_OTHER);
    let f = raw_frame(&n2, &stranger, &stranger_pk, 1, &raw_cmd(&n2, &net));
    assert_eq!(outcome(&n2.classify(&net, s, &f)), ("skip", Some(code::UNKNOWN_AUTHOR)));
    // Step 4.
    let mut bad = good.clone();
    let last = bad.len() - 1;
    bad[last] ^= 1;
    assert_eq!(outcome(&n2.classify(&net, s, &bad)), ("skip", Some(code::BAD_SIGNATURE)));
    // Step 5.
    let f = raw_frame(&n2, &n1dev, &n1pk, 3, &raw_cmd(&n2, &net));
    let r = n2.classify(&net, s, &f);
    assert_eq!(outcome(&r), ("tamper", Some(code::COUNTER_GAP)));
    assert_eq!(r.membership.to_bytes(), n2.m.to_bytes());
    let f = raw_frame(&n2, &n1dev, &n1pk, 1, &raw_cmd(&n2, &net));
    assert_eq!(outcome(&n2.classify(&net, s, &f)), ("tamper", Some(code::COUNTER_REPLAY)));
    // Step 6: a frame under this device's own dev that it has no record of.
    let r = n1.classify(&net, s, &good);
    assert_eq!(outcome(&r), ACCEPT);
    assert!(r.own_dev_foreign);
    n1.own.insert(2, h(&[&good]));
    assert!(!n1.classify(&net, s, &good).own_dev_foreign);
    assert!(!n2.classify(&net, s, &good).own_dev_foreign);
    // Step 8.
    let mut raw = raw_cmd(&n2, &net);
    raw.ver = 2;
    let r = n2.classify(&net, s, &raw_frame(&n2, &n1dev, &n1pk, 2, &raw));
    assert_eq!(outcome(&r), refused(code::BAD_VERSION));
    assert_eq!(r.membership.device(&n1dev).map(|d| d.last_ctr), Some(2));
    // Step 9.
    let mut raw = raw_cmd(&n2, &net);
    raw.epoch = 2;
    assert_eq!(outcome(&n2.classify(&net, s, &raw_frame(&n2, &n1dev, &n1pk, 2, &raw))), refused(code::BAD_EPOCH));
    // Step 10.
    let mut raw = raw_cmd(&n2, &net);
    raw.root = [0; 32];
    assert_eq!(outcome(&n2.classify(&net, s, &raw_frame(&n2, &n1dev, &n1pk, 2, &raw))), refused(code::AEAD));
    // Step 11: non-zero padding, a kind of 5, an `at_ms` out of range.
    let mut raw = raw_cmd(&n2, &net);
    let end = raw.plain.len() - 1;
    raw.plain[end] = 1;
    assert_eq!(outcome(&n2.classify(&net, s, &raw_frame(&n2, &n1dev, &n1pk, 2, &raw))), refused(code::BAD_PLAINTEXT));
    let mut raw = raw_cmd(&n2, &net);
    raw.plain = plain(5, 5, &net.head(), b"x");
    assert_eq!(outcome(&n2.classify(&net, s, &raw_frame(&n2, &n1dev, &n1pk, 2, &raw))), refused(code::BAD_PLAINTEXT));
    let mut raw = raw_cmd(&n2, &net);
    raw.plain = plain(1, -1, &net.head(), b"x");
    assert_eq!(outcome(&n2.classify(&net, s, &raw_frame(&n2, &n1dev, &n1pk, 2, &raw))), refused(code::BAD_PLAINTEXT));
    // Step 12.
    let mut raw = raw_cmd(&n2, &net);
    raw.plain = plain(1, 5, &Head { seq: s, chain: [0; 32] }, b"x");
    let r = n2.classify(&net, s, &raw_frame(&n2, &n1dev, &n1pk, 2, &raw));
    assert_eq!(outcome(&r), ("tamper", Some(code::BASED_ON_FUTURE)));
    assert_eq!(r.membership.device(&n1dev).map(|d| d.last_ctr), Some(1));
    raw.plain = plain(1, 5, &Head { seq: 1, chain: [0; 32] }, b"x");
    let f = raw_frame(&n2, &n1dev, &n1pk, 2, &raw);
    assert_eq!(outcome(&n1.classify(&net, s, &f)), ("tamper", Some(code::BASED_ON_FORK)));
    raw.plain = plain(1, 5, &Head { seq: 0, chain: [1; 32] }, b"x");
    let f0 = raw_frame(&n2, &n1dev, &n1pk, 2, &raw);
    assert_eq!(outcome(&n1.classify(&net, s, &f0)), ("tamper", Some(code::BASED_ON_FORK)));
    // Device 2 holds the chain only from its bootstrap seq (1): seq 0's check still runs, 1's runs.
    n2.retained_from = 2;
    let r = n2.classify(&net, s, &f);
    assert_eq!(outcome(&r), ACCEPT);
    assert!(r.based_on_unchecked);
    n2.retained_from = 1;
    // Step 15.
    let mut raw = raw_cmd(&n2, &net);
    raw.plain = plain(1, 5, &net.head(), b"");
    assert_eq!(outcome(&n2.classify(&net, s, &raw_frame(&n2, &n1dev, &n1pk, 2, &raw))), refused(code::BAD_PAYLOAD));
    let beat = encode_beat(&beat_info(vec![2, 1])).expect("encodes");
    raw.plain = plain(3, 5, &net.head(), &beat);
    assert_eq!(outcome(&n2.classify(&net, s, &raw_frame(&n2, &n1dev, &n1pk, 2, &raw))), refused(code::BAD_PAYLOAD));
    let snap = encode_snap(&SnapInfo {
        seq: 1,
        hash: [0; 32],
        hash_version: 1,
        blob: [0; 16],
        sha256: [0; 32],
        size: 67,
        parts: 1,
        inflated: 1,
        epoch: 1,
    });
    raw.plain = plain(4, 5, &net.head(), &snap);
    assert_eq!(outcome(&n2.classify(&net, s, &raw_frame(&n2, &n1dev, &n1pk, 2, &raw))), refused(code::BAD_PAYLOAD));
    let mut payload = encode_items(&[Item::Upgrade { rules: 2, mode: 2 }]).expect("encodes");
    payload.extend_from_slice(&[0; 64]);
    raw.plain = plain(2, 5, &net.head(), &payload);
    assert_eq!(outcome(&n2.classify(&net, s, &raw_frame(&n2, &n1dev, &n1pk, 2, &raw))), refused(code::BAD_CTL_SIG));
    raw.plain = plain(2, 5, &net.head(), &[0; 10]);
    assert_eq!(outcome(&n2.classify(&net, s, &raw_frame(&n2, &n1dev, &n1pk, 2, &raw))), refused(code::BAD_PAYLOAD));
    // Sealing: a payload over the cap (SEAL-PAD-002), a `based_on` of 0 off `C_0`, `ctr = 0`.
    let entry = |body, based_on| PlainEntry { entry_id: [1; 16], at_ms: 0, based_on, body };
    let big = entry(Body::Cmd(vec![1; 257_980]), net.head());
    assert_eq!(err_code(n1.fake.seal(&n1.m, 50, &big)), code::PAYLOAD_TOO_LARGE);
    let fits = entry(Body::Cmd(vec![1; 257_979]), net.head());
    let fits_frame = ok(n1.fake.seal(&n1.m, 50, &fits)).frame;
    assert_eq!(fits_frame.len(), 258_206);
    let off = entry(Body::Cmd(vec![1]), Head { seq: 0, chain: [0; 32] });
    assert_eq!(err_code(n1.fake.seal(&n1.m, 51, &off)), code::BAD_PLAINTEXT);
    assert_eq!(err_code(n1.fake.seal(&n1.m, 0, &entry(Body::Cmd(vec![1]), net.head()))), code::FRAME_FORMAT);
    // SEAL-FRAME-005: a counter already sealed returns the stored bytes.
    let again = ok(n1.fake.seal(&n1.m, 50, &entry(Body::Cmd(vec![2]), net.head())));
    assert_eq!(h(&[&again.frame]), again.frame_hash);
    assert!(again.frame == fits_frame && !again.keys_changed);
    assert_eq!(err_code(n1.fake.frame_author(&good[..10])), code::FRAME_FORMAT);
    assert_eq!(ok(n1.fake.frame_author(&good)), (n1dev, 2));
}

#[test]
fn epochs_old_stale_and_missing_roots() {
    let (mut net, mut n1, mut n2) = two_devices();
    let m_epoch1 = n1.m.clone();
    let m2_epoch1 = n2.m.clone();
    let rotate = || Body::Ctl(CtlIntent::Rotate { revoke: vec![], reinstate: vec![], purge_before: None });
    let f = n1.seal(&net, 1_000, rotate()).frame;
    assert_all(&submit(&mut net, &mut [&mut n1, &mut n2], f), ACCEPT);
    let epoch_seq = net.head().seq;

    // Under E − 1: a cmd composed before the rotation is accepted (§13.9).
    let s = net.next_seq();
    let ctr = n2.ctr + 1;
    let stale = |n2: &mut Node, entry_id: u8, based_on: Head, body: Body| {
        n2.fake.sealed.remove(&ctr);
        let e = PlainEntry { entry_id: [entry_id; 16], at_ms: 1_100, based_on, body };
        let sealed = ok(n2.fake.seal(&m2_epoch1, ctr, &e));
        assert_eq!(sealed.epoch, 1);
        sealed.frame
    };
    let before_rotation = Head { seq: epoch_seq - 1, chain: net.chains[(epoch_seq - 1) as usize] };
    let stale_ok = stale(&mut n2, 3, Head { ..before_rotation }, Body::Cmd(vec![1]));
    assert_eq!(outcome(&n1.classify(&net, s, &stale_ok)), ACCEPT);
    // ... one composed after it is not (step 14), nor a ctl (step 13).
    let late = stale(&mut n2, 4, net.head(), Body::Cmd(vec![1]));
    assert_eq!(outcome(&n1.classify(&net, s, &late)), refused(code::STALE_EPOCH));
    let fence = Body::Ctl(CtlIntent::Fence { host: "a.example".into(), door: [0; 5], home_gen: 2 });
    let stale_ctl = stale(&mut n2, 5, Head { ..before_rotation }, fence);
    assert_eq!(outcome(&n1.classify(&net, s, &stale_ctl)), refused(code::STALE_EPOCH));
    let stale_ok = stale(&mut n2, 3, Head { ..before_rotation }, Body::Cmd(vec![1]));
    n2.fake.sealed.remove(&ctr);
    // ... nor after 30 days of log clock since the rotation.
    let f = n1.seal(&net, 1_000 + 31 * DAY, Body::Cmd(vec![2])).frame;
    assert_all(&submit(&mut net, &mut [&mut n1, &mut n2], f), ACCEPT);
    assert_eq!(outcome(&n1.classify(&net, net.next_seq(), &stale_ok)), refused(code::STALE_EPOCH));

    // Two rotations on, epoch 1 is old (step 9).
    let f = n1.seal(&net, 2_000 + 31 * DAY, rotate()).frame;
    assert_all(&submit(&mut net, &mut [&mut n1, &mut n2], f), ACCEPT);
    assert_eq!(n1.m.epoch, 3);
    let n1dev = n1.dev();
    let n1pk = n1.fake.sign_pk;
    let raw = Raw { ver: 1, suite: 1, epoch: 1, root: n1.fake.roots[&1], plain: plain(1, 5, &net.head(), b"x") };
    let f = raw_frame(&n1, &n1dev, &n1pk, n1.ctr + 1, &raw);
    assert_eq!(outcome(&n2.classify(&net, net.next_seq(), &f)), refused(code::OLD_EPOCH));
    // A sealer seals under its membership's epoch, and needs that root.
    n1.fake.roots.remove(&3);
    let e = PlainEntry { entry_id: [6; 16], at_ms: 0, based_on: net.head(), body: Body::Cmd(vec![1]) };
    assert_eq!(err_code(n1.fake.seal(&n1.m, 77, &e)), code::MISSING_ROOT);
    let f = cmd(&mut n2, &net, 3_000 + 31 * DAY);
    assert_eq!(err_code(n1.try_classify(&net, net.next_seq(), &f, true)), code::MISSING_ROOT);
    assert_eq!(err_code(n1.fake.open_own(&m_epoch1, &f)), code::UNKNOWN_AUTHOR);
}

// ---------- the items of §13.6

#[test]
fn control_items_refuse_with_their_codes() {
    let (mut net, mut n1, mut n2) = two_devices();
    let s = net.next_seq();
    let n1dev = n1.dev();
    let mut rk = recovery_of(&n1);
    let (n1pk, n1hpke, d2) = (n1.fake.sign_pk, n1.fake.hpke_pk, n2.dev());
    let m_before = n1.m.clone();
    let host = || "relay.example".to_string();
    let fence = |g| Item::Fence { host: host(), door: [0; 5], home_gen: g };

    let mut check = |n1: &mut Node, items: &[Item], want: &'static str| {
        let f = raw_own(n1, &net, 500, items);
        n1.ctr -= 1;
        assert_eq!(outcome(&n2.classify(&net, s, &f)), refused(want));
    };
    check(&mut n1, &[fence(2), fence(3)], code::CTL_SHAPE);
    check(&mut n1, &[fence(3)], code::CTL_HOME_GEN);
    let (d3, s3, p3) = FakeSealer::join_keys(SEED3);
    let add = |dev, sign_pk, hpke_pk, bootstrap_seq| Item::DeviceAdded {
        dev,
        sign_pk,
        hpke_pk,
        name: "gamma".into(),
        transcript: [0; 32],
        bootstrap_seq,
    };
    check(&mut n1, &[add(n1dev, s3, p3, 1)], code::CTL_DUPLICATE_KEY);
    check(&mut n1, &[add(d3, n1pk, p3, 1)], code::CTL_DUPLICATE_KEY);
    check(&mut n1, &[add(d3, s3, n1hpke, 1)], code::CTL_DUPLICATE_KEY);
    check(&mut n1, &[add(rk.dev, s3, p3, 1)], code::CTL_DUPLICATE_KEY);
    check(&mut n1, &[add(d3, s3, p3, s)], code::CTL_TARGET);
    let recipients = recipients_of(&m_before);
    let rot = |e: u32| rotation_item(&m_before, e, &[0x99; 32], &recipients);
    check(&mut n1, &[Item::Revoked([0xFE; 16]), rot(2)], code::CTL_TARGET);
    check(&mut n1, &[Item::Revoked(n1dev), rot(2)], code::CTL_AUTHOR);
    check(&mut n1, &[Item::Reinstated(d2), rot(2)], code::CTL_AUTHOR);
    check(&mut n1, &[rot(3)], code::CTL_EPOCH);
    let mut fewer = recipients.clone();
    fewer.remove(&d2);
    fewer.insert([0xFF; 16], [0; 32]);
    let rot_fewer = rotation_item(&m_before, 2, &[0x99; 32], &fewer);
    check(&mut n1, &[rot_fewer], code::CTL_WRAPS);
    let mut purge = rot(2);
    if let Item::Rotated { purge_before, .. } = &mut purge {
        *purge_before = s + 1;
    }
    check(&mut n1, &[purge], code::CTL_TARGET);
    check(&mut n1, &[Item::ResetTo { seq: s, state_hash: [0; 32], hash_version: 1 }], code::CTL_TARGET);
    check(&mut n1, &[Item::ResetTo { seq: 1, state_hash: [0; 32], hash_version: 1 }], code::CTL_TARGET);
    check(&mut n1, &[Item::ResetTo { seq: 2, state_hash: [0; 32], hash_version: 2 }], code::CTL_TARGET);
    check(&mut n1, &[Item::Recovery { mode: 1, keys: Some((s3, p3)) }, rot(2)], code::CTL_AUTHOR);
    check(&mut n1, &[Item::Recovery { mode: 3, keys: Some((s3, p3)) }, rot(2)], code::CTL_NO_PENDING);

    // By the recovery identity: what it may not author (§13.12).
    let rkdev = rk.dev;
    let rkpk = rk.sign_pk;
    let mut by_r = |n1: &mut Node, items: &[Item], want: &'static str| {
        let f = raw_ctl(n1, &rkdev, &rkpk, rk.ctr + 1, &net, 500, items);
        assert_eq!(outcome(&n2.classify(&net, s, &f)), refused(want));
    };
    by_r(&mut n1, &[add(d3, s3, p3, 1)], code::CTL_AUTHOR);
    by_r(&mut n1, &[Item::ResetTo { seq: 2, state_hash: [0; 32], hash_version: 1 }], code::CTL_AUTHOR);
    by_r(&mut n1, &[Item::Upgrade { rules: 2, mode: 1 }], code::CTL_AUTHOR);
    by_r(&mut n1, &[Item::Recovery { mode: 2, keys: Some((s3, p3)) }], code::CTL_AUTHOR);
    let mut purge = rot(2);
    if let Item::Rotated { purge_before, .. } = &mut purge {
        *purge_before = 1;
    }
    by_r(&mut n1, &[purge.clone()], code::CTL_AUTHOR);
    by_r(&mut n1, &[Item::Reinstated(d2), rot(2)], code::CTL_TARGET);

    // Accepted: a ResetTo above every bootstrap, a purge by a device.
    let f = raw_own(&mut n1, &net, 500, &[Item::ResetTo { seq: 2, state_hash: [0xCD; 32], hash_version: 1 }]);
    assert_all(&submit(&mut net, &mut [&mut n1, &mut n2], f), ACCEPT);
    assert_eq!(n2.fake.view(&n2.m).last_reset, Some((s, 2)));
    let body = Body::Ctl(CtlIntent::Rotate { revoke: vec![], reinstate: vec![], purge_before: Some(2) });
    let f = n1.seal(&net, 600, body).frame;
    let rs = submit(&mut net, &mut [&mut n1, &mut n2], f);
    assert_all(&rs, ACCEPT);
    assert_eq!(n2.m.purge_seq, 2);
    // A DeviceAdded now needs a bootstrap seq at or above the purge seq.
    let f = raw_own(&mut n1, &net, 700, &[add(d3, s3, p3, 1)]);
    n1.ctr -= 1;
    assert_eq!(outcome(&n2.classify(&net, net.next_seq(), &f)), refused(code::CTL_TARGET));
    let _ = &mut rk;
}

#[test]
fn the_last_device_cannot_be_revoked() {
    let (net, mut n1) = board();
    let mut rk = recovery_of(&n1);
    let mut after = n1.m.clone();
    if let Some(d) = after.device_mut(&n1.dev()) {
        d.current = false;
    }
    let mut recipients = recipients_of(&after);
    recipients.insert([0xFF; 16], [0; 32]);
    let rot = rotation_item(&n1.m, 2, &[0x99; 32], &recipients);
    rk.ctr += 1;
    let (rkdev, rkpk) = (rk.dev, rk.sign_pk);
    let d1 = n1.dev();
    let f = raw_ctl(&mut n1, &rkdev, &rkpk, rk.ctr, &net, 10, &[Item::Revoked(d1), rot]);
    assert_eq!(outcome(&n1.classify(&net, net.next_seq(), &f)), refused(code::CTL_LAST_DEVICE));
}

#[test]
fn rules_upgrades_support_override_and_the_rules_halt() {
    let (mut net, mut n1, mut n2) = two_devices();
    let upgrade = |mode| Body::Ctl(CtlIntent::RulesUpgrade { rules: 2, mode });
    // Device 2 is active and its latest beat lacks 2.
    let f = n1.seal(&net, 1_000, Body::Beat(beat_info(vec![1, 2]))).frame;
    assert_all(&submit(&mut net, &mut [&mut n1, &mut n2], f), ACCEPT);
    let f = n1.seal(&net, 1_100, upgrade(UpgradeMode::Normal)).frame;
    assert_all(&submit(&mut net, &mut [&mut n1, &mut n2], f), refused(code::CTL_SUPPORT));
    // Two override votes by two devices activate it; a second vote by the same voter is refused.
    let f = n1.seal(&net, 1_200, upgrade(UpgradeMode::Override)).frame;
    assert_all(&submit(&mut net, &mut [&mut n1, &mut n2], f), ACCEPT);
    assert_eq!(n2.m.pending_override, Some((2, n1.dev(), net.head().seq)));
    let f = n1.seal(&net, 1_300, upgrade(UpgradeMode::Override)).frame;
    assert_all(&submit(&mut net, &mut [&mut n1, &mut n2], f), refused(code::CTL_DUPLICATE));
    n1.fake.set_supported_rules(&[1, 2]);
    let f = n2.seal(&net, 1_400, upgrade(UpgradeMode::Override)).frame;
    let rs = submit(&mut net, &mut [&mut n1, &mut n2], f);
    assert_eq!(outcome(&rs[0]), ACCEPT);
    assert_eq!(outcome(&rs[1]), ("accept", Some(code::UNKNOWN_RULES)));
    assert!(matches!(rs[1].halt, Some(Halt::UnknownRules(2))));
    assert_eq!((n1.m.rules, n2.m.rules), (2, 2));
    assert!(n1.m.pending_override.is_none());

    // A single device is the only active member: normal activates on its own beat.
    let (mut net, mut n1) = board();
    let f = n1.seal(&net, 1_000, Body::Beat(beat_info(vec![1, 2]))).frame;
    assert_all(&submit(&mut net, &mut [&mut n1], f), ACCEPT);
    let f = n1.seal(&net, 1_100, upgrade(UpgradeMode::Normal)).frame;
    let rs = submit(&mut net, &mut [&mut n1], f);
    assert_eq!(outcome(&rs[0]), ("accept", Some(code::UNKNOWN_RULES)));
    assert_eq!(n1.m.rules, 2);
}

#[test]
fn skips_need_their_target_frames() {
    let (mut net, mut n1, mut n2) = two_devices();
    let f = cmd(&mut n2, &net, 100);
    let t_hash = h(&[&f]);
    assert_all(&submit(&mut net, &mut [&mut n1, &mut n2], f), ACCEPT);
    let t = net.head().seq;
    let skip = |targets| Body::Ctl(CtlIntent::Skip { targets });

    // A ctl seq and an own frame are refused; a wrong hash is refused once supplied.
    let f = n1.seal(&net, 200, skip(vec![(2, [0; 32])])).frame;
    assert_all(&submit(&mut net, &mut [&mut n1, &mut n2], f), refused(code::CTL_TARGET));
    let f = n2.seal(&net, 200, skip(vec![(t, t_hash)])).frame;
    assert_all(&submit(&mut net, &mut [&mut n1, &mut n2], f), refused(code::CTL_AUTHOR));
    let f = n1.seal(&net, 200, skip(vec![(t, [0; 32])])).frame;
    assert_all(&submit(&mut net, &mut [&mut n1, &mut n2], f), refused(code::CTL_TARGET));

    // Without the frame: NeedFrames, nothing changes; with it: accepted.
    let f = n1.seal(&net, 300, skip(vec![(t, t_hash)])).frame;
    let s = net.push(f.clone());
    let r = ok(n2.try_classify(&net, s, &f, false));
    let Class::NeedFrames(need) = &r.class else { panic!("NeedFrames") };
    assert_eq!(need.as_slice(), &[(t, t_hash)]);
    assert_eq!(r.membership.to_bytes(), n2.m.to_bytes());
    assert_eq!(outcome(&n1.deliver(&net, s)), ACCEPT);
    assert_eq!(outcome(&n2.deliver(&net, s)), ACCEPT);
    assert_eq!(n2.fake.view(&n2.m).skips, vec![(t, t_hash)]);
    // Already skipped.
    let f = n1.seal(&net, 400, skip(vec![(t, t_hash)])).frame;
    assert_all(&submit(&mut net, &mut [&mut n1, &mut n2], f), refused(code::CTL_TARGET));
    // The recovery identity may skip a device's frame.
    let f = cmd(&mut n1, &net, 500);
    let (t2, t2_hash) = (net.next_seq(), h(&[&f]));
    assert_all(&submit(&mut net, &mut [&mut n1, &mut n2], f), ACCEPT);
    let mut rk = recovery_of(&n1);
    let f = seal_as_recovery(&mut n1, &mut rk, &net, 600, skip(vec![(t2, t2_hash)]));
    assert_all(&submit(&mut net, &mut [&mut n1, &mut n2], f), ACCEPT);
}

#[test]
fn the_device_and_recovery_caps() {
    let (mut net, mut n1) = board();
    for i in 0..15u64 {
        let (dev, sign_pk, hpke_pk) = FakeSealer::join_keys(1_000 + i);
        let item = Item::DeviceAdded { dev, sign_pk, hpke_pk, name: format!("d{i}"), transcript: [0; 32], bootstrap_seq: 1 };
        let f = raw_own(&mut n1, &net, 10, &[item]);
        assert_all(&submit(&mut net, &mut [&mut n1], f), ACCEPT);
    }
    assert_eq!(n1.m.current_count(), 16);
    assert_eq!(n1.m.next_device_no, 17);
    let (dev, sign_pk, hpke_pk) = FakeSealer::join_keys(2_000);
    let item = Item::DeviceAdded { dev, sign_pk, hpke_pk, name: "full".into(), transcript: [0; 32], bootstrap_seq: 1 };
    let f = raw_own(&mut n1, &net, 10, &[item]);
    assert_all(&submit(&mut net, &mut [&mut n1], f), refused(code::CTL_DEVICE_CAP));

    // 64 recovery records at most (`seal/ctl_cap`).
    let (mut net, mut n1) = board();
    let mut rk = recovery_of(&n1);
    for i in 0..63u8 {
        let (s, p) = public_keys(&[i; 32]);
        let body = Body::Ctl(CtlIntent::RecoveryChange { mode: RecoveryMode::Direct, sign_pk: Some(s), hpke_pk: Some(p) });
        let f = seal_as_recovery(&mut n1, &mut rk, &net, 10, body);
        assert_all(&submit(&mut net, &mut [&mut n1], f), ACCEPT);
        rk = Rk { dev: dev_r_of(&n1.fake.board_id, &s), sign_pk: s, ctr: 0 };
    }
    assert_eq!(n1.m.recovery.len(), 64);
    let (s, p) = public_keys(&[0xF0; 32]);
    let body = Body::Ctl(CtlIntent::RecoveryChange { mode: RecoveryMode::Direct, sign_pk: Some(s), hpke_pk: Some(p) });
    let f = seal_as_recovery(&mut n1, &mut rk, &net, 10, body);
    assert_all(&submit(&mut net, &mut [&mut n1], f), refused(code::CTL_CAP));
    assert_eq!(n1.m.epoch, 64);
}

// ---------- membership bytes

#[test]
fn the_membership_parser_is_strict() {
    let (_net, n1, n2) = two_devices();
    let bytes = n1.m.to_bytes();
    assert_eq!(bytes, n2.m.to_bytes());
    assert_eq!(ok(n1.fake.membership_from_bytes(&bytes)).to_bytes(), bytes);
    assert_eq!(ok(n1.fake.membership_from_bytes(&FakeMembership::default().to_bytes())).to_bytes(), FakeMembership::default().to_bytes());
    let reject = |b: &[u8]| err_code(n1.fake.membership_from_bytes(b));
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert_eq!(reject(&trailing), code::MEMBERSHIP_FORMAT);
    assert_eq!(reject(&bytes[..bytes.len() - 1]), code::MEMBERSHIP_FORMAT);
    let mut magic = bytes.clone();
    magic[3] = b'2';
    assert_eq!(reject(&magic), code::MEMBERSHIP_FORMAT);
    // Devices out of order.
    let mut swapped = n1.m.clone();
    swapped.devices.reverse();
    assert_eq!(reject(&swapped.to_bytes()), code::MEMBERSHIP_FORMAT);
    // A flag other than 0 or 1: the pending_recovery flag sits after the device records.
    let flags_at = bytes.len() - 2 * 8 - 4 - 4 - 3;
    assert_eq!(&bytes[flags_at..flags_at + 3], &[0, 0, 0]);
    let mut flag = bytes.clone();
    flag[flags_at] = 2;
    assert_eq!(reject(&flag), code::MEMBERSHIP_FORMAT);
    // Two current recovery keys.
    let mut two = n1.m.clone();
    let mut r = two.recovery[0].clone();
    r.since_seq = 5;
    two.recovery.push(r);
    assert_eq!(reject(&two.to_bytes()), code::MEMBERSHIP_FORMAT);
}

// ---------- snapshots (§20)

#[test]
fn snapshots_refuse_with_the_codes_of_20_5() {
    let (net, mut n1, n2) = two_devices();
    let material = material_at(&n1, &net);
    let blob = ok(n1.fake.seal_snapshot(&n1.m, &material));
    assert_eq!(&blob.bytes[..7], b"TDB1\x01\x01\x01");
    assert_eq!(blob.bytes[7..11], [0, 0, 0, 1]);
    assert_eq!(blob.bytes[11..27], blob.id);
    assert_eq!((blob.parts, blob.epoch), (1, 1));
    assert_eq!(blob.sha256, sha(&[&blob.bytes]));
    assert_eq!(blob.bytes.len() as u64, 67 + pad_len(4 + blob.inflated));
    let info = |b: &SealedBlob| SnapInfo {
        seq: material.seq,
        hash: material.state_hash,
        hash_version: 1,
        blob: b.id,
        sha256: b.sha256,
        size: b.bytes.len() as u64,
        parts: b.parts,
        inflated: b.inflated,
        epoch: b.epoch,
    };
    let w = n1.dev();
    let open = |i: &SnapInfo, parts: &[&[u8]], writer: &Dev, exact: bool| err_code(n2.fake.open_snapshot(parts, i, writer, exact));
    let good = info(&blob);
    assert_eq!(open(&good, &[&blob.bytes], &w, true), "ok");
    let mut i = info(&blob);
    i.parts = 2;
    assert_eq!(open(&i, &[&blob.bytes], &w, true), code::BLOB_SIZE);
    let (a, b) = blob.bytes.split_at(100);
    assert_eq!(open(&good, &[a, b], &w, true), code::BLOB_SIZE);
    let mut flipped = blob.bytes.clone();
    flipped[60] ^= 1;
    assert_eq!(open(&good, &[&flipped], &w, true), code::BLOB_HASH);
    let mut i = info(&blob);
    i.blob = [0; 16];
    assert_eq!(open(&i, &[&blob.bytes], &w, true), code::BLOB_HEADER);
    let mut i = info(&blob);
    i.epoch = 2;
    assert_eq!(open(&i, &[&blob.bytes], &w, true), code::BLOB_HEADER);
    assert_eq!(open(&i, &[&blob.bytes], &w, false), "ok");
    assert_eq!(open(&good, &[&blob.bytes], &n2.dev(), true), code::BLOB_OPEN);
    let mut i = info(&blob);
    i.seq += 1;
    assert_eq!(open(&i, &[&blob.bytes], &w, true), code::BLOB_OPEN);
    let mut i = info(&blob);
    i.hash = [0; 32];
    assert_eq!(open(&i, &[&blob.bytes], &w, true), code::BLOB_OPEN);
    assert_eq!(open(&i, &[&blob.bytes], &w, false), "ok");
    let mut i = info(&blob);
    i.inflated += 1;
    assert_eq!(open(&i, &[&blob.bytes], &w, true), code::INFLATE);

    // The history disagrees with the membership (§20.5 step 7).
    let mut forged_m = n1.m.clone();
    forged_m.devices[0].current = !forged_m.devices[0].current;
    let forged = SnapshotMaterial { membership: forged_m.to_bytes(), ..material_at(&n1, &net) };
    let fb = ok(n1.fake.seal_snapshot(&n1.m, &forged));
    let mut fi = info(&fb);
    fi.hash = forged.state_hash;
    assert_eq!(open(&fi, &[&fb.bytes], &w, true), code::BLOB_OPEN);

    // Too large, before compression (SEAL-BLOB-009): nothing drawn, nothing sealed.
    let state = n1.fake.rng.state;
    let large = SnapshotMaterial { board: vec![0; BLOB_MAX as usize], ..material_at(&n1, &net) };
    assert_eq!(err_code(n1.fake.seal_snapshot(&n1.m, &large)), code::SNAPSHOT_LARGE);
    assert_eq!(n1.fake.rng.state, state);

    // No root for the header's epoch, in the declared mode too.
    let mut n2 = n2;
    n2.fake.roots.clear();
    assert_eq!(err_code(n2.fake.open_snapshot(&[&blob.bytes], &good, &w, false)), code::MISSING_ROOT);
}

#[test]
fn a_paired_device_seals_snapshots_with_the_whole_history() {
    let (mut net, mut n1, mut n2) = two_devices();
    let f = n1.seal(&net, 100, Body::Ctl(CtlIntent::Rotate { revoke: vec![], reinstate: vec![], purge_before: None })).frame;
    assert_all(&submit(&mut net, &mut [&mut n1, &mut n2], f), ACCEPT);
    // Device 2 never classified seq 1; its history came from the bundle.
    let material = material_at(&n2, &net);
    let blob = ok(n2.fake.seal_snapshot(&n2.m, &material));
    let info = SnapInfo {
        seq: material.seq,
        hash: material.state_hash,
        hash_version: 1,
        blob: blob.id,
        sha256: blob.sha256,
        size: blob.bytes.len() as u64,
        parts: blob.parts,
        inflated: blob.inflated,
        epoch: blob.epoch,
    };
    let (m, epoch, _) = ok(n1.fake.open_snapshot(&[&blob.bytes], &info, &n2.dev(), true));
    assert_eq!((m.membership, epoch), (material.membership, 2));
    assert_eq!(n2.fake.history.borrow().keys().copied().collect::<Vec<_>>(), vec![1, 2, 3]);
}

#[test]
fn the_identity_export_holds_keys_roots_pending_roots_and_anchor() {
    let (net, mut n1) = board();
    let before = n1.fake.export_identity().to_vec();
    assert_eq!(&before[..8], b"FKID\x00\x00\x00\x01");
    assert_eq!(before.len(), 8 + 16 + 32 + 32 + 32 + 16 + 1 + 32 + 16 + 32 + 4 + 36 + 4);
    let _ = n1.seal(&net, 1, Body::Ctl(CtlIntent::Rotate { revoke: vec![], reinstate: vec![], purge_before: None }));
    let after = n1.fake.export_identity().to_vec();
    assert_eq!(after.len(), before.len() + 52);
    assert_eq!(n1.fake.supported_envelopes(), vec![(1, 1)]);
    assert_eq!(n1.fake.own_dev(), n1.dev());
    assert_eq!(n1.fake.chain_zero(), net.chains[0]);
    assert_eq!(n1.fake.chain_step(&net.chains[0], 1, &net.frames[0]), net.chains[1]);
    assert_eq!(n1.fake.frame_hash(&net.frames[0]), sha(&[&net.frames[0]]));
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

    /// Classification never panics (SEAL-GEN-011), whatever bytes arrive.
    #[test]
    fn classify_never_panics(at in 0usize..1182, byte in any::<u8>(), cut in 0usize..1300, junk in proptest::collection::vec(any::<u8>(), 0..800)) {
        let (fake, frame, _) = FakeSealer::create(SEED1, "alpha");
        let net = Net::new(&fake.board_id);
        let mut n = Node::new(fake);
        let mut f = frame.clone();
        f[at] ^= byte | 1;
        let _ = n.try_classify(&net, 1, &f, true);
        let _ = n.try_classify(&net, 1, &frame[..cut.min(frame.len())], true);
        let _ = n.try_classify(&net, 1, &junk, true);
        let mut padded = junk.clone();
        padded.resize(700, 0);
        padded[..4].copy_from_slice(b"TDF1");
        let _ = n.try_classify(&net, 1, &padded, true);
    }
}
