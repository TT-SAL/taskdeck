//! The golden logs of rules version 1 (core.md §18.3, CORE-RULES-003): every
//! case of every file under `vectors/golden/rules-1/` is replayed from its
//! start board, and every entry's result, code, notices, counts, restored id
//! and board hash, and the end board, are compared with the recorded ones.
//!
//! A missing `board_sha256`, `end_text` or `end_sha256` fails the test and
//! prints the computed value on a `FILL` line, so a new case's hashes can be
//! read off and checked before they are stored (brief b2-apply point 9).

use serde_json::Value;
use sha2::{Digest, Sha256};
use taskdeck_core::apply::{Ctx, apply};
use taskdeck_core::board::{ArchiveRow, Block, BoardData, Item, ItemStamps, Kind, Mark, Outcome, Stamp};
use taskdeck_core::command::parse_command;
use taskdeck_core::time::Instant;

const FILES: [(&str, &str); 9] = [
    ("create.json", include_str!("vectors/golden/rules-1/create.json")),
    ("ids.json", include_str!("vectors/golden/rules-1/ids.json")),
    ("blocks.json", include_str!("vectors/golden/rules-1/blocks.json")),
    ("archive.json", include_str!("vectors/golden/rules-1/archive.json")),
    ("notes.json", include_str!("vectors/golden/rules-1/notes.json")),
    ("subscriptions.json", include_str!("vectors/golden/rules-1/subscriptions.json")),
    ("settings.json", include_str!("vectors/golden/rules-1/settings.json")),
    ("conflicts.json", include_str!("vectors/golden/rules-1/conflicts.json")),
    ("caps.json", include_str!("vectors/golden/rules-1/caps.json")),
];

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn sha(b: &[u8]) -> String {
    hex(&Sha256::digest(b))
}

fn uint(v: &Value, key: &str) -> u64 {
    v.get(key).map_or(0, |x| x.as_u64().unwrap_or_else(|| panic!("{key} is an integer")))
}

/// The generated start board of core.md §18.3: `I` items, `B` blocks, `R`
/// rows and the one legacy mark.
fn generated(g: &Value) -> BoardData {
    let (i, blocks, r) = (uint(g, "items"), uint(g, "blocks"), uint(g, "rows"));
    assert!(blocks <= 1000 * i, "B ≤ 1000 × I");
    let epoch = Instant::MIN;
    let st = Stamp { seq: 1, device_no: 0 };
    let mut b = BoardData::empty();
    b.items = (1..=i)
        .map(|id| Item {
            id,
            kind: Kind::Task,
            name: "t".to_owned(),
            created: epoch,
            deadline: None,
            importance: None,
            time_importance: None,
            duration_minutes: None,
            recurrence: None,
            blocks: Vec::new(),
            stamps: ItemStamps { estimate: st, levels: st, name: st, repeat: st, time: st },
        })
        .collect();
    for s in 1..=blocks {
        let owner = usize::try_from((s - 1) / 1000).expect("index");
        b.items[owner].blocks.push(Block { sid: s, start: epoch, minutes: 15, stamp: st });
    }
    b.archive = (1..=r)
        .map(|j| ArchiveRow {
            id: i + j,
            kind: Kind::Task,
            name: "t".to_owned(),
            created: epoch,
            deadline: None,
            importance: None,
            time_importance: None,
            duration_minutes: None,
            recurrence: None,
            blocks: Vec::new(),
            archived_at: Instant::from_parts(i64::try_from(j - 1).expect("secs"), 0).expect("in range"),
            outcome: Outcome::Finished,
        })
        .collect();
    if i + r > 0 || blocks > 0 {
        b.marks = vec![Mark { device_no: 0, item: i + r, block: blocks }];
    }
    b
}

fn start(case: &Value) -> BoardData {
    match (case.get("start_text"), case.get("start_gen")) {
        (Some(t), None) => {
            let bytes = t.as_str().expect("start_text").as_bytes();
            let b = BoardData::from_canonical_bytes(bytes).expect("the start board loads");
            assert_eq!(b.to_canonical_bytes(), bytes);
            b
        }
        (None, Some(g)) => generated(g),
        _ => panic!("exactly one of start_text and start_gen"),
    }
}

fn ctx(v: &Value) -> Ctx {
    Ctx {
        seq: uint(v, "seq"),
        device_no: u16::try_from(uint(v, "device_no")).expect("u16"),
        at: Instant::from_at_ms(i64::try_from(uint(v, "at_ms")).expect("i64")).expect("at_ms converts"),
        based_on: uint(v, "based_on"),
    }
}

/// Replays one case; returns the mismatches found.
fn replay(file: &str, k: usize, case: &Value, out: &mut Vec<String>) {
    let name = case["name"].as_str().expect("name");
    let at = |e: &str| format!("{file} case {k} ({name}) {e}");
    let mut board = start(case);
    board.check().expect("the start board passes check");
    for (n, entry) in case["entries"].as_array().expect("entries").iter().enumerate() {
        let here = at(&format!("entry {n}"));
        let ex = &entry["expect"];
        let before = board.to_canonical_bytes();
        let text = entry["command_text"].as_str().expect("command_text");
        let (result, code, notices, moved, skipped, restored) = match parse_command(text.as_bytes()) {
            Err(e) => ("parse_error", Some(e.code), Vec::new(), 0, 0, None),
            Ok(cmd) => {
                assert_eq!(cmd.to_canonical(), text.as_bytes(), "{here}: a parsed command is canonical");
                match apply(&mut board, &cmd, &ctx(&entry["ctx"])) {
                    Err(r) => ("refused", Some(r.code), Vec::new(), 0, 0, None),
                    Ok(a) => {
                        let notices: Vec<String> =
                            a.notices.iter().map(|n| String::from_utf8(n.to_canonical()).expect("utf-8")).collect();
                        let result = if a.benign { "benign" } else { "applied" };
                        (result, None, notices, a.moved, a.skipped, a.restored_as)
                    }
                }
            }
        };
        let after = board.to_canonical_bytes();
        // CORE-DATA-006: a board that passes check still does.
        if let Err(e) = board.check() {
            out.push(format!("{here}: the board fails check: {e:?}"));
        }
        // CORE-APPLY-002, CORE-APPLY-004: refused, unparsed and benign entries change nothing.
        if result != "applied" && after != before {
            out.push(format!("{here}: a {result} entry changed the board"));
        }
        let want_notices: Vec<String> = ex
            .get("notices")
            .map(|v| v.as_array().expect("notices").iter().map(|n| n.as_str().expect("notice text").to_owned()).collect())
            .unwrap_or_default();
        let got = (result, code, &notices, u64::from(moved), u64::from(skipped), restored);
        let want = (
            ex["result"].as_str().expect("result"),
            ex.get("code").map(|c| c.as_str().expect("code")),
            &want_notices,
            uint(ex, "moved"),
            uint(ex, "skipped"),
            ex.get("restored_as").map(|r| r.as_u64().expect("restored_as")),
        );
        if got != want {
            out.push(format!("{here}: got {got:?}, recorded {want:?}"));
        }
        let hash = sha(&after);
        match ex.get("board_sha256").and_then(Value::as_str) {
            Some(h) if h == hash => {}
            Some(h) => out.push(format!("{here}: board_sha256 {hash}, recorded {h}")),
            None => {
                println!("FILL {file} {k} {n} {hash}");
                out.push(format!("{here}: no board_sha256"));
            }
        }
    }
    let end = board.to_canonical_bytes();
    let end_text = String::from_utf8(end.clone()).expect("utf-8");
    if case.get("start_gen").is_some() {
        match case.get("end_sha256").and_then(Value::as_str) {
            Some(h) if h == sha(&end) => {}
            Some(_) => out.push(at("end_sha256 differs")),
            None => {
                println!("FILL {file} {k} end_sha256 {}", sha(&end));
                out.push(at("no end_sha256"));
            }
        }
    } else {
        match case.get("end_text").and_then(Value::as_str) {
            Some(t) if t == end_text => {}
            Some(t) => out.push(format!("{}: end board\n  got      {end_text}\n  recorded {t}", at(""))),
            None => {
                println!("FILL {file} {k} end_text {end_text}");
                out.push(at("no end_text"));
            }
        }
    }
}

// spec: CORE-RULES-003
// spec: CORE-CMD-004
// spec: CORE-CMD-005
// spec: CORE-CMD-006
// spec: CORE-CMD-007
// spec: CORE-CMD-008
// spec: CORE-CMD-009
// spec: CORE-ID-002
// spec: CORE-ID-003
// spec: CORE-ID-005
// spec: CORE-CTX-001
// spec: CORE-CTX-003
// spec: CORE-APPLY-001
// spec: CORE-APPLY-002
// spec: CORE-APPLY-004
// spec: CORE-CONF-001
// spec: CORE-CONF-002
// spec: CORE-CONF-003
// spec: CORE-CONF-004
// spec: CORE-CONF-005
// spec: CORE-DATA-003
// spec: CORE-DATA-006
#[test]
fn golden_logs_of_rules_1_replay() {
    let mut out = Vec::new();
    let mut cases = 0;
    for (file, text) in FILES {
        let v: Value = serde_json::from_str(text).expect("a golden file is JSON");
        assert!(v["spec"].as_str().is_some_and(|s| s.starts_with("core.md §")), "{file} names its section");
        assert_eq!(v["rules"].as_u64(), Some(1), "{file} is a rules-1 log");
        let list = v["cases"].as_array().expect("cases");
        assert!(!list.is_empty());
        for (k, case) in list.iter().enumerate() {
            replay(file, k, case, &mut out);
            cases += 1;
        }
    }
    assert!(out.is_empty(), "{} mismatches over {cases} cases:\n{}", out.len(), out.join("\n"));
}
