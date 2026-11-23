//! The fuzz crate's lockfile is covered by the root's (SYNC_PLAN.md §3.4,
//! the status file's decision L16; task S7's fix pass). `fuzz/` is a crate
//! of its own with its own `[workspace]` table and `Cargo.lock`, which the
//! root's supply-chain gate does not read; it stays covered by that gate as
//! long as every package it holds stands in the root lockfile with the same
//! name, version, source and checksum. The one exception is the fuzz
//! package itself.

use crate::common::crate_dir;

/// A `[[package]]` table of a lockfile: name, version, source, checksum
/// (the last two absent for a path package).
type Package = (String, String, Option<String>, Option<String>);

/// The `[[package]]` tables of a `Cargo.lock`, read by hand: each table's
/// `key = "value"` lines up to the next table header.
fn packages(lock: &str) -> Vec<Package> {
    fn done(fields: &mut Vec<(String, String)>, out: &mut Vec<Package>) {
        if fields.is_empty() {
            return;
        }
        let get = |k: &str| fields.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone());
        let name = get("name").expect("a package without a name");
        let version = get("version").expect("a package without a version");
        out.push((name, version, get("source"), get("checksum")));
        fields.clear();
    }
    let mut out = Vec::new();
    let mut fields: Vec<(String, String)> = Vec::new();
    let mut inside = false;
    for line in lock.lines().map(str::trim) {
        if line.starts_with('[') && !line.starts_with("[\"") && line.ends_with(']') && !line.contains('=') {
            done(&mut fields, &mut out);
            inside = line == "[[package]]";
            continue;
        }
        if !inside {
            continue;
        }
        if let Some((k, v)) = line.split_once(" = ")
            && let Some(v) = v.strip_prefix('"').and_then(|v| v.strip_suffix('"'))
        {
            fields.push((k.trim().to_string(), v.to_string()));
        }
    }
    done(&mut fields, &mut out);
    out
}

/// The packages of `fuzz` that `root` does not hold, the fuzz package left out.
fn not_in_root(root: &str, fuzz: &str) -> Vec<Package> {
    let root = packages(root);
    packages(fuzz).into_iter().filter(|p| p.0 != "taskdeck-seal-fuzz" && !root.contains(p)).collect()
}

// Every package of `fuzz/Cargo.lock` but the fuzz crate itself stands in the
// root `Cargo.lock` with the same name, version, source and checksum. The
// check finds a changed version, a changed checksum and an added package
// in a made-up lockfile.
// rule: SYNC_PLAN.md §3.4 (dependencies), decision L16 (no clause of seal.md)
#[test]
fn the_fuzz_lockfile_is_covered_by_the_root_lockfile() {
    let root = std::fs::read_to_string(crate_dir().join("../../Cargo.lock")).unwrap();
    let fuzz = std::fs::read_to_string(crate_dir().join("fuzz/Cargo.lock")).unwrap();
    let (r, f) = (packages(&root), packages(&fuzz));
    assert!(r.len() > 100 && f.len() > 20, "too few packages read: {} and {}", r.len(), f.len());
    assert!(f.iter().any(|p| p.0 == "taskdeck-seal-fuzz"));
    assert!(f.iter().any(|p| p.0 == "libfuzzer-sys" && p.3.is_some()));
    assert_eq!(not_in_root(&root, &fuzz), Vec::<Package>::new());

    // The check can fail.
    let lock = |v: &str, sum: &str, extra: &str| {
        format!(
            "version = 4\n\n[[package]]\nname = \"a\"\nversion = \"{v}\"\nsource = \"registry+x\"\nchecksum = \"{sum}\"\ndependencies = [\n \"b\",\n]\n\n[[package]]\nname = \"taskdeck-seal-fuzz\"\nversion = \"0.0.0\"\n{extra}"
        )
    };
    let base = lock("1.0.0", "00", "");
    assert_eq!(packages(&base).len(), 2);
    assert!(not_in_root(&base, &base).is_empty());
    assert_eq!(not_in_root(&base, &lock("1.0.1", "00", "")).len(), 1);
    assert_eq!(not_in_root(&base, &lock("1.0.0", "01", "")).len(), 1);
    let added = "\n[[package]]\nname = \"c\"\nversion = \"2.0.0\"\nsource = \"registry+x\"\nchecksum = \"02\"\n";
    assert_eq!(not_in_root(&base, &lock("1.0.0", "00", added)).len(), 1);
}
