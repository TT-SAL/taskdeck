//! Typed text with check characters (task S7): the recovery sheet (§18),
//! the pairing code (§17.2) and the move code (§15.4). Every character
//! replaced by every other character of the alphabet, and by one outside
//! it, is rejected. A check of 20 or 10 bits cannot catch every change to
//! the secret's characters: where the check happens to match, the text
//! names another secret, and that secret is rejected where it is used (a
//! recovery key the board does not hold; a ticket that is not E's, whose
//! slot A then fails N's MAC check). Look-alikes (`O`, `I`, `L`, lowercase)
//! are the same character after §5.2's normalisation and are no change.

use crate::board::*;
use crate::pairing::*;
use taskdeck_seal::pair::*;
use taskdeck_seal::*;

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Each text with the character at `i` (a position of a non-separator)
/// replaced by each other alphabet character, and by `U` (not in the
/// alphabet): `(position, replacement, text)`.
fn substitutions(text: &str, sep: char) -> Vec<(usize, char, String)> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    for (i, c) in chars.iter().enumerate() {
        if *c == sep {
            continue;
        }
        for r in ALPHABET.iter().map(|&b| char::from(b)).chain(['U']) {
            if r != *c {
                let mut t = chars.clone();
                t[i] = r;
                out.push((i, r, t.into_iter().collect()));
            }
        }
    }
    out
}

/// The sheet of a fixed seed, computed here from §18.1's words: the 52
/// characters of the seed and the first 20 bits of
/// `H(ctx("sheet-check") ‖ seed)`, in groups of 4 joined by spaces.
fn sheet_of(seed: &[u8; 32]) -> String {
    let d: [u8; 32] = <sha2::Sha256 as sha2::Digest>::digest([ctx::SHEET_CHECK, &seed[..]].concat()).into();
    let v = (u32::from(d[0]) << 12) | (u32::from(d[1]) << 4) | (u32::from(d[2]) >> 4);
    let mut chars = base32::encode(seed);
    for shift in [15, 10, 5, 0] {
        chars.push(char::from(ALPHABET[((v >> shift) & 31) as usize]));
    }
    let groups: Vec<String> = chars.as_bytes().chunks(4).map(|g| String::from_utf8(g.to_vec()).unwrap()).collect();
    groups.join(" ")
}

// Every character of a typed sheet replaced: a check character is always
// `seal/sheet_check`; a seed character is refused at step 2, 3 or 4 of
// §18.2, or (if the 20 bits happen to match) reads as another seed, whose
// recovery key is not the board's.
// spec: SEAL-SHEET-002
// spec: SEAL-SHEET-001
#[test]
fn every_character_of_a_sheet() {
    let seed = [0x5Au8, 0x01, 0xC3, 0x7E, 0x99, 0x10, 0x42, 0xAB, 0x00, 0xFF, 0x13, 0x37, 0x8D, 0x6E, 0x21, 0xF0, 0x0F, 0xE1,
        0x2D, 0x3C, 0x4B, 0x5A, 0x69, 0x78, 0x87, 0x96, 0xA5, 0xB4, 0xC3, 0xD2, 0xE1, 0xF0];
    let sheet = sheet_of(&seed);
    let good = RecoverySeed::from_sheet(&sheet).unwrap();
    assert_eq!(good.to_sheet().as_str(), sheet);
    let want_pk = good.keys().public().sign_pk;
    let mut other_seeds = 0;
    for (i, r, text) in substitutions(&sheet, ' ') {
        let check_char = i >= 65; // the last group (characters 52 to 55) starts at 13 × 5
        match RecoverySeed::from_sheet(&text) {
            Err(e) => {
                let want: &[SealError] = if r == 'U' {
                    &[SealError::SheetChar]
                } else if check_char {
                    &[SealError::SheetCheck]
                } else {
                    &[SealError::SheetCheck, SealError::SheetPadding]
                };
                assert!(want.contains(&e), "position {i} with {r}: {e:?}");
            }
            Ok(s) => {
                assert!(!check_char && r != 'U', "position {i} with {r} accepted");
                assert_ne!(s.keys().public().sign_pk, want_pk, "position {i} with {r}: the same seed");
                other_seeds += 1;
            }
        }
    }
    // At most a few of the 52 × 31 changes can match a 20-bit check.
    assert!(other_seeds <= 2, "{other_seeds}");
}

// Every character of a pairing code replaced and typed into N: a check
// character is always `seal/code_check` (N asks again, derives nothing,
// sends nothing); a secret character is `seal/code_check` or
// `seal/code_format`, or (if the 10 bits happen to match) names another
// ticket, and E's slot A then aborts N at the MAC.
// spec: SEAL-CODE-002
// spec: SEAL-PAIR-003
#[test]
fn every_character_of_a_pairing_code() {
    let r = Run::new(Sim::new(81, 2), 0);
    let event = |code: String| NewEvent::CodeEntered {
        code,
        host: HOST.into(),
        board_id: r.sim.board,
        door: DOOR,
        name: NAME.into(),
        now: PT0 + 1,
    };
    assert_eq!(r.code.len(), 22);
    let mut other_tickets = 0;
    for (i, c, text) in substitutions(&r.code, '-') {
        let check_char = i >= 20;
        assert!(parse_code(&text).is_err() || (!check_char && c != 'U'), "position {i} with {c}");
        let mut n = New::new();
        let fx = n.on(event(text), &mut OsEntropy);
        match &fx[..] {
            [NewEffect::AskAgain { code }] => {
                let want = if c == 'U' { SealError::CodeFormat } else { SealError::CodeCheck };
                assert!(*code == want || (*code == SealError::CodeFormat && !check_char), "position {i} with {c}: {code:?}");
            }
            [NewEffect::FetchSlot { tid, .. }] => {
                assert!(!check_char && c != 'U', "position {i} with {c} accepted");
                assert_ne!(*tid, r.k.tid, "position {i} with {c}: E's ticket");
                let fx = n.on(NewEvent::Slot { bytes: r.slot_a.clone() }, &mut OsEntropy);
                assert_eq!(n_abort(&fx), Some(SealError::SlotMac), "position {i} with {c}");
                other_tickets += 1;
            }
            other => panic!("position {i} with {c}: {:?}", other.iter().map(n_name).collect::<Vec<_>>()),
        }
    }
    // 16 × 31 changes against a 10-bit check: a handful may match.
    assert!(other_tickets <= 16, "{other_tickets}");
}

// Every character of a move code replaced: `seal/move_code`, always (the
// whole 40 bits are compared).
// spec: SEAL-FP-004
#[test]
fn every_character_of_a_move_code() {
    let board = BoardId([0x42; 16]);
    let hash = FrameHash([0x17; 32]);
    let code = move_code(&board, 900, &hash);
    assert!(check_move_code(&board, 900, &hash, &code).is_ok());
    let subs = substitutions(&code, '-');
    assert_eq!(subs.len(), 8 * 32);
    for (i, c, text) in subs {
        assert_eq!(check_move_code(&board, 900, &hash, &text), Err(SealError::MoveCode), "position {i} with {c}");
    }
}
