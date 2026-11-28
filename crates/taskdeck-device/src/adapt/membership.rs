//! The sealer adapter's membership value (engine.md §6.1, §6.2; seal.md §13).
//!
//! `taskdeck_seal::Membership` is not `Clone` and exposes only `board`,
//! `epoch`, `rules`, `devices`, `current_recovery` and `device_no` (seal.md
//! §26); the engine needs a `Clone` value and the whole view of §6.2. So the
//! adapter holds the seal value behind an `Arc` (a clone is a reference
//! count) with its canonical bytes, and reads every other field of §13.1
//! from those bytes with the parser below (finding f1-1 in
//! `docs/sync/status/device/fit.md`). The bytes are always ones
//! `Membership::from_canonical_bytes` accepted or `to_canonical_bytes`
//! produced, so this parser never has to judge a state; it still checks
//! every length before it reads (plan §3.4).

use std::sync::{Arc, OnceLock};

use taskdeck_seal::{Membership, RecoveryPublic, key_fingerprint};
use taskdeck_sync::{DeviceStatus, DeviceView, MembershipView, RecoveryView};

/// A membership state as the engine holds it (`Sealer::Membership`).
#[derive(Clone)]
pub struct SealMembership(Arc<Inner>);

struct Inner {
    m: Membership,
    bytes: Vec<u8>,
    fields: OnceLock<Option<Fields>>,
}

impl SealMembership {
    /// Wraps a state `taskdeck-seal` produced or parsed.
    pub(crate) fn new(m: Membership) -> Self {
        let bytes = m.to_canonical_bytes();
        SealMembership(Arc::new(Inner { m, bytes, fields: OnceLock::new() }))
    }

    /// The seal value.
    pub(crate) fn seal(&self) -> &Membership {
        &self.0.m
    }

    /// The canonical bytes (seal.md §13.2).
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.0.bytes
    }

    /// Every field of seal.md §13.1, read from the canonical bytes.
    pub(crate) fn fields(&self) -> Option<&Fields> {
        self.0.fields.get_or_init(|| Fields::parse(&self.0.bytes)).as_ref()
    }

    /// `c_e`, the commitment the state records for epoch `e` (seal.md §13.8).
    pub(crate) fn commitment(&self, e: u32) -> Option<[u8; 32]> {
        self.fields()?.commitments.iter().find(|c| c.0 == e).map(|c| c.2)
    }

    /// The read-only view of engine.md §6.2.
    pub(crate) fn view(&self) -> MembershipView {
        self.fields().map(Fields::view).unwrap_or_default()
    }
}

/// One device record of seal.md §13.2.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DevRec {
    pub dev: [u8; 16],
    pub sign_pk: [u8; 32],
    pub hpke_pk: [u8; 32],
    pub device_no: u16,
    /// 1 current, 2 revoked.
    pub status: u8,
    pub added_seq: u64,
    pub added_by: [u8; 16],
    pub bootstrap_seq: u64,
    pub status_seq: u64,
    pub status_by: [u8; 16],
    pub last_ctr: u64,
    pub beat_ms: i64,
    pub rules: Vec<u32>,
    pub envelopes: Vec<(u8, u8)>,
    pub hash_versions: Vec<u32>,
    pub name: String,
}

/// One recovery record of seal.md §13.2.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RecRec {
    pub dev: [u8; 16],
    pub sign_pk: [u8; 32],
    pub hpke_pk: [u8; 32],
    pub since_seq: u64,
    pub until_seq: u64,
    pub last_ctr: u64,
}

/// Every field of a membership state, in the order of seal.md §13.2. The
/// default is `M_0` (seal.md §13.3: every field zero, every list empty).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Fields {
    pub board_id: [u8; 16],
    pub salt: [u8; 16],
    pub rules: u32,
    pub epoch: u32,
    pub epoch_seq: u64,
    pub epoch_clock_ms: i64,
    pub clock_ms: i64,
    pub home_gen: u32,
    pub home_seq: u64,
    pub home_host: String,
    pub home_door: [u8; 5],
    pub purge_seq: u64,
    pub next_device_no: u16,
    /// `(e, seq, c_e)`, ascending by `e`.
    pub commitments: Vec<(u32, u64, [u8; 32])>,
    pub recovery: Vec<RecRec>,
    pub devices: Vec<DevRec>,
    /// `(rules, voter, seq)`.
    pub pending_override: Option<(u32, [u8; 16], u64)>,
    /// `(at_seq, target, hash_version, state_hash)`.
    pub last_reset: Option<(u64, u64, u32, [u8; 32])>,
    pub skips: Vec<(u64, [u8; 32])>,
    pub ctl_seqs: Vec<u64>,
}

/// A bounds-checked reader (every length is checked before it is used).
struct Reader<'a> {
    b: &'a [u8],
}

impl<'a> Reader<'a> {
    fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.b.len() < n {
            return None;
        }
        let (head, rest) = self.b.split_at(n);
        self.b = rest;
        Some(head)
    }
    fn arr<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.bytes(N)?.try_into().ok()
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.arr::<1>()?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes(self.arr()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.arr()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_be_bytes(self.arr()?))
    }
    fn i64(&mut self) -> Option<i64> {
        Some(i64::from_be_bytes(self.arr()?))
    }
    fn text8(&mut self) -> Option<String> {
        let n = usize::from(self.u8()?);
        String::from_utf8(self.bytes(n)?.to_vec()).ok()
    }
    /// A count whose items take at least `min_len` bytes each, checked
    /// against what remains before anything is allocated.
    fn count(&self, n: usize, min_len: usize) -> Option<usize> {
        (n.checked_mul(min_len)? <= self.b.len()).then_some(n)
    }
    fn flag(&mut self) -> Option<bool> {
        match self.u8()? {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }
}

/// The fixed part of a device record (seal.md §13.2).
const DEVICE_FIXED: usize = 155;

impl Fields {
    /// Reads canonical membership bytes (seal.md §13.2); `None` on any
    /// layout fault. It does not judge the state (seal's parser does).
    pub(crate) fn parse(bytes: &[u8]) -> Option<Fields> {
        let mut r = Reader { b: bytes };
        if r.bytes(4)? != b"TDM1" || r.u32()? != 1 {
            return None;
        }
        let mut f = Fields {
            board_id: r.arr()?,
            salt: r.arr()?,
            rules: r.u32()?,
            epoch: r.u32()?,
            epoch_seq: r.u64()?,
            epoch_clock_ms: r.i64()?,
            clock_ms: r.i64()?,
            home_gen: r.u32()?,
            home_seq: r.u64()?,
            home_host: r.text8()?,
            home_door: r.arr()?,
            purge_seq: r.u64()?,
            next_device_no: r.u16()?,
            ..Fields::default()
        };
        let n = r.u32()?;
        let n = r.count(usize::try_from(n).ok()?, 44)?;
        f.commitments.reserve(n);
        for _ in 0..n {
            f.commitments.push((r.u32()?, r.u64()?, r.arr()?));
        }
        let n = r.u16()?;
        let n = r.count(usize::from(n), 104)?;
        f.recovery.reserve(n);
        for _ in 0..n {
            f.recovery.push(RecRec {
                dev: r.arr()?,
                sign_pk: r.arr()?,
                hpke_pk: r.arr()?,
                since_seq: r.u64()?,
                until_seq: r.u64()?,
                last_ctr: r.u64()?,
            });
        }
        let n = r.u16()?;
        let n = r.count(usize::from(n), DEVICE_FIXED + 4)?;
        f.devices.reserve(n);
        for _ in 0..n {
            let mut d = DevRec {
                dev: r.arr()?,
                sign_pk: r.arr()?,
                hpke_pk: r.arr()?,
                device_no: r.u16()?,
                status: r.u8()?,
                added_seq: r.u64()?,
                added_by: r.arr()?,
                bootstrap_seq: r.u64()?,
                status_seq: r.u64()?,
                status_by: r.arr()?,
                last_ctr: r.u64()?,
                beat_ms: r.i64()?,
                rules: Vec::new(),
                envelopes: Vec::new(),
                hash_versions: Vec::new(),
                name: String::new(),
            };
            let k = r.u8()?;
            let k = r.count(usize::from(k), 4)?;
            for _ in 0..k {
                d.rules.push(r.u32()?);
            }
            let k = r.u8()?;
            let k = r.count(usize::from(k), 2)?;
            for _ in 0..k {
                d.envelopes.push((r.u8()?, r.u8()?));
            }
            let k = r.u8()?;
            let k = r.count(usize::from(k), 4)?;
            for _ in 0..k {
                d.hash_versions.push(r.u32()?);
            }
            d.name = r.text8()?;
            f.devices.push(d);
        }
        if r.flag()? {
            f.pending_override = Some((r.u32()?, r.arr()?, r.u64()?));
        }
        if r.flag()? {
            f.last_reset = Some((r.u64()?, r.u64()?, r.u32()?, r.arr()?));
        }
        let n = r.u32()?;
        let n = r.count(usize::try_from(n).ok()?, 40)?;
        f.skips.reserve(n);
        for _ in 0..n {
            f.skips.push((r.u64()?, r.arr()?));
        }
        let n = r.u32()?;
        let n = r.count(usize::try_from(n).ok()?, 8)?;
        f.ctl_seqs.reserve(n);
        for _ in 0..n {
            f.ctl_seqs.push(r.u64()?);
        }
        r.b.is_empty().then_some(f)
    }

    /// The canonical bytes of these fields (seal.md §13.2). Used only to
    /// build a state that seal then parses and judges itself
    /// (`Membership::from_canonical_bytes`): `M_1` before seq 1 is sealed,
    /// and the probe of `open_own`.
    pub(crate) fn encode(&self) -> Option<Vec<u8>> {
        let mut o = Vec::with_capacity(256 + 300 * self.devices.len() + 8 * self.ctl_seqs.len());
        o.extend_from_slice(b"TDM1");
        o.extend_from_slice(&1u32.to_be_bytes());
        o.extend_from_slice(&self.board_id);
        o.extend_from_slice(&self.salt);
        o.extend_from_slice(&self.rules.to_be_bytes());
        o.extend_from_slice(&self.epoch.to_be_bytes());
        o.extend_from_slice(&self.epoch_seq.to_be_bytes());
        o.extend_from_slice(&self.epoch_clock_ms.to_be_bytes());
        o.extend_from_slice(&self.clock_ms.to_be_bytes());
        o.extend_from_slice(&self.home_gen.to_be_bytes());
        o.extend_from_slice(&self.home_seq.to_be_bytes());
        put_vb8(&mut o, self.home_host.as_bytes())?;
        o.extend_from_slice(&self.home_door);
        o.extend_from_slice(&self.purge_seq.to_be_bytes());
        o.extend_from_slice(&self.next_device_no.to_be_bytes());
        o.extend_from_slice(&u32::try_from(self.commitments.len()).ok()?.to_be_bytes());
        for (e, seq, c) in &self.commitments {
            o.extend_from_slice(&e.to_be_bytes());
            o.extend_from_slice(&seq.to_be_bytes());
            o.extend_from_slice(c);
        }
        o.extend_from_slice(&u16::try_from(self.recovery.len()).ok()?.to_be_bytes());
        for r in &self.recovery {
            o.extend_from_slice(&r.dev);
            o.extend_from_slice(&r.sign_pk);
            o.extend_from_slice(&r.hpke_pk);
            o.extend_from_slice(&r.since_seq.to_be_bytes());
            o.extend_from_slice(&r.until_seq.to_be_bytes());
            o.extend_from_slice(&r.last_ctr.to_be_bytes());
        }
        o.extend_from_slice(&u16::try_from(self.devices.len()).ok()?.to_be_bytes());
        for d in &self.devices {
            o.extend_from_slice(&d.dev);
            o.extend_from_slice(&d.sign_pk);
            o.extend_from_slice(&d.hpke_pk);
            o.extend_from_slice(&d.device_no.to_be_bytes());
            o.push(d.status);
            o.extend_from_slice(&d.added_seq.to_be_bytes());
            o.extend_from_slice(&d.added_by);
            o.extend_from_slice(&d.bootstrap_seq.to_be_bytes());
            o.extend_from_slice(&d.status_seq.to_be_bytes());
            o.extend_from_slice(&d.status_by);
            o.extend_from_slice(&d.last_ctr.to_be_bytes());
            o.extend_from_slice(&d.beat_ms.to_be_bytes());
            o.push(u8::try_from(d.rules.len()).ok()?);
            for v in &d.rules {
                o.extend_from_slice(&v.to_be_bytes());
            }
            o.push(u8::try_from(d.envelopes.len()).ok()?);
            for (ver, suite) in &d.envelopes {
                o.push(*ver);
                o.push(*suite);
            }
            o.push(u8::try_from(d.hash_versions.len()).ok()?);
            for v in &d.hash_versions {
                o.extend_from_slice(&v.to_be_bytes());
            }
            put_vb8(&mut o, d.name.as_bytes())?;
        }
        match &self.pending_override {
            None => o.push(0),
            Some((rules, voter, seq)) => {
                o.push(1);
                o.extend_from_slice(&rules.to_be_bytes());
                o.extend_from_slice(voter);
                o.extend_from_slice(&seq.to_be_bytes());
            }
        }
        match &self.last_reset {
            None => o.push(0),
            Some((at, t, hv, sh)) => {
                o.push(1);
                o.extend_from_slice(&at.to_be_bytes());
                o.extend_from_slice(&t.to_be_bytes());
                o.extend_from_slice(&hv.to_be_bytes());
                o.extend_from_slice(sh);
            }
        }
        o.extend_from_slice(&u32::try_from(self.skips.len()).ok()?.to_be_bytes());
        for (t, fh) in &self.skips {
            o.extend_from_slice(&t.to_be_bytes());
            o.extend_from_slice(fh);
        }
        o.extend_from_slice(&u32::try_from(self.ctl_seqs.len()).ok()?.to_be_bytes());
        for s in &self.ctl_seqs {
            o.extend_from_slice(&s.to_be_bytes());
        }
        Some(o)
    }

    /// The view of engine.md §6.2.
    pub(crate) fn view(&self) -> MembershipView {
        let current = self.recovery.iter().find(|r| r.until_seq == 0);
        MembershipView {
            board_id: self.board_id,
            rules: self.rules,
            epoch: self.epoch,
            epoch_seq: self.epoch_seq,
            epoch_clock_ms: self.epoch_clock_ms,
            clock_ms: self.clock_ms,
            purge_seq: self.purge_seq,
            home_gen: self.home_gen,
            home_seq: self.home_seq,
            home_host: self.home_host.clone(),
            home_door: self.home_door,
            devices: self
                .devices
                .iter()
                .map(|d| DeviceView {
                    dev: d.dev,
                    sign_pk: d.sign_pk,
                    device_no: d.device_no,
                    status: if d.status == 1 { DeviceStatus::Current } else { DeviceStatus::Revoked },
                    name: d.name.clone(),
                    key_fp: key_fingerprint(&d.sign_pk, &d.hpke_pk),
                    added_seq: d.added_seq,
                    added_by: d.added_by,
                    bootstrap_seq: d.bootstrap_seq,
                    status_seq: d.status_seq,
                    status_by: d.status_by,
                    last_ctr: d.last_ctr,
                    beat_ms: d.beat_ms,
                    rules: d.rules.clone(),
                    envelopes: d.envelopes.clone(),
                    hash_versions: d.hash_versions.clone(),
                })
                .collect(),
            recovery_dev: current.map_or([0; 16], |r| r.dev),
            recovery_fp: current.map_or_else(String::new, |r| {
                RecoveryPublic { sign_pk: r.sign_pk, hpke_pk: r.hpke_pk }.fingerprint()
            }),
            recovery: self
                .recovery
                .iter()
                .map(|r| RecoveryView {
                    dev_r: r.dev,
                    sign_pk: r.sign_pk,
                    hpke_pk: r.hpke_pk,
                    since_seq: r.since_seq,
                    until_seq: r.until_seq,
                    last_ctr: r.last_ctr,
                })
                .collect(),
            pending_override: self.pending_override,
            last_reset: self.last_reset.map(|(at, t, _, _)| (at, t)),
            skips: self.skips.clone(),
            ctl_seqs: self.ctl_seqs.clone(),
        }
    }
}

fn put_vb8(o: &mut Vec<u8>, b: &[u8]) -> Option<()> {
    o.push(u8::try_from(b.len()).ok()?);
    o.extend_from_slice(b);
    Some(())
}
