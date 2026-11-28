//! The per-device settings record of `device.tdk` (kind 12; device.md §16),
//! in the canonical encoding (`core.md` §4.2) with its defaults (§4.3).
//!
//! The store reads and writes the fields it owns itself: `device_name`,
//! `log_floor` and `floor_chain` (§7.3), `minter` (§8.3) and
//! `history_complete` (§15.7). The other fields of the table are carried
//! verbatim in `other`, checked only by name and by being absent at their
//! default where the table gives one, until the tasks that own them read
//! them (finding f2-4).

use std::collections::BTreeMap;

use serde_json::{Map, Number, Value};
use taskdeck_core::ids::IdMinter;
use zeroize::{Zeroize, Zeroizing};

use super::codec::{RECORD_FORMAT, err, fmt};
use super::rjson::RECORD_RANGE;
use crate::DeviceError;

/// `device.tdk` / `device.json` (device.md §16, §22).
pub const SETTINGS_MAX: usize = 1_048_576;

/// The fields of §16 that the store carries without reading.
const OTHER_FIELDS: &[&str] = &[
    "added_here",
    "backup",
    "look",
    "named",
    "own_place",
    "quarantined",
    "reminders",
    "requests",
    "weather_source",
];

const MAX_INT: u64 = (1 << 53) - 1;

/// A relay token and capability stored for one host (device.md §10.3, §16
/// `tokens`). The token is opaque (SEAL-KEY-015), held zeroizing, and never
/// printed: `Debug` shows its host, generation and `push` only (DEV-NET-005).
#[derive(Clone, PartialEq, Eq)]
pub struct RelayToken {
    pub host: String,
    pub generation: u32,
    /// Absent until the device has learned the relay's capability.
    pub push: Option<bool>,
    pub token: Zeroizing<String>,
}

impl std::fmt::Debug for RelayToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RelayToken").field("host", &self.host).field("gen", &self.generation).field("push", &self.push).finish_non_exhaustive()
    }
}

/// At most 4 tokens (device.md §16).
const TOKENS_MAX: usize = 4;

/// The record of `device.tdk` (device.md §16).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceSettings {
    pub device_name: String,
    /// Default 1.
    pub log_floor: u64,
    pub floor_chain: Option<[u8; 32]>,
    pub minter: Option<IdMinter>,
    /// Default `false`.
    pub history_complete: bool,
    /// Default `[]`; ascending by host, distinct.
    pub tokens: Vec<RelayToken>,
    /// Every other field of the table, by name, as written.
    pub other: BTreeMap<String, Value>,
}

impl DeviceSettings {
    /// A new identity's settings: the name, `log_floor` 1, no floor chain.
    pub fn new(device_name: &str) -> DeviceSettings {
        DeviceSettings {
            device_name: device_name.to_string(),
            log_floor: 1,
            floor_chain: None,
            minter: None,
            history_complete: false,
            tokens: Vec::new(),
            other: BTreeMap::new(),
        }
    }

    fn to_value(&self) -> Result<Value, DeviceError> {
        let mut m = Map::new();
        for (k, v) in &self.other {
            if !OTHER_FIELDS.contains(&k.as_str()) {
                return Err(err(RECORD_RANGE));
            }
            m.insert(k.clone(), v.clone());
        }
        m.insert("device_name".into(), Value::String(self.device_name.clone()));
        if self.log_floor != 1 {
            if self.log_floor > MAX_INT {
                return Err(err(RECORD_RANGE));
            }
            m.insert("log_floor".into(), Value::Number(Number::from(self.log_floor)));
        }
        if let Some(c) = &self.floor_chain {
            m.insert("floor_chain".into(), Value::String(c.iter().map(|b| format!("{b:02x}")).collect()));
        }
        if self.history_complete {
            m.insert("history_complete".into(), Value::Bool(true));
        }
        if let Some(mi) = &self.minter {
            if mi.next_item > MAX_INT || mi.next_block > MAX_INT {
                return Err(err(RECORD_RANGE));
            }
            let mut o = Map::new();
            o.insert("block".into(), Value::Number(Number::from(mi.next_block)));
            o.insert("device_no".into(), Value::Number(Number::from(mi.device_no)));
            o.insert("item".into(), Value::Number(Number::from(mi.next_item)));
            m.insert("minter".into(), Value::Object(o));
        }
        if !self.tokens.is_empty() {
            if self.tokens.len() > TOKENS_MAX || self.tokens.windows(2).any(|w| w[0].host >= w[1].host) {
                return Err(err(RECORD_RANGE));
            }
            let mut a = Vec::with_capacity(self.tokens.len());
            for t in &self.tokens {
                if !token_ok(&t.host, &t.token) {
                    return Err(err(RECORD_RANGE));
                }
                let mut o = Map::new();
                o.insert("gen".into(), Value::Number(Number::from(t.generation)));
                o.insert("host".into(), Value::String(t.host.clone()));
                if let Some(p) = t.push {
                    o.insert("push".into(), Value::Bool(p));
                }
                o.insert("token".into(), Value::String(t.token.to_string()));
                a.push(Value::Object(o));
            }
            m.insert("tokens".into(), Value::Array(a));
        }
        Ok(Value::Object(m))
    }

    /// The token stored for `host`, if any.
    pub fn token(&self, host: &str) -> Option<&RelayToken> {
        self.tokens.iter().find(|t| t.host == host)
    }

    /// Stores `t`, replacing the token of its host (ascending by host). A
    /// fifth host replaces the first host other than `t`'s (device.md §16:
    /// at most 4).
    pub fn put_token(&mut self, t: RelayToken) {
        self.tokens.retain(|x| x.host != t.host);
        if self.tokens.len() >= TOKENS_MAX {
            self.tokens.remove(0);
        }
        let at = self.tokens.partition_point(|x| x.host < t.host);
        self.tokens.insert(at, t);
    }

    /// The canonical bytes (device.md §16), zeroizing: they may hold tokens.
    pub fn to_bytes(&self) -> Result<Zeroizing<Vec<u8>>, DeviceError> {
        let mut v = self.to_value()?;
        let b = taskdeck_core::canon::encode(&v).map_err(|_| err(RECORD_RANGE));
        wipe_tokens(&mut v);
        let b = Zeroizing::new(b?);
        if b.len() > SETTINGS_MAX { Err(err(RECORD_RANGE)) } else { Ok(b) }
    }

    /// Reads the record: canonical bytes (which also refuses a field written
    /// at its default), the fields the store reads checked against the table,
    /// no field the table lacks; else `device/record_format`.
    pub fn from_bytes(b: &[u8]) -> Result<DeviceSettings, DeviceError> {
        let mut v = taskdeck_core::canon::parse(b, SETTINGS_MAX).map_err(|_| err(RECORD_FORMAT))?;
        let r = Self::from_value(&v);
        wipe_tokens(&mut v);
        r
    }

    fn from_value(v: &Value) -> Result<DeviceSettings, DeviceError> {
        let m = v.as_object().ok_or_else(fmt)?;
        let mut s = DeviceSettings::new("");
        let mut name = None;
        for (k, v) in m {
            match k.as_str() {
                "device_name" => name = Some(v.as_str().ok_or_else(fmt)?.to_string()),
                "log_floor" => {
                    let n = v.as_u64().filter(|n| *n <= MAX_INT && *n != 1).ok_or_else(fmt)?;
                    s.log_floor = n;
                }
                "floor_chain" => {
                    let t = v.as_str().ok_or_else(fmt)?.as_bytes();
                    if t.len() != 64 {
                        return Err(fmt());
                    }
                    let nib = |c: u8| match c {
                        b'0'..=b'9' => Ok(c - b'0'),
                        b'a'..=b'f' => Ok(c - b'a' + 10),
                        _ => Err(fmt()),
                    };
                    let mut c = [0u8; 32];
                    for (i, o) in c.iter_mut().enumerate() {
                        *o = (nib(t[2 * i])? << 4) | nib(t[2 * i + 1])?;
                    }
                    s.floor_chain = Some(c);
                }
                "history_complete" => {
                    if v.as_bool() != Some(true) {
                        return Err(fmt());
                    }
                    s.history_complete = true;
                }
                "minter" => {
                    let o = v.as_object().filter(|o| o.len() == 3).ok_or_else(fmt)?;
                    let get = |k: &str| o.get(k).and_then(Value::as_u64).filter(|n| *n <= MAX_INT).ok_or_else(fmt);
                    let device_no = u16::try_from(get("device_no")?).ok().filter(|n| (1..=8191).contains(n));
                    s.minter = Some(IdMinter {
                        device_no: device_no.ok_or_else(fmt)?,
                        next_item: get("item")?,
                        next_block: get("block")?,
                    });
                }
                "tokens" => {
                    let a = v.as_array().filter(|a| !a.is_empty() && a.len() <= TOKENS_MAX).ok_or_else(fmt)?;
                    for t in a {
                        let o = t.as_object().ok_or_else(fmt)?;
                        let push = match o.get("push") {
                            None => None,
                            Some(p) => Some(p.as_bool().ok_or_else(fmt)?),
                        };
                        if o.len() != 3 + usize::from(push.is_some()) {
                            return Err(fmt());
                        }
                        let generation = o.get("gen").and_then(Value::as_u64).and_then(|g| u32::try_from(g).ok()).ok_or_else(fmt)?;
                        let host = o.get("host").and_then(Value::as_str).ok_or_else(fmt)?.to_string();
                        let token = Zeroizing::new(o.get("token").and_then(Value::as_str).ok_or_else(fmt)?.to_string());
                        if !token_ok(&host, &token) || s.tokens.last().is_some_and(|l| l.host >= host) {
                            return Err(fmt());
                        }
                        s.tokens.push(RelayToken { host, generation, push, token });
                    }
                }
                k if OTHER_FIELDS.contains(&k) => {
                    s.other.insert(k.to_string(), v.clone());
                }
                _ => return Err(fmt()),
            }
        }
        s.device_name = name.ok_or_else(fmt)?;
        Ok(s)
    }
}

/// A host of `seal.md` §5.4 and an ASCII token of 1 to 512 bytes (§16).
fn token_ok(host: &str, token: &str) -> bool {
    taskdeck_seal::relay_origin(host).is_ok() && (1..=512).contains(&token.len()) && token.bytes().all(|b| b.is_ascii_graphic())
}

/// Overwrites the token strings of a settings value before it is dropped.
fn wipe_tokens(v: &mut Value) {
    if let Some(Value::Array(a)) = v.get_mut("tokens") {
        for t in a {
            if let Some(Value::String(s)) = t.get_mut("token") {
                s.zeroize();
            }
        }
    }
}

/// The element-wise maximum of two minters (device.md §8.3).
pub(crate) fn max_minter(a: Option<IdMinter>, b: Option<IdMinter>) -> Option<IdMinter> {
    match (a, b) {
        (None, x) | (x, None) => x,
        (Some(a), Some(b)) => Some(IdMinter {
            device_no: a.device_no.max(b.device_no),
            next_item: a.next_item.max(b.next_item),
            next_block: a.next_block.max(b.next_block),
        }),
    }
}
