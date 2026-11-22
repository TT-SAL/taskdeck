//! What `phone::snapshot` returns today, for a documented set of requests.

use std::path::Path;

use chrono::{DateTime, Local, NaiveDate};
use task_deck::initialization::{BACKGROUND_BLUR_DEFAULT, BACKGROUND_LIGHT_DEFAULT};
use task_deck::phone;

use crate::clock::{Zone, compact, day, utc};
use crate::common::{open_copy, palette_of};
use crate::out;

pub struct Request {
    pub from: NaiveDate,
    pub days: u32,
    pub now: DateTime<Local>,
}

impl Request {
    /// `<from>_<days>d_at-<now in UTC>.json`
    pub fn file_name(&self) -> String {
        format!("{}_{}d_at-{}.json", self.from, self.days, compact(self.now))
    }
}

/// The boards whose snapshots are written a second time under New York.
pub const NEW_YORK_BOARDS: [&str; 2] = ["small", "dst-edges"];

/// The requests for each board. `days` 0 and 40 are there for the clamp to
/// 1..=31 (`phone::MAX_SNAPSHOT_DAYS`).
pub fn requests(board: &str) -> Vec<Request> {
    let r = |from, days, now| Request { from, days, now };
    let standard = utc(2026, 10, 20, 8, 15, 0);
    match board {
        "small" => vec![
            r(day(2026, 10, 20), 1, standard),
            r(day(2026, 10, 23), 1, standard),
            r(day(2026, 10, 25), 1, standard),
            r(day(2026, 11, 1), 1, standard),
            r(day(2026, 10, 19), 7, standard),
            r(day(2026, 10, 5), 31, standard),
            r(day(2026, 10, 20), 0, standard),
            r(day(2026, 10, 20), 40, standard),
        ],
        "archive-10k" => vec![
            r(day(2026, 10, 20), 1, standard),
            r(day(2026, 10, 19), 7, standard),
            r(day(2026, 10, 5), 31, standard),
        ],
        "dst-edges" => {
            // Now inside Berlin's repeated hour (the second 02:30), just after
            // Berlin's gap (03:30), and in New York's two 01:30s.
            let fold = utc(2026, 10, 25, 1, 30, 0);
            let gap = utc(2026, 3, 29, 1, 30, 0);
            let ny_first = utc(2026, 11, 1, 5, 30, 0);
            let ny_second = utc(2026, 11, 1, 6, 30, 0);
            vec![
                r(day(2026, 3, 29), 1, gap),
                r(day(2026, 10, 25), 1, fold),
                r(day(2026, 10, 24), 1, fold),
                r(day(2026, 3, 8), 1, ny_first),
                r(day(2026, 11, 1), 1, ny_first),
                r(day(2026, 11, 1), 1, ny_second),
                r(day(2026, 10, 20), 2, fold),
                r(day(2026, 12, 31), 2, fold),
                r(day(1970, 1, 1), 1, fold),
                r(day(9999, 12, 31), 1, fold),
                r(day(2026, 10, 22), 7, fold),
                r(day(2026, 3, 1), 31, gap),
            ]
        }
        "id-limits" | "unreadable" => vec![r(day(2026, 10, 20), 1, standard), r(day(2026, 10, 19), 7, standard)],
        "empty" => vec![r(day(2026, 10, 20), 1, standard)],
        _ => Vec::new(),
    }
}

/// The process-wide inputs of a snapshot, pinned: no backdrop (so `look` is
/// absent) and the default dials (11, 39). The weather is not in a snapshot.
pub fn pin_globals() {
    phone::set_backdrop(None);
    phone::set_look_dials(phone::LookDials { blur_percent: BACKGROUND_BLUR_DEFAULT, light_percent: BACKGROUND_LIGHT_DEFAULT });
}

pub fn write(out_dir: &Path, zone: Zone, boards: &[&str]) -> Result<(), String> {
    pin_globals();
    for board_name in boards {
        let (mut board, _problems, copy) = open_copy(&out_dir.join("boards").join(board_name))?;
        let palette = palette_of(copy.path());
        for request in requests(board_name) {
            let snapshot = phone::snapshot(&mut board, palette, None, request.from, request.days, request.now);
            let path = out_dir.join("snapshots").join(zone.label()).join(board_name).join(request.file_name());
            out::write(&path, out::pretty(&snapshot))?;
        }
    }
    Ok(())
}
