//! The `reset.json` (core.md §11.3) and `rebind.json` (core.md §7.5) vector
//! families (§18.2): every case of both files is checked.

use serde_json::Value;
use sha2::{Digest, Sha256};
use taskdeck_core::apply::reset;
use taskdeck_core::board::BoardData;
use taskdeck_core::canon::Record;
use taskdeck_core::command::parse_command;
use taskdeck_core::ids::{IdMinter, rebind};

const RESET: &str = include_str!("vectors/reset.json");
const REBIND: &str = include_str!("vectors/rebind.json");

fn family(text: &str, spec: &str) -> Vec<Value> {
    let v: Value = serde_json::from_str(text).expect("vector file is JSON");
    assert_eq!(v["spec"].as_str(), Some(spec), "the family names its section");
    let cases = v["cases"].as_array().expect("cases").clone();
    assert!(!cases.is_empty());
    cases
}

fn text<'a>(case: &'a Value, key: &str) -> &'a str {
    case[key].as_str().unwrap_or_else(|| panic!("{key}"))
}

fn sha(b: &[u8]) -> String {
    Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

/// A board from its canonical text; a board on a rules version this build
/// lacks fails `check` (rule 9), so it is read without it.
fn board(t: &str) -> BoardData {
    let b = BoardData::from_canonical(t.as_bytes()).expect("a canonical board");
    if b.rules == 1 {
        b.check().expect("a rules-1 board in a vector passes check");
    }
    b
}

fn minter(v: &Value) -> IdMinter {
    let n = |k: &str| v[k].as_u64().unwrap_or_else(|| panic!("{k}"));
    IdMinter { device_no: u16::try_from(n("device_no")).expect("u16"), next_item: n("next_item"), next_block: n("next_block") }
}

// spec: CORE-RULES-005
// spec: CORE-RULES-002
#[test]
fn reset_vectors() {
    for case in family(RESET, "core.md §11.3") {
        let name = text(&case, "name");
        let target = board(text(&case, "target_text"));
        let current = board(text(&case, "current_text"));
        let (t0, c0) = (target.clone(), current.clone());
        let got = reset(&target, &current);
        assert_eq!((target, current), (t0, c0), "{name}: reset leaves its inputs");
        match case.get("code") {
            Some(code) => assert_eq!(got.map(|_| ()).map_err(|e| e.code), Err(code.as_str().expect("code")), "{name}"),
            None => {
                let b = got.unwrap_or_else(|e| panic!("{name}: {e:?}"));
                let bytes = b.to_canonical_bytes();
                assert_eq!(String::from_utf8(bytes.clone()).expect("utf-8"), text(&case, "result_text"), "{name}");
                assert_eq!(sha(&bytes), text(&case, "result_sha256"), "{name}");
                b.check().expect("the reset board passes check");
                // A function of its arguments alone (CORE-CTX-002).
                assert_eq!(reset(&board(text(&case, "target_text")), &board(text(&case, "current_text"))), Ok(b));
            }
        }
    }
}

// spec: CORE-ID-006
#[test]
fn rebind_vectors() {
    for case in family(REBIND, "core.md §7.5") {
        let name = text(&case, "name");
        let commands: Vec<_> = case["commands"]
            .as_array()
            .expect("commands")
            .iter()
            .map(|c| parse_command(c.as_str().expect("command text").as_bytes()).unwrap_or_else(|e| panic!("{name}: {e:?}")))
            .collect();
        let from = u16::try_from(case["from_device_no"].as_u64().expect("from_device_no")).expect("u16");
        let view = board(text(&case, "view_text"));
        let before = minter(&case["minter"]);
        let mut m = before;
        let got = rebind(&commands, from, &view, &mut m);
        match case.get("code") {
            Some(code) => {
                assert_eq!(got.map(|_| ()).map_err(|e| e.code), Err(code.as_str().expect("code")), "{name}");
                assert_eq!(m, before, "{name}: the minter is unchanged on core/bad_id");
            }
            None => {
                let out = got.unwrap_or_else(|e| panic!("{name}: {e:?}"));
                let want: Vec<&str> =
                    case["expect_commands"].as_array().expect("expect_commands").iter().map(|c| c.as_str().expect("text")).collect();
                let texts: Vec<String> = out.iter().map(|c| String::from_utf8(c.to_canonical()).expect("utf-8")).collect();
                assert_eq!(texts, want, "{name}");
                // Every returned command parses (§7.5 step 3).
                for t in &texts {
                    assert!(parse_command(t.as_bytes()).is_ok(), "{name}: {t}");
                }
                assert_eq!(m, minter(&case["expect_minter"]), "{name}: the minter after");
            }
        }
    }
}
