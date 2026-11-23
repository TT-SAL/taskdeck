//! Both machines through whole pairings against a simulated relay and a
//! `Sim` board (seal.md §17): every check of §17.6 and §17.9 failing once,
//! in order; the `NeedFrames` round trip; a rotation racing the pairing in
//! both orders; slots replayed under another label or reflected.

use taskdeck_seal::pair::*;
use taskdeck_seal::*;

use crate::board::*;
use crate::pairing::*;

fn names_n(fx: &[NewEffect]) -> Vec<&'static str> {
    fx.iter().map(n_name).collect()
}

fn names_e(fx: &[ExistingEffect]) -> Vec<&'static str> {
    fx.iter().map(e_name).collect()
}

/// `[Abort{code}]` and nothing else: nothing is handed out on a failure.
fn aborts(fx: &[NewEffect], code: SealError) {
    assert_eq!((names_n(fx), n_abort(fx)), (vec!["Abort"], Some(code)));
}

fn joined(fx: Vec<NewEffect>) -> (u16, DeviceKeys, Bundle, SnapshotBody) {
    let names = names_n(&fx);
    let code = n_abort(&fx);
    match fx.into_iter().next() {
        Some(NewEffect::Joined { device_no, keys, bundle, snapshot }) => (device_no, keys, bundle, snapshot),
        _ => panic!("not joined: {names:?} {code:?}"),
    }
}

/// A pairing on `sim` up to E's confirmation, with the snapshot taken.
fn confirmed(sim: Sim, ei: usize) -> Run {
    let mut r = Run::new(sim, ei);
    r.step_b().step_c().step_sas().step_confirmed().take_snapshot();
    r
}

/// From `Registered` to N in `Joining`.
fn to_joining(mut r: Run, ev: ExistingEvent) -> Run {
    r.post_d(ev);
    r.step_joining();
    r
}

// ------------------------------------------------------ a whole pairing

/// A whole pairing on a board at epoch 2 with entries before and after the
/// `DeviceAdded`: E asks for exactly slot B's keys and name and `T`; N
/// hands out nothing before `Joined`, which carries the `device_no` the
/// accepted `DeviceAdded` assigned (from the board, not the bundle), N's
/// keys, the bundle as E's engine gave it, and the snapshot at `h`.
// spec: SEAL-PAIR-010
// spec: SEAL-PAIR-011
// spec: SEAL-PAIR-012
#[test]
fn a_whole_pairing() {
    let mut sim = Sim::new(610, 3);
    sim.rotate(Who::D(1));
    let f = sim.cmd(Who::D(2), &Spec::default());
    sim.accept(f);
    let mut r = Run::new(sim, 1);
    r.step_b().step_c().step_sas().step_confirmed();
    let device = r.device.clone().unwrap();
    let t = transcript(&r.sim.board, &r.k.tid, &r.dev_e(), &r.slot_a, &r.slot_b, &r.slot_c);
    let b = body(&r.slot_b).to_vec();
    assert_eq!(
        (device.dev.0.to_vec(), device.sign_pk.to_vec(), device.hpke_pk.to_vec(), device.name.as_str(), device.transcript),
        (b[..16].to_vec(), b[16..48].to_vec(), b[48..80].to_vec(), NAME, t)
    );
    r.take_snapshot();
    let h = r.snap.as_ref().unwrap().h;
    let f = r.sim.cmd(Who::D(0), &Spec::default());
    r.sim.accept(f);
    let rc = r.append_added(|_| {}, &Spec::default());
    assert!(matches!(rc.class, Class::Accept));
    let f = r.sim.beat(Who::D(0), vec![1], &Spec::default());
    r.sim.accept(f);
    // A device added after N does not change N's number.
    r.sim.add_device();
    let ev = r.registered();
    r.post_d(ev);
    let fx = r.send_d();
    let [NewEffect::FetchSnapshot { token, blob, parts }] = &fx[..] else { panic!("{:?}", names_n(&fx)) };
    let ann = r.snap.as_ref().unwrap().ann.clone();
    assert_eq!((token.as_str(), *blob, *parts), (TOKEN, ann.blob, ann.parts));
    let fx = r.send_snapshot(None);
    let [NewEffect::ReadLog { token, from }] = &fx[..] else { panic!("{:?}", names_n(&fx)) };
    assert_eq!((token.as_str(), *from), (TOKEN, h + 1));
    let (device_no, keys, bundle, snapshot) = joined(r.send_log());
    assert_eq!(Some(device_no), r.sim.m.device_no(&device.dev));
    assert_eq!(device_no, 4);
    let p = keys.public();
    assert_eq!((p.dev, p.sign_pk, p.hpke_pk), (device.dev, device.sign_pk, device.hpke_pk));
    assert_eq!((bundle.board_id, bundle.host.as_str(), bundle.door, bundle.head.seq), (r.sim.board, HOST, DOOR, h));
    assert_eq!((bundle.added_seq, bundle.token.as_str(), &bundle.snapshot), (r.added_seq, TOKEN, &ann));
    let roots: Vec<(u32, [u8; 32])> = bundle.roots.iter().map(|(e, k)| (*e, *k.to_bytes())).collect();
    assert_eq!(roots, vec![(1, r.sim.roots[&1]), (2, r.sim.roots[&2])]);
    assert_eq!((snapshot.seq, snapshot.chain), (h, bundle.head.chain));
    // Done: nothing more.
    assert!(r.n_on(NewEvent::Cancel).is_empty());
}

/// A board at epoch 1: the bundle has one root; N joins.
// spec: SEAL-PAIR-009
#[test]
fn a_pairing_at_epoch_1() {
    let mut r = Run::new(Sim::new(611, 1), 0);
    r.through_d().step_joining();
    let (device_no, _, bundle, _) = joined(r.send_log());
    assert_eq!(device_no, 2);
    assert_eq!(bundle.roots.iter().map(|(e, _)| *e).collect::<Vec<_>>(), [1]);
}

// ------------------------------------------------- E's checks on slot B

/// E on a slot B that a test playing N builds from `make`.
fn e_on_b(sim: Sim, make: impl FnOnce(&Run, &FakeN) -> Vec<u8>) -> Vec<ExistingEffect> {
    let mut r = Run::new(sim, 0);
    let f = FakeN::new(&r.k, rand32());
    let b = make(&r, &f);
    r.e_on(ExistingEvent::SlotB { bytes: b })
}

fn with(p: DevicePublic, dev: Option<DevId>, sign_pk: Option<[u8; 32]>, hpke_pk: Option<[u8; 32]>) -> DevicePublic {
    DevicePublic { dev: dev.unwrap_or(p.dev), sign_pk: sign_pk.unwrap_or(p.sign_pk), hpke_pk: hpke_pk.unwrap_or(p.hpke_pk) }
}

/// §17.6 in order, each check failing once, and a pair of failures giving
/// the earlier check's code: 1 the MAC, label and role; 2 the layout; 3 N's
/// keys and id new to E's membership; 4 E's rules version in N's list; 5
/// room for a device. Every failure deletes the ticket and posts no slot C.
// spec: SEAL-PAIR-005
// spec: SEAL-PAIR-003
#[test]
fn e_checks_slot_b_in_order() {
    let fails = |fx: &[ExistingEffect], code: SealError| {
        assert_eq!((names_e(fx), e_abort(fx)), (vec!["DeleteTicket", "Abort"], Some(code)));
    };
    let s = || Sim::new(620, 2);
    // Passes, and with more rules versions too.
    for rules in [vec![1], vec![1, 2, 7]] {
        let fx = e_on_b(s(), |_, f| f.slot_b(&rules, NAME));
        assert_eq!(names_e(&fx), ["PostSlot", "ShowSas"]);
    }
    // 2: a body one byte short of the smallest.
    fails(&e_on_b(s(), |r, f| r.k.slot(B, ROLE_N, &body(&f.slot_b(&[1], "P"))[..118])), SealError::SlotFormat);
    // 1 before 2: a bad MAC on a body without a name.
    let fx = e_on_b(s(), |_, f| {
        let mut b = f.slot_b(&[2], "");
        let n = b.len();
        b[n - 5] ^= 0x10;
        b
    });
    fails(&fx, SealError::SlotMac);
    // 2 before 3: an empty rules list with E's own id.
    let fx = e_on_b(s(), |r, f| {
        let p = with(f.keys.public(), Some(r.sim.devs[0].public().dev), None, None);
        r.k.slot(B, ROLE_N, &b_body(&p, &f.epk, &[], NAME))
    });
    fails(&fx, SealError::SlotFormat);
    // 3, each key, with an unsupported rules list too (3 before 4).
    type Pick = fn(&Run, DevicePublic) -> DevicePublic;
    let picks: Vec<Pick> = vec![
        |r, p| with(p, Some(r.sim.devs[1].public().dev), None, None),
        |r, p| with(p, Some(r.sim.m.current_recovery().dev), None, None),
        |r, p| with(p, None, Some(r.sim.devs[0].public().sign_pk), None),
        |r, p| with(p, None, Some(r.sim.m.current_recovery().sign_pk), None),
        |r, p| with(p, None, None, Some(r.sim.devs[1].public().hpke_pk)),
        |r, p| with(p, None, None, Some(r.sim.m.current_recovery().hpke_pk)),
    ];
    for pick in picks {
        for rules in [vec![1], vec![2]] {
            let fx = e_on_b(s(), |r, f| r.k.slot(B, ROLE_N, &b_body(&pick(r, f.keys.public()), &f.epk, &rules, NAME)));
            fails(&fx, SealError::PairKeysConflict);
        }
    }
    // 4, and 4 before 5 on a full board.
    fails(&e_on_b(s(), |_, f| f.slot_b(&[2, 3], NAME)), SealError::PairRules);
    fails(&e_on_b(Sim::new(621, 16), |_, f| f.slot_b(&[2], NAME)), SealError::PairRules);
    // 5: sixteen current devices.
    fails(&e_on_b(Sim::new(621, 16), |_, f| f.slot_b(&[1], NAME)), SealError::CtlDeviceCap);
}

// ------------------------------------------- N's checks on the bundle

/// §17.9 steps 1 and 2 on E's slot D: its MAC, label and role
/// (`seal/slot_mac`), its width (`seal/slot_format`), then the AEAD
/// (`seal/bundle_open`) on a slot re-MACed by a relay that knows the code.
// spec: SEAL-PAIR-003
// spec: SEAL-PAIR-009
// spec: SEAL-PAIR-011
#[test]
fn n_checks_slot_d_steps_1_and_2() {
    let mut base = Run::new(Sim::new(630, 1), 0);
    base.through_d();
    let n = base.slot_d.len();
    type Edit = Box<dyn Fn(&Code, &[u8]) -> Vec<u8>>;
    let flip = |at: usize| -> Edit {
        Box::new(move |_, d: &[u8]| {
            let mut v = d.to_vec();
            v[at] ^= 1;
            v
        })
    };
    let cases: Vec<(Edit, SealError)> = vec![
        (flip(n - 1), SealError::SlotMac),
        (flip(40), SealError::SlotMac),
        (Box::new(|k, d| k.slot(C, ROLE_E, body(d))), SealError::SlotMac),
        (Box::new(|k, d| k.slot(D, ROLE_N, body(d))), SealError::SlotMac),
        (Box::new(|k, d| k.slot(D, ROLE_E, &body(d)[..39])), SealError::SlotFormat),
        // Over the largest slot body: the framing fails (§23, SEAL-GEN-008).
        (Box::new(|k, d| k.slot(D, ROLE_E, &[body(d), &vec![0; 16_041 - body(d).len()]].concat())), SealError::SlotMac),
        (Box::new(|k, d| k.remac(&{ let mut v = d.to_vec(); v[40] ^= 1; v })), SealError::BundleOpen),
        (Box::new(|k, d| k.remac(&{ let mut v = d.to_vec(); v[5] ^= 1; v })), SealError::BundleOpen),
        (Box::new(|k, d| k.slot(D, ROLE_E, &body(d)[..body(d).len() - 1])), SealError::BundleOpen),
    ];
    for (edit, code) in cases {
        let mut r = Run::new(Sim::new(630, 1), 0);
        r.through_d();
        let bad = edit(&r.k, &r.slot_d);
        aborts(&r.n_on(NewEvent::Slot { bytes: bad }), code);
    }
    let fx = base.send_d();
    assert_eq!(names_n(&fx), ["FetchSnapshot"]);
}

fn ann(h: u64) -> SnapAnnounce {
    SnapAnnounce { seq: h, hash: [1; 32], hash_version: 1, blob: BlobId([2; 16]), sha256: [3; 32], size: 3_000_000, parts: 3, inflated: 100, epoch: 2 }
}

/// §17.9 step 3, with a test playing E: the JSON must parse strictly and
/// name the board, host and door N was given; else `seal/bundle_invalid`.
// spec: SEAL-PAIR-009
// spec: SEAL-PAIR-011
#[test]
fn n_checks_the_bundle_json() {
    let board = BoardId([0xB0; 16]);
    let head = Head { seq: 70, chain: Chain([5; 32]) };
    let roots = [(1, [7u8; 32]), (2, [8; 32])];
    let good = bundle_json(&board, HOST, &DOOR, &roots, head, &ann(70), 71, TOKEN);
    let text = String::from_utf8(good.clone()).unwrap();
    let pretty = serde_json::to_vec_pretty(&serde_json::from_slice::<serde_json::Value>(&good).unwrap()).unwrap();
    let cases: Vec<Vec<u8>> = vec![
        pretty,
        bundle_json(&BoardId([0xB1; 16]), HOST, &DOOR, &roots, head, &ann(70), 71, TOKEN),
        bundle_json(&board, "relay.example.net", &DOOR, &roots, head, &ann(70), 71, TOKEN),
        bundle_json(&board, HOST, &[0; 5], &roots, head, &ann(70), 71, TOKEN),
        bundle_json(&board, HOST, &DOOR, &roots, head, &ann(70), 70, TOKEN),
        bundle_json(&board, HOST, &DOOR, &roots[1..], head, &ann(70), 71, TOKEN),
        text.replacen("\"added_seq\"", "\"seed\":\"00\",\"added_seq\"", 1).into_bytes(),
        text.replacen(&format!(",\"token\":\"{TOKEN}\""), "", 1).into_bytes(),
        format!("{text}\n").into_bytes(),
        b"[]".to_vec(),
        vec![],
    ];
    for pt in cases {
        let w = WithFakeE::new(board, DevId([0xE0; 16])).accept();
        let d = w.slot_d(&pt);
        let mut n = w.n;
        aborts(&n.on(NewEvent::Slot { bytes: d }, &mut OsEntropy), SealError::BundleInvalid);
    }
    // A bundle over 16,000 bytes needs a slot D body over 16,040 bytes,
    // which the slot's framing refuses (§23).
    let w = WithFakeE::new(board, DevId([0xE0; 16])).accept();
    let big = text.replacen(TOKEN, &"x".repeat(16_000 - text.len() + TOKEN.len() + 1), 1);
    assert_eq!(big.len(), 16_001);
    let d = w.slot_d(big.as_bytes());
    let mut n = w.n;
    aborts(&n.on(NewEvent::Slot { bytes: d }, &mut OsEntropy), SealError::SlotMac);
    let w = WithFakeE::new(board, DevId([0xE0; 16])).accept();
    let d = w.slot_d(&good);
    let mut n = w.n;
    assert_eq!(names_n(&n.on(NewEvent::Slot { bytes: d }, &mut OsEntropy)), ["FetchSnapshot"]);
}

/// `Registered` for a log that ends before `added_seq`, edited by `f`.
fn registered_with(r: &mut Run, f: &dyn Fn(&mut ExistingEvent)) -> ExistingEvent {
    r.added_seq = r.sim.seq() + 1;
    let mut ev = r.registered();
    f(&mut ev);
    ev
}

/// §17.9 steps 4 and 5: the snapshot passes §20.5 with `S = h`, writer
/// `dev_E` and the bundle's reference and root; its chain is `head.chain`;
/// its membership names `dev_E` as current and the highest root's epoch as
/// current; every root matches its commitment there; its state hash is the
/// bundle's. Each failing once is `seal/bundle_invalid`, with nothing handed
/// out.
// spec: SEAL-PAIR-011
// spec: SEAL-KEY-013
#[test]
fn n_checks_the_snapshot() {
    type Edit = fn(&mut Run) -> ExistingEvent;
    let epoch2 = || {
        let mut s = Sim::new(640, 2);
        s.rotate(Who::D(1));
        s
    };
    let cases: Vec<(&str, Edit)> = vec![
        ("the sha256", |r| registered_with(r, &|ev| if let ExistingEvent::Registered { snapshot, .. } = ev { snapshot.sha256[0] ^= 1 })),
        ("the current root", |r| registered_with(r, &|ev| if let ExistingEvent::Registered { roots, .. } = ev { roots[1].1 = EpochRoot::from_bytes([1; 32]) })),
        ("the inflated length", |r| registered_with(r, &|ev| if let ExistingEvent::Registered { snapshot, .. } = ev { snapshot.inflated += 1 })),
        ("the chain", |r| registered_with(r, &|ev| if let ExistingEvent::Registered { chain, .. } = ev { chain.0[0] ^= 1 })),
        ("the previous root", |r| registered_with(r, &|ev| if let ExistingEvent::Registered { roots, .. } = ev { roots[0].1 = EpochRoot::from_bytes([1; 32]) })),
        ("the state hash", |r| registered_with(r, &|ev| if let ExistingEvent::Registered { snapshot, .. } = ev { snapshot.hash[0] ^= 1 })),
        ("the hash version", |r| registered_with(r, &|ev| if let ExistingEvent::Registered { snapshot, .. } = ev { snapshot.hash_version = 2 })),
        ("the head", |r| registered_with(r, &|ev| if let ExistingEvent::Registered { h, snapshot, .. } = ev { *h -= 1; snapshot.seq -= 1 })),
        ("an epoch not the current one", |r| {
            // The state at epoch 2 sealed under `K_snap,1`, announced as epoch 1.
            let s = &mut r.sim;
            let body = crate::blob::snap_body(s);
            let key = derive_epoch_key(&EpochRoot::from_bytes(s.roots[&1]), &s.board, 1, KeyKind::Snap);
            let w = s.devs[0].public().dev;
            let sb = seal_snapshot(&body, &key, &s.board, 1, &w, &mut OsEntropy).unwrap();
            let snap = r.snap.as_mut().unwrap();
            snap.ann.epoch = 1;
            snap.ann.blob = sb.id;
            snap.ann.sha256 = sb.sha256;
            snap.ann.size = sb.bytes.len() as u64;
            snap.ann.parts = sb.parts;
            snap.bytes = sb.bytes;
            snap.roots.truncate(1);
            registered_with(r, &|_| {})
        }),
        ("a hash version the membership's rules do not allow", |r| {
            // A body of version 2 whose state hash is computed under version
            // 2, announced with that hash and version: `open_snapshot`
            // compares them equal (task S6b), so only N's own recomputation
            // of step 5 (`engine.md`, the rules of §14.3) refuses it.
            let s = &mut r.sim;
            let mut body = crate::blob::snap_body(s);
            body.hash_version = 2;
            body.state_hash = state_hash(2, &crate::backup::sha(&[&body.board]), &body.membership);
            let e = s.m.epoch();
            let key = derive_epoch_key(&EpochRoot::from_bytes(s.roots[&e]), &s.board, e, KeyKind::Snap);
            let w = s.devs[0].public().dev;
            let sb = seal_snapshot(&body, &key, &s.board, e, &w, &mut OsEntropy).unwrap();
            let snap = r.snap.as_mut().unwrap();
            snap.ann.hash = body.state_hash;
            snap.ann.hash_version = 2;
            snap.ann.blob = sb.id;
            snap.ann.sha256 = sb.sha256;
            snap.ann.size = sb.bytes.len() as u64;
            snap.ann.parts = sb.parts;
            snap.ann.inflated = crate::blob::body_bytes(&body).len() as u64;
            snap.bytes = sb.bytes;
            registered_with(r, &|_| {})
        }),
    ];
    for (what, edit) in cases {
        let mut r = confirmed(epoch2(), 0);
        let ev = edit(&mut r);
        r.post_d(ev);
        assert_eq!(names_n(&r.send_d()), ["FetchSnapshot"], "{what}");
        let fx = r.send_snapshot(None);
        assert_eq!((names_n(&fx), n_abort(&fx)), (vec!["Abort"], Some(SealError::BundleInvalid)), "{what}");
    }
    // Parts that are not the snapshot's.
    for bad in [vec![0u8; 100], vec![]] {
        let mut r = confirmed(epoch2(), 0);
        r.added_seq = r.sim.seq() + 1;
        let ev = r.registered();
        r.post_d(ev);
        r.send_d();
        aborts(&r.send_snapshot(Some(bad)), SealError::BundleInvalid);
    }
    // `dev_E` revoked by the time of the snapshot.
    let mut r = confirmed(epoch2(), 0);
    r.sim.revoke(Who::D(1), 0);
    r.take_snapshot();
    r.added_seq = r.sim.seq() + 1;
    let ev = r.registered();
    r.post_d(ev);
    r.send_d();
    aborts(&r.send_snapshot(None), SealError::BundleInvalid);
    // The unedited snapshot passes.
    let mut r = confirmed(epoch2(), 0);
    r.added_seq = r.sim.seq() + 1;
    let ev = r.registered();
    let r = to_joining(r, ev);
    assert_eq!(r.snap.as_ref().unwrap().roots.len(), 2);
}

/// §17.9 step 6: every entry from `h + 1` classified in seq order from
/// `M_h` and `C_h`: `Skip` and `Refuse` continue; `Tamper` aborts; a gap
/// or another order aborts; all with `seal/bundle_invalid`.
// spec: SEAL-PAIR-011
#[test]
fn n_classifies_the_log() {
    // Skip and Refuse before the `DeviceAdded` continue.
    let mut r = confirmed(Sim::new(650, 2), 0);
    let junk = vec![0u8; 700];
    r.sim.push(junk);
    let e = r.sim.epoch();
    let f = r.sim.cmd(Who::D(1), &Spec { epoch: Some(e + 1), root: Some([1; 32]), ..Spec::default() });
    let rc = r.sim.push(f);
    assert!(matches!(rc.class, Class::Refuse));
    r.append_added(|_| {}, &Spec::default());
    let ev = r.registered();
    let mut r = to_joining(r, ev);
    joined(r.send_log());

    type Edit = fn(&Run, Vec<(u64, Vec<u8>)>) -> Vec<(u64, Vec<u8>)>;
    let edits: Vec<(&str, Edit)> = vec![
        ("a replayed frame", |r, mut l| {
            let h = r.snap.as_ref().unwrap().h as usize;
            l.insert(0, (h as u64 + 1, r.sim.frames[h - 1].clone()));
            for (i, e) in l.iter_mut().enumerate() {
                e.0 = h as u64 + 1 + i as u64;
            }
            l
        }),
        ("a gap", |_, mut l| {
            l.remove(0);
            l
        }),
        ("another order", |_, mut l| {
            l.swap(0, 1);
            l
        }),
        ("a seq out of place", |_, mut l| {
            l[0].0 += 1;
            l
        }),
    ];
    for (what, edit) in edits {
        let mut r = confirmed(Sim::new(651, 2), 0);
        let f = r.sim.cmd(Who::D(1), &Spec::default());
        r.sim.accept(f);
        r.append_added(|_| {}, &Spec::default());
        let ev = r.registered();
        let mut r = to_joining(r, ev);
        let log = edit(&r, r.log());
        let fx = r.n_on(NewEvent::Entries { frames: log });
        assert_eq!((names_n(&fx), n_abort(&fx)), (vec!["Abort"], Some(SealError::BundleInvalid)), "{what}");
    }
}

/// A `Skip` after `h` whose target is at or below `h`: N asks the relay
/// for the frame (`FetchFrames`), checks its seq and `frame_hash`, and
/// classifies the entry again; a target above `h` is taken from the entries
/// read, with no request. A frame not served, served with another seq or
/// another hash, or a second `Entries`, aborts with `seal/bundle_invalid`.
// spec: SEAL-PAIR-011
#[test]
fn n_resolves_need_frames() {
    let setup = |seed: u64, below: bool| -> (Run, u64) {
        let mut sim = Sim::new(seed, 2);
        let f = sim.cmd(Who::D(1), &Spec::default());
        sim.accept(f);
        let mut r = confirmed(sim, 0);
        let t = if below {
            r.snap.as_ref().unwrap().h
        } else {
            let f = r.sim.cmd(Who::D(1), &Spec::default());
            r.sim.accept(f);
            r.sim.seq()
        };
        let fh = frame_hash(&r.sim.frames[t as usize - 1]);
        let f = r.sim.ctl(Who::D(0), vec![CtlItem::Skip(SkipItem { seq: t, frame_hash: fh })], &Spec::default());
        let o = Opts { targets: vec![t], ..Opts::default() };
        let rc = r.sim.classify(&f, &o).unwrap();
        assert!(matches!(rc.class, Class::Accept), "{:?}", rc.code);
        r.sim.commit(f, rc);
        r.append_added(|_| {}, &Spec::default());
        let ev = r.registered();
        (to_joining(r, ev), t)
    };
    // Above `h`: no request.
    let (mut r, _) = setup(660, false);
    joined(r.send_log());
    // At `h`: one request, then joined.
    let (mut r, t) = setup(661, true);
    let fx = r.send_log();
    let [NewEffect::FetchFrames { token, seqs }] = &fx[..] else { panic!("{:?}", names_n(&fx)) };
    assert_eq!((token.as_str(), seqs.as_slice()), (TOKEN, &[t][..]));
    let frame = r.sim.frames[t as usize - 1].clone();
    let (device_no, _, _, _) = joined(r.n_on(NewEvent::Frames { frames: vec![(t, frame.clone())] }));
    assert_eq!(device_no, 3);
    // Served wrong.
    let mut other = frame.clone();
    other[400] ^= 1;
    let bad: Vec<Vec<(u64, Vec<u8>)>> = vec![
        vec![],
        vec![(t - 1, frame.clone())],
        vec![(t, other)],
        vec![(t, frame.clone()), (t, frame.clone())],
        vec![(t, frame.clone()), (t - 1, r.sim.frames[t as usize - 2].clone())],
    ];
    for frames in bad {
        let (mut r, _) = setup(661, true);
        assert_eq!(names_n(&r.send_log()), ["FetchFrames"]);
        aborts(&r.n_on(NewEvent::Frames { frames }), SealError::BundleInvalid);
    }
    let (mut r, _) = setup(661, true);
    r.send_log();
    aborts(&r.send_log(), SealError::BundleInvalid);
}

/// §17.9 step 7: the entry at `added_seq` is accepted, authored by `dev_E`,
/// based on `h`, and holds one `DeviceAdded` naming exactly N's keys, name
/// and `T` (and `bootstrap_seq = h`); otherwise `seal/device_added_mismatch`.
/// Step 8: `device_no` is what that entry assigned.
// spec: SEAL-PAIR-010
// spec: SEAL-PAIR-011
// spec: SEAL-PAIR-012
#[test]
fn n_checks_the_device_added() {
    type Edit = fn(&mut Run);
    let edits: Vec<(&str, Edit)> = vec![
        ("another name", |r| {
            r.append_added(|d| d.name = "Phone 2".into(), &Spec::default());
        }),
        ("another transcript", |r| {
            r.append_added(|d| d.transcript[0] ^= 1, &Spec::default());
        }),
        ("another signing key", |r| {
            r.append_added(|d| d.sign_pk = [0x5A; 32], &Spec::default());
        }),
        ("another X25519 key", |r| {
            r.append_added(|d| d.hpke_pk = [0x5B; 32], &Spec::default());
        }),
        ("another bootstrap_seq", |r| {
            r.append_added(|d| d.bootstrap_seq -= 1, &Spec::default());
        }),
        ("based on another seq", |r| {
            let h = r.snap.as_ref().unwrap().h;
            let b = Head { seq: h - 1, chain: r.sim.chains[h as usize - 1] };
            r.append_added(|_| {}, &Spec { based_on: Some(b), ..Spec::default() });
        }),
        ("by another device", |r| {
            let mut item = r.device.clone().unwrap();
            item.bootstrap_seq = r.snap.as_ref().unwrap().h;
            let f = r.sim.ctl(Who::D(1), vec![CtlItem::DeviceAdded(item)], &Spec::default());
            r.sim.accept(f);
            r.added_seq = r.sim.seq();
        }),
        ("not a DeviceAdded", |r| {
            let f = r.sim.cmd(Who::D(0), &Spec::default());
            r.sim.accept(f);
            r.added_seq = r.sim.seq();
        }),
        ("past the end of the log", |r| {
            r.append_added(|_| {}, &Spec::default());
            r.added_seq += 1;
        }),
    ];
    for (what, edit) in edits {
        let mut r = confirmed(Sim::new(670, 3), 0);
        edit(&mut r);
        let ev = r.registered();
        let mut r = to_joining(r, ev);
        let fx = r.send_log();
        assert_eq!((names_n(&fx), n_abort(&fx)), (vec!["Abort"], Some(SealError::DeviceAddedMismatch)), "{what}");
    }
    // Step 8: the number the board assigned, here 4 after three devices
    // and one revoked (numbers are never reused).
    let mut sim = Sim::new(671, 3);
    sim.revoke(Who::D(0), 2);
    let mut r = confirmed(sim, 0);
    r.append_added(|_| {}, &Spec::default());
    let ev = r.registered();
    let mut r = to_joining(r, ev);
    let (device_no, keys, _, _) = joined(r.send_log());
    assert_eq!((device_no, r.sim.m.device_no(&keys.public().dev)), (4, Some(4)));
}

/// A rotation sequenced between `h` and `added_seq`, in either order. If E
/// sealed the `DeviceAdded` before it classified the rotation, the entry is
/// a stale-epoch control entry, refused: step 7 gives `seal/pair_raced`,
/// and so does step 6 when an entry under the new epoch follows (N holds no
/// root for it: `seal/missing_root`). If E classified the rotation first,
/// its engine reports `seal/pair_raced` and E aborts; a `DeviceAdded` under
/// the new epoch would meet `seal/missing_root` on N. A rotation after
/// `added_seq` wraps the root to N, which learns it and joins.
// spec: SEAL-PAIR-011
#[test]
fn a_rotation_racing_the_pairing() {
    for later in [false, true] {
        let mut r = confirmed(Sim::new(680, 2), 0);
        r.sim.rotate(Who::D(1));
        let rc = r.append_added(|_| {}, &Spec { epoch: Some(1), ..Spec::default() });
        assert_eq!((class_name(&rc.class), rc.code), ("Refuse", Some("seal/stale_epoch")));
        if later {
            let f = r.sim.cmd(Who::D(1), &Spec::default());
            r.sim.accept(f);
        }
        let ev = r.registered();
        let mut r = to_joining(r, ev);
        aborts(&r.send_log(), SealError::PairRaced);
    }
    // E classified the rotation first.
    let mut r = confirmed(Sim::new(681, 2), 0);
    let fx = r.e_on(ExistingEvent::RegisterFailed { code: SealError::PairRaced });
    assert_eq!((names_e(&fx), e_abort(&fx)), (vec!["Abort"], Some(SealError::PairRaced)));
    assert!(r.e_on(ExistingEvent::UserConfirmed).is_empty());
    // A `DeviceAdded` sealed under the new epoch, all the same.
    let mut q = confirmed(Sim::new(682, 2), 0);
    q.sim.rotate(Who::D(1));
    q.append_added(|_| {}, &Spec::default());
    let ev = q.registered();
    let mut q = to_joining(q, ev);
    aborts(&q.send_log(), SealError::PairRaced);
    // A rotation after `added_seq`, then an entry under the new epoch.
    let mut r = confirmed(Sim::new(683, 2), 0);
    r.append_added(|_| {}, &Spec::default());
    r.sim.rotate(Who::D(1));
    let f = r.sim.cmd(Who::D(0), &Spec::default());
    r.sim.accept(f);
    let ev = r.registered();
    let mut r = to_joining(r, ev);
    joined(r.send_log());
}

/// Slots replayed under another label or reflected to their sender abort
/// with `seal/slot_mac`: E's own slot A or C as slot B, N's own slot B back
/// to N, and a slot whose label byte the relay rewrote (slot A as C, slot C
/// as D). Slot A again where C is awaited, and slot C again where D is
/// awaited, byte for byte as N accepted them, are ignored (§17.10, ruling
/// Q60; task S7b turned these two round, `s7b_repeats` has the rest).
// spec: SEAL-PAIR-002
// spec: SEAL-PAIR-003
#[test]
fn slots_replayed_or_reflected() {
    for which in 0..2 {
        let mut r = Run::new(Sim::new(690, 1), 0);
        r.step_b();
        let own = if which == 0 { r.slot_a.clone() } else { r.slot_b.clone() };
        let bytes = if which == 0 { own } else { [&[B, ROLE_E][..], &own[2..]].concat() };
        let fx = r.e_on(ExistingEvent::SlotB { bytes });
        assert_eq!(e_abort(&fx), Some(SealError::SlotMac));
    }
    let mut r = Run::new(Sim::new(691, 1), 0);
    r.step_b();
    let bytes = r.slot_a.clone();
    assert!(r.n_on(NewEvent::Slot { bytes }).is_empty());
    let n_cases: Vec<fn(&Run) -> Vec<u8>> = vec![
        |r| r.slot_b.clone(),
        |r| [&[C][..], &r.slot_a[1..]].concat(),
        |r| [&[C, ROLE_E][..], &r.slot_b[2..]].concat(),
    ];
    for case in n_cases {
        let mut r = Run::new(Sim::new(691, 1), 0);
        r.step_b();
        let bytes = case(&r);
        aborts(&r.n_on(NewEvent::Slot { bytes }), SealError::SlotMac);
    }
    // Where slot D is awaited: slot C again, and slot C relabelled D.
    for relabel in [false, true] {
        let mut r = Run::new(Sim::new(692, 1), 0);
        r.step_b().step_c().step_sas();
        let fx = r.n_on(NewEvent::SasEntered { digits: r.digits.clone() });
        assert_eq!(names_n(&fx), ["ShowAccepted", "FetchSlot"]);
        if relabel {
            let bytes = [&[D][..], &r.slot_c[1..]].concat();
            aborts(&r.n_on(NewEvent::Slot { bytes }), SealError::SlotMac);
        } else {
            let bytes = r.slot_c.clone();
            assert!(r.n_on(NewEvent::Slot { bytes }).is_empty());
        }
    }
}
