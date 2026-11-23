//! `parse_intent` (core.md §12.2): the length step, the naive time carried,
//! `session` a sid, today's messages. The vectors are `tests/vectors/intents.json`.

use super::*;

/// A `set_notes` intent of exactly `len` bytes.
fn notes_of_len(len: usize) -> String {
    let frame = r#"{"op":"set_notes","text":""}"#;
    format!(r#"{{"op":"set_notes","text":"{}"}}"#, "a".repeat(len - frame.len()))
}

// spec: CORE-INT-001
#[test]
fn the_length_is_checked_first() {
    let at_limit = notes_of_len(INTENT_LIMIT);
    assert_eq!(at_limit.len(), 84_096);
    assert!(matches!(parse_intent(&at_limit), Ok(Intent::SetNotes { .. })));
    let over = notes_of_len(INTENT_LIMIT + 1);
    let e = parse_intent(&over).expect_err("too large");
    assert_eq!((e.code, e.status, e.message.as_str()), ("core/intent_json", 400, "That request is too large."));
    // Over the limit is refused even when it is not JSON at all.
    let e = parse_intent(&"x".repeat(INTENT_LIMIT + 1)).expect_err("too large");
    assert_eq!(e.message, "That request is too large.");
}

// spec: CORE-INT-002
#[test]
fn a_naive_time_is_carried_unresolved() {
    let got = parse_intent(r#"{"op":"set_deadline","id":4,"deadline":" 2026-03-29T02:30 "}"#).expect("parses");
    let naive = chrono::NaiveDate::from_ymd_opt(2026, 3, 29).and_then(|d| d.and_hms_opt(2, 30, 0)).expect("a time");
    assert_eq!(got, Intent::SetDeadline { id: 4, deadline: Some(WireTime::Naive(naive, "2026-03-29T02:30".to_owned())) });
    // An offset makes it exact; nothing is converted here.
    let got = parse_intent(r#"{"op":"complete","id":4,"at":"2026-03-29T02:30:00+05:00"}"#).expect("parses");
    let exact = chrono::DateTime::parse_from_rfc3339("2026-03-29T02:30:00+05:00").expect("rfc 3339");
    assert_eq!(got, Intent::Complete { id: 4, at: Some(WireTime::Exact(exact)) });
}

// spec: CORE-INT-003
#[test]
fn session_is_a_sid() {
    let sid = (5u64 << 40) + 3;
    let got = parse_intent(&format!(r#"{{"op":"remove_block","id":1,"session":{sid}}}"#)).expect("parses");
    assert_eq!(got, Intent::RemoveBlock { id: 1, session: sid });
    let got = parse_intent(r#"{"op":"move_block","id":1,"session":1,"day":"2026-03-10","start":0,"minutes":15}"#)
        .expect("parses");
    assert!(matches!(got, Intent::MoveBlock { session: Some(1), .. }));
}

// spec: CORE-INT-001
#[test]
fn failures_carry_the_parsers_message() {
    let e = parse_intent(r#"{"op":"explode"}"#).expect_err("unknown");
    assert_eq!((e.code, e.status), ("core/intent_json", 400));
    assert!(e.message.starts_with("unknown variant `explode`, expected one of `snapshot`, `feed`, `board`, `add`"), "{}", e.message);
    let e = parse_intent(r#"{"op":"rename","id":1}"#).expect_err("missing");
    assert!(e.message.starts_with("missing field `name`"), "{}", e.message);
    let e = parse_intent(r#"{"op":"quick_add","name":"a","deadline":"friday"}"#).expect_err("bad time");
    assert_eq!(e.message, "not a time: `friday`");
}
