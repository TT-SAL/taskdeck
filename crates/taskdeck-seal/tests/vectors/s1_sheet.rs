//! `seal-sheet.json` (seal.md §18): the sheet text of seeds, and typed
//! sheets with their seed or their error, one per step of §18.2.

use crate::common::hex;
use crate::{check_or_write, count};
use serde_json::{Value, json};
use taskdeck_seal::{FixedEntropy, RecoverySeed};

fn sheet(name: &str, seed: Vec<u8>) -> (Value, String) {
    let s = RecoverySeed::generate(&mut FixedEntropy::new(seed.clone())).unwrap();
    let text = s.to_sheet().to_string();
    (json!({ "name": name, "entropy": hex(&seed), "sheet": text }), text)
}

fn typed(name: &str, text: &str) -> Value {
    let mut v = json!({ "name": name, "typed": text });
    match RecoverySeed::from_sheet(text) {
        // The seed leaves the crate only as a sheet: its recovery signing key
        // and its sheet stand for it.
        Ok(s) => {
            v["sheet"] = json!(s.to_sheet().to_string());
            v["rsign_pk"] = json!(hex(&s.keys().public().sign_pk));
        }
        Err(e) => v["error"] = json!(e.code()),
    }
    v
}

#[test]
fn seal_sheet() {
    let (a, ta) = sheet("seed 00..1f", count(0, 32));
    let (b, _) = sheet("seed of 00", vec![0; 32]);
    let (c, _) = sheet("seed of ff", vec![0xff; 32]);
    let mut cases = vec![a, b, c];
    let plain: String = ta.chars().filter(|&c| c != ' ').collect();
    let looks: String = plain
        .chars()
        .map(|c| match c { '0' => 'o', '1' => 'L', c => c.to_ascii_lowercase() })
        .collect();
    let hyphens: String = plain.as_bytes().chunks(4).map(|g| std::str::from_utf8(g).unwrap()).collect::<Vec<_>>().join("-");
    cases.push(typed("typed as printed", &ta));
    cases.push(typed("typed without spaces", &plain));
    cases.push(typed("typed in lowercase with look-alikes and a tab", &format!("{}\t{}", &looks[..28], &looks[28..])));
    cases.push(typed("typed with hyphens", &hyphens));
    cases.push(typed("55 characters", &plain[..55]));
    cases.push(typed("57 characters", &format!("{plain}0")));
    cases.push(typed("a character u", &format!("u{}", &plain[1..])));
    cases.push(typed("a newline inside", &format!("{}\n{}", &plain[..28], &plain[29..])));
    // The 4 padding bits are the low bits of character 52.
    let mut pad: Vec<u8> = plain.clone().into_bytes();
    let alphabet = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let idx = alphabet.iter().position(|&a| a == pad[51]).unwrap();
    pad[51] = alphabet[idx | 1];
    cases.push(typed("padding bits not zero", &String::from_utf8(pad).unwrap()));
    let mut check: Vec<u8> = plain.clone().into_bytes();
    check[55] = if check[55] == b'0' { b'1' } else { b'0' };
    cases.push(typed("check character wrong", &String::from_utf8(check).unwrap()));
    let mut seed: Vec<u8> = plain.into_bytes();
    seed[0] = if seed[0] == b'0' { b'1' } else { b'0' };
    cases.push(typed("seed character wrong", &String::from_utf8(seed).unwrap()));
    check_or_write("seal-sheet.json", json!({ "spec": "seal.md §18", "cases": cases }));
}
