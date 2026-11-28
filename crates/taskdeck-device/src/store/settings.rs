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
    "tokens",
    "weather_source",
];

const MAX_INT: u64 = (1 << 53) - 1;

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
        Ok(Value::Object(m))
    }

    /// The canonical bytes (device.md §16).
    pub fn to_bytes(&self) -> Result<Vec<u8>, DeviceError> {
        let b = taskdeck_core::canon::encode(&self.to_value()?).map_err(|_| err(RECORD_RANGE))?;
        if b.len() > SETTINGS_MAX { Err(err(RECORD_RANGE)) } else { Ok(b) }
    }

    /// Reads the record: canonical bytes (which also refuses a field written
    /// at its default), the fields the store reads checked against the table,
    /// no field the table lacks; else `device/record_format`.
    pub fn from_bytes(b: &[u8]) -> Result<DeviceSettings, DeviceError> {
        let v = taskdeck_core::canon::parse(b, SETTINGS_MAX).map_err(|_| err(RECORD_FORMAT))?;
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
