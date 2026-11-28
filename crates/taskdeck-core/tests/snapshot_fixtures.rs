//! The snapshot fixtures (core.md §18.5, §18.7): every recorded snapshot of
//! today's `phone::snapshot` against `snapshot` of the imported board.
//!
//! Each `SnapshotInput` is built from `import` of the fixture folder, the
//! folder's `subscribed_cache.json` as the overlay where it has one (else an
//! empty overlay), no look, dials 11 and 39, version 0, `ranked` none, and
//! the zone of the snapshot's directory. The recorded JSON is transformed as
//! §18.5 states and then compared as a JSON value (member order ignored).
//! `dst-edges` and `id-limits` are treated as §18.7 states.

mod common;

use std::collections::BTreeSet;

use common::snapshots::{self, Removed, first_difference, request, transform};
use common::{boards, zones};
use serde_json::Value;
use taskdeck_core::board::BoardData;
use taskdeck_core::legacy::{FailureKind, LegacyFolder, import};
use taskdeck_core::overlay::Overlay;
use taskdeck_core::snapshot::{Dials, SnapshotInput, answer_snapshot};
use taskdeck_core::time::Instant;

const SMALL_OVERLAY: &str = include_str!("../../../fixtures/boards/small/subscribed_cache.json");

/// 2^40: the first id with a high part (core.md §7.1).
const HIGH_ONE: u64 = 1 << 40;
/// Today's temporary ids start here (core.md §15.3 step 9.1).
const TEMPORARY_FLOOR: u64 = 1 << 62;

/// The lines of a JSON Lines file, without those whose `id` is in `ids`.
fn without_rows(archived: &[u8], ids: &BTreeSet<u64>) -> Vec<u8> {
    archived
        .split(|&c| c == b'\n')
        .filter(|line| {
            let id = serde_json::from_slice::<Value>(line).ok().and_then(|v| v["id"].as_u64());
            !id.is_some_and(|id| ids.contains(&id))
        })
        .collect::<Vec<_>>()
        .join(&b'\n')
}

/// The rows of a JSON Lines file whose `id` is in `ids`.
fn rows_with(archived: &[u8], ids: &BTreeSet<u64>) -> Vec<Value> {
    archived
        .split(|&c| c == b'\n')
        .filter_map(|line| serde_json::from_slice::<Value>(line).ok())
        .filter(|row| row["id"].as_u64().is_some_and(|id| ids.contains(&id)))
        .collect()
}

/// How a fixture folder is compared (core.md §18.7).
enum Imported {
    Board(Box<BoardData>, Removed),
    /// Refused for a reason other than `instant_range`: not compared.
    Refused,
}

/// Every id of a list that §15.3 step 9.4 names: from 2^40 to 2^62 − 1 with
/// a high part of 8192 or more or a low part of 0.
fn out_of_range_ids(list: &[Value]) -> Vec<u64> {
    list.iter()
        .filter_map(|v| v["id"].as_u64())
        .filter(|&id| (HIGH_ONE..TEMPORARY_FLOOR).contains(&id) && (id / HIGH_ONE > 8191 || id % HIGH_ONE == 0))
        .collect()
}

fn import_board(name: &str) -> Imported {
    let mut folder: LegacyFolder = boards::board(name);
    let (board, report) = import(&folder);
    if let Some(board) = board {
        return Imported::Board(Box::new(board), Removed::default());
    }
    let failure = report.failure.expect("a refusal names its failure");
    match failure.kind {
        FailureKind::InstantRange => {
            // Remove exactly the items and rows holding an instant outside
            // §5.1's range, check that their ids are the failure's, import.
            let items: Vec<Value> =
                serde_json::from_slice(folder.read_at_startup.as_deref().expect("items")).expect("JSON");
            let archived = folder.archived.clone().unwrap_or_default();
            let rows: Vec<Value> =
                archived.split(|&c| c == b'\n').filter_map(|l| serde_json::from_slice::<Value>(l).ok()).collect();
            let out_of_range = |v: &Value| {
                ["created", "deadline", "planned_start", "inactivated"].iter().any(|k| holds_early_instant(&v[*k]))
                    || v["sessions"].as_array().is_some_and(|s| s.iter().any(|s| holds_early_instant(&s["start"])))
            };
            let removed_items: BTreeSet<u64> =
                items.iter().filter(|v| out_of_range(v)).filter_map(|v| v["id"].as_u64()).collect();
            let removed_rows: BTreeSet<u64> =
                rows.iter().filter(|v| out_of_range(v)).filter_map(|v| v["id"].as_u64()).collect();
            let listed: BTreeSet<u64> = failure.ids.iter().copied().collect();
            assert_eq!(removed_items.union(&removed_rows).copied().collect::<BTreeSet<u64>>(), listed, "{name}");
            let row_names = rows_with(&archived, &removed_rows)
                .iter()
                .map(|r| (r["id"].as_u64().expect("id"), r["name"].as_str().expect("name").to_owned()))
                .collect();
            let kept: Vec<Value> = items
                .into_iter()
                .filter(|v| !v["id"].as_u64().is_some_and(|id| removed_items.contains(&id)))
                .collect();
            folder.read_at_startup = Some(serde_json::to_vec(&kept).expect("JSON"));
            folder.archived = Some(without_rows(&archived, &removed_rows));
            let (board, report) = import(&folder);
            let board = board.unwrap_or_else(|| panic!("{name}, repaired: {:?}", report.failure));
            Imported::Board(Box::new(board), Removed { items: removed_items, rows: row_names })
        }
        FailureKind::IdRange if name == "id-limits" => {
            // Not replayed; the failure lists exactly the ids of step 9.4.
            let mut expected = Vec::new();
            let items: Vec<Value> =
                serde_json::from_slice(folder.read_at_startup.as_deref().expect("items")).expect("JSON");
            expected.extend(out_of_range_ids(&items));
            let rows: Vec<Value> = folder
                .archived
                .as_deref()
                .unwrap_or_default()
                .split(|&c| c == b'\n')
                .filter_map(|l| serde_json::from_slice::<Value>(l).ok())
                .collect();
            expected.extend(out_of_range_ids(&rows));
            if let Some(subs) = folder.subscriptions.as_deref() {
                let subs: Vec<Value> = serde_json::from_slice(subs).expect("JSON");
                expected.extend(out_of_range_ids(&subs));
            }
            // An id the item list and the archive share is listed once.
            let mut listed = failure.ids.clone();
            listed.sort_unstable();
            expected.sort_unstable();
            expected.dedup();
            assert!(!expected.is_empty());
            assert_eq!(listed, expected, "{name}: the ids of step 9.4");
            Imported::Refused
        }
        FailureKind::FieldRange if name == "unreadable" => {
            // Not replayed (§18.7, "any other failure"): the archive row 907
            // has a name of 50,071 bytes, over the 4,096 of §6.6.
            assert_eq!(failure.ids, vec![907], "{name}");
            Imported::Refused
        }
        other => panic!("{name} refused with {other:?}"),
    }
}

/// Whether a today's `DateTime<Local>` text denotes an instant before
/// 1970-01-01T00:00:00Z.
fn holds_early_instant(v: &Value) -> bool {
    v.as_str()
        .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
        .is_some_and(|t| t.timestamp() < 0)
}

// Every recorded snapshot, built again from the imported board and compared
// field for field after §18.5's transformation; the files §18.7 leaves out
// are counted, so that none can drop out silently.
// spec: CORE-SNAP-001
// spec: CORE-SNAP-002
// spec: CORE-READ-001
// spec: CORE-READ-002
#[test]
fn every_recorded_snapshot_equals_the_built_one() {
    let small_overlay: Overlay = serde_json::from_str(SMALL_OVERLAY).expect("the recorded overlay");
    let names: BTreeSet<&str> = snapshots::FILES.iter().map(|f| f.1).collect();
    let imported: Vec<(&str, Imported)> = names.into_iter().map(|n| (n, import_board(n))).collect();
    let empty = Overlay::default();
    let mut compared = 0;
    let mut not_compared: Vec<String> = Vec::new();
    let mut failures = Vec::new();
    for (zone_dir, board_name, file, text) in snapshots::FILES {
        let what = format!("{zone_dir}/{board_name}/{file}");
        let Some((_, Imported::Board(board, removed))) = imported.iter().find(|(n, _)| *n == board_name) else {
            not_compared.push(what);
            continue;
        };
        let recorded: Value = serde_json::from_str(text).expect("recorded JSON");
        let Some(expected) = transform(recorded, board, removed) else {
            not_compared.push(what);
            continue;
        };
        let zone = zones::of_directory(zone_dir);
        let (from, days, now) = request(file);
        let overlay = if board_name == "small" { &small_overlay } else { &empty };
        let before = board.to_canonical_bytes();
        let input = SnapshotInput {
            board,
            overlay,
            zone: &zone,
            now,
            from,
            days,
            version: 0,
            ranked: None,
            look: None,
            dials: Dials { blur: 11, light: 39 },
        };
        let built = answer_snapshot(&input);
        // CORE-READ-002: the query leaves the board as it was.
        assert_eq!(board.to_canonical_bytes(), before, "{what}");
        compared += 1;
        if let Some(difference) = first_difference("", &built, &expected) {
            failures.push(format!("{what}: {difference}"));
        }
    }
    assert!(failures.is_empty(), "{} of {compared} differ:\n{}", failures.len(), failures.join("\n"));
    assert_eq!(compared + not_compared.len(), 48);
    assert_eq!((compared, not_compared.len()), (43, 5), "compared and not compared: {not_compared:#?}");
    // Not compared, by §18.7: the day on which a removed record of
    // `dst-edges` places an entry (Berlin's 1970-01-01: the event 21 and the
    // due marker of 22), and the folders refused for another reason
    // (`id-limits`: `id_range`; `unreadable`: `field_range`).
    assert_eq!(
        not_compared,
        [
            "europe-berlin/dst-edges/1970-01-01_1d_at-20261025T0130Z.json",
            "europe-berlin/id-limits/2026-10-19_7d_at-20261020T0815Z.json",
            "europe-berlin/id-limits/2026-10-20_1d_at-20261020T0815Z.json",
            "europe-berlin/unreadable/2026-10-19_7d_at-20261020T0815Z.json",
            "europe-berlin/unreadable/2026-10-20_1d_at-20261020T0815Z.json",
        ]
    );
}

// The generated zones have the transitions the fixtures were recorded with
// (fixtures/README.md: Berlin's gap 2026-03-29 02:00→03:00 and fold
// 2026-10-25 03:00→02:00; New York's gap 2026-03-08 07:00Z and fold
// 2026-11-01 06:00Z), and standard time on the winter days of 1970 and 9999.
#[test]
fn the_zones_have_the_fixtures_transitions() {
    use taskdeck_core::time::Zone;
    let t = |s: &str| Instant::parse_canonical(s).expect("instant");
    let berlin = zones::berlin();
    assert_eq!(berlin.offset_at(t("2026-03-29T00:59:59.000000000Z")), 3600);
    assert_eq!(berlin.offset_at(t("2026-03-29T01:00:00.000000000Z")), 7200);
    assert_eq!(berlin.offset_at(t("2026-10-25T00:59:59.000000000Z")), 7200);
    assert_eq!(berlin.offset_at(t("2026-10-25T01:00:00.000000000Z")), 3600);
    let new_york = zones::new_york();
    assert_eq!(new_york.offset_at(t("2026-03-08T06:59:59.000000000Z")), -18_000);
    assert_eq!(new_york.offset_at(t("2026-03-08T07:00:00.000000000Z")), -14_400);
    assert_eq!(new_york.offset_at(t("2026-11-01T05:59:59.000000000Z")), -14_400);
    assert_eq!(new_york.offset_at(t("2026-11-01T06:00:00.000000000Z")), -18_000);
    for zone in [&berlin, &new_york] {
        assert_eq!(zone.offset_at(t("1970-01-01T00:00:00.000000000Z")), zone.initial);
        assert_eq!(zone.offset_at(t("9999-12-31T12:00:00.000000000Z")), zone.initial);
    }
}
