use std::{collections::HashMap, error::Error, fs::{self, File, OpenOptions}, io::{BufReader, BufWriter, Write}, path::PathBuf};
use chrono::{DateTime, Local, NaiveDate};
use rev_lines::RevLines;
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Active {
    /// Stable identity for the item. Unlike `name` (which is cosmetic and may
    /// repeat), this is what delete/complete/lookup key on. `0` is the
    /// "unassigned" sentinel for items loaded from a pre-id or hand-edited save;
    /// `assign_missing_ids` backfills those at startup.
    #[serde(default)]
    pub id: u64,
    pub importance: Option<u8>,
    pub time_importance: Option<u8>,
    pub name: String,
    pub created: DateTime<Local>,
    pub deadline: Option<DateTime<Local>>,
    pub is_event: bool,
}

impl Active {
    pub fn importance_score(&self, time_now: DateTime<Local>) -> f32 {
        let  score = match (self.importance, self.time_importance, self.deadline) {
            (Some(importance), _, Some(deadline)) => {
                let days_since_creation = (deadline - time_now).num_hours() as f32 / 24.0;

                match importance {
                    4 => 1.2_f32.powf(0.5 * days_since_creation + 20.0) + 5.0,
                    3 => 1.17_f32.powf(0.5 * days_since_creation + 20.0) + 5.0,
                    2 => 0.1747502645671 * days_since_creation + 11.3587671968606,
                    1 => 0.0965675735297 * days_since_creation + 6.276892278847,
                    _ => 0.0402194752135 * days_since_creation + 2.6142658953751,
                }
            },
            (_, Some(time_importance), _) => {
                let days_since_creation = (time_now - self.created).num_hours() as f32 / 24.0;

                match time_importance {
                    2 => 1.15_f32.powf(0.4 * days_since_creation + 20.0) - 5.0,
                    1 => 0.5403960772338 * days_since_creation + 8.3798162245677,
                    _ => 0.0440665332331 * days_since_creation + 0.6833311078751,
                }
            },
            (None, None, Some(deadline)) => {
                let time_until_event = (deadline - time_now).abs().num_hours() as f32 / 24.0 + 1.0;
                1000000000.0 / time_until_event
            }
            _ => 1000000000.0, //highlight broken entries
        };

        let random_time = chrono::Local::now();
        let random_variation = (random_time.timestamp_subsec_millis() as f32 / 10000.0) + 1.0;

        return score * random_variation;
    }
    pub fn to_inactive(self) -> InActive {
        InActive {
            id: self.id,
            importance: self.importance,
            name: self.name,
            created: self.created,
            deadline: self.deadline,
            is_event: self.is_event,
            inactivated: chrono::Local::now(),
        }
    }
    pub fn calendar_item_color(&self) -> usize {
        if self.is_event {
            5
        } else if let Some(importance) = self.importance {
            importance as usize
        } else if let Some(time_importance) = self.time_importance {
            time_importance as usize
        } else {
            0
        }
    }
}

/// Group dated items by their deadline day, preserving input order within each
/// day's bucket. The returned vectors borrow from `items`, so the caller can
/// build the calendar with O(1) per-cell lookups instead of re-scanning every
/// item for every day (the old `O(days × items)` rebuild). Items without a
/// deadline are skipped (they are never placed on the grid).
pub fn bucket_by_deadline_day(items: &[Active]) -> HashMap<NaiveDate, Vec<&Active>> {
    let mut buckets: HashMap<NaiveDate, Vec<&Active>> = HashMap::new();
    for item in items {
        if let Some(deadline) = item.deadline {
            buckets.entry(deadline.date_naive()).or_default().push(item);
        }
    }
    buckets
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InActive {
    /// Carried over from the `Active` item so archived rows keep a stable
    /// identity. See `Active::id`.
    #[serde(default)]
    pub id: u64,
    pub importance: Option<u8>,
    pub name: String,
    pub created: DateTime<Local>,
    pub deadline: Option<DateTime<Local>>,
    pub is_event: bool,
    pub inactivated: DateTime<Local>,
}


pub fn get_data_dir(exe_path: &PathBuf) -> Result<PathBuf, Box<dyn Error>> {
    let exe_dir = exe_path.parent().ok_or("Could not find exe directory")?;
    let data_in_exe_dir = exe_dir.join("taskdeck_data");

    if data_in_exe_dir.exists() {
        return Ok(data_in_exe_dir);
    }

    // Fallback for development (e.g., target/debug/app)
    let maybe_project_root = exe_dir
        .parent() // target/
        .and_then(|p| p.parent()); // project root

    let dev_data_path = maybe_project_root
        .ok_or("Could not determine project root for dev mode")?
        .join("taskdeck_data");

    if dev_data_path.exists() {
        Ok(dev_data_path)
    } else {
        Err("Could not locate 'data' directory".into())
    }
}

pub fn read_at_startup(exe_path: &PathBuf) -> Result<Vec<Active>, Box<dyn Error>> {
    let dir_path: PathBuf = get_data_dir(exe_path)?;
    
    let file_path = dir_path.join("read_at_startup.json");
    
    if !file_path.exists() {
        let mut file = File::create(&file_path).expect("failed to create active save JSON file");
        file.write_all(b"[]").expect("failed to write to JSON file");
    }

    let file = File::open(&file_path)?;
    let reader = BufReader::new(file);

    let read_at_startup: Vec<Active> = serde_json::from_reader(reader)?;

    return Ok(read_at_startup);
}

/// Backfill stable ids onto any items that lack one (`id == 0`) — e.g. loaded
/// from a pre-id save file or a hand-edited file. Existing non-zero ids are
/// preserved, and newly assigned ids continue past the current maximum so they
/// never collide. Returns the next free id, used to seed `TaskApp::next_id`.
pub fn assign_missing_ids(items: &mut [Active]) -> u64 {
    let mut next = items.iter().map(|a| a.id).max().unwrap_or(0) + 1;
    for item in items.iter_mut() {
        if item.id == 0 {
            item.id = next;
            next += 1;
        }
    }
    next
}

/// Move a corrupt or unreadable startup data file aside so the app can boot from
/// a clean default instead of panicking. The bad file is renamed to
/// `<file_name>.corrupt-<timestamp>` (preserved for manual recovery), and a
/// human-readable description is returned for display in the error window.
pub fn quarantine_corrupt_file(exe_path: &PathBuf, file_name: &str, cause: &dyn Error) -> String {
    let data_dir = match get_data_dir(exe_path) {
        Ok(dir) => dir,
        // No data directory to quarantine within (e.g. first run / missing dir);
        // there is nothing to move aside, so just report and start from defaults.
        Err(_) => return format!("Could not read {file_name} ({cause}). Started from defaults."),
    };

    let file_path = data_dir.join(file_name);
    if !file_path.exists() {
        return format!("Could not read {file_name} ({cause}). Started from defaults.");
    }

    let timestamp = Local::now().format("%Y%m%d-%H%M%S");
    let quarantine_path = data_dir.join(format!("{file_name}.corrupt-{timestamp}"));

    match fs::rename(&file_path, &quarantine_path) {
        Ok(()) => format!(
            "{file_name} was unreadable ({cause}).\nIt was moved to {} and the app started from defaults.",
            quarantine_path.display()
        ),
        Err(rename_err) => format!(
            "{file_name} was unreadable ({cause}), and it could not be moved aside ({rename_err}). Started from defaults."
        ),
    }
}

pub fn oversafe_activesave(payload: &Vec<Active>, exe_path: &PathBuf) -> Result<(), Box<dyn Error>> {
    // Determine the path to the target JSON file
    let data_dir = get_data_dir(exe_path)?;

    let final_path = data_dir.join("read_at_startup.json");

    // Ensure the directory exists
    fs::create_dir_all(&data_dir)?;

    // Serialize first to avoid writing an invalid file
    let json = serde_json::to_string_pretty(payload)?;

    // Write to a temporary file first
    let mut temp_file = NamedTempFile::new_in(&data_dir)?;
    {
        let mut writer = BufWriter::new(&mut temp_file);
        writer.write_all(json.as_bytes())?;
        writer.flush()?; // Ensure everything's written to the OS buffers
    }

    // Ensure file contents hit disk
    temp_file.as_file_mut().sync_all()?; 

    // Atomically replace the original file
    temp_file.persist(&final_path)?;

    Ok(())
}

pub fn save_inactive(payload: &InActive, exe_path: &PathBuf) -> Result<(), Box<dyn Error>> {
    let data_dir = get_data_dir(exe_path)?;
    let final_path = data_dir.join("archived.jsonl");

    // Ensure the directory exists
    fs::create_dir_all(&data_dir)?;

    let mut json = serde_json::to_string(payload)?;
    json.push_str("\n");

    let mut file = OpenOptions::new().create(true).append(true).open(final_path)?;

    {
        let mut writer = BufWriter::new(&mut file);
        writer.write_all(json.as_bytes())?;
        writer.flush()?;
    }

    Ok(file.sync_all()?)
}

pub fn read_lines_range(offset: usize, limit: usize, exe_path: &PathBuf) -> Result<Vec<InActive>, Box<dyn Error>> {
    let data_dir = get_data_dir(exe_path)?;
    let path = data_dir.join("archived.jsonl");

    let file = File::open(path)?;
    let rev_lines = RevLines::new(file);

    let archives: Vec<InActive> = rev_lines
        .skip(offset)
        .take(limit)
        .filter_map(|line| serde_json::from_str::<InActive>(&line.ok()?).ok())
        .collect();

    Ok(archives)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn active(
        importance: Option<u8>,
        time_importance: Option<u8>,
        is_event: bool,
        deadline: Option<DateTime<Local>>,
    ) -> Active {
        Active {
            id: 0,
            importance,
            time_importance,
            name: "test".to_string(),
            created: Local.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap(),
            deadline,
            is_event,
        }
    }

    #[test]
    fn calendar_item_color_mapping() {
        let dl = Some(Local.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap());
        // Events always map to palette index 5, regardless of importance.
        assert_eq!(active(None, None, true, dl).calendar_item_color(), 5);
        // Deadline tasks map by importance.
        assert_eq!(active(Some(3), None, false, dl).calendar_item_color(), 3);
        // Urgency tasks map by time_importance.
        assert_eq!(active(None, Some(2), false, None).calendar_item_color(), 2);
        // Nothing set falls back to 0.
        assert_eq!(active(None, None, false, None).calendar_item_color(), 0);
    }

    #[test]
    fn importance_score_malformed_is_huge_despite_jitter() {
        // No importance, no time_importance, no deadline hits the "broken entry"
        // branch (1e9), times the <=10% random tie-break multiplier in [1.0, 1.1).
        let now = Local.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap();
        let score = active(None, None, false, None).importance_score(now);
        assert!(score >= 1_000_000_000.0, "score was {score}");
        assert!(score < 1_100_000_000.0, "score was {score}");
    }

    #[test]
    fn importance_score_event_closer_scores_higher() {
        // Event-like items (deadline only) score 1e9 / (|days_to_event| + 1), so a
        // nearer event must outrank a farther one even after the <=10% jitter.
        let now = Local.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap();
        let soon = active(None, None, true, Some(now + chrono::Duration::days(1)));
        let later = active(None, None, true, Some(now + chrono::Duration::days(10)));
        assert!(
            soon.importance_score(now) > later.importance_score(now),
            "nearer event should score higher"
        );
    }

    #[test]
    fn quarantine_moves_corrupt_file_aside() {
        // A fake exe living directly in a dir that contains taskdeck_data/, so
        // get_data_dir resolves to <tmp>/taskdeck_data.
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("taskdeck_data");
        fs::create_dir_all(&data_dir).unwrap();

        let bad_file = data_dir.join("read_at_startup.json");
        fs::write(&bad_file, b"{ this is not valid json").unwrap();

        let fake_exe = tmp.path().join("app.exe");
        let cause = std::io::Error::new(std::io::ErrorKind::InvalidData, "bad json");

        let msg = quarantine_corrupt_file(&fake_exe, "read_at_startup.json", &cause);

        // The corrupt file is moved aside, not left in place...
        assert!(!bad_file.exists(), "corrupt file should have been renamed away");
        // ...to a sibling preserved for manual recovery...
        let quarantined: Vec<_> = fs::read_dir(&data_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("read_at_startup.json.corrupt-")
            })
            .collect();
        assert_eq!(quarantined.len(), 1, "expected exactly one quarantined copy");
        // ...and the message names the file so the error window is meaningful.
        assert!(msg.contains("read_at_startup.json"), "message was {msg}");
    }

    #[test]
    fn quarantine_reports_when_no_file_present() {
        // No taskdeck_data dir at all: nothing to move, but we still get a
        // human-readable message rather than panicking.
        let tmp = tempfile::tempdir().unwrap();
        let fake_exe = tmp.path().join("app.exe");
        let cause = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");

        let msg = quarantine_corrupt_file(&fake_exe, "colorschemes.json", &cause);
        assert!(msg.contains("colorschemes.json"), "message was {msg}");
    }

    #[test]
    fn assign_missing_ids_backfills_and_preserves() {
        // A legacy/hand-edited mix: two unassigned (id 0) items around one that
        // already carries id 5.
        let mut items = vec![
            active(Some(2), None, false, None),
            Active { id: 5, ..active(Some(2), None, false, None) },
            active(Some(2), None, false, None),
        ];

        let next = assign_missing_ids(&mut items);

        // Existing id is untouched; the two zeros get fresh ids past the max.
        assert_eq!(items[1].id, 5, "existing id must be preserved");
        assert_eq!(items[0].id, 6);
        assert_eq!(items[2].id, 7);
        assert_eq!(next, 8, "next free id continues past the assigned maximum");

        // Every item now has a distinct, non-zero id.
        let ids: std::collections::HashSet<u64> = items.iter().map(|a| a.id).collect();
        assert_eq!(ids.len(), items.len());
        assert!(!ids.contains(&0));
    }

    #[test]
    fn assign_missing_ids_starts_at_one_when_empty() {
        let mut items: Vec<Active> = Vec::new();
        assert_eq!(assign_missing_ids(&mut items), 1);
    }

    #[test]
    fn bucket_by_deadline_day_groups_and_preserves_order() {
        let day1_morning = Local.with_ymd_and_hms(2025, 6, 1, 9, 0, 0).unwrap();
        let day1_evening = Local.with_ymd_and_hms(2025, 6, 1, 17, 0, 0).unwrap();
        let day2 = Local.with_ymd_and_hms(2025, 6, 2, 12, 0, 0).unwrap();

        // Two items on day 1 (in this order), one on day 2, one deadline-less.
        let mut a = active(Some(2), None, false, Some(day1_morning));
        a.id = 1;
        let mut b = active(Some(2), None, false, Some(day1_evening));
        b.id = 2;
        let mut c = active(None, None, true, Some(day2));
        c.id = 3;
        let mut d = active(None, Some(1), false, None); // no deadline → skipped
        d.id = 4;

        let items = vec![a, b, c, d];
        let buckets = bucket_by_deadline_day(&items);

        // Only the two distinct deadline days are present (deadline-less skipped).
        assert_eq!(buckets.len(), 2);
        // Day 1's bucket keeps input order.
        let d1: Vec<u64> = buckets[&day1_morning.date_naive()].iter().map(|x| x.id).collect();
        assert_eq!(d1, vec![1, 2]);
        // Day 2 has just the one item.
        let d2: Vec<u64> = buckets[&day2.date_naive()].iter().map(|x| x.id).collect();
        assert_eq!(d2, vec![3]);
    }
}

