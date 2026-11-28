//! Every failing seed becomes a regression test (engine.md §31.8, ENG-SIM-005):
//! each file under `tests/regress/` is `{"spec": "engine.md §31", "cases":
//! [{"name", "seed", "config", "invariant", "trace_sha256", "first_bad_line"}]}`;
//! every case's seed runs with its configuration and must hold every invariant
//! of §31.7 (ENG-SIM-002). `trace_sha256` records the failing run and is not
//! compared. A file that does not parse fails the test.

use serde::Deserialize;
use taskdeck_sync::sim::{self, FaultSet, OpSet, SimConfig};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    spec: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    name: String,
    seed: u64,
    config: Config,
    invariant: u8,
    trace_sha256: String,
    first_bad_line: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    devices: u8,
    days: u32,
    faults: Faults,
    ops: Ops,
    rules2: Vec<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Faults {
    loss_ppm: u32,
    dup_ppm: u32,
    delay_ms: u32,
    disconnects_per_hour: u32,
    crash_ppm: u32,
    skew_ms: i64,
    rollbacks: u32,
    fork: bool,
    withhold: bool,
    injected: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ops {
    revoke: u32,
    skip: bool,
    rules_upgrade: bool,
    purge: u32,
    move_relay: bool,
    restore: u32,
    recovery_key: u32,
    poison: u32,
}

impl Config {
    fn sim(&self) -> SimConfig {
        let f = &self.faults;
        let o = &self.ops;
        SimConfig {
            devices: self.devices,
            days: self.days,
            faults: FaultSet {
                loss_ppm: f.loss_ppm,
                dup_ppm: f.dup_ppm,
                delay_ms: f.delay_ms,
                disconnects_per_hour: f.disconnects_per_hour,
                crash_ppm: f.crash_ppm,
                skew_ms: f.skew_ms,
                rollbacks: f.rollbacks,
                fork: f.fork,
                withhold: f.withhold,
                injected: f.injected,
            },
            ops: OpSet {
                revoke: o.revoke,
                skip: o.skip,
                rules_upgrade: o.rules_upgrade,
                purge: o.purge,
                move_relay: o.move_relay,
                restore: o.restore,
                recovery_key: o.recovery_key,
                poison: o.poison,
            },
            rules2: self.rules2.clone(),
        }
    }
}

/// Parses one regression file; an error names what is wrong.
fn parse(text: &str) -> Result<File, String> {
    let f: File = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if f.spec != "engine.md §31" {
        return Err(format!("spec is {:?}, not \"engine.md §31\"", f.spec));
    }
    if f.cases.is_empty() {
        return Err("no cases".to_owned());
    }
    for c in &f.cases {
        if !(1..=6).contains(&c.invariant) || c.trace_sha256.len() != 64 || c.name.is_empty() {
            return Err(format!("case {:?}: bad invariant, name or trace_sha256", c.name));
        }
        let _ = c.first_bad_line;
    }
    Ok(f)
}

/// The regression files, by name (the one place this target reads files:
/// the directory of §31.8; `clippy.toml` disallows file access elsewhere).
#[allow(clippy::disallowed_methods)]
fn files() -> Vec<(String, String)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("regress");
    let Ok(rd) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut out: Vec<(String, String)> = rd
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .map(|p| {
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let text = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{name}: {e}"));
            (name, text)
        })
        .collect();
    out.sort();
    out
}

// ENG-SIM-005: every committed regression file runs and holds every invariant.
#[test]
fn every_regression_file_holds_every_invariant() {
    let mut bad = Vec::new();
    for (name, text) in files() {
        let file = parse(&text).unwrap_or_else(|e| panic!("tests/regress/{name} does not parse: {e}"));
        for c in &file.cases {
            let r = sim::run(c.seed, &c.config.sim());
            for v in &r.violations {
                bad.push(format!("{name} ({}): seed {} invariant {} at line {}: {}", c.name, c.seed, v.invariant, v.line, v.detail));
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

// §31.8: `invariant`, `trace_sha256` and `first_bad_line` record the failing
// run. These seeds' failing runs were reproduced with this simulator and the
// engine before the fixes of 2139a96 (the run the files must record, not the
// fixed run's trace or a line 0).
#[test]
fn files_record_the_reproduced_failing_runs() {
    let known: [(&str, u8, &str, u64); 3] = [
        ("102.json", 2, "640cfb2ecdc46205395eacba999b9302f248e4922fa2c6006bf0180016c7ce4b", 2_229_941),
        ("111.json", 6, "e02d2109b27b77d3b16c961e5c17f8d4f196620371fd1defc601853384a1d173", 6_403),
        ("116.json", 6, "ff1a4b2d2bf6720b9823206a7799f92b04adc9ffde069abf0d87f05c22da5146", 4_141_842),
    ];
    let files = files();
    for (name, invariant, sha, line) in known {
        let text = &files.iter().find(|(n, _)| n == name).unwrap_or_else(|| panic!("tests/regress/{name} is missing")).1;
        let f = parse(text).unwrap_or_else(|e| panic!("tests/regress/{name} does not parse: {e}"));
        let c = &f.cases[0];
        assert_eq!((c.invariant, c.trace_sha256.as_str(), c.first_bad_line), (invariant, sha, line), "tests/regress/{name}");
    }
}

// §31.8: the parser takes exactly the file format, and a file that does not
// parse is an error (so a broken file fails the test above).
#[test]
fn the_format_parses_and_a_bad_file_does_not() {
    let config = SimConfig::named("quiet").expect("quiet");
    let good = format!(
        "{{\"spec\": \"engine.md §31\", \"cases\": [{{\"name\": \"quiet seed 1\", \"seed\": 1, \"config\": {}, \"invariant\": 2, \"trace_sha256\": \"{}\", \"first_bad_line\": 10}}]}}",
        config.to_json(),
        "ab".repeat(32)
    );
    let f = parse(&good).expect("the format of §31.8");
    assert_eq!(f.cases[0].config.sim(), config);
    // The configuration of every named run survives the JSON form.
    for name in sim::NAMES {
        let c = SimConfig::named(name).expect("named");
        let cfg: Config = serde_json::from_str(&c.to_json()).expect("to_json parses");
        assert_eq!(cfg.sim(), c, "{name}");
    }
    assert!(parse("{").is_err());
    assert!(parse(&good.replace("engine.md §31", "engine.md §30")).is_err());
    assert!(parse(&good.replace("\"invariant\": 2", "\"invariant\": 7")).is_err());
    assert!(parse(&good.replace("\"seed\": 1,", "\"seed\": 1, \"extra\": 0,")).is_err());
    assert!(parse(&good.replace("\"first_bad_line\": 10", "\"first_bad_line\": -1")).is_err());
}
