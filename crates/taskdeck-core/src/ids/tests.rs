use super::*;

// spec: CORE-ID-001
#[test]
fn compose_and_split() {
    assert_eq!(compose(0, 1), Some(1));
    assert_eq!(compose(1, 1), Some((1 << 40) + 1));
    assert_eq!(compose(5, 1), Some(5 * (1 << 40) + 1));
    assert_eq!(compose(8191, (1 << 40) - 1), Some((1 << 53) - 1));
    assert_eq!(compose(8192, 1), None);
    assert_eq!(compose(1, 0), None);
    assert_eq!(compose(1, 1 << 40), None);
    assert_eq!(split((1 << 40) + 3), (1, 3));
    assert_eq!(split((1 << 53) - 1), (8191, (1 << 40) - 1));
    assert_eq!(split(3 << 40), (3, 0));
    assert_eq!(split(7), (0, 7));
    assert_eq!(split(u64::MAX), (u16::MAX, (1 << 40) - 1));
    for (d, n) in [(0u16, 1u64), (17, 99), (8191, 12345)] {
        assert_eq!(compose(d, n).map(split), Some((d, n)));
    }
}

// spec: CORE-ID-007
#[test]
fn a_new_minter() {
    assert_eq!(IdMinter::new(42), IdMinter { device_no: 42, next_item: 1, next_block: 1 });
}

// spec: CORE-ID-004
#[test]
fn minting_takes_the_larger_of_minter_and_mark() {
    let mut view = BoardData::empty();
    let mut m = IdMinter::new(2);
    assert_eq!(m.mint_item(&view), compose(2, 1));
    assert_eq!(m.mint_item(&view), compose(2, 2));
    assert_eq!(m.mint_block(&view), compose(2, 1));
    view.marks.push(crate::board::Mark { device_no: 2, item: 10, block: 0 });
    assert_eq!(m.mint_item(&view), compose(2, 11));
    assert_eq!(m.next_item, 12);
    assert_eq!(m.mint_block(&view), compose(2, 2));
    // The view's mark went back (another view): the minter still moves on.
    let empty = BoardData::empty();
    assert_eq!(m.mint_item(&empty), compose(2, 12));
    assert_eq!(m, IdMinter { device_no: 2, next_item: 13, next_block: 3 });
}

#[test]
fn minting_stops_at_the_end_of_the_number_space() {
    let view = BoardData::empty();
    let mut m = IdMinter { device_no: 3, next_item: (1 << 40) - 1, next_block: 1 << 40 };
    assert_eq!(m.mint_item(&view), compose(3, (1 << 40) - 1));
    assert_eq!(m.mint_item(&view), None);
    assert_eq!(m.next_item, 1 << 40);
    assert_eq!(m.mint_block(&view), None);
    assert_eq!(m.next_block, 1 << 40);
    let mut legacy = IdMinter::new(0);
    assert_eq!(legacy.mint_item(&view), None);
    let mut big = IdMinter::new(8192);
    assert_eq!(big.mint_block(&view), None);
    let mut top = IdMinter { device_no: 1, next_item: u64::MAX, next_block: 1 };
    assert_eq!(top.mint_item(&view), None);
}
