//! The canonical parser refuses a text whose parsed tree would exceed its budget, with an error and
//! without exhausting memory (core.md §4.5, CORE-GEN-007): the reproduction of the high finding of task A's
//! security pass, kept as a regression test.

// spec: CORE-GEN-007
use taskdeck_core::board::BoardData;

#[test]
fn sixty_mib_of_nested_objects_is_refused_with_an_error() {
    let limit = 62_914_560;
    let unit = format!("{}0{},", "{\"\":".repeat(30), "}".repeat(30));
    let (head, tail) = ("{\"archive\":[", "0],\"rules\":1}");
    let mut b = String::with_capacity(limit);
    b.push_str(head);
    for _ in 0..(limit - head.len() - tail.len()) / unit.len() {
        b.push_str(&unit);
    }
    b.push_str(tail);
    assert!(b.len() <= limit);
    let r = BoardData::from_canonical_bytes(b.as_bytes());
    assert_eq!(r.map(|_| ()).unwrap_err().code, "core/schema");
}
