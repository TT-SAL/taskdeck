//! Source scans for SEAL-GEN-001, -002, -003 and -005 (seal.md §3).
//!
//! The library's sources are every `.rs` file under `src/` except the unit
//! test files (`tests.rs`, `*_tests.rs` and anything under a `tests`
//! directory). Line comments (`//`, `///`, `//!`) are removed before a scan,
//! so that a comment naming a clock or a constant does not count.
//!
//! What these scans do not catch, and what covers it instead:
//! - code a dependency runs on the crate's behalf (a pinned crate that read
//!   a clock would not show here; the dependency set is checked against the
//!   list below, and `getrandom` is reached only through `OsEntropy`);
//! - forms the scan does not know: a path through a re-export
//!   (`some_crate::fs`), `extern crate std as x`, code a macro builds, or an
//!   item hidden by an unusual `cfg`;
//! - for SEAL-GEN-003, a primitive written without any of its tell-tale
//!   constants or rotations; the known-answer suites (`tests/kat`) and the
//!   published vectors, recomputed by the reviewers from the spec text, are
//!   the other half of that clause's proof;
//! - `env!` and `include_str!` are read at compile time, not at run time, and
//!   are not counted as reading the environment or the file system.

use std::path::{Path, PathBuf};

use crate::common::crate_dir;

/// The library's source files with their text, comments removed.
fn sources() -> Vec<(PathBuf, String)> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                if p.file_name().unwrap() != "tests" {
                    walk(&p, out);
                }
            } else if p.extension().is_some_and(|x| x == "rs") {
                let name = p.file_name().unwrap().to_str().unwrap();
                if name != "tests.rs" && !name.ends_with("_tests.rs") {
                    out.push(p);
                }
            }
        }
    }
    let mut files = Vec::new();
    walk(&crate_dir().join("src"), &mut files);
    files.sort();
    assert!(files.len() >= 20, "too few sources found: {files:?}");
    files
        .into_iter()
        .map(|p| {
            let text = std::fs::read_to_string(&p).unwrap();
            (p, strip_comments(&text))
        })
        .collect()
}

/// `text` without line comments, outside string literals (a `"` opens and
/// closes a string; `\"` inside one does not close it).
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let b = line.as_bytes();
        let mut in_str = false;
        let mut cut = b.len();
        let mut i = 0;
        while i < b.len() {
            match b[i] {
                b'\\' if in_str => i += 1,
                b'"' => in_str = !in_str,
                b'/' if !in_str && b.get(i + 1) == Some(&b'/') => {
                    cut = i;
                    break;
                }
                _ => {}
            }
            i += 1;
        }
        out.push_str(&line[..cut]);
        out.push('\n');
    }
    out
}

/// `text` with all whitespace removed, so that `use std :: time` and
/// `use std::time` look the same.
fn squeeze(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

/// The modules of `std` and the items that read a clock, the file system,
/// the network, the environment, a thread's or the process's state.
const STD_MODULES: [&str; 7] = ["time", "fs", "net", "env", "thread", "process", "io"];
const ITEMS: [&str; 10] = [
    "SystemTime",
    "Instant::now",
    "UNIX_EPOCH",
    "File::open",
    "File::create",
    "TcpStream",
    "UdpSocket",
    "thread::current",
    "#[thread_local]",
    "LocalKey",
];

/// Every place in one squeezed source that reads outside state at run time.
fn outside_state(squeezed: &str) -> Vec<String> {
    let mut found = Vec::new();
    for root in ["std::", "core::", "alloc::"] {
        for m in STD_MODULES {
            for tail in ["::", ";", ",", "}", "as"] {
                let pat = format!("{root}{m}{tail}");
                if squeezed.contains(&pat) {
                    found.push(pat);
                }
            }
        }
        // `use std::{fs, time::Instant}` and the like.
        let open = format!("{root}{{");
        let mut rest = squeezed;
        while let Some(i) = rest.find(&open) {
            let group = &rest[i + open.len()..];
            let end = group.find(';').unwrap_or(group.len());
            for name in group[..end].split([',', '{', '}', ':']) {
                if STD_MODULES.contains(&name) {
                    found.push(format!("{root}{{..{name}..}}"));
                }
            }
            rest = &rest[i + open.len()..];
        }
    }
    // `core::time` holds `Duration`, which reads no clock.
    found.retain(|f| !f.starts_with("core::time"));
    for item in ITEMS {
        if squeezed.contains(item) {
            found.push(item.to_string());
        }
    }
    for form in ["std::env::var", "env::var(", "env::args", "option_env!", "std::io::stdin"] {
        if squeezed.contains(form) {
            found.push(form.to_string());
        }
    }
    found.sort();
    found.dedup();
    found
}

/// Lines of `text` declaring thread-local state outside a test build: a
/// `thread_local!` whose previous non-blank line is not `#[cfg(test)]`.
fn thread_locals(text: &str) -> Vec<usize> {
    let lines: Vec<&str> = text.lines().collect();
    let mut bad = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if l.contains("thread_local!") {
            let prev = lines[..i].iter().rev().map(|l| l.trim()).find(|l| !l.is_empty());
            if prev != Some("#[cfg(test)]") {
                bad.push(i + 1);
            }
        }
    }
    bad
}

// The crate root declares `#![forbid(unsafe_code)]` as a line of its own,
// among the crate-level attributes before the first item, and no source
// of the library says `unsafe` or tries to allow it again.
// spec: SEAL-GEN-001
#[test]
fn the_root_forbids_unsafe_code() {
    let lib = std::fs::read_to_string(crate_dir().join("src/lib.rs")).unwrap();
    let code = strip_comments(&lib);
    let first_item = code
        .lines()
        .map(str::trim)
        .position(|l| !l.is_empty() && !l.starts_with("#!["))
        .unwrap();
    let forbid = code.lines().map(str::trim).position(|l| l == "#![forbid(unsafe_code)]");
    assert!(matches!(forbid, Some(i) if i < first_item), "no #![forbid(unsafe_code)] before the first item");
    for (p, text) in sources() {
        let s = squeeze(&text);
        assert!(!s.contains("unsafe{") && !s.contains("unsafefn") && !s.contains("unsafeimpl"), "{}", p.display());
        assert!(!s.contains("allow(unsafe_code") && !s.contains("expect(unsafe_code"), "{}", p.display());
    }
}

// No source of the library reads a clock, the file system, the network,
// the environment or thread-local state at run time: no path into those
// modules of `std`, none of the items that do it, and every `thread_local!`
// is compiled into test builds only. Every time value is therefore an
// argument. The scan itself finds each form in a made-up source (it can
// fail).
// spec: SEAL-GEN-002
#[test]
fn the_library_reads_no_outside_state() {
    for (p, text) in sources() {
        assert_eq!(outside_state(&squeeze(&text)), Vec::<String>::new(), "{}", p.display());
        assert_eq!(thread_locals(&text), Vec::<usize>::new(), "{}: thread_local! outside cfg(test)", p.display());
    }
    // The scan finds what it looks for.
    for bad in [
        "let t = std::time::SystemTime::now();",
        "use std::{collections::BTreeMap, fs};",
        "use std :: env;",
        "let v = std::env::var(\"X\");",
        "let n = std::thread::current();",
        "use std::net::TcpStream;",
        "use std::{io::Read};",
        "let t = Instant::now();",
    ] {
        assert!(!outside_state(&squeeze(bad)).is_empty(), "{bad}");
    }
    assert_eq!(thread_locals("thread_local! {}\n"), vec![1]);
    assert_eq!(thread_locals("#[cfg(test)]\nthread_local! {}\n"), Vec::<usize>::new());
    assert!(outside_state(&squeeze("let d = core::time::Duration::ZERO;")).is_empty());
}

/// Constants that only an implementation of one of SEAL-GEN-003's
/// primitives needs, written as they would appear in Rust source.
const PRIMITIVE_CONSTANTS: [(&str, &str); 12] = [
    ("SHA-256 round constant K[0]", "0x428a2f98"),
    ("SHA-256 initial H[0]", "0x6a09e667"),
    ("SHA-512 initial H[0]", "0x6a09e667f3bcc908"),
    ("ChaCha constant", "0x61707865"),
    ("ChaCha constant as text", "expand 32-byte k"),
    ("Poly1305 clamp", "0x0ffffffc"),
    ("X25519 a24", "121665"),
    ("X25519 a24 + 1", "121666"),
    ("2^255 - 19, low limb", "0x7ffffffffffed"),
    ("2^255 - 19 as hex", "7fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffed"),
    ("Ed25519 group order", "1000000000000000000000000000000014def9dea2f79cd65812631a5cf5d3ed"),
    ("Ed25519 d", "37095705934669439343138083508754565189542113879843219016388785533085940283555"),
];

/// The crate's dependencies: the pinned crates of SYNC.md §5.1 that
/// implement the primitives, and the others the crate's manifest declares
/// for what is not one of them.
const PRIMITIVE_CRATES: [&str; 6] = ["chacha20poly1305", "ed25519-dalek", "x25519-dalek", "hpke", "hkdf", "sha2"];
const OTHER_CRATES: [&str; 6] = ["getrandom", "zeroize", "subtle", "serde", "serde_json", "flate2"];

/// The names under `[dependencies]` of a manifest.
fn dependencies(manifest: &str) -> Vec<String> {
    let mut deps = Vec::new();
    let mut inside = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            inside = line == "[dependencies]";
            continue;
        }
        if inside && !line.is_empty() && !line.starts_with('#') {
            deps.push(line.split(['=', ' ']).next().unwrap().to_string());
        }
    }
    deps.sort();
    deps
}

// No source of the library holds a constant of SHA-256, SHA-512, ChaCha20,
// Poly1305, X25519 or Ed25519, nor the bit rotations their rounds are made
// of, and the crate depends on the pinned crates for every primitive and
// on nothing else that could provide one: SHA-256, HKDF, HMAC, AEAD,
// Ed25519, X25519 and HPKE come only from those crates. (The other half of
// the proof is the known-answer suites of `tests/kat` and the published
// vectors.) The scan finds each constant in a made-up source.
// spec: SEAL-GEN-003
#[test]
fn the_library_has_no_primitive_of_its_own() {
    for (p, text) in sources() {
        let lower = text.to_ascii_lowercase().replace('_', "");
        for (what, c) in PRIMITIVE_CONSTANTS {
            assert!(!lower.contains(&c.to_ascii_lowercase()), "{}: {what}", p.display());
        }
        let s = squeeze(&text);
        for op in [".rotate_left(", ".rotate_right(", "fnquarter_round", "fnpoly1305", "fnsha256"] {
            assert!(!s.contains(op), "{}: {op}", p.display());
        }
    }
    let manifest = std::fs::read_to_string(crate_dir().join("Cargo.toml")).unwrap();
    let mut want: Vec<String> = PRIMITIVE_CRATES.iter().chain(OTHER_CRATES.iter()).map(|s| s.to_string()).collect();
    want.sort();
    assert_eq!(dependencies(&manifest), want);
    // HMAC is the `hmac` crate that `hkdf` re-exports.
    let prim = std::fs::read_to_string(crate_dir().join("src/prim.rs")).unwrap();
    assert!(prim.contains("hkdf::hmac::"));
    // The scan can fail.
    let fake = "const K: [u32; 2] = [0x428A_2F98, 0x7137_4491];".to_ascii_lowercase().replace('_', "");
    assert!(fake.contains(PRIMITIVE_CONSTANTS[0].1));
    assert_eq!(dependencies("[dependencies]\na = 1\n# c\nb = { x = 1 }\n[dev-dependencies]\nc = 1\n"), ["a", "b"]);
}

// `Entropy` is sealed: its supertrait `Sealed` lives in a module private
// to the crate (`pub(crate) mod private` in `entropy.rs`, and `entropy` is
// no public module), so another crate cannot implement it (the
// `compile_fail` example on `Entropy` shows the error). Inside the crate,
// the only implementations are `OsEntropy`'s and, each behind
// `#[cfg(feature = "test-entropy")]`, `FixedEntropy`'s: a build without
// the feature has no other implementation.
// spec: SEAL-GEN-005
#[test]
fn entropy_is_sealed_and_only_os_entropy_without_the_feature() {
    let lib = strip_comments(&std::fs::read_to_string(crate_dir().join("src/lib.rs")).unwrap());
    assert!(lib.lines().any(|l| l.trim() == "mod entropy;"), "entropy must stay a private module");
    assert!(!lib.contains("pub mod entropy") && !lib.contains("private"), "nothing of entropy::private is re-exported");
    let entropy = strip_comments(&std::fs::read_to_string(crate_dir().join("src/entropy.rs")).unwrap());
    let s = squeeze(&entropy);
    assert!(s.contains("pub(crate)modprivate{") && s.contains("pubtraitSealed{}"));
    assert!(s.contains("pubtraitEntropy:private::Sealed{"));

    let mut impls = Vec::new();
    for (p, text) in sources() {
        let lines: Vec<&str> = text.lines().collect();
        for (i, l) in lines.iter().enumerate() {
            let sq = squeeze(l);
            for tr in ["Entropyfor", "Sealedfor"] {
                if sq.starts_with("impl") && sq.contains(tr) {
                    let ty = sq.split(tr).nth(1).unwrap().trim_end_matches(['{', '}']).to_string();
                    let prev = lines[..i].iter().rev().map(|l| l.trim()).find(|l| !l.is_empty()).unwrap_or("");
                    let gated = prev == "#[cfg(feature = \"test-entropy\")]";
                    impls.push((p.file_name().unwrap().to_str().unwrap().to_string(), tr, ty, gated));
                }
            }
        }
    }
    impls.sort();
    let want = |tr: &'static str, ty: &str, gated| ("entropy.rs".to_string(), tr, ty.to_string(), gated);
    assert_eq!(
        impls,
        vec![
            want("Entropyfor", "FixedEntropy", true),
            want("Entropyfor", "OsEntropy", false),
            want("Sealedfor", "FixedEntropy", true),
            want("Sealedfor", "OsEntropy", false),
        ]
    );
    // `FixedEntropy` itself exists only with the feature.
    assert!(entropy.contains("#[cfg(feature = \"test-entropy\")]\npub struct FixedEntropy"));
}
