use std::{collections::HashMap, error::Error, fs::{self, File, OpenOptions}, io::{BufReader, BufWriter, Write}, path::Path};
use chrono::{DateTime, Duration, Local, NaiveDate};
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
    /// When the user set aside time to **work on** this item, as opposed to
    /// `deadline`, which is when it is **due**. Those are genuinely different
    /// facts — a report due Friday can be written on Tuesday morning — so the
    /// planner gets its own field rather than overloading the deadline and
    /// changing what the task list and calendar mean.
    ///
    /// Only tasks use this. An event's `deadline` already *is* when it happens,
    /// so events are planned by moving their deadline.
    ///
    /// `#[serde(default)]`: absent in pre-planner save files, and serde_json
    /// ignores unknown fields, so saves round-trip through either version.
    #[serde(default)]
    pub planned_start: Option<DateTime<Local>>,
    /// How long the planned block runs, in minutes. `None` means the item has no
    /// extent: an event not yet given a length, or a task's due time. See
    /// `planner::Placement`.
    #[serde(default)]
    pub duration_minutes: Option<u32>,
}

/* ─────────────────────────── Priority scoring ───────────────────────────
 *
 * Every task gets one number, and the task list is sorted by it, highest
 * first. The number is always
 *
 *     score = weight × pressure
 *
 * `weight` is how much the task matters — fixed, chosen by the user.
 * `pressure` is how much it matters *right now* — it moves with the clock.
 *
 * Splitting the two is what makes the scale mean something. Both factors are
 * bounded, so scores from different kinds of task are directly comparable, a
 * far-off deadline can't drown out an imminent one, and nothing can overflow.
 *
 * Two kinds of task, two kinds of pressure:
 *
 *   Dated task    pressure doubles as the deadline nears, reaching 1.0 exactly
 *                 at the deadline and continuing to climb — capped — once late.
 *   Undated task  pressure ripens with age towards 1.0 and stops there, so a
 *                 task nobody dated can rise into view but never shout down
 *                 something that is actually due.
 *
 * The tables below are the whole policy. They are meant to be edited: to make
 * "Highly important" start nagging a week earlier, change one number.
 */

/// How much each importance level counts for, indexed by `Active::importance`.
/// Doubling per level means one step of importance is worth exactly one
/// doubling of time pressure, which is what makes the two comparable.
const WEIGHT_BY_IMPORTANCE: [f32; 5] = [1.0, 2.0, 4.0, 8.0, 16.0];

/// How far ahead of its deadline each importance level starts to feel urgent:
/// the task is at half pressure this far out, and full pressure at the deadline.
/// More important work is noticed earlier, which is the real difference between
/// "lethally important" and "not important" — both are equally due on the day.
///
/// These also bound how long importance out-argues urgency. A task out-ranks a
/// trivial one sitting at *its* deadline for `lead × log2(weight)` days —
/// about a month for the top level, which is roughly how far ahead a big piece
/// of work is worth thinking about. Lengthening a lead time lengthens that
/// dominance too.
const LEAD_DAYS_BY_IMPORTANCE: [f32; 5] = [0.5, 1.0, 2.0, 4.0, 8.0];

/// How much each urgency level counts for, indexed by `Active::time_importance`.
const WEIGHT_BY_URGENCY: [f32; 3] = [1.0, 2.0, 4.0];

/// How long an undated task takes to ripen: it reaches half its weight this
/// long after being created, and approaches full weight from there.
const RIPEN_DAYS_BY_URGENCY: [f32; 3] = [30.0, 10.0, 3.0];

/// Once a deadline is missed, pressure keeps doubling this often. It is the
/// same for every importance level: being late is late, and the weight already
/// says how much this particular lateness matters.
const OVERDUE_DOUBLING_DAYS: f32 = 1.0;

/// Ceiling on overdue pressure, reached two days late.
///
/// Without it a task forgotten for a year would out-score everything else by
/// astronomical margins and the list below it would be meaningless. The value
/// also sets a deliberate boundary: being maximally late is worth two steps of
/// importance, no more. So a trivial task a week overdue reads as about as
/// pressing as an important one due today — a nag, not an emergency — and
/// "lethally important" still beats it from a fortnight out.
const OVERDUE_PRESSURE_CAP: f32 = 4.0;

/// Importance assumed for a dated task that has none. Not reachable from the
/// UI, but a hand-edited or partially-written save can produce it, and treating
/// it as the middle of the scale is far better than treating it as broken.
const ASSUMED_IMPORTANCE: u8 = 2;

/// Score for an item with nothing to go on: no deadline, no importance, no
/// urgency. Deliberately far above any real score (the maximum is
/// `16 × 4 = 64`) so a corrupt entry surfaces at the top of the list where it
/// will be noticed and fixed, rather than hiding at the bottom.
const MALFORMED_SCORE: f32 = 1.0e6;

/// Size of the tie-break jitter: each score is multiplied by a factor in
/// `[1.0, 1.0 + JITTER)`. See `Active::tie_break_jitter`.
const JITTER: f32 = 0.08;

/// Pressure from a deadline. `1.0` exactly at the deadline, halving for every
/// `lead_days` of remaining time, and climbing past `1.0` once overdue.
///
/// Continuous at the deadline and monotonically increasing as time passes,
/// which is the property the whole list ordering rests on.
fn deadline_pressure(days_left: f32, lead_days: f32) -> f32 {
    let lead_days = lead_days.max(0.01);
    if days_left >= 0.0 {
        // Underflows to 0 for absurdly distant deadlines, which is the right
        // answer — such a task has no bearing on today.
        (-days_left / lead_days).exp2()
    } else {
        // `exp2` of a large number is +inf; `min` collapses that to the cap, so
        // no infinity ever reaches the comparator.
        (-days_left / OVERDUE_DOUBLING_DAYS).exp2().min(OVERDUE_PRESSURE_CAP)
    }
}

/// Pressure from age alone, for a task with no deadline. Rises from 0 towards
/// (but never reaching) 1.0, at half after `ripen_days`.
fn ripeness(age_days: f32, ripen_days: f32) -> f32 {
    let ripen_days = ripen_days.max(0.01);
    1.0 - (-age_days.max(0.0) / ripen_days).exp2()
}

fn weight_for(table: &[f32], level: u8) -> f32 {
    table[(level as usize).min(table.len() - 1)]
}

impl Active {
    /// Priority of this task right now. Higher sorts first.
    ///
    /// See the module-level notes above for the model. In short:
    /// `weight × pressure`, where a dated task's pressure doubles as its
    /// deadline approaches and an undated task's ripens with age.
    pub fn importance_score(&self, time_now: DateTime<Local>) -> f32 {
        self.base_score(time_now) * self.tie_break_jitter(time_now)
    }

    /// The score without the tie-break jitter — the part that is a pure
    /// function of the task and the time, and so the part worth testing.
    fn base_score(&self, time_now: DateTime<Local>) -> f32 {
        // A deadline is the strongest thing a task can tell us, so it decides
        // the model whenever there is one. An importance is used if set, and
        // assumed otherwise; a `time_importance` alongside a deadline is
        // ignored rather than given its own precedence puzzle.
        if let Some(deadline) = self.deadline {
            let importance = self.importance.unwrap_or(ASSUMED_IMPORTANCE);
            let days_left = duration_in_days(deadline - time_now);
            return weight_for(&WEIGHT_BY_IMPORTANCE, importance)
                * deadline_pressure(days_left, weight_for(&LEAD_DAYS_BY_IMPORTANCE, importance));
        }

        let age_days = duration_in_days(time_now - self.created);

        // No deadline, but the user said how urgent it is: ripen at that rate.
        if let Some(urgency) = self.time_importance {
            return weight_for(&WEIGHT_BY_URGENCY, urgency)
                * ripeness(age_days, weight_for(&RIPEN_DAYS_BY_URGENCY, urgency));
        }

        // No deadline, but an importance — a shape the UI doesn't produce, but a
        // reasonable one. Carry the importance weight and ripen at the middle
        // rate rather than calling it broken.
        if let Some(importance) = self.importance {
            return weight_for(&WEIGHT_BY_IMPORTANCE, importance)
                * ripeness(age_days, RIPEN_DAYS_BY_URGENCY[1]);
        }

        MALFORMED_SCORE
    }

    /// A small per-task, per-rebuild multiplier in `[1.0, 1.0 + JITTER)`.
    ///
    /// This keeps the intentional gentle shuffle described in
    /// `DOCUMENTATION.md` §14.4 — the list shouldn't look frozen — while
    /// actually delivering one. The old version read the current millisecond
    /// *at the moment of the call*, so every task scored in the same
    /// millisecond got the identical multiplier and nothing was shuffled at
    /// all; when a rebuild happened to straddle a millisecond boundary, an
    /// arbitrary subset jumped by up to 10% instead. Hashing the task's id with
    /// the rebuild's timestamp gives each task its own factor, stable within a
    /// rebuild and different in the next one.
    ///
    /// It is deliberately small: enough to keep near-ties moving, never enough
    /// to reorder tasks that genuinely differ in priority.
    fn tie_break_jitter(&self, time_now: DateTime<Local>) -> f32 {
        // splitmix64, so the id and the timestamp are properly mixed rather
        // than merely added — adjacent ids must not produce adjacent factors.
        let mut mixed = self
            .id
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (time_now.timestamp() as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        mixed ^= mixed >> 30;
        mixed = mixed.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        mixed ^= mixed >> 27;
        mixed = mixed.wrapping_mul(0x94D0_49BB_1331_11EB);
        mixed ^= mixed >> 31;

        // Top 24 bits, scaled into [0, 1).
        let unit = (mixed >> 40) as f32 / (1u64 << 24) as f32;
        1.0 + unit * JITTER
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

    /// The instant this item occupies on the planner's timeline, if any: an
    /// event sits at its `deadline`, a task at the time set aside for it.
    /// `None` for an unplanned, deadline-less task — those live in the planner's
    /// backlog tray instead.
    pub fn planner_anchor(&self) -> Option<DateTime<Local>> {
        if self.is_event {
            self.deadline
        } else {
            self.planned_start.or(self.deadline)
        }
    }

    /// True when the item has been given time on the planner, as opposed to
    /// merely having a due date. Events count as planned once they have a
    /// length; a task counts once it has a `planned_start`.
    pub fn is_planned(&self) -> bool {
        if self.is_event {
            self.deadline.is_some() && self.duration_minutes.is_some()
        } else {
            self.planned_start.is_some()
        }
    }
}

/// A `Duration` as fractional days.
///
/// Minutes rather than the old `num_hours()`, which truncated: a task due in 90
/// minutes and one due in 110 scored identically for the whole hour between.
fn duration_in_days(duration: Duration) -> f32 {
    duration.num_minutes() as f32 / (24.0 * 60.0)
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


pub fn read_at_startup(data_dir: &Path) -> Result<Vec<Active>, Box<dyn Error>> {
    let file_path = data_dir.join("read_at_startup.json");

    if !file_path.exists() {
        // An empty JSON array is what an "no tasks yet" save looks like; a
        // failure to seed it is reported like any other read failure rather
        // than aborting the boot.
        fs::write(&file_path, b"[]")?;
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
pub fn quarantine_corrupt_file(data_dir: &Path, file_name: &str, cause: &dyn Error) -> String {
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

pub fn oversafe_activesave(payload: &Vec<Active>, data_dir: &Path) -> Result<(), Box<dyn Error>> {
    let final_path = data_dir.join("read_at_startup.json");

    // Ensure the directory exists (it may have been removed while running)
    fs::create_dir_all(data_dir)?;

    // Serialize first to avoid writing an invalid file
    let json = serde_json::to_string_pretty(payload)?;

    // Write to a temporary file first
    let mut temp_file = NamedTempFile::new_in(data_dir)?;
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

pub fn save_inactive(payload: &InActive, data_dir: &Path) -> Result<(), Box<dyn Error>> {
    let final_path = data_dir.join("archived.jsonl");

    // Ensure the directory exists (it may have been removed while running)
    fs::create_dir_all(data_dir)?;

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

pub fn read_lines_range(offset: usize, limit: usize, data_dir: &Path) -> Result<Vec<InActive>, Box<dyn Error>> {
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
            planned_start: None,
            duration_minutes: None,
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

    /// A dated task, `days` from its deadline (negative = overdue).
    fn dated(importance: u8, days: f64, now: DateTime<Local>) -> Active {
        Active {
            deadline: Some(now + Duration::minutes((days * 1440.0) as i64)),
            ..active(Some(importance), None, false, None)
        }
    }

    /// An undated task of the given urgency, created `age` days ago.
    fn undated(urgency: u8, age: f64, now: DateTime<Local>) -> Active {
        Active {
            created: now - Duration::minutes((age * 1440.0) as i64),
            ..active(None, Some(urgency), false, None)
        }
    }

    fn noon() -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap()
    }

    #[test]
    fn score_rises_as_a_deadline_approaches() {
        let now = noon();
        // The property the whole list rests on, and the one the previous model
        // had backwards: it scored a task *higher* the further away its deadline
        // was, so deadlines sank as they approached and overdue work ended up at
        // the bottom of the list.
        for importance in 0..=4 {
            let far = dated(importance, 30.0, now).base_score(now);
            let near = dated(importance, 1.0, now).base_score(now);
            let due = dated(importance, 0.0, now).base_score(now);
            let late = dated(importance, -2.0, now).base_score(now);

            assert!(far < near, "importance {importance}: {far} !< {near}");
            assert!(near < due, "importance {importance}: {near} !< {due}");
            assert!(due < late, "importance {importance}: {due} !< {late}");
        }
    }

    #[test]
    fn pressure_is_exactly_one_at_the_deadline() {
        let now = noon();
        // At the deadline a task is worth precisely its weight, which is what
        // makes weights comparable across the two models.
        for importance in 0..=4u8 {
            let score = dated(importance, 0.0, now).base_score(now);
            assert!(
                (score - WEIGHT_BY_IMPORTANCE[importance as usize]).abs() < 0.001,
                "importance {importance} scored {score}"
            );
        }
    }

    #[test]
    fn importance_separates_tasks_sharing_a_deadline() {
        let now = noon();
        let scores: Vec<f32> = (0..=4).map(|i| dated(i, 3.0, now).base_score(now)).collect();
        for pair in scores.windows(2) {
            assert!(pair[0] < pair[1], "importance should break the tie: {scores:?}");
        }
    }

    #[test]
    fn importance_stops_out_arguing_urgency_eventually() {
        let now = noon();
        // Importance wins near-term: a lethally important task a fortnight out
        // is above a trivial one due today, which is what lets big work surface
        // early enough to plan.
        let trivial_due_now = dated(0, 0.0, now).base_score(now);
        assert!(dated(4, 14.0, now).base_score(now) > trivial_due_now);

        // But its reach is bounded. Far enough out, the thing actually due today
        // takes the top — the old model had no such crossover at all, and ranked
        // the distant task higher no matter how far away it was.
        assert!(dated(4, 45.0, now).base_score(now) < trivial_due_now);
    }

    #[test]
    fn a_distant_deadline_still_ranks_by_importance() {
        let now = noon();
        // ...but at the same distance, the big thing is still the bigger thing.
        assert!(dated(4, 30.0, now).base_score(now) > dated(0, 30.0, now).base_score(now));
    }

    #[test]
    fn overdue_pressure_levels_off_at_the_cap() {
        let now = noon();
        // A task forgotten for a year must not out-score everything else by an
        // astronomical margin — past the cap, lateness stops adding.
        for importance in 0..=4u8 {
            let capped = WEIGHT_BY_IMPORTANCE[importance as usize] * OVERDUE_PRESSURE_CAP;
            let a_week = dated(importance, -7.0, now).base_score(now);
            let a_year = dated(importance, -365.0, now).base_score(now);
            assert!((a_week - capped).abs() < 0.001, "{a_week} != {capped}");
            assert_eq!(a_week, a_year, "lateness past the cap should stop counting");
        }
    }

    #[test]
    fn a_late_trivial_task_does_not_bury_important_upcoming_work() {
        let now = noon();
        // The cap is chosen so this holds: the most overdue a trivial task can
        // be still sits below a lethally important one due in a fortnight.
        let maximally_late_trivial = dated(0, -365.0, now).base_score(now);
        let important_upcoming = dated(4, 14.0, now).base_score(now);
        assert!(
            important_upcoming > maximally_late_trivial,
            "{important_upcoming} should beat {maximally_late_trivial}"
        );
    }

    #[test]
    fn undated_tasks_ripen_towards_their_weight_and_stop() {
        let now = noon();
        for urgency in 0..=2u8 {
            let weight = WEIGHT_BY_URGENCY[urgency as usize];
            let fresh = undated(urgency, 0.0, now).base_score(now);
            let ripening = undated(urgency, RIPEN_DAYS_BY_URGENCY[urgency as usize] as f64, now).base_score(now);
            let ancient = undated(urgency, 3650.0, now).base_score(now);

            assert!(fresh < ripening && ripening < ancient, "urgency {urgency} should ripen");
            // Half weight at the ripening time, by construction.
            assert!((ripening - weight / 2.0).abs() < 0.01, "{ripening} != half of {weight}");
            // And never past its weight, however long it sits.
            assert!(ancient <= weight, "{ancient} exceeded its weight {weight}");
        }
    }

    #[test]
    fn a_ripe_undated_task_never_outranks_comparable_overdue_work() {
        let now = noon();
        // An undated task can rise into view — that is the point of urgency —
        // but "I keep meaning to" must not outrank a missed deadline of the same
        // standing. (A *heavier* undated task outranking a trivial overdue one is
        // correct: that is what the weights are for.)
        let ripest = undated(2, 3650.0, now).base_score(now);
        let ripest_weight = WEIGHT_BY_URGENCY[2];
        for importance in 0..=4u8 {
            if WEIGHT_BY_IMPORTANCE[importance as usize] < ripest_weight {
                continue;
            }
            let overdue = dated(importance, -1.0, now).base_score(now);
            assert!(overdue > ripest, "overdue {overdue} should beat ripe {ripest}");
        }
    }

    #[test]
    fn scores_stay_finite_at_absurd_distances() {
        let now = noon();
        // Far-future deadlines used to overflow to +inf, which made every such
        // task compare exactly equal (CODE_REVIEW E9). Now they underflow
        // towards zero instead, which orders correctly and can't produce NaN.
        for importance in 0..=4 {
            let far = dated(importance, 365.0 * 200.0, now).base_score(now);
            let late = dated(importance, -365.0 * 200.0, now).base_score(now);
            assert!(far.is_finite() && far >= 0.0, "far score was {far}");
            assert!(late.is_finite(), "late score was {late}");
        }
    }

    #[test]
    fn a_dated_task_missing_its_importance_is_scored_as_middling() {
        let now = noon();
        // Not a shape the UI makes, but a hand-edited save can. Treating it as
        // the middle of the scale beats treating it as broken.
        let no_importance = Active {
            deadline: Some(now),
            ..active(None, None, false, None)
        };
        let middling = dated(ASSUMED_IMPORTANCE, 0.0, now).base_score(now);
        assert!((no_importance.base_score(now) - middling).abs() < 0.001);
    }

    #[test]
    fn an_item_with_nothing_to_go_on_is_surfaced_at_the_top() {
        let now = noon();
        let nothing = active(None, None, false, None).base_score(now);
        // Above every reachable real score, so a corrupt entry gets noticed.
        let highest_real = WEIGHT_BY_IMPORTANCE[4] * OVERDUE_PRESSURE_CAP;
        assert!(nothing > highest_real, "{nothing} should stand out from {highest_real}");
    }

    #[test]
    fn an_event_closer_in_time_scores_higher() {
        let now = noon();
        let soon = active(None, None, true, Some(now + Duration::days(1)));
        let later = active(None, None, true, Some(now + Duration::days(10)));
        assert!(
            soon.importance_score(now) > later.importance_score(now),
            "nearer event should score higher"
        );
    }

    #[test]
    fn jitter_is_small_bounded_and_actually_varies_per_task() {
        let now = noon();
        let factors: Vec<f32> = (1..=64u64)
            .map(|id| Active { id, ..active(Some(2), None, false, None) }.tie_break_jitter(now))
            .collect();

        for factor in &factors {
            assert!((1.0..1.0 + JITTER).contains(factor), "factor {factor} out of range");
        }

        // The point of the rewrite: the old version read the clock at call time,
        // so every task scored in the same millisecond got an identical factor
        // and nothing was shuffled at all. Distinct ids must give distinct
        // factors.
        let distinct = factors
            .iter()
            .map(|f| f.to_bits())
            .collect::<std::collections::HashSet<_>>()
            .len();
        assert!(distinct > 60, "only {distinct} distinct factors out of 64");
    }

    #[test]
    fn jitter_is_stable_within_a_rebuild_and_moves_between_them() {
        let now = noon();
        let task = Active { id: 7, ..active(Some(2), None, false, None) };

        // Stable for a given instant, so one sort is self-consistent.
        assert_eq!(task.tie_break_jitter(now), task.tie_break_jitter(now));
        // ...and different a moment later, which is the gentle shuffle.
        assert_ne!(task.tie_break_jitter(now), task.tie_break_jitter(now + Duration::seconds(1)));
    }

    #[test]
    fn jitter_cannot_reorder_tasks_that_genuinely_differ() {
        let now = noon();
        // One importance step is a factor of two; the jitter is 8%. It should
        // never be able to flip a real difference in priority.
        let lower = dated(2, 0.0, now);
        let higher = dated(3, 0.0, now);
        assert!(higher.importance_score(now) > lower.importance_score(now));
    }

    #[test]
    fn quarantine_moves_corrupt_file_aside() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("taskdeck_data");
        fs::create_dir_all(&data_dir).unwrap();

        let bad_file = data_dir.join("read_at_startup.json");
        fs::write(&bad_file, b"{ this is not valid json").unwrap();

        let cause = std::io::Error::new(std::io::ErrorKind::InvalidData, "bad json");

        let msg = quarantine_corrupt_file(&data_dir, "read_at_startup.json", &cause);

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
        // An empty data dir: nothing to move, but we still get a human-readable
        // message rather than panicking.
        let tmp = tempfile::tempdir().unwrap();
        let cause = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");

        let msg = quarantine_corrupt_file(tmp.path(), "colorschemes.json", &cause);
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

