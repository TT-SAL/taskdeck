//! The legacy import (core.md §15): every case of `vectors/import.json`, the
//! counts against the fixtures' `MIGRATION.json`, the report's freedom from
//! board text, the limits, and the property test of CORE-GEN-007.

mod common;

use common::boards;
use proptest::prelude::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use taskdeck_core::board::BoardData;
use taskdeck_core::legacy::{FailureKind, IdList, ImportFailure, ImportReport, ImportWarning, LegacyConfig, LegacyFolder, import};

const IMPORT: &str = include_str!("vectors/import.json");
const MIGRATION_MAIN: &str = include_str!("../../../fixtures/legacy-main/MIGRATION.json");
const MIGRATION_OTHER: &str = include_str!("../../../fixtures/legacy-other/MIGRATION.json");

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2));
    (0..s.len()).step_by(2).map(|k| u8::from_str_radix(&s[k..k + 2], 16).expect("hex")).collect()
}

fn kind_text(k: FailureKind) -> &'static str {
    match k {
        FailureKind::TooLarge => "too_large",
        FailureKind::ItemsUnreadable => "items_unreadable",
        FailureKind::NotesUnreadable => "notes_unreadable",
        FailureKind::SubscriptionsUnreadable => "subscriptions_unreadable",
        FailureKind::IdRange => "id_range",
        FailureKind::InstantRange => "instant_range",
        FailureKind::FieldRange => "field_range",
        FailureKind::Cap => "cap",
        FailureKind::Invariant => "invariant",
    }
}

fn failure_json(f: &ImportFailure) -> Value {
    json!({"kind": kind_text(f.kind), "ids": f.ids})
}

/// The report of an accepted import in the vector's form (§15.4).
fn report_json(r: &ImportReport) -> Value {
    let list = |l: IdList| match l {
        IdList::Items => "items",
        IdList::Archive => "archive",
        IdList::Subscriptions => "subscriptions",
    };
    let warning = |w: &ImportWarning| match w {
        ImportWarning::PaletteUnreadable => "palette_unreadable",
        ImportWarning::HomeOutOfRange => "home_out_of_range",
        ImportWarning::NotesOverCap => "notes_over_cap",
    };
    json!({
        "items_task": r.items_task, "items_event": r.items_event, "items_routine": r.items_routine,
        "blocks": r.blocks, "archive_rows": r.archive_rows, "unreadable_lines": r.unreadable_lines,
        "subscriptions": r.subscriptions, "notes_scalars": r.notes_scalars,
        "palette_set": r.palette_set, "home_set": r.home_set,
        "renumbered": r.renumbered.iter().map(|n| json!([list(n.list), n.old, n.new])).collect::<Vec<_>>(),
        "live_and_archived": r.live_and_archived,
        "warnings": r.warnings.iter().map(warning).collect::<Vec<_>>(),
    })
}

fn config_of(v: Option<&Value>) -> LegacyConfig {
    let coordinates = v.and_then(|c| c.get("coordinates")).map(|c| {
        let a = c.as_array().expect("[lat, lon]");
        [a[0].as_f64().expect("lat"), a[1].as_f64().expect("lon")]
    });
    let selected_colorscheme_id =
        v.and_then(|c| c.get("selected_colorscheme_id")).map(|n| u32::try_from(n.as_u64().expect("id")).expect("u32"));
    LegacyConfig { coordinates, selected_colorscheme_id }
}

/// The folder a case names: a fixture folder, or the files it gives.
fn folder_of(case: &Value) -> LegacyFolder {
    let config = config_of(case.get("config"));
    if let Some(path) = case["folder"].as_str() {
        let folder = boards::by_path(path);
        assert_eq!(folder.config, config, "{path}: the shared table holds the config the vector names");
        return folder;
    }
    let files = &case["files"];
    let file = |key: &str| -> Option<Vec<u8>> {
        files
            .get(key)
            .map(|t| t.as_str().expect("text").as_bytes().to_vec())
            .or_else(|| files.get(format!("{key}_hex")).map(|h| unhex(h.as_str().expect("hex"))))
    };
    LegacyFolder {
        read_at_startup: file("read_at_startup"),
        archived: file("archived"),
        notepad: file("notepad"),
        subscriptions: file("subscriptions"),
        colorschemes: file("colorschemes"),
        config,
    }
}

/// An accepted board passes `check` and round-trips through its canonical bytes.
fn assert_sound(board: &BoardData, name: &str) {
    assert_eq!(board.check(), Ok(()), "{name}: the genesis board passes check");
    let bytes = board.to_canonical_bytes();
    let back = BoardData::from_canonical_bytes(&bytes).unwrap_or_else(|e| panic!("{name}: {e:?}"));
    assert_eq!(&back, board, "{name}: round trip");
    assert_eq!(back.to_canonical_bytes(), bytes, "{name}: equal bytes");
}

// Every case of import.json: the genesis bytes (as text or as SHA-256) and
// the report, or the failure kind and its ids; every accepted board passes
// check and round-trips; the same bytes imported twice give equal results.
// spec: CORE-IMP-001
// spec: CORE-IMP-002
// spec: CORE-IMP-004
// spec: CORE-IMP-005
// spec: CORE-IMP-006
// spec: CORE-IMP-007
// spec: CORE-IMP-009
#[test]
fn every_import_vector() {
    let v: Value = serde_json::from_str(IMPORT).expect("vector file is JSON");
    assert_eq!(v["spec"].as_str(), Some("core.md §15"));
    let cases = v["cases"].as_array().expect("cases");
    assert!(cases.len() >= 30);
    for case in cases {
        let name = case["name"].as_str().expect("name");
        let folder = folder_of(case);
        let (board, report) = import(&folder);
        let (board2, report2) = import(&folder);
        assert_eq!(report, report2, "{name}: equal reports for equal inputs");
        assert_eq!(board, board2, "{name}: equal boards for equal inputs");
        if let Some(expected) = case.get("failure") {
            assert!(board.is_none(), "{name}: refused");
            let failure = report.failure.as_ref().unwrap_or_else(|| panic!("{name}: a failure"));
            assert_eq!(&failure_json(failure), expected, "{name}");
            assert_eq!(ImportReport { failure: None, ..report.clone() }, ImportReport::default(), "{name}: a failed report holds only the failure");
            continue;
        }
        let board = board.unwrap_or_else(|| panic!("{name}: accepted, not {:?}", report.failure));
        assert_eq!(report.failure, None, "{name}");
        assert_eq!(&report_json(&report), &case["report"], "{name}: the report");
        let bytes = board.to_canonical_bytes();
        if let Some(text) = case["board_text"].as_str() {
            assert_eq!(String::from_utf8(bytes.clone()).expect("UTF-8"), text, "{name}: the genesis board");
        }
        if let Some(sha) = case["board_sha256"].as_str() {
            assert_eq!(hex(&Sha256::digest(&bytes)), sha, "{name}: the genesis board's SHA-256");
        }
        assert!(case.get("board_text").is_some() || case.get("board_sha256").is_some(), "{name}");
        assert_sound(&board, name);
    }
}

// Every folder under fixtures/boards, legacy-main and legacy-other has a row
// in import.json.
// spec: CORE-IMP-007
#[test]
fn every_fixture_folder_has_a_vector() {
    let v: Value = serde_json::from_str(IMPORT).expect("vector file is JSON");
    let named: Vec<&str> = v["cases"].as_array().expect("cases").iter().filter_map(|c| c["folder"].as_str()).collect();
    for (group, names) in boards::GROUPS {
        for name in names {
            let path = format!("{group}/{name}");
            assert!(named.contains(&path.as_str()), "{path} has a case");
        }
    }
}

fn opened(entry: &Value, key: &str) -> u64 {
    entry["opened"][key].as_u64().unwrap_or_else(|| panic!("{key}"))
}

/// The counts both the import and today's `Board::open` (MIGRATION.json) give.
fn assert_counts(entry: &Value, board: &BoardData, report: &ImportReport, what: &str) {
    let items = report.items_task + report.items_event + report.items_routine;
    assert_eq!(u64::from(items), opened(entry, "items"), "{what}: items");
    assert_eq!(u64::from(report.archive_rows), opened(entry, "archive_rows"), "{what}: archive rows");
    let rows_0 = board.archive.iter().filter(|r| r.id == 0).count();
    assert_eq!(rows_0 as u64, opened(entry, "archive_rows_with_id_0"), "{what}: rows with id 0");
    assert_eq!(u64::from(report.unreadable_lines), opened(entry, "archive_unreadable_lines"), "{what}: unreadable lines");
    // The legacy folders carry no sessions of their own (main has no plans),
    // so every block is one made from `planned_start`.
    assert_eq!(u64::from(report.blocks), opened(entry, "items_given_sessions_from_planned_start"), "{what}: sessions from planned_start");
    assert_eq!(opened(entry, "items_with_planned_start_left"), 0);
    assert_eq!(u64::from(report.palette_set), opened(entry, "palette_found"), "{what}: palette found");
    assert_eq!(u64::from(report.home_set), opened(entry, "home_place_found"), "{what}: home place found");
}

/// The bytes of `text` without the lines for which `drop` holds.
fn without_lines(text: &[u8], drop: impl Fn(&[u8]) -> bool) -> Vec<u8> {
    let kept: Vec<&[u8]> = text.split(|&c| c == b'\n').filter(|l| !drop(l)).collect();
    kept.join(&b'\n')
}

fn contains(hay: &[u8], needle: &str) -> bool {
    hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}

/// `unreadable` with its two lines over the limits of §6 removed: the row
/// whose name is 50,071 bytes (step 12 `field_range`) and the 70,016-byte
/// line (a cap of §17).
fn repaired_unreadable(mut folder: LegacyFolder) -> LegacyFolder {
    let archived = folder.archived.take().expect("archive");
    folder.archived = Some(without_lines(&archived, |l| contains(l, "\"id\":907,") || contains(l, "\"over_long\"")));
    folder
}

// The counts against MIGRATION.json, where both count the same thing. Of the
// folders the import refuses, `unreadable` is compared once its two lines
// over the limits of §6 are removed (each removal is one line fewer).
// spec: CORE-IMP-004
// spec: CORE-IMP-007
// spec: CORE-IMP-009
#[test]
fn counts_match_today_s_open_of_the_legacy_folders() {
    for (group, text) in [("legacy-main", MIGRATION_MAIN), ("legacy-other", MIGRATION_OTHER)] {
        let entries: Value = serde_json::from_str(text).expect("MIGRATION.json");
        for entry in entries.as_array().expect("entries") {
            let name = entry["board"].as_str().expect("board");
            let what = format!("{group}/{name}");
            let folder = boards::by_path(&what);
            assert_eq!(u64::from(folder.config.selected_colorscheme_id.expect("set")), opened(entry, "selected_colorscheme_id"));
            let (board, report) = import(&folder);
            match (what.as_str(), board) {
                (_, Some(board)) => assert_counts(entry, &board, &report, &what),
                ("legacy-main/unreadable", None) => {
                    assert_eq!(report.failure.as_ref().map(|f| (f.kind, f.ids.clone())), Some((FailureKind::FieldRange, vec![907])));
                    let (board, report) = import(&repaired_unreadable(folder));
                    let board = board.expect("the repaired folder imports");
                    assert_eq!(report.archive_rows + 1, u32::try_from(opened(entry, "archive_rows")).expect("u32"));
                    assert_eq!(report.unreadable_lines + 1, u32::try_from(opened(entry, "archive_unreadable_lines")).expect("u32"));
                    assert_eq!(board.archive.iter().filter(|r| r.id == 0).count(), 0);
                    assert_eq!(u64::from(report.items_task + report.items_event + report.items_routine), opened(entry, "items"));
                }
                // §15.3 steps 4, 9.4 and 11 refuse these (§18.7); MIGRATION.json
                // records today's opening, which repairs or ignores them.
                ("legacy-main/dst-edges", None) => assert_eq!(report.failure.map(|f| f.kind), Some(FailureKind::InstantRange)),
                ("legacy-main/id-limits", None) => assert_eq!(report.failure.map(|f| f.kind), Some(FailureKind::IdRange)),
                ("legacy-other/empty", None) => {
                    assert_eq!(opened(entry, "notes_placeholder"), 1, "today shows the placeholder");
                    assert_eq!(report.failure.map(|f| f.kind), Some(FailureKind::NotesUnreadable));
                }
                (_, None) => panic!("{what} refused: {:?}", report.failure),
            }
        }
    }
}

// The `unreadable` fixture, repaired: every unparsed line kept byte for byte,
// only the empty and blank lines dropped, the live-and-archived id reported.
// spec: CORE-IMP-004
#[test]
fn unreadable_lines_are_kept_byte_for_byte() {
    let folder = repaired_unreadable(boards::board("unreadable"));
    let source = folder.archived.clone().expect("archive");
    let (board, report) = import(&folder);
    let board = board.expect("the repaired folder imports");
    assert_eq!(report.unreadable_lines, 7);
    assert_eq!(report.archive_rows, 5);
    assert_eq!(report.live_and_archived, vec![1]);
    let lines: Vec<&[u8]> = source.split(|&c| c == b'\n').collect();
    for kept in &board.unreadable {
        assert!(lines.contains(&kept.as_slice()), "an unreadable line is one line of the file");
    }
    // Every line is a row, an unreadable line, or empty after trim.
    let blank = lines.iter().filter(|l| std::str::from_utf8(l).is_ok_and(|t| t.trim().is_empty())).count();
    assert_eq!(lines.len(), blank + 7 + 5);
    assert_sound(&board, "unreadable, repaired");
}

// dst-edges as §18.7 repairs it: without the records holding an instant
// before 1970, whose ids are exactly the ones the failure lists, it imports.
// spec: CORE-IMP-001
// spec: CORE-IMP-007
#[test]
fn dst_edges_imports_without_the_records_instant_range_names() {
    for mut folder in [boards::board("dst-edges"), boards::legacy_main("dst-edges")] {
        let (_, report) = import(&folder);
        let failure = report.failure.expect("refused");
        assert_eq!((failure.kind, failure.ids.clone()), (FailureKind::InstantRange, vec![21, 22, 31]));
        let items: Vec<Value> = serde_json::from_slice(folder.read_at_startup.as_deref().expect("items")).expect("JSON");
        let items: Vec<Value> = items.into_iter().filter(|i| !failure.ids.contains(&i["id"].as_u64().expect("id"))).collect();
        folder.read_at_startup = Some(serde_json::to_vec(&items).expect("JSON"));
        let archived = folder.archived.take().expect("archive");
        folder.archived = Some(without_lines(&archived, |l| contains(l, "\"id\":31,")));
        let (board, report) = import(&folder);
        let board = board.unwrap_or_else(|| panic!("repaired: {:?}", report.failure));
        assert_eq!(board.archive.iter().filter(|r| r.id == 31).count(), 0);
        assert_sound(&board, "dst-edges, repaired");
    }
}

/// Every name, the notes and every URL of a folder.
fn texts_of(folder: &LegacyFolder) -> Vec<String> {
    let mut out = Vec::new();
    let names = |v: &Value, out: &mut Vec<String>| {
        for k in ["name", "url"] {
            if let Some(s) = v.get(k).and_then(Value::as_str) {
                out.push(s.to_owned());
            }
        }
    };
    let items: Value = serde_json::from_slice(folder.read_at_startup.as_deref().expect("items")).expect("JSON");
    items.as_array().expect("array").iter().for_each(|i| names(i, &mut out));
    for line in folder.archived.as_deref().unwrap_or_default().split(|&c| c == b'\n') {
        if let Ok(v) = serde_json::from_slice::<Value>(line) {
            names(&v, &mut out);
        }
    }
    if let Some(s) = folder.subscriptions.as_deref() {
        let subs: Value = serde_json::from_slice(s).expect("JSON");
        subs.as_array().expect("array").iter().for_each(|s| names(s, &mut out));
    }
    let notes: String = serde_json::from_slice(folder.notepad.as_deref().expect("notes")).expect("a string");
    out.push(notes.replace('\t', "    "));
    out.extend(notes.lines().filter(|l| l.trim().len() >= 4).map(str::to_owned));
    out.retain(|t| !t.is_empty());
    out
}

// The report holds counts, ids and kinds only: no name, notes text or URL of
// the folder appears in its Debug text, accepted or refused.
// spec: CORE-IMP-003
// spec: CORE-GEN-010
#[test]
fn the_report_holds_no_text_from_the_board() {
    for folder in [boards::board("small"), boards::board("unreadable"), boards::legacy_other("small"), boards::board("id-limits")] {
        let texts = texts_of(&folder);
        assert!(texts.len() > 5);
        let (_, report) = import(&folder);
        let debug = format!("{report:?}");
        for t in &texts {
            assert!(!debug.contains(t.as_str()), "the report quotes board text");
        }
        assert!(!debug.contains("https"), "no URL");
        assert!(!debug.contains("Fixture"), "no name");
    }
}

fn items_folder(items: &Value) -> LegacyFolder {
    LegacyFolder {
        read_at_startup: Some(serde_json::to_vec(items).expect("JSON")),
        archived: None,
        notepad: None,
        subscriptions: None,
        colorschemes: None,
        config: LegacyConfig { coordinates: None, selected_colorscheme_id: None },
    }
}

fn failure_of(folder: &LegacyFolder) -> Option<(FailureKind, Vec<u64>)> {
    import(folder).1.failure.map(|f| (f.kind, f.ids))
}

// Step 1: an input over 256 MiB is refused before anything is parsed.
// spec: CORE-IMP-001
#[test]
fn an_input_over_256_mib_is_too_large_before_any_parsing() {
    const LIMIT: usize = 268_435_456;
    let mut folder = items_folder(&json!([]));
    folder.read_at_startup = Some(vec![0; LIMIT]);
    assert_eq!(failure_of(&folder), Some((FailureKind::ItemsUnreadable, vec![])), "at the limit: parsed, and not JSON");
    for slot in 0..5 {
        let mut folder = items_folder(&json!([]));
        folder.notepad = Some(b"not a string".to_vec());
        let big = Some(vec![0; LIMIT + 1]);
        match slot {
            0 => folder.read_at_startup = big,
            1 => folder.archived = big,
            2 => folder.notepad = big,
            3 => folder.subscriptions = big,
            _ => folder.colorschemes = big,
        }
        assert_eq!(failure_of(&folder), Some((FailureKind::TooLarge, vec![])), "slot {slot}");
    }
}

fn item(id: u64) -> Value {
    json!({"id": id, "name": "n", "created": "2026-01-01T00:00:00Z", "is_event": false})
}

// Step 12: names, notes and subscriptions over their limits; the caps of §17.
// spec: CORE-IMP-001
#[test]
fn fields_and_caps_over_their_limits_are_refused() {
    let mut long = item(3);
    long["name"] = json!("é".repeat(2048));
    assert!(import(&items_folder(&json!([long]))).0.is_some(), "4,096 bytes");
    long["name"] = json!(format!("{}x", "é".repeat(2048)));
    assert_eq!(failure_of(&items_folder(&json!([item(1), long]))), Some((FailureKind::FieldRange, vec![3])));

    let mut notes = items_folder(&json!([]));
    notes.notepad = Some(serde_json::to_vec(&"n".repeat(1_048_576)).expect("JSON"));
    assert!(import(&notes).0.is_some(), "1,048,576 bytes of notes");
    notes.notepad = Some(serde_json::to_vec(&"\t".repeat(262_145)).expect("JSON"));
    assert_eq!(failure_of(&notes), Some((FailureKind::FieldRange, vec![])), "over 1,048,576 bytes after detab");

    let subs: Vec<Value> = (1..=21)
        .map(|k| json!({"id": k, "name": "s", "url": format!("https://example.invalid/{k}"), "color": [0, 0, 0, 0], "enabled": true}))
        .collect();
    let mut folder = items_folder(&json!([]));
    folder.subscriptions = Some(serde_json::to_vec(&subs[..20]).expect("JSON"));
    assert!(import(&folder).0.is_some(), "20 subscriptions");
    folder.subscriptions = Some(serde_json::to_vec(&subs).expect("JSON"));
    assert_eq!(failure_of(&folder), Some((FailureKind::FieldRange, vec![21])), "21 subscriptions");

    let mut crowded = item(2);
    let session = json!({"start": "2026-01-01T00:00:00Z", "minutes": 15});
    crowded["sessions"] = json!(vec![session.clone(); 1000]);
    assert!(import(&items_folder(&json!([crowded.clone()]))).0.is_some(), "1,000 blocks");
    crowded["sessions"] = json!(vec![session; 1001]);
    assert_eq!(failure_of(&items_folder(&json!([item(1), crowded]))), Some((FailureKind::Cap, vec![2])));

    let row = br#"{"id":1,"name":"r","created":"2026-01-01T00:00:00Z","is_event":false,"inactivated":"2026-01-01T00:00:00Z"}"#;
    let mut folder = items_folder(&json!([]));
    let mut archive = row.to_vec();
    archive.push(b'\n');
    archive.extend(vec![b'z'; 65_536]);
    folder.archived = Some(archive.clone());
    let (board, _) = import(&folder);
    assert_eq!(board.expect("a 65,536-byte line").unreadable[0].len(), 65_536);
    archive.push(b'z');
    folder.archived = Some(archive);
    assert_eq!(failure_of(&folder), Some((FailureKind::Cap, vec![])), "a 65,537-byte line");

    let many: Vec<Value> = (1..=10_001).map(item).collect();
    assert_eq!(failure_of(&items_folder(&json!(many[..10_001]))), Some((FailureKind::Cap, vec![])), "10,001 items");
    assert!(import(&items_folder(&json!(many[..10_000]))).0.is_some(), "10,000 items");
}

// Step 13: notes over 20,000 scalars after detab are kept, with a warning.
// spec: CORE-IMP-001
#[test]
fn notes_over_the_cap_are_kept_with_a_warning() {
    let mut folder = items_folder(&json!([]));
    folder.notepad = Some(serde_json::to_vec(&"\t".repeat(5000)).expect("JSON"));
    let (board, report) = import(&folder);
    assert_eq!(board.expect("accepted").notes, " ".repeat(20_000));
    assert_eq!((report.notes_scalars, report.warnings), (20_000, vec![]));
    folder.notepad = Some(serde_json::to_vec(&format!("{}é", "\t".repeat(5000))).expect("JSON"));
    let (board, report) = import(&folder);
    assert_eq!(board.expect("accepted").notes.chars().count(), 20_001);
    assert_eq!((report.notes_scalars, report.warnings), (20_001, vec![ImportWarning::NotesOverCap]));
}

// Only the import normalises: canonical board bytes that would need it are
// refused, never repaired (a tab in the notes, an id of 0, missing stamps).
// spec: CORE-IMP-008
#[test]
fn no_other_function_normalises() {
    let stamps = r#""stamps":{"estimate":[1,0],"levels":[1,0],"name":[1,0],"repeat":[1,0],"time":[1,0]}"#;
    let tab = br#"{"notes":"a\tb","rules":1}"#;
    assert_eq!(BoardData::from_canonical_bytes(tab).map_err(|e| (e.code, e.detail)), Err(("core/invariant", Some(7))));
    let id_0 = format!(r#"{{"items":[{{"created":"2026-01-01T00:00:00.000000000Z","id":0,"kind":"task","name":"a",{stamps}}}],"rules":1}}"#);
    assert_eq!(BoardData::from_canonical_bytes(id_0.as_bytes()).map_err(|e| (e.code, e.detail)), Err(("core/invariant", Some(3))));
    let no_stamps = br#"{"items":[{"created":"2026-01-01T00:00:00.000000000Z","id":1,"kind":"task","name":"a"}],"marks":[{"block":0,"device_no":0,"item":1}],"rules":1}"#;
    assert_eq!(BoardData::from_canonical_bytes(no_stamps).map(|_| ()).map_err(|e| e.code), Err("core/schema"));
    let no_mark = format!(r#"{{"items":[{{"created":"2026-01-01T00:00:00.000000000Z","id":1,"kind":"task","name":"a",{stamps}}}],"rules":1}}"#);
    assert_eq!(BoardData::from_canonical_bytes(no_mark.as_bytes()).map_err(|e| (e.code, e.detail)), Err(("core/invariant", Some(4))));
    // The same notes through the import are detabbed.
    let mut folder = items_folder(&json!([]));
    folder.notepad = Some(br#""a\tb""#.to_vec());
    assert_eq!(import(&folder).0.expect("accepted").notes, "a    b");
}

fn mutate(mut bytes: Vec<u8>, edits: &[(usize, u8, u8)]) -> Vec<u8> {
    for &(at, op, byte) in edits {
        let at = if bytes.is_empty() { 0 } else { at % (bytes.len() + 1) };
        match op % 4 {
            0 if at < bytes.len() => bytes[at] = byte,
            1 => bytes.insert(at, byte),
            2 if at < bytes.len() => {
                bytes.remove(at);
            }
            _ => bytes.truncate(at),
        }
    }
    bytes
}

fn mutated(mut folder: LegacyFolder, slot: usize, edits: &[(usize, u8, u8)]) -> LegacyFolder {
    let file = match slot % 5 {
        0 => &mut folder.read_at_startup,
        1 => &mut folder.archived,
        2 => &mut folder.notepad,
        3 => &mut folder.subscriptions,
        _ => &mut folder.colorschemes,
    };
    *file = Some(mutate(file.take().unwrap_or_default(), edits));
    folder
}

fn imports_soundly(folder: &LegacyFolder) {
    let (board, report) = import(folder);
    match board {
        Some(board) => {
            assert_eq!(report.failure, None);
            assert_eq!(board.check(), Ok(()));
            assert_eq!(BoardData::from_canonical_bytes(&board.to_canonical_bytes()).as_ref(), Ok(&board));
        }
        None => assert!(report.failure.is_some()),
    }
}

proptest! {
    // `import` returns, never panics, on mutated copies of `small` and
    // `unreadable`; what it accepts passes check and round-trips.
    // spec: CORE-GEN-007
    #[test]
    fn import_never_panics_on_mutated_folders(
        which in 0usize..2,
        slot in 0usize..5,
        edits in proptest::collection::vec((any::<usize>(), any::<u8>(), any::<u8>()), 1..8),
    ) {
        let base = if which == 0 { boards::board("small") } else { boards::board("unreadable") };
        imports_soundly(&mutated(base, slot, &edits));
    }

    // Random bytes in every file.
    // spec: CORE-GEN-007
    #[test]
    fn import_never_panics_on_random_files(
        files in proptest::collection::vec(proptest::option::of(proptest::collection::vec(any::<u8>(), 0..64)), 5),
        lat in any::<f64>(),
        lon in any::<f64>(),
        selected in proptest::option::of(any::<u32>()),
    ) {
        let folder = LegacyFolder {
            read_at_startup: files[0].clone(),
            archived: files[1].clone(),
            notepad: files[2].clone(),
            subscriptions: files[3].clone(),
            colorschemes: files[4].clone(),
            config: LegacyConfig { coordinates: Some([lat, lon]), selected_colorscheme_id: selected },
        };
        imports_soundly(&folder);
    }
}
