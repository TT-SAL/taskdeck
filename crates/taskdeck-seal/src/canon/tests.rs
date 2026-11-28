use super::*;
use proptest::prelude::*;
use serde_json::json;

fn p(s: &str) -> Result<Value, JsonError> {
    parse(s.as_bytes())
}

// The encoder writes core.md §4.2: no whitespace, members sorted by their
// keys' bytes (a proper prefix first, whatever the map's order), the escapes
// of rule 4 and nothing else, integers in plain decimal.
// spec: SEAL-ENC-003
#[test]
fn the_encoder_writes_core_4_2() {
    let mut m = Map::new();
    // Inserted out of order; "é" (c3 a9) sorts after "z" as bytes.
    for k in ["z", "é", "a", "ab", "B", "a\u{0}"] {
        m.insert(k.into(), json!(1));
    }
    let v = json!({ "o": Value::Object(m), "arr": [true, false, 0, 9007199254740991u64, -9007199254740991i64, -5] });
    assert_eq!(
        String::from_utf8(encode(&v).unwrap()).unwrap(),
        "{\"arr\":[true,false,0,9007199254740991,-9007199254740991,-5],\"o\":{\"B\":1,\"a\":1,\"a\\u0000\":1,\"ab\":1,\"z\":1,\"é\":1}}"
    );
    let s = json!("\"\\\u{08}\t\n\u{0C}\r\u{01}\u{1f}/\u{7f}é\u{10FFFF}");
    assert_eq!(
        String::from_utf8(encode(&s).unwrap()).unwrap(),
        "\"\\\"\\\\\\b\\t\\n\\f\\r\\u0001\\u001f/\u{7f}é\u{10FFFF}\""
    );
    // What serde_json's compact serializer writes, as core.md §4.2 notes.
    assert_eq!(encode(&s).unwrap(), serde_json::to_vec(&s).unwrap());
}

// The encoder has no bytes for null, a float or an integer beyond ±(2^53 − 1).
// spec: SEAL-ENC-004
#[test]
fn the_encoder_refuses_what_has_no_encoding() {
    assert_eq!(encode(&json!(null)), Err(JsonError::Null));
    assert_eq!(encode(&json!([1.5])), Err(JsonError::Float));
    assert_eq!(encode(&json!(9007199254740992u64)), Err(JsonError::IntRange));
    assert_eq!(encode(&json!(-9007199254740992i64)), Err(JsonError::IntRange));
    assert_eq!(int_value(INT_MAX + 1), Err(JsonError::IntRange));
}

// The canonical bytes parse back to the value; anything else that is valid
// JSON for the same value is not canonical (SEAL-ENC-003).
// spec: SEAL-ENC-003
#[test]
fn only_the_canonical_bytes_parse() {
    let good = r#"{"a":[1,"x\n"],"b":{"c":true}}"#;
    assert_eq!(p(good).unwrap(), json!({"a": [1, "x\n"], "b": {"c": true}}));
    for bad in [
        r#"{"b":{"c":true},"a":[1,"x\n"]}"#,   // key order
        r#"{"a":[1,"x\n"], "b":{"c":true}}"#,  // whitespace
        " {\"a\":1}",                          // leading whitespace
        "{\"a\":1}\n",                         // trailing whitespace
        r#"{"a":"\u0041"}"#,                   // an escape of a character above U+001F
        r#"{"a":"\/"}"#,                       // an escape of "/"
        r#"{"a":"\u000a"}"#,                   // \u000a for \n
        r#"{"a":"\u001F"}"#,                   // uppercase hex in an escape
        r#"{"a":-0}"#,                         // -0
    ] {
        assert_eq!(p(bad), Err(JsonError::NotCanonical), "{bad}");
    }
}

// Each rejection of SEAL-ENC-004 the parser itself makes, by name; and
// text that is not JSON at all.
// spec: SEAL-ENC-004
#[test]
fn parser_rejections_by_name() {
    assert_eq!(p(r#"{"a":1,"a":1}"#), Err(JsonError::DuplicateKey));
    assert_eq!(p(r#"{"a":{"b":1,"b":2}}"#), Err(JsonError::DuplicateKey));
    assert_eq!(p(r#"{"a":null}"#), Err(JsonError::Null));
    assert_eq!(p("null"), Err(JsonError::Null));
    for f in ["1.0", "1e3", "1E3", "-2.5", "0.0", "1e-1"] {
        assert_eq!(p(&format!("{{\"a\":{f}}}")), Err(JsonError::Float), "{f}");
    }
    assert_eq!(p(r#"{"a":9007199254740992}"#), Err(JsonError::IntRange));
    assert_eq!(p(r#"{"a":-9007199254740992}"#), Err(JsonError::IntRange));
    assert_eq!(p(r#"{"a":123456789012345678901234567890}"#), Err(JsonError::IntRange));
    assert_eq!(p(r#"{"a":9007199254740991}"#).unwrap(), json!({"a": 9007199254740991u64}));
    for bad in [
        "", "{", "}", "{\"a\"}", "{\"a\":}", "[1,]", "{\"a\":1,}", "01", "+1", "-", "1.", "1e", "\"\u{1}\"", "tru",
        "\"abc", "\"\\x\"", "\"\\ud800\"", "\"\\udc00\"", "\"\\ud800\\u0041\"", "{1:2}", "[1 2]", "{\"a\":1}x",
    ] {
        assert_eq!(p(bad), Err(JsonError::NotJson), "{bad:?}");
    }
    // Not UTF-8.
    assert_eq!(parse(b"\"\xff\""), Err(JsonError::NotJson));
    assert_eq!(parse(b"\"\xed\xa0\x80\""), Err(JsonError::NotJson));
}

// Nesting depth at most 32 (core.md §4.5 step 2).
// spec: SEAL-ENC-004
#[test]
fn depth_32_is_the_limit() {
    let nest = |d: usize| format!("{}{}", "[".repeat(d), "]".repeat(d));
    assert!(p(&nest(32)).is_ok());
    assert_eq!(p(&nest(33)), Err(JsonError::TooDeep));
    let objs = |d: usize| format!("{}1{}", "{\"a\":".repeat(d), "}".repeat(d));
    assert!(p(&objs(32)).is_ok());
    assert_eq!(p(&objs(33)), Err(JsonError::TooDeep));
    // Deep input fails without exhausting the stack.
    assert_eq!(p(&nest(100_000)), Err(JsonError::TooDeep));
}

// The schema helpers name the remaining rejections of SEAL-ENC-004:
// unknown and missing fields, wrong types, integers out of the stated range,
// bad hex, and array counts.
// spec: SEAL-ENC-004
#[test]
fn schema_rejections_by_name() {
    let v = json!({"a": 1, "b": "00ff", "c": [1, 2]});
    let mut f = Fields::of(&v).unwrap();
    assert_eq!(int(f.req("a").unwrap(), 1, 1), Ok(1));
    assert_eq!(f.req("zz").err(), Some(JsonError::MissingField));
    assert_eq!(f.finish(), Err(JsonError::UnknownField));
    let mut f = Fields::of(&v).unwrap();
    f.req("a").unwrap();
    f.req("b").unwrap();
    f.opt("c").unwrap();
    assert_eq!(f.finish(), Ok(()));
    assert_eq!(Fields::of(&json!([])).err(), Some(JsonError::WrongType));

    assert_eq!(int(&json!(5), 6, 9), Err(JsonError::IntRange));
    assert_eq!(int(&json!(10), 6, 9), Err(JsonError::IntRange));
    assert_eq!(int(&json!(-1), 0, 9), Err(JsonError::IntRange));
    assert_eq!(int(&json!("5"), 0, 9), Err(JsonError::WrongType));
    assert_eq!(int(&json!(5.0), 0, 9), Err(JsonError::Float));
    assert_eq!(int32(&json!(4294967295u64), 0, u32::MAX), Ok(u32::MAX));

    assert_eq!(hex::<2>(&json!("00ff")), Ok([0, 255]));
    assert_eq!(hex::<2>(&json!("00FF")), Err(JsonError::BadHex));
    assert_eq!(hex::<2>(&json!("00f")), Err(JsonError::BadHex));
    assert_eq!(hex::<2>(&json!("00ff00")), Err(JsonError::BadHex));
    assert_eq!(hex::<2>(&json!("00fg")), Err(JsonError::BadHex));
    assert_eq!(hex::<2>(&json!(12)), Err(JsonError::WrongType));
    assert_eq!(hex_value(&[0xab, 0x01]), json!("ab01"));

    assert_eq!(array(&json!([1, 2]), 1, 2).map(<[Value]>::len), Ok(2));
    assert_eq!(array(&json!([1, 2, 3]), 1, 2).err(), Some(JsonError::ArrayCount));
    assert_eq!(array(&json!([]), 1, 2).err(), Some(JsonError::ArrayCount));
    assert_eq!(array(&json!({}), 1, 2).err(), Some(JsonError::WrongType));
    assert_eq!(string(&json!(1)).err(), Some(JsonError::WrongType));
}

fn value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        any::<bool>().prop_map(Value::Bool),
        (0..=INT_MAX).prop_map(|u| json!(u)),
        (-(INT_MAX as i64)..0).prop_map(|i| json!(i)),
        ".{0,8}".prop_map(Value::String),
    ];
    leaf.prop_recursive(4, 32, 6, |inner| {
        prop_oneof![
            proptest::collection::vec(inner.clone(), 0..6).prop_map(Value::Array),
            proptest::collection::btree_map(".{0,4}", inner, 0..6).prop_map(|m| Value::Object(m.into_iter().collect())),
        ]
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]

    // Every value's canonical bytes parse back to it.
    // spec: SEAL-ENC-003
    #[test]
    fn encode_then_parse(v in value()) {
        let bytes = encode(&v).unwrap();
        prop_assert_eq!(parse(&bytes).unwrap(), v);
    }

    // Random and mutated bytes: the parser returns, and what it accepts is
    // canonical.
    // spec: SEAL-GEN-011
    // spec: SEAL-ENC-003
    #[test]
    fn parse_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..64), v in value(), at in any::<usize>(), b in any::<u8>()) {
        if let Ok(x) = parse(&bytes) {
            prop_assert_eq!(encode(&x).unwrap(), bytes);
        }
        let mut m = encode(&v).unwrap();
        if !m.is_empty() {
            let i = at % m.len();
            m[i] = b;
        }
        if let Ok(x) = parse(&m) {
            prop_assert_eq!(encode(&x).unwrap(), m);
        }
    }
}
