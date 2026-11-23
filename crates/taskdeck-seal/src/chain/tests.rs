use super::*;
use sha2::{Digest, Sha256};

fn sha(parts: &[&[u8]]) -> [u8; 32] {
    let mut s = Sha256::new();
    for p in parts {
        s.update(p);
    }
    s.finalize().into()
}

/// Crockford base32 by hand, from §5.2's text.
fn b32(bytes: &[u8]) -> String {
    const A: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut bits: Vec<u8> = bytes.iter().flat_map(|b| (0..8).rev().map(move |k| (b >> k) & 1)).collect();
    while !bits.len().is_multiple_of(5) {
        bits.push(0);
    }
    bits.chunks(5).map(|c| char::from(A[c.iter().fold(0usize, |a, &b| (a << 1) | usize::from(b))])).collect()
}

const BOARD: BoardId = BoardId([0xB0; 16]);

fn spec_wordlist() -> Vec<u8> {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/sync/spec/wordlist.txt");
    std::fs::read(p).unwrap()
}

// The embedded list is the committed one: its SHA-256, 2048 lines, each a
// word and one 0x0A, byte for byte equal to docs/sync/spec/wordlist.txt.
// spec: SEAL-FP-002
#[test]
fn the_word_list_is_embedded_and_pinned() {
    let digest = sha(&[WORDLIST.as_bytes()]);
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(hex, "2f5eed53a4727b4bf8880d8f3f199efc90e58503646d9ff8eff3a2ed3b24dbda");
    assert_eq!(WORDLIST.lines().count(), 2048);
    assert_eq!(WORDLIST.bytes().filter(|&b| b == b'\n').count(), 2048);
    assert!(WORDLIST.ends_with('\n'));
    assert!(WORDLIST.lines().all(|w| (3..=8).contains(&w.len()) && w.bytes().all(|c| c.is_ascii_lowercase())));
    assert_eq!(WORDLIST.as_bytes(), spec_wordlist().as_slice());
    assert_eq!(word(0), "abandon");
    assert_eq!(word(2047), "zoo");
}

// spec: SEAL-CHAIN-001
// spec: SEAL-CHAIN-002
// spec: SEAL-CTX-001
#[test]
fn chain_follows_section_15_1() {
    let c0 = chain_zero(&BOARD);
    assert_eq!(c0.0, sha(&[b"taskdeck/v1/chain\0", &BOARD.0]));
    assert_eq!([b"taskdeck/v1/chain\0".as_slice(), &BOARD.0].concat().len(), 34);
    let mut prev = c0;
    // Any bytes are a frame: the chain never parses them.
    for (s, frame) in [(1u64, b"".as_slice()), (2, b"not a frame"), (u64::MAX, &[0xFF; 700])] {
        let next = chain_step(&prev, s, frame);
        let input = [b"taskdeck/v1/chain\0".as_slice(), &prev.0, &s.to_be_bytes(), &sha(&[frame])].concat();
        assert_eq!(input.len(), 90);
        assert_eq!(next.0, sha(&[&input]));
        assert_eq!(chain_step(&prev, s, frame), next);
        prev = next;
    }
}

// spec: SEAL-FP-001
// spec: SEAL-CTX-001
#[test]
fn sync_fingerprint_follows_section_15_3() {
    let list = String::from_utf8(spec_wordlist()).unwrap();
    let words: Vec<&str> = list.lines().collect();
    for (s, c) in [(0u64, [0u8; 32]), (4096, [0x11; 32]), (u64::MAX, [0xEE; 32])] {
        let head = Head { seq: s, chain: Chain(c) };
        let fp = sync_fingerprint(&BOARD, &head);
        let x = sha(&[b"taskdeck/v1/fingerprint\0", &BOARD.0, &s.to_be_bytes(), &c]);
        let bits: Vec<usize> = x.iter().flat_map(|b| (0..8).rev().map(move |k| usize::from((b >> k) & 1))).collect();
        let idx = |k: usize| bits[11 * k..11 * k + 11].iter().fold(0, |a, b| (a << 1) | b);
        assert_eq!(fp.seq, s);
        assert_eq!(fp.words, [words[idx(0)], words[idx(1)], words[idx(2)], words[idx(3)]]);
        assert_eq!(fp.to_string(), format!("{s} {} {} {} {}", fp.words[0], fp.words[1], fp.words[2], fp.words[3]));
    }
}

// spec: SEAL-FP-003
// spec: SEAL-ENC-001
#[test]
fn move_code_follows_section_15_4() {
    let fh = FrameHash([0x77; 32]);
    let code = move_code(&BOARD, 9, &fh);
    let mc = sha(&[b"taskdeck/v1/move\0", &BOARD.0, &9u64.to_be_bytes(), &fh.0]);
    let chars = b32(&mc[..5]);
    assert_eq!(chars.len(), 8);
    assert_eq!(code, format!("{}-{}", &chars[..4], &chars[4..]));
}

// The typed code must decode to exactly the 5 bytes of this Fence: typed
// variants are accepted; another seq, another hash, another board, a
// wrong length or a bad character are `seal/move_code`.
// spec: SEAL-FP-004
// spec: SEAL-ENC-002
#[test]
fn check_move_code_accepts_only_this_fence() {
    let fh = FrameHash([0x77; 32]);
    let code = move_code(&BOARD, 9, &fh);
    assert_eq!(check_move_code(&BOARD, 9, &fh, &code), Ok(()));
    let plain: String = code.chars().filter(|&c| c != '-').collect();
    assert_eq!(check_move_code(&BOARD, 9, &fh, &plain.to_lowercase()), Ok(()));
    assert_eq!(check_move_code(&BOARD, 9, &fh, &format!(" {} - {} ", &plain[..4], &plain[4..])), Ok(()));
    let looks: String = plain.chars().map(|c| match c { '0' => 'O', '1' => 'l', c => c }).collect();
    assert_eq!(check_move_code(&BOARD, 9, &fh, &looks), Ok(()));
    assert_eq!(check_move_code(&BOARD, 10, &fh, &code), Err(SealError::MoveCode));
    assert_eq!(check_move_code(&BOARD, 9, &FrameHash([0x78; 32]), &code), Err(SealError::MoveCode));
    assert_eq!(check_move_code(&BoardId([1; 16]), 9, &fh, &code), Err(SealError::MoveCode));
    assert_eq!(check_move_code(&BOARD, 9, &fh, &plain[..7]), Err(SealError::MoveCode));
    assert_eq!(check_move_code(&BOARD, 9, &fh, &format!("{plain}0")), Err(SealError::MoveCode));
    assert_eq!(check_move_code(&BOARD, 9, &fh, &format!("U{}", &plain[1..])), Err(SealError::MoveCode));
    assert_eq!(check_move_code(&BOARD, 9, &fh, ""), Err(SealError::MoveCode));
    let mut other: Vec<u8> = plain.into_bytes();
    other[7] = if other[7] == b'0' { b'1' } else { b'0' };
    assert_eq!(check_move_code(&BOARD, 9, &fh, &String::from_utf8(other).unwrap()), Err(SealError::MoveCode));
}

// spec: SEAL-KEY-016
// spec: SEAL-ENC-001
#[test]
fn key_fingerprint_follows_section_6_8() {
    let (s, x) = ([1u8; 32], [2u8; 32]);
    let fp = key_fingerprint(&s, &x);
    let chars = b32(&sha(&[b"taskdeck/v1/key-fp\0", &s, &x])[..10]);
    assert_eq!(chars.len(), 16);
    assert_eq!(fp, format!("{}-{}-{}-{}", &chars[..4], &chars[4..8], &chars[8..12], &chars[12..]));
    assert_eq!(crate::RecoveryPublic { sign_pk: s, hpke_pk: x }.fingerprint(), fp);
    let dp = crate::DevicePublic { dev: crate::DevId([0; 16]), sign_pk: s, hpke_pk: x };
    assert_eq!(dp.fingerprint(), fp);
    assert_ne!(key_fingerprint(&x, &s), fp);
}

// spec: ENG-HASH-001
#[test]
fn state_hash_is_sha256_of_the_86_bytes() {
    let board_hash = [0xAB; 32];
    let membership = b"canonical membership bytes";
    let input = [b"taskdeck/v1/state\0".as_slice(), &1u32.to_be_bytes(), &board_hash, &sha(&[membership])].concat();
    assert_eq!(input.len(), 86);
    assert_eq!(state_hash(1, &board_hash, membership), sha(&[&input]));
    assert_ne!(state_hash(2, &board_hash, membership), state_hash(1, &board_hash, membership));
}

// The move-code comparison is `ct_eq`.
// spec: SEAL-GEN-007
#[test]
fn the_move_code_comparison_is_constant_time() {
    let src = include_str!("../chain.rs");
    let start = src.find("pub fn check_move_code(").unwrap();
    let body = &src[start..start + src[start..].find("\n}\n").unwrap()];
    assert!(body.contains(".ct_eq("));
    assert!(!body.contains("== want") && !body.contains("want =="));
}
