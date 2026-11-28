use super::*;

fn s(text: &str) -> Value {
    Value::String(text.to_owned())
}

// (a base64 field's decoded length is checked before decoding)
// spec: CORE-GEN-009
#[test]
fn base64_round_trip_and_only_the_canonical_text() {
    for len in 0..10usize {
        let bytes: Vec<u8> = (0..len).map(|k| u8::try_from(k * 37 % 256).unwrap_or(0)).collect();
        let text = base64_text(&bytes);
        assert_eq!(text.len(), len.div_ceil(3) * 4);
        if len >= 1 {
            assert_eq!(base64(&s(&text), 1, 64), Ok(bytes));
        }
    }
    assert_eq!(base64_text(b"\x01"), "AQ==");
    assert_eq!(base64_text(b"\x01\x02"), "AQI=");
    assert_eq!(base64_text(b"\xff"), "/w==");
    assert_eq!(base64_text(b"foobar"), "Zm9vYmFy");
    for bad in ["AR==", "AQJ=", "AQ", "AQ=", "A===", "====", "", "AQ==AQ==", "AQ=A", "A Q=", "-w=="] {
        assert_eq!(base64(&s(bad), 0, 64).map(|_| ()), Err(schema()), "{bad}");
    }
    // The length bound is applied to the decoded length.
    assert_eq!(base64(&s("AAAA"), 1, 2).map(|_| ()), Err(schema()));
    assert_eq!(base64(&s("AAA="), 1, 2), Ok(vec![0, 0]));
    assert_eq!(base64(&Value::from(1), 1, 2).map(|_| ()), Err(schema()));
}

#[test]
fn hex_is_lowercase_and_exact() {
    assert_eq!(hex::<2>(&s("00ff")), Ok([0, 255]));
    assert_eq!(hex_text(&[0, 255, 16]), "00ff10");
    for bad in ["00FF", "0ff", "00ff0", "zz00", ""] {
        assert_eq!(hex::<2>(&s(bad)), Err(schema()), "{bad}");
    }
}

// (unknown and missing fields)
// spec: CORE-ENC-005
#[test]
fn obj_finds_unknown_and_missing_fields() {
    let v: Value = serde_json::json!({"a": 1, "b": 2});
    let mut o = Obj::new(&v).unwrap_or_else(|_| panic!("object"));
    assert!(o.req("a").is_ok());
    assert_eq!(o.done().map(|_| ()), Err(schema()));
    let mut o = Obj::new(&v).unwrap_or_else(|_| panic!("object"));
    assert!(o.opt("a").is_some() && o.opt("b").is_some() && o.opt("c").is_none());
    assert_eq!(o.req("c").map(|_| ()), Err(schema()));
    assert_eq!(o.done(), Ok(()));
    assert!(Obj::new(&Value::from(1)).is_err());
}

// (range of integers)
// spec: CORE-ENC-005
#[test]
fn int_ranges_and_types() {
    assert_eq!(int(&Value::from(5), 0, 5), Ok(5));
    assert_eq!(int(&Value::from(6), 0, 5), Err(schema()));
    assert_eq!(int(&Value::from(1.0), 0, 5), Err(schema()));
    assert_eq!(int(&s("1"), 0, 5), Err(schema()));
    assert_eq!(u53(&Value::from(MAX_INT)), Ok(MAX_INT.unsigned_abs()));
    assert_eq!(u53(&Value::from(MAX_INT + 1)), Err(schema()));
    assert_eq!(u53(&Value::from(-1)), Err(schema()));
    assert_eq!(u53(&Value::from(u64::MAX)), Err(schema()));
}

#[test]
fn integers_are_written_in_decimal() {
    for (i, t) in [(0i64, "0"), (-1, "-1"), (10, "10"), (MAX_INT, "9007199254740991"), (-MAX_INT, "-9007199254740991")] {
        assert_eq!(encode(&Value::from(i)).as_deref(), Ok(t.as_bytes()));
    }
}

// (depth: a scalar is 0, `[]` is 1; 32 parse, 33 do not)
// spec: CORE-ENC-004
#[test]
fn depth_counts_containers() {
    let deep = |n: usize| format!("{}{}", "[".repeat(n), "]".repeat(n));
    assert!(parse(deep(32).as_bytes(), 1000).is_ok());
    assert_eq!(parse(deep(33).as_bytes(), 1000).map(|_| ()), Err(CoreError::code(NOT_JSON)));
    // Very deep input fails without exhausting the stack.
    assert_eq!(parse(deep(100_000).as_bytes(), 1_000_000).map(|_| ()), Err(CoreError::code(NOT_JSON)));
    assert!(parse(b"1", 10).is_ok());
}

// (step 1 comes first, )
// spec: CORE-ENC-004
// spec: CORE-GEN-009
#[test]
fn the_limit_is_checked_first() {
    assert_eq!(parse(b"\xff\xff", 1).map(|_| ()), Err(CoreError::code(TOO_LARGE)));
    assert_eq!(parse(b"", 0).map(|_| ()), Err(CoreError::code(NOT_JSON)));
    assert_eq!(record_from_canonical::<crate::board::Home>(b"{\"lat\":0,\"lon\":0}", 5), Err(CoreError::code(TOO_LARGE)));
}

// (step 4 before step 5 for a record)
// spec: CORE-ENC-004
#[test]
fn record_steps_in_order() {
    use crate::board::Home;
    assert_eq!(Home::from_canonical(b"{\"lat\":1,\"lon\":2}"), Ok(Home { lat: 1, lon: 2 }));
    assert_eq!(Home::from_canonical(b"{\"lon\":2,\"lat\":1}"), Err(CoreError::code(NOT_CANONICAL)));
    assert_eq!(Home::from_canonical(b"{\"lon\":2, \"lat\":1,\"x\":0}"), Err(schema()));
    assert_eq!(Home::from_canonical(b"{\"lat\":1,\"lon\":2,\"lat\":1}"), Err(schema()));
    assert_eq!(Home::from_canonical(b"{\"lat\":1,\"lon\":2"), Err(CoreError::code(NOT_JSON)));
    assert_eq!(Home { lat: -1, lon: 0 }.to_canonical(), b"{\"lat\":-1,\"lon\":0}".to_vec());
}

fn filled(head: &str, unit: &str, tail: &str, len: usize) -> String {
    let mut b = String::from(head);
    while b.len() + unit.len() + tail.len() <= len {
        b.push_str(unit);
    }
    b.push_str(tail);
    b
}

// (a tree that would take far more memory than its input is refused, not
// built: one-member objects are charged about 113 times their length)
// spec: CORE-GEN-007
#[test]
fn a_tree_over_its_memory_budget_is_schema_and_syntax_still_comes_first() {
    let limit = 4 << 20;
    let cheap = filled("[", "{\"\":0},", "0]", limit);
    assert_eq!(parse_value(cheap.as_bytes(), limit).map(|_| ()), Err(schema()));
    let nested = filled("[", &format!("{}0{},", "{\"\":".repeat(30), "}".repeat(30)), "0]", limit);
    assert_eq!(parse_value(nested.as_bytes(), limit).map(|_| ()), Err(schema()));
    // Past the budget the grammar is still read to its end (CORE-ENC-004).
    let broken = filled("[", "{\"\":0},", "0", limit);
    assert_eq!(parse_value(broken.as_bytes(), limit), Err(CoreError::code(NOT_JSON)));
    let escaped = filled("[", "\"\\u0041\",", "\"\\x\"]", limit);
    assert_eq!(parse_value(escaped.as_bytes(), limit), Err(CoreError::code(NOT_JSON)));
}

// (the budget and its charges are fixed numbers, the same on every platform)
// spec: CORE-ENC-004
#[test]
fn the_tree_budget_is_fixed_numbers() {
    assert_eq!(tree_budget(0), 1_048_576);
    assert_eq!(tree_budget(32_768), 1_048_576);
    assert_eq!(tree_budget(32_769), 1_048_608);
    assert_eq!(tree_budget(62_914_560), 2_013_265_920);
    assert_eq!(tree_budget(usize::MAX), usize::MAX);
    // `n` copies of `unit` in an array, then `tail` copies of `{}`.
    let list = |unit: &str, n: usize, tail: usize| {
        let mut v = vec![unit; n];
        v.extend(std::iter::repeat_n("{}", tail));
        format!("[{}]", v.join(","))
    };
    let at = |text: &str, limit: usize| {
        assert!(text.len() <= limit);
        parse_value(text.as_bytes(), limit).map(|_| ())
    };
    // Limit 40,000: budget 1,280,000.
    // Six members are two groups: 32 + 2 × 744 + 6 × (1 + 16) = 1,622 each;
    // 789 of them are 1,279,758, and seven `{}` (32 each, no group) 224 more.
    let six = r#"{"a":0,"b":0,"c":0,"d":0,"e":0,"f":0}"#;
    assert_eq!(at(&list(six, 789, 7), 40_000), Ok(()));
    assert_eq!(at(&list(six, 789, 8), 40_000), Err(schema()));
    assert_eq!(at(&list(six, 790, 0), 40_000), Err(schema()));
    // An escaped key is charged as written: `{"\n":0}` is 32 + 744 + 2 + 16 = 794;
    // 1,612 are 1,279,928, 1,613 are 1,280,722.
    assert_eq!(at(&list(r#"{"\n":0}"#, 1612, 0), 40_000), Ok(()));
    assert_eq!(at(&list(r#"{"\n":0}"#, 1613, 0), 40_000), Err(schema()));
    // Five members are one group: 32 + 744 + 5 × 17 = 861; 1,217 of them,
    // 38,945 bytes, are 1,047,837, within the budget of their limit.
    let five = r#"{"a":0,"b":0,"c":0,"d":0,"e":0}"#;
    let text = list(five, 1217, 0);
    assert_eq!(at(&text, text.len()), Ok(()));
}

// (the densest shapes a board holds stay under the budget at the limit)
// spec: CORE-GEN-007
#[test]
fn dense_valid_shapes_at_the_limit_fit_the_budget() {
    let limit = 4 << 20;
    for (head, unit, tail) in [
        ("[", "[0,0],", "[0,0]]"),
        ("[", "{\"block\":0,\"device_no\":0,\"item\":0},", "{}]"),
        ("[", "{\"estimate\":[0,0],\"levels\":[0,0],\"name\":[0,0],\"repeat\":[0,0],\"time\":[0,0]},", "{}]"),
        ("[", "\"a\\u0001\",", "\"\"]"),
        ("[", "0,", "0]"),
    ] {
        let b = filled(head, unit, tail, limit);
        assert!(parse(b.as_bytes(), limit).is_ok(), "{unit}");
    }
}
