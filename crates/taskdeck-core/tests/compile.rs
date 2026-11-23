//! Intents and the compile step (core.md §12, §13): the `intents.json` and
//! `compile.json` vector families (§18.2), every case of both; the caps and
//! the rules check on boards built here; and the property tests: neither
//! `parse_intent` nor `compile` panics (CORE-GEN-007), every command `compile`
//! returns parses and is accepted by `apply` on its view (CORE-COMP-008), and
//! the minter moves only on success (CORE-COMP-004).

use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;
use serde_json::Value;
use taskdeck_core::CompileError;
use taskdeck_core::apply::{Ctx, apply};
use taskdeck_core::board::{
    ArchiveRow, Block, BoardData, Item, ItemStamps, Kind, Mark, Outcome, PictureRef, Recurrence, RowBlock, Stamp,
    Subscription,
};
use taskdeck_core::canon::Record;
use taskdeck_core::command::{LogCommand, parse_command};
use taskdeck_core::compile::{CompileCtx, Compiled, Reply, checked_url, compile};
use taskdeck_core::ids::IdMinter;
use taskdeck_core::intent::{Intent, WireTime, parse_intent};
use taskdeck_core::overlay::{Overlay, OverlayEvent};
use taskdeck_core::planner::Flowable;
use taskdeck_core::read::reflow_inputs;
use taskdeck_core::time::{FixedZone, Instant, TableZone, Zone};

const INTENTS: &str = include_str!("vectors/intents.json");
const COMPILE: &str = include_str!("vectors/compile.json");

const L: u64 = 1 << 40;

fn family(text: &str, spec: &str) -> Vec<Value> {
    let v: Value = serde_json::from_str(text).expect("vector file is JSON");
    assert_eq!(v["spec"].as_str(), Some(spec), "the family names its section");
    let cases = v["cases"].as_array().expect("cases").clone();
    assert!(!cases.is_empty());
    cases
}

fn text<'a>(case: &'a Value, key: &str) -> &'a str {
    case[key].as_str().unwrap_or_else(|| panic!("{key}"))
}

fn u(v: &Value) -> u64 {
    v.as_u64().unwrap_or_else(|| panic!("an integer: {v}"))
}

fn date(v: &Value) -> chrono::NaiveDate {
    v.as_str().and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()).unwrap_or_else(|| panic!("a date: {v}"))
}

fn rfc3339(v: &Value) -> chrono::DateTime<chrono::FixedOffset> {
    v.as_str().and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).unwrap_or_else(|| panic!("RFC 3339: {v}"))
}

fn rgba(v: &Value) -> [u8; 4] {
    let a = v.as_array().expect("rgba");
    let mut out = [0u8; 4];
    for (o, x) in out.iter_mut().zip(a) {
        *o = u8::try_from(u(x)).expect("u8");
    }
    out
}

fn opt<T>(v: &Value, f: impl Fn(&Value) -> T) -> Option<T> {
    if v.is_null() { None } else { Some(f(v)) }
}

fn wire(v: &Value) -> Option<WireTime> {
    opt(v, |w| {
        if let Some(e) = w.get("exact") {
            WireTime::Exact(rfc3339(e))
        } else {
            let naive = chrono::NaiveDateTime::parse_from_str(text(w, "naive"), "%Y-%m-%dT%H:%M:%S").expect("naive");
            WireTime::Naive(naive, text(w, "text").to_owned())
        }
    })
}

fn hex<const N: usize>(s: &str) -> [u8; N] {
    let mut out = [0u8; N];
    for (k, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&s[2 * k..2 * k + 2], 16).expect("hex");
    }
    out
}

fn picture(v: &Value) -> PictureRef {
    PictureRef {
        blob: hex::<16>(text(v, "blob")),
        epoch: u32::try_from(u(&v["epoch"])).expect("u32"),
        parts: u32::try_from(u(&v["parts"])).expect("u32"),
        sha256: hex::<32>(text(v, "sha256")),
        size: u(&v["size"]),
    }
}

/// The intent a vector's `intent` object names, field by field (§12.3).
fn expected_intent(v: &Value) -> Intent {
    let id = || u(&v["id"]);
    let s = |k: &str| text(v, k).to_owned();
    let small = |k: &str| u8::try_from(u(&v[k])).expect("u8");
    let i32_of = |k: &str| i32::try_from(v[k].as_i64().expect("i32")).expect("i32");
    let u32_of = |k: &str| u32::try_from(u(&v[k])).expect("u32");
    match text(v, "op") {
        "add" => Intent::Add {
            name: s("name"),
            deadline: wire(&v["deadline"]),
            importance: opt(&v["importance"], |x| u8::try_from(u(x)).expect("u8")),
            is_event: v["is_event"].as_bool().expect("bool"),
            time_importance: opt(&v["time_importance"], |x| u8::try_from(u(x)).expect("u8")),
        },
        "quick_add" => Intent::QuickAdd { name: s("name"), deadline: wire(&v["deadline"]) },
        "create" => Intent::Create {
            kind: match text(v, "kind") {
                "task" => Kind::Task,
                "event" => Kind::Event,
                _ => Kind::Routine,
            },
            name: s("name"),
            day: date(&v["day"]),
            start: i32_of("start"),
            minutes: u32_of("minutes"),
        },
        "rename" => Intent::Rename { id: id(), name: s("name") },
        "complete" => Intent::Complete { id: id(), at: wire(&v["at"]) },
        "delete" => Intent::Delete { id: id(), at: wire(&v["at"]) },
        "forget" => Intent::Forget { id: id() },
        "move_block" => Intent::MoveBlock {
            id: id(),
            session: opt(&v["session"], u),
            day: date(&v["day"]),
            start: i32_of("start"),
            minutes: u32_of("minutes"),
        },
        "add_block" => Intent::AddBlock {
            id: id(),
            day: date(&v["day"]),
            start: i32_of("start"),
            minutes: opt(&v["minutes"], |x| u32::try_from(u(x)).expect("u32")),
        },
        "remove_block" => Intent::RemoveBlock { id: id(), session: u(&v["session"]) },
        "unplan" => Intent::Unplan { id: id() },
        "set_estimate" => Intent::SetEstimate { id: id(), minutes: u32_of("minutes") },
        "set_deadline" => Intent::SetDeadline { id: id(), deadline: wire(&v["deadline"]) },
        "set_severity" => Intent::SetSeverity { id: id(), level: small("level") },
        "set_horizon" => Intent::SetHorizon { id: id(), level: small("level") },
        "set_repeat" => Intent::SetRepeat { id: id(), days: small("days"), day: date(&v["day"]) },
        "reflow" => Intent::Reflow {
            day: date(&v["day"]),
            from: opt(&v["from"], |x| i32::try_from(x.as_i64().expect("i32")).expect("i32")),
        },
        "set_notes" => Intent::SetNotes { text: s("text"), base: opt(&v["base"], u) },
        "restore" => Intent::Restore { id: id(), archived_at: rfc3339(&v["archived_at"]) },
        "forget_archived" => Intent::ForgetArchived { id: id(), archived_at: rfc3339(&v["archived_at"]) },
        "add_subscription" => Intent::AddSubscription { name: s("name"), url: s("url"), color: opt(&v["color"], rgba) },
        "remove_subscription" => Intent::RemoveSubscription { id: id() },
        "rename_subscription" => Intent::RenameSubscription { id: id(), name: s("name") },
        "set_subscription_color" => Intent::SetSubscriptionColor { id: id(), color: rgba(&v["color"]) },
        "set_subscription_enabled" => {
            Intent::SetSubscriptionEnabled { id: id(), enabled: v["enabled"].as_bool().expect("bool") }
        }
        "set_palette" => {
            let a = v["colors"].as_array().expect("colors");
            let mut colors = [[0u8; 4]; 6];
            for (c, x) in colors.iter_mut().zip(a) {
                *c = rgba(x);
            }
            Intent::SetPalette { colors }
        }
        "set_home" => Intent::SetHome { lat: v["lat"].as_f64().expect("f64"), lon: v["lon"].as_f64().expect("f64") },
        "set_picture" => Intent::SetPicture { picture: opt(&v["picture"], picture) },
        other => panic!("no intent {other}"),
    }
}

// spec: CORE-INT-001
// spec: CORE-INT-002
// spec: CORE-INT-003
#[test]
fn intents_vectors() {
    let cases = family(INTENTS, "core.md §12");
    let mut ops = BTreeSet::new();
    for case in cases {
        let name = text(&case, "name");
        let got = parse_intent(text(&case, "intent_text"));
        match case.get("code") {
            Some(code) => {
                let e = got.err().unwrap_or_else(|| panic!("{name}: parsed"));
                assert_eq!(e.code, code.as_str().expect("code"), "{name}: {}", e.message);
                assert_eq!(u64::from(e.status), u(&case["status"]), "{name}");
                if let Some(m) = case.get("message") {
                    assert_eq!(e.message, m.as_str().expect("message"), "{name}");
                }
            }
            None => {
                let want = expected_intent(&case["intent"]);
                assert_eq!(got.unwrap_or_else(|e| panic!("{name}: {e:?}")), want, "{name}");
                ops.insert(text(&case["intent"], "op").to_owned());
            }
        }
    }
    // Every op of §12.3 that is not a query is in the family.
    assert_eq!(ops.len(), 28, "{ops:?}");
}

// ---------------------------------------------------------------- compile.json

fn zone_of(v: &Value) -> Box<dyn Zone> {
    if let Some(o) = v.get("fixed") {
        return Box::new(FixedZone(i32::try_from(o.as_i64().expect("offset")).expect("i32")));
    }
    let t = &v["table"];
    let initial = i32::try_from(t["initial"].as_i64().expect("initial")).expect("i32");
    let transitions = t["transitions"]
        .as_array()
        .expect("transitions")
        .iter()
        .map(|p| (p[0].as_i64().expect("utc_ms"), i32::try_from(p[1].as_i64().expect("offset")).expect("i32")))
        .collect();
    Box::new(TableZone::new(initial, transitions).expect("a table zone"))
}

/// An overlay holding one busy event per anchor `[day, start, end]`.
fn overlay_of(v: &Value) -> Overlay {
    let events = v
        .as_array()
        .expect("anchors")
        .iter()
        .map(|a| OverlayEvent {
            subscription: 8,
            day: date(&a[0]),
            start: i32::try_from(a[1].as_i64().expect("start")).expect("i32"),
            end: i32::try_from(a[2].as_i64().expect("end")).expect("i32"),
            all_day: false,
            free: false,
            summary: String::new(),
            location: String::new(),
            description: String::new(),
        })
        .collect();
    Overlay::sealed(events, Vec::new())
}

fn minter_of(v: &Value) -> IdMinter {
    IdMinter {
        device_no: u16::try_from(u(&v["device_no"])).expect("u16"),
        next_item: u(&v["next_item"]),
        next_block: u(&v["next_block"]),
    }
}

fn reply_of(v: &Value) -> Reply {
    Reply {
        id: opt(&v["id"], u),
        session: opt(&v["session"], u),
        moved: opt(&v["moved"], |x| u32::try_from(u(x)).expect("u32")),
    }
}

fn ctx(device_no: u16, at: Instant) -> Ctx {
    Ctx { seq: 1000, device_no, at, based_on: 999 }
}

/// CORE-COMP-008: the command parses after canonical encoding and `apply`
/// accepts it on `view` with the origin's `device_no` and `at = now`.
fn accepted(view: &BoardData, command: &LogCommand, device_no: u16, now: Instant) -> Result<(), &'static str> {
    let bytes = command.to_canonical();
    assert_eq!(parse_command(&bytes).as_ref(), Ok(command), "{}", String::from_utf8_lossy(&bytes));
    let mut b = view.clone();
    apply(&mut b, command, &ctx(device_no, now)).map(|_| ()).map_err(|r| r.code)
}

// spec: CORE-COMP-001
// spec: CORE-COMP-002
// spec: CORE-COMP-003
// spec: CORE-COMP-004
// spec: CORE-COMP-005
// spec: CORE-COMP-006
// spec: CORE-COMP-008
// spec: CORE-ID-004
// spec: CORE-TIME-003
#[test]
fn compile_vectors() {
    let mut ops = BTreeSet::new();
    for case in family(COMPILE, "core.md §13") {
        let name = text(&case, "name");
        let view = BoardData::from_canonical_bytes(text(&case, "view_text").as_bytes())
            .unwrap_or_else(|e| panic!("{name}: the view is a board: {e:?}"));
        let zone = zone_of(&case["zone"]);
        let now = Instant::parse_canonical(text(&case, "now")).expect("now");
        let device_no = u16::try_from(u(&case["device_no"])).expect("u16");
        let overlay = overlay_of(&case["anchors"]);
        let before = minter_of(&case["minter"]);
        let mut minter = before;
        let got = parse_intent(text(&case, "intent_text")).and_then(|intent| {
            let cx = CompileCtx { view: &view, zone: &*zone, now, device_no, overlay: &overlay };
            compile(&intent, &cx, &mut minter)
        });
        if let Some(code) = case.get("code") {
            let e = got.err().unwrap_or_else(|| panic!("{name}: compiled"));
            let want = (code.as_str().expect("code"), u(&case["status"]), text(&case, "message"));
            assert_eq!((e.code, u64::from(e.status), e.message.as_str()), want, "{name}");
            assert_eq!(minter, before, "{name}: the minter does not move on an error");
            continue;
        }
        let Compiled { command, reply } = got.unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert_eq!(reply, reply_of(&case["reply"]), "{name}");
        let after = case.get("minter_after").map_or(before, minter_of);
        assert_eq!(minter, after, "{name}: the minter after");
        match (&command, case["command_text"].as_str()) {
            (None, None) => {}
            (Some(c), Some(t)) => {
                assert_eq!(String::from_utf8(c.to_canonical()).expect("utf-8"), t, "{name}");
                ops.insert(c.op());
                let verdict = accepted(&view, c, device_no, now);
                match case.get("apply_code") {
                    Some(code) => assert_eq!(verdict, Err(code.as_str().expect("code")), "{name}"),
                    None => assert_eq!(verdict, Ok(()), "{name}"),
                }
            }
            (c, t) => panic!("{name}: {c:?} against {t:?}"),
        }
    }
    // Every op of the rules-1 table (§9.3) is emitted by some case (§13.4).
    assert_eq!(ops.len(), 29, "{ops:?}");
}

// ---------------------------------------------------------------- caps and rules on built boards

fn at(minutes: i64) -> Instant {
    Instant::from_parts(1_773_100_800 + minutes * 60, 0).expect("in range") // 2026-03-10T00:00Z + minutes
}

const STAMP: Stamp = Stamp { seq: 1, device_no: 0 };
const STAMPS: ItemStamps = ItemStamps { estimate: STAMP, levels: STAMP, name: STAMP, repeat: STAMP, time: STAMP };

fn task(id: u64, sids: impl IntoIterator<Item = u64>) -> Item {
    Item {
        id,
        kind: Kind::Task,
        name: "t".to_owned(),
        created: at(0),
        deadline: None,
        importance: None,
        time_importance: None,
        duration_minutes: None,
        recurrence: None,
        blocks: sids.into_iter().map(|sid| Block { sid, start: at(600), minutes: 30, stamp: STAMP }).collect(),
        stamps: STAMPS,
    }
}

fn row(id: u64, archived_at: i64) -> ArchiveRow {
    ArchiveRow {
        id,
        kind: Kind::Task,
        name: "r".to_owned(),
        created: at(0),
        deadline: None,
        importance: None,
        time_importance: None,
        duration_minutes: None,
        recurrence: None,
        blocks: Vec::new(),
        archived_at: at(archived_at),
        outcome: Outcome::Finished,
    }
}

fn run(view: &BoardData, intent: &Intent) -> (Result<Compiled, CompileError>, IdMinter) {
    let overlay = Overlay::default();
    let cx = CompileCtx { view, zone: &FixedZone(0), now: at(720), device_no: 1, overlay: &overlay };
    let mut minter = IdMinter::new(1);
    let got = compile(intent, &cx, &mut minter);
    (got, minter)
}

fn code(r: &Result<Compiled, CompileError>) -> Option<&'static str> {
    r.as_ref().err().map(|e| e.code)
}

fn day() -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(2026, 3, 10).expect("a date")
}

fn creates() -> Vec<Intent> {
    vec![
        Intent::Add { name: "a".into(), deadline: None, importance: None, is_event: false, time_importance: None },
        Intent::QuickAdd { name: "a".into(), deadline: None },
        Intent::Create { kind: Kind::Task, name: String::new(), day: day(), start: 600, minutes: 30 },
        Intent::Create { kind: Kind::Event, name: String::new(), day: day(), start: 600, minutes: 30 },
        Intent::Create { kind: Kind::Routine, name: String::new(), day: day(), start: 600, minutes: 30 },
    ]
}

// spec: CORE-COMP-001
// spec: CORE-COMP-004
#[test]
fn the_live_item_cap() {
    let mut view = BoardData::empty();
    view.items = (1..=10_000).map(|id| task(id, [])).collect();
    view.archive = vec![row(10_001, 0)];
    let none = IdMinter::new(1);
    for intent in creates() {
        let (got, minter) = run(&view, &intent);
        assert_eq!(code(&got), Some("core/cap_items"), "{intent:?}");
        assert_eq!(got.err().map(|e| (e.status, e.message)), Some((400, "That is as many items as a board can hold.".to_owned())));
        assert_eq!(minter, none);
    }
    let restore = Intent::Restore { id: 10_001, archived_at: at(0).to_chrono().fixed_offset() };
    assert_eq!(code(&run(&view, &restore).0), Some("core/cap_items"));
    // One fewer, and they compile.
    view.items.pop();
    for intent in creates() {
        assert!(run(&view, &intent).0.is_ok(), "{intent:?}");
    }
    assert!(run(&view, &restore).0.is_ok());
}

// spec: CORE-COMP-001
#[test]
fn the_block_caps() {
    // 50,000 live blocks on 50 tasks of 1,000.
    let mut view = BoardData::empty();
    view.items = (0..50u64).map(|k| task(k + 1, (k * 1000 + 1)..=(k * 1000 + 1000))).collect();
    view.items.push(task(51, []));
    view.archive = vec![ArchiveRow { blocks: vec![RowBlock { sid: 60_000, start: at(0), minutes: 15 }], ..row(52, 0) }];
    let task_create = Intent::Create { kind: Kind::Task, name: String::new(), day: day(), start: 600, minutes: 30 };
    assert_eq!(code(&run(&view, &task_create).0), Some("core/cap_blocks"));
    let event_create = Intent::Create { kind: Kind::Event, name: String::new(), day: day(), start: 600, minutes: 30 };
    assert!(run(&view, &event_create).0.is_ok(), "an event takes no block");
    let add = Intent::AddBlock { id: 51, day: day(), start: 600, minutes: None };
    assert_eq!(code(&run(&view, &add).0), Some("core/cap_blocks"));
    let restore = Intent::Restore { id: 52, archived_at: at(0).to_chrono().fixed_offset() };
    assert_eq!(code(&run(&view, &restore).0), Some("core/cap_blocks"));
    // One block fewer on the board: the board cap passes, the item cap of
    // 1,000 still holds for the full task.
    view.items[0].blocks.pop();
    assert!(run(&view, &task_create).0.is_ok());
    assert!(run(&view, &add).0.is_ok());
    assert!(run(&view, &restore).0.is_ok());
    let full = Intent::AddBlock { id: 2, day: day(), start: 600, minutes: None };
    assert_eq!(code(&run(&view, &full).0), Some("core/cap_blocks"));
    assert_eq!(
        run(&view, &full).0.err().map(|e| (e.status, e.message)),
        Some((400, "That is as many blocks as a board can hold.".to_owned()))
    );
}

// spec: CORE-COMP-001
#[test]
fn the_archive_cap() {
    let mut view = BoardData::empty();
    view.items = vec![task(1, [])];
    view.archive = (0..200_000).map(|k| row(2, k)).collect();
    for intent in [Intent::Complete { id: 1, at: None }, Intent::Delete { id: 1, at: None }] {
        let got = run(&view, &intent).0;
        assert_eq!(got.err().map(|e| (e.code, e.status, e.message)), Some(("core/cap_archive", 400, "The archive is full.".to_owned())));
    }
    view.archive.pop();
    assert!(run(&view, &Intent::Complete { id: 1, at: None }).0.is_ok());
}

// spec: CORE-COMP-001
#[test]
fn the_subscription_cap() {
    let mut view = BoardData::empty();
    view.subscriptions = (1..=20)
        .map(|id| Subscription { id, name: "s".into(), url: format!("https://e.example/{id}"), color: [0; 4], enabled: true })
        .collect();
    // The cap comes before the address.
    let add = Intent::AddSubscription { name: String::new(), url: "http://x".into(), color: None };
    let got = run(&view, &add).0;
    assert_eq!(
        got.err().map(|e| (e.code, e.status, e.message)),
        Some(("core/calendars_full", 400, "That is already 20 calendars, which is as many as this keeps.".to_owned()))
    );
    view.subscriptions.pop();
    let add = Intent::AddSubscription { name: String::new(), url: "https://x.example".into(), color: None };
    let got = run(&view, &add).0.expect("19 subscriptions take one more");
    // n = 19: DEFAULT_COLORS[19 mod 6 = 1].
    assert!(matches!(got.command, Some(LogCommand::AddSubscription { color: [200, 120, 90, 255], .. })));
}

// spec: CORE-COMP-001
#[test]
fn notes_are_capped_after_detab() {
    let view = BoardData::empty();
    // 19,997 scalars and one tab: 20,000 after detab.
    let fits = Intent::SetNotes { text: format!("{}\t", "é".repeat(19_996)), base: None };
    let got = run(&view, &fits).0.expect("20,000 scalars fit");
    assert!(matches!(&got.command, Some(LogCommand::SetNotes { text, base: 0 }) if text.chars().count() == 20_000));
    let over = Intent::SetNotes { text: format!("{}\t", "é".repeat(19_997)), base: None };
    assert_eq!(
        run(&view, &over).0.err().map(|e| (e.code, e.status, e.message)),
        Some(("core/notes_long", 400, "Those notes are too long for the notepad.".to_owned()))
    );
}

// spec: CORE-RULES-004
#[test]
fn nothing_outside_the_op_table_of_the_view_rules() {
    let mut view = BoardData::empty();
    view.items = vec![task(1, [7])];
    view.rules = 2; // no version this build knows: an empty op table
    let none = IdMinter::new(1);
    for intent in creates().into_iter().chain([
        Intent::Rename { id: 1, name: "x".into() },
        Intent::Unplan { id: 1 },
        Intent::SetPalette { colors: [[0; 4]; 6] },
    ]) {
        let (got, minter) = run(&view, &intent);
        assert_eq!(code(&got), Some("core/rules"), "{intent:?}");
        assert_eq!(minter, none, "{intent:?}");
    }
    // An intent that compiles to no command is not refused.
    view.items = vec![task(1, [])];
    assert!(run(&view, &Intent::Unplan { id: 1 }).0.is_ok());
}

// spec: CORE-COMP-001
#[test]
fn the_url_check() {
    assert_eq!(checked_url("  webcal://a.b/c  ").as_deref(), Ok("https://a.b/c"));
    assert_eq!(checked_url("https://a.b?x=1").as_deref(), Ok("https://a.b?x=1"));
    // 2,000 scalars after trim, then the rewrite: allowed.
    let long = format!("webcal://a.b/{}", "x".repeat(1987));
    assert_eq!(long.chars().count(), 2000);
    assert!(checked_url(&format!("\u{3000}{long}\u{3000}")).is_ok());
    assert_eq!(checked_url(&format!("{long}x")).map_err(|e| e.code), Err("core/url_long"));
    assert_eq!(checked_url("https://a#b.c").map_err(|e| e.code), Err("core/url_host"));
    assert_eq!(checked_url("HTTPS://a.b").map_err(|e| e.code), Err("core/url_scheme"));
    assert_eq!(checked_url("\u{200B}https://a.b").map_err(|e| e.code), Err("core/url_scheme"));
}

// spec: CORE-COMP-005
#[test]
fn reflow_inputs_in_order() {
    // Item 1: blocks with sids 9 and 4 stored ascending (4, 9), a deadline
    // marker; item 2: an event block; item 3: a routine; item 5: a block on
    // another day.
    let mut view = BoardData::empty();
    let mut one = task(1, [4, 9]);
    one.blocks[0].start = at(540);
    one.deadline = Some(at(1000));
    let mut two = task(2, []);
    two.kind = Kind::Event;
    two.deadline = Some(at(600));
    two.duration_minutes = Some(5);
    let mut three = task(3, []);
    three.kind = Kind::Routine;
    three.recurrence = Some(REC);
    let mut five = task(5, [11]);
    five.blocks[0].start = at(1440 + 600);
    view.items = vec![one, two, three, five];
    let overlay = Overlay::sealed(
        vec![OverlayEvent {
            subscription: 8,
            day: day(),
            start: 700,
            end: 760,
            all_day: false,
            free: false,
            summary: String::new(),
            location: String::new(),
            description: String::new(),
        }],
        Vec::new(),
    );
    let (flowables, anchors) = reflow_inputs(&view, &overlay, day(), &FixedZone(0));
    assert_eq!(
        flowables,
        vec![Flowable { key: (1, 4), start: 540, minutes: 30 }, Flowable { key: (1, 9), start: 600, minutes: 30 }]
    );
    // The event's block at least 15 minutes, the routine's rule, then the overlay.
    assert_eq!(anchors, vec![(600, 615), (1320, 1380), (700, 760)]);
}

// spec: CORE-COMP-007
#[test]
fn now_is_the_only_clock() {
    let mut view = BoardData::empty();
    view.items = vec![task(1, [])];
    let overlay = Overlay::default();
    for m in [0i64, 1, 59, 600, 1439, 100_000] {
        let now = at(m);
        let cx = CompileCtx { view: &view, zone: &FixedZone(0), now, device_no: 1, overlay: &overlay };
        let mut minter = IdMinter::new(1);
        let got = compile(&Intent::Complete { id: 1, at: None }, &cx, &mut minter).expect("compiles");
        assert_eq!(got.command, Some(LogCommand::Complete { id: 1, at: now }));
        // The same inputs, the same result.
        let again = compile(&Intent::Complete { id: 1, at: None }, &cx, &mut minter).expect("compiles");
        assert_eq!(again, got);
    }
}

// ---------------------------------------------------------------- property tests

fn seeds() -> Vec<String> {
    let mut out: Vec<String> =
        family(INTENTS, "core.md §12").iter().map(|c| text(c, "intent_text").to_owned()).collect();
    out.extend(family(COMPILE, "core.md §13").iter().map(|c| text(c, "intent_text").to_owned()));
    out
}

#[derive(Clone, Debug)]
enum Edit {
    Flip(usize, u8),
    Cut(usize),
    Insert(usize, String),
}

fn edit() -> impl Strategy<Value = Edit> {
    prop_oneof![
        (any::<usize>(), any::<u8>()).prop_map(|(i, b)| Edit::Flip(i, b)),
        any::<usize>().prop_map(Edit::Cut),
        (any::<usize>(), prop_oneof![
            Just("null".to_owned()),
            Just("\"\"".to_owned()),
            Just("-1".to_owned()),
            Just("1e999".to_owned()),
            Just("[".to_owned()),
            Just("\"2026-03-29T02:30\"".to_owned()),
            "\\PC{0,8}",
        ])
            .prop_map(|(i, s)| Edit::Insert(i, s)),
    ]
}

fn mutate(text: &str, edits: &[Edit]) -> String {
    let mut b = text.as_bytes().to_vec();
    for e in edits {
        match e {
            Edit::Flip(i, x) if !b.is_empty() => {
                let k = i % b.len();
                b[k] = *x;
            }
            Edit::Cut(i) if !b.is_empty() => {
                b.truncate(i % b.len());
            }
            Edit::Insert(i, s) => {
                let k = i % (b.len() + 1);
                b.splice(k..k, s.bytes());
            }
            _ => {}
        }
    }
    String::from_utf8_lossy(&b).into_owned()
}

/// A day the boards below use, or one at an edge.
fn a_day() -> impl Strategy<Value = chrono::NaiveDate> {
    prop_oneof![
        4 => (0u32..4).prop_map(|k| day() + chrono::Days::new(u64::from(k))),
        1 => Just(chrono::NaiveDate::from_ymd_opt(2026, 3, 29).expect("a date")),
        1 => Just(chrono::NaiveDate::from_ymd_opt(1970, 1, 1).expect("a date")),
        1 => Just(chrono::NaiveDate::from_ymd_opt(9999, 12, 31).expect("a date")),
        1 => Just(chrono::NaiveDate::from_ymd_opt(1969, 12, 31).expect("a date")),
    ]
}

fn a_wire() -> impl Strategy<Value = Option<WireTime>> {
    let naive = (a_day(), 0u32..24, 0u32..60).prop_map(|(d, h, m)| {
        let n = d.and_hms_opt(h, m, 0).expect("a time");
        WireTime::Naive(n, n.format("%Y-%m-%dT%H:%M").to_string())
    });
    let exact = (-200_000i64..5_000_000, -50_000i32..50_000).prop_map(|(m, o)| {
        let t = chrono::DateTime::from_timestamp(1_773_100_800 + m * 60, 0).expect("chrono");
        WireTime::Exact(t.with_timezone(&chrono::FixedOffset::east_opt(o).expect("offset")))
    });
    prop_oneof![2 => Just(None), 2 => naive.prop_map(Some), 2 => exact.prop_map(Some),
        1 => Just(Some(WireTime::Exact(chrono::DateTime::parse_from_rfc3339("0001-01-01T00:00:00Z").expect("rfc"))))]
}

/// An id of the small pool the boards share, or one no board holds.
fn an_id() -> impl Strategy<Value = u64> {
    prop_oneof![4 => (0u64..3, 0u64..5).prop_map(|(d, n)| d * L + n), 1 => any::<u64>()]
}

fn a_name() -> impl Strategy<Value = String> {
    prop_oneof![4 => "[ a-z\t\u{3000}]{0,6}", 1 => Just("x".repeat(201)), 1 => Just(format!(" {} ", "y".repeat(200)))]
}

fn a_kind() -> impl Strategy<Value = Kind> {
    prop_oneof![Just(Kind::Task), Just(Kind::Event), Just(Kind::Routine)]
}

fn an_archived_at() -> impl Strategy<Value = chrono::DateTime<chrono::FixedOffset>> {
    (0i64..6).prop_map(|k| at(k * 60).to_chrono().fixed_offset())
}

fn a_picture() -> impl Strategy<Value = Option<PictureRef>> {
    proptest::option::of((0u32..3, 0u32..70, 0u64..70_000_000).prop_map(|(epoch, parts, size)| PictureRef {
        blob: [1; 16],
        epoch,
        parts,
        sha256: [2; 32],
        size,
    }))
}

fn an_intent() -> impl Strategy<Value = Intent> {
    let start = prop_oneof![-100i32..1600, any::<i32>()];
    let minutes = prop_oneof![0u32..1600, any::<u32>()];
    prop_oneof![
        (a_name(), a_wire(), proptest::option::of(0u8..6), any::<bool>(), proptest::option::of(0u8..5)).prop_map(
            |(name, deadline, importance, is_event, time_importance)| Intent::Add {
                name,
                deadline,
                importance,
                is_event,
                time_importance
            }
        ),
        (a_name(), a_wire()).prop_map(|(name, deadline)| Intent::QuickAdd { name, deadline }),
        (a_kind(), a_name(), a_day(), start.clone(), minutes.clone())
            .prop_map(|(kind, name, day, start, minutes)| Intent::Create { kind, name, day, start, minutes }),
        (an_id(), a_name()).prop_map(|(id, name)| Intent::Rename { id, name }),
        (an_id(), a_wire()).prop_map(|(id, at)| Intent::Complete { id, at }),
        (an_id(), a_wire()).prop_map(|(id, at)| Intent::Delete { id, at }),
        an_id().prop_map(|id| Intent::Forget { id }),
        (an_id(), proptest::option::of(an_id()), a_day(), start.clone(), minutes.clone()).prop_map(
            |(id, session, day, start, minutes)| Intent::MoveBlock { id, session, day, start, minutes }
        ),
        (an_id(), a_day(), start.clone(), proptest::option::of(minutes.clone()))
            .prop_map(|(id, day, start, minutes)| Intent::AddBlock { id, day, start, minutes }),
        (an_id(), an_id()).prop_map(|(id, session)| Intent::RemoveBlock { id, session }),
        an_id().prop_map(|id| Intent::Unplan { id }),
        (an_id(), prop_oneof![10u32..1450, any::<u32>()]).prop_map(|(id, minutes)| Intent::SetEstimate { id, minutes }),
        (an_id(), a_wire()).prop_map(|(id, deadline)| Intent::SetDeadline { id, deadline }),
        (an_id(), 0u8..7).prop_map(|(id, level)| Intent::SetSeverity { id, level }),
        (an_id(), 0u8..7).prop_map(|(id, level)| Intent::SetHorizon { id, level }),
        (an_id(), any::<u8>(), a_day()).prop_map(|(id, days, day)| Intent::SetRepeat { id, days, day }),
        (a_day(), proptest::option::of(prop_oneof![-100i32..1600, any::<i32>()]))
            .prop_map(|(day, from)| Intent::Reflow { day, from }),
        "[a\t\n é]{0,12}".prop_map(|text| Intent::SetNotes { text, base: None }),
        (an_id(), an_archived_at()).prop_map(|(id, archived_at)| Intent::Restore { id, archived_at }),
        (an_id(), an_archived_at()).prop_map(|(id, archived_at)| Intent::ForgetArchived { id, archived_at }),
        (
            a_name(),
            prop_oneof![
                (0u64..3, 1u64..5).prop_map(|(d, n)| format!("https://e.example/{}", d * L + n)),
                "(https|http|webcal|ftp)://[a-z.@/]{0,8}",
            ],
            proptest::option::of(any::<[u8; 4]>())
        )
            .prop_map(|(name, url, color)| Intent::AddSubscription { name, url, color }),
        an_id().prop_map(|id| Intent::RemoveSubscription { id }),
        (an_id(), a_name()).prop_map(|(id, name)| Intent::RenameSubscription { id, name }),
        (an_id(), any::<[u8; 4]>()).prop_map(|(id, color)| Intent::SetSubscriptionColor { id, color }),
        (an_id(), any::<bool>()).prop_map(|(id, enabled)| Intent::SetSubscriptionEnabled { id, enabled }),
        any::<[[u8; 4]; 6]>().prop_map(|colors| Intent::SetPalette { colors }),
        (prop_oneof![-100.0f64..100.0, any::<f64>()], prop_oneof![-200.0f64..200.0, any::<f64>()])
            .prop_map(|(lat, lon)| Intent::SetHome { lat, lon }),
        a_picture().prop_map(|picture| Intent::SetPicture { picture }),
    ]
}

fn a_stamp() -> impl Strategy<Value = Stamp> {
    (1u64..30, 0u16..4).prop_map(|(seq, device_no)| Stamp { seq, device_no })
}

const REC: Recurrence = Recurrence {
    days: 3,
    anchor: match chrono::NaiveDate::from_ymd_opt(2026, 3, 10) {
        Some(d) => d,
        None => panic!("a date"),
    },
    start_minutes: 1320,
    minutes: 60,
};

/// A board that passes `check`, its blocks and deadlines over three days
/// from 2026-03-10: ids distinct, live sids distinct, the archive in
/// archive order, marks at or above every id.
fn a_board() -> impl Strategy<Value = BoardData> {
    let raw_item = (
        (0u64..3, 1u64..5),
        0u8..3,
        proptest::option::of(0u8..5),
        proptest::option::of(0i64..4320),
        proptest::option::of(0u32..1441),
        proptest::collection::vec(((0u64..3, 1u64..5), 0i64..4320, 0u32..1441, a_stamp()), 0..3),
    );
    let raw_row = ((0u64..3, 0u64..5), 0u8..3, 0i64..6, proptest::collection::vec((0u64..3, 1u64..5), 0..3));
    (
        proptest::collection::vec(raw_item, 0..10),
        proptest::collection::vec(raw_row, 0..5),
        proptest::collection::vec(((0u64..3, 1u64..5), any::<bool>()), 0..3),
        0u64..4,
        proptest::collection::vec((0u16..4, 0u64..3, 0u64..3), 0..3),
    )
        .prop_map(|(raw_items, raw_rows, raw_subs, notes_rev, extra)| {
            let mut b = BoardData::empty();
            let mut items: BTreeMap<u64, Item> = BTreeMap::new();
            let mut live_sids = BTreeSet::new();
            for ((d, n), k, importance, deadline, duration, raw_blocks) in raw_items {
                let id = d * L + n;
                if items.contains_key(&id) {
                    continue;
                }
                let kind = match k {
                    0 => Kind::Task,
                    1 => Kind::Event,
                    _ => Kind::Routine,
                };
                let mut blocks: Vec<Block> = Vec::new();
                if kind == Kind::Task {
                    for ((bd, bn), start, minutes, stamp) in raw_blocks {
                        let sid = bd * L + bn;
                        if live_sids.insert(sid) {
                            blocks.push(Block { sid, start: at(start), minutes, stamp });
                        }
                    }
                }
                blocks.sort_by_key(|x| x.sid);
                let mut item = task(id, []);
                item.kind = kind;
                item.blocks = blocks;
                item.deadline = deadline.filter(|_| kind != Kind::Routine).map(at);
                item.duration_minutes = duration.filter(|_| kind != Kind::Routine);
                item.importance = importance.filter(|_| kind == Kind::Task);
                item.recurrence = (kind == Kind::Routine).then_some(REC);
                items.insert(id, item);
            }
            b.items = items.into_values().collect();
            let mut rows: Vec<ArchiveRow> = raw_rows
                .into_iter()
                .map(|((d, n), k, archived_at, sids)| {
                    let kind = if k == 0 { Kind::Task } else { Kind::Event };
                    let mut sids: Vec<u64> = sids.iter().map(|&(d, n)| d * L + n).collect();
                    sids.sort_unstable();
                    sids.dedup();
                    ArchiveRow {
                        id: if n == 0 { 0 } else { d * L + n },
                        kind,
                        blocks: sids.into_iter().map(|sid| RowBlock { sid, start: at(30), minutes: 15 }).collect(),
                        ..row(0, archived_at * 60)
                    }
                })
                .collect();
            rows.sort_by_key(|r| (r.archived_at, r.id, r.to_canonical()));
            b.archive = rows;
            let mut subs: BTreeMap<u64, Subscription> = BTreeMap::new();
            for ((d, n), enabled) in raw_subs {
                let id = d * L + n;
                if !items_hold(&b, id) {
                    subs.insert(
                        id,
                        Subscription { id, name: "s".to_owned(), url: format!("https://e.example/{id}"), color: [0; 4], enabled },
                    );
                }
            }
            b.subscriptions = subs.into_values().collect();
            b.notes_rev = notes_rev;
            let mut marks: BTreeMap<u16, (u64, u64)> = BTreeMap::new();
            let mut see = |id: u64, sid: bool| {
                let (h, n) = (u16::try_from(id / L).expect("high part"), id % L);
                let m = marks.entry(h).or_default();
                if sid {
                    m.1 = m.1.max(n);
                } else {
                    m.0 = m.0.max(n);
                }
            };
            for i in &b.items {
                see(i.id, false);
                i.blocks.iter().for_each(|x| see(x.sid, true));
            }
            for r in &b.archive {
                if r.id != 0 {
                    see(r.id, false);
                }
                r.blocks.iter().for_each(|x| see(x.sid, true));
            }
            b.subscriptions.iter().for_each(|s| see(s.id, false));
            for (d, i, k) in extra {
                let m = marks.entry(d).or_default();
                m.0 += i;
                m.1 += k;
            }
            b.marks = marks
                .into_iter()
                .filter(|&(_, (i, k))| (i, k) != (0, 0))
                .map(|(device_no, (item, block))| Mark { device_no, item, block })
                .collect();
            b
        })
}

/// The ids, sids and fallbacks a command mints (core.md §7.5 step 1), each
/// with its space: item (`false`) or block (`true`).
fn minted(c: &LogCommand) -> Vec<(bool, u64)> {
    match c {
        LogCommand::CreateTask(t) => std::iter::once((false, t.id)).chain(t.blocks.iter().map(|b| (true, b.sid))).collect(),
        LogCommand::CreateEvent(e) => vec![(false, e.id)],
        LogCommand::CreateRoutine(r) => vec![(false, r.id)],
        LogCommand::AddBlock { sid, .. } => vec![(true, *sid)],
        LogCommand::Restore { fallback, .. } => vec![(false, *fallback)],
        LogCommand::AddSubscription { id, .. } => vec![(false, *id)],
        _ => Vec::new(),
    }
}

fn items_hold(b: &BoardData, id: u64) -> bool {
    b.item(id).is_some()
}

fn a_zone() -> impl Strategy<Value = TableZone> {
    prop_oneof![
        (-50_000i32..50_000).prop_map(|o| TableZone::new(o, Vec::new()).expect("a zone")),
        Just(TableZone::new(3600, vec![(1_774_746_000_000, 7200), (1_792_890_000_000, 3600)]).expect("a zone")),
        Just(TableZone::new(0, vec![(1_773_140_400_000, 10_800)]).expect("a zone")), // +3 h from 2026-03-10T11:00Z
    ]
}

fn anchors() -> impl Strategy<Value = Overlay> {
    proptest::collection::vec((0u32..4, -10i32..1500, 0i32..800), 0..4).prop_map(|v| {
        let events = v
            .into_iter()
            .map(|(k, start, len)| OverlayEvent {
                subscription: 1,
                day: day() + chrono::Days::new(u64::from(k)),
                start,
                end: start + len,
                all_day: false,
                free: false,
                summary: String::new(),
                location: String::new(),
                description: String::new(),
            })
            .collect();
        Overlay::sealed(events, Vec::new())
    })
}

proptest! {
    // spec: CORE-GEN-007
    // spec: CORE-INT-001
    #[test]
    fn parse_intent_never_panics_on_random_text(text in "\\PC{0,200}") {
        let _ = parse_intent(&text);
    }

    // spec: CORE-GEN-007
    // spec: CORE-INT-001
    #[test]
    fn parse_intent_never_panics_on_mutated_intents(
        k in any::<usize>(),
        edits in proptest::collection::vec(edit(), 1..4),
    ) {
        let seeds = seeds();
        let text = mutate(&seeds[k % seeds.len()], &edits);
        if let Err(e) = parse_intent(&text) {
            prop_assert!(e.code == "core/intent_json" || e.code == "core/query", "{}", e.code);
            prop_assert_eq!(e.status, 400);
        }
    }

    // spec: CORE-GEN-007
    // spec: CORE-COMP-004
    // spec: CORE-COMP-008
    // spec: CORE-ID-004
    // spec: CORE-RULES-004
    #[test]
    fn compiled_commands_parse_and_apply_on_the_view(
        view in a_board(),
        intent in an_intent(),
        zone in a_zone(),
        now_min in 0i64..4320,
        device_no in 1u16..4,
        next in (1u64..8, 1u64..8),
        overlay in anchors(),
    ) {
        let now = at(now_min);
        let before = IdMinter { device_no, next_item: next.0, next_block: next.1 };
        let mut minter = before;
        let cx = CompileCtx { view: &view, zone: &zone, now, device_no, overlay: &overlay };
        match compile(&intent, &cx, &mut minter) {
            Err(e) => {
                prop_assert_eq!(minter, before, "the minter moved on {}", e.code);
                prop_assert!(e.status == 400 || e.status == 410);
                prop_assert!(!e.message.is_empty());
            }
            Ok(Compiled { command: None, .. }) => prop_assert_eq!(minter, before),
            Ok(Compiled { command: Some(c), .. }) => {
                prop_assert_eq!(accepted(&view, &c, device_no, now), Ok(()), "{:?} from {:?}", c, intent);
                // Never the same number twice from one minter (CORE-ID-004).
                prop_assert!(minter.next_item >= before.next_item && minter.next_block >= before.next_block);
                let mut again = minter;
                if let Ok(Compiled { command: Some(c2), .. }) = compile(&intent, &cx, &mut again) {
                    let (first, second) = (minted(&c), minted(&c2));
                    prop_assert!(first.iter().all(|x| !second.contains(x)), "{:?} then {:?}", first, second);
                }
            }
        }
    }

    // spec: CORE-COMP-008
    #[test]
    fn set_notes_with_the_view_base_applies(view in a_board(), text in "[a\t\n é]{0,40}") {
        let overlay = Overlay::default();
        let cx = CompileCtx { view: &view, zone: &FixedZone(0), now: at(0), device_no: 1, overlay: &overlay };
        let mut minter = IdMinter::new(1);
        for base in [None, Some(view.notes_rev)] {
            let got = compile(&Intent::SetNotes { text: text.clone(), base }, &cx, &mut minter).expect("compiles");
            let c = got.command.expect("a command");
            prop_assert_eq!(accepted(&view, &c, 1, at(0)), Ok(()));
        }
    }
}
