//! `reminders.json` (core.md §14.5, §18.2): every case of the family, and the
//! default leads and the stable key.

use serde_json::Value;
use taskdeck_core::board::BoardData;
use taskdeck_core::canon;
use taskdeck_core::reminders::{ReminderDefaults, ReminderKind, reminders};
use taskdeck_core::time::Instant;

const REMINDERS: &str = include_str!("vectors/reminders.json");

fn instant(v: &Value) -> Instant {
    Instant::parse_canonical(v.as_str().expect("instant text")).expect("a valid instant")
}

fn lead(v: &Value, key: &str) -> u32 {
    u32::try_from(v[key].as_u64().expect("a lead")).expect("u32")
}

fn defaults(case: &Value) -> ReminderDefaults {
    match case.get("defaults") {
        None => ReminderDefaults::default(),
        Some(d) => ReminderDefaults {
            event_lead_minutes: lead(d, "event_lead_minutes"),
            block_lead_minutes: lead(d, "block_lead_minutes"),
            due_lead_minutes: lead(d, "due_lead_minutes"),
        },
    }
}

fn board(case: &Value) -> BoardData {
    BoardData::from_canonical_bytes(case["board_text"].as_str().expect("board_text").as_bytes()).expect("a board")
}

// Every case: the sources of §14.5 whose fire_at is in [from, until), in
// ascending (fire_at, key), each as its canonical record.
// spec: CORE-REM-001
// spec: CORE-REM-002
#[test]
fn every_case_of_reminders_json() {
    let family: Value = serde_json::from_str(REMINDERS).expect("vector file is JSON");
    assert_eq!(family["spec"].as_str(), Some("core.md §14.5"));
    let cases = family["cases"].as_array().expect("cases");
    assert!(cases.len() >= 8);
    for case in cases {
        let name = case["name"].as_str().expect("name");
        let b = board(case);
        let got = reminders(&b, instant(&case["from"]), instant(&case["until"]), &defaults(case));
        let texts: Vec<String> =
            got.iter().map(|r| String::from_utf8(r.to_canonical()).expect("UTF-8")).collect();
        let expected: Vec<&str> =
            case["reminders_text"].as_array().expect("reminders_text").iter().map(|t| t.as_str().expect("text")).collect();
        assert_eq!(texts, expected, "{name}");
        for (r, text) in got.iter().zip(&texts) {
            // A canonical record, its fields the reminder's.
            let v = canon::parse(text.as_bytes(), 1 << 20).expect("canonical");
            assert_eq!(v["key"].as_str(), Some(r.key.as_str()), "{name}");
            assert_eq!(v["item"].as_u64(), Some(r.item), "{name}");
            assert_eq!(v.get("sid").and_then(Value::as_u64), r.sid, "{name}");
            assert_eq!(r.sid.is_some(), r.kind == ReminderKind::Block, "{name}");
        }
    }
}

// The default leads: events 10 minutes before, blocks and due times on time.
// spec: CORE-REM-001
#[test]
fn the_default_leads_are_10_0_and_0() {
    let d = ReminderDefaults::default();
    assert_eq!((d.event_lead_minutes, d.block_lead_minutes, d.due_lead_minutes), (10, 0, 0));
}

// The key is `<kind>:<item>:<sid or 0>:<at>`; the same source gives the same
// key whatever the window or the leads, so a recomputed reminder is
// recognised.
// spec: CORE-REM-002
#[test]
fn a_recomputed_reminder_has_the_same_key() {
    let family: Value = serde_json::from_str(REMINDERS).expect("vector file is JSON");
    let case = &family["cases"][0];
    let b = board(case);
    let day = reminders(&b, instant(&case["from"]), instant(&case["until"]), &ReminderDefaults::default());
    let keys: Vec<&str> = day.iter().map(|r| r.key.as_str()).collect();
    assert_eq!(
        keys,
        [
            "block:1:2:2026-03-10T08:00:00.000000000Z",
            "block:1:1:2026-03-10T09:00:00.000000000Z",
            "event:2:0:2026-03-10T10:00:00.000000000Z",
            "event:5:0:2026-03-10T12:00:00.000000000Z",
            "due:1:0:2026-03-10T17:00:00.000000000Z",
        ]
    );
    let early = ReminderDefaults { event_lead_minutes: 60, block_lead_minutes: 60, due_lead_minutes: 60 };
    let again = reminders(&b, Instant::MIN, Instant::MAX, &early);
    let again_keys: Vec<&str> = again.iter().map(|r| r.key.as_str()).collect();
    for key in keys {
        assert!(again_keys.contains(&key), "{key}");
    }
    for (r, s) in day.iter().zip(reminders(&b, instant(&case["from"]), instant(&case["until"]), &ReminderDefaults::default())) {
        assert_eq!(r, &s);
    }
}
