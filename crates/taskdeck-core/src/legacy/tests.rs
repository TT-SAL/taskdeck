//! Unit tests of the import's private helpers (core.md §15.3).

use super::*;

// spec: CORE-IMP-005
#[test]
fn device_minted_ids_are_high_parts_1_to_8191_with_a_non_zero_low_part() {
    assert!(device_minted((1 << 40) + 3));
    assert!(device_minted((5 << 40) + 1));
    assert!(device_minted((8191 << 40) + ((1 << 40) - 1)));
    assert!(!device_minted(3 << 40), "a low part of 0");
    assert!(!device_minted((8192 << 40) + 1), "a high part of 8192");
    assert!(!device_minted(7), "the legacy space");
}

// spec: CORE-IMP-005
#[test]
fn renumbering_stops_at_2_to_the_40() {
    let mut ids = Ids { next: (1 << 40) - 1, out_of_range: Vec::new(), overflowed: Vec::new(), renumbered: Vec::new() };
    assert_eq!(ids.renumber(IdList::Items, 0), (1 << 40) - 1);
    assert_eq!(ids.renumber(IdList::Archive, 1 << 62), 1 << 62, "no new id once next reaches 2^40");
    assert_eq!(ids.renumbered, vec![Renumbered { list: IdList::Items, old: 0, new: (1 << 40) - 1 }]);
    assert_eq!(ids.overflowed, vec![1 << 62]);
    ids.check_kept(1 << 40);
    ids.check_kept((1 << 40) + 1);
    ids.check_kept((1 << 62) - 1);
    assert_eq!(ids.out_of_range, vec![1 << 40, (1 << 62) - 1]);
}

// spec: CORE-IMP-007
#[test]
fn a_legacy_time_keeps_every_fraction_digit_and_its_offset() {
    let t: LocalTime = serde_json::from_str("\"2026-08-15T20:28:41.882008+03:00\"").expect("RFC 3339");
    assert_eq!(instant(&t).map(|i| i.to_canonical()), Some("2026-08-15T17:28:41.882008000Z".to_owned()));
    let t: LocalTime = serde_json::from_str("\"2026-09-01T10:00:00.999999999+02:00\"").expect("RFC 3339");
    assert_eq!(instant(&t).map(|i| i.to_canonical()), Some("2026-09-01T08:00:00.999999999Z".to_owned()));
    let t: LocalTime = serde_json::from_str("\"1970-01-01T00:30:00+01:00\"").expect("RFC 3339");
    assert_eq!(instant(&t), None, "1969 in UTC");
}

// spec: CORE-IMP-006
#[test]
fn kinds_follow_recurrence_then_is_event() {
    assert_eq!(kind_of(true, true), Kind::Routine);
    assert_eq!(kind_of(false, true), Kind::Event);
    assert_eq!(kind_of(false, false), Kind::Task);
}
