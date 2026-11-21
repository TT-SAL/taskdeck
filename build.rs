use embed_resource;
use chrono::{DateTime, Datelike, Utc};
use std::env;

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
/// Only years 0 to 9999 are accepted, so BUILD_DATE is always YYYY-MM-DD.
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
        .filter(|date| (0..=9999).contains(&date.year()))
        .ok_or_else(|| format!("{text:?} is outside the years 0 to 9999"))
}

/// Tells cargo when to run this script again.
///
/// Without these lines cargo reran the script, and so refreshed BUILD_DATE,
/// whenever any file in the package changed. Printing any rerun line switches
/// that default off, so the inputs are listed here instead. BUILD_DATE is now
/// refreshed when the app's own sources, manifest, lockfile or resources change
/// (or SOURCE_DATE_EPOCH does), no longer when an unrelated file in the
/// repository changes. `src` is a directory; cargo scans it recursively.
fn rerun_directives() {
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    for path in [
        "build.rs",
        "resources.rc",
        "icon.ico",
        "Cargo.toml",
        "Cargo.lock",
        "src",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
}
