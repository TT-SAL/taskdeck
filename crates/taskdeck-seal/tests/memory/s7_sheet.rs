//! The recovery sheet (seal.md §18, task S7): writing the sheet and reading
//! a typed one leave no freed heap block holding the seed, its 52 base32
//! characters or the 56 values the typed text normalises to. Repairs the
//! citation of SEAL-SHEET-003, which a type check alone did not prove.

use taskdeck_seal::*;

use crate::freed_with_bytes;

/// A seed, its sheet, and the byte patterns of §18's buffers: the 32 seed
/// bytes, the 52 characters of the seed without the spaces of the sheet,
/// and the 56 alphabet values of the typed sheet.
fn fixture() -> (RecoverySeed, String, Vec<Vec<u8>>) {
    let seed = RecoverySeed::generate(&mut OsEntropy).unwrap();
    let sheet: String = seed.to_sheet().as_str().to_owned();
    let chars: String = sheet.chars().filter(|c| *c != ' ').collect();
    assert_eq!(chars.len(), 56);
    let seed_chars = chars[..52].to_owned();
    let bytes = base32::decode_typed(&seed_chars, 32).unwrap();
    // The alphabet values (0 to 31) of the 56 characters, as `normalise`
    // holds them.
    const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let values: Vec<u8> =
        chars.bytes().map(|c| ALPHABET.iter().position(|&a| a == c).unwrap() as u8).collect();
    (seed, sheet, vec![bytes, seed_chars.into_bytes(), values])
}

// `to_sheet` (its 52 characters, the check, the grouped text, dropped by the
// caller as the zeroizing string it is) and `from_sheet` (the normalised
// values, the decoded bits, the check), on a good sheet and on one whose
// check fails at the last step, free no block holding the seed or one of
// its intermediate buffers. Each pattern, held in a plain Vec and dropped,
// is found (the control: the scan sees what an unwiped buffer leaves).
// spec: SEAL-SHEET-003
// spec: SEAL-GEN-006
#[test]
fn the_sheet_leaves_no_unwiped_buffer() {
    let (seed, sheet, pats) = fixture();
    for (i, p) in pats.iter().enumerate() {
        let copy = p.clone();
        let hits = freed_with_bytes(vec![p.clone()], || drop(copy));
        assert!(!hits.is_empty(), "control {i}");
    }

    let hits = freed_with_bytes(pats.clone(), || drop(seed.to_sheet()));
    assert!(hits.is_empty(), "to_sheet: {hits:?}");

    let hits = freed_with_bytes(pats.clone(), || {
        let s = RecoverySeed::from_sheet(&sheet).unwrap();
        drop(s);
    });
    assert!(hits.is_empty(), "from_sheet: {hits:?}");

    // The same seed with a wrong check character: refused at step 4,
    // after every buffer of steps 1 to 3 was filled.
    let mut bad: Vec<char> = sheet.chars().collect();
    let last = bad.len() - 1;
    bad[last] = if bad[last] == '0' { '1' } else { '0' };
    let bad: String = bad.into_iter().collect();
    let mut bad_pats = pats.clone();
    bad_pats.pop(); // the values differ in the last character
    let hits = freed_with_bytes(bad_pats, || {
        assert_eq!(RecoverySeed::from_sheet(&bad).err(), Some(SealError::SheetCheck));
    });
    assert!(hits.is_empty(), "from_sheet refused: {hits:?}");
}
