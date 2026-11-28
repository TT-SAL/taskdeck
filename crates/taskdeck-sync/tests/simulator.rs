//! The simulator's gate (engine.md §31, SYNC_PLAN.md §6.5; `scripts/gates/engine.gate`).
//!
//! - `seeds_hold_every_invariant`: seeds `base..base + N` of the gate
//!   configuration (or `TD_SIM_CONFIG`), every invariant of §31.7 checked at
//!   the end of every run (ENG-SIM-002); `N` is `SIM_SEEDS` (50 when unset),
//!   `base` is `TD_SIM_BASE` when set, else the first 16 hex digits of the
//!   commit id read as a `u64` (ENG-SIM-004). It prints `base`, every failing
//!   seed with its first five violations (every one with `TD_SIM_ALL=1`)
//!   and, for each, the JSON of its regression file (§31.8, ENG-SIM-005).
//! - `same_seed_same_trace`: ENG-SIM-003.
//! - `one_seed`: `TD_SIM_SEED=<seed> cargo test -p taskdeck-sync --test
//!   simulator one_seed -- --nocapture` prints that seed's trace up to its
//!   first violation (`TD_SIM_CONFIG=<name>` chooses a named configuration);
//!   without `TD_SIM_SEED` it runs seed 1 of the quiet configuration.
//!
//! The model runtime under every run performs the runtime's duties the engine
//! relies on: one serial persist queue with `Persisted` in effect order
//! (ENG-PERSIST-001), durable storage of exactly the confirmed writes plus a
//! crashed write in progress when kept (ENG-PERSIST-004), and every other
//! effect in emission order without waiting for a `Persist` (ENG-EFFECT-001).
//! The invariants of §31.7 lean on ENG-SESS-005 and ENG-PERSIST-005 (no
//! deletion on a relay's word, invariant 5), ENG-STATE-001 (records stay until
//! applied, invariants 2 and 3) and §21.1's alarms (invariant 4).

use taskdeck_sync::sim::{self, SimConfig, SimReport};

/// What the gate reads from its environment: `SIM_SEEDS`, `TD_SIM_BASE`,
/// `TD_SIM_CONFIG`, `TD_SIM_SEED`, `TD_SIM_FULL`, `TD_SIM_ALL`, and the
/// commit id from `.git`.
struct Env {
    seeds: u64,
    base: u64,
    base_from: String,
    config: String,
    seed: Option<u64>,
    /// `TD_SIM_FULL=1`: print the whole trace, not only up to the first violation.
    full: bool,
    /// `TD_SIM_ALL=1`: `seeds_hold_every_invariant` prints every violation of
    /// every failing seed, not only the first five (task D5).
    all: bool,
}

/// The one place a test of this crate reads the environment and files: the
/// gate's seed range and configuration (ENG-SIM-004) come from the variables
/// CI sets and from the commit id, read from `.git` by hand (no process is
/// spawned). The crate's `clippy.toml` disallows these calls everywhere else
/// (ENG-GEN-003).
#[allow(clippy::disallowed_methods)]
fn env() -> Env {
    let var = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
    let seeds = var("SIM_SEEDS").and_then(|v| v.trim().parse().ok()).unwrap_or(50);
    let config = var("TD_SIM_CONFIG").unwrap_or_else(|| "gate".to_owned());
    let seed = var("TD_SIM_SEED").and_then(|v| v.trim().parse().ok());
    let full = var("TD_SIM_FULL").is_some();
    let all = var("TD_SIM_ALL").is_some();
    if let Some(b) = var("TD_SIM_BASE").and_then(|v| v.trim().parse().ok()) {
        return Env { seeds, base: b, base_from: "TD_SIM_BASE".to_owned(), config, seed, full, all };
    }
    // The commit id: walk up from the crate to `.git` (a directory, or a
    // worktree's file naming its git directory), follow HEAD.
    let read = |p: &std::path::Path| std::fs::read_to_string(p).ok();
    let mut dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let commit = loop {
        let dot = dir.join(".git");
        if let Some(text) = read(&dot) {
            // A worktree: "gitdir: <path>".
            let gd = std::path::PathBuf::from(text.trim().trim_start_matches("gitdir:").trim());
            break head_commit(&gd, &read);
        }
        if std::fs::read_dir(&dot).is_ok() {
            break head_commit(&dot, &read);
        }
        if !dir.pop() {
            break None;
        }
    };
    match commit.and_then(|c| c.get(..16).and_then(|h| u64::from_str_radix(h, 16).ok())) {
        Some(b) => Env { seeds, base: b, base_from: "the commit id".to_owned(), config, seed, full, all },
        None => Env { seeds, base: 0, base_from: "no commit id found".to_owned(), config, seed, full, all },
    }
}

fn head_commit(gitdir: &std::path::Path, read: &dyn Fn(&std::path::Path) -> Option<String>) -> Option<String> {
    let common = read(&gitdir.join("commondir"))
        .map(|c| gitdir.join(c.trim()))
        .unwrap_or_else(|| gitdir.to_path_buf());
    let head = read(&gitdir.join("HEAD"))?;
    let head = head.trim();
    let Some(r) = head.strip_prefix("ref:") else { return Some(head.to_owned()) };
    let r = r.trim();
    for d in [gitdir, common.as_path()] {
        if let Some(c) = read(&d.join(r)) {
            return Some(c.trim().to_owned());
        }
    }
    let packed = read(&common.join("packed-refs"))?;
    packed.lines().find_map(|l| l.strip_suffix(r).map(|h| h.trim().to_owned()))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The JSON of a regression file for a failing run (engine.md §31.8).
fn regression_json(name: &str, config: &SimConfig, r: &SimReport) -> String {
    let v = &r.violations[0];
    format!(
        "{{\"spec\": \"engine.md §31\", \"cases\": [{{\"name\": \"{name} seed {seed}\", \"seed\": {seed}, \"config\": {cfg}, \"invariant\": {inv}, \"trace_sha256\": \"{sha}\", \"first_bad_line\": {line}}}]}}",
        seed = r.seed,
        cfg = config.to_json(),
        inv = v.invariant,
        sha = hex(&r.trace_sha256),
        line = v.line,
    )
}

// ENG-SIM-002, ENG-SIM-004, ENG-SIM-005: every seed of the range holds the six
// invariants; base and failing seeds are printed, each with its regression file.
#[test]
fn seeds_hold_every_invariant() {
    let e = env();
    let config = SimConfig::named(&e.config).unwrap_or_else(|| panic!("no configuration named {}", e.config));
    println!("simulator: base = {} ({}), seeds = {}, configuration = {}", e.base, e.base_from, e.seeds, e.config);
    let mut failing = Vec::new();
    for k in 0..e.seeds {
        let seed = e.base.wrapping_add(k);
        let r = sim::run(seed, &config);
        if !r.violations.is_empty() {
            println!("failing seed {seed}:");
            let shown = if e.all { usize::MAX } else { 5 };
            for v in r.violations.iter().take(shown) {
                println!("  invariant {} at trace line {}: {}", v.invariant, v.line, v.detail);
            }
            println!("  tests/regress/{seed}.json: {}", regression_json(&e.config, &config, &r));
            failing.push(seed);
        }
    }
    assert!(failing.is_empty(), "{} failing seed(s) from base {}: {:?}", failing.len(), e.base, failing);
}

// ENG-SIM-003: two runs with the same seed and configuration give the same trace.
#[test]
fn same_seed_same_trace() {
    for name in ["quiet", "gate"] {
        let config = SimConfig::named(name).expect("named");
        let a = sim::run_traced(7, &config);
        let b = sim::run(7, &config);
        assert_eq!(a.0.trace_sha256, b.trace_sha256, "{name}");
        assert_eq!(a.0.stats, b.stats, "{name}");
        // The hash is SHA-256 of the whole trace (§31.9).
        use sha2::Digest;
        let sha: [u8; 32] = sha2::Sha256::digest(&a.1).into();
        assert_eq!(sha, a.0.trace_sha256, "{name}");
        // Another seed gives another trace.
        assert_ne!(sim::run(8, &config).trace_sha256, a.0.trace_sha256, "{name}");
    }
}

// One seed and its trace up to the first violation (task D's brief).
#[test]
fn one_seed() {
    let e = env();
    let name = if e.seed.is_some() { e.config.clone() } else { "quiet".to_owned() };
    let config = SimConfig::named(&name).unwrap_or_else(|| panic!("no configuration named {name}"));
    let seed = e.seed.unwrap_or(1);
    let (r, trace) = sim::run_traced(seed, &config);
    let text = String::from_utf8(trace).expect("the trace is UTF-8 (§31.9)");
    let upto = if e.full { u64::MAX } else { r.violations.iter().map(|v| v.line).min().unwrap_or(u64::MAX) };
    if e.seed.is_some() {
        for (i, l) in text.lines().enumerate() {
            if i as u64 >= upto {
                break;
            }
            println!("{l}");
        }
    }
    println!("seed {seed} ({name}): trace_sha256 {}, {} lines", hex(&r.trace_sha256), r.stats.trace_lines);
    println!("{:?}", r.stats);
    for v in &r.violations {
        println!("invariant {} at line {}: {}", v.invariant, v.line, v.detail);
    }
    // Every line is one JSON object with `t` and `k` first (§31.9).
    for l in text.lines() {
        let v: serde_json::Value = serde_json::from_str(l).expect("a trace line is JSON");
        let obj = v.as_object().expect("an object");
        let keys: Vec<&String> = obj.keys().collect();
        assert!(keys.len() >= 2, "{l}");
    }
    assert!(text.lines().all(|l| l.starts_with("{\"t\":") && l.contains(",\"k\":")));
    assert!(r.violations.is_empty(), "seed {seed} of {name} broke an invariant");
}
