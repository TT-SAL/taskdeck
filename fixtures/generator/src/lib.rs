//! The fixture generator (SYNC_PLAN.md §6.1 item 5): made-up boards written by
//! today's board code at a pinned clock and zone, the same boards in older
//! builds' formats, the phone snapshots and the command transcripts of today's
//! code. `fixtures/README.md` describes the output.

pub mod boards;
pub mod clock;
pub mod common;
pub mod legacy;
pub mod out;
pub mod rng;
pub mod snapshots;
pub mod transcripts;

use std::path::Path;

use clock::Zone;

/// The directories the generator owns under `fixtures/`; the manifest lists
/// every file in them.
pub const GENERATED: [&str; 5] = ["boards", "legacy-main", "legacy-other", "snapshots", "transcripts"];
pub const MANIFEST: &str = "MANIFEST.sha256";

/// Write one zone's set into `out_dir`. Berlin first (it writes the boards,
/// the legacy formats, and its snapshots and transcripts); New York second
/// (it reads those boards, writes its own snapshots and transcripts, and the
/// manifest of everything).
pub fn generate(zone: Zone, out_dir: &Path) -> Result<(), String> {
    clock::check_zone(zone)?;
    snapshots::pin_globals();
    match zone {
        Zone::Berlin => {
            if let Some(there) = GENERATED.iter().find(|dir| out_dir.join(dir).exists()) {
                return Err(format!("{} already exists: generate into a directory without it", out_dir.join(there).display()));
            }
            boards::write_all(&out_dir.join("boards"))?;
            legacy::write_all(out_dir)?;
            snapshots::write(out_dir, zone, &boards::BOARDS)?;
            transcripts::write(out_dir, zone)?;
        }
        Zone::NewYork => {
            if !out_dir.join("boards").is_dir() {
                return Err(format!("{} is missing: run the Berlin set first", out_dir.join("boards").display()));
            }
            for dir in ["snapshots", "transcripts"] {
                if out_dir.join(dir).join(zone.label()).exists() {
                    return Err(format!("{dir}/{} already exists", zone.label()));
                }
            }
            snapshots::write(out_dir, zone, &snapshots::NEW_YORK_BOARDS)?;
            transcripts::write(out_dir, zone)?;
            out::write(&out_dir.join(MANIFEST), out::manifest(out_dir, &GENERATED)?)?;
        }
    }
    Ok(())
}
