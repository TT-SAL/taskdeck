//! The JSON payloads of kinds `ctl`, `beat` and `snap` (seal.md §11.3 to
//! §11.5) and the control items of §12: one encoder and one strict parser
//! per table, the inner control signature, and the shapes of §12.1.
//!
//! Every encoder builds the JSON value, runs the same parser over it (with
//! no entry context) and refuses what the parser rejects, so an encoder
//! never produces a value its parser would reject.

use serde_json::{Map, Value};

use crate::canon::{self, Fields, INT_MAX, JsonError, array, hex, hex_value, int, int_value, int32, string};
use crate::encodings::{device_name_is_valid, host_is_valid};
use crate::entries::{
    AckVerdict, Beat, BeatAck, BeatState, CtlItem, DeviceAdded, EpochRotated, Fence, Genesis, GenesisDevice,
    GenesisSnapshot, RecoveryKeyChange, RecoveryKeyMode, ResetTo, RulesUpgrade, RulesUpgradeMode, SkipItem,
    SnapAnnounce,
};
use crate::prim::{h, take};
use crate::{BlobId, BoardId, DevId, EntryId, FrameHash, RecoveryPublic, SealError, ctx};

/// Items per control entry (seal.md §11.3, §23).
pub(crate) const CTL_ITEMS_MAX: usize = 16;
/// Revocations and reinstatements per `R`-shape entry (seal.md §12.1, §23).
pub(crate) const R_TARGETS_MAX: usize = 15;
/// Skip items per entry (seal.md §12.1, §23).
pub(crate) const SKIPS_MAX: usize = 16;
/// Wraps per rotation (seal.md §12.5, §23).
pub(crate) const ROTATION_WRAPS_MIN: usize = 2;
pub(crate) const ROTATION_WRAPS_MAX: usize = 17;
/// Values per beat list (seal.md §11.4, §23).
pub(crate) const BEAT_LIST_MAX: usize = 16;
/// Beat `acks` (seal.md §11.4, §23).
pub(crate) const BEAT_ACKS_MAX: usize = 64;
/// `2^31 − 1`, the largest rules, hash or envelope-list version.
pub(crate) const VERSION_MAX: u32 = i32::MAX as u32;
/// The snapshot interval: `state.seq` and `snap.seq` are positive multiples (§11.4, §11.5).
pub(crate) const SNAP_INTERVAL: u64 = 64;
/// The smallest sealed blob (seal.md §11.5).
pub(crate) const BLOB_SIZE_MIN: u64 = 67;
/// The largest sealed blob and inflated snapshot (seal.md §11.5, §23).
pub(crate) const BLOB_SIZE_MAX: u64 = 67_108_864;
/// One blob part (seal.md §11.5, §23).
pub(crate) const PART_SIZE: u64 = 1_048_576;

// ------------------------------------------------------------- items (§12)

fn type_str(item: &CtlItem) -> &'static str {
    match item {
        CtlItem::Genesis(_) => "Genesis",
        CtlItem::DeviceAdded(_) => "DeviceAdded",
        CtlItem::DeviceRevoked(_) => "DeviceRevoked",
        CtlItem::DeviceReinstated(_) => "DeviceReinstated",
        CtlItem::EpochRotated(_) => "EpochRotated",
        CtlItem::RecoveryKeyChange(_) => "RecoveryKeyChange",
        CtlItem::Fence(_) => "Fence",
        CtlItem::ResetTo(_) => "ResetTo",
        CtlItem::Skip(_) => "Skip",
        CtlItem::RulesUpgrade(_) => "RulesUpgrade",
    }
}

fn wraps_value(wraps: &[[u8; 96]]) -> Value {
    Value::Array(wraps.iter().map(|w| hex_value(w)).collect())
}

fn snapshot_value(s: &GenesisSnapshot) -> Result<Value, JsonError> {
    let mut m = Map::new();
    m.insert("blob".into(), hex_value(&s.blob.0));
    m.insert("sha256".into(), hex_value(&s.sha256));
    m.insert("size".into(), int_value(s.size)?);
    m.insert("parts".into(), int_value(u64::from(s.parts))?);
    m.insert("inflated".into(), int_value(s.inflated)?);
    Ok(Value::Object(m))
}

/// The JSON object of one item (seal.md §12.2 to §12.10), unchecked.
fn item_value(item: &CtlItem) -> Result<Value, JsonError> {
    let mut m = Map::new();
    m.insert("type".into(), Value::String(type_str(item).into()));
    match item {
        CtlItem::Genesis(g) => {
            m.insert("salt".into(), hex_value(&g.salt));
            let mut r = Map::new();
            r.insert("sign_pk".into(), hex_value(&g.recovery.sign_pk));
            r.insert("hpke_pk".into(), hex_value(&g.recovery.hpke_pk));
            m.insert("recovery".into(), Value::Object(r));
            let mut d = Map::new();
            d.insert("dev".into(), hex_value(&g.device.dev.0));
            d.insert("sign_pk".into(), hex_value(&g.device.sign_pk));
            d.insert("hpke_pk".into(), hex_value(&g.device.hpke_pk));
            d.insert("name".into(), Value::String(g.device.name.clone()));
            m.insert("device".into(), Value::Object(d));
            m.insert("epoch".into(), int_value(u64::from(g.epoch))?);
            m.insert("commit".into(), hex_value(&g.commit));
            m.insert("wraps".into(), wraps_value(&g.wraps));
            m.insert("rules".into(), int_value(u64::from(g.rules))?);
            m.insert("hash_version".into(), int_value(u64::from(g.hash_version))?);
            m.insert("state_hash".into(), hex_value(&g.state_hash));
            if let Some(s) = &g.snapshot {
                m.insert("snapshot".into(), snapshot_value(s)?);
            }
        }
        CtlItem::DeviceAdded(a) => {
            m.insert("dev".into(), hex_value(&a.dev.0));
            m.insert("sign_pk".into(), hex_value(&a.sign_pk));
            m.insert("hpke_pk".into(), hex_value(&a.hpke_pk));
            m.insert("name".into(), Value::String(a.name.clone()));
            m.insert("transcript".into(), hex_value(&a.transcript));
            m.insert("bootstrap_seq".into(), int_value(a.bootstrap_seq)?);
        }
        CtlItem::DeviceRevoked(d) | CtlItem::DeviceReinstated(d) => {
            m.insert("dev".into(), hex_value(&d.0));
        }
        CtlItem::EpochRotated(r) => {
            m.insert("epoch".into(), int_value(u64::from(r.epoch))?);
            m.insert("commit".into(), hex_value(&r.commit));
            m.insert("wraps".into(), wraps_value(&r.wraps));
            if let Some(p) = r.purge_before {
                m.insert("purge_before".into(), int_value(p)?);
            }
        }
        CtlItem::RecoveryKeyChange(k) => {
            let mode = match k.mode {
                RecoveryKeyMode::Direct => "direct",
                RecoveryKeyMode::Propose => "propose",
                RecoveryKeyMode::Confirm => "confirm",
                RecoveryKeyMode::Cancel => "cancel",
            };
            m.insert("mode".into(), Value::String(mode.into()));
            if let Some(pk) = &k.sign_pk {
                m.insert("sign_pk".into(), hex_value(pk));
            }
            if let Some(pk) = &k.hpke_pk {
                m.insert("hpke_pk".into(), hex_value(pk));
            }
        }
        CtlItem::Fence(f) => {
            m.insert("host".into(), Value::String(f.host.clone()));
            m.insert("door".into(), hex_value(&f.door));
            m.insert("home_gen".into(), int_value(u64::from(f.home_gen))?);
        }
        CtlItem::ResetTo(r) => {
            m.insert("seq".into(), int_value(r.seq)?);
            m.insert("state_hash".into(), hex_value(&r.state_hash));
            m.insert("hash_version".into(), int_value(u64::from(r.hash_version))?);
        }
        CtlItem::Skip(s) => {
            m.insert("seq".into(), int_value(s.seq)?);
            m.insert("frame_hash".into(), hex_value(&s.frame_hash.0));
        }
        CtlItem::RulesUpgrade(u) => {
            m.insert("rules".into(), int_value(u64::from(u.rules))?);
            let mode = match u.mode {
                RulesUpgradeMode::Normal => "normal",
                RulesUpgradeMode::Override => "override",
            };
            m.insert("mode".into(), Value::String(mode.into()));
        }
    }
    Ok(Value::Object(m))
}

/// A device name of §5.5 (SEAL-ENC-007).
fn name(v: &Value) -> Result<String, JsonError> {
    let s = string(v)?;
    if !device_name_is_valid(s.as_bytes()) {
        return Err(JsonError::StringLimit);
    }
    Ok(s.to_owned())
}

/// The recipient id of a wrap: its first 16 bytes (seal.md §7).
pub(crate) fn wrap_recipient(w: &[u8; 96]) -> [u8; 16] {
    take::<16>(w, 0).copied().unwrap_or([0u8; 16])
}

/// `min` to `max` wraps, strictly ascending by recipient id (so distinct).
fn wraps(v: &Value, min: usize, max: usize) -> Result<Vec<[u8; 96]>, JsonError> {
    let a = array(v, min, max)?;
    let mut out: Vec<[u8; 96]> = Vec::with_capacity(a.len());
    for e in a {
        let w = hex::<96>(e)?;
        if let Some(prev) = out.last()
            && wrap_recipient(prev) >= wrap_recipient(&w)
        {
            return Err(JsonError::ArrayOrder);
        }
        out.push(w);
    }
    Ok(out)
}

/// The constraints of §11.5 on `size`, `parts` and `inflated`, shared by
/// `snap` and Genesis's `snapshot`.
fn blob_dims(f: &mut Fields<'_>) -> Result<(u64, u32, u64), JsonError> {
    let size = int(f.req("size")?, BLOB_SIZE_MIN, BLOB_SIZE_MAX)?;
    let parts = int(f.req("parts")?, 0, INT_MAX)?;
    if parts != size.div_ceil(PART_SIZE) {
        return Err(JsonError::Constraint);
    }
    let parts = u32::try_from(parts).map_err(|_| JsonError::Constraint)?;
    let inflated = int(f.req("inflated")?, 1, BLOB_SIZE_MAX)?;
    Ok((size, parts, inflated))
}

fn genesis(f: &mut Fields<'_>) -> Result<Genesis, JsonError> {
    let salt = hex::<16>(f.req("salt")?)?;
    let mut r = Fields::of(f.req("recovery")?)?;
    let recovery = RecoveryPublic { sign_pk: hex::<32>(r.req("sign_pk")?)?, hpke_pk: hex::<32>(r.req("hpke_pk")?)? };
    r.finish()?;
    let mut d = Fields::of(f.req("device")?)?;
    let device = GenesisDevice {
        dev: DevId(hex::<16>(d.req("dev")?)?),
        sign_pk: hex::<32>(d.req("sign_pk")?)?,
        hpke_pk: hex::<32>(d.req("hpke_pk")?)?,
        name: name(d.req("name")?)?,
    };
    d.finish()?;
    let epoch = int32(f.req("epoch")?, 1, 1)?;
    let commit = hex::<32>(f.req("commit")?)?;
    // Exactly two wraps, ascending by recipient id (§12.2). That they go to
    // `device.dev` and to `dev_R`, and that `dev` differs from `dev_R`, needs
    // the board: §13.3 checks it (task S3, `seal/ctl_genesis`).
    let wraps = wraps(f.req("wraps")?, 2, 2)?;
    let rules = int32(f.req("rules")?, 1, 1)?;
    let hash_version = int32(f.req("hash_version")?, 1, 1)?;
    let state_hash = hex::<32>(f.req("state_hash")?)?;
    let snapshot = match f.opt("snapshot") {
        None => None,
        Some(v) => {
            let mut s = Fields::of(v)?;
            let blob = BlobId(hex::<16>(s.req("blob")?)?);
            let sha256 = hex::<32>(s.req("sha256")?)?;
            let (size, parts, inflated) = blob_dims(&mut s)?;
            s.finish()?;
            Some(GenesisSnapshot { blob, sha256, size, parts, inflated })
        }
    };
    Ok(Genesis { salt, recovery, device, epoch, commit, wraps, rules, hash_version, state_hash, snapshot })
}

/// Parses one control item against its table (seal.md §12.2 to §12.10,
/// SEAL-CTL-003, SEAL-ENC-004).
///
/// Only what the item alone decides is checked here. The bounds of §12
/// that name the entry's seq (`bootstrap_seq < s`, `purge_before ≤ s`,
/// `ResetTo.seq < s`, `Skip.seq < s`) are checked by §13.6 with
/// `seal/ctl_target` (task S3), which §13.6 assigns them; so are the
/// Genesis checks that need the board (§13.3, `seal/ctl_genesis`).
// SPEC-QUESTION: §12.3, §12.5, §12.8 and §12.9 state "< the entry's seq" /
// "≤ the entry's seq" in the schema tables (step 15, `seal/bad_payload`),
// and §13.6 states the same bounds again with `seal/ctl_target` (step 16).
// This parser leaves them to §13.6, whose code is the specific one.
pub(crate) fn item_from_value(v: &Value) -> Result<CtlItem, JsonError> {
    let mut f = Fields::of(v)?;
    let ty = string(f.req("type")?)?;
    let item = match ty {
        "Genesis" => CtlItem::Genesis(genesis(&mut f)?),
        "DeviceAdded" => CtlItem::DeviceAdded(DeviceAdded {
            dev: DevId(hex::<16>(f.req("dev")?)?),
            sign_pk: hex::<32>(f.req("sign_pk")?)?,
            hpke_pk: hex::<32>(f.req("hpke_pk")?)?,
            name: name(f.req("name")?)?,
            transcript: hex::<32>(f.req("transcript")?)?,
            bootstrap_seq: int(f.req("bootstrap_seq")?, 1, INT_MAX)?,
        }),
        "DeviceRevoked" => CtlItem::DeviceRevoked(DevId(hex::<16>(f.req("dev")?)?)),
        "DeviceReinstated" => CtlItem::DeviceReinstated(DevId(hex::<16>(f.req("dev")?)?)),
        "EpochRotated" => CtlItem::EpochRotated(EpochRotated {
            epoch: int32(f.req("epoch")?, 2, u32::MAX)?,
            commit: hex::<32>(f.req("commit")?)?,
            wraps: wraps(f.req("wraps")?, ROTATION_WRAPS_MIN, ROTATION_WRAPS_MAX)?,
            purge_before: f.opt("purge_before").map(|p| int(p, 1, INT_MAX)).transpose()?,
        }),
        "RecoveryKeyChange" => {
            let mode = match string(f.req("mode")?)? {
                "direct" => RecoveryKeyMode::Direct,
                "propose" => RecoveryKeyMode::Propose,
                "confirm" => RecoveryKeyMode::Confirm,
                "cancel" => RecoveryKeyMode::Cancel,
                _ => return Err(JsonError::Constraint),
            };
            let (sign_pk, hpke_pk) = match mode {
                // Absent for `cancel`: present, they are fields its row does not list.
                RecoveryKeyMode::Cancel => (None, None),
                _ => (Some(hex::<32>(f.req("sign_pk")?)?), Some(hex::<32>(f.req("hpke_pk")?)?)),
            };
            CtlItem::RecoveryKeyChange(RecoveryKeyChange { mode, sign_pk, hpke_pk })
        }
        "Fence" => {
            let host = string(f.req("host")?)?;
            if !host_is_valid(host) {
                return Err(JsonError::Host);
            }
            CtlItem::Fence(Fence {
                host: host.to_owned(),
                door: hex::<5>(f.req("door")?)?,
                home_gen: int32(f.req("home_gen")?, 2, u32::MAX)?,
            })
        }
        "ResetTo" => CtlItem::ResetTo(ResetTo {
            seq: int(f.req("seq")?, 1, INT_MAX)?,
            state_hash: hex::<32>(f.req("state_hash")?)?,
            hash_version: int32(f.req("hash_version")?, 1, VERSION_MAX)?,
        }),
        "Skip" => CtlItem::Skip(SkipItem {
            seq: int(f.req("seq")?, 1, INT_MAX)?,
            frame_hash: FrameHash(hex::<32>(f.req("frame_hash")?)?),
        }),
        "RulesUpgrade" => CtlItem::RulesUpgrade(RulesUpgrade {
            rules: int32(f.req("rules")?, 2, VERSION_MAX)?,
            mode: match string(f.req("mode")?)? {
                "normal" => RulesUpgradeMode::Normal,
                "override" => RulesUpgradeMode::Override,
                _ => return Err(JsonError::Constraint),
            },
        }),
        _ => return Err(JsonError::UnknownItem),
    };
    f.finish()?;
    Ok(item)
}

/// The value of one item, refused when its own parser rejects it.
pub(crate) fn checked_item_value(item: &CtlItem) -> Result<Value, JsonError> {
    let v = item_value(item)?;
    item_from_value(&v)?;
    Ok(v)
}

// ------------------------------------------------------- shapes (§12.1)

/// The shapes of seal.md §12.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shape {
    /// `Genesis`.
    G,
    /// `DeviceAdded`.
    A,
    /// 0 to 15 `Revoke`/`Reinstate`, then `EpochRotated`.
    R,
    /// `RecoveryKeyChange` `direct`/`confirm`, then `EpochRotated`.
    K,
    /// `RecoveryKeyChange` `propose`/`cancel`.
    P,
    /// `Fence`.
    F,
    /// `ResetTo`.
    T,
    /// 1 to 16 `Skip`.
    S,
    /// `RulesUpgrade`.
    U,
}

/// §14.2 step 16, first part: the shape the items form (seal.md §12.1,
/// SEAL-CTL-001), or `seal/ctl_shape`.
///
/// Structural only: which seq a shape may stand at (`G` only at seq 1, and
/// seq 1 only `G`, SEAL-CTL-002) is the caller's check (task S3), as are
/// the item rules of §13.6. `R` needs pairwise distinct targets, `S`
/// pairwise distinct target seqs.
pub(crate) fn ctl_shape(items: &[CtlItem]) -> Result<Shape, SealError> {
    let bad = SealError::CtlShape;
    let rkc_mode = |i: &CtlItem| match i {
        CtlItem::RecoveryKeyChange(k) => Some(match k.mode {
            RecoveryKeyMode::Direct | RecoveryKeyMode::Confirm => Shape::K,
            RecoveryKeyMode::Propose | RecoveryKeyMode::Cancel => Shape::P,
        }),
        _ => None,
    };
    match items {
        [CtlItem::Genesis(_)] => return Ok(Shape::G),
        [CtlItem::DeviceAdded(_)] => return Ok(Shape::A),
        [CtlItem::Fence(_)] => return Ok(Shape::F),
        [CtlItem::ResetTo(_)] => return Ok(Shape::T),
        [CtlItem::RulesUpgrade(_)] => return Ok(Shape::U),
        [k] if rkc_mode(k) == Some(Shape::P) => return Ok(Shape::P),
        [k, CtlItem::EpochRotated(_)] if rkc_mode(k) == Some(Shape::K) => return Ok(Shape::K),
        _ => {}
    }
    if items.iter().all(|i| matches!(i, CtlItem::Skip(_))) {
        if items.is_empty() || items.len() > SKIPS_MAX {
            return Err(bad);
        }
        let mut seqs: Vec<u64> =
            items.iter().filter_map(|i| if let CtlItem::Skip(s) = i { Some(s.seq) } else { None }).collect();
        seqs.sort_unstable();
        if seqs.windows(2).any(|w| matches!(w, [a, b] if a == b)) {
            return Err(bad);
        }
        return Ok(Shape::S);
    }
    if let Some((CtlItem::EpochRotated(_), targets)) = items.split_last() {
        if targets.len() > R_TARGETS_MAX {
            return Err(bad);
        }
        let mut devs = Vec::with_capacity(targets.len());
        for t in targets {
            match t {
                CtlItem::DeviceRevoked(d) | CtlItem::DeviceReinstated(d) => devs.push(d.0),
                _ => return Err(bad),
            }
        }
        devs.sort_unstable();
        if devs.windows(2).any(|w| matches!(w, [a, b] if a == b)) {
            return Err(bad);
        }
        return Ok(Shape::R);
    }
    Err(bad)
}

// ------------------------------------------------------------ ctl (§11.3)

/// `inner = ctx("ctl") ‖ board_id ‖ dev ‖ u64 ctr ‖ entry_id ‖ i64 at_ms ‖
/// H(items_bytes)` (seal.md §11.3, SEAL-KIND-003).
pub(crate) fn ctl_inner(board: &BoardId, dev: &DevId, ctr: u64, entry_id: &EntryId, at_ms: i64, items_bytes: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(ctx::CTL.len() + 16 + 16 + 8 + 16 + 8 + 32);
    v.extend_from_slice(ctx::CTL);
    v.extend_from_slice(&board.0);
    v.extend_from_slice(&dev.0);
    v.extend_from_slice(&ctr.to_be_bytes());
    v.extend_from_slice(&entry_id.0);
    v.extend_from_slice(&at_ms.to_be_bytes());
    v.extend_from_slice(&h(&[items_bytes]));
    v
}

/// The `items` value and its bytes `enc(items)`, every item checked
/// against its table and the count against 1 to 16. Errors: the item's
/// code (`seal/host_format` for a Fence host, else `seal/bad_payload`).
pub(crate) fn items_bytes(items: &[CtlItem]) -> Result<(Value, Vec<u8>), SealError> {
    if items.is_empty() || items.len() > CTL_ITEMS_MAX {
        return Err(SealError::BadPayload);
    }
    let mut a = Vec::with_capacity(items.len());
    for i in items {
        a.push(checked_item_value(i).map_err(|e| match e {
            JsonError::Host => SealError::HostFormat,
            _ => SealError::BadPayload,
        })?);
    }
    let v = Value::Array(a);
    let bytes = canon::encode(&v).map_err(|_| SealError::BadPayload)?;
    Ok((v, bytes))
}

/// The payload `{"items": …, "sig": …}` with `sig` given.
pub(crate) fn ctl_value(items: Value, sig: &[u8; 64]) -> Value {
    let mut m = Map::new();
    m.insert("items".into(), items);
    m.insert("sig".into(), hex_value(sig));
    Value::Object(m)
}

/// What §11.3 binds the inner signature to, and the key that verifies it.
pub(crate) struct CtlContext<'a> {
    pub board: &'a BoardId,
    pub dev: &'a DevId,
    pub ctr: u64,
    pub entry_id: &'a EntryId,
    pub at_ms: i64,
    /// The frame author's `sign_pk`.
    pub sign_pk: &'a [u8; 32],
}

/// §14.2 step 15 for kind `ctl` (seal.md §11.3, SEAL-KIND-003,
/// SEAL-KIND-004, SEAL-ENC-003, SEAL-ENC-004).
///
/// In order: the bytes parse as canonical JSON (§5.3), the object is the
/// table of §11.3 (1 to 16 items, `sig` 64 bytes) and every item its table
/// of §12 (see `item_from_value` for what that leaves to §13.6); any
/// failure is `seal/bad_payload`. Then the inner signature verifies
/// strictly under `cx.sign_pk` over `ctl_inner` of `enc(items)`, else
/// `seal/bad_ctl_sig`. The shape (step 16) is `ctl_shape`'s.
pub(crate) fn parse_ctl(payload: &[u8], cx: &CtlContext<'_>) -> Result<Vec<CtlItem>, SealError> {
    let u = parse_ctl_unsigned(payload).map_err(|_| SealError::BadPayload)?;
    let inner = ctl_inner(cx.board, cx.dev, cx.ctr, cx.entry_id, cx.at_ms, &u.items_bytes);
    crate::signatures::verify_strict(cx.sign_pk, &inner, &u.sig).map_err(|_| SealError::BadCtlSig)?;
    Ok(u.items)
}

/// A control payload whose JSON passed §5.3 and the tables, its inner
/// signature not yet verified.
pub(crate) struct UnsignedCtl {
    pub items: Vec<CtlItem>,
    /// `enc(items)`, the bytes the inner signature hashes.
    pub items_bytes: Vec<u8>,
    pub sig: [u8; 64],
}

/// The JSON steps of `parse_ctl`: the items, `enc(items)` and the signature.
pub(crate) fn parse_ctl_unsigned(payload: &[u8]) -> Result<UnsignedCtl, JsonError> {
    let v = canon::parse(payload)?;
    let mut f = Fields::of(&v)?;
    let a = array(f.req("items")?, 1, CTL_ITEMS_MAX)?;
    let mut items = Vec::with_capacity(a.len());
    for i in a {
        items.push(item_from_value(i)?);
    }
    let sig = hex::<64>(f.req("sig")?)?;
    f.finish()?;
    // The record's own encoding: every item re-encoded from its struct
    // (a field written at its default, or in a non-canonical form, fails).
    let mut re = Vec::with_capacity(items.len());
    for i in &items {
        re.push(item_value(i)?);
    }
    let re = Value::Array(re);
    let items_bytes = canon::encode(&re)?;
    if canon::encode(&ctl_value(re, &sig))? != payload {
        return Err(JsonError::NotCanonical);
    }
    Ok(UnsignedCtl { items, items_bytes, sig })
}

/// The `items` of a history record (seal.md §21.5, §21.6 step 5): the bytes
/// must be exactly the canonical encoding (§5.3) of an array of 1 to 16
/// items, each its table of §12, as `parse_ctl_unsigned` re-encodes them.
/// The shape is `ctl_shape`'s.
pub(crate) fn parse_items_bytes(bytes: &[u8]) -> Result<Vec<CtlItem>, JsonError> {
    let v = canon::parse(bytes)?;
    let a = array(&v, 1, CTL_ITEMS_MAX)?;
    let mut items = Vec::with_capacity(a.len());
    let mut re = Vec::with_capacity(a.len());
    for i in a {
        let item = item_from_value(i)?;
        re.push(item_value(&item)?);
        items.push(item);
    }
    if canon::encode(&Value::Array(re))? != bytes {
        return Err(JsonError::NotCanonical);
    }
    Ok(items)
}

// ----------------------------------------------------------- beat (§11.4)

fn beat_value(b: &Beat) -> Result<Value, JsonError> {
    let ints = |xs: &[u32]| -> Result<Value, JsonError> {
        Ok(Value::Array(xs.iter().map(|&x| int_value(u64::from(x))).collect::<Result<_, _>>()?))
    };
    let mut m = Map::new();
    m.insert("rules".into(), ints(&b.rules)?);
    let mut env = Vec::with_capacity(b.envelopes.len());
    for &(ver, suite) in &b.envelopes {
        env.push(Value::Array(vec![int_value(u64::from(ver))?, int_value(u64::from(suite))?]));
    }
    m.insert("envelopes".into(), Value::Array(env));
    m.insert("hash_versions".into(), ints(&b.hash_versions)?);
    if let Some(s) = &b.state {
        let mut sm = Map::new();
        sm.insert("seq".into(), int_value(s.seq)?);
        sm.insert("hash".into(), hex_value(&s.hash));
        sm.insert("hash_version".into(), int_value(u64::from(s.hash_version))?);
        m.insert("state".into(), Value::Object(sm));
    }
    // Default `[]`: omitted when empty (§5.3).
    if !b.acks.is_empty() {
        let mut acks = Vec::with_capacity(b.acks.len());
        for a in &b.acks {
            let mut am = Map::new();
            am.insert("entry".into(), int_value(a.entry)?);
            let verdict = match a.verdict {
                AckVerdict::Ack => "ack",
                AckVerdict::Mismatch => "mismatch",
            };
            am.insert("verdict".into(), Value::String(verdict.into()));
            acks.push(Value::Object(am));
        }
        m.insert("acks".into(), Value::Array(acks));
    }
    Ok(Value::Object(m))
}

/// 1 to 16 distinct versions from 1 to 2^31 − 1, ascending.
fn versions(v: &Value) -> Result<Vec<u32>, JsonError> {
    let a = array(v, 1, BEAT_LIST_MAX)?;
    let mut out: Vec<u32> = Vec::with_capacity(a.len());
    for e in a {
        let x = int32(e, 1, VERSION_MAX)?;
        if out.last().is_some_and(|&p| p >= x) {
            return Err(JsonError::ArrayOrder);
        }
        out.push(x);
    }
    Ok(out)
}

/// A positive multiple of 64 (§11.4 `state.seq`, §11.5 `seq`).
fn snapshot_seq(v: &Value) -> Result<u64, JsonError> {
    let s = int(v, SNAP_INTERVAL, INT_MAX)?;
    if !s.is_multiple_of(SNAP_INTERVAL) {
        return Err(JsonError::Constraint);
    }
    Ok(s)
}

/// The entry fields a beat's table names: its own seq and `based_on_seq`.
#[derive(Clone, Copy)]
pub(crate) struct BeatContext {
    pub seq: u64,
    pub based_on_seq: u64,
}

fn beat_from_value(v: &Value, cx: Option<BeatContext>) -> Result<Beat, JsonError> {
    let mut f = Fields::of(v)?;
    let rules = versions(f.req("rules")?)?;
    let ea = array(f.req("envelopes")?, 1, BEAT_LIST_MAX)?;
    let mut envelopes: Vec<(u8, u8)> = Vec::with_capacity(ea.len());
    for e in ea {
        let pair = array(e, 2, 2)?;
        let (ver, suite) = match pair {
            [a, b] => (int(a, 1, 255)?, int(b, 1, 255)?),
            _ => return Err(JsonError::ArrayCount),
        };
        let p = (u8::try_from(ver).map_err(|_| JsonError::IntRange)?, u8::try_from(suite).map_err(|_| JsonError::IntRange)?);
        if envelopes.last().is_some_and(|&q| q >= p) {
            return Err(JsonError::ArrayOrder);
        }
        envelopes.push(p);
    }
    let hash_versions = versions(f.req("hash_versions")?)?;
    let state = match f.opt("state") {
        None => None,
        Some(sv) => {
            let mut s = Fields::of(sv)?;
            let seq = snapshot_seq(s.req("seq")?)?;
            if cx.is_some_and(|c| seq > c.based_on_seq) {
                return Err(JsonError::Constraint);
            }
            let st = BeatState {
                seq,
                hash: hex::<32>(s.req("hash")?)?,
                hash_version: int32(s.req("hash_version")?, 1, VERSION_MAX)?,
            };
            s.finish()?;
            Some(st)
        }
    };
    let acks = match f.opt("acks") {
        None => Vec::new(),
        Some(av) => {
            let a = array(av, 0, BEAT_ACKS_MAX)?;
            let mut out: Vec<BeatAck> = Vec::with_capacity(a.len());
            for e in a {
                let mut am = Fields::of(e)?;
                let entry = int(am.req("entry")?, 0, INT_MAX)?;
                let verdict = match string(am.req("verdict")?)? {
                    "ack" => AckVerdict::Ack,
                    "mismatch" => AckVerdict::Mismatch,
                    _ => return Err(JsonError::Constraint),
                };
                am.finish()?;
                if out.last().is_some_and(|p| p.entry >= entry) {
                    return Err(JsonError::ArrayOrder);
                }
                if cx.is_some_and(|c| entry >= c.seq) {
                    return Err(JsonError::Constraint);
                }
                out.push(BeatAck { entry, verdict });
            }
            out
        }
    };
    f.finish()?;
    Ok(Beat { rules, envelopes, hash_versions, state, acks })
}

/// A beat's payload bytes, refused (`seal/bad_payload`) where its parser
/// without entry context rejects it.
pub(crate) fn encode_beat(b: &Beat) -> Result<Vec<u8>, SealError> {
    let bytes = beat_value(b).and_then(|v| canon::encode(&v)).map_err(|_| SealError::BadPayload)?;
    parse_beat_json(&bytes, None).map_err(|_| SealError::BadPayload)?;
    Ok(bytes)
}

/// §14.2 step 15 for kind `beat` (seal.md §11.4, SEAL-KIND-005,
/// SEAL-ENC-003, SEAL-ENC-004): the table of §11.4 with the entry's own
/// `seq` (every `acks.entry` below it) and its `based_on_seq` (`state.seq`
/// at most it). Any failure is `seal/bad_payload`.
pub(crate) fn parse_beat(payload: &[u8], cx: BeatContext) -> Result<Beat, SealError> {
    parse_beat_json(payload, Some(cx)).map_err(|_| SealError::BadPayload)
}

pub(crate) fn parse_beat_json(payload: &[u8], cx: Option<BeatContext>) -> Result<Beat, JsonError> {
    let v = canon::parse(payload)?;
    let b = beat_from_value(&v, cx)?;
    if canon::encode(&beat_value(&b)?)? != payload {
        return Err(JsonError::NotCanonical);
    }
    Ok(b)
}

// ----------------------------------------------------------- snap (§11.5)

fn snap_value(s: &SnapAnnounce) -> Result<Value, JsonError> {
    let mut m = Map::new();
    m.insert("seq".into(), int_value(s.seq)?);
    m.insert("hash".into(), hex_value(&s.hash));
    m.insert("hash_version".into(), int_value(u64::from(s.hash_version))?);
    m.insert("blob".into(), hex_value(&s.blob.0));
    m.insert("sha256".into(), hex_value(&s.sha256));
    m.insert("size".into(), int_value(s.size)?);
    m.insert("parts".into(), int_value(u64::from(s.parts))?);
    m.insert("inflated".into(), int_value(s.inflated)?);
    m.insert("epoch".into(), int_value(u64::from(s.epoch))?);
    Ok(Value::Object(m))
}

/// The entry fields a snap's table names: `based_on_seq` and the frame's epoch.
#[derive(Clone, Copy)]
pub(crate) struct SnapContext {
    pub based_on_seq: u64,
    pub epoch: u32,
}

fn snap_from_value(v: &Value, cx: Option<SnapContext>) -> Result<SnapAnnounce, JsonError> {
    let mut f = Fields::of(v)?;
    let seq = snapshot_seq(f.req("seq")?)?;
    if cx.is_some_and(|c| seq > c.based_on_seq) {
        return Err(JsonError::Constraint);
    }
    let hash = hex::<32>(f.req("hash")?)?;
    let hash_version = int32(f.req("hash_version")?, 1, VERSION_MAX)?;
    let blob = BlobId(hex::<16>(f.req("blob")?)?);
    let sha256 = hex::<32>(f.req("sha256")?)?;
    let (size, parts, inflated) = blob_dims(&mut f)?;
    // Epochs start at 1 (§6.5); the frame's epoch is never 0.
    let epoch = int32(f.req("epoch")?, 1, u32::MAX)?;
    if cx.is_some_and(|c| epoch != c.epoch) {
        return Err(JsonError::Constraint);
    }
    f.finish()?;
    Ok(SnapAnnounce { seq, hash, hash_version, blob, sha256, size, parts, inflated, epoch })
}

/// A snap's payload bytes, refused (`seal/bad_payload`) where its parser
/// without entry context rejects it.
pub(crate) fn encode_snap(s: &SnapAnnounce) -> Result<Vec<u8>, SealError> {
    let bytes = snap_value(s).and_then(|v| canon::encode(&v)).map_err(|_| SealError::BadPayload)?;
    parse_snap_json(&bytes, None).map_err(|_| SealError::BadPayload)?;
    Ok(bytes)
}

/// §14.2 step 15 for kind `snap` (seal.md §11.5, SEAL-KIND-006,
/// SEAL-ENC-003, SEAL-ENC-004): the table of §11.5 with the entry's
/// `based_on_seq` (`seq` at most it) and the frame's `epoch` (equal). Any
/// failure is `seal/bad_payload`.
pub(crate) fn parse_snap(payload: &[u8], cx: SnapContext) -> Result<SnapAnnounce, SealError> {
    parse_snap_json(payload, Some(cx)).map_err(|_| SealError::BadPayload)
}

pub(crate) fn parse_snap_json(payload: &[u8], cx: Option<SnapContext>) -> Result<SnapAnnounce, JsonError> {
    let v = canon::parse(payload)?;
    let s = snap_from_value(&v, cx)?;
    if canon::encode(&snap_value(&s)?)? != payload {
        return Err(JsonError::NotCanonical);
    }
    Ok(s)
}

#[cfg(test)]
mod tests;
