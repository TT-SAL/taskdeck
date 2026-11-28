use super::*;
use crate::board::{Home, Mark, PictureRef, Subscription};
use crate::command::{BlockMove, NewBlock};
use proptest::prelude::*;

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
// spec: CORE-ID-003
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
    // The same restore again, with a fresh fallback above the mark: a benign
    // repeat (§10.6), which accepts no id and raises no mark.
    let before = b.to_canonical_bytes();
    let a = applied(&mut b, &LogCommand::Restore { id: D1 + 1, archived_at: key.archived_at, fallback: D1 + 30 });
    assert_eq!(a, Applied { benign: true, ..Applied::default() });
    assert_eq!(b.mark(1), Mark { device_no: 1, item: 20, block: 12 }, "a benign repeat raises no mark (CORE-ID-003)");
    assert_eq!(b.to_canonical_bytes(), before);

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
    // At the last notes revision, in either board: `core/cap`, and the
    // inputs are left as they were.
    for (t_rev, c_rev) in [(MAX_ID, 9), (9, MAX_ID), (MAX_ID, MAX_ID)] {
        let mut t2 = target.clone();
        t2.notes_rev = t_rev;
        let mut c2 = current.clone();
        c2.notes_rev = c_rev;
        let (t0, c0) = (t2.clone(), c2.clone());
        assert_eq!(reset(&t2, &c2), Err(CoreError::code("core/cap")));
        assert_eq!((t2, c2), (t0, c0));
    }
    // One below: raised to the last revision, and the board passes check.
    let mut t3 = target.clone();
    t3.notes_rev = MAX_ID - 1;
    let r = reset(&t3, &current).expect("reset");
    assert_eq!(r.notes_rev, MAX_ID);
    r.check().expect("check");
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

// ---------------------------------------------------------------- the size check (§8.4 step 4)

const MAX_ID: u64 = (1 << 53) - 1;
const MAX_LOW: u64 = (1 << 40) - 1;

fn date(y: i32) -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(y, 12, 31).expect("date")
}

/// A string of `n` control characters, each written as `\u00XX` (six bytes).
fn controls(n: usize) -> String {
    "\u{1}".repeat(n)
}

/// A live item with every optional field present at its longest, a name of
/// `name` and `blocks` blocks at their longest.
fn longest_item(name: &str, blocks: u64) -> Item {
    let s = st(MAX_U53, 8191);
    Item {
        id: MAX_ID,
        kind: Kind::Routine,
        name: name.to_owned(),
        created: Instant::MAX,
        deadline: Some(Instant::MAX),
        importance: Some(4),
        time_importance: Some(3),
        duration_minutes: Some(1440),
        recurrence: Some(Recurrence { days: 127, anchor: date(9999), start_minutes: 1439, minutes: 1440 }),
        blocks: (0..blocks).map(|k| Block { sid: MAX_ID - k, start: Instant::MAX, minutes: 1440, stamp: s }).collect(),
        stamps: all_stamps(s),
    }
}

fn longest_row(name: &str, blocks: u64) -> ArchiveRow {
    ArchiveRow {
        id: MAX_ID,
        kind: Kind::Routine,
        name: name.to_owned(),
        created: Instant::MAX,
        deadline: Some(Instant::MAX),
        importance: Some(4),
        time_importance: Some(3),
        duration_minutes: Some(1440),
        recurrence: Some(Recurrence { days: 127, anchor: date(9999), start_minutes: 1439, minutes: 1440 }),
        blocks: (0..blocks).map(|k| RowBlock { sid: MAX_ID - k, start: Instant::MAX, minutes: 1440 }).collect(),
        archived_at: Instant::MAX,
        outcome: Outcome::Dropped,
    }
}

fn longest_sub(name: &str, url: &str) -> Subscription {
    Subscription { id: MAX_ID, name: name.to_owned(), url: url.to_owned(), color: [255; 4], enabled: false }
}

const LONGEST_MARK: Mark = Mark { device_no: 8191, item: MAX_LOW, block: MAX_LOW };

/// A board with every setting at its longest and one longest element in
/// each list.
fn longest_board() -> BoardData {
    let mut b = BoardData::empty();
    b.rules = 2_147_483_647;
    b.items = vec![longest_item(&controls(7), 2)];
    b.archive = vec![longest_row(&controls(5), 2)];
    b.unreadable = vec![vec![0; 1], vec![0; 2], vec![0; 3], vec![0; 4]];
    b.notes = controls(3);
    b.notes_rev = MAX_U53;
    b.subscriptions = vec![longest_sub(&controls(2), &controls(4))];
    b.palette = [[255; 4]; 6];
    b.home = Home { lat: -9000, lon: -18000 };
    b.picture = Some(PictureRef { blob: [255; 16], epoch: u32::MAX, parts: 64, sha256: [255; 32], size: 67_108_864 });
    b.marks = vec![LONGEST_MARK, Mark { device_no: 8190, ..LONGEST_MARK }];
    b
}

// (each constant of the bound is above the longest record it stands for,
// its comma included; a string byte takes at most six bytes)
// spec: CORE-APPLY-005
#[test]
fn the_bound_constants_cover_the_longest_records() {
    use crate::canon::Record;
    let len = |v: Vec<u8>| v.len() + 1;
    assert!(len(longest_item("", 0).to_canonical()) <= ITEM_BOUND);
    assert!(len(longest_item("", 1).to_canonical()) <= ITEM_BOUND + BLOCK_BOUND);
    assert!(len(longest_item("", 1000).to_canonical()) <= ITEM_BOUND + 1000 * BLOCK_BOUND);
    assert!(len(longest_row("", 0).to_canonical()) <= ROW_BOUND);
    assert!(len(longest_row("", 1000).to_canonical()) <= ROW_BOUND + 1000 * ROW_BLOCK_BOUND);
    assert!(len(longest_sub("", "").to_canonical()) <= SUB_BOUND);
    assert!(len(LONGEST_MARK.to_canonical()) <= MARK_BOUND);
    // A control character is six bytes, the most any byte takes.
    assert_eq!(longest_item(&controls(10), 0).to_canonical().len(), longest_item("", 0).to_canonical().len() + 6 * 10);
    assert_eq!(ESC, 6);
    // Every list with one element, every setting at its longest.
    let b = longest_board();
    assert!(b.to_canonical_bytes().len() <= size_bound(&b));
    assert_eq!(canonical_len(&b), b.to_canonical_bytes().len());
    assert_eq!(canonical_len(&BoardData::empty()), 11);
    let mut skeleton = longest_board();
    skeleton.notes = "x".to_owned();
    let lists = skeleton.to_canonical_bytes().len();
    assert!(lists <= BOARD_BOUND + 6 + ITEM_BOUND + ROW_BOUND + SUB_BOUND + 2 * MARK_BOUND + 2 * BLOCK_BOUND + 2 * ROW_BLOCK_BOUND + 120);
    // Unreadable lines are counted exactly: base64 with padding, quotes and a comma.
    let mut u = BoardData::empty();
    u.unreadable = vec![vec![0; 65_536]];
    assert_eq!(size_bound(&u) - BOARD_BOUND, 87_384 + 3);
}

// (the growth bound of each command above what its effects add in the worst
// case: the longest fields written over absent ones, with the longest stamps)
// spec: CORE-APPLY-005
#[test]
fn the_growth_bound_covers_the_longest_effects() {
    let x = Ctx { seq: MAX_U53, device_no: 8191, at: Instant::MAX, based_on: 0 };
    let hi = 8191u64 << 40;
    let mut b = BoardData::empty();
    let mut bare = task(hi + 1, st(2, 0), &[hi + 1]);
    bare.name = String::new();
    bare.blocks[0].minutes = 0;
    let mut ev = task(hi + 2, st(2, 0), &[]);
    ev.kind = Kind::Event;
    let mut rt = task(hi + 3, st(2, 0), &[]);
    rt.kind = Kind::Routine;
    rt.recurrence = Some(Recurrence { days: 0, anchor: date(1970), start_minutes: 0, minutes: 0 });
    b.items = vec![bare, ev, rt];
    let mut r = row(0, 1);
    r.blocks = (1..=1000).map(|k| RowBlock { sid: hi + 100 + k, start: t(1), minutes: 0 }).collect();
    b.archive = vec![r];
    b.subscriptions = vec![Subscription { id: hi + 4, name: String::new(), url: "u".to_owned(), color: [0; 4], enabled: true }];
    b.marks = vec![Mark { device_no: 8191, item: 4, block: 1100 }];
    let at = Instant::MAX;
    let long_task = CreateTask {
        id: hi + MAX_LOW,
        name: controls(200),
        deadline: Some(at),
        importance: Some(4),
        time_importance: Some(3),
        duration_minutes: Some(1440),
        blocks: (0..1000).map(|k| NewBlock { sid: hi + MAX_LOW - k, start: at, minutes: 1440 }).collect(),
    };
    let commands = [
        LogCommand::CreateTask(long_task),
        LogCommand::CreateEvent(CreateEvent {
            id: hi + MAX_LOW,
            name: controls(200),
            start: at,
            minutes: Some(1440),
            importance: Some(4),
            time_importance: Some(3),
        }),
        LogCommand::CreateRoutine(CreateRoutine { id: hi + MAX_LOW, name: controls(200), days: 127, anchor: date(9999), start_minutes: 1425, minutes: 1440 }),
        LogCommand::Rename { id: hi + 1, name: controls(200) },
        LogCommand::Complete { id: hi + 1, at },
        LogCommand::Delete { id: hi + 3, at },
        LogCommand::MoveBlock { id: hi + 1, sid: hi + 1, start: at, minutes: 1440 },
        LogCommand::MoveBlocks { moves: vec![BlockMove { id: hi + 1, sid: hi + 1, start: at, minutes: 1440 }] },
        LogCommand::AddBlock { id: hi + 1, sid: hi + MAX_LOW, start: at, minutes: 1440 },
        LogCommand::SetEstimate { id: hi + 1, minutes: 1440 },
        LogCommand::SetDeadline { id: hi + 1, deadline: Some(at), seed: Some(4) },
        LogCommand::SetSeverity { id: hi + 1, level: 4 },
        LogCommand::SetHorizon { id: hi + 1, level: 3 },
        LogCommand::SetRepeat { id: hi + 3, days: 127, anchor: date(9999) },
        LogCommand::MoveRoutine { id: hi + 3, start_minutes: 1425, minutes: 1440 },
        LogCommand::SetEventSpan { id: hi + 2, start: at, minutes: 1440 },
        LogCommand::SetNotes { text: controls(20_000), base: 0 },
        LogCommand::Restore { id: 0, archived_at: t(1), fallback: hi + MAX_LOW },
        LogCommand::AddSubscription { id: hi + MAX_LOW, name: controls(200), url: controls(2000), color: [255; 4] },
        LogCommand::RenameSubscription { id: hi + 4, name: controls(200) },
        LogCommand::SetSubscriptionColor { id: hi + 4, color: [255; 4] },
        LogCommand::SetSubscriptionEnabled { id: hi + 4, enabled: false },
        LogCommand::SetPalette { colors: [[255; 4]; 6] },
        LogCommand::SetHome { lat: -9000, lon: -18000 },
        LogCommand::SetPicture {
            picture: Some(PictureRef { blob: [255; 16], epoch: u32::MAX, parts: 64, sha256: [255; 32], size: 67_108_864 }),
        },
    ];
    let before = b.to_canonical_bytes().len();
    for c in &commands {
        let mut after = b.clone();
        let a = apply(&mut after, c, &x).unwrap_or_else(|r| panic!("{} refused: {}", c.op(), r.code));
        assert!(!a.benign, "{}", c.op());
        let grown = after.to_canonical_bytes().len().saturating_sub(before);
        assert!(grown <= growth_bound(c, &b), "{}: {grown} over {}", c.op(), growth_bound(c, &b));
        assert!(after.to_canonical_bytes().len() <= size_bound(&after), "{}", c.op());
        assert_eq!(canonical_len(&after), after.to_canonical_bytes().len(), "{}", c.op());
    }
}

/// `lines` unreadable lines of 65,536 zero bytes: 87,387 bytes each in the
/// canonical board bytes, plus 26 (`{"rules":1,"unreadable":[` and `]}`).
fn padded(lines: usize) -> BoardData {
    let mut b = BoardData::empty();
    b.unreadable = vec![vec![0; 65_536]; lines];
    assert_eq!(b.to_canonical_bytes().len(), 87_387 * lines + 26);
    assert_eq!(canonical_len(&b), 87_387 * lines + 26);
    b
}

// spec: CORE-APPLY-005
// spec: CORE-APPLY-001
// spec: CORE-APPLY-002
#[test]
fn a_command_that_lengthens_the_board_past_the_cap_is_refused() {
    // 707 lines: 61,782,635 bytes. `set_notes` from revision 0 adds
    // `"notes":"…",` (11 and the text) and `"notes_rev":1,` (14).
    let mut b = padded(707);
    let room = SIZE_CAP - b.to_canonical_bytes().len() - 25;
    assert_eq!(room, 83_324);
    // 13,887 control characters (six bytes each) and two letters: exactly the cap.
    let exact = format!("{}ab", controls(13_887));
    let x = Ctx { seq: 2, device_no: 1, at: t(0), based_on: 1 };
    let mut one = b.clone();
    let over = LogCommand::SetNotes { text: format!("{exact}c"), base: 0 };
    let r = apply(&mut one, &over, &x).expect_err("one byte over the cap");
    assert_eq!(r, Refusal { code: "core/cap", op: "set_notes", target: None, notes: None });
    assert_eq!(one, b, "a refusal for size changes nothing");
    // A refusal of the command's own checks comes first (§8.4 step 3).
    let stale = LogCommand::SetNotes { text: format!("{exact}c"), base: 1 };
    assert_eq!(apply(&mut one, &stale, &x).map(|_| ()).expect_err("stale").code, "core/notes_stale");
    apply(&mut b, &LogCommand::SetNotes { text: exact, base: 0 }, &x).expect("exactly the cap");
    assert_eq!(b.to_canonical_bytes().len(), SIZE_CAP);
}

// spec: CORE-APPLY-005
// spec: CORE-APPLY-004
#[test]
fn on_a_board_over_the_cap_only_a_longer_one_is_refused() {
    // 708 lines: 61,870,048 bytes, over the cap; plus an item `1` named `t`.
    let mut b = padded(708);
    b.items = vec![task(1, st(1, 0), &[])];
    b.items[0].name = "t".to_owned();
    b.marks = vec![Mark { device_no: 0, item: 1, block: 0 }];
    b.check().expect("check");
    let len = |b: &BoardData| b.to_canonical_bytes().len();
    let x = |seq| Ctx { seq, device_no: 1, at: t(0), based_on: 1 };
    // Same length: the name and the stamp `[1,0]` to `[2,1]`.
    let start = len(&b);
    apply(&mut b, &LogCommand::Rename { id: 1, name: "u".to_owned() }, &x(2)).expect("same length");
    assert_eq!((len(&b), b.items[0].name.as_str()), (start, "u"));
    // One byte longer.
    let before = b.clone();
    let r = apply(&mut b, &LogCommand::Rename { id: 1, name: "uu".to_owned() }, &x(3)).expect_err("longer");
    assert_eq!((r.code, r.target), ("core/cap", Some(1)));
    assert_eq!(b, before);
    // A benign repeat and a check refusal are what they are.
    assert!(apply(&mut b, &LogCommand::RemoveBlocks { blocks: vec![(1, 5)] }, &x(4)).expect("benign").benign);
    assert_eq!(apply(&mut b, &LogCommand::Rename { id: 9, name: "uu".to_owned() }, &x(4)).map(|_| ()).expect_err("gone").code, "core/gone");
    // Shorter.
    apply(&mut b, &LogCommand::Forget { id: 1 }, &x(4)).expect("shorter");
    assert!(len(&b) < start && len(&b) > SIZE_CAP);
}

// ---------------------------------------------------------------- the bounds, generated

/// Strings heavy in the characters that take the most bytes.
fn text(max: usize) -> impl Strategy<Value = String> {
    proptest::collection::vec(prop_oneof![Just('a'), Just('\u{1}'), Just('\u{1f}'), Just('"'), Just('\\'), Just('\n'), Just('é'), Just('😀')], 0..max)
        .prop_map(|v| v.into_iter().collect())
}

/// Small numbers, the largest ones, and anything between.
fn big(max: u64) -> impl Strategy<Value = u64> {
    prop_oneof![0u64..4, Just(max), 0..=max]
}

fn any_stamp() -> impl Strategy<Value = Stamp> {
    (big(MAX_U53), prop_oneof![0u16..3, Just(8191u16)]).prop_map(|(seq, device_no)| Stamp { seq, device_no })
}

fn any_instant() -> impl Strategy<Value = Instant> {
    prop_oneof![Just(Instant::MIN), Just(Instant::MAX), (0i64..4).prop_map(t)]
}

/// An id of a small pool (devices 1, 2 and 8191, low parts 1 to 4), so that
/// generated commands meet generated items.
fn pool_id() -> impl Strategy<Value = u64> {
    (prop_oneof![Just(1u64), Just(2u64), Just(8191u64)], 1u64..5).prop_map(|(d, n)| (d << 40) + n)
}

fn opt_fields() -> impl Strategy<Value = (Option<Instant>, Option<u8>, Option<u8>, Option<u32>)> {
    (
        proptest::option::of(any_instant()),
        proptest::option::of(0u8..5),
        proptest::option::of(0u8..4),
        proptest::option::of(prop_oneof![Just(0u32), Just(1440u32), 0u32..1441]),
    )
}

fn any_recurrence() -> impl Strategy<Value = Recurrence> {
    (0u8..128, prop_oneof![Just(1970), Just(9999), 1970i32..10000], 0i32..1440, 0u32..1441)
        .prop_map(|(days, y, start_minutes, minutes)| Recurrence { days, anchor: date(y), start_minutes, minutes })
}

fn any_item() -> impl Strategy<Value = Item> {
    (
        pool_id(),
        0u8..3,
        text(6),
        any_instant(),
        opt_fields(),
        any_recurrence(),
        proptest::collection::vec((pool_id(), any_instant(), 0u32..1441, any_stamp()), 0..3),
        [any_stamp(), any_stamp(), any_stamp(), any_stamp(), any_stamp()],
    )
        .prop_map(|(id, k, name, created, (deadline, importance, time_importance, duration_minutes), r, blocks, s)| {
            let kind = [Kind::Task, Kind::Event, Kind::Routine][usize::from(k)];
            let mut blocks: Vec<Block> = blocks.into_iter().map(|(sid, start, minutes, stamp)| Block { sid, start, minutes, stamp }).collect();
            blocks.sort_by_key(|b| b.sid);
            blocks.dedup_by_key(|b| b.sid);
            let [estimate, levels, name_stamp, repeat, time] = s;
            Item {
                id,
                kind,
                name,
                created,
                deadline,
                importance,
                time_importance,
                duration_minutes,
                recurrence: (kind == Kind::Routine).then_some(r),
                blocks,
                stamps: ItemStamps { estimate, levels, name: name_stamp, repeat, time },
            }
        })
}

fn any_row() -> impl Strategy<Value = ArchiveRow> {
    (
        prop_oneof![Just(0u64), pool_id(), Just(MAX_ID)],
        0u8..3,
        text(6),
        (any_instant(), any_instant()),
        opt_fields(),
        any_recurrence(),
        proptest::collection::vec((pool_id(), any_instant(), 0u32..1441), 0..3),
        any::<bool>(),
    )
        .prop_map(|(id, k, name, (created, archived_at), (deadline, importance, time_importance, duration_minutes), r, blocks, dropped)| {
            let kind = [Kind::Task, Kind::Event, Kind::Routine][usize::from(k)];
            let mut blocks: Vec<RowBlock> = blocks.into_iter().map(|(sid, start, minutes)| RowBlock { sid, start, minutes }).collect();
            blocks.sort_by_key(|b| b.sid);
            blocks.dedup_by_key(|b| b.sid);
            ArchiveRow {
                id,
                kind,
                name,
                created,
                deadline,
                importance,
                time_importance,
                duration_minutes,
                recurrence: (kind == Kind::Routine).then_some(r),
                blocks,
                archived_at,
                outcome: if dropped { Outcome::Dropped } else { Outcome::Finished },
            }
        })
}

fn any_picture() -> impl Strategy<Value = PictureRef> {
    (any::<u8>(), 1u32..=u32::MAX, 1u32..65, 67u64..=67_108_864).prop_map(|(x, epoch, parts, size)| PictureRef {
        blob: [x; 16],
        epoch,
        parts,
        sha256: [x; 32],
        size,
    })
}

/// Any board of the shapes `apply` meets, the lists in their orders; it need
/// not pass `check`, since the bounds hold for every board.
fn any_board() -> impl Strategy<Value = BoardData> {
    (
        proptest::collection::vec(any_item(), 0..10),
        proptest::collection::vec(any_row(), 0..4),
        proptest::collection::vec(proptest::collection::vec(any::<u8>(), 1..8), 0..3),
        (text(8), prop_oneof![3 => 0u64..3, 1 => big(MAX_U53)]),
        proptest::collection::vec((pool_id(), text(4), text(6), any::<u8>(), any::<bool>()), 0..3),
        (any::<u8>(), -9000i32..9001, -18000i32..18001, proptest::option::of(any_picture()), prop_oneof![6 => Just(1u64), 1 => big(2_147_483_647)]),
        proptest::collection::vec((prop_oneof![0u16..3, Just(8191u16)], 0u64..4, 0u64..4), 0..3),
    )
        .prop_map(|(items, rows, mut unreadable, (notes, notes_rev), subs, (c, lat, lon, picture, rules), marks)| {
            let mut b = BoardData::empty();
            for i in items {
                if let Err(k) = b.item_index(i.id) {
                    b.items.insert(k, i);
                }
            }
            for r in rows {
                b.insert_archive_row(r);
            }
            unreadable.sort();
            b.unreadable = unreadable;
            b.notes = notes;
            b.notes_rev = notes_rev;
            for (id, name, url, color, enabled) in subs {
                if let Err(k) = b.subscription_index(id) {
                    b.subscriptions.insert(k, Subscription { id, name, url: format!("u{url}"), color: [color; 4], enabled });
                }
            }
            b.palette = [[c; 4]; 6];
            b.home = Home { lat, lon };
            b.picture = picture;
            b.rules = u32::try_from(rules.max(1)).unwrap_or(1);
            for (d, item, block) in marks {
                b.raise_mark(d, item, block);
            }
            b
        })
}

fn any_command() -> impl Strategy<Value = LogCommand> {
    let creates = prop_oneof![
        (pool_id(), text(8), opt_fields(), proptest::collection::vec((pool_id(), any_instant(), 15u32..1441), 0..3)).prop_map(
            |(id, name, (deadline, importance, time_importance, duration_minutes), blocks)| {
                let mut blocks: Vec<NewBlock> = blocks.into_iter().map(|(sid, start, minutes)| NewBlock { sid, start, minutes }).collect();
                blocks.sort_by_key(|b| b.sid);
                blocks.dedup_by_key(|b| b.sid);
                LogCommand::CreateTask(CreateTask { id, name, deadline, importance, time_importance, duration_minutes, blocks })
            }
        ),
        (pool_id(), text(8), any_instant(), opt_fields()).prop_map(|(id, name, start, (_, importance, time_importance, minutes))| {
            LogCommand::CreateEvent(CreateEvent { id, name, start, minutes, importance, time_importance })
        }),
        (pool_id(), text(8), any_recurrence()).prop_map(|(id, name, r)| {
            LogCommand::CreateRoutine(CreateRoutine { id, name, days: r.days, anchor: r.anchor, start_minutes: r.start_minutes, minutes: r.minutes })
        }),
        (pool_id(), text(8)).prop_map(|(id, name)| LogCommand::Rename { id, name }),
        (pool_id(), any_instant()).prop_map(|(id, at)| LogCommand::Complete { id, at }),
        (pool_id(), any_instant()).prop_map(|(id, at)| LogCommand::Delete { id, at }),
        pool_id().prop_map(|id| LogCommand::Forget { id }),
    ];
    let fields = prop_oneof![
        (pool_id(), pool_id(), any_instant(), 0u32..1441).prop_map(|(id, sid, start, minutes)| LogCommand::MoveBlock { id, sid, start, minutes }),
        proptest::collection::vec((pool_id(), pool_id(), any_instant(), 0u32..1441), 1..4).prop_map(|m| LogCommand::MoveBlocks {
            moves: m.into_iter().map(|(id, sid, start, minutes)| BlockMove { id, sid, start, minutes }).collect()
        }),
        (pool_id(), pool_id(), any_instant(), 15u32..1441).prop_map(|(id, sid, start, minutes)| LogCommand::AddBlock { id, sid, start, minutes }),
        proptest::collection::vec((pool_id(), pool_id()), 1..4).prop_map(|blocks| LogCommand::RemoveBlocks { blocks }),
        (pool_id(), 15u32..1441).prop_map(|(id, minutes)| LogCommand::SetEstimate { id, minutes }),
        (pool_id(), proptest::option::of(any_instant()), proptest::option::of(0u8..5))
            .prop_map(|(id, deadline, seed)| LogCommand::SetDeadline { id, deadline, seed }),
        (pool_id(), 0u8..5).prop_map(|(id, level)| LogCommand::SetSeverity { id, level }),
        (pool_id(), 0u8..4).prop_map(|(id, level)| LogCommand::SetHorizon { id, level }),
        (pool_id(), any_recurrence()).prop_map(|(id, r)| LogCommand::SetRepeat { id, days: r.days, anchor: r.anchor }),
        (pool_id(), any_recurrence()).prop_map(|(id, r)| LogCommand::MoveRoutine { id, start_minutes: r.start_minutes, minutes: r.minutes }),
        (pool_id(), any_instant(), 15u32..1441).prop_map(|(id, start, minutes)| LogCommand::SetEventSpan { id, start, minutes }),
    ];
    let rest = prop_oneof![
        (text(12), prop_oneof![3 => 0u64..3, 1 => big(MAX_U53)]).prop_map(|(text, base)| LogCommand::SetNotes { text, base }),
        (prop_oneof![Just(0u64), pool_id(), Just(MAX_ID)], any_instant(), pool_id())
            .prop_map(|(id, archived_at, fallback)| LogCommand::Restore { id, archived_at, fallback }),
        (prop_oneof![Just(0u64), pool_id()], any_instant()).prop_map(|(id, archived_at)| LogCommand::ForgetArchived { id, archived_at }),
        (pool_id(), text(6), text(10), any::<u8>()).prop_map(|(id, name, url, c)| LogCommand::AddSubscription {
            id,
            name,
            url: format!("https://{url}"),
            color: [c; 4]
        }),
        pool_id().prop_map(|id| LogCommand::RemoveSubscription { id }),
        (pool_id(), text(8)).prop_map(|(id, name)| LogCommand::RenameSubscription { id, name }),
        (pool_id(), any::<u8>()).prop_map(|(id, c)| LogCommand::SetSubscriptionColor { id, color: [c; 4] }),
        (pool_id(), any::<bool>()).prop_map(|(id, enabled)| LogCommand::SetSubscriptionEnabled { id, enabled }),
        any::<u8>().prop_map(|c| LogCommand::SetPalette { colors: [[c; 4]; 6] }),
        (-9000i32..9001, -18000i32..18001).prop_map(|(lat, lon)| LogCommand::SetHome { lat, lon }),
        proptest::option::of(any_picture()).prop_map(|picture| LogCommand::SetPicture { picture }),
    ];
    prop_oneof![creates, fields, rest]
}

fn any_ctx() -> impl Strategy<Value = Ctx> {
    (prop_oneof![2u64..40, Just(MAX_U53), 2..=MAX_U53], prop_oneof![1u16..3, Just(8191u16)], any_instant(), any::<u64>())
        .prop_map(|(seq, device_no, at, back)| Ctx { seq, device_no, at, based_on: back % seq })
}

proptest! {
    // (U ≥ the canonical length on generated boards, and every applied
    // command adds at most its growth bound; so the fast path of step 4 never
    // skips a refusal)
    // spec: CORE-APPLY-005
    #[test]
    fn the_size_bound_and_the_growth_bound_hold(start in any_board(), list in proptest::collection::vec((any_command(), any_ctx()), 1..8)) {
        let mut b = start;
        prop_assert!(b.to_canonical_bytes().len() <= size_bound(&b));
        prop_assert_eq!(canonical_len(&b), b.to_canonical_bytes().len());
        for (c, x) in &list {
            let before = b.to_canonical_bytes().len();
            let bound = growth_bound(c, &b);
            let mut next = b.clone();
            if apply(&mut next, c, x).is_ok() {
                let after = next.to_canonical_bytes().len();
                prop_assert!(after <= before + bound, "{} added {} over its bound {}", c.op(), after.saturating_sub(before), bound);
                prop_assert!(after <= size_bound(&next));
                prop_assert_eq!(canonical_len(&next), after);
                b = next;
            } else {
                prop_assert_eq!(&next, &b);
            }
        }
    }
}

// (notice and refusal records are at most 4,096 bytes, compared before
// anything else is read)
// spec: CORE-CONF-006
// spec: CORE-ENC-004
#[test]
fn notice_and_refusal_records_have_a_limit_of_4096_bytes() {
    assert_eq!(RECORD_LIMIT, 4096);
    let mut big = vec![b' '; 4097];
    big[0] = 0xff;
    assert_eq!(Notice::from_canonical(&big), Err(CoreError::code("core/too_large")));
    assert_eq!(RefusalRecord::from_canonical(&big), Err(CoreError::code("core/too_large")));
    assert_eq!(Notice::from_canonical(&big[..4096]), Err(CoreError::code("core/not_json")));
    assert_eq!(RefusalRecord::from_canonical(&big[..4096]), Err(CoreError::code("core/not_json")));
    // A refusal of 4,096 bytes reads; one of 4,097 does not.
    let record = |n: usize| {
        let head = r#"{"author":3,"code":""#;
        let tail = r#"","seq":12}"#;
        format!("{head}{}{tail}", "x".repeat(n - head.len() - tail.len()))
    };
    assert!(RefusalRecord::from_canonical(record(4096).as_bytes()).is_ok());
    assert_eq!(RefusalRecord::from_canonical(record(4097).as_bytes()), Err(CoreError::code("core/too_large")));
}
