//! Clauses a build proves (core.md §3): the crate's own files, read as text.

const LIB: &str = include_str!("../src/lib.rs");
const CI: &str = include_str!("../ci.sh");
const CLIPPY: &str = include_str!("../clippy.toml");
const MANIFEST: &str = include_str!("../Cargo.toml");
const CANON: &str = include_str!("../src/canon.rs");
const BOARD: &str = include_str!("../src/board.rs");
const IDS: &str = include_str!("../src/ids.rs");
const TIME: &str = include_str!("../src/time.rs");

// spec: CORE-GEN-001
#[test]
fn the_crate_forbids_unsafe_code() {
    assert!(LIB.lines().any(|l| l.trim() == "#![forbid(unsafe_code)]"));
}

// spec: CORE-GEN-002
// spec: CORE-GEN-003
#[test]
fn ci_builds_the_library_alone_for_wasm32() {
    let checks: Vec<&str> = CI.lines().map(str::trim).filter(|l| l.starts_with("cargo check")).collect();
    assert!(checks.iter().any(|l| l.contains("-p taskdeck-core") && l.contains("--lib") && !l.contains("--target")));
    assert!(checks.iter().any(|l| {
        l.contains("-p taskdeck-core") && l.contains("--lib") && l.contains("--target wasm32-unknown-unknown")
    }));
    // Without the dev-dependencies, so without chrono's `clock` (CORE-GEN-003).
    assert!(checks.iter().all(|l| !l.contains("--all-targets") && !l.contains("--tests")));
    assert!(CI.contains("set -euo pipefail"));
}

/// The `path` values of one list of `clippy.toml`.
fn clippy_paths(list: &str) -> Vec<&'static str> {
    let start = CLIPPY.find(&format!("{list} = [")).unwrap_or_else(|| panic!("{list} is listed"));
    let body = &CLIPPY[start..];
    let body = &body[..body.find("\n]").unwrap_or(body.len())];
    body.split("path = \"").skip(1).filter_map(|p| p.split('"').next()).collect()
}

// spec: CORE-GEN-004
// spec: CORE-GEN-003
#[test]
fn clippy_disallows_the_clock_and_the_local_zone() {
    let types = clippy_paths("disallowed-types");
    let methods = clippy_paths("disallowed-methods");
    for t in ["chrono::Local", "chrono::offset::Local"] {
        assert!(types.contains(&t), "{t}");
    }
    for m in ["chrono::Utc::now", "chrono::Local::now", "std::time::SystemTime::now", "std::time::Instant::now"] {
        assert!(methods.contains(&m), "{m}");
    }
    // Files, network, threads, environment and random sources (CORE-GEN-003).
    for m in ["std::fs::read", "std::net::TcpStream::connect", "std::thread::spawn", "std::env::var", "std::hash::RandomState::new"] {
        assert!(methods.contains(&m), "{m}");
    }
}

// spec: CORE-GEN-005
#[test]
fn exactly_three_dependencies() {
    // By lines, so that a checkout with CRLF line endings (Windows) reads the same.
    assert!(MANIFEST.lines().any(|l| l.trim() == "[dependencies]"), "a [dependencies] table");
    let mut names: Vec<&str> = MANIFEST
        .lines()
        .map(str::trim)
        .skip_while(|l| *l != "[dependencies]")
        .skip(1)
        .take_while(|l| !l.starts_with('['))
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            // At the workspace's pinned version.
            assert!(l.contains("workspace = true"), "{l}");
            l.split('=').next().unwrap_or("").trim()
        })
        .collect();
    names.sort_unstable();
    assert_eq!(names, ["chrono", "serde", "serde_json"]);
    assert!(!MANIFEST.contains("[target."), "no platform-specific dependencies");
}

// (the encoder, parser, check and id functions use no float and no hash collection)
// spec: CORE-GEN-006
#[test]
fn no_float_and_no_hash_collection() {
    for (file, text) in [("canon.rs", CANON), ("board.rs", BOARD), ("ids.rs", IDS), ("time.rs", TIME)] {
        for word in ["f32", "f64", "HashMap", "HashSet"] {
            assert!(!text.contains(word), "{file} mentions {word}");
        }
    }
}
