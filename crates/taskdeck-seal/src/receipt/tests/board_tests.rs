//! A board simulator for the unit tests of `classify`: a recovery key, a
//! genesis, devices, epoch roots and the log, built with the public sealing
//! functions (and, for frames the public sealer refuses to make, the private
//! pieces it is made of), classified one seq at a time.

use std::collections::BTreeMap;

use chacha20poly1305::aead::{Aead as _, KeyInit as _, Payload};

use crate::entries::{FRAME_OVERHEAD, HEADER_LEN, MAGIC, entry_ad, pad_plaintext};
use crate::membership::DeviceStatus;
use crate::signatures::{sign_with, signer_identity};
use crate::*;

pub fn rand<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    OsEntropy.fill(&mut b).unwrap();
    b
}

/// A copy of a root (roots are not `Clone`).
pub fn copy(r: &EpochRoot) -> EpochRoot {
    EpochRoot::from_bytes(*r.to_bytes())
}

#[derive(Clone, Copy, Debug)]
pub enum Who {
    /// The device at this index of `Board::devs`.
    D(usize),
    /// The current recovery identity (the last of `Board::rec`).
    R,
    /// An earlier recovery identity, by index of `Board::rec`.
    OldR(usize),
}

/// Overrides of the fields a frame would get by default.
#[derive(Default, Clone)]
pub struct Spec {
    pub epoch: Option<u32>,
    pub ctr: Option<u64>,
    pub at_ms: Option<i64>,
    pub based_on: Option<Head>,
    pub root: Option<[u8; 32]>,
}

/// Overrides of the classification inputs.
#[derive(Default)]
pub struct Opts {
    pub own: Option<usize>,
    pub supported: Option<Vec<u32>>,
    pub retained_from: Option<u64>,
    pub hide_frames: Vec<u64>,
    pub hide_roots: Vec<u32>,
    pub no_chain_at: bool,
    /// Replaces the root the receiver holds for an epoch.
    pub wrong_root: Option<u32>,
    /// Classifies at this seq instead of the next.
    pub seq: Option<u64>,
}

pub struct Board {
    pub rec: Vec<RecoveryKeys>,
    pub salt: [u8; 16],
    pub board: BoardId,
    pub devs: Vec<DeviceKeys>,
    pub roots: BTreeMap<u32, EpochRoot>,
    pub m: Membership,
    /// `chains[i]` is `C_i`.
    pub chains: Vec<Chain>,
    /// `frames[i]` is the frame at seq `i + 1`.
    pub frames: Vec<Vec<u8>>,
    pub now: i64,
    pub sealed: BTreeMap<(DevId, u64), FrameHash>,
    /// The root of the last rotation built, stored once a rotation is accepted.
    pub staged: Option<(u32, [u8; 32])>,
}

/// A padded plaintext of §10.1 built field by field (the sealer refuses
/// some of the values the tests need).
pub fn padded(kind: Kind, at_ms: i64, based_on: Head, payload: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&rand::<16>());
    v.push(kind as u8);
    v.extend_from_slice(&at_ms.to_be_bytes());
    v.extend_from_slice(&based_on.seq.to_be_bytes());
    v.extend_from_slice(&based_on.chain.0);
    v.extend_from_slice(&u32::try_from(payload.len()).unwrap().to_be_bytes());
    v.extend_from_slice(payload);
    v.resize(padded_len(v.len() as u64) as usize, 0);
    v
}

pub const T0: i64 = 1_767_225_600_000;

impl Board {
    /// A board with `n` current devices: Genesis with device 0, then
    /// `DeviceAdded` by device 0 for the others.
    pub fn new(n: usize) -> Board {
        let rk = RecoverySeed::generate(&mut OsEntropy).unwrap().keys();
        let salt = rand::<16>();
        let board = board_id(&rk.public().sign_pk, &salt);
        let mut b = Board {
            rec: vec![rk],
            salt,
            board,
            devs: vec![DeviceKeys::generate(&mut OsEntropy).unwrap()],
            roots: BTreeMap::new(),
            m: Membership::empty(),
            chains: vec![chain_zero(&board)],
            frames: Vec::new(),
            now: T0,
            sealed: BTreeMap::new(),
            staged: None,
        };
        let r1 = rand::<32>();
        b.roots.insert(1, EpochRoot::from_bytes(r1));
        let g = b.genesis_item();
        let f = b.ctl(Who::R, vec![CtlItem::Genesis(g)], &Spec::default());
        let r = b.classify_genesis(&f, &b.anchor());
        assert_eq!(r.code, None);
        assert!(matches!(r.class, Class::Accept));
        b.commit(f, &r);
        for _ in 1..n {
            b.add_device();
        }
        b
    }

    pub fn genesis_item(&self) -> Genesis {
        let root = &self.roots[&1];
        let commit = root.commitment(&self.board, 1);
        let rp = self.rec[0].public();
        let d = self.devs[0].public();
        let mut rc = [(d.dev, d.hpke_pk), (rp.dev(&self.board), rp.hpke_pk)];
        rc.sort();
        let wraps =
            rc.iter().map(|(id, pk)| wrap_root(root, &self.board, 1, &commit, *id, pk, &mut OsEntropy).unwrap()).collect();
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

    pub fn anchor(&self) -> GenesisAnchor {
        GenesisAnchor { rsign_pk: self.rec[0].public().sign_pk, salt: self.salt, root1: copy(&self.roots[&1]) }
    }

    pub fn classify_genesis(&self, frame: &[u8], anchor: &GenesisAnchor) -> Receipt {
        let roots = |_: u32| None;
        let chain_at = |_: u64| None;
        let skip = |_: u64| None;
        let input = ReceiptInput {
            seq: 1,
            frame,
            prev_chain: chain_zero(&self.board),
            membership: &self.m,
            roots: &roots,
            chain_at: &chain_at,
            retained_from: 1,
            own: None,
            supported_rules: &[1],
            genesis: Some(anchor),
            skip_targets: &skip,
        };
        classify(&input).unwrap()
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

    pub fn dev_pub(&self, i: usize) -> DevicePublic {
        self.devs[i].public()
    }

    pub fn next_ctr(&self, who: Who) -> u64 {
        let dev = self.dev(who);
        if let Some(d) = self.m.device(&dev) {
            return d.last_ctr + 1;
        }
        self.m.recovery_record(&dev).map_or(1, |r| r.last_ctr + 1)
    }

    pub fn epoch(&self) -> u32 {
        self.m.epoch.max(1)
    }

    fn fill(&self, who: Who, sp: &Spec) -> (u64, u32, EpochRoot, i64, Head) {
        let ctr = sp.ctr.unwrap_or_else(|| self.next_ctr(who));
        let epoch = sp.epoch.unwrap_or(self.epoch());
        let root = match sp.root {
            Some(r) => EpochRoot::from_bytes(r),
            None => copy(&self.roots[&epoch]),
        };
        (ctr, epoch, root, sp.at_ms.unwrap_or(self.now), sp.based_on.unwrap_or(self.head()))
    }

    /// Seals `plain` as `who`'s frame and records its hash as sealed.
    pub fn seal(&mut self, who: Who, plain: &Plain, ctr: u64, epoch: u32, root: &EpochRoot) -> Vec<u8> {
        let key = derive_epoch_key(root, &self.board, epoch, KeyKind::Entry);
        let f = seal_frame(&self.signer(who), &self.board, ctr, epoch, &key, plain, &mut OsEntropy).unwrap();
        self.sealed.insert((self.dev(who), ctr), frame_hash(&f));
        f
    }

    /// A `cmd`, `beat` or `snap` frame with the given payload.
    pub fn plain(&mut self, who: Who, kind: Kind, payload: Vec<u8>, sp: &Spec) -> Vec<u8> {
        let (ctr, epoch, root, at_ms, based_on) = self.fill(who, sp);
        let plain = Plain { entry_id: EntryId(rand()), kind, at_ms, based_on, payload };
        self.seal(who, &plain, ctr, epoch, &root)
    }

    pub fn cmd(&mut self, who: Who, sp: &Spec) -> Vec<u8> {
        self.plain(who, Kind::Cmd, b"{\"op\":\"x\"}".to_vec(), sp)
    }

    pub fn beat(&mut self, who: Who, rules: Vec<u32>, sp: &Spec) -> Vec<u8> {
        let beat = Beat { rules, envelopes: vec![(1, 1)], hash_versions: vec![1], state: None, acks: vec![] };
        self.plain(who, Kind::Beat, beat_payload(&beat).unwrap(), sp)
    }

    /// A control frame with these items (refused by `ctl_payload` when they
    /// form no shape: use `ctl_raw` for those).
    pub fn ctl(&mut self, who: Who, items: Vec<CtlItem>, sp: &Spec) -> Vec<u8> {
        let (ctr, epoch, root, at_ms, based_on) = self.fill(who, sp);
        let entry_id = EntryId(rand());
        let payload = ctl_payload(&items, &self.signer(who), &self.board, ctr, &entry_id, at_ms).unwrap();
        let plain = Plain { entry_id, kind: Kind::Ctl, at_ms, based_on, payload };
        self.seal(who, &plain, ctr, epoch, &root)
    }

    /// A control frame whose payload is built without the shape check, and
    /// whose inner signature is by `inner` (normally the author).
    pub fn ctl_raw(&mut self, who: Who, inner: Who, items: Vec<CtlItem>, sp: &Spec) -> Vec<u8> {
        let (ctr, epoch, root, at_ms, based_on) = self.fill(who, sp);
        let entry_id = EntryId(rand());
        let (value, bytes) = crate::payloads::items_bytes(&items).unwrap();
        let dev = self.dev(who);
        let inner_bytes = crate::payloads::ctl_inner(&self.board, &dev, ctr, &entry_id, at_ms, &bytes);
        let (_, sk) = signer_identity(&self.signer(inner), &self.board).unwrap();
        let sig = sign_with(sk, &[&inner_bytes]);
        let payload = crate::canon::encode(&crate::payloads::ctl_value(value, &sig)).unwrap();
        let plain = Plain { entry_id, kind: Kind::Ctl, at_ms, based_on, payload };
        self.seal(who, &plain, ctr, epoch, &root)
    }

    /// A control frame whose `items` value is `items` exactly as given (no
    /// table or shape check), with a valid inner signature by the author
    /// over its bytes.
    pub fn ctl_json(&mut self, who: Who, items: serde_json::Value, sp: &Spec) -> Vec<u8> {
        let (ctr, epoch, root, at_ms, based_on) = self.fill(who, sp);
        let entry_id = EntryId(rand());
        let bytes = crate::canon::encode(&items).unwrap();
        let dev = self.dev(who);
        let inner_bytes = crate::payloads::ctl_inner(&self.board, &dev, ctr, &entry_id, at_ms, &bytes);
        let (_, sk) = signer_identity(&self.signer(who), &self.board).unwrap();
        let sig = sign_with(sk, &[&inner_bytes]);
        let payload = crate::canon::encode(&crate::payloads::ctl_value(items, &sig)).unwrap();
        let plain = Plain { entry_id, kind: Kind::Ctl, at_ms, based_on, payload };
        self.seal(who, &plain, ctr, epoch, &root)
    }

    /// A frame with any `ver`, `suite` and padded plaintext, signed by `who`.
    #[allow(clippy::too_many_arguments)]
    pub fn raw(&self, who: Who, ctr: u64, ver: u8, suite: u8, epoch: u32, root: &EpochRoot, padded: &[u8]) -> Vec<u8> {
        let (dev, sk) = signer_identity(&self.signer(who), &self.board).unwrap();
        let len = u32::try_from(30 + padded.len() + 16).unwrap();
        let mut header = [0u8; HEADER_LEN];
        header[..4].copy_from_slice(MAGIC);
        header[4..20].copy_from_slice(&self.board.0);
        header[20..36].copy_from_slice(&dev.0);
        header[36..44].copy_from_slice(&ctr.to_be_bytes());
        header[44..48].copy_from_slice(&len.to_be_bytes());
        let key = derive_epoch_key(root, &self.board, epoch, KeyKind::Entry);
        let nonce: [u8; 24] = rand();
        let ad = entry_ad(&header, ver, suite, epoch);
        let ct = chacha20poly1305::XChaCha20Poly1305::new_from_slice(key.as_bytes())
            .unwrap()
            .encrypt(&chacha20poly1305::XNonce::from(nonce), Payload { msg: padded, aad: &ad })
            .unwrap();
        let mut f = Vec::with_capacity(FRAME_OVERHEAD + len as usize);
        f.extend_from_slice(&header);
        f.extend_from_slice(&[ver, suite]);
        f.extend_from_slice(&epoch.to_be_bytes());
        f.extend_from_slice(&nonce);
        f.extend_from_slice(&ct);
        let sig = sign_with(sk, &[ctx::FRAME, &f]);
        f.extend_from_slice(&sig);
        f
    }

    /// The padded plaintext of a `cmd` at the board's head.
    pub fn padded_cmd(&self) -> Vec<u8> {
        let plain =
            Plain { entry_id: EntryId(rand()), kind: Kind::Cmd, at_ms: self.now, based_on: self.head(), payload: b"x".to_vec() };
        pad_plaintext(&self.board, &plain).unwrap()
    }

    pub fn classify(&self, frame: &[u8]) -> Receipt {
        self.classify_with(frame, &Opts::default()).unwrap()
    }

    pub fn classify_with(&self, frame: &[u8], o: &Opts) -> Result<Receipt, SealError> {
        let seq = o.seq.unwrap_or(self.seq() + 1);
        let wrong = EpochRoot::from_bytes([0xEE; 32]);
        let roots = |e: u32| {
            if o.hide_roots.contains(&e) {
                None
            } else if o.wrong_root == Some(e) {
                Some(&wrong)
            } else {
                self.roots.get(&e)
            }
        };
        let chain_at = |b: u64| if o.no_chain_at { None } else { self.chains.get(b as usize).copied() };
        let skip = |t: u64| {
            if o.hide_frames.contains(&t) || t == 0 {
                None
            } else {
                self.frames.get(t as usize - 1).map(|f| f.as_slice())
            }
        };
        let own_dev = o.own.map(|i| self.devs[i].public().dev);
        let sealed = move |ctr: u64| own_dev.and_then(|d| self.sealed.get(&(d, ctr)).copied());
        let supported = o.supported.clone().unwrap_or_else(|| vec![1]);
        let prev = seq.checked_sub(1).and_then(|p| self.chains.get(p as usize)).copied().unwrap_or(*self.chains.last().unwrap());
        let input = ReceiptInput {
            seq,
            frame,
            prev_chain: prev,
            membership: &self.m,
            roots: &roots,
            chain_at: &chain_at,
            retained_from: o.retained_from.unwrap_or(1),
            own: o.own.map(|i| OwnIdentity { keys: &self.devs[i], sealed: &sealed }),
            supported_rules: &supported,
            genesis: None,
            skip_targets: &skip,
        };
        classify(&input)
    }

    /// Records a classified frame at the next seq.
    pub fn commit(&mut self, frame: Vec<u8>, r: &Receipt) {
        let old = self.m.epoch;
        self.chains.push(r.chain);
        self.frames.push(frame);
        self.m = r.membership.dup();
        if self.m.epoch > old
            && let Some((e, root)) = self.staged.take()
        {
            self.roots.insert(e, EpochRoot::from_bytes(root));
        }
        self.now += 1000;
    }

    /// Classifies a frame at the next seq and records it.
    pub fn push(&mut self, frame: Vec<u8>) -> Receipt {
        let r = self.classify(&frame);
        self.commit(frame, &r);
        r
    }

    /// Pushes a frame that must be accepted without a halt.
    pub fn accept(&mut self, frame: Vec<u8>) -> Receipt {
        let r = self.push(frame);
        assert!(matches!(r.class, Class::Accept), "not accepted: {:?}", r.code);
        assert_eq!(r.code, None);
        r
    }

    /// Adds a new device by `DeviceAdded` from device 0; returns its index.
    pub fn add_device(&mut self) -> usize {
        self.add_device_by(Who::D(0))
    }

    pub fn add_device_by(&mut self, who: Who) -> usize {
        let k = DeviceKeys::generate(&mut OsEntropy).unwrap();
        let item = self.added_item(&k);
        self.devs.push(k);
        let f = self.ctl(who, vec![item], &Spec::default());
        self.accept(f);
        self.devs.len() - 1
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

    /// The recipients of a rotation after `prefix` (revocations,
    /// reinstatements, a recovery-key change) applied to the current state.
    pub fn recipients_after(&self, prefix: &[CtlItem]) -> Vec<(DevId, [u8; 32])> {
        let mut out: Vec<(DevId, [u8; 32])> = Vec::new();
        for d in &self.m.devices {
            let revoked = prefix.iter().any(|i| matches!(i, CtlItem::DeviceRevoked(t) if *t == d.dev));
            let reinstated = prefix.iter().any(|i| matches!(i, CtlItem::DeviceReinstated(t) if *t == d.dev));
            let current = matches!(d.status, DeviceStatus::Current);
            if (current && !revoked) || reinstated {
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
        match new_rec {
            Some(r) => out.push(r),
            None => {
                let r = self.m.current_recovery();
                out.push((r.dev, r.hpke_pk));
            }
        }
        out.sort();
        out
    }

    /// A rotation to `epoch` wrapping a fresh root to `recipients`; the root
    /// is staged for `commit`.
    pub fn rotation_to(&mut self, epoch: u32, recipients: &[(DevId, [u8; 32])], purge: Option<u64>) -> EpochRotated {
        let bytes = rand::<32>();
        let root = EpochRoot::from_bytes(bytes);
        let commit = root.commitment(&self.board, epoch);
        let mut rc = recipients.to_vec();
        rc.sort();
        let wraps = rc
            .iter()
            .map(|(id, pk)| wrap_root(&root, &self.board, epoch, &commit, *id, pk, &mut OsEntropy).unwrap())
            .collect();
        self.staged = Some((epoch, bytes));
        EpochRotated { epoch, commit, wraps, purge_before: None }.with_purge(purge)
    }

    /// `prefix` followed by a rotation to the next epoch with the right
    /// recipients.
    pub fn rotate_items(&mut self, prefix: Vec<CtlItem>) -> Vec<CtlItem> {
        let rc = self.recipients_after(&prefix);
        let r = self.rotation_to(self.m.epoch + 1, &rc, None);
        let mut items = prefix;
        items.push(CtlItem::EpochRotated(r));
        items
    }

    /// Revokes device `t` by `who` (with a rotation); must be accepted.
    pub fn revoke(&mut self, who: Who, t: usize) {
        let items = self.rotate_items(vec![CtlItem::DeviceRevoked(self.dev(Who::D(t)))]);
        let f = self.ctl(who, items, &Spec::default());
        self.accept(f);
    }

    /// Rotates by `who`; must be accepted.
    pub fn rotate(&mut self, who: Who) {
        let items = self.rotate_items(vec![]);
        let f = self.ctl(who, items, &Spec::default());
        self.accept(f);
    }
}

trait WithPurge {
    fn with_purge(self, p: Option<u64>) -> Self;
}

impl WithPurge for EpochRotated {
    fn with_purge(mut self, p: Option<u64>) -> Self {
        self.purge_before = p;
        self
    }
}

pub fn class_name(r: &Receipt) -> &'static str {
    match r.class {
        Class::Skip => "Skip",
        Class::Tamper => "Tamper",
        Class::Refuse => "Refuse",
        Class::Accept => "Accept",
        Class::NeedFrames(_) => "NeedFrames",
    }
}

/// Asserts the class and code.
#[track_caller]
pub fn expect(r: &Receipt, class: &str, code: Option<&str>) {
    assert_eq!((class_name(r), r.code), (class, code));
}
