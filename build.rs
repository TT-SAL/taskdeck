use embed_resource;
use chrono::{DateTime, Utc};
use std::env;
use std::fs;
use std::path::Path;

fn main() {
    // Compile resources
    embed_resource::compile("resources.rc", embed_resource::NONE)
        .manifest_optional()
        .unwrap();

    // The date shown as the build date. Taken from SOURCE_DATE_EPOCH when it
    // is set, so a rebuild of the same source gives the same binary
    // (https://reproducible-builds.org/specs/source-date-epoch/); otherwise
    // today's UTC date, as before.
    let date = match env::var_os("SOURCE_DATE_EPOCH") {
        Some(raw) => match source_date(raw.to_str()) {
            Ok(date) => date,
            // A set but unusable value is a mistake in the release setup:
            // stop rather than quietly stamp the wrong date.
            Err(why) => panic!("SOURCE_DATE_EPOCH is set but unusable: {why}"),
        },
        None => Utc::now(),
    };
    println!("cargo:rustc-env=BUILD_DATE={}", date.format("%Y-%m-%d"));

    rerun_directives();
}

/// Parses SOURCE_DATE_EPOCH: an integer count of seconds since the Unix
/// epoch, written like `date +%s` writes it (digits, an optional leading `-`).
fn source_date(raw: Option<&str>) -> Result<DateTime<Utc>, String> {
    let text = raw.ok_or("the value is not valid UTF-8")?;
    let digits = text.strip_prefix('-').unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!(
            "{text:?} is not an integer number of seconds since 1970-01-01 UTC"
        ));
    }
    let secs: i64 = text
        .parse()
        .map_err(|_| format!("{text:?} does not fit in a 64-bit integer"))?;
    DateTime::from_timestamp(secs, 0)
        .ok_or_else(|| format!("{text:?} is outside the dates chrono can represent"))
}

/// Tells cargo when to run this script again.
///
/// embed-resource prints no `rerun-if-changed` lines, so until now cargo used
/// its default: rerun when any file of the package changes, which also kept
/// BUILD_DATE fresh. Printing `rerun-if-env-changed` switches that default
/// off, so the package's top-level entries are listed here instead (cargo
/// scans a listed directory recursively). Left out, as cargo's default leaves
/// them out: the target directory, version-control metadata, the workspace's
/// member crates under `crates/`, and the top-level names `.gitignore` lists.
fn rerun_directives() {
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");

    let root = env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
    let root = Path::new(&root);
    let ignored = top_level_ignores(&root.join(".gitignore"));
    let entries = fs::read_dir(root).expect("the package directory is readable");
    let mut names: Vec<String> = entries
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| {
            !name.starts_with('.') || name == ".gitignore" || name == ".github"
        })
        .filter(|name| !matches!(name.as_str(), "target" | "crates" | ".git"))
        .filter(|name| !name.ends_with(".pdb") && !name.ends_with(".rs.bk"))
        .filter(|name| !ignored.iter().any(|ignored| ignored == name))
        .collect();
    names.sort();
    for name in names {
        println!("cargo:rerun-if-changed={name}");
    }
}

/// The anchored top-level names in `.gitignore` (`/name` or `/name/`).
fn top_level_ignores(path: &Path) -> Vec<String> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .map(str::trim)
        .filter_map(|line| line.strip_prefix('/'))
        .map(|name| name.trim_end_matches('/'))
        .filter(|name| !name.is_empty() && !name.contains(['/', '*', '?', '[']))
        .map(str::to_owned)
        .collect()
}
