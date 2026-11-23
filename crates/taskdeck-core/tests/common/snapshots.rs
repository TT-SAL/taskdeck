//! Every file under `fixtures/snapshots/` (core.md §18.5), embedded: the zone
//! directory, the board, the file name (which carries `from`, `days` and `now`)
//! and the recorded JSON; and the request a file name holds and §18.5's
//! transformation of the recorded JSON, for every target that compares one.

use std::collections::BTreeSet;

use serde_json::Value;
use taskdeck_core::board::BoardData;
use taskdeck_core::time::Instant;

/// `(zone directory, board, file name, text)` of each recorded snapshot.
pub const FILES: [(&str, &str, &str, &str); 48] = [
    ("america-new-york", "dst-edges", "1970-01-01_1d_at-20261025T0130Z.json", include_str!("../../../../fixtures/snapshots/america-new-york/dst-edges/1970-01-01_1d_at-20261025T0130Z.json")),
    ("america-new-york", "dst-edges", "2026-03-01_31d_at-20260329T0130Z.json", include_str!("../../../../fixtures/snapshots/america-new-york/dst-edges/2026-03-01_31d_at-20260329T0130Z.json")),
    ("america-new-york", "dst-edges", "2026-03-08_1d_at-20261101T0530Z.json", include_str!("../../../../fixtures/snapshots/america-new-york/dst-edges/2026-03-08_1d_at-20261101T0530Z.json")),
    ("america-new-york", "dst-edges", "2026-03-29_1d_at-20260329T0130Z.json", include_str!("../../../../fixtures/snapshots/america-new-york/dst-edges/2026-03-29_1d_at-20260329T0130Z.json")),
    ("america-new-york", "dst-edges", "2026-10-20_2d_at-20261025T0130Z.json", include_str!("../../../../fixtures/snapshots/america-new-york/dst-edges/2026-10-20_2d_at-20261025T0130Z.json")),
    ("america-new-york", "dst-edges", "2026-10-22_7d_at-20261025T0130Z.json", include_str!("../../../../fixtures/snapshots/america-new-york/dst-edges/2026-10-22_7d_at-20261025T0130Z.json")),
    ("america-new-york", "dst-edges", "2026-10-24_1d_at-20261025T0130Z.json", include_str!("../../../../fixtures/snapshots/america-new-york/dst-edges/2026-10-24_1d_at-20261025T0130Z.json")),
    ("america-new-york", "dst-edges", "2026-10-25_1d_at-20261025T0130Z.json", include_str!("../../../../fixtures/snapshots/america-new-york/dst-edges/2026-10-25_1d_at-20261025T0130Z.json")),
    ("america-new-york", "dst-edges", "2026-11-01_1d_at-20261101T0530Z.json", include_str!("../../../../fixtures/snapshots/america-new-york/dst-edges/2026-11-01_1d_at-20261101T0530Z.json")),
    ("america-new-york", "dst-edges", "2026-11-01_1d_at-20261101T0630Z.json", include_str!("../../../../fixtures/snapshots/america-new-york/dst-edges/2026-11-01_1d_at-20261101T0630Z.json")),
    ("america-new-york", "dst-edges", "2026-12-31_2d_at-20261025T0130Z.json", include_str!("../../../../fixtures/snapshots/america-new-york/dst-edges/2026-12-31_2d_at-20261025T0130Z.json")),
    ("america-new-york", "dst-edges", "9999-12-31_1d_at-20261025T0130Z.json", include_str!("../../../../fixtures/snapshots/america-new-york/dst-edges/9999-12-31_1d_at-20261025T0130Z.json")),
    ("america-new-york", "small", "2026-10-05_31d_at-20261020T0815Z.json", include_str!("../../../../fixtures/snapshots/america-new-york/small/2026-10-05_31d_at-20261020T0815Z.json")),
    ("america-new-york", "small", "2026-10-19_7d_at-20261020T0815Z.json", include_str!("../../../../fixtures/snapshots/america-new-york/small/2026-10-19_7d_at-20261020T0815Z.json")),
    ("america-new-york", "small", "2026-10-20_0d_at-20261020T0815Z.json", include_str!("../../../../fixtures/snapshots/america-new-york/small/2026-10-20_0d_at-20261020T0815Z.json")),
    ("america-new-york", "small", "2026-10-20_1d_at-20261020T0815Z.json", include_str!("../../../../fixtures/snapshots/america-new-york/small/2026-10-20_1d_at-20261020T0815Z.json")),
    ("america-new-york", "small", "2026-10-20_40d_at-20261020T0815Z.json", include_str!("../../../../fixtures/snapshots/america-new-york/small/2026-10-20_40d_at-20261020T0815Z.json")),
    ("america-new-york", "small", "2026-10-23_1d_at-20261020T0815Z.json", include_str!("../../../../fixtures/snapshots/america-new-york/small/2026-10-23_1d_at-20261020T0815Z.json")),
    ("america-new-york", "small", "2026-10-25_1d_at-20261020T0815Z.json", include_str!("../../../../fixtures/snapshots/america-new-york/small/2026-10-25_1d_at-20261020T0815Z.json")),
    ("america-new-york", "small", "2026-11-01_1d_at-20261020T0815Z.json", include_str!("../../../../fixtures/snapshots/america-new-york/small/2026-11-01_1d_at-20261020T0815Z.json")),
    ("europe-berlin", "archive-10k", "2026-10-05_31d_at-20261020T0815Z.json", include_str!("../../../../fixtures/snapshots/europe-berlin/archive-10k/2026-10-05_31d_at-20261020T0815Z.json")),
    ("europe-berlin", "archive-10k", "2026-10-19_7d_at-20261020T0815Z.json", include_str!("../../../../fixtures/snapshots/europe-berlin/archive-10k/2026-10-19_7d_at-20261020T0815Z.json")),
    ("europe-berlin", "archive-10k", "2026-10-20_1d_at-20261020T0815Z.json", include_str!("../../../../fixtures/snapshots/europe-berlin/archive-10k/2026-10-20_1d_at-20261020T0815Z.json")),
    ("europe-berlin", "dst-edges", "1970-01-01_1d_at-20261025T0130Z.json", include_str!("../../../../fixtures/snapshots/europe-berlin/dst-edges/1970-01-01_1d_at-20261025T0130Z.json")),
    ("europe-berlin", "dst-edges", "2026-03-01_31d_at-20260329T0130Z.json", include_str!("../../../../fixtures/snapshots/europe-berlin/dst-edges/2026-03-01_31d_at-20260329T0130Z.json")),
    ("europe-berlin", "dst-edges", "2026-03-08_1d_at-20261101T0530Z.json", include_str!("../../../../fixtures/snapshots/europe-berlin/dst-edges/2026-03-08_1d_at-20261101T0530Z.json")),
    ("europe-berlin", "dst-edges", "2026-03-29_1d_at-20260329T0130Z.json", include_str!("../../../../fixtures/snapshots/europe-berlin/dst-edges/2026-03-29_1d_at-20260329T0130Z.json")),
    ("europe-berlin", "dst-edges", "2026-10-20_2d_at-20261025T0130Z.json", include_str!("../../../../fixtures/snapshots/europe-berlin/dst-edges/2026-10-20_2d_at-20261025T0130Z.json")),
    ("europe-berlin", "dst-edges", "2026-10-22_7d_at-20261025T0130Z.json", include_str!("../../../../fixtures/snapshots/europe-berlin/dst-edges/2026-10-22_7d_at-20261025T0130Z.json")),
    ("europe-berlin", "dst-edges", "2026-10-24_1d_at-20261025T0130Z.json", include_str!("../../../../fixtures/snapshots/europe-berlin/dst-edges/2026-10-24_1d_at-20261025T0130Z.json")),
    ("europe-berlin", "dst-edges", "2026-10-25_1d_at-20261025T0130Z.json", include_str!("../../../../fixtures/snapshots/europe-berlin/dst-edges/2026-10-25_1d_at-20261025T0130Z.json")),
    ("europe-berlin", "dst-edges", "2026-11-01_1d_at-20261101T0530Z.json", include_str!("../../../../fixtures/snapshots/europe-berlin/dst-edges/2026-11-01_1d_at-20261101T0530Z.json")),
    ("europe-berlin", "dst-edges", "2026-11-01_1d_at-20261101T0630Z.json", include_str!("../../../../fixtures/snapshots/europe-berlin/dst-edges/2026-11-01_1d_at-20261101T0630Z.json")),
    ("europe-berlin", "dst-edges", "2026-12-31_2d_at-20261025T0130Z.json", include_str!("../../../../fixtures/snapshots/europe-berlin/dst-edges/2026-12-31_2d_at-20261025T0130Z.json")),
    ("europe-berlin", "dst-edges", "9999-12-31_1d_at-20261025T0130Z.json", include_str!("../../../../fixtures/snapshots/europe-berlin/dst-edges/9999-12-31_1d_at-20261025T0130Z.json")),
    ("europe-berlin", "empty", "2026-10-20_1d_at-20261020T0815Z.json", include_str!("../../../../fixtures/snapshots/europe-berlin/empty/2026-10-20_1d_at-20261020T0815Z.json")),
    ("europe-berlin", "id-limits", "2026-10-19_7d_at-20261020T0815Z.json", include_str!("../../../../fixtures/snapshots/europe-berlin/id-limits/2026-10-19_7d_at-20261020T0815Z.json")),
    ("europe-berlin", "id-limits", "2026-10-20_1d_at-20261020T0815Z.json", include_str!("../../../../fixtures/snapshots/europe-berlin/id-limits/2026-10-20_1d_at-20261020T0815Z.json")),
    ("europe-berlin", "small", "2026-10-05_31d_at-20261020T0815Z.json", include_str!("../../../../fixtures/snapshots/europe-berlin/small/2026-10-05_31d_at-20261020T0815Z.json")),
    ("europe-berlin", "small", "2026-10-19_7d_at-20261020T0815Z.json", include_str!("../../../../fixtures/snapshots/europe-berlin/small/2026-10-19_7d_at-20261020T0815Z.json")),
    ("europe-berlin", "small", "2026-10-20_0d_at-20261020T0815Z.json", include_str!("../../../../fixtures/snapshots/europe-berlin/small/2026-10-20_0d_at-20261020T0815Z.json")),
    ("europe-berlin", "small", "2026-10-20_1d_at-20261020T0815Z.json", include_str!("../../../../fixtures/snapshots/europe-berlin/small/2026-10-20_1d_at-20261020T0815Z.json")),
    ("europe-berlin", "small", "2026-10-20_40d_at-20261020T0815Z.json", include_str!("../../../../fixtures/snapshots/europe-berlin/small/2026-10-20_40d_at-20261020T0815Z.json")),
    ("europe-berlin", "small", "2026-10-23_1d_at-20261020T0815Z.json", include_str!("../../../../fixtures/snapshots/europe-berlin/small/2026-10-23_1d_at-20261020T0815Z.json")),
    ("europe-berlin", "small", "2026-10-25_1d_at-20261020T0815Z.json", include_str!("../../../../fixtures/snapshots/europe-berlin/small/2026-10-25_1d_at-20261020T0815Z.json")),
    ("europe-berlin", "small", "2026-11-01_1d_at-20261020T0815Z.json", include_str!("../../../../fixtures/snapshots/europe-berlin/small/2026-11-01_1d_at-20261020T0815Z.json")),
    ("europe-berlin", "unreadable", "2026-10-19_7d_at-20261020T0815Z.json", include_str!("../../../../fixtures/snapshots/europe-berlin/unreadable/2026-10-19_7d_at-20261020T0815Z.json")),
    ("europe-berlin", "unreadable", "2026-10-20_1d_at-20261020T0815Z.json", include_str!("../../../../fixtures/snapshots/europe-berlin/unreadable/2026-10-20_1d_at-20261020T0815Z.json")),
];

/// `from`, `days` and `now` from a file name `<from>_<days>d_at-<now>.json`.
pub fn request(name: &str) -> (chrono::NaiveDate, u32, Instant) {
    let stem = name.strip_suffix(".json").expect(".json");
    let (from, rest) = stem.split_once('_').expect("from");
    let (days, now) = rest.split_once("d_at-").expect("days and now");
    let from = chrono::NaiveDate::parse_from_str(from, "%Y-%m-%d").expect("from");
    let days: u32 = days.parse().expect("days");
    let now = chrono::NaiveDateTime::parse_from_str(now, "%Y%m%dT%H%MZ").expect("now").and_utc();
    (from, days, Instant::from_chrono(now).expect("now in range"))
}

/// What §18.7 removed from a folder before importing it.
#[derive(Default)]
pub struct Removed {
    pub items: BTreeSet<u64>,
    /// `(id, name)` of each removed archive row.
    pub rows: Vec<(u64, String)>,
}

/// The recorded JSON transformed as §18.5 and §18.7 state, or `None` when
/// §18.7 says the snapshot is not compared.
pub fn transform(mut recorded: Value, board: &BoardData, removed: &Removed) -> Option<Value> {
    let gone = |v: &Value| v["id"].as_u64().is_some_and(|id| removed.items.contains(&id));
    // §18.7: a removed record that places an entry or a ghost on a day.
    for day in recorded["days"].as_array().expect("days") {
        if day["entries"].as_array().expect("entries").iter().any(gone) {
            return None;
        }
        let ghost_of_removed = |g: &Value| removed.rows.iter().any(|(_, name)| g["name"].as_str() == Some(name));
        if day["ghosts"].as_array().expect("ghosts").iter().any(ghost_of_removed) {
            return None;
        }
    }
    // §18.7: the removed records deleted from the lists that carry them.
    for path in ["items", "ranked"] {
        let list = recorded[path].as_array_mut().expect("list");
        list.retain(|v| !gone(v) && !v.as_u64().is_some_and(|id| removed.items.contains(&id)));
    }
    for part in ["due", "later"] {
        recorded["tray"][part].as_array_mut().expect("tray").retain(|v| !gone(v));
    }
    let removed_row = |v: &Value| removed.rows.iter().any(|(id, _)| v["id"].as_u64() == Some(*id));
    recorded["done"].as_array_mut().expect("done").retain(|v| !removed_row(v));

    // §18.5: items sorted by id.
    recorded["items"].as_array_mut().expect("items").sort_by_key(|v| v["id"].as_u64().expect("id"));
    // §18.5: each session index replaced by the sid the import gave it: the
    // i-th block of the item in ascending sid.
    let sid = |id: u64, index: &Value| -> Value {
        let index = usize::try_from(index.as_u64().expect("index")).expect("usize");
        let item = board.item(id).unwrap_or_else(|| panic!("item {id} imported"));
        Value::from(item.blocks.get(index).unwrap_or_else(|| panic!("item {id} block {index}")).sid)
    };
    for day in recorded["days"].as_array_mut().expect("days") {
        for entry in day["entries"].as_array_mut().expect("entries") {
            if !entry["session"].is_null() {
                let id = entry["id"].as_u64().expect("id");
                entry["session"] = sid(id, &entry["session"]);
            }
        }
    }
    for item in recorded["items"].as_array_mut().expect("items") {
        let id = item["id"].as_u64().expect("id");
        for session in item["sessions"].as_array_mut().expect("sessions") {
            session["index"] = sid(id, &session["index"]);
        }
    }
    // §18.5: `archived_at` as the canonical text of the same instant.
    for row in recorded["done"].as_array_mut().expect("done") {
        let text = row["archived_at"].as_str().expect("archived_at");
        let at = chrono::DateTime::parse_from_rfc3339(text).expect("RFC 3339").with_timezone(&chrono::Utc);
        row["archived_at"] = Value::String(Instant::from_chrono(at).expect("in range").to_canonical());
    }
    // §18.5: `notes_base` 0, and the imported palette.
    recorded["notes_base"] = Value::from(0);
    recorded["palette"] = serde_json::to_value(board.palette).expect("JSON");
    Some(recorded)
}

/// The first JSON path where two values differ, with both values.
pub fn first_difference(path: &str, a: &Value, b: &Value) -> Option<String> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            let keys: BTreeSet<&String> = x.keys().chain(y.keys()).collect();
            keys.into_iter().find_map(|k| match (x.get(k), y.get(k)) {
                (Some(p), Some(q)) => first_difference(&format!("{path}.{k}"), p, q),
                (p, q) => Some(format!("{path}.{k}: built {p:?}, recorded {q:?}")),
            })
        }
        (Value::Array(x), Value::Array(y)) => {
            let found = x.iter().zip(y).enumerate().find_map(|(i, (p, q))| first_difference(&format!("{path}[{i}]"), p, q));
            found.or_else(|| (x.len() != y.len()).then(|| format!("{path}: built {} entries, recorded {}", x.len(), y.len())))
        }
        _ => (a != b).then(|| format!("{path}: built {a}, recorded {b}")),
    }
}

