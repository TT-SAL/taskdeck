use super::*;

// spec: SEAL-ENC-006
#[test]
fn hosts_that_match_section_5_4() {
    for h in [
        "a",
        "relay.example.com",
        "r-1.example.com",
        "1.2.3.4",
        "xn--bcher-kva.example",
        "example.com:8443",
        "example.com:1",
        "example.com:65535",
        "example.com:442",
        "example.com:444",
    ] {
        assert!(host_is_valid(h), "{h}");
    }
    let label63 = "a".repeat(63);
    assert!(host_is_valid(&label63));
    // 253 bytes before the colon: four labels of 63 and one of 1.
    let name253 = format!("{0}.{0}.{0}.{1}", "a".repeat(63), "a".repeat(61));
    assert_eq!(name253.len(), 253);
    assert!(host_is_valid(&name253));
    assert!(host_is_valid(&format!("{name253}:1")));
    assert!(host_is_valid(&format!("{name253}:9")));
    assert_eq!(format!("{name253}:65535").len(), 259);
}

// spec: SEAL-ENC-006
#[test]
fn hosts_that_do_not() {
    let name254 = format!("{0}.{0}.{0}.{1}", "a".repeat(63), "a".repeat(62));
    assert_eq!(name254.len(), 254);
    for h in [
        String::new(),
        "Example.com".into(),
        "example.com.".into(),
        ".example.com".into(),
        "a..b".into(),
        "-a.com".into(),
        "a-.com".into(),
        "a.-b".into(),
        "a_b.com".into(),
        "a b".into(),
        "ä.com".into(),
        "https://a.com".into(),
        "a.com/".into(),
        "a.com:".into(),
        "a.com:443".into(),
        "a.com:0".into(),
        "a.com:080".into(),
        "a.com:65536".into(),
        "a.com:123456".into(),
        "a.com:+80".into(),
        "a.com:80:81".into(),
        ":80".into(),
        "[::1]".into(),
        "a".repeat(64),
        name254.clone(),
        format!("{name254}:1"),
    ] {
        assert!(!host_is_valid(&h), "{h}");
        assert_eq!(relay_origin(&h), Err(SealError::HostFormat), "{h}");
    }
}

// The relay origin is "https://" and the host, nothing else: no default
// port, no trailing slash, at most 263 bytes.
// spec: SEAL-ENC-005
// spec: SEAL-ENC-006
#[test]
fn relay_origin_is_the_https_serialisation() {
    assert_eq!(relay_origin("relay.example.com").unwrap(), "https://relay.example.com");
    assert_eq!(relay_origin("relay.example.com:8443").unwrap(), "https://relay.example.com:8443");
    let name253 = format!("{0}.{0}.{0}.{1}", "a".repeat(63), "a".repeat(61));
    let host = format!("{name253}:1");
    assert!(host.len() <= HOST_MAX);
    assert!(relay_origin(&host).unwrap().len() <= 263);
    assert_eq!(relay_origin("relay.example.com:443"), Err(SealError::HostFormat));
}

// spec: SEAL-ENC-007
#[test]
fn device_names() {
    assert!(device_name_is_valid(b"x"));
    assert!(device_name_is_valid("Pixel 8 \u{00E9}\u{1F600}".as_bytes()));
    assert!(device_name_is_valid(&[b'a'; 64]));
    assert!(device_name_is_valid("\u{00A0}".as_bytes()));
    assert!(!device_name_is_valid(b""));
    assert!(!device_name_is_valid(&[b'a'; 65]));
    // 63 bytes and a 2-byte character: 65 bytes.
    let mut long = vec![b'a'; 63];
    long.extend_from_slice("é".as_bytes());
    assert!(!device_name_is_valid(&long));
    for c in ['\u{0000}', '\u{0009}', '\u{000A}', '\u{001F}', '\u{007F}', '\u{0080}', '\u{009F}'] {
        let name = format!("a{c}b");
        assert!(!device_name_is_valid(name.as_bytes()), "{:?}", c);
    }
    assert!(device_name_is_valid("a\u{00A0}b".as_bytes()));
    assert!(!device_name_is_valid(&[0xC3]));
    assert!(!device_name_is_valid(&[b'a', 0xFF]));
    assert!(!device_name_is_valid(&[0xED, 0xA0, 0x80]));
}
