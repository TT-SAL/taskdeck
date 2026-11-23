//! CORE-CMD-006: `apply` keeps every value of a command as given. A name with
//! leading or trailing White_Space (spaces, a tab, U+00A0, U+3000) is stored
//! byte for byte by every command that carries one, and a start or a length
//! off the quarter-hour grid is stored as given, not snapped. The values here
//! are the commands' own, so the expected board values are the command's text.

use serde_json::Value;
use taskdeck_core::apply::{Ctx, apply};
use taskdeck_core::board::BoardData;
use taskdeck_core::command::parse_command;
use taskdeck_core::time::Instant;

const CREATE: &str = include_str!("vectors/golden/rules-1/create.json");

/// The start board of the golden `create.json` cases: device 1 may use ids
/// from 1099511627787 on, and subscription 1099511627783 exists.
fn board() -> BoardData {
    let v: Value = serde_json::from_str(CREATE).expect("JSON");
    let text = v["cases"][0]["start_text"].as_str().expect("start_text");
    BoardData::from_canonical_bytes(text.as_bytes()).expect("the start board loads")
}

fn ctx(seq: u64) -> Ctx {
    Ctx { seq, device_no: 1, at: Instant::from_at_ms(1_769_904_000_000).expect("at"), based_on: 4 }
}

/// Applies one command given as a JSON value (encoded by serde_json, then
/// required to be canonical) and asserts it applied.
fn run(b: &mut BoardData, seq: u64, cmd: &Value) {
    let text = serde_json::to_string(cmd).expect("encode");
    let c = parse_command(text.as_bytes()).unwrap_or_else(|e| panic!("{text} parses: {e:?}"));
    let a = apply(b, &c, &ctx(seq)).unwrap_or_else(|r| panic!("{text} applies: {r:?}"));
    assert!(!a.benign, "{text} is not benign");
}

const NAMES: [&str; 5] = ["  padded  ", "\ttabbed\t", " \t mixed \t ", "\u{a0}nbsp\u{a0}", "\u{3000}ideo\u{3000}"];

fn item_name(b: &BoardData, id: u64) -> &str {
    &b.items.iter().find(|i| i.id == id).expect("the item is live").name
}

fn sub_name(b: &BoardData, id: u64) -> &str {
    &b.subscriptions.iter().find(|s| s.id == id).expect("the subscription exists").name
}

// spec: CORE-CMD-006
#[test]
fn rename_keeps_the_name_untrimmed() {
    for name in NAMES {
        let mut b = board();
        run(&mut b, 10, &serde_json::json!({"id": 1, "name": name, "op": "rename"}));
        assert_eq!(item_name(&b, 1).as_bytes(), name.as_bytes(), "rename to {name:?}");
        // And on the canonical round trip.
        let back = BoardData::from_canonical_bytes(&b.to_canonical_bytes()).expect("reloads");
        assert_eq!(item_name(&back, 1), name);
    }
}

// spec: CORE-CMD-006
#[test]
fn create_commands_keep_the_name_untrimmed() {
    for name in NAMES {
        let mut b = board();
        run(&mut b, 10, &serde_json::json!({"id": 1_099_511_627_787_u64, "name": name, "op": "create_task"}));
        run(
            &mut b,
            11,
            &serde_json::json!({"id": 1_099_511_627_788_u64, "name": name, "op": "create_event",
                "start": "2026-02-04T10:00:00.000000000Z"}),
        );
        run(
            &mut b,
            12,
            &serde_json::json!({"anchor": "2026-02-02", "days": 1, "id": 1_099_511_627_789_u64, "minutes": 60,
                "name": name, "op": "create_routine", "start_minutes": 60}),
        );
        for id in 1_099_511_627_787..=1_099_511_627_789 {
            assert_eq!(item_name(&b, id).as_bytes(), name.as_bytes(), "item {id}, name {name:?}");
        }
    }
}

// spec: CORE-CMD-006
#[test]
fn subscription_commands_keep_the_name_untrimmed() {
    for name in NAMES {
        let mut b = board();
        run(
            &mut b,
            10,
            &serde_json::json!({"color": [0, 0, 0, 0], "id": 1_099_511_627_787_u64, "name": name,
                "op": "add_subscription", "url": "https://example.com/z"}),
        );
        assert_eq!(sub_name(&b, 1_099_511_627_787).as_bytes(), name.as_bytes(), "add_subscription {name:?}");
        run(&mut b, 11, &serde_json::json!({"id": 1_099_511_627_783_u64, "name": name, "op": "rename_subscription"}));
        assert_eq!(sub_name(&b, 1_099_511_627_783).as_bytes(), name.as_bytes(), "rename_subscription {name:?}");
    }
}

// spec: CORE-CMD-006
#[test]
fn starts_and_lengths_off_the_grid_are_not_snapped() {
    let mut b = board();
    let start = "2026-02-04T10:07:13.500000000Z";
    run(
        &mut b,
        10,
        &serde_json::json!({"id": 1_099_511_627_787_u64, "minutes": 22, "name": "e", "op": "create_event", "start": start}),
    );
    let e = b.items.iter().find(|i| i.id == 1_099_511_627_787).expect("the event");
    assert_eq!(e.deadline.map(|d| d.to_canonical()), Some(start.to_owned()));
    assert_eq!(e.duration_minutes, Some(22));
    run(
        &mut b,
        11,
        &serde_json::json!({"id": 1, "minutes": 22, "op": "add_block", "sid": 1_099_511_627_788_u64, "start": start}),
    );
    let blk = b.items.iter().find(|i| i.id == 1).expect("item 1").blocks.iter().find(|k| k.sid == 1_099_511_627_788);
    let blk = blk.expect("the block");
    assert_eq!((blk.start.to_canonical(), blk.minutes), (start.to_owned(), 22));
}
