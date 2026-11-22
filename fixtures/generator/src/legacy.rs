//! The boards again in the file formats of older builds, and the proof that
//! today's code opens them.
//!
//! `legacy-main/<board>/` is what the `origin/main` build writes, from its
//! source (the README lists each shape with the line it comes from; the bytes
//! were also compared once with that build's own save functions):
//!
//! - `read_at_startup.json`: `tasks::oversafe_activesave`, i.e.
//!   `serde_json::to_string_pretty` of `Vec<Active>` where `Active` is `id`,
//!   `importance`, `time_importance`, `name`, `created`, `deadline`,
//!   `is_event` and nothing else: no blocks, estimate or routine;
//! - `archived.jsonl`: `tasks::save_inactive`, one compact `InActive` per line
//!   (`id`, `importance`, `name`, `created`, `deadline`, `is_event`,
//!   `inactivated`), written only by "complete" (`TaskApp::complete_active_thing`);
//! - `notepad_text.json`: `utilities::save_notepad_text`, a pretty JSON string
//!   (`""` when nothing was typed);
//! - `colorschemes.json`: `color::save_colorschemes` of the map the app holds:
//!   scheme 0 (`COLORSCHEME ZERO`, user-configurable, put in memory when the
//!   file is empty) and the palette the user made from it, which gets id 1
//!   (`keys().max() + 1`); main has no built-in palettes;
//! - `userconfig.toml`: `initialization::write_normalized_config` on the empty
//!   file of a first start (11 keys, typed values, that order), then the
//!   setters for the home place and the palette.
//!
//! `legacy-other/<board>/` holds shapes main reads but never writes: items and
//! archive rows with no `id` (builds before ids), items with `planned_start`
//! and `duration_minutes` (day-planner builds before blocks), numbers written
//! as strings and no `ui_scale_percent` in `userconfig.toml` (the setters
//! before `toml_edit`), and the `{}` notepad seed (before main's fix).

use std::fs;
use std::path::Path;

use chrono::{DateTime, Local};
use serde::Serialize;
use serde_json::{Value, json};
use task_deck::archive::{ARCHIVE_FILE, Archived, Outcome};
use task_deck::color::ColorScheme;
use task_deck::tasks::Active;

use crate::common::{FIXTURE_PALETTE, HOME_PLACE, open_copy, palette_of};
use crate::out;

const NOTES_PLACEHOLDER: &str = "There was something wrong with taskdeck_data/notepad_text.json!";

/// The palette's id in a legacy directory: the first scheme main makes beside
/// scheme 0. Today's desktop has a built-in palette on that id and moves the
/// user's one off it (`color::install_builtins`).
pub const LEGACY_SCHEME_ID: u32 = 1;

/// Which build's shapes a directory is written in.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// What `origin/main` writes.
    Main,
    /// What main reads but never writes (see the module comment).
    Older,
}

/// main's `Active`, field for field; under `Shape::Older` the `id` may be
/// absent and a task may carry the day-planner's single plan.
#[derive(Serialize)]
pub struct LegacyItem {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    pub importance: Option<u8>,
    pub time_importance: Option<u8>,
    pub name: String,
    pub created: DateTime<Local>,
    pub deadline: Option<DateTime<Local>>,
    pub is_event: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planned_start: Option<DateTime<Local>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_minutes: Option<u32>,
}

/// main's `InActive`, field for field; the `id` may be absent under
/// `Shape::Older`.
#[derive(Serialize)]
pub struct LegacyRow {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    pub importance: Option<u8>,
    pub name: String,
    pub created: DateTime<Local>,
    pub deadline: Option<DateTime<Local>>,
    pub is_event: bool,
    pub inactivated: DateTime<Local>,
}

/// `userconfig.toml` as main leaves it: `write_normalized_config`'s keys in
/// its order and types on a first start, then `set_weather_coordinates` and
/// `set_colorscheme` (both `toml_edit`, which keeps the key where it is).
pub fn main_userconfig() -> String {
    format!(
        "start_in_fullscreen = false\n\
         coordinates = [{}, {}]\n\
         background = \"\"\n\
         enable_fps_counter = false\n\
         window_size_startup = [1280.0, 720.0]\n\
         calendar_weeks_to_show = 100\n\
         selected_monitor_name = \"\"\n\
         selected_colorscheme_id = {LEGACY_SCHEME_ID}\n\
         three_day_weather = false\n\
         background_image_tint_percent = 30\n\
         ui_scale_percent = 0\n",
        HOME_PLACE[0], HOME_PLACE[1]
    )
}

/// `userconfig.toml` as the setters before `toml_edit` left it: numbers as
/// strings (main's own test, `write_normalized_config_preserves_comments_and_
/// writes_typed_values`, starts from that), and no `ui_scale_percent`.
pub fn older_userconfig() -> String {
    format!(
        "start_in_fullscreen = false\n\
         coordinates = [{}, {}]\n\
         background = \"\"\n\
         enable_fps_counter = false\n\
         window_size_startup = [1280.0, 720.0]\n\
         calendar_weeks_to_show = \"100\"\n\
         background_image_tint_percent = \"30\"\n\
         selected_monitor_name = \"\"\n\
         selected_colorscheme_id = \"{LEGACY_SCHEME_ID}\"\n\
         three_day_weather = false\n",
        HOME_PLACE[0], HOME_PLACE[1]
    )
}

/// `colorschemes.json` as main's `save_colorschemes` writes the two schemes,
/// in key order (main writes a `HashMap`, so its own order varies).
pub fn legacy_colorschemes() -> String {
    let zero = ColorScheme { name: "COLORSCHEME ZERO".into(), colors: [[0; 4]; 6], is_user_configurable: true };
    let palette = ColorScheme { name: "Fixture palette (made up)".into(), colors: FIXTURE_PALETTE, is_user_configurable: true };
    let schemes: std::collections::BTreeMap<u32, ColorScheme> = [(0, zero), (LEGACY_SCHEME_ID, palette)].into_iter().collect();
    serde_json::to_string_pretty(&schemes).expect("schemes serialise")
}

/// One live item in a legacy shape, or `None` for a routine (no legacy build
/// could hold one). The horizon goes back to the three-level scale (3 → 0).
pub fn legacy_item(item: &Active, shape: Shape, with_id: bool) -> Option<LegacyItem> {
    if item.recurrence.is_some() {
        return None;
    }
    let plan = item.sessions.first().filter(|_| shape == Shape::Older && !item.is_event);
    Some(LegacyItem {
        id: with_id.then_some(item.id),
        importance: item.importance,
        time_importance: item.time_importance.map(|level| if level >= 3 { 0 } else { level }),
        name: item.name.clone(),
        created: item.created,
        deadline: item.deadline,
        is_event: item.is_event,
        planned_start: plan.map(|session| session.start),
        duration_minutes: plan.map(|session| session.minutes),
    })
}

/// One archive row in the legacy shape, or `None` for a row no legacy build
/// wrote (a dropped task: main's delete writes nothing; a routine).
pub fn legacy_row(row: &Archived, with_id: bool) -> Option<LegacyRow> {
    if row.is_routine() || (!row.is_event && row.outcome == Outcome::Dropped) {
        return None;
    }
    Some(LegacyRow {
        id: with_id.then_some(row.id),
        importance: row.importance,
        name: row.name.clone(),
        created: row.created,
        deadline: row.deadline,
        is_event: row.is_event,
        inactivated: row.archived_at,
    })
}

/// The boards written in each shape, and where.
const OLDER_BOARDS: [&str; 2] = ["empty", "small"];

pub fn write_all(out_dir: &Path) -> Result<(), String> {
    for (root, shape, names) in [("legacy-main", Shape::Main, &crate::boards::BOARDS[..]), ("legacy-other", Shape::Older, &OLDER_BOARDS[..])] {
        let mut report = Vec::new();
        for name in names {
            let to = out_dir.join(root).join(name);
            let written = write_one(name, &out_dir.join("boards").join(name), &to, shape)?;
            report.push(prove(&format!("{root}/{name}"), name, &to, written)?);
        }
        out::write(&out_dir.join(root).join("MIGRATION.json"), out::pretty(&Value::Array(report)))?;
    }
    Ok(())
}

/// What the down-conversion left out, counted.
#[derive(Default)]
struct Written {
    routines_left_out: usize,
    blocks_left_out: usize,
    archive_rows_left_out: usize,
    items_without_id: usize,
    rows_without_id: usize,
    planned_start_items: usize,
    tabs: usize,
}

fn write_one(name: &str, from: &Path, to: &Path, shape: Shape) -> Result<Written, String> {
    let read = |file: &str| fs::read(from.join(file)).map_err(|e| format!("{}/{file}: {e}", from.display()));
    let mut written = Written::default();
    // Under `Shape::Older`, every third item and row has no id.
    let keeps_id = |n: usize| shape == Shape::Main || n % 3 != 2;

    let items: Vec<Active> = serde_json::from_slice(&read("read_at_startup.json")?).map_err(|e| e.to_string())?;
    let mut legacy = Vec::new();
    for item in &items {
        match legacy_item(item, shape, keeps_id(legacy.len())) {
            Some(old) => {
                written.items_without_id += usize::from(old.id.is_none());
                written.planned_start_items += usize::from(old.planned_start.is_some());
                written.blocks_left_out += item.sessions.len() - usize::from(old.planned_start.is_some());
                legacy.push(old);
            }
            None => written.routines_left_out += 1,
        }
    }
    out::write(&to.join("read_at_startup.json"), serde_json::to_string_pretty(&legacy).map_err(|e| e.to_string())?)?;

    // The archive, line by line: a line today's code cannot read is copied as
    // the bytes it is (it stands for damage from outside the program).
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
                Some(row) => match legacy_row(&row, keeps_id(kept)) {
                    Some(old) => {
                        kept += 1;
                        written.rows_without_id += usize::from(old.id.is_none());
                        archive.extend_from_slice(serde_json::to_string(&old).map_err(|e| e.to_string())?.as_bytes());
                    }
                    None => {
                        written.archive_rows_left_out += 1;
                        continue;
                    }
                },
                None => archive.extend_from_slice(part),
            }
            if n < parts.len() - 1 {
                archive.push(b'\n');
            }
        }
        out::write(&to.join(ARCHIVE_FILE), archive)?;
    }

    let notes = match (name, shape) {
        ("empty", Shape::Main) => "\"\"".to_string(),
        ("empty", Shape::Older) => "{}".to_string(),
        _ => {
            let today: String = serde_json::from_slice(&read("notepad_text.json")?).map_err(|e| e.to_string())?;
            let legacy = format!("{today}\n\tIndented with a tab, as the old notepad kept it.\n\t\tTwo tabs.");
            written.tabs = legacy.matches('\t').count();
            serde_json::to_string_pretty(&legacy).map_err(|e| e.to_string())?
        }
    };
    out::write(&to.join("notepad_text.json"), notes)?;
    out::write(&to.join("colorschemes.json"), legacy_colorschemes())?;
    let config = if shape == Shape::Main { main_userconfig() } else { older_userconfig() };
    out::write(&to.join("userconfig.toml"), config)?;
    Ok(written)
}

/// Open the legacy directory (a copy of it) with today's code and count what
/// it made of it. Fails the run when today's code reports a problem.
fn prove(label: &str, name: &str, dir: &Path, written: Written) -> Result<Value, String> {
    let (mut board, problems, copy) = open_copy(dir)?;
    if !problems.is_empty() {
        return Err(format!("today's code could not open {label}: {problems:?}"));
    }
    board.load_archive().map_err(|e| format!("{label}: {e}"))?;
    let mut ids: Vec<u64> = board.items.iter().map(|item| item.id).collect();
    ids.sort_unstable();
    ids.dedup();
    if ids.len() != board.items.len() || ids.first() == Some(&0) {
        return Err(format!("{label}: ids are not unique and non-zero after opening"));
    }
    let config = task_deck::initialization::read_config_only(&copy.path().join("userconfig.toml"));
    let rows = board.archive.entries();
    Ok(json!({
        "board": name,
        "written": {
            "routines_left_out": written.routines_left_out,
            "blocks_left_out": written.blocks_left_out,
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
            "ui_scale_percent": config.ui_scale_percent,
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use task_deck::tasks::Session;

    fn task() -> Active {
        Active {
            id: 97,
            importance: Some(2),
            time_importance: Some(3),
            name: "old row".into(),
            created: crate::clock::utc(2026, 8, 15, 17, 28, 41),
            deadline: None,
            is_event: false,
            sessions: vec![Session { start: crate::clock::utc(2026, 8, 16, 8, 0, 0), minutes: 90 }],
            planned_start: None,
            duration_minutes: Some(30),
            recurrence: None,
        }
    }

    /// The keys of a JSON object line, in the order written.
    fn keys(line: &str) -> Vec<String> {
        line.split('"').skip(1).step_by(2).filter(|key| line.contains(&format!("\"{key}\":"))).map(String::from).collect()
    }

    #[test]
    fn legacy_rows_have_mains_shape_and_order() {
        let row = Archived::retire(task(), Outcome::Finished, crate::clock::utc(2026, 8, 15, 18, 35, 10));
        let line = serde_json::to_string(&legacy_row(&row, true).unwrap()).unwrap();
        assert_eq!(keys(&line), ["id", "importance", "name", "created", "deadline", "is_event", "inactivated"]);
        // And today's code reads it as the test in archive.rs says it must.
        let back: Archived = serde_json::from_str(&line).unwrap();
        assert_eq!((back.id, back.outcome), (97, Outcome::Finished));
        assert!(!serde_json::to_string(&legacy_row(&row, false).unwrap()).unwrap().contains("\"id\""));
        // main's delete wrote nothing, so a dropped task has no row.
        assert!(legacy_row(&Archived::retire(task(), Outcome::Dropped, row.archived_at), true).is_none());
    }

    #[test]
    fn legacy_items_have_mains_fields_and_only_older_ones_keep_a_plan() {
        let main = serde_json::to_string(&legacy_item(&task(), Shape::Main, true).unwrap()).unwrap();
        assert_eq!(keys(&main), ["id", "importance", "time_importance", "name", "created", "deadline", "is_event"]);
        assert!(main.contains("\"time_importance\":0"), "the horizon on the three-level scale: {main}");
        let older = serde_json::to_string(&legacy_item(&task(), Shape::Older, false).unwrap()).unwrap();
        assert_eq!(keys(&older), ["importance", "time_importance", "name", "created", "deadline", "is_event", "planned_start", "duration_minutes"]);
        assert!(older.ends_with("\"duration_minutes\":90}"), "the first block's length: {older}");
        let mut routine = task();
        routine.recurrence = Some(task_deck::tasks::Recurrence {
            days: 127,
            anchor: chrono::NaiveDate::from_ymd_opt(2026, 9, 1).unwrap(),
            start_minutes: 0,
            minutes: 60,
        });
        assert!(legacy_item(&routine, Shape::Main, true).is_none());
    }

    #[test]
    fn the_userconfigs_have_their_builds_keys_and_types() {
        let main = main_userconfig();
        let keys: Vec<&str> = main.lines().map(|line| line.split(" = ").next().unwrap()).collect();
        assert_eq!(
            keys,
            [
                "start_in_fullscreen",
                "coordinates",
                "background",
                "enable_fps_counter",
                "window_size_startup",
                "calendar_weeks_to_show",
                "selected_monitor_name",
                "selected_colorscheme_id",
                "three_day_weather",
                "background_image_tint_percent",
                "ui_scale_percent",
            ]
        );
        assert!(main.contains("calendar_weeks_to_show = 100\n") && main.contains("selected_colorscheme_id = 1\n"));
        let older = older_userconfig();
        assert!(older.contains("calendar_weeks_to_show = \"100\"") && !older.contains("ui_scale_percent"));
        assert!(legacy_colorschemes().contains("\"is_user_configurable\": true") && !legacy_colorschemes().contains("false"));
    }
}
