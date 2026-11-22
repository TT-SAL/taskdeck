//! The boards again, in the older build's file formats, and the proof that
//! today's code opens them.
//!
//! The older build's code was not available to this generator (see the
//! README: `origin/main` was not in the checkout), so the shapes are the ones
//! today's code and documentation record of it:
//!
//! - archive rows as the old `save_inactive` wrote them, the shape of
//!   `legacy_rows_load_and_mean_what_they_meant` in `src/archive.rs`: `id`,
//!   `importance`, `name`, `created`, `deadline`, `is_event`, `inactivated`,
//!   in that order; no outcome, sessions, estimate or horizon. Only the
//!   complete path wrote a row (DOCUMENTATION.md §6, "Superseded `InActive`"),
//!   so dropped tasks and routines have none; events do (the `saunas` row);
//! - items with no `sessions`, `duration_minutes` as the length of a single
//!   `planned_start` slot, and no routines (the pre-sessions shape of
//!   `a_legacy_save_loads_migrated_and_a_corrupt_one_is_quarantined` in
//!   `src/board.rs`); the horizon on the old three-level scale (0–2);
//! - rows and items written before ids existed: no `id` at all;
//! - a notepad seeded with `{}` (`utilities::read_notepad_text`: "not the `{}`
//!   this used to seed it with"), and tabs in the notepad (§9.1);
//! - `userconfig.toml` with numbers written as strings, as "the old buggy
//!   runtime setters used to write" (`initialization.rs` tests), and only the
//!   keys DOCUMENTATION.md §11 lists that predate the phone, the server and
//!   the newer dials;
//! - no `subscriptions.json` or `subscribed_cache.json` (subscribed calendars
//!   are §23, newer than the archive redesign).

use std::fs;
use std::path::Path;

use serde_json::{Value, json};
use task_deck::archive::{ARCHIVE_FILE, Archived, Outcome};

use crate::common::{FIXTURE_PALETTE, FIXTURE_SCHEME_ID, HOME_PLACE, open_copy, palette_of};
use crate::out;

const NOTES_PLACEHOLDER: &str = "There was something wrong with taskdeck_data/notepad_text.json!";

/// The legacy `userconfig.toml`: the palette, the home place and the other
/// keys that build wrote.
pub fn legacy_userconfig() -> String {
    format!(
        "start_in_fullscreen = false\n\
         coordinates = [{}, {}]\n\
         background = \"\"\n\
         enable_fps_counter = false\n\
         window_size_startup = [1280.0, 720.0]\n\
         calendar_weeks_to_show = \"100\"\n\
         background_image_tint_percent = \"30\"\n\
         selected_monitor_name = \"\"\n\
         selected_colorscheme_id = \"{FIXTURE_SCHEME_ID}\"\n\
         three_day_weather = false\n",
        HOME_PLACE[0], HOME_PLACE[1]
    )
}

/// A JSON object written field by field, in the order given — the order the
/// old structs declared them in.
fn object(fields: &[(&str, &Value)]) -> String {
    let body: Vec<String> = fields.iter().map(|(key, value)| format!("{}:{}", json!(key), value)).collect();
    format!("{{{}}}", body.join(","))
}

/// One live item in the old shape, or `None` for a routine (which the old
/// build could not hold). `with_id` false writes the pre-id shape.
pub fn legacy_item(item: &Value, with_id: bool) -> Option<String> {
    if !item["recurrence"].is_null() {
        return None;
    }
    let horizon = match item["time_importance"].as_u64() {
        Some(level) if level >= 3 => json!(0),
        Some(level) => json!(level),
        None => Value::Null,
    };
    let mut fields: Vec<(&str, &Value)> = Vec::new();
    if with_id {
        fields.push(("id", &item["id"]));
    }
    fields.extend([
        ("importance", &item["importance"]),
        ("time_importance", &horizon),
        ("name", &item["name"]),
        ("created", &item["created"]),
        ("deadline", &item["deadline"]),
        ("is_event", &item["is_event"]),
    ]);
    let first = &item["sessions"][0];
    if item["is_event"] == json!(false) && !first.is_null() {
        fields.push(("planned_start", &first["start"]));
        fields.push(("duration_minutes", &first["minutes"]));
    }
    Some(object(&fields))
}

/// One archive row in the old shape, or `None` for a row the old build never
/// wrote (a dropped task or routine).
pub fn legacy_row(row: &Archived, with_id: bool) -> Option<String> {
    if row.is_routine() || (!row.is_event && row.outcome == Outcome::Dropped) {
        return None;
    }
    let value = serde_json::to_value(row).expect("a row serialises");
    let mut fields: Vec<(&str, &Value)> = Vec::new();
    if with_id {
        fields.push(("id", &value["id"]));
    }
    fields.extend([
        ("importance", &value["importance"]),
        ("name", &value["name"]),
        ("created", &value["created"]),
        ("deadline", &value["deadline"]),
        ("is_event", &value["is_event"]),
        ("inactivated", &value["inactivated"]),
    ]);
    Some(object(&fields))
}

/// Whether the board keeps every id (the limits board does: its ids are the
/// point) or writes every fifth item and row in the pre-id shape.
fn keeps_ids(name: &str) -> bool {
    name == "id-limits"
}

pub fn write_all(out_dir: &Path) -> Result<(), String> {
    let mut report = Vec::new();
    for name in crate::boards::BOARDS {
        let from = out_dir.join("boards").join(name);
        let to = out_dir.join("legacy-main").join(name);
        let written = write_one(name, &from, &to)?;
        report.push(prove(name, &to, written)?);
    }
    out::write(&out_dir.join("legacy-main/MIGRATION.json"), out::pretty(&Value::Array(report)))
}

/// What the down-conversion left out, counted.
struct Written {
    routines_left_out: usize,
    archive_rows_left_out: usize,
    items_without_id: usize,
    rows_without_id: usize,
    planned_start_items: usize,
    tabs: usize,
}

fn write_one(name: &str, from: &Path, to: &Path) -> Result<Written, String> {
    let read = |file: &str| fs::read(from.join(file)).map_err(|e| format!("{}/{file}: {e}", from.display()));
    let mut written = Written { routines_left_out: 0, archive_rows_left_out: 0, items_without_id: 0, rows_without_id: 0, planned_start_items: 0, tabs: 0 };

    let items: Vec<Value> = serde_json::from_slice(&read("read_at_startup.json")?).map_err(|e| e.to_string())?;
    let mut lines = Vec::new();
    for (n, item) in items.iter().enumerate() {
        let with_id = keeps_ids(name) || n % 5 != 4;
        match legacy_item(item, with_id) {
            Some(line) => {
                written.items_without_id += usize::from(!with_id);
                written.planned_start_items += usize::from(line.contains("\"planned_start\""));
                lines.push(format!("  {line}"));
            }
            None => written.routines_left_out += 1,
        }
    }
    let body = if lines.is_empty() { "[]".to_string() } else { format!("[\n{}\n]", lines.join(",\n")) };
    out::write(&to.join("read_at_startup.json"), body)?;

    // The archive, line by line: a line today's code cannot read is copied as
    // the bytes it is.
    if let Ok(bytes) = fs::read(from.join(ARCHIVE_FILE)) {
        let mut archive: Vec<u8> = Vec::new();
        let mut kept = 0usize;
        let parts: Vec<&[u8]> = bytes.split(|b| *b == b'\n').collect();
        for (n, part) in parts.iter().enumerate() {
            if n == parts.len() - 1 && part.is_empty() {
                break;
            }
            let parsed = std::str::from_utf8(part).ok().and_then(|text| serde_json::from_str::<Archived>(text).ok());
            match parsed {
                Some(row) => {
                    let with_id = keeps_ids(name) || kept % 5 != 4;
                    match legacy_row(&row, with_id) {
                        Some(line) => {
                            kept += 1;
                            written.rows_without_id += usize::from(!with_id);
                            archive.extend_from_slice(line.as_bytes());
                        }
                        None => {
                            written.archive_rows_left_out += 1;
                            continue;
                        }
                    }
                }
                None => archive.extend_from_slice(part),
            }
            if n < parts.len() - 1 {
                archive.push(b'\n');
            }
        }
        out::write(&to.join(ARCHIVE_FILE), archive)?;
    }

    let notes = if name == "empty" {
        "{}".to_string()
    } else {
        let today: String = serde_json::from_slice(&read("notepad_text.json")?).map_err(|e| e.to_string())?;
        let legacy = format!("{today}\n\tIndented with a tab, as the old notepad kept it.\n\t\tTwo tabs.");
        written.tabs = legacy.matches('\t').count();
        serde_json::to_string_pretty(&legacy).map_err(|e| e.to_string())?
    };
    out::write(&to.join("notepad_text.json"), notes)?;
    let schemes = if name == "empty" { b"{}".to_vec() } else { read("colorschemes.json")? };
    out::write(&to.join("colorschemes.json"), schemes)?;
    out::write(&to.join("userconfig.toml"), legacy_userconfig())?;
    Ok(written)
}

/// Open the legacy directory (a copy of it) with today's code and count what
/// it made of it. Fails the run when today's code reports a problem.
fn prove(name: &str, dir: &Path, written: Written) -> Result<Value, String> {
    let (mut board, problems, copy) = open_copy(dir)?;
    if !problems.is_empty() {
        return Err(format!("today's code could not open legacy-main/{name}: {problems:?}"));
    }
    board.load_archive().map_err(|e| format!("legacy-main/{name}: {e}"))?;
    let mut ids: Vec<u64> = board.items.iter().map(|item| item.id).collect();
    ids.sort_unstable();
    ids.dedup();
    if ids.len() != board.items.len() || ids.first() == Some(&0) {
        return Err(format!("legacy-main/{name}: ids are not unique and non-zero after opening"));
    }
    let config = task_deck::initialization::read_config_only(&copy.path().join("userconfig.toml"));
    let rows = board.archive.entries();
    Ok(json!({
        "board": name,
        "written": {
            "routines_left_out": written.routines_left_out,
            "archive_rows_left_out": written.archive_rows_left_out,
            "items_without_id": written.items_without_id,
            "archive_rows_without_id": written.rows_without_id,
            "items_with_planned_start": written.planned_start_items,
            "notepad_tabs": written.tabs,
        },
        "opened": {
            "problems": problems.len(),
            "items": board.items.len(),
            "items_given_sessions_from_planned_start": board.items.iter().filter(|item| !item.sessions.is_empty()).count(),
            "items_with_planned_start_left": board.items.iter().filter(|item| item.planned_start.is_some()).count(),
            "archive_rows": rows.len(),
            "archive_rows_with_id_0": rows.iter().filter(|row| row.id == 0).count(),
            "archive_rows_finished": rows.iter().filter(|row| row.outcome == Outcome::Finished).count(),
            "archive_events_marked_finished": rows.iter().filter(|row| row.is_event && row.outcome == Outcome::Finished).count(),
            "archive_unreadable_lines": board.archive.unreadable(),
            "notes_placeholder": usize::from(board.notes == NOTES_PLACEHOLDER),
            "notes_tabs_left": board.notes.matches('\t').count(),
            "palette_found": usize::from(palette_of(copy.path()) == FIXTURE_PALETTE),
            "home_place_found": usize::from(config.coordinates == HOME_PLACE),
            "selected_colorscheme_id": config.selected_colorscheme_id,
            "calendar_weeks_to_show": config.calendar_weeks_to_show,
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_rows_have_the_old_shape_and_order() {
        let item = task_deck::tasks::Active {
            id: 97,
            importance: Some(2),
            time_importance: Some(3),
            name: "old row".into(),
            created: crate::clock::utc(2026, 8, 15, 17, 28, 41),
            deadline: None,
            is_event: false,
            sessions: Vec::new(),
            planned_start: None,
            duration_minutes: Some(30),
            recurrence: None,
        };
        let row = Archived::retire(item.clone(), Outcome::Finished, crate::clock::utc(2026, 8, 15, 18, 35, 10));
        let line = legacy_row(&row, true).unwrap();
        let keys: Vec<&str> = ["\"id\"", "\"importance\"", "\"name\"", "\"created\"", "\"deadline\"", "\"is_event\"", "\"inactivated\""]
            .into_iter()
            .collect();
        let mut at = 0;
        for key in keys {
            let found = line[at..].find(key).unwrap_or_else(|| panic!("{key} missing or out of order in {line}"));
            at += found + key.len();
        }
        assert!(!line.contains("outcome") && !line.contains("sessions") && !line.contains("time_importance"));
        // And today's code reads it as the test in archive.rs says it must.
        let back: Archived = serde_json::from_str(&line).unwrap();
        assert_eq!((back.id, back.outcome), (97, Outcome::Finished));
        assert!(!legacy_row(&row, false).unwrap().contains("\"id\""));
        // A dropped task was never written by the old build.
        assert!(legacy_row(&Archived::retire(item, Outcome::Dropped, row.archived_at), true).is_none());
    }

    #[test]
    fn legacy_items_fold_sessions_and_the_horizon() {
        let item = json!({"id":5,"importance":null,"time_importance":3,"name":"n","created":"2026-09-01T09:00:00+02:00",
            "deadline":null,"is_event":false,"sessions":[{"start":"2026-09-03T10:00:00+02:00","minutes":90}],
            "planned_start":null,"duration_minutes":120,"recurrence":null});
        let line = legacy_item(&item, false).unwrap();
        assert_eq!(
            line,
            r#"{"importance":null,"time_importance":0,"name":"n","created":"2026-09-01T09:00:00+02:00","deadline":null,"is_event":false,"planned_start":"2026-09-03T10:00:00+02:00","duration_minutes":90}"#
        );
        let mut routine = item.clone();
        routine["recurrence"] = json!({"days":127,"anchor":"2026-09-01","start_minutes":0,"minutes":60});
        assert!(legacy_item(&routine, true).is_none());
        assert!(legacy_userconfig().contains("calendar_weeks_to_show = \"100\""));
    }
}
