//! The membership state (seal.md §13, §26): the state, its canonical bytes
//! (§13.2) and their strict parser, the genesis state (§13.3), and the
//! transitions of §13.6 (`transitions`), which `receipt::classify` drives.

use crate::encodings::{device_name_is_valid, host_is_valid};
use crate::prim::take;
use crate::{BoardId, DevId, EpochRoot, FrameHash, SealError, board_id};

pub(crate) mod transitions;

/// `"TDM1"` (seal.md §13.2).
const MAGIC: &[u8; 4] = b"TDM1";
/// The layout version (seal.md §13.2).
const VERSION: u32 = 1;
/// Commitments (epochs) recorded (seal.md §23).
pub(crate) const COMMIT_MAX: usize = 65_535;
/// Recovery records (seal.md §23).
pub(crate) const RECOVERY_MAX: usize = 64;
/// Device records per board (seal.md §23).
pub(crate) const DEVICES_MAX: usize = 256;
/// Current devices (seal.md §23).
pub(crate) const CURRENT_MAX: usize = 16;
/// The largest `device_no` (seal.md §13.7, §23).
pub(crate) const DEVICE_NO_MAX: u16 = 8191;
/// Skip targets recorded (seal.md §23).
pub(crate) const SKIPS_MAX: usize = 65_535;
/// Control entries recorded, `ctl_seqs` (seal.md §23).
pub(crate) const CTL_SEQS_MAX: usize = 1_048_576;
/// Values per list of a device record: the beat lists (seal.md §11.4, §23).
const LIST_MAX: usize = 16;
/// The fixed part of a device record (seal.md §13.2).
const DEVICE_FIXED: usize = 155;
/// The smallest device record: the fixed part, three empty lists and a
/// one-byte name.
const DEVICE_MIN: usize = DEVICE_FIXED + 3 + 2;
/// One commitment, one recovery record, one skip, one control seq (seal.md §13.2).
const COMMIT_LEN: usize = 44;
const RECOVERY_LEN: usize = 104;
const SKIP_LEN: usize = 40;
const CTL_LEN: usize = 8;

/// The membership state `M` (seal.md §13.1, §26).
pub struct Membership {
    pub(crate) board_id: BoardId,
    pub(crate) salt: [u8; 16],
    pub(crate) rules: u32,
    pub(crate) epoch: u32,
    pub(crate) epoch_seq: u64,
    pub(crate) epoch_clock_ms: i64,
    pub(crate) clock_ms: i64,
    pub(crate) home_gen: u32,
    pub(crate) home_seq: u64,
    pub(crate) home_host: String,
    pub(crate) home_door: [u8; 5],
    pub(crate) purge_seq: u64,
    pub(crate) next_device_no: u16,
    /// `(e, seq, c_e)`, ascending by `e`.
    pub(crate) commitments: Vec<(u32, u64, [u8; 32])>,
    pub(crate) recovery: Vec<RecoveryRecord>,
    pub(crate) devices: Vec<DeviceRecord>,
    pub(crate) pending_recovery: Option<PendingRecovery>,
    /// `(rules, voter, seq)`.
    pub(crate) pending_override: Option<(u32, DevId, u64)>,
    /// `(at_seq, target, hash_version, state_hash)`.
    pub(crate) last_reset: Option<(u64, u64, u32, [u8; 32])>,
    /// `(t, frame_hash)`, ascending by `t`.
    pub(crate) skips: Vec<(u64, FrameHash)>,
    pub(crate) ctl_seqs: Vec<u64>,
}

/// What `current_recovery` returns for the empty state `M_0`, which has no
/// recovery record (seal.md §2: "`M_0` is empty").
static NO_RECOVERY: RecoveryRecord =
    RecoveryRecord { dev: DevId([0; 16]), sign_pk: [0; 32], hpke_pk: [0; 32], since_seq: 0, until_seq: 0, last_ctr: 0 };

impl Membership {
    /// The canonical membership bytes (seal.md §13.2, SEAL-MEM-001).
    pub fn to_canonical_bytes(&self) -> Vec<u8> {
        let mut o = Vec::with_capacity(
            128 + self.home_host.len()
                + COMMIT_LEN * self.commitments.len()
                + RECOVERY_LEN * self.recovery.len()
                + 300 * self.devices.len()
                + SKIP_LEN * self.skips.len()
                + CTL_LEN * self.ctl_seqs.len(),
        );
        o.extend_from_slice(MAGIC);
        o.extend_from_slice(&VERSION.to_be_bytes());
        o.extend_from_slice(&self.board_id.0);
        o.extend_from_slice(&self.salt);
        o.extend_from_slice(&self.rules.to_be_bytes());
        o.extend_from_slice(&self.epoch.to_be_bytes());
        o.extend_from_slice(&self.epoch_seq.to_be_bytes());
        o.extend_from_slice(&self.epoch_clock_ms.to_be_bytes());
        o.extend_from_slice(&self.clock_ms.to_be_bytes());
        o.extend_from_slice(&self.home_gen.to_be_bytes());
        o.extend_from_slice(&self.home_seq.to_be_bytes());
        put_vb8(&mut o, self.home_host.as_bytes());
        o.extend_from_slice(&self.home_door);
        o.extend_from_slice(&self.purge_seq.to_be_bytes());
        o.extend_from_slice(&self.next_device_no.to_be_bytes());
        // Every list is bounded by its limit of §23, far below its count's width.
        o.extend_from_slice(&count32(self.commitments.len()).to_be_bytes());
        for (e, seq, c) in &self.commitments {
            o.extend_from_slice(&e.to_be_bytes());
            o.extend_from_slice(&seq.to_be_bytes());
            o.extend_from_slice(c);
        }
        o.extend_from_slice(&count16(self.recovery.len()).to_be_bytes());
        for r in &self.recovery {
            o.extend_from_slice(&r.dev.0);
            o.extend_from_slice(&r.sign_pk);
            o.extend_from_slice(&r.hpke_pk);
            o.extend_from_slice(&r.since_seq.to_be_bytes());
            o.extend_from_slice(&r.until_seq.to_be_bytes());
            o.extend_from_slice(&r.last_ctr.to_be_bytes());
        }
        o.extend_from_slice(&count16(self.devices.len()).to_be_bytes());
        for d in &self.devices {
            d.write(&mut o);
        }
        match &self.pending_recovery {
            None => o.push(0),
            Some(p) => {
                o.push(1);
                o.extend_from_slice(&p.proposer.0);
                o.extend_from_slice(&p.sign_pk);
                o.extend_from_slice(&p.hpke_pk);
                o.extend_from_slice(&p.seq.to_be_bytes());
                o.extend_from_slice(&p.start_ms.to_be_bytes());
            }
        }
        match &self.pending_override {
            None => o.push(0),
            Some((rules, voter, seq)) => {
                o.push(1);
                o.extend_from_slice(&rules.to_be_bytes());
                o.extend_from_slice(&voter.0);
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
        o.extend_from_slice(&count32(self.skips.len()).to_be_bytes());
        for (t, fh) in &self.skips {
            o.extend_from_slice(&t.to_be_bytes());
            o.extend_from_slice(&fh.0);
        }
        o.extend_from_slice(&count32(self.ctl_seqs.len()).to_be_bytes());
        for s in &self.ctl_seqs {
            o.extend_from_slice(&s.to_be_bytes());
        }
        o
    }

    /// Parses canonical membership bytes (seal.md §13.2, SEAL-MEM-002).
    ///
    /// Every count is checked against its limit of §23 and against the
    /// bytes remaining before anything is allocated (SEAL-GEN-008); every
    /// list must be strictly ascending in its order (so without duplicates),
    /// every flag 0 or 1, every field in its range, and no byte may follow.
    /// Beyond the layout, the state must be one the transitions can produce:
    /// either the empty state `M_0` (below) or a state with exactly one
    /// current recovery record, commitments for the epochs 1 to `epoch`,
    /// distinct ids and `device_no` values, and a `board_id` that the first
    /// recovery key and the salt give (§6.2). Anything else is
    /// `seal/membership_format`.
    ///
    /// The empty state `M_0` (§2) has the canonical bytes of a state whose
    /// every integer is 0, whose byte strings are zero or empty and whose
    /// lists are empty; `epoch = 0` marks it, and only those exact bytes are
    /// accepted with `epoch = 0`.
    // SPEC-QUESTION: §13.2 lays out the bytes of a state after Genesis; §2
    // says `M_0` is empty, and §26 gives no other way to build a `Membership`
    // than these bytes. The empty state's bytes are this crate's reading
    // (every field zero or empty); `classify` uses `M_0` at seq 1 whatever
    // state its caller passes (§13.3: "the state before seq 1 is empty").
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, SealError> {
        let m = parse(bytes).ok_or(SealError::MembershipFormat)?;
        // The layout has one encoding per state; this holds by construction
        // and is checked once more.
        if m.to_canonical_bytes() != bytes {
            return Err(SealError::MembershipFormat);
        }
        if m.epoch == 0 {
            if bytes != Membership::empty().to_canonical_bytes() {
                return Err(SealError::MembershipFormat);
            }
        } else if !m.consistent() {
            return Err(SealError::MembershipFormat);
        }
        Ok(m)
    }

    /// `board_id` (seal.md §13.1).
    pub fn board(&self) -> BoardId {
        self.board_id
    }

    /// The current epoch `E` (seal.md §13.1).
    pub fn epoch(&self) -> u32 {
        self.epoch
    }

    /// The active rules version (seal.md §13.1).
    pub fn rules(&self) -> u32 {
        self.rules
    }

    /// Every device ever added, ascending by `dev` (seal.md §13.1).
    pub fn devices(&self) -> &[DeviceRecord] {
        &self.devices
    }

    /// The recovery record with `until_seq = 0` (seal.md §13.1). The empty
    /// state `M_0` has none; it returns an all-zero record then.
    pub fn current_recovery(&self) -> &RecoveryRecord {
        self.recovery.iter().find(|r| r.until_seq == 0).unwrap_or(&NO_RECOVERY)
    }

    /// The proposed recovery-key change, if any (seal.md §13.1, §13.11).
    pub fn pending_recovery(&self) -> Option<&PendingRecovery> {
        self.pending_recovery.as_ref()
    }

    /// A device's `device_no` (seal.md §13.7).
    pub fn device_no(&self, dev: &DevId) -> Option<u16> {
        self.device(dev).map(|d| d.device_no)
    }

    // ------------------------------------------------------------ private

    /// The empty state `M_0` (seal.md §2, §13.3).
    pub(crate) fn empty() -> Membership {
        Membership {
            board_id: BoardId([0; 16]),
            salt: [0; 16],
            rules: 0,
            epoch: 0,
            epoch_seq: 0,
            epoch_clock_ms: 0,
            clock_ms: 0,
            home_gen: 0,
            home_seq: 0,
            home_host: String::new(),
            home_door: [0; 5],
            purge_seq: 0,
            next_device_no: 0,
            commitments: Vec::new(),
            recovery: Vec::new(),
            devices: Vec::new(),
            pending_recovery: None,
            pending_override: None,
            last_reset: None,
            skips: Vec::new(),
            ctl_seqs: Vec::new(),
        }
    }

    /// A copy (the working copy `W` of §13.6). `Membership` holds no
    /// secret (§13.1); the copy is private so that §26 stays the API.
    pub(crate) fn dup(&self) -> Membership {
        Membership {
            board_id: self.board_id,
            salt: self.salt,
            rules: self.rules,
            epoch: self.epoch,
            epoch_seq: self.epoch_seq,
            epoch_clock_ms: self.epoch_clock_ms,
            clock_ms: self.clock_ms,
            home_gen: self.home_gen,
            home_seq: self.home_seq,
            home_host: self.home_host.clone(),
            home_door: self.home_door,
            purge_seq: self.purge_seq,
            next_device_no: self.next_device_no,
            commitments: self.commitments.clone(),
            recovery: self.recovery.iter().map(RecoveryRecord::dup).collect(),
            devices: self.devices.iter().map(DeviceRecord::dup).collect(),
            pending_recovery: self.pending_recovery.as_ref().map(PendingRecovery::dup),
            pending_override: self.pending_override,
            last_reset: self.last_reset,
            skips: self.skips.clone(),
            ctl_seqs: self.ctl_seqs.clone(),
        }
    }

    /// The device record of `dev`.
    pub(crate) fn device(&self, dev: &DevId) -> Option<&DeviceRecord> {
        let i = self.devices.binary_search_by(|d| d.dev.cmp(dev)).ok()?;
        self.devices.get(i)
    }

    pub(crate) fn device_mut(&mut self, dev: &DevId) -> Option<&mut DeviceRecord> {
        let i = self.devices.binary_search_by(|d| d.dev.cmp(dev)).ok()?;
        self.devices.get_mut(i)
    }

    /// The recovery record of `dev`.
    pub(crate) fn recovery_record(&self, dev: &DevId) -> Option<&RecoveryRecord> {
        self.recovery.iter().find(|r| r.dev == *dev)
    }

    /// The commitment `c_e` recorded for epoch `e`.
    pub(crate) fn commitment(&self, e: u32) -> Option<&[u8; 32]> {
        let i = self.commitments.binary_search_by(|c| c.0.cmp(&e)).ok()?;
        self.commitments.get(i).map(|c| &c.2)
    }

    /// The number of current devices.
    pub(crate) fn current_count(&self) -> usize {
        self.devices.iter().filter(|d| d.is_current()).count()
    }

    /// Sets the author's counter (the bookkeeping of §13.5 for `Refuse` and
    /// `Accept`); an author the state does not record is left as it is.
    pub(crate) fn set_last_ctr(&mut self, dev: &DevId, ctr: u64) {
        if let Some(d) = self.device_mut(dev) {
            d.last_ctr = ctr;
        } else if let Some(r) = self.recovery.iter_mut().find(|r| r.dev == *dev) {
            r.last_ctr = ctr;
        }
    }

    /// The state an accepted Genesis at seq 1 produces (seal.md §13.3,
    /// SEAL-MEM-006): `at_ms` is the entry's, `commit` the item's.
    pub(crate) fn genesis(
        board: BoardId,
        g: &crate::Genesis,
        dev_r: DevId,
        at_ms: i64,
    ) -> Membership {
        let mut m = Membership::empty();
        m.board_id = board;
        m.salt = g.salt;
        m.rules = 1;
        m.epoch = 1;
        m.epoch_seq = 1;
        m.epoch_clock_ms = at_ms;
        m.clock_ms = at_ms;
        m.home_gen = 1;
        m.home_seq = 0;
        m.purge_seq = 0;
        m.next_device_no = 2;
        m.commitments = vec![(1, 1, g.commit)];
        m.recovery = vec![RecoveryRecord {
            dev: dev_r,
            sign_pk: g.recovery.sign_pk,
            hpke_pk: g.recovery.hpke_pk,
            since_seq: 1,
            until_seq: 0,
            last_ctr: 1,
        }];
        m.devices = vec![DeviceRecord {
            dev: g.device.dev,
            sign_pk: g.device.sign_pk,
            hpke_pk: g.device.hpke_pk,
            device_no: 1,
            status: DeviceStatus::Current,
            added_seq: 1,
            added_by: dev_r,
            bootstrap_seq: 0,
            status_seq: 1,
            status_by: dev_r,
            last_ctr: 0,
            beat_ms: at_ms,
            rules: vec![1],
            envelopes: vec![(1, 1)],
            hash_versions: vec![1],
            name: g.device.name.clone(),
        }];
        m.ctl_seqs = vec![1];
        m
    }

    /// The invariants of a non-empty state that the transitions keep and
    /// the parser checks (see `from_canonical_bytes`).
    fn consistent(&self) -> bool {
        if self.rules == 0 || self.home_gen == 0 || self.next_device_no == 0 {
            return false;
        }
        // Commitments for exactly the epochs 1 to E.
        let contiguous = self.commitments.len() == self.epoch as usize
            && self.commitments.iter().enumerate().all(|(i, c)| u32::try_from(i + 1).ok() == Some(c.0));
        if !contiguous {
            return false;
        }
        // Exactly one current recovery record; the first is the genesis key,
        // which with the salt gives `board_id` (§6.2).
        if self.recovery.iter().filter(|r| r.until_seq == 0).count() != 1 {
            return false;
        }
        match self.recovery.first() {
            Some(r) if board_id(&r.sign_pk, &self.salt) == self.board_id => {}
            _ => return false,
        }
        // Ids distinct across devices and recovery identities (§13.6
        // `DeviceAdded`, `RecoveryKeyChange`); device numbers distinct and
        // below `next_device_no` (§13.7).
        let mut ids: Vec<[u8; 16]> =
            self.devices.iter().map(|d| d.dev.0).chain(self.recovery.iter().map(|r| r.dev.0)).collect();
        ids.sort_unstable();
        if ids.windows(2).any(|w| matches!(w, [a, b] if a == b)) {
            return false;
        }
        let mut nos: Vec<u16> = self.devices.iter().map(|d| d.device_no).collect();
        nos.sort_unstable();
        if nos.windows(2).any(|w| matches!(w, [a, b] if a == b)) || nos.iter().any(|&n| n >= self.next_device_no) {
            return false;
        }
        true
    }
}

fn count32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn count16(n: usize) -> u16 {
    u16::try_from(n).unwrap_or(u16::MAX)
}

/// `vb8(x)`; every `x` written here is at most 255 bytes (a host, §5.4; a
/// name, §5.5).
fn put_vb8(o: &mut Vec<u8>, x: &[u8]) {
    let n = u8::try_from(x.len()).unwrap_or(u8::MAX);
    o.push(n);
    o.extend_from_slice(x.get(..usize::from(n)).unwrap_or(x));
}

/// A cursor over input bytes; every read checks the bytes remaining.
struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn arr<const N: usize>(&mut self) -> Option<[u8; N]> {
        let v = *take::<N>(self.b, self.at)?;
        self.at = self.at.checked_add(N)?;
        Some(v)
    }
    fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n)?;
        let v = self.b.get(self.at..end)?;
        self.at = end;
        Some(v)
    }
    fn u8(&mut self) -> Option<u8> {
        self.arr::<1>().map(|[x]| x)
    }
    fn u16(&mut self) -> Option<u16> {
        self.arr().map(u16::from_be_bytes)
    }
    fn u32(&mut self) -> Option<u32> {
        self.arr().map(u32::from_be_bytes)
    }
    fn u64(&mut self) -> Option<u64> {
        self.arr().map(u64::from_be_bytes)
    }
    fn i64(&mut self) -> Option<i64> {
        self.arr().map(i64::from_be_bytes)
    }
    fn remaining(&self) -> usize {
        self.b.len().saturating_sub(self.at)
    }
    /// A count of records of at least `min_len` bytes each, at most `max`,
    /// that the bytes remaining can hold (SEAL-GEN-008).
    fn count(&self, n: usize, max: usize, min_len: usize) -> Option<usize> {
        (n <= max && n.checked_mul(min_len)? <= self.remaining()).then_some(n)
    }
    /// A flag byte: 0 or 1.
    fn flag(&mut self) -> Option<bool> {
        match self.u8()? {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }
}

/// Strictly ascending by `key`.
fn ascending<T, K: Ord>(xs: &[T], key: impl Fn(&T) -> K) -> bool {
    xs.windows(2).all(|w| matches!(w, [a, b] if key(a) < key(b)))
}

/// The layout of §13.2, every field in its range and every list in order.
fn parse(bytes: &[u8]) -> Option<Membership> {
    let mut r = Reader { b: bytes, at: 0 };
    if &r.arr::<4>()? != MAGIC || r.u32()? != VERSION {
        return None;
    }
    let board_id = BoardId(r.arr()?);
    let salt = r.arr()?;
    let rules = r.u32()?;
    let epoch = r.u32()?;
    let epoch_seq = r.u64()?;
    let epoch_clock_ms = r.i64()?;
    let clock_ms = r.i64()?;
    let home_gen = r.u32()?;
    let home_seq = r.u64()?;
    let host_len = usize::from(r.u8()?);
    let home_host = core::str::from_utf8(r.bytes(host_len)?).ok()?.to_owned();
    if !home_host.is_empty() && !host_is_valid(&home_host) {
        return None;
    }
    let home_door = r.arr()?;
    let purge_seq = r.u64()?;
    let next_device_no = r.u16()?;
    if next_device_no > DEVICE_NO_MAX + 1 {
        return None;
    }

    let n = r.u32()?;
    let n = r.count(usize::try_from(n).ok()?, COMMIT_MAX, COMMIT_LEN)?;
    let mut commitments = Vec::with_capacity(n);
    for _ in 0..n {
        commitments.push((r.u32()?, r.u64()?, r.arr()?));
    }
    if !ascending(&commitments, |c| c.0) {
        return None;
    }

    let n = usize::from(r.u16()?);
    let n = r.count(n, RECOVERY_MAX, RECOVERY_LEN)?;
    let mut recovery = Vec::with_capacity(n);
    for _ in 0..n {
        recovery.push(RecoveryRecord {
            dev: DevId(r.arr()?),
            sign_pk: r.arr()?,
            hpke_pk: r.arr()?,
            since_seq: r.u64()?,
            until_seq: r.u64()?,
            last_ctr: r.u64()?,
        });
    }
    if !ascending(&recovery, |x| x.since_seq) {
        return None;
    }

    let n = usize::from(r.u16()?);
    let n = r.count(n, DEVICES_MAX, DEVICE_MIN)?;
    let mut devices = Vec::with_capacity(n);
    for _ in 0..n {
        devices.push(DeviceRecord::read(&mut r)?);
    }
    if !ascending(&devices, |d| d.dev) || devices.iter().filter(|d| d.is_current()).count() > CURRENT_MAX {
        return None;
    }

    let pending_recovery = if r.flag()? {
        Some(PendingRecovery {
            proposer: DevId(r.arr()?),
            sign_pk: r.arr()?,
            hpke_pk: r.arr()?,
            seq: r.u64()?,
            start_ms: r.i64()?,
        })
    } else {
        None
    };
    let pending_override = if r.flag()? { Some((r.u32()?, DevId(r.arr()?), r.u64()?)) } else { None };
    let last_reset = if r.flag()? { Some((r.u64()?, r.u64()?, r.u32()?, r.arr()?)) } else { None };

    let n = r.u32()?;
    let n = r.count(usize::try_from(n).ok()?, SKIPS_MAX, SKIP_LEN)?;
    let mut skips = Vec::with_capacity(n);
    for _ in 0..n {
        skips.push((r.u64()?, FrameHash(r.arr()?)));
    }
    if !ascending(&skips, |s| s.0) {
        return None;
    }

    let n = r.u32()?;
    let n = r.count(usize::try_from(n).ok()?, CTL_SEQS_MAX, CTL_LEN)?;
    let mut ctl_seqs = Vec::with_capacity(n);
    for _ in 0..n {
        ctl_seqs.push(r.u64()?);
    }
    if !ascending(&ctl_seqs, |s| *s) || r.remaining() != 0 {
        return None;
    }

    Some(Membership {
        board_id,
        salt,
        rules,
        epoch,
        epoch_seq,
        epoch_clock_ms,
        clock_ms,
        home_gen,
        home_seq,
        home_host,
        home_door,
        purge_seq,
        next_device_no,
        commitments,
        recovery,
        devices,
        pending_recovery,
        pending_override,
        last_reset,
        skips,
        ctl_seqs,
    })
}

/// A device record (seal.md §13.2, §26).
pub struct DeviceRecord {
    pub dev: DevId,
    pub sign_pk: [u8; 32],
    pub hpke_pk: [u8; 32],
    pub device_no: u16,
    pub status: DeviceStatus,
    pub added_seq: u64,
    pub added_by: DevId,
    pub bootstrap_seq: u64,
    pub status_seq: u64,
    pub status_by: DevId,
    pub last_ctr: u64,
    pub beat_ms: i64,
    pub rules: Vec<u32>,
    pub envelopes: Vec<(u8, u8)>,
    pub hash_versions: Vec<u32>,
    pub name: String,
}

impl DeviceRecord {
    /// `status = current`.
    pub(crate) fn is_current(&self) -> bool {
        matches!(self.status, DeviceStatus::Current)
    }

    pub(crate) fn dup(&self) -> DeviceRecord {
        DeviceRecord {
            dev: self.dev,
            sign_pk: self.sign_pk,
            hpke_pk: self.hpke_pk,
            device_no: self.device_no,
            status: match self.status {
                DeviceStatus::Current => DeviceStatus::Current,
                DeviceStatus::Revoked => DeviceStatus::Revoked,
            },
            added_seq: self.added_seq,
            added_by: self.added_by,
            bootstrap_seq: self.bootstrap_seq,
            status_seq: self.status_seq,
            status_by: self.status_by,
            last_ctr: self.last_ctr,
            beat_ms: self.beat_ms,
            rules: self.rules.clone(),
            envelopes: self.envelopes.clone(),
            hash_versions: self.hash_versions.clone(),
            name: self.name.clone(),
        }
    }

    fn write(&self, o: &mut Vec<u8>) {
        o.extend_from_slice(&self.dev.0);
        o.extend_from_slice(&self.sign_pk);
        o.extend_from_slice(&self.hpke_pk);
        o.extend_from_slice(&self.device_no.to_be_bytes());
        o.push(match self.status {
            DeviceStatus::Current => 1,
            DeviceStatus::Revoked => 2,
        });
        o.extend_from_slice(&self.added_seq.to_be_bytes());
        o.extend_from_slice(&self.added_by.0);
        o.extend_from_slice(&self.bootstrap_seq.to_be_bytes());
        o.extend_from_slice(&self.status_seq.to_be_bytes());
        o.extend_from_slice(&self.status_by.0);
        o.extend_from_slice(&self.last_ctr.to_be_bytes());
        o.extend_from_slice(&self.beat_ms.to_be_bytes());
        // The lists hold at most 16 values each (§11.4, §23).
        o.push(u8::try_from(self.rules.len()).unwrap_or(u8::MAX));
        for v in self.rules.iter().take(usize::from(u8::MAX)) {
            o.extend_from_slice(&v.to_be_bytes());
        }
        o.push(u8::try_from(self.envelopes.len()).unwrap_or(u8::MAX));
        for (ver, suite) in self.envelopes.iter().take(usize::from(u8::MAX)) {
            o.push(*ver);
            o.push(*suite);
        }
        o.push(u8::try_from(self.hash_versions.len()).unwrap_or(u8::MAX));
        for v in self.hash_versions.iter().take(usize::from(u8::MAX)) {
            o.extend_from_slice(&v.to_be_bytes());
        }
        put_vb8(o, self.name.as_bytes());
    }

    fn read(r: &mut Reader<'_>) -> Option<DeviceRecord> {
        let dev = DevId(r.arr()?);
        let sign_pk = r.arr()?;
        let hpke_pk = r.arr()?;
        let device_no = r.u16()?;
        if !(1..=DEVICE_NO_MAX).contains(&device_no) {
            return None;
        }
        let status = match r.u8()? {
            1 => DeviceStatus::Current,
            2 => DeviceStatus::Revoked,
            _ => return None,
        };
        let added_seq = r.u64()?;
        let added_by = DevId(r.arr()?);
        let bootstrap_seq = r.u64()?;
        let status_seq = r.u64()?;
        let status_by = DevId(r.arr()?);
        let last_ctr = r.u64()?;
        let beat_ms = r.i64()?;
        let n = usize::from(r.u8()?);
    let n = r.count(n, LIST_MAX, 4)?;
        let mut rules = Vec::with_capacity(n);
        for _ in 0..n {
            rules.push(r.u32()?);
        }
        let n = usize::from(r.u8()?);
    let n = r.count(n, LIST_MAX, 2)?;
        let mut envelopes = Vec::with_capacity(n);
        for _ in 0..n {
            envelopes.push((r.u8()?, r.u8()?));
        }
        let n = usize::from(r.u8()?);
    let n = r.count(n, LIST_MAX, 4)?;
        let mut hash_versions = Vec::with_capacity(n);
        for _ in 0..n {
            hash_versions.push(r.u32()?);
        }
        if !ascending(&rules, |v| *v) || !ascending(&envelopes, |v| *v) || !ascending(&hash_versions, |v| *v) {
            return None;
        }
        let name_len = usize::from(r.u8()?);
        let name = r.bytes(name_len)?;
        if !device_name_is_valid(name) {
            return None;
        }
        let name = core::str::from_utf8(name).ok()?.to_owned();
        Some(DeviceRecord {
            dev,
            sign_pk,
            hpke_pk,
            device_no,
            status,
            added_seq,
            added_by,
            bootstrap_seq,
            status_seq,
            status_by,
            last_ctr,
            beat_ms,
            rules,
            envelopes,
            hash_versions,
            name,
        })
    }
}

/// A device record's `status` byte (seal.md §13.2).
pub enum DeviceStatus {
    Current = 1,
    Revoked = 2,
}

/// A recovery record (seal.md §13.2, §26).
pub struct RecoveryRecord {
    pub dev: DevId,
    pub sign_pk: [u8; 32],
    pub hpke_pk: [u8; 32],
    pub since_seq: u64,
    pub until_seq: u64,
    pub last_ctr: u64,
}

impl RecoveryRecord {
    pub(crate) fn dup(&self) -> RecoveryRecord {
        RecoveryRecord {
            dev: self.dev,
            sign_pk: self.sign_pk,
            hpke_pk: self.hpke_pk,
            since_seq: self.since_seq,
            until_seq: self.until_seq,
            last_ctr: self.last_ctr,
        }
    }
}

/// A proposed recovery-key change (seal.md §13.2, §26).
pub struct PendingRecovery {
    pub proposer: DevId,
    pub sign_pk: [u8; 32],
    pub hpke_pk: [u8; 32],
    pub seq: u64,
    pub start_ms: i64,
}

impl PendingRecovery {
    pub(crate) fn dup(&self) -> PendingRecovery {
        PendingRecovery {
            proposer: self.proposer,
            sign_pk: self.sign_pk,
            hpke_pk: self.hpke_pk,
            seq: self.seq,
            start_ms: self.start_ms,
        }
    }
}

/// The caller's anchor for seq 1 (seal.md §13.3, §26).
pub struct GenesisAnchor {
    pub rsign_pk: [u8; 32],
    pub salt: [u8; 16],
    pub root1: EpochRoot,
}

#[cfg(test)]
mod tests;
