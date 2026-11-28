//! `seal-chain.json` (seal.md §15): `C_0`, `C_s` over arbitrary frames (the
//! chain hashes any bytes), sync fingerprints, move codes and typed move
//! codes.

use crate::check_or_write;
use crate::common::hex;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use taskdeck_seal::{BoardId, Chain, FrameHash, Head, chain_step, chain_zero, check_move_code, move_code, sync_fingerprint};

fn chain(name: &str, board: BoardId, frames: &[(u64, Vec<u8>)]) -> Value {
    let c0 = chain_zero(&board);
    let mut prev = c0;
    let mut steps = vec![];
    for (seq, frame) in frames {
        let c = chain_step(&prev, *seq, frame);
        let fh = FrameHash(Sha256::digest(frame).into());
        steps.push(json!({
            "seq": seq,
            "frame": hex(frame),
            "frame_hash": hex(&fh.0),
            "chain": hex(&c.0),
            "fingerprint": sync_fingerprint(&board, &Head { seq: *seq, chain: c }).to_string(),
            "move_code": move_code(&board, *seq, &fh),
        }));
        prev = c;
    }
    json!({
        "name": name,
        "board_id": hex(&board.0),
        "c_0": hex(&c0.0),
        "fingerprint_0": sync_fingerprint(&board, &Head { seq: 0, chain: c0 }).to_string(),
        "steps": steps,
    })
}

fn typed(name: &str, board: BoardId, seq: u64, fh: FrameHash, text: &str) -> Value {
    let mut v = json!({
        "name": name,
        "board_id": hex(&board.0),
        "fence_seq": seq,
        "fence_hash": hex(&fh.0),
        "typed": text,
    });
    match check_move_code(&board, seq, &fh, text) {
        Ok(()) => v["result"] = json!("accepted"),
        Err(e) => v["error"] = json!(e.code()),
    }
    v
}

#[test]
fn seal_chain() {
    let board = BoardId([0x42; 16]);
    let frames: Vec<(u64, Vec<u8>)> = vec![
        (1, vec![]),
        (2, b"any bytes are a frame to the chain".to_vec()),
        (3, vec![0xff; 670]),
        (4, (0..=255).collect()),
    ];
    let mut cases = vec![
        chain("board 42.., four frames", board, &frames),
        chain("board 00.., one frame at seq 2^53-1", BoardId([0; 16]), &[(9_007_199_254_740_991, vec![0x01])]),
    ];
    let fh = FrameHash([0x77; 32]);
    let code = move_code(&board, 9, &fh);
    let plain: String = code.chars().filter(|&c| c != '-').collect();
    let looks: String = plain.to_lowercase().chars().map(|c| match c { '0' => 'o', '1' => 'l', c => c }).collect();
    let mut wrong: Vec<char> = plain.chars().collect();
    wrong[7] = if wrong[7] == '0' { '1' } else { '0' };
    let wrong: String = wrong.into_iter().collect();
    cases.extend([
        typed("move code as shown", board, 9, fh, &code),
        typed("move code lowercase, look-alikes, spaces", board, 9, fh, &format!(" {} {} ", &looks[..4], &looks[4..])),
        typed("move code of another seq", board, 10, fh, &code),
        typed("move code of another frame hash", board, 9, FrameHash([0x78; 32]), &code),
        typed("move code with the last character changed", board, 9, fh, &wrong),
        typed("move code of 7 characters", board, 9, fh, &plain[..7]),
        typed("move code with u", board, 9, fh, &format!("u{}", &plain[1..])),
    ]);
    // A head is a seq and a chain value; the fingerprint is defined on any.
    let any = Head { seq: 4096, chain: Chain([0x11; 32]) };
    cases.push(json!({
        "name": "sync fingerprint of an arbitrary head",
        "board_id": hex(&board.0),
        "seq": any.seq,
        "chain": hex(&any.chain.0),
        "fingerprint": sync_fingerprint(&board, &any).to_string(),
    }));
    check_or_write("seal-chain.json", json!({ "spec": "seal.md §15", "cases": cases }));
}
