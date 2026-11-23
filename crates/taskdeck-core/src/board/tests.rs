use super::*;

fn t(text: &str) -> Instant {
    Instant::parse_canonical(text).unwrap_or(Instant::MIN)
}

fn row(id: u64, at: &str, name: &str) -> ArchiveRow {
    ArchiveRow {
        id,
        kind: Kind::Task,
        name: name.to_owned(),
        created: t("2026-09-01T00:00:00.000000000Z"),
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

const ZERO: Stamp = Stamp { seq: 0, device_no: 0 };

fn item(id: u64) -> Item {
    Item {
        id,
        kind: Kind::Task,
        name: "n".to_owned(),
        created: t("2026-09-01T00:00:00.000000000Z"),
        deadline: None,
        importance: None,
        time_importance: None,
        duration_minutes: None,
        recurrence: None,
        blocks: Vec::new(),
        stamps: ItemStamps { estimate: ZERO, levels: ZERO, name: ZERO, repeat: ZERO, time: ZERO },
    }
}

const A1: &str = "2026-09-02T00:00:00.000000000Z";
const A2: &str = "2026-09-03T00:00:00.000000000Z";

// (the archive helper keeps the order of §6.12)
// spec: CORE-DATA-003
#[test]
fn insert_archive_row_keeps_the_archive_order() {
    let mut b = BoardData::empty();
    b.marks.push(Mark { device_no: 0, item: 9, block: 0 });
    for r in [row(2, A2, "x"), row(1, A2, "x"), row(1, A1, "b"), row(1, A1, "a"), row(0, A1, "z"), row(1, A1, "b"), row(9, A1, "a")] {
        b.insert_archive_row(r);
        assert_eq!(b.check(), Ok(()));
    }
    let keys: Vec<(u64, &str)> = b.archive.iter().map(|r| (r.id, r.name.as_str())).collect();
    assert_eq!(keys, vec![(0, "z"), (1, "a"), (1, "b"), (1, "b"), (9, "a"), (1, "x"), (2, "x")]);
    assert_eq!(b.find_archive_row(&ArchiveKey { id: 1, archived_at: t(A1) }), Some(1));
    assert_eq!(b.find_archive_row(&ArchiveKey { id: 1, archived_at: t(A2) }), Some(5));
    assert_eq!(b.find_archive_row(&ArchiveKey { id: 3, archived_at: t(A1) }), None);
    assert_eq!(b.find_archive_row(&ArchiveKey { id: 3, archived_at: t(A2) }), None);
    assert_eq!(archive_cmp(&row(1, A1, "a"), &row(1, A1, "ab")), Ordering::Less);
    assert_eq!(archive_cmp(&row(5, A1, "a"), &row(1, A2, "a")), Ordering::Less);
}

#[test]
fn raise_mark_inserts_at_its_place_and_never_lowers() {
    let mut b = BoardData::empty();
    b.raise_mark(5, 0, 0);
    assert!(b.marks.is_empty(), "an all-zero mark is not stored");
    b.raise_mark(5, 3, 0);
    b.raise_mark(2, 0, 7);
    b.raise_mark(5, 1, 4);
    assert_eq!(b.marks, vec![Mark { device_no: 2, item: 0, block: 7 }, Mark { device_no: 5, item: 3, block: 4 }]);
    assert_eq!(b.mark(5), Mark { device_no: 5, item: 3, block: 4 });
    assert_eq!(b.mark(4), Mark { device_no: 4, item: 0, block: 0 });
    assert_eq!(b.check(), Ok(()));
}

#[test]
fn lookups_by_id() {
    let mut b = BoardData::empty();
    b.items = vec![item(1), item(3), item(7)];
    b.marks.push(Mark { device_no: 0, item: 9, block: 0 });
    b.subscriptions.push(Subscription { id: 9, name: "s".into(), url: "https://x".into(), color: [0; 4], enabled: true });
    assert_eq!(b.item(3).map(|i| i.id), Some(3));
    assert!(b.item(2).is_none());
    assert_eq!(b.item_index(5), Err(2));
    assert_eq!(b.subscription(9).map(|s| s.id), Some(9));
    assert!(b.subscription(1).is_none());
    assert_eq!(b.subscription_index(10), Err(1));
    assert_eq!(b.check(), Ok(()));
}

// (check also holds the schema ranges of values built in memory)
// spec: CORE-DATA-001
#[test]
fn check_holds_the_schema_of_built_values() {
    let base = {
        let mut b = BoardData::empty();
        b.items = vec![item(1)];
        b.marks.push(Mark { device_no: 0, item: 1, block: 0 });
        b
    };
    assert_eq!(base.check(), Ok(()));
    type Case = (fn(&mut BoardData), u32);
    let cases: Vec<Case> = vec![
        (|b| b.items[0].importance = Some(5), 1),
        (|b| b.items[0].time_importance = Some(4), 1),
        (|b| b.items[0].duration_minutes = Some(1441), 1),
        (|b| b.items[0].name = "x".repeat(4097), 1),
        (|b| b.items[0].stamps.name.device_no = 8192, 1),
        (|b| b.items[0].stamps.time.seq = 1 << 53, 1),
        (|b| b.rules = 0, 1),
        (|b| b.home.lat = 9001, 1),
        (|b| b.notes_rev = 1 << 53, 1),
        (|b| b.unreadable.push(Vec::new()), 1),
        (|b| b.marks[0].item = 1 << 40, 1),
        (|b| b.items[0].recurrence = Some(Recurrence { days: 128, anchor: chrono::NaiveDate::default(), start_minutes: 0, minutes: 0 }), 1),
        (|b| b.items[0].kind = Kind::Routine, 6),
        (|b| b.items[0].blocks = vec![Block { sid: 1, start: Instant::MIN, minutes: 15, stamp: ZERO }; 1001], 1),
        (|b| b.items[0].blocks = (1..=1001).map(|sid| Block { sid, start: Instant::MIN, minutes: 15, stamp: ZERO }).collect(), 2),
        (|b| b.notes = "\t".into(), 7),
        (|b| b.rules = 3, 9),
    ];
    for (k, (change, rule)) in cases.into_iter().enumerate() {
        let mut b = base.clone();
        change(&mut b);
        assert_eq!(b.check(), Err(CoreError::invariant(rule)), "case {k}");
    }
}

// (no per-device value: the board's fields are exactly those of §6.1)
// spec: CORE-DATA-007
#[test]
fn the_board_holds_only_replicated_fields() {
    let mut b = BoardData::empty();
    b.home = Home { lat: 1, lon: 1 };
    b.notes = "n".into();
    b.notes_rev = 1;
    b.palette[0] = [1, 1, 1, 1];
    b.picture = Some(PictureRef { blob: [0; 16], epoch: 1, parts: 1, sha256: [0; 32], size: 67 });
    b.items = vec![item(1)];
    b.archive = vec![row(1, A1, "r")];
    b.subscriptions.push(Subscription { id: 2, name: "s".into(), url: "u".into(), color: [0; 4], enabled: true });
    b.unreadable.push(vec![1]);
    b.marks.push(Mark { device_no: 0, item: 2, block: 0 });
    let v = b.to_value();
    let keys: Vec<&str> = v.as_object().map(|m| m.keys().map(String::as_str).collect()).unwrap_or_default();
    let mut keys = keys;
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["archive", "home", "items", "marks", "notes", "notes_rev", "palette", "picture", "rules", "subscriptions", "unreadable"]
    );
    assert_eq!(BoardData::from_canonical_bytes(&b.to_canonical_bytes()), Ok(b));
}

// (the board limit is compared before anything else)
// spec: CORE-GEN-009
#[test]
fn the_board_limit() {
    let mut big = vec![b' '; canon::BOARD_LIMIT + 1];
    big[0] = 0xff;
    assert_eq!(BoardData::from_canonical_bytes(&big), Err(CoreError::code("core/too_large")));
    // Notes at their byte limit load; one byte more is a schema error.
    let at = format!("{{\"notes\":\"{}\",\"rules\":1}}", "x".repeat(NOTES_BYTES));
    assert!(BoardData::from_canonical_bytes(at.as_bytes()).is_ok());
    let over = format!("{{\"notes\":\"{}\",\"rules\":1}}", "x".repeat(NOTES_BYTES + 1));
    assert_eq!(BoardData::from_canonical_bytes(over.as_bytes()), Err(schema()));
    // An unreadable line of 65,536 bytes loads; 65,537 bytes is a schema error.
    let line = |n: usize| format!("{{\"rules\":1,\"unreadable\":[\"{}\"]}}", canon::base64_text(&vec![0u8; n]));
    assert!(BoardData::from_canonical_bytes(line(LINE_BYTES).as_bytes()).is_ok());
    assert_eq!(BoardData::from_canonical_bytes(line(LINE_BYTES + 1).as_bytes()), Err(schema()));
}

#[test]
fn the_empty_board() {
    assert_eq!(BoardData::empty().to_canonical_bytes(), b"{\"rules\":1}".to_vec());
    assert_eq!(BoardData::empty().check(), Ok(()));
}
