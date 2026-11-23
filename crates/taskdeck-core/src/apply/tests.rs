use super::*;
use crate::board::{Home, Mark, PictureRef, Subscription};
use crate::command::{BlockMove, NewBlock};

const D1: u64 = 1 << 40;
const D2: u64 = 2 << 40;

fn t(secs: i64) -> Instant {
    Instant::from_parts(secs, 0).expect("in range")
}

fn st(seq: u64, device_no: u16) -> Stamp {
    Stamp { seq, device_no }
}

fn task(id: u64, s: Stamp, sids: &[u64]) -> Item {
    Item {
        id,
        kind: Kind::Task,
        name: "secret name".to_owned(),
        created: t(100),
        deadline: None,
        importance: None,
        time_importance: None,
        duration_minutes: None,
        recurrence: None,
        blocks: sids.iter().map(|&sid| Block { sid, start: t(1000), minutes: 30, stamp: s }).collect(),
        stamps: all_stamps(s),
    }
}

fn row(id: u64, at: i64) -> ArchiveRow {
    ArchiveRow {
        id,
        kind: Kind::Task,
        name: "r".to_owned(),
        created: t(1),
        deadline: None,
        importance: None,
        time_importance: None,
        duration_minutes: None,
        recurrence: None,
        blocks: Vec::new(),
        archived_at: t(at),
        outcome: Outcome::Finished,
    }
}

/// Device 1's task `D1 + 1` with block `D1 + 1`, device 2's task `D2 + 1`
/// (stamps `(5, 2)`) with block `D2 + 1`, an event `D2 + 2`, one row and one
/// subscription.
fn board() -> BoardData {
    let mut b = BoardData::empty();
    let mut ev = task(D2 + 2, st(5, 2), &[]);
    ev.kind = Kind::Event;
    ev.deadline = Some(t(5000));
    b.items = vec![task(D1 + 1, st(6, 1), &[D1 + 1]), task(D2 + 1, st(5, 2), &[D2 + 1]), ev];
    b.archive = vec![row(D1 + 5, 50)];
    b.subscriptions =
        vec![Subscription { id: D1 + 7, name: "cal".to_owned(), url: "https://a.example/x".to_owned(), color: [1, 2, 3, 4], enabled: true }];
    b.notes_rev = 3;
    b.marks = vec![Mark { device_no: 1, item: 10, block: 10 }, Mark { device_no: 2, item: 10, block: 10 }];
    b.check().expect("the test board passes check");
    b
}

fn ctx() -> Ctx {
    Ctx { seq: 10, device_no: 1, at: t(1_000_000), based_on: 4 }
}

fn applied(b: &mut BoardData, c: &LogCommand) -> Applied {
    let a = apply(b, c, &ctx()).expect("applied");
    b.check().expect("check holds after apply");
    a
}

fn refused(b: &mut BoardData, c: &LogCommand, x: &Ctx) -> Refusal {
    let before = b.to_canonical_bytes();
    let r = apply(b, c, x).expect_err("refused");
    assert_eq!(b.to_canonical_bytes(), before, "a refusal changes nothing");
    r
}

// spec: CORE-CTX-001
// spec: CORE-APPLY-001
#[test]
fn a_context_outside_its_range_is_refused_first() {
    let mut b = board();
    // A command that would also fail its own check (gone) reports bad_ctx.
    let c = LogCommand::Forget { id: 99 };
    let bad = [
        Ctx { device_no: 0, ..ctx() },
        Ctx { device_no: 8192, ..ctx() },
        Ctx { seq: 1, based_on: 0, ..ctx() },
        Ctx { seq: 1 << 53, ..ctx() },
        Ctx { based_on: 10, ..ctx() },
        Ctx { based_on: 11, ..ctx() },
    ];
    for x in bad {
        assert_eq!(refused(&mut b, &c, &x).code, "core/bad_ctx", "{x:?}");
    }
    for x in [Ctx { device_no: 8191, ..ctx() }, Ctx { seq: 2, based_on: 1, ..ctx() }, Ctx { seq: (1 << 53) - 1, ..ctx() }] {
        assert!(apply(&mut b.clone(), &c, &x).is_ok(), "{x:?}");
    }
}

// spec: CORE-RULES-001
#[test]
fn apply_dispatches_on_the_rules_version() {
    let mut b = board();
    b.rules = 2;
    let r = refused(&mut b, &LogCommand::SetHome { lat: 1, lon: 1 }, &ctx());
    assert_eq!((r.code, r.op, r.target), ("core/rules", "set_home", None));
    // bad_ctx comes before the op table.
    assert_eq!(refused(&mut b, &LogCommand::SetHome { lat: 1, lon: 1 }, &Ctx { device_no: 0, ..ctx() }).code, "core/bad_ctx");
    // Every op of §9.3 is in version 1's table.
    assert_eq!(op_table(1).len(), 29);
    assert!(op_table(2).is_empty());
}

// spec: CORE-CMD-008
// spec: CORE-GEN-010
#[test]
fn a_refusal_names_op_and_target_and_carries_no_text() {
    let mut b = board();
    let r = refused(&mut b, &LogCommand::Rename { id: 77, name: "hidden words".to_owned() }, &ctx());
    assert_eq!(r, Refusal { code: "core/gone", op: "rename", target: Some(77), notes: None });
    assert!(!format!("{r:?}").contains("hidden"));
    let r = refused(&mut b, &LogCommand::SetNotes { text: "draft".to_owned(), base: 2 }, &ctx());
    assert_eq!(r, Refusal { code: "core/notes_stale", op: "set_notes", target: None, notes: Some(NotesConflict { base: 2, current: 3 }) });
    let a = applied(&mut b, &LogCommand::SetNotes { text: "draft".to_owned(), base: 3 });
    assert_eq!((b.notes.as_str(), b.notes_rev), ("draft", 4));
    assert!(a.changes.notes);
    // The second edit from the same view is stale; one from the new revision is not.
    assert_eq!(refused(&mut b, &LogCommand::SetNotes { text: "x".to_owned(), base: 3 }, &ctx()).code, "core/notes_stale");
    applied(&mut b, &LogCommand::SetNotes { text: String::new(), base: 4 });
    assert_eq!(b.notes_rev, 5);
}

// spec: CORE-APPLY-003
// spec: CORE-APPLY-004
#[test]
fn the_change_set_names_what_changed_and_nothing_else() {
    let mut b = board();
    let c = LogCommand::CreateTask(CreateTask {
        id: D1 + 11,
        name: "n".to_owned(),
        deadline: None,
        importance: None,
        time_importance: None,
        duration_minutes: None,
        blocks: vec![NewBlock { sid: D1 + 12, start: t(5), minutes: 15 }],
    });
    let a = applied(&mut b, &c);
    assert_eq!(a.changes, ChangeSet { items: [D1 + 11].into(), marks: true, ..ChangeSet::default() });
    assert_eq!(b.mark(1), Mark { device_no: 1, item: 11, block: 12 });

    let a = applied(&mut b, &LogCommand::Complete { id: D1 + 1, at: t(2_000_000) });
    // Clamped to a day after ctx.at (CORE-CMD-005).
    let key = ArchiveKey { id: D1 + 1, archived_at: t(1_000_000 + 86_400) };
    assert_eq!(a.changes, ChangeSet { items: [D1 + 1].into(), archive_inserted: vec![key], ..ChangeSet::default() });

    let a = applied(&mut b, &LogCommand::Restore { id: D1 + 1, archived_at: key.archived_at, fallback: D1 + 20 });
    assert_eq!(a.restored_as, Some(D1 + 1));
    assert_eq!(a.changes, ChangeSet { items: [D1 + 1].into(), archive_removed: vec![key], marks: true, ..ChangeSet::default() });
    assert_eq!(b.mark(1).item, 20, "the mark rises to the unused fallback (CORE-ID-003)");

    let a = applied(&mut b, &LogCommand::SetSubscriptionEnabled { id: D1 + 7, enabled: false });
    assert_eq!(a.changes, ChangeSet { subscriptions: [D1 + 7].into(), overlay_forget: [D1 + 7].into(), ..ChangeSet::default() });
    let a = applied(&mut b, &LogCommand::SetSubscriptionColor { id: D1 + 7, color: [1, 2, 3, 4] });
    assert_eq!(a.changes, ChangeSet::default(), "the same colour changes nothing");
    let a = applied(&mut b, &LogCommand::RenameSubscription { id: D1 + 7, name: "c2".to_owned() });
    assert_eq!(a.changes.subscriptions, [D1 + 7].into());
    let a = applied(&mut b, &LogCommand::RemoveSubscription { id: D1 + 7 });
    assert_eq!(a.changes, ChangeSet { subscriptions: [D1 + 7].into(), overlay_forget: [D1 + 7].into(), ..ChangeSet::default() });
    let a = applied(&mut b, &LogCommand::AddSubscription { id: D1 + 21, name: "n".to_owned(), url: "https://b.example".to_owned(), color: [0; 4] });
    assert_eq!(a.changes, ChangeSet { subscriptions: [D1 + 21].into(), marks: true, ..ChangeSet::default() });

    let a = applied(&mut b, &LogCommand::SetHome { lat: 1, lon: 2 });
    assert_eq!(a.changes, ChangeSet { settings: true, ..ChangeSet::default() });
    assert_eq!(b.home, Home { lat: 1, lon: 2 });
    let a = applied(&mut b, &LogCommand::SetHome { lat: 1, lon: 2 });
    assert_eq!(a.changes, ChangeSet::default());
    let p = PictureRef { blob: [1; 16], epoch: 1, parts: 1, sha256: [2; 32], size: 67 };
    assert!(applied(&mut b, &LogCommand::SetPicture { picture: Some(p) }).changes.settings);
    assert!(applied(&mut b, &LogCommand::SetPicture { picture: None }).changes.settings);
    assert!(applied(&mut b, &LogCommand::SetPalette { colors: [[9; 4]; 6] }).changes.settings);

    // Benign repeats: nothing changes and nothing is reported.
    for c in [
        LogCommand::Forget { id: 12345 },
        LogCommand::Complete { id: D1 + 5, at: t(9) },
        LogCommand::ForgetArchived { id: 3, archived_at: t(3) },
        LogCommand::RemoveBlocks { blocks: vec![(D2 + 1, 999)] },
        LogCommand::RemoveSubscription { id: D1 + 7 },
    ] {
        let before = b.to_canonical_bytes();
        let a = applied(&mut b, &c);
        assert_eq!(a, Applied { benign: true, ..Applied::default() }, "{c:?}");
        assert_eq!(b.to_canonical_bytes(), before);
    }
    let a = applied(&mut b, &LogCommand::ForgetArchived { id: D1 + 5, archived_at: t(50) });
    assert_eq!(a.changes.archive_removed, vec![ArchiveKey { id: D1 + 5, archived_at: t(50) }]);
}

// spec: CORE-CONF-001
// spec: CORE-CONF-002
// spec: CORE-CONF-003
#[test]
fn stamps_notices_and_skips() {
    let mut b = board();
    let a = applied(&mut b, &LogCommand::SetSeverity { id: D2 + 1, level: 4 });
    let n = Notice::Overwrite { seq: 10, author: 1, other: 2, other_seq: 5, item: D2 + 1, sid: None, group: Group::Levels };
    assert_eq!(a.notices, vec![n]);
    let item = b.item(D2 + 1).expect("live");
    assert_eq!(item.stamps.levels, st(10, 1));
    assert_eq!(item.stamps.name, st(5, 2), "only the named group is stamped");
    assert!(n.concerns(1) && n.concerns(2) && !n.concerns(3));

    let moves = vec![
        BlockMove { id: D2 + 1, sid: D2 + 1, start: t(7), minutes: 0 },
        BlockMove { id: D1 + 1, sid: D1 + 1, start: t(8), minutes: 20 },
        BlockMove { id: D1 + 1, sid: 4242, start: t(8), minutes: 20 },
    ];
    let a = applied(&mut b, &LogCommand::MoveBlocks { moves });
    assert_eq!((a.moved, a.skipped), (1, 2));
    let changed = Notice::Skipped { seq: 10, author: 1, item: D2 + 1, sid: D2 + 1, reason: SkipReason::Changed, other: Some((2, 5)) };
    let gone = Notice::Skipped { seq: 10, author: 1, item: D1 + 1, sid: 4242, reason: SkipReason::Gone, other: None };
    assert_eq!(a.notices, vec![changed, gone]);
    assert!(changed.concerns(1) && !changed.concerns(2));
    assert_eq!(a.changes.items, [D1 + 1].into());

    // A move naming an event is refused before anything moves.
    let moves = vec![BlockMove { id: D1 + 1, sid: D1 + 1, start: t(9), minutes: 20 }, BlockMove { id: D2 + 2, sid: 1, start: t(9), minutes: 20 }];
    assert_eq!(refused(&mut b, &LogCommand::MoveBlocks { moves }, &ctx()).code, "core/kind");

    // Within the origin's view (based_on = 5): no notice.
    let x = Ctx { based_on: 5, ..ctx() };
    let a = apply(&mut b, &LogCommand::Rename { id: D2 + 1, name: "z".to_owned() }, &x).expect("applied");
    assert!(a.notices.is_empty());
}

// spec: CORE-CONF-006
#[test]
fn notice_and_refusal_records_are_canonical() {
    let cases = [
        (
            Notice::Overwrite { seq: 9, author: 1, other: 2, other_seq: 5, item: 7, sid: Some(8), group: Group::Block },
            r#"{"author":1,"group":"block","item":7,"kind":"overwrite","other":2,"other_seq":5,"seq":9,"sid":8}"#,
        ),
        (
            Notice::Overwrite { seq: 9, author: 1, other: 2, other_seq: 5, item: 7, sid: None, group: Group::Name },
            r#"{"author":1,"group":"name","item":7,"kind":"overwrite","other":2,"other_seq":5,"seq":9}"#,
        ),
        (
            Notice::Skipped { seq: 9, author: 1, item: 7, sid: 8, reason: SkipReason::Gone, other: None },
            r#"{"author":1,"group":"block","item":7,"kind":"skipped","reason":"gone","seq":9,"sid":8}"#,
        ),
        (
            Notice::Skipped { seq: 9, author: 1, item: 7, sid: 8, reason: SkipReason::Changed, other: Some((3, 4)) },
            r#"{"author":1,"group":"block","item":7,"kind":"skipped","other":3,"other_seq":4,"reason":"changed","seq":9,"sid":8}"#,
        ),
    ];
    for (n, text) in cases {
        assert_eq!(String::from_utf8(n.to_canonical()).expect("utf-8"), text);
        assert_eq!(Notice::from_canonical(text.as_bytes()), Ok(n));
    }
    for g in ["name", "time", "levels", "estimate", "repeat", "item"] {
        let text = format!(r#"{{"author":1,"group":"{g}","item":7,"kind":"overwrite","other":2,"other_seq":5,"seq":9}}"#);
        assert!(Notice::from_canonical(text.as_bytes()).is_ok(), "{g}");
    }
    for bad in [
        r#"{"author":1,"group":"name","item":7,"kind":"overwrite","other":2,"seq":9}"#,
        r#"{"author":1,"group":"name","item":7,"kind":"overwrite","other":2,"other_seq":5,"seq":9,"sid":8}"#,
        r#"{"author":1,"group":"block","item":7,"kind":"overwrite","other":2,"other_seq":5,"seq":9}"#,
        r#"{"author":0,"group":"name","item":7,"kind":"overwrite","other":2,"other_seq":5,"seq":9}"#,
        r#"{"author":1,"group":"name","item":7,"kind":"overwrite","other":8192,"other_seq":5,"seq":9}"#,
        r#"{"author":1,"group":"name","item":7,"kind":"overwrite","other":2,"other_seq":5,"reason":"gone","seq":9}"#,
        r#"{"author":1,"group":"block","item":7,"kind":"skipped","seq":9,"sid":8}"#,
        r#"{"author":1,"group":"block","item":7,"kind":"skipped","reason":"changed","seq":9,"sid":8}"#,
        r#"{"author":1,"group":"block","item":7,"kind":"skipped","other":3,"other_seq":4,"reason":"gone","seq":9,"sid":8}"#,
        r#"{"author":1,"group":"time","item":7,"kind":"skipped","reason":"gone","seq":9}"#,
        r#"{"author":1,"group":"other","item":7,"kind":"overwrite","other":2,"other_seq":5,"seq":9}"#,
        r#"{"author":1,"group":"name","item":7,"kind":"refused","other":2,"other_seq":5,"seq":9}"#,
        r#"{"group":"name","item":7,"kind":"overwrite","other":2,"other_seq":5,"seq":9,"author":1}"#,
        r#"{"author":1,"group":"name","item":7,"kind":"overwrite","other":2,"other_seq":5,"seq":9,"name":"x"}"#,
    ] {
        assert!(Notice::from_canonical(bad.as_bytes()).is_err(), "{bad}");
    }

    let r = RefusalRecord {
        seq: 12,
        author: 3,
        code: "core/notes_stale".to_owned(),
        op: Some("set_notes".to_owned()),
        target: None,
        base: Some(1),
        current: Some(2),
    };
    let text = r#"{"author":3,"base":1,"code":"core/notes_stale","current":2,"op":"set_notes","seq":12}"#;
    assert_eq!(String::from_utf8(r.to_canonical()).expect("utf-8"), text);
    assert_eq!(RefusalRecord::from_canonical(text.as_bytes()), Ok(r));
    let r = RefusalRecord { seq: 12, author: 3, code: "core/gone".to_owned(), op: Some("rename".to_owned()), target: Some(5), base: None, current: None };
    let text = r#"{"author":3,"code":"core/gone","op":"rename","seq":12,"target":5}"#;
    assert_eq!(String::from_utf8(r.to_canonical()).expect("utf-8"), text);
    assert_eq!(RefusalRecord::from_canonical(text.as_bytes()), Ok(r));
    let text = r#"{"author":3,"code":"core/schema","seq":12}"#;
    assert!(RefusalRecord::from_canonical(text.as_bytes()).is_ok(), "op absent when the payload did not parse");
    for bad in [
        r#"{"author":3,"base":1,"code":"core/gone","op":"rename","seq":12}"#,
        r#"{"author":0,"code":"core/gone","seq":12}"#,
        r#"{"author":3,"code":"core/gone"}"#,
        r#"{"author":3, "code":"core/gone","seq":12}"#,
        r#"{"author":3,"code":"core/gone","seq":12,"url":"https://x"}"#,
    ] {
        assert!(RefusalRecord::from_canonical(bad.as_bytes()).is_err(), "{bad}");
    }
}

// spec: CORE-RULES-002
#[test]
fn upgrade_rules_steps() {
    let mut b = board();
    let before = b.clone();
    assert_eq!(upgrade_rules(&mut b, 1), Err(CoreError::code("core/unknown_rules")));
    assert_eq!(upgrade_rules(&mut b, 0), Err(CoreError::code("core/unknown_rules")));
    assert_eq!(upgrade_rules(&mut b, 2), Err(CoreError::code("core/unknown_rules")));
    assert_eq!(b, before, "an error leaves the board unchanged");
    // Step 3 on a board below version 1 (no such board passes check).
    b.rules = 0;
    assert_eq!(upgrade_rules(&mut b, 1), Ok(()));
    assert_eq!(b, before);
    assert_eq!(SUPPORTED_RULES, &[1]);
    assert_eq!(HASH_VERSIONS, &[1]);
}

// spec: CORE-RULES-005
#[test]
fn reset_keeps_the_target_but_marks_and_notes() {
    let target = board();
    let mut current = board();
    current.items.clear();
    current.marks = vec![Mark { device_no: 2, item: 3, block: 40 }, Mark { device_no: 5, item: 1, block: 0 }];
    current.notes_rev = 9;
    let r = reset(&target, &current).expect("reset");
    let mut want = target.clone();
    want.marks = vec![Mark { device_no: 1, item: 10, block: 10 }, Mark { device_no: 2, item: 10, block: 40 }, Mark { device_no: 5, item: 1, block: 0 }];
    want.notes_rev = 10;
    assert_eq!(r, want);
    let mut newer = target.clone();
    newer.rules = 2;
    assert_eq!(reset(&newer, &current), Err(CoreError::code("core/unknown_rules")));
    // A target below the current rules is raised by upgrade_rules, whose
    // error is returned (version 2 is not supported here).
    let mut older = target.clone();
    older.rules = 0;
    assert_eq!(reset(&older, &current).map(|b| b.rules), Ok(1));
    let mut current2 = current.clone();
    current2.rules = 2;
    assert_eq!(reset(&target, &current2), Err(CoreError::code("core/unknown_rules")));
}

// spec: CORE-GEN-006
#[test]
fn apply_and_rebind_use_no_float_and_no_hash_order() {
    for (name, src) in [("apply.rs", include_str!("../apply.rs")), ("ids.rs", include_str!("../ids.rs"))] {
        let code: String = src.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        for word in ["f32", "f64", "HashMap", "HashSet"] {
            assert!(!code.contains(word), "{name} uses {word}");
        }
    }
}

// spec: CORE-ID-002
// spec: CORE-ID-005
#[test]
fn restore_uses_the_fallback_when_the_row_id_is_held() {
    let mut b = board();
    // A row whose id is a live item's.
    b.archive.push(row(D1 + 1, 60));
    b.check().expect("check");
    let a = applied(&mut b, &LogCommand::Restore { id: D1 + 1, archived_at: t(60), fallback: D1 + 11 });
    assert_eq!(a.restored_as, Some(D1 + 11));
    let item = b.item(D1 + 11).expect("restored");
    assert_eq!((item.created, item.stamps), (t(1), all_stamps(st(10, 1))));
    // The fallback's mint check: another device's high part, or at the mark.
    for fallback in [D2 + 11, D1 + 11, 12] {
        let c = LogCommand::Restore { id: D1 + 5, archived_at: t(50), fallback };
        assert_eq!(refused(&mut b, &c, &ctx()).code, "core/bad_id");
    }
}

// spec: CORE-CMD-007
// spec: CORE-CTX-003
#[test]
fn apply_consults_no_zone_and_makes_no_instant() {
    let code: String = include_str!("../apply.rs").lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
    for word in ["Zone", "resolve_on_day", "local_day", "local_minutes", "from_parts", "from_at_ms", "from_chrono", "parse_canonical", "now("] {
        assert!(!code.contains(word), "apply.rs uses {word}");
    }
    // `created` of a created item is ctx.at.
    let mut b = board();
    applied(&mut b, &LogCommand::CreateRoutine(CreateRoutine {
        id: D1 + 11,
        name: "r".to_owned(),
        days: 1,
        anchor: chrono::NaiveDate::from_ymd_opt(2026, 3, 1).expect("date"),
        start_minutes: 1425,
        minutes: 15,
    }));
    let item = b.item(D1 + 11).expect("created");
    assert_eq!(item.created, ctx().at);
    assert_eq!(item.recurrence.map(|r| (r.start_minutes, r.minutes)), Some((1425, 15)));
}
