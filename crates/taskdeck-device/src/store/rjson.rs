//! Record JSON (device.md §6.5, DEV-STORE-007): the engine's `Registry`,
//! `OutboxRecord` and `Meta` as canonical JSON (`core.md` §4.2), by the
//! mapping of §6.5 applied to the Rust definitions of `engine.md` §37:
//!
//! - a struct is an object of its fields; an `Option` field that is `None`
//!   is absent, every other field present;
//! - a tuple is an array;
//! - a unit variant is its name in `snake_case`; a variant with fields is a
//!   one-member object of that name and the object of its fields; a tuple
//!   variant a one-member object of that name and its element;
//! - integers within ±(2^53 − 1), else `device/record_range` on writing;
//! - `[u8; N]` is `hexN`, `Vec<u8>` is `base64` (`core.md` §4.4).
//!
//! The engine's types have no `serde` implementations (finding f2-1), so the
//! mapping is written out here over `serde_json::Value`, and encoded and
//! parsed with `taskdeck_core::canon` (CORE-ENC-008), whose parser bounds the
//! depth at 32 and refuses duplicate keys and non-canonical bytes.

use base64::Engine as _;
use serde_json::{Map, Number, Value};
use taskdeck_sync::{
    AlarmKind, AlarmRecord, Banner, BannerDetail, BannerKind, BeatInfo, Body, CtlIntent, DutyState, Evidence,
    ForkSource, HaltKind, HaltRecord, Head, Kind, LastEntry, Meta, OpRecord, Operation, OutboxRecord, Registry,
    RelayTarget, Report, SnapInfo, SnapRecord, StopKind, StopRecord, UpgradeMode, Verdict, Via,
};

use super::codec::{RECORD_FORMAT, err, fmt};
use crate::DeviceError;

/// `device/record_range` (device.md §21).
pub(crate) const RECORD_RANGE: &str = "device/record_range";

/// The registry's JSON (device.md §6.4.2).
pub const REGISTRY_MAX: usize = 1_048_576;
/// One `OutboxRecord`'s JSON (device.md §6.4.4).
pub const OUTBOX_RECORD_MAX: usize = 1_048_576;
/// `meta.tdk`'s JSON (device.md §6.4.6).
pub const META_MAX: usize = 4_194_304;

const MAX_INT: u64 = (1 << 53) - 1;

type R<T> = Result<T, DeviceError>;

fn range() -> DeviceError {
    err(RECORD_RANGE)
}

/// One value of the mapping.
pub(crate) trait Rj: Sized {
    fn enc(&self) -> R<Value>;
    fn dec(v: &Value) -> R<Self>;
}

// ---------------------------------------------------------------- scalars

fn uint(v: u64) -> R<Value> {
    if v > MAX_INT { Err(range()) } else { Ok(Value::Number(Number::from(v))) }
}

fn get_uint(v: &Value, max: u64) -> R<u64> {
    let n = v.as_u64().ok_or_else(fmt)?;
    if n > max.min(MAX_INT) { Err(fmt()) } else { Ok(n) }
}

macro_rules! rj_uint {
    ($($t:ty),*) => {$(
        impl Rj for $t {
            fn enc(&self) -> R<Value> { uint(u64::from(*self)) }
            fn dec(v: &Value) -> R<Self> {
                <$t>::try_from(get_uint(v, u64::from(<$t>::MAX))?).map_err(|_| fmt())
            }
        }
    )*};
}
rj_uint!(u8, u16, u32, u64);

impl Rj for i64 {
    fn enc(&self) -> R<Value> {
        if self.unsigned_abs() > MAX_INT { Err(range()) } else { Ok(Value::Number(Number::from(*self))) }
    }
    fn dec(v: &Value) -> R<Self> {
        let n = v.as_i64().ok_or_else(fmt)?;
        if n.unsigned_abs() > MAX_INT { Err(fmt()) } else { Ok(n) }
    }
}

impl Rj for bool {
    fn enc(&self) -> R<Value> {
        Ok(Value::Bool(*self))
    }
    fn dec(v: &Value) -> R<Self> {
        v.as_bool().ok_or_else(fmt)
    }
}

impl Rj for String {
    fn enc(&self) -> R<Value> {
        Ok(Value::String(self.clone()))
    }
    fn dec(v: &Value) -> R<Self> {
        Ok(v.as_str().ok_or_else(fmt)?.to_string())
    }
}

/// `hexN` (`core.md` §4.4): exactly `2N` lowercase hexadecimal digits.
impl<const N: usize> Rj for [u8; N] {
    fn enc(&self) -> R<Value> {
        Ok(Value::String(self.iter().map(|b| format!("{b:02x}")).collect()))
    }
    fn dec(v: &Value) -> R<Self> {
        let s = v.as_str().ok_or_else(fmt)?.as_bytes();
        if s.len() != 2 * N {
            return Err(fmt());
        }
        let nib = |c: u8| match c {
            b'0'..=b'9' => Ok(c - b'0'),
            b'a'..=b'f' => Ok(c - b'a' + 10),
            _ => Err(fmt()),
        };
        let mut out = [0u8; N];
        for (i, o) in out.iter_mut().enumerate() {
            *o = (nib(s[2 * i])? << 4) | nib(s[2 * i + 1])?;
        }
        Ok(out)
    }
}

/// `base64` (`core.md` §4.4): RFC 4648 §4 with padding; only the encoding
/// of the decoded bytes is accepted.
fn b64(b: &[u8]) -> Value {
    Value::String(base64::engine::general_purpose::STANDARD.encode(b))
}

fn get_b64(v: &Value) -> R<Vec<u8>> {
    let s = v.as_str().ok_or_else(fmt)?;
    let b = base64::engine::general_purpose::STANDARD.decode(s).map_err(|_| fmt())?;
    if base64::engine::general_purpose::STANDARD.encode(&b) != s {
        return Err(fmt());
    }
    Ok(b)
}

impl<T: Rj> Rj for Vec<T> {
    fn enc(&self) -> R<Value> {
        Ok(Value::Array(self.iter().map(Rj::enc).collect::<R<_>>()?))
    }
    fn dec(v: &Value) -> R<Self> {
        v.as_array().ok_or_else(fmt)?.iter().map(T::dec).collect()
    }
}

fn arr(v: &Value, n: usize) -> R<&[Value]> {
    let a = v.as_array().ok_or_else(fmt)?;
    if a.len() != n { Err(fmt()) } else { Ok(a) }
}

impl<A: Rj, B: Rj> Rj for (A, B) {
    fn enc(&self) -> R<Value> {
        Ok(Value::Array(vec![self.0.enc()?, self.1.enc()?]))
    }
    fn dec(v: &Value) -> R<Self> {
        let a = arr(v, 2)?;
        Ok((A::dec(&a[0])?, B::dec(&a[1])?))
    }
}

impl<A: Rj, B: Rj, C: Rj> Rj for (A, B, C) {
    fn enc(&self) -> R<Value> {
        Ok(Value::Array(vec![self.0.enc()?, self.1.enc()?, self.2.enc()?]))
    }
    fn dec(v: &Value) -> R<Self> {
        let a = arr(v, 3)?;
        Ok((A::dec(&a[0])?, B::dec(&a[1])?, C::dec(&a[2])?))
    }
}

// ---------------------------------------------------------------- objects

/// An object being written.
struct Obj(Map<String, Value>);

impl Obj {
    fn new() -> Obj {
        Obj(Map::new())
    }
    /// A field that is always present.
    fn f<T: Rj>(mut self, k: &str, v: &T) -> R<Obj> {
        self.0.insert(k.to_string(), v.enc()?);
        Ok(self)
    }
    /// An `Option` field: absent when `None`.
    fn o<T: Rj>(mut self, k: &str, v: &Option<T>) -> R<Obj> {
        if let Some(x) = v {
            self.0.insert(k.to_string(), x.enc()?);
        }
        Ok(self)
    }
    /// A `Vec<u8>` field.
    fn b(mut self, k: &str, v: &[u8]) -> Obj {
        self.0.insert(k.to_string(), b64(v));
        self
    }
    /// An `Option<Vec<u8>>` field.
    fn ob(mut self, k: &str, v: &Option<Vec<u8>>) -> Obj {
        if let Some(x) = v {
            self.0.insert(k.to_string(), b64(x));
        }
        self
    }
    fn v(self) -> Value {
        Value::Object(self.0)
    }
}

/// An object being read: every member must be read once, no other.
struct Fields<'a> {
    m: &'a Map<String, Value>,
    seen: usize,
}

impl<'a> Fields<'a> {
    fn of(v: &'a Value) -> R<Fields<'a>> {
        Ok(Fields { m: v.as_object().ok_or_else(fmt)?, seen: 0 })
    }
    fn f<T: Rj>(&mut self, k: &str) -> R<T> {
        let v = self.m.get(k).ok_or_else(fmt)?;
        self.seen += 1;
        T::dec(v)
    }
    fn o<T: Rj>(&mut self, k: &str) -> R<Option<T>> {
        match self.m.get(k) {
            None => Ok(None),
            Some(v) => {
                self.seen += 1;
                Ok(Some(T::dec(v)?))
            }
        }
    }
    fn b(&mut self, k: &str) -> R<Vec<u8>> {
        let v = self.m.get(k).ok_or_else(fmt)?;
        self.seen += 1;
        get_b64(v)
    }
    fn ob(&mut self, k: &str) -> R<Option<Vec<u8>>> {
        match self.m.get(k) {
            None => Ok(None),
            Some(v) => {
                self.seen += 1;
                Ok(Some(get_b64(v)?))
            }
        }
    }
    /// No member the type does not have.
    fn end<T>(self, t: T) -> R<T> {
        if self.seen == self.m.len() { Ok(t) } else { Err(fmt()) }
    }
}

/// A variant: `(name, None)` for a unit variant's string, `(name,
/// Some(value))` for a one-member object.
fn variant(v: &Value) -> R<(&str, Option<&Value>)> {
    match v {
        Value::String(s) => Ok((s.as_str(), None)),
        Value::Object(m) if m.len() == 1 => {
            let (k, x) = m.iter().next().ok_or_else(fmt)?;
            Ok((k.as_str(), Some(x)))
        }
        _ => Err(fmt()),
    }
}

fn tag(name: &str, inner: Value) -> Value {
    let mut m = Map::new();
    m.insert(name.to_string(), inner);
    Value::Object(m)
}

fn unit(name: &str) -> Value {
    Value::String(name.to_string())
}

/// Unit-only enums: the names in `snake_case`.
macro_rules! rj_units {
    ($t:ty { $($v:ident => $n:literal),* $(,)? }) => {
        impl Rj for $t {
            fn enc(&self) -> R<Value> {
                Ok(unit(match self { $(<$t>::$v => $n),* }))
            }
            fn dec(v: &Value) -> R<Self> {
                match v.as_str().ok_or_else(fmt)? {
                    $($n => Ok(<$t>::$v),)*
                    _ => Err(fmt()),
                }
            }
        }
    };
}

rj_units!(Kind { Cmd => "cmd", Ctl => "ctl", Beat => "beat", Snap => "snap" });
rj_units!(Verdict { Ack => "ack", Mismatch => "mismatch" });
rj_units!(UpgradeMode { Normal => "normal", Override => "override" });
rj_units!(ForkSource {
    Welcome => "welcome",
    PageHead => "page_head",
    RepeatedEntry => "repeated_entry",
    BasedOn => "based_on",
});
rj_units!(BannerKind {
    DeviceAdded => "device_added",
    DeviceRevoked => "device_revoked",
    DeviceReinstated => "device_reinstated",
    KeysRotated => "keys_rotated",
    HistoryPurged => "history_purged",
    RecoveryChanged => "recovery_changed",
    Moved => "moved",
    Reset => "reset",
    Skipped => "skipped",
    RulesUpgraded => "rules_upgraded",
    RulesVote => "rules_vote",
});
rj_units!(AlarmKind {
    RelayBehind => "relay_behind",
    Fork => "fork",
    Counter => "counter",
    Unechoed => "unechoed",
    Withheld => "withheld",
    ForeignSigner => "foreign_signer",
    IdentityClone => "identity_clone",
    StateMismatch => "state_mismatch",
    SnapshotMismatch => "snapshot_mismatch",
    ResetMismatch => "reset_mismatch",
    SkippedFrame => "skipped_frame",
    RefusedFrame => "refused_frame",
    RelayProtocol => "relay_protocol",
    BlobFailure => "blob_failure",
    RestoredCopy => "restored_copy",
});

// ---------------------------------------------------------------- structs

impl Rj for Head {
    fn enc(&self) -> R<Value> {
        Ok(Obj::new().f("seq", &self.seq)?.f("chain", &self.chain)?.v())
    }
    fn dec(v: &Value) -> R<Self> {
        let mut f = Fields::of(v)?;
        let h = Head { seq: f.f("seq")?, chain: f.f("chain")? };
        f.end(h)
    }
}

impl Rj for RelayTarget {
    fn enc(&self) -> R<Value> {
        Ok(Obj::new().f("host", &self.host)?.f("door", &self.door)?.f("home_gen", &self.home_gen)?.v())
    }
    fn dec(v: &Value) -> R<Self> {
        let mut f = Fields::of(v)?;
        let t = RelayTarget { host: f.f("host")?, door: f.f("door")?, home_gen: f.f("home_gen")? };
        f.end(t)
    }
}

impl Rj for SnapInfo {
    fn enc(&self) -> R<Value> {
        Ok(Obj::new()
            .f("seq", &self.seq)?
            .f("hash", &self.hash)?
            .f("hash_version", &self.hash_version)?
            .f("blob", &self.blob)?
            .f("sha256", &self.sha256)?
            .f("size", &self.size)?
            .f("parts", &self.parts)?
            .f("inflated", &self.inflated)?
            .f("epoch", &self.epoch)?
            .v())
    }
    fn dec(v: &Value) -> R<Self> {
        let mut f = Fields::of(v)?;
        let s = SnapInfo {
            seq: f.f("seq")?,
            hash: f.f("hash")?,
            hash_version: f.f("hash_version")?,
            blob: f.f("blob")?,
            sha256: f.f("sha256")?,
            size: f.f("size")?,
            parts: f.f("parts")?,
            inflated: f.f("inflated")?,
            epoch: f.f("epoch")?,
        };
        f.end(s)
    }
}

impl Rj for BeatInfo {
    fn enc(&self) -> R<Value> {
        Ok(Obj::new()
            .f("rules", &self.rules)?
            .f("envelopes", &self.envelopes)?
            .f("hash_versions", &self.hash_versions)?
            .o("state", &self.state)?
            .f("acks", &self.acks)?
            .v())
    }
    fn dec(v: &Value) -> R<Self> {
        let mut f = Fields::of(v)?;
        let b = BeatInfo {
            rules: f.f("rules")?,
            envelopes: f.f("envelopes")?,
            hash_versions: f.f("hash_versions")?,
            state: f.o("state")?,
            acks: f.f("acks")?,
        };
        f.end(b)
    }
}

impl Rj for SnapRecord {
    fn enc(&self) -> R<Value> {
        Ok(Obj::new()
            .f("entry_seq", &self.entry_seq)?
            .f("author", &self.author)?
            .f("info", &self.info)?
            .f("entry_clock", &self.entry_clock)?
            .f("verdicts", &self.verdicts)?
            .o("own", &self.own)?
            .v())
    }
    fn dec(v: &Value) -> R<Self> {
        let mut f = Fields::of(v)?;
        let s = SnapRecord {
            entry_seq: f.f("entry_seq")?,
            author: f.f("author")?,
            info: f.f("info")?,
            entry_clock: f.f("entry_clock")?,
            verdicts: f.f("verdicts")?,
            own: f.o("own")?,
        };
        f.end(s)
    }
}

impl Rj for Report {
    fn enc(&self) -> R<Value> {
        Ok(Obj::new()
            .f("seq", &self.seq)?
            .f("dev", &self.dev)?
            .f("hash", &self.hash)?
            .f("hash_version", &self.hash_version)?
            .f("report_seq", &self.report_seq)?
            .v())
    }
    fn dec(v: &Value) -> R<Self> {
        let mut f = Fields::of(v)?;
        let r = Report {
            seq: f.f("seq")?,
            dev: f.f("dev")?,
            hash: f.f("hash")?,
            hash_version: f.f("hash_version")?,
            report_seq: f.f("report_seq")?,
        };
        f.end(r)
    }
}

impl Rj for LastEntry {
    fn enc(&self) -> R<Value> {
        Ok(Obj::new()
            .f("dev", &self.dev)?
            .f("at_ms", &self.at_ms)?
            .f("seq", &self.seq)?
            .f("based_on_seq", &self.based_on_seq)?
            .v())
    }
    fn dec(v: &Value) -> R<Self> {
        let mut f = Fields::of(v)?;
        let l = LastEntry { dev: f.f("dev")?, at_ms: f.f("at_ms")?, seq: f.f("seq")?, based_on_seq: f.f("based_on_seq")? };
        f.end(l)
    }
}

impl Rj for Registry {
    fn enc(&self) -> R<Value> {
        Ok(Obj::new()
            .f("snapshots", &self.snapshots)?
            .f("reports", &self.reports)?
            .f("pending_verdicts", &self.pending_verdicts)?
            .f("last_entry", &self.last_entry)?
            .o("restored", &self.restored)?
            .v())
    }
    fn dec(v: &Value) -> R<Self> {
        let mut f = Fields::of(v)?;
        let r = Registry {
            snapshots: f.f("snapshots")?,
            reports: f.f("reports")?,
            pending_verdicts: f.f("pending_verdicts")?,
            last_entry: f.f("last_entry")?,
            restored: f.o("restored")?,
        };
        f.end(r)
    }
}

impl Rj for CtlIntent {
    fn enc(&self) -> R<Value> {
        Ok(match self {
            CtlIntent::Rotate { revoke, reinstate, purge_before } => tag(
                "rotate",
                Obj::new().f("revoke", revoke)?.f("reinstate", reinstate)?.o("purge_before", purge_before)?.v(),
            ),
            CtlIntent::AddDevice { dev, sign_pk, hpke_pk, name, transcript, bootstrap_seq } => tag(
                "add_device",
                Obj::new()
                    .f("dev", dev)?
                    .f("sign_pk", sign_pk)?
                    .f("hpke_pk", hpke_pk)?
                    .f("name", name)?
                    .f("transcript", transcript)?
                    .f("bootstrap_seq", bootstrap_seq)?
                    .v(),
            ),
            CtlIntent::Fence { host, door, home_gen } => {
                tag("fence", Obj::new().f("host", host)?.f("door", door)?.f("home_gen", home_gen)?.v())
            }
            CtlIntent::ResetTo { seq, state_hash, hash_version } => tag(
                "reset_to",
                Obj::new().f("seq", seq)?.f("state_hash", state_hash)?.f("hash_version", hash_version)?.v(),
            ),
            CtlIntent::Skip { targets } => tag("skip", Obj::new().f("targets", targets)?.v()),
            CtlIntent::RulesUpgrade { rules, mode } => {
                tag("rules_upgrade", Obj::new().f("rules", rules)?.f("mode", mode)?.v())
            }
        })
    }
    fn dec(v: &Value) -> R<Self> {
        let (name, inner) = variant(v)?;
        let mut f = Fields::of(inner.ok_or_else(fmt)?)?;
        let c = match name {
            "rotate" => {
                CtlIntent::Rotate { revoke: f.f("revoke")?, reinstate: f.f("reinstate")?, purge_before: f.o("purge_before")? }
            }
            "add_device" => CtlIntent::AddDevice {
                dev: f.f("dev")?,
                sign_pk: f.f("sign_pk")?,
                hpke_pk: f.f("hpke_pk")?,
                name: f.f("name")?,
                transcript: f.f("transcript")?,
                bootstrap_seq: f.f("bootstrap_seq")?,
            },
            "fence" => CtlIntent::Fence { host: f.f("host")?, door: f.f("door")?, home_gen: f.f("home_gen")? },
            "reset_to" => {
                CtlIntent::ResetTo { seq: f.f("seq")?, state_hash: f.f("state_hash")?, hash_version: f.f("hash_version")? }
            }
            "skip" => CtlIntent::Skip { targets: f.f("targets")? },
            "rules_upgrade" => CtlIntent::RulesUpgrade { rules: f.f("rules")?, mode: f.f("mode")? },
            _ => return Err(fmt()),
        };
        f.end(c)
    }
}

impl Rj for Body {
    fn enc(&self) -> R<Value> {
        Ok(match self {
            Body::Cmd(b) => tag("cmd", b64(b)),
            Body::Ctl(c) => tag("ctl", c.enc()?),
            Body::Beat(b) => tag("beat", b.enc()?),
            Body::Snap(s) => tag("snap", s.enc()?),
        })
    }
    fn dec(v: &Value) -> R<Self> {
        let (name, inner) = variant(v)?;
        let inner = inner.ok_or_else(fmt)?;
        Ok(match name {
            "cmd" => Body::Cmd(get_b64(inner)?),
            "ctl" => Body::Ctl(CtlIntent::dec(inner)?),
            "beat" => Body::Beat(BeatInfo::dec(inner)?),
            "snap" => Body::Snap(SnapInfo::dec(inner)?),
            _ => return Err(fmt()),
        })
    }
}

impl Rj for Via {
    fn enc(&self) -> R<Value> {
        Ok(match self {
            Via::Append => unit("append"),
            Via::Register { dev, sign_pk } => tag("register", Obj::new().f("dev", dev)?.f("sign_pk", sign_pk)?.v()),
            Via::Fence { host, door, home_gen } => {
                tag("fence", Obj::new().f("host", host)?.f("door", door)?.f("home_gen", home_gen)?.v())
            }
        })
    }
    fn dec(v: &Value) -> R<Self> {
        match variant(v)? {
            ("append", None) => Ok(Via::Append),
            ("register", Some(x)) => {
                let mut f = Fields::of(x)?;
                let r = Via::Register { dev: f.f("dev")?, sign_pk: f.f("sign_pk")? };
                f.end(r)
            }
            ("fence", Some(x)) => {
                let mut f = Fields::of(x)?;
                let r = Via::Fence { host: f.f("host")?, door: f.f("door")?, home_gen: f.f("home_gen")? };
                f.end(r)
            }
            _ => Err(fmt()),
        }
    }
}

impl Rj for OutboxRecord {
    fn enc(&self) -> R<Value> {
        Ok(Obj::new()
            .f("entry_id", &self.entry_id)?
            .f("order", &self.order)?
            .f("at_ms", &self.at_ms)?
            .f("based_on", &self.based_on)?
            .f("kind", &self.kind)?
            .f("body", &self.body)?
            .b("origin_state", &self.origin_state)
            .f("via", &self.via)?
            .f("hold", &self.hold)?
            .o("ctr", &self.ctr)?
            .ob("frame", &self.frame)
            .o("frame_hash", &self.frame_hash)?
            .o("epoch", &self.epoch)?
            .o("ack", &self.ack)?
            .o("seen", &self.seen)?
            .o("op", &self.op)?
            .f("reseals", &self.reseals)?
            .v())
    }
    fn dec(v: &Value) -> R<Self> {
        let mut f = Fields::of(v)?;
        let r = OutboxRecord {
            entry_id: f.f("entry_id")?,
            order: f.f("order")?,
            at_ms: f.f("at_ms")?,
            based_on: f.f("based_on")?,
            kind: f.f("kind")?,
            body: f.f("body")?,
            origin_state: f.b("origin_state")?,
            via: f.f("via")?,
            hold: f.f("hold")?,
            ctr: f.o("ctr")?,
            frame: f.ob("frame")?,
            frame_hash: f.o("frame_hash")?,
            epoch: f.o("epoch")?,
            ack: f.o("ack")?,
            seen: f.o("seen")?,
            op: f.o("op")?,
            reseals: f.f("reseals")?,
        };
        f.end(r)
    }
}

impl Rj for HaltKind {
    fn enc(&self) -> R<Value> {
        Ok(match self {
            HaltKind::Crash { frame_hash } => tag("crash", Obj::new().f("frame_hash", frame_hash)?.v()),
            HaltKind::Rules { version } => tag("rules", Obj::new().f("version", version)?.v()),
            HaltKind::Commitment => unit("commitment"),
        })
    }
    fn dec(v: &Value) -> R<Self> {
        match variant(v)? {
            ("commitment", None) => Ok(HaltKind::Commitment),
            ("crash", Some(x)) => {
                let mut f = Fields::of(x)?;
                let h = HaltKind::Crash { frame_hash: f.f("frame_hash")? };
                f.end(h)
            }
            ("rules", Some(x)) => {
                let mut f = Fields::of(x)?;
                let h = HaltKind::Rules { version: f.f("version")? };
                f.end(h)
            }
            _ => Err(fmt()),
        }
    }
}

impl Rj for HaltRecord {
    fn enc(&self) -> R<Value> {
        Ok(Obj::new()
            .f("kind", &self.kind)?
            .f("seq", &self.seq)?
            .f("author", &self.author)?
            .f("since_ms", &self.since_ms)?
            .f("code", &self.code)?
            .v())
    }
    fn dec(v: &Value) -> R<Self> {
        let mut f = Fields::of(v)?;
        let h = HaltRecord {
            kind: f.f("kind")?,
            seq: f.f("seq")?,
            author: f.f("author")?,
            since_ms: f.f("since_ms")?,
            code: f.f("code")?,
        };
        f.end(h)
    }
}

impl Rj for StopKind {
    fn enc(&self) -> R<Value> {
        Ok(match self {
            StopKind::Evidence => unit("evidence"),
            StopKind::Divergence => unit("divergence"),
            StopKind::ForeignSigner => unit("foreign_signer"),
            StopKind::IdentityClone => unit("identity_clone"),
            StopKind::Revoked { by_log } => tag("revoked", Obj::new().f("by_log", by_log)?.v()),
            StopKind::LocalFault { code } => tag("local_fault", Obj::new().f("code", code)?.v()),
            StopKind::Stranded => unit("stranded"),
            StopKind::Rebootstrap { after } => tag("rebootstrap", Obj::new().f("after", after)?.v()),
            StopKind::Closed => unit("closed"),
            StopKind::Rejoin => unit("rejoin"),
        })
    }
    fn dec(v: &Value) -> R<Self> {
        let one = |x: &Value, k: &str| -> R<Value> {
            let m = x.as_object().ok_or_else(fmt)?;
            if m.len() != 1 {
                return Err(fmt());
            }
            m.get(k).cloned().ok_or_else(fmt)
        };
        Ok(match variant(v)? {
            ("evidence", None) => StopKind::Evidence,
            ("divergence", None) => StopKind::Divergence,
            ("foreign_signer", None) => StopKind::ForeignSigner,
            ("identity_clone", None) => StopKind::IdentityClone,
            ("stranded", None) => StopKind::Stranded,
            ("closed", None) => StopKind::Closed,
            ("rejoin", None) => StopKind::Rejoin,
            ("revoked", Some(x)) => StopKind::Revoked { by_log: bool::dec(&one(x, "by_log")?)? },
            ("local_fault", Some(x)) => StopKind::LocalFault { code: String::dec(&one(x, "code")?)? },
            ("rebootstrap", Some(x)) => StopKind::Rebootstrap { after: u64::dec(&one(x, "after")?)? },
            _ => return Err(fmt()),
        })
    }
}

impl Rj for StopRecord {
    fn enc(&self) -> R<Value> {
        Ok(Obj::new().f("kind", &self.kind)?.f("since_ms", &self.since_ms)?.o("alarm", &self.alarm)?.v())
    }
    fn dec(v: &Value) -> R<Self> {
        let mut f = Fields::of(v)?;
        let s = StopRecord { kind: f.f("kind")?, since_ms: f.f("since_ms")?, alarm: f.o("alarm")? };
        f.end(s)
    }
}

impl Rj for Evidence {
    fn enc(&self) -> R<Value> {
        Ok(match self {
            Evidence::RelayBehind { relay_head, relay_last_ctr, local_head, ack } => tag(
                "relay_behind",
                Obj::new()
                    .f("relay_head", relay_head)?
                    .o("relay_last_ctr", relay_last_ctr)?
                    .f("local_head", local_head)?
                    .o("ack", ack)?
                    .v(),
            ),
            Evidence::Fork { local_chain, relay_chain, source } => tag(
                "fork",
                Obj::new().f("local_chain", local_chain)?.f("relay_chain", relay_chain)?.f("source", source)?.v(),
            ),
            Evidence::Counter { ctr, expected } => {
                tag("counter", Obj::new().f("ctr", ctr)?.f("expected", expected)?.v())
            }
            Evidence::Unechoed { ack_frame_hash, ack_chain, log_frame_hash, log_chain } => tag(
                "unechoed",
                Obj::new()
                    .f("ack_frame_hash", ack_frame_hash)?
                    .f("ack_chain", ack_chain)?
                    .f("log_frame_hash", log_frame_hash)?
                    .f("log_chain", log_chain)?
                    .v(),
            ),
            Evidence::Withheld { relay_head, claimed_ctr, cursor, missing_from, missing_to, counters } => tag(
                "withheld",
                Obj::new()
                    .f("relay_head", relay_head)?
                    .o("claimed_ctr", claimed_ctr)?
                    .f("cursor", cursor)?
                    .f("missing_from", missing_from)?
                    .f("missing_to", missing_to)?
                    .f("counters", counters)?
                    .v(),
            ),
            Evidence::ForeignSigner { ctr } => tag("foreign_signer", Obj::new().f("ctr", ctr)?.v()),
            Evidence::IdentityClone { session, ctrs } => {
                tag("identity_clone", Obj::new().o("session", session)?.f("ctrs", ctrs)?.v())
            }
            Evidence::Mismatch { groups } => tag("mismatch", Obj::new().f("groups", groups)?.v()),
            Evidence::Frame { frame_hash } => tag("frame", Obj::new().o("frame_hash", frame_hash)?.v()),
            Evidence::Request { request } => tag("request", Obj::new().f("request", request)?.v()),
            Evidence::Blob { blob } => tag("blob", Obj::new().f("blob", blob)?.v()),
            Evidence::RestoredCopy { entries } => tag("restored_copy", Obj::new().f("entries", entries)?.v()),
        })
    }
    fn dec(v: &Value) -> R<Self> {
        let (name, inner) = variant(v)?;
        let mut f = Fields::of(inner.ok_or_else(fmt)?)?;
        let e = match name {
            "relay_behind" => Evidence::RelayBehind {
                relay_head: f.f("relay_head")?,
                relay_last_ctr: f.o("relay_last_ctr")?,
                local_head: f.f("local_head")?,
                ack: f.o("ack")?,
            },
            "fork" => {
                Evidence::Fork { local_chain: f.f("local_chain")?, relay_chain: f.f("relay_chain")?, source: f.f("source")? }
            }
            "counter" => Evidence::Counter { ctr: f.f("ctr")?, expected: f.f("expected")? },
            "unechoed" => Evidence::Unechoed {
                ack_frame_hash: f.f("ack_frame_hash")?,
                ack_chain: f.f("ack_chain")?,
                log_frame_hash: f.f("log_frame_hash")?,
                log_chain: f.f("log_chain")?,
            },
            "withheld" => Evidence::Withheld {
                relay_head: f.f("relay_head")?,
                claimed_ctr: f.o("claimed_ctr")?,
                cursor: f.f("cursor")?,
                missing_from: f.f("missing_from")?,
                missing_to: f.f("missing_to")?,
                counters: f.f("counters")?,
            },
            "foreign_signer" => Evidence::ForeignSigner { ctr: f.f("ctr")? },
            "identity_clone" => Evidence::IdentityClone { session: f.o("session")?, ctrs: f.f("ctrs")? },
            "mismatch" => Evidence::Mismatch { groups: f.f("groups")? },
            "frame" => Evidence::Frame { frame_hash: f.o("frame_hash")? },
            "request" => Evidence::Request { request: f.f("request")? },
            "blob" => Evidence::Blob { blob: f.f("blob")? },
            "restored_copy" => Evidence::RestoredCopy { entries: f.f("entries")? },
            _ => return Err(fmt()),
        };
        f.end(e)
    }
}

impl Rj for AlarmRecord {
    fn enc(&self) -> R<Value> {
        Ok(Obj::new()
            .f("id", &self.id)?
            .f("kind", &self.kind)?
            .f("raised_ms", &self.raised_ms)?
            .o("seq", &self.seq)?
            .o("author", &self.author)?
            .o("code", &self.code)?
            .f("evidence", &self.evidence)?
            .f("resolved", &self.resolved)?
            .v())
    }
    fn dec(v: &Value) -> R<Self> {
        let mut f = Fields::of(v)?;
        let a = AlarmRecord {
            id: f.f("id")?,
            kind: f.f("kind")?,
            raised_ms: f.f("raised_ms")?,
            seq: f.o("seq")?,
            author: f.o("author")?,
            code: f.o("code")?,
            evidence: f.f("evidence")?,
            resolved: f.f("resolved")?,
        };
        f.end(a)
    }
}

impl Rj for BannerDetail {
    fn enc(&self) -> R<Value> {
        Ok(match self {
            BannerDetail::None => unit("none"),
            BannerDetail::Purge { before_seq } => tag("purge", Obj::new().f("before_seq", before_seq)?.v()),
            BannerDetail::Recovery { key_fp } => tag("recovery", Obj::new().f("key_fp", key_fp)?.v()),
            BannerDetail::Moved { host, home_gen } => {
                tag("moved", Obj::new().f("host", host)?.f("home_gen", home_gen)?.v())
            }
            BannerDetail::Reset { target } => tag("reset", Obj::new().f("target", target)?.v()),
            BannerDetail::Skipped { target, target_author } => {
                tag("skipped", Obj::new().f("target", target)?.f("target_author", target_author)?.v())
            }
            BannerDetail::Rules { version } => tag("rules", Obj::new().f("version", version)?.v()),
        })
    }
    fn dec(v: &Value) -> R<Self> {
        let (name, inner) = variant(v)?;
        if name == "none" {
            return if inner.is_none() { Ok(BannerDetail::None) } else { Err(fmt()) };
        }
        let mut f = Fields::of(inner.ok_or_else(fmt)?)?;
        let d = match name {
            "purge" => BannerDetail::Purge { before_seq: f.f("before_seq")? },
            "recovery" => BannerDetail::Recovery { key_fp: f.f("key_fp")? },
            "moved" => BannerDetail::Moved { host: f.f("host")?, home_gen: f.f("home_gen")? },
            "reset" => BannerDetail::Reset { target: f.f("target")? },
            "skipped" => BannerDetail::Skipped { target: f.f("target")?, target_author: f.f("target_author")? },
            "rules" => BannerDetail::Rules { version: f.f("version")? },
            _ => return Err(fmt()),
        };
        f.end(d)
    }
}

impl Rj for Banner {
    fn enc(&self) -> R<Value> {
        Ok(Obj::new()
            .f("id", &self.id)?
            .f("kind", &self.kind)?
            .f("seq", &self.seq)?
            .f("at_ms", &self.at_ms)?
            .f("author", &self.author)?
            .f("author_name", &self.author_name)?
            .f("author_fp", &self.author_fp)?
            .o("subject", &self.subject)?
            .o("subject_name", &self.subject_name)?
            .o("subject_fp", &self.subject_fp)?
            .f("detail", &self.detail)?
            .f("acked", &self.acked)?
            .v())
    }
    fn dec(v: &Value) -> R<Self> {
        let mut f = Fields::of(v)?;
        let b = Banner {
            id: f.f("id")?,
            kind: f.f("kind")?,
            seq: f.f("seq")?,
            at_ms: f.f("at_ms")?,
            author: f.f("author")?,
            author_name: f.f("author_name")?,
            author_fp: f.f("author_fp")?,
            subject: f.o("subject")?,
            subject_name: f.o("subject_name")?,
            subject_fp: f.o("subject_fp")?,
            detail: f.f("detail")?,
            acked: f.f("acked")?,
        };
        f.end(b)
    }
}

impl Rj for DutyState {
    fn enc(&self) -> R<Value> {
        Ok(Obj::new()
            .o("snapshot", &self.snapshot)?
            .f("last_compact_day", &self.last_compact_day)?
            .o("purge", &self.purge)?
            .o("upgrade", &self.upgrade)?
            .v())
    }
    fn dec(v: &Value) -> R<Self> {
        let mut f = Fields::of(v)?;
        let d = DutyState {
            snapshot: f.o("snapshot")?,
            last_compact_day: f.f("last_compact_day")?,
            purge: f.o("purge")?,
            upgrade: f.o("upgrade")?,
        };
        f.end(d)
    }
}

impl Rj for Operation {
    fn enc(&self) -> R<Value> {
        Ok(match self {
            Operation::RestoreTo { seq } => tag("restore_to", Obj::new().f("seq", seq)?.v()),
            Operation::Purge => unit("purge"),
            Operation::Move { host } => tag("move", Obj::new().f("host", host)?.v()),
            Operation::FollowAddress { host, door, code } => {
                tag("follow_address", Obj::new().f("host", host)?.f("door", door)?.f("code", code)?.v())
            }
            Operation::Revoke { dev, replace } => tag("revoke", Obj::new().f("dev", dev)?.f("replace", replace)?.v()),
            Operation::Skip { seq } => tag("skip", Obj::new().f("seq", seq)?.v()),
            Operation::RulesOverride { version } => tag("rules_override", Obj::new().f("version", version)?.v()),
            Operation::CloseBoard { host, door } => {
                tag("close_board", Obj::new().f("host", host)?.f("door", door)?.v())
            }
            Operation::Adopt { dev, typed_fp } => tag("adopt", Obj::new().f("dev", dev)?.f("typed_fp", typed_fp)?.v()),
        })
    }
    fn dec(v: &Value) -> R<Self> {
        let (name, inner) = variant(v)?;
        if name == "purge" {
            return if inner.is_none() { Ok(Operation::Purge) } else { Err(fmt()) };
        }
        let mut f = Fields::of(inner.ok_or_else(fmt)?)?;
        let o = match name {
            "restore_to" => Operation::RestoreTo { seq: f.f("seq")? },
            "move" => Operation::Move { host: f.f("host")? },
            "follow_address" => Operation::FollowAddress { host: f.f("host")?, door: f.f("door")?, code: f.f("code")? },
            "revoke" => Operation::Revoke { dev: f.f("dev")?, replace: f.f("replace")? },
            "skip" => Operation::Skip { seq: f.f("seq")? },
            "rules_override" => Operation::RulesOverride { version: f.f("version")? },
            "close_board" => Operation::CloseBoard { host: f.f("host")?, door: f.f("door")? },
            "adopt" => Operation::Adopt { dev: f.f("dev")?, typed_fp: f.f("typed_fp")? },
            _ => return Err(fmt()),
        };
        f.end(o)
    }
}

impl Rj for OpRecord {
    fn enc(&self) -> R<Value> {
        Ok(Obj::new()
            .f("ticket", &self.ticket)?
            .f("op", &self.op)?
            .f("stage", &self.stage)?
            .f("attempts", &self.attempts)?
            .f("started_ms", &self.started_ms)?
            .f("mark", &self.mark)?
            .v())
    }
    fn dec(v: &Value) -> R<Self> {
        let mut f = Fields::of(v)?;
        let o = OpRecord {
            ticket: f.f("ticket")?,
            op: f.f("op")?,
            stage: f.f("stage")?,
            attempts: f.f("attempts")?,
            started_ms: f.f("started_ms")?,
            mark: f.f("mark")?,
        };
        f.end(o)
    }
}

impl Rj for Meta {
    fn enc(&self) -> R<Value> {
        Ok(Obj::new()
            .f("attempt", &self.attempt)?
            .o("last_failed", &self.last_failed)?
            .f("build_id", &self.build_id)?
            .o("halt", &self.halt)?
            .f("stops", &self.stops)?
            .f("alarms", &self.alarms)?
            .f("banners", &self.banners)?
            .f("relay", &self.relay)?
            .o("prev_session", &self.prev_session)?
            .f("conflicts", &self.conflicts)?
            .o("clock", &self.clock)?
            .f("last_beat_ms", &self.last_beat_ms)?
            .f("duties", &self.duties)?
            .f("ops", &self.ops)?
            .f("move_tries", &self.move_tries)?
            .o("continuity_unchecked", &self.continuity_unchecked)?
            .o("restored_copy", &self.restored_copy)?
            .f("based_on_unchecked", &self.based_on_unchecked)?
            .v())
    }
    fn dec(v: &Value) -> R<Self> {
        let mut f = Fields::of(v)?;
        let m = Meta {
            attempt: f.f("attempt")?,
            last_failed: f.o("last_failed")?,
            build_id: f.f("build_id")?,
            halt: f.o("halt")?,
            stops: f.f("stops")?,
            alarms: f.f("alarms")?,
            banners: f.f("banners")?,
            relay: f.f("relay")?,
            prev_session: f.o("prev_session")?,
            conflicts: f.f("conflicts")?,
            clock: f.o("clock")?,
            last_beat_ms: f.f("last_beat_ms")?,
            duties: f.f("duties")?,
            ops: f.f("ops")?,
            move_tries: f.f("move_tries")?,
            continuity_unchecked: f.o("continuity_unchecked")?,
            restored_copy: f.o("restored_copy")?,
            based_on_unchecked: f.f("based_on_unchecked")?,
        };
        f.end(m)
    }
}

// ---------------------------------------------------------------- bytes

/// The canonical bytes of a mapped value within `limit`; a value over the
/// limit cannot be written (`device/record_range`, a local fault).
fn to_bytes<T: Rj>(t: &T, limit: usize) -> R<Vec<u8>> {
    let b = taskdeck_core::canon::encode(&t.enc()?).map_err(|_| range())?;
    if b.len() > limit { Err(range()) } else { Ok(b) }
}

/// The canonical parser (`core.md` §4.5, depth 32) then the mapping; any
/// refusal is `device/record_format`.
fn from_bytes<T: Rj>(b: &[u8], limit: usize) -> R<T> {
    let v = taskdeck_core::canon::parse(b, limit).map_err(|_| err(RECORD_FORMAT))?;
    T::dec(&v)
}

pub(crate) fn registry_bytes(r: &Registry) -> R<Vec<u8>> {
    to_bytes(r, REGISTRY_MAX)
}
pub(crate) fn registry_from(b: &[u8]) -> R<Registry> {
    from_bytes(b, REGISTRY_MAX)
}
pub(crate) fn outbox_record_bytes(r: &OutboxRecord) -> R<Vec<u8>> {
    to_bytes(r, OUTBOX_RECORD_MAX)
}
pub(crate) fn outbox_record_from(b: &[u8]) -> R<OutboxRecord> {
    from_bytes(b, OUTBOX_RECORD_MAX)
}
pub(crate) fn meta_bytes(m: &Meta) -> R<Vec<u8>> {
    to_bytes(m, META_MAX)
}
pub(crate) fn meta_from(b: &[u8]) -> R<Meta> {
    from_bytes(b, META_MAX)
}
