//! `seal-code.json` (seal.md §17.2, §5.2): codes drawn from given entropy,
//! and typed codes with case, look-alikes, separators and errors, each with
//! the `ticket_key` and `tid` N derives for a given board, or the error N
//! asks again with.

use serde_json::{Value, json};
use taskdeck_seal::pair::*;
use taskdeck_seal::*;

use crate::common::hex;
use crate::pairing::*;
use crate::{check_or_write, count};

const BOARD: BoardId = BoardId([0xB0; 16]);

fn drawn(name: &str, entropy: Vec<u8>) -> (Value, String) {
    let (_, code) = new_code(&mut FixedEntropy::new(entropy.clone())).unwrap();
    // The code text computed here from §17.2.
    let s: [u8; 10] = entropy.clone().try_into().unwrap();
    let plain = code_text(&s);
    assert_eq!(code.replace('-', ""), plain);
    (json!({ "name": name, "entropy": hex(&entropy), "code": code }), code)
}

fn typed(name: &str, text: &str) -> Value {
    let mut n = New::new();
    let ev = NewEvent::CodeEntered { code: text.into(), host: HOST.into(), board_id: BOARD, door: DOOR, name: NAME.into(), now: 0 };
    let fx = n.on(ev, &mut FixedEntropy::new(vec![]));
    let mut v = json!({ "name": name, "typed": text, "board_id": hex(&BOARD.0) });
    match (&fx[..], parse_code(text)) {
        ([NewEffect::FetchSlot { tid, ticket_key, label: 0x41 }], Ok(_)) => {
            v["ticket_key"] = json!(hex(&ticket_key[..]));
            v["tid"] = json!(hex(tid));
        }
        ([NewEffect::AskAgain { code }], Err(e)) => {
            assert_eq!(*code, e);
            v["error"] = json!(code.code());
        }
        _ => panic!("{name}: N and parse_code disagree"),
    }
    v
}

// spec: SEAL-CODE-001
// spec: SEAL-CODE-002
#[test]
fn seal_code() {
    let (a, code) = drawn("s = 00..09", count(0, 10));
    let (b, _) = drawn("s of 00", vec![0; 10]);
    let (c, _) = drawn("s of ff", vec![0xff; 10]);
    let mut cases = vec![a, b, c];
    let plain: String = code.chars().filter(|&c| c != '-').collect();
    let k = Code::from_secret(count(0, 10).try_into().unwrap(), &BOARD);
    let looks: String = plain
        .chars()
        .map(|c| match c {
            '0' => 'o',
            '1' => 'i',
            c => c.to_ascii_lowercase(),
        })
        .collect();
    let ok = [
        ("typed as displayed", code.clone()),
        ("typed without hyphens", plain.clone()),
        ("lowercase, look-alikes, spaces and a tab", format!(" {} {}\t{} ", &looks[..4], &looks[4..12], &looks[12..])),
    ];
    for (name, text) in ok {
        let v = typed(name, &text);
        assert_eq!(v["tid"], json!(hex(&k.tid)), "{name}");
        cases.push(v);
    }
    cases.push(typed("17 characters", &plain[..17]));
    cases.push(typed("19 characters", &format!("{plain}0")));
    cases.push(typed("a U", &format!("U{}", &plain[1..])));
    cases.push(typed("a newline inside", &format!("{}\n{}", &plain[..9], &plain[10..])));
    cases.push(typed("a character outside ASCII", &format!("{}\u{e9}{}", &plain[..9], &plain[10..])));
    let mut chk = plain.clone().into_bytes();
    chk[17] = if chk[17] == b'0' { b'1' } else { b'0' };
    cases.push(typed("check character wrong", &String::from_utf8(chk).unwrap()));
    let mut sec = plain.clone().into_bytes();
    sec[2] = if sec[2] == b'Z' { b'Y' } else { b'Z' };
    let v = typed("secret character wrong", &String::from_utf8(sec).unwrap());
    assert_eq!(v["error"], json!("seal/code_check"));
    cases.push(v);
    check_or_write("seal-code.json", json!({ "spec": "seal.md §17.2", "cases": cases }));
}
