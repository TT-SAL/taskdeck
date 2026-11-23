//! Log commands v1 (core.md §9): the `commands.json` vector family (§18.2),
//! the field mapping of a few commands, and the property tests of the parser.

use proptest::prelude::*;
use serde_json::Value;
use taskdeck_core::board::PictureRef;
use taskdeck_core::command::{BlockMove, CreateEvent, CreateRoutine, CreateTask, LogCommand, NewBlock, parse_command};
use taskdeck_core::time::Instant;

const COMMANDS: &str = include_str!("vectors/commands.json");

const MAX_ID: u64 = (1 << 53) - 1;

const OPS: [&str; 29] = [
    "create_task",
    "create_event",
    "create_routine",
    "rename",
    "complete",
    "delete",
    "forget",
    "move_block",
    "move_blocks",
    "add_block",
    "remove_blocks",
    "set_estimate",
    "set_deadline",
    "set_severity",
    "set_horizon",
    "set_repeat",
    "move_routine",
    "set_event_span",
    "set_notes",
    "restore",
    "forget_archived",
    "add_subscription",
    "remove_subscription",
    "rename_subscription",
    "set_subscription_color",
    "set_subscription_enabled",
    "set_palette",
    "set_home",
    "set_picture",
];

fn unhex(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2));
    (0..s.len()).step_by(2).map(|k| u8::from_str_radix(&s[k..k + 2], 16).expect("hex")).collect()
}

/// The input bytes of a case: `command_hex`, or `command_text` with each
/// `repeat` marker replaced by `count` copies of `unit` joined by `sep`, where
/// `#I#` in the unit is `first`, `first + 1`, ….
fn case_bytes(case: &Value) -> Vec<u8> {
    if let Some(h) = case.get("command_hex") {
        return unhex(h.as_str().expect("command_hex"));
    }
    let mut text = case["command_text"].as_str().expect("command_text").to_owned();
    if let Some(reps) = case.get("repeat") {
        for r in reps.as_array().expect("repeat") {
            let unit = r["unit"].as_str().expect("unit");
            let sep = r["sep"].as_str().expect("sep");
            let first = r.get("first").map_or(0, |f| f.as_u64().expect("first"));
            let count = r["count"].as_u64().expect("count");
            let mut fill = String::new();
            for k in 0..count {
                if k > 0 {
                    fill.push_str(sep);
                }
                fill.push_str(&unit.replace("#I#", &(first + k).to_string()));
            }
            let marker = r["marker"].as_str().expect("marker");
            assert_eq!(text.matches(marker).count(), 1, "one marker {marker}");
            text = text.replace(marker, &fill);
        }
    }
    text.into_bytes()
}

// spec: CORE-CMD-001, CORE-CMD-002, CORE-CMD-003
#[test]
fn commands_json_every_case() {
    let doc: Value = serde_json::from_str(COMMANDS).expect("vector file is JSON");
    assert_eq!(doc["spec"].as_str(), Some("core.md §9"));
    let cases = doc["cases"].as_array().expect("cases");
    let mut valid_per_op = std::collections::BTreeMap::<String, usize>::new();
    let mut invalid_per_op = std::collections::BTreeMap::<String, usize>::new();
    let mut codes = std::collections::BTreeSet::<String>::new();
    for case in cases {
        let name = case["name"].as_str().expect("name");
        let bytes = case_bytes(case);
        if let Some(n) = case.get("length") {
            assert_eq!(bytes.len() as u64, n.as_u64().expect("length"), "{name}: length");
        }
        let got = parse_command(&bytes);
        match case.get("code") {
            Some(code) => {
                let code = code.as_str().expect("code");
                assert_eq!(got.as_ref().err().map(|e| e.code), Some(code), "{name}");
                codes.insert(code.to_owned());
                if let Some(op) = name.split(':').next().filter(|op| OPS.contains(op)) {
                    *invalid_per_op.entry(op.to_owned()).or_default() += 1;
                }
            }
            None => {
                let c = got.unwrap_or_else(|e| panic!("{name}: {e:?}"));
                let op = case["op"].as_str().expect("op");
                assert_eq!(c.op(), op, "{name}");
                assert_eq!(c.target(), case.get("target").map(|t| t.as_u64().expect("target")), "{name}: target");
                assert_eq!(c.to_canonical(), bytes, "{name}: to_canonical");
                *valid_per_op.entry(op.to_owned()).or_default() += 1;
            }
        }
    }
    for op in OPS {
        assert!(valid_per_op.get(op).copied().unwrap_or(0) >= 2, "{op}: valid cases");
        assert!(invalid_per_op.get(op).copied().unwrap_or(0) >= 2, "{op}: invalid cases");
    }
    for code in ["core/too_large", "core/not_json", "core/schema", "core/unknown_op", "core/not_canonical"] {
        assert!(codes.contains(code), "a case with {code}");
    }
}

fn t(s: &str) -> Instant {
    Instant::parse_canonical(s).expect("instant")
}

fn parse(s: &str) -> LogCommand {
    parse_command(s.as_bytes()).unwrap_or_else(|e| panic!("{s}: {e:?}"))
}

// spec: CORE-CMD-002
#[test]
fn fields_land_where_the_schema_says() {
    let a = "2026-09-04T09:00:00.000000000Z";
    let b = "2026-09-05T10:30:00.500000000Z";
    assert_eq!(
        parse(&format!(
            r#"{{"blocks":[{{"minutes":15,"sid":3,"start":"{a}"}},{{"minutes":1440,"sid":9,"start":"{b}"}}],"deadline":"{b}","duration_minutes":60,"id":7,"importance":4,"name":"n","op":"create_task","time_importance":1}}"#
        )),
        LogCommand::CreateTask(CreateTask {
            id: 7,
            name: "n".into(),
            deadline: Some(t(b)),
            importance: Some(4),
            time_importance: Some(1),
            duration_minutes: Some(60),
            blocks: vec![NewBlock { sid: 3, start: t(a), minutes: 15 }, NewBlock { sid: 9, start: t(b), minutes: 1440 }],
        })
    );
    // `RemoveBlocks` holds `(id, sid)` pairs, in that order.
    assert_eq!(
        parse(r#"{"blocks":[{"id":70,"sid":1},{"id":60,"sid":2}],"op":"remove_blocks"}"#),
        LogCommand::RemoveBlocks { blocks: vec![(70, 1), (60, 2)] }
    );
    assert_eq!(
        parse(&format!(r#"{{"moves":[{{"id":5,"minutes":0,"sid":9,"start":"{a}"}},{{"id":6,"minutes":30,"sid":2,"start":"{b}"}}],"op":"move_blocks"}}"#)),
        LogCommand::MoveBlocks {
            moves: vec![BlockMove { id: 5, sid: 9, start: t(a), minutes: 0 }, BlockMove { id: 6, sid: 2, start: t(b), minutes: 30 }]
        }
    );
    assert_eq!(parse(r#"{"op":"set_notes"}"#), LogCommand::SetNotes { text: String::new(), base: 0 });
    assert_eq!(parse(r#"{"base":4,"op":"set_notes","text":"x"}"#), LogCommand::SetNotes { text: "x".into(), base: 4 });
    assert_eq!(parse(r#"{"id":5,"op":"set_deadline"}"#), LogCommand::SetDeadline { id: 5, deadline: None, seed: None });
    assert_eq!(
        parse(&format!(r#"{{"deadline":"{a}","id":5,"op":"set_deadline","seed":3}}"#)),
        LogCommand::SetDeadline { id: 5, deadline: Some(t(a)), seed: Some(3) }
    );
    assert_eq!(parse(r#"{"lat":-12,"lon":345,"op":"set_home"}"#), LogCommand::SetHome { lat: -12, lon: 345 });
    assert_eq!(
        parse(&format!(r#"{{"archived_at":"{a}","fallback":9,"id":0,"op":"restore"}}"#)),
        LogCommand::Restore { id: 0, archived_at: t(a), fallback: 9 }
    );
    assert_eq!(
        parse(r#"{"anchor":"2026-09-07","days":5,"id":1,"minutes":30,"name":"r","op":"create_routine","start_minutes":600}"#),
        LogCommand::CreateRoutine(CreateRoutine {
            id: 1,
            name: "r".into(),
            days: 5,
            anchor: chrono::NaiveDate::from_ymd_opt(2026, 9, 7).expect("date"),
            start_minutes: 600,
            minutes: 30,
        })
    );
    assert_eq!(
        parse(r#"{"color":[1,2,3,4],"id":8,"name":"s","op":"add_subscription","url":"https://x.test/a"}"#),
        LogCommand::AddSubscription { id: 8, name: "s".into(), url: "https://x.test/a".into(), color: [1, 2, 3, 4] }
    );
    assert_eq!(
        parse(&format!(r#"{{"op":"set_picture","picture":{{"blob":"{}","epoch":3,"parts":2,"sha256":"{}","size":1048577}}}}"#, "0f".repeat(16), "a0".repeat(32))),
        LogCommand::SetPicture { picture: Some(PictureRef { blob: [15; 16], epoch: 3, parts: 2, sha256: [160; 32], size: 1_048_577 }) }
    );
}

#[test]
fn with_name_replaces_the_name_of_a_create_only() {
    let at = t("2026-09-04T09:00:00.000000000Z");
    let ev = LogCommand::CreateEvent(CreateEvent {
        id: 3,
        name: "a".into(),
        start: at,
        minutes: Some(30),
        importance: None,
        time_importance: Some(2),
    });
    let renamed = ev.clone().with_name("b".into()).expect("a create");
    match &renamed {
        LogCommand::CreateEvent(e) => {
            assert_eq!(e.name, "b");
            assert_eq!((e.id, e.start, e.minutes, e.time_importance), (3, at, Some(30), Some(2)));
        }
        other => panic!("{other:?}"),
    }
    let task = LogCommand::CreateTask(CreateTask {
        id: 4,
        name: "a".into(),
        deadline: None,
        importance: None,
        time_importance: None,
        duration_minutes: None,
        blocks: vec![],
    });
    assert!(matches!(task.with_name("c".into()), Ok(LogCommand::CreateTask(CreateTask { ref name, id: 4, .. })) if name == "c"));
    let routine = parse(r#"{"anchor":"2026-09-07","days":5,"id":1,"minutes":30,"name":"r","op":"create_routine","start_minutes":600}"#);
    assert!(matches!(routine.with_name("z".into()), Ok(LogCommand::CreateRoutine(CreateRoutine { ref name, .. })) if name == "z"));
    let rename = LogCommand::Rename { id: 3, name: "x".into() };
    assert_eq!(rename.clone().with_name("y".into()), Err(rename));
    let notes = LogCommand::SetNotes { text: "x".into(), base: 1 };
    assert_eq!(notes.clone().with_name("y".into()), Err(notes));
}

#[test]
fn target_is_the_id_field() {
    assert_eq!(parse(r#"{"id":5,"op":"forget"}"#).target(), Some(5));
    assert_eq!(parse(r#"{"id":11,"op":"remove_subscription"}"#).target(), Some(11));
    assert_eq!(parse(r#"{"op":"set_notes"}"#).target(), None);
    assert_eq!(parse(r#"{"blocks":[{"id":70,"sid":1}],"op":"remove_blocks"}"#).target(), None);
    assert_eq!(parse(r#"{"lat":0,"lon":0,"op":"set_home"}"#).target(), None);
}

// ---------------------------------------------------------------- property tests

fn instant() -> impl Strategy<Value = Instant> {
    (0i64..=253_402_300_799, 0u32..1_000_000_000).prop_map(|(s, n)| Instant::from_parts(s, n).expect("in range"))
}

fn id() -> impl Strategy<Value = u64> {
    prop_oneof![0..=MAX_ID, 0u64..100, Just(MAX_ID)]
}

fn len() -> impl Strategy<Value = u32> {
    15u32..=1440
}

fn text(lo: usize, hi: usize) -> impl Strategy<Value = String> {
    proptest::collection::vec(any::<char>(), lo..=hi).prop_map(String::from_iter)
}

fn name() -> impl Strategy<Value = String> {
    prop_oneof![text(1, 200), "[a-z ]{1,20}"]
}

fn date() -> impl Strategy<Value = chrono::NaiveDate> {
    (1970i32..=9999, 1u32..=365).prop_map(|(y, o)| chrono::NaiveDate::from_yo_opt(y, o).expect("date"))
}

fn routine_span() -> impl Strategy<Value = (i32, u32)> {
    (0i32..=1425).prop_flat_map(|s| (Just(s), 15u32..=u32::try_from(1440 - s).expect("span")))
}

fn level(hi: u8) -> impl Strategy<Value = u8> {
    0..=hi
}

fn picture() -> impl Strategy<Value = PictureRef> {
    (any::<[u8; 16]>(), 1u32..=u32::MAX, any::<[u8; 32]>(), 67u64..=67_108_864).prop_map(|(blob, epoch, sha256, size)| PictureRef {
        blob,
        epoch,
        parts: u32::try_from(size.div_ceil(1_048_576)).expect("parts"),
        sha256,
        size,
    })
}

/// One generated command of each of the 29 ops, values in range.
fn command() -> BoxedStrategy<LogCommand> {
    let arms: Vec<BoxedStrategy<LogCommand>> = vec![
        (
            id(),
            name(),
            proptest::option::of(instant()),
            proptest::option::of(level(4)),
            proptest::option::of(level(3)),
            proptest::option::of(len()),
            proptest::collection::vec((id(), instant(), len()), 0..12),
        )
            .prop_map(|(id, name, deadline, importance, time_importance, duration_minutes, raw)| {
                let mut blocks: Vec<NewBlock> = raw.into_iter().map(|(sid, start, minutes)| NewBlock { sid, start, minutes }).collect();
                blocks.sort_by_key(|b| b.sid);
                blocks.dedup_by_key(|b| b.sid);
                LogCommand::CreateTask(CreateTask { id, name, deadline, importance, time_importance, duration_minutes, blocks })
            })
            .boxed(),
        (id(), name(), instant(), proptest::option::of(len()), proptest::option::of(level(4)), proptest::option::of(level(3)))
            .prop_map(|(id, name, start, minutes, importance, time_importance)| {
                LogCommand::CreateEvent(CreateEvent { id, name, start, minutes, importance, time_importance })
            })
            .boxed(),
        (id(), name(), 0u8..=127, date(), routine_span())
            .prop_map(|(id, name, days, anchor, (start_minutes, minutes))| {
                LogCommand::CreateRoutine(CreateRoutine { id, name, days, anchor, start_minutes, minutes })
            })
            .boxed(),
        (id(), name()).prop_map(|(id, name)| LogCommand::Rename { id, name }).boxed(),
        (id(), instant()).prop_map(|(id, at)| LogCommand::Complete { id, at }).boxed(),
        (id(), instant()).prop_map(|(id, at)| LogCommand::Delete { id, at }).boxed(),
        id().prop_map(|id| LogCommand::Forget { id }).boxed(),
        (id(), id(), instant(), len()).prop_map(|(id, sid, start, minutes)| LogCommand::MoveBlock { id, sid, start, minutes }).boxed(),
        proptest::collection::vec((id(), id(), instant(), 0u32..=1440), 1..12)
            .prop_map(|raw| {
                let mut moves: Vec<BlockMove> = Vec::new();
                for (id, sid, start, minutes) in raw {
                    if moves.iter().all(|m| m.sid != sid) {
                        moves.push(BlockMove { id, sid, start, minutes });
                    }
                }
                LogCommand::MoveBlocks { moves }
            })
            .boxed(),
        (id(), id(), instant(), len()).prop_map(|(id, sid, start, minutes)| LogCommand::AddBlock { id, sid, start, minutes }).boxed(),
        proptest::collection::vec((id(), id()), 1..12)
            .prop_map(|mut blocks| {
                blocks.sort_by_key(|p| p.1);
                blocks.dedup_by_key(|p| p.1);
                LogCommand::RemoveBlocks { blocks }
            })
            .boxed(),
        (id(), len()).prop_map(|(id, minutes)| LogCommand::SetEstimate { id, minutes }).boxed(),
        (id(), proptest::option::of(instant()), proptest::option::of(level(4)))
            .prop_map(|(id, deadline, seed)| LogCommand::SetDeadline { id, deadline, seed })
            .boxed(),
        (id(), level(4)).prop_map(|(id, level)| LogCommand::SetSeverity { id, level }).boxed(),
        (id(), level(3)).prop_map(|(id, level)| LogCommand::SetHorizon { id, level }).boxed(),
        (id(), 0u8..=127, date()).prop_map(|(id, days, anchor)| LogCommand::SetRepeat { id, days, anchor }).boxed(),
        (id(), routine_span())
            .prop_map(|(id, (start_minutes, minutes))| LogCommand::MoveRoutine { id, start_minutes, minutes })
            .boxed(),
        (id(), instant(), len()).prop_map(|(id, start, minutes)| LogCommand::SetEventSpan { id, start, minutes }).boxed(),
        (text(0, 300), prop_oneof![Just(0u64), 0..=MAX_ID])
            .prop_map(|(text, base)| LogCommand::SetNotes { text: text.replace('\t', " "), base })
            .boxed(),
        (id(), instant(), id()).prop_map(|(id, archived_at, fallback)| LogCommand::Restore { id, archived_at, fallback }).boxed(),
        (id(), instant()).prop_map(|(id, archived_at)| LogCommand::ForgetArchived { id, archived_at }).boxed(),
        (id(), name(), text(1, 300), any::<[u8; 4]>())
            .prop_map(|(id, name, rest, color)| LogCommand::AddSubscription { id, name, url: format!("https://{rest}"), color })
            .boxed(),
        id().prop_map(|id| LogCommand::RemoveSubscription { id }).boxed(),
        (id(), name()).prop_map(|(id, name)| LogCommand::RenameSubscription { id, name }).boxed(),
        (id(), any::<[u8; 4]>()).prop_map(|(id, color)| LogCommand::SetSubscriptionColor { id, color }).boxed(),
        (id(), any::<bool>()).prop_map(|(id, enabled)| LogCommand::SetSubscriptionEnabled { id, enabled }).boxed(),
        any::<[[u8; 4]; 6]>().prop_map(|colors| LogCommand::SetPalette { colors }).boxed(),
        (-9000i32..=9000, -18000i32..=18000).prop_map(|(lat, lon)| LogCommand::SetHome { lat, lon }).boxed(),
        proptest::option::of(picture()).prop_map(|picture| LogCommand::SetPicture { picture }).boxed(),
    ];
    assert_eq!(arms.len(), 29);
    proptest::strategy::Union::new(arms).boxed()
}

/// Bytes that look like commands: JSON punctuation, digits, letters of the op
/// names and fields, quotes and escapes.
fn jsonish() -> impl Strategy<Value = Vec<u8>> {
    let alphabet: Vec<u8> = br#"{}[]":,.-+0123456789eE \tnulltruefalseopidnamecreate_taskblocksminutessidstart\u00"#.to_vec();
    proptest::collection::vec(proptest::sample::select(alphabet), 0..200)
}

proptest! {
    // spec: CORE-CMD-002
    #[test]
    fn canonical_bytes_parse_back_to_the_command(c in command()) {
        let bytes = c.to_canonical();
        prop_assert!(bytes.len() <= 257_979);
        prop_assert_eq!(parse_command(&bytes), Ok(c.clone()));
        prop_assert!(OPS.contains(&c.op()));
    }

    // spec: CORE-GEN-007
    #[test]
    fn random_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..400)) {
        if let Ok(c) = parse_command(&bytes) {
            prop_assert_eq!(c.to_canonical(), bytes);
        }
    }

    // spec: CORE-GEN-007
    #[test]
    fn jsonish_bytes_never_panic(bytes in jsonish()) {
        if let Ok(c) = parse_command(&bytes) {
            prop_assert_eq!(c.to_canonical(), bytes);
        }
    }

    // spec: CORE-GEN-007, CORE-CMD-001
    // A mutated command either fails or is the canonical encoding of what it
    // parses to (step 6 makes the bytes unique).
    #[test]
    fn mutated_commands_never_panic(
        c in command(),
        edits in proptest::collection::vec((any::<prop::sample::Index>(), 0u8..4, any::<u8>()), 1..4),
    ) {
        let mut bytes = c.to_canonical();
        for (at, kind, b) in edits {
            let i = at.index(bytes.len() + 1);
            match kind {
                0 => bytes.insert(i, b),
                1 if i < bytes.len() => { bytes.remove(i); }
                2 if i < bytes.len() => bytes[i] = b,
                _ => bytes.truncate(i),
            }
        }
        if let Ok(p) = parse_command(&bytes) {
            prop_assert_eq!(p.to_canonical(), bytes);
        }
    }
}

// spec: CORE-GEN-009
// The length is checked before anything else: an input one byte over the
// limit is `core/too_large` whatever it holds, and one at the limit is not.
#[test]
fn the_length_is_checked_first() {
    for fill in [b'{', b'[', b'"', b' ', 0xff] {
        let over = vec![fill; 257_980];
        assert_eq!(parse_command(&over).map_err(|e| e.code), Err("core/too_large"));
        let at = vec![fill; 257_979];
        assert_ne!(parse_command(&at).map_err(|e| e.code), Err("core/too_large"));
    }
}
