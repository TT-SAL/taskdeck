//! The read-side modules ported in step c2 (`ics`, `overlay`, `weather`) read
//! no clock, zone, file, network, thread or environment (CORE-GEN-003,
//! core.md §14.6): none of them names one. Clippy's disallowed lists and the
//! lib-alone build of ci.sh enforce the same; this says it per module.

const ICS: &str = include_str!("../src/ics.rs");
const OVERLAY: &str = include_str!("../src/overlay.rs");
const WEATHER: &str = include_str!("../src/weather.rs");

// spec: CORE-GEN-003
#[test]
fn the_ported_read_side_names_no_clock_zone_file_network_thread_or_environment() {
    for (file, text) in [("ics.rs", ICS), ("overlay.rs", OVERLAY), ("weather.rs", WEATHER)] {
        for word in [
            "Local",
            "SystemTime",
            "Instant::now",
            "Utc::now",
            "::now(",
            "std::fs",
            "std::net",
            "std::thread",
            "std::env",
            "RandomState",
            "HashMap::new",
            "HashSet",
        ] {
            assert!(!text.contains(word), "{file} names {word}");
        }
    }
}

// spec: CORE-GEN-003
#[test]
fn ics_parse_takes_its_zone_as_an_argument() {
    // CORE-READ-004 through CORE-GEN-003: the zone reaches the parser as a
    // parameter, never from the process.
    // Compared with every run of whitespace as one space, so that a checkout
    // with CRLF line endings (Windows) reads the same.
    let flat = ICS.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(flat.contains("zone: &(impl Zone + ?Sized), ) -> Result<Parsed, String>"));
}
