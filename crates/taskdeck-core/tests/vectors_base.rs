//! The vector families of task a-base (core.md §18.2): `canonical.json`,
//! `instants.json`, `planner.json`, `board-bytes.json`. Every case of every
//! file is checked.

use serde_json::Value;
use sha2::{Digest, Sha256};
use taskdeck_core::board::BoardData;
use taskdeck_core::canon;
use taskdeck_core::time::{self, FixedZone, Instant, LocalTime, TableZone, Zone};

const CANONICAL: &str = include_str!("vectors/canonical.json");
const INSTANTS: &str = include_str!("vectors/instants.json");
const PLANNER: &str = include_str!("vectors/planner.json");
const BOARD_BYTES: &str = include_str!("vectors/board-bytes.json");

fn family(text: &str, spec: &str) -> Value {
    let v: Value = serde_json::from_str(text).expect("vector file is JSON");
    assert_eq!(v["spec"].as_str(), Some(spec), "the family names its section");
    assert!(!v["cases"].as_array().expect("cases").is_empty());
    v
}

fn name(case: &Value) -> &str {
    case["name"].as_str().expect("every case has a name")
}

fn unhex(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2));
    (0..s.len()).step_by(2).map(|k| u8::from_str_radix(&s[k..k + 2], 16).expect("hex")).collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn instant(v: &Value) -> Instant {
    Instant::parse_canonical(v.as_str().expect("instant text")).expect("a valid instant")
}

fn zone(v: &Value) -> Box<dyn Zone> {
    if let Some(o) = v.get("fixed") {
        return Box::new(FixedZone(i32::try_from(o.as_i64().expect("offset")).expect("i32")));
    }
    let t = &v["table"];
    let initial = i32::try_from(t["initial"].as_i64().expect("initial")).expect("i32");
    let transitions = t["transitions"]
        .as_array()
        .expect("transitions")
        .iter()
        .map(|p| (p[0].as_i64().expect("utc_ms"), i32::try_from(p[1].as_i64().expect("offset")).expect("i32")))
        .collect();
    Box::new(TableZone::new(initial, transitions).expect("a valid table"))
}

fn date(v: &Value) -> chrono::NaiveDate {
    let a = v.as_array().expect("[y, m, d]");
    chrono::NaiveDate::from_ymd_opt(
        i32::try_from(a[0].as_i64().expect("y")).expect("i32"),
        u32::try_from(a[1].as_i64().expect("m")).expect("u32"),
        u32::try_from(a[2].as_i64().expect("d")).expect("u32"),
    )
    .expect("a date")
}

fn opt_text(v: &Value) -> Option<&str> {
    v.as_str()
}

// Every encode case and every parse case of canonical.json, the tree budget
// vectors of §4.5 included (charges worked by hand in their names' terms:
// a text charged exactly the budget, one unit over, and over then not JSON).
// spec: CORE-ENC-001
// spec: CORE-ENC-002
// spec: CORE-ENC-003
// spec: CORE-ENC-004
// spec: CORE-ENC-005
// spec: CORE-ENC-006
// spec: CORE-ENC-008
// spec: CORE-GEN-009
#[test]
fn canonical_vectors() {
    let v = family(CANONICAL, "core.md §4");
    let encode_cases = v["encode"].as_array().expect("encode cases");
    for case in encode_cases {
        let got = canon::encode(&case["value"]);
        match case.get("error") {
            Some(code) => assert_eq!(got.err().map(|e| e.code), code.as_str(), "{}", name(case)),
            None => {
                let want = case["enc_text"].as_str().expect("enc_text");
                assert_eq!(got.as_deref(), Ok(want.as_bytes()), "{}", name(case));
                // The canonical text parses back to the value.
                assert_eq!(canon::parse(want.as_bytes(), want.len()).as_ref(), Ok(&case["value"]), "{}", name(case));
            }
        }
    }
    for case in v["cases"].as_array().expect("cases") {
        let bytes = match (case.get("input_text"), case.get("input_hex")) {
            (Some(t), None) => t.as_str().expect("text").as_bytes().to_vec(),
            (None, Some(h)) => unhex(h.as_str().expect("hex")),
            _ => panic!("{}: one input", name(case)),
        };
        let limit = usize::try_from(case["limit"].as_u64().expect("limit")).expect("usize");
        let got = canon::parse(&bytes, limit);
        match case.get("error") {
            Some(code) => {
                assert_eq!(got.as_ref().err().map(|e| e.code), code.as_str(), "{}", name(case));
                assert_eq!(got.as_ref().err().and_then(|e| e.detail), None, "{}", name(case));
                assert!(!canon::is_canonical(&bytes, limit), "{}", name(case));
            }
            None => {
                assert_eq!(got.as_ref(), Ok(&case["value"]), "{}", name(case));
                assert!(canon::is_canonical(&bytes, limit), "{}", name(case));
            }
        }
    }
}

// spec: CORE-ENC-007
// spec: CORE-TIME-001
#[test]
fn instant_vectors() {
    let v = family(INSTANTS, "core.md §4.4, §5.1");
    for case in v["cases"].as_array().expect("cases") {
        let n = name(case);
        let want = opt_text(&case["text"]);
        if let Some(p) = case.get("parse") {
            let got = Instant::parse_canonical(p.as_str().expect("text"));
            match case["secs"].as_i64() {
                Some(secs) => {
                    let nanos = u32::try_from(case["nanos"].as_u64().expect("nanos")).expect("u32");
                    assert_eq!(got, Instant::from_parts(secs, nanos), "{n}");
                    assert!(got.is_some(), "{n}");
                    assert_eq!(got.map(|t| t.to_canonical()).as_deref(), p.as_str(), "{n}");
                }
                None => assert_eq!(got, None, "{n}"),
            }
        } else if let Some(ms) = case.get("at_ms") {
            let got = Instant::from_at_ms(ms.as_i64().expect("at_ms"));
            assert_eq!(got.map(|t| t.to_canonical()).as_deref(), want, "{n}");
        } else if let Some(r) = case.get("rfc3339") {
            let t = chrono::DateTime::parse_from_rfc3339(r.as_str().expect("rfc3339")).expect("rfc3339");
            let got = Instant::from_chrono(t.with_timezone(&chrono::Utc));
            assert_eq!(got.map(|t| t.to_canonical()).as_deref(), want, "{n}");
            if let Some(t) = got {
                assert_eq!(Instant::from_chrono(t.to_chrono()), Some(t), "{n}");
            }
        } else if let Some(s) = case.get("plus_seconds") {
            let got = instant(&case["at"]).plus_seconds(s.as_i64().expect("seconds"));
            assert_eq!(got.map(|t| t.to_canonical()).as_deref(), want, "{n}");
        } else if let Some(ms) = case.get("unix_ms") {
            assert_eq!(instant(&case["at"]).as_unix_ms(), ms.as_i64().expect("ms"), "{n}");
        } else {
            let secs = case["secs"].as_i64().expect("secs");
            let nanos = u32::try_from(case["nanos"].as_u64().expect("nanos")).expect("u32");
            let got = Instant::from_parts(secs, nanos);
            assert_eq!(got.map(|t| t.to_canonical()).as_deref(), want, "{n}");
            if let Some(t) = got {
                assert_eq!(t.to_canonical().len(), 30, "{n}");
                assert_eq!(Instant::parse_canonical(&t.to_canonical()), Some(t), "{n}");
            }
        }
    }
}

fn i64_of(v: &Value) -> i64 {
    v.as_i64().expect("integer")
}

// spec: CORE-TIME-002
// spec: CORE-TIME-003
// spec: CORE-TIME-004
// spec: CORE-TIME-005
#[test]
fn planner_vectors() {
    let v = family(PLANNER, "core.md §5");
    for case in v["cases"].as_array().expect("cases") {
        let n = name(case);
        let r = &case["result"];
        if let Some(m) = case.get("snap") {
            assert_eq!(i64::from(time::snap(i64_of(m))), i64_of(r), "{n}");
        } else if let Some(a) = case.get("clamp_block") {
            let (s, len) = time::clamp_block(i64_of(&a[0]), i64_of(&a[1]));
            assert_eq!((i64::from(s), i64::from(len)), (i64_of(&r[0]), i64_of(&r[1])), "{n}");
        } else if let Some(m) = case.get("snap_up") {
            let m = i32::try_from(i64_of(m)).expect("i32");
            assert_eq!(i64::from(time::snap_up(m)), i64_of(r), "{n}");
        } else if let Some(a) = case.get("resolve_on_day") {
            let z = zone(&a["zone"]);
            let m = i32::try_from(i64_of(&a["minutes"])).expect("i32");
            let got = time::resolve_on_day(date(&a["day"]), m, z.as_ref());
            assert_eq!(got.map(|t| t.to_canonical()).as_deref(), r.as_str(), "{n}");
        } else if let Some(a) = case.get("offset_at") {
            let z = zone(&a["zone"]);
            assert_eq!(i64::from(z.offset_at(instant(&a["at"]))), i64_of(r), "{n}");
        } else if let Some(a) = case.get("from_local") {
            let z = zone(&a["zone"]);
            let l = a["local"].as_array().expect("local");
            let local = date(&Value::Array(l[..3].to_vec()))
                .and_hms_opt(u32::try_from(i64_of(&l[3])).expect("h"), u32::try_from(i64_of(&l[4])).expect("m"), 0)
                .expect("time");
            let got: Vec<String> = match z.from_local(local) {
                LocalTime::None => vec![],
                LocalTime::Single(t) => vec![t.to_canonical()],
                LocalTime::Ambiguous(a, b) => {
                    assert!(a < b, "{n}: ascending");
                    vec![a.to_canonical(), b.to_canonical()]
                }
            };
            let want: Vec<String> =
                r.as_array().expect("list").iter().map(|t| t.as_str().expect("text").to_owned()).collect();
            assert_eq!(got, want, "{n}");
        } else if let Some(a) = case.get("local") {
            let z = zone(&a["zone"]);
            let t = instant(&a["at"]);
            assert_eq!(time::local_day(t, z.as_ref()), date(&case["day"]), "{n}");
            assert_eq!(i64::from(time::local_minutes(t, z.as_ref())), i64_of(&case["minutes"]), "{n}");
        } else if let Some(a) = case.get("now_marker") {
            let z = zone(&a["zone"]);
            let got = time::now_marker(date(&a["day"]), instant(&a["now"]), z.as_ref());
            assert_eq!(got.map(i64::from), r.as_i64(), "{n}");
        } else if let Some(a) = case.get("table_new") {
            let initial = i32::try_from(i64_of(&a["initial"])).expect("i32");
            let transitions = a["transitions"]
                .as_array()
                .expect("transitions")
                .iter()
                .map(|p| (i64_of(&p[0]), i32::try_from(i64_of(&p[1])).expect("i32")))
                .collect();
            let got = TableZone::new(initial, transitions);
            assert_eq!(got.is_ok(), case["ok"].as_bool().expect("ok"), "{n}");
            if let Err(e) = got {
                assert_eq!(e.code, "core/schema", "{n}");
            }
        } else {
            panic!("{n}: unknown case");
        }
    }
}

// spec: CORE-DATA-001
// spec: CORE-DATA-002
// spec: CORE-DATA-004
// spec: CORE-DATA-005
// spec: CORE-ID-001
// spec: CORE-ENC-002
#[test]
fn board_bytes_vectors() {
    let v = family(BOARD_BYTES, "core.md §6");
    let mut rules_seen = [false; 10];
    for case in v["cases"].as_array().expect("cases") {
        let n = name(case);
        let text = case["board_text"].as_str().expect("board_text");
        let got = BoardData::from_canonical_bytes(text.as_bytes());
        match case.get("error") {
            None => {
                let board = got.unwrap_or_else(|e| panic!("{n}: {e:?}"));
                assert_eq!(board.check(), Ok(()), "{n}");
                let again = board.to_canonical_bytes();
                assert_eq!(String::from_utf8_lossy(&again), text, "{n}: loading changes nothing");
                assert_eq!(hex(&Sha256::digest(&again)), case["board_sha256"].as_str().expect("sha"), "{n}");
            }
            Some(code) => {
                let e = got.err().unwrap_or_else(|| panic!("{n}: accepted"));
                assert_eq!(Some(e.code), code.as_str(), "{n}");
                let rule = case.get("rule").and_then(Value::as_u64).map(|r| u32::try_from(r).expect("u32"));
                assert_eq!(e.detail, rule, "{n}");
                if let Some(r) = rule {
                    rules_seen[usize::try_from(r).expect("usize")] = true;
                }
            }
        }
    }
    assert!(rules_seen[1..].iter().all(|&s| s), "one case per rule of §6.12");
}
