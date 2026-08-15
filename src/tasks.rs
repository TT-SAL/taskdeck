use std::{collections::HashMap, error::Error, fs::{self, File}, io::{BufReader, BufWriter, Write}, path::Path};
use chrono::{DateTime, Duration, Local, NaiveDate};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

/// One planned block of work: when it starts, and for how long.
///
/// A task owns a *list* of these, because "when will I do it" is not always one
/// answer — two hours of homework due Friday may be an hour on Tuesday and an
/// hour on Thursday. The task itself stays whole: sessions are reserved time,
/// not sub-tasks, so there is still one record and one ✓, and completing the
/// task simply releases them all.
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub struct Session {
    pub start: DateTime<Local>,
    pub minutes: u32,
}

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
    /// The time set aside to **work on** this task, as opposed to `deadline`,
    /// which is when it is **due**. Those are genuinely different facts — a
    /// report due Friday can be written on Tuesday morning — and they can
    /// differ more than once: the same report may be worked on Tuesday *and*
    /// Wednesday. So a task carries a list of sessions rather than one slot.
    ///
    /// Only tasks use this. An event's `deadline` already *is* when it happens,
    /// so events are planned by moving that, and their length lives in
    /// `duration_minutes`.
    ///
    /// `#[serde(default)]`: absent in older save files, and serde_json ignores
    /// unknown fields, so saves round-trip through older builds (which simply
    /// don't see the plans).
    #[serde(default)]
    pub sessions: Vec<Session>,
    /// **Legacy.** The single planned slot from before tasks could carry more
    /// than one. Read so old saves load; `migrate_legacy_plans` folds it into
    /// `sessions` at startup and it is `None` from then on. Nothing else may
    /// read or write it.
    #[serde(default)]
    pub planned_start: Option<DateTime<Local>>,
    /// How long this takes, in minutes.
    ///
    /// For an event, the length of its block. For a **task**, an *estimate* —
    /// "the physics homework takes two hours" is a fact about the task, not
    /// about any particular slot, so it lives here rather than on a session,
    /// and it survives planning and unplanning. The planner spends it twice:
    /// dragging the task out of the tray lands a block of the *remaining*
    /// unplanned time (`Active::remaining_minutes`), and the tray card shows
    /// how much of the estimate is still unplaced.
    ///
    /// `None` means no extent and no estimate: an event not yet given a length,
    /// or a task nobody has sized.
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

/* ── The horizon: an undated task's whole input ──────────────────────────
 *
 * A dated task asks two questions: when is it due (the deadline), and how bad
 * is missing that (the 5-level importance — severity). An undated task asks
 * only one: *roughly how soon should this happen?* "Book a doctor's
 * appointment" has no due date and no meaningful severity ranking; what its
 * owner actually knows is "within a week or so". That answer is the horizon,
 * and it drives both how fast the task ripens and how much it weighs once
 * ripe.
 *
 * The horizon is stored in `Active::time_importance`, and the first three
 * indices are **load-compatible** with the old 3-level "urgency" scale (same
 * weights: 1, 2, 4), so old saves mean today what they meant then. That is why
 * the arrays are ordered month → week → days with "whenever" appended at
 * index 3, even though the UI presents them soonest-first: the index order is
 * a serialization fact, the display order a presentation one.
 */

/// How much a ripened task of each horizon counts for, indexed by
/// `Active::time_importance`: within a month / a week / days / whenever.
///
/// "Whenever" weighs half of the lowest dated tier — a parking-lot idea should
/// eventually surface, but never by out-arguing anything with a real claim.
const WEIGHT_BY_HORIZON: [f32; 4] = [1.0, 2.0, 4.0, 0.5];

/// How long a task of each horizon takes to ripen: it reaches half its weight
/// this long after being created, and approaches full weight from there. The
/// horizon *is* this number, worn openly — "within a week" ripens over ten
/// days.
const RIPEN_DAYS_BY_HORIZON: [f32; 4] = [30.0, 10.0, 3.0, 90.0];

/// Index of the "whenever" horizon: the appended parking-lot tier.
pub const HORIZON_WHENEVER: u8 = 3;

/// How far ahead of a *planned* slot a task starts to feel pressing. Short by
/// design: a plan says "do it at this time", so it should climb into view over
/// the hours before its slot rather than days ahead the way a deadline does.
const PLANNED_LEAD_DAYS: f32 = 0.5;

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
    /// `shuffle_seed` changes once per list rebuild, not with the clock — see
    /// `tie_break_jitter`.
    pub fn importance_score(&self, time_now: DateTime<Local>, shuffle_seed: u64) -> f32 {
        self.base_score(time_now) * self.tie_break_jitter(shuffle_seed)
    }

    /// The score without the tie-break jitter — the part that is a pure
    /// function of the task and the time, and so the part worth testing.
    fn base_score(&self, time_now: DateTime<Local>) -> f32 {
        // Pressure from planned sessions, if the task has any. The *most
        // pressing* session decides: with work booked for this afternoon and
        // more for Thursday, this afternoon is what matters now. Capped at
        // 1.0: a session that has come and gone is a plan you didn't keep,
        // which is not the same thing as a missed deadline and shouldn't
        // escalate like one.
        let planned_pressure = self
            .sessions
            .iter()
            .map(|session| {
                deadline_pressure(duration_in_days(session.start - time_now), PLANNED_LEAD_DAYS)
                    .min(1.0)
            })
            .fold(None, |best: Option<f32>, pressure| {
                Some(best.map_or(pressure, |b| b.max(pressure)))
            });

        // A deadline is the strongest thing a task can tell us, so it decides
        // the model whenever there is one. An importance is used if set, and
        // assumed otherwise; a `time_importance` alongside a deadline is
        // ignored rather than given its own precedence puzzle.
        if let Some(deadline) = self.deadline {
            let importance = self.importance.unwrap_or(ASSUMED_IMPORTANCE);
            let days_left = duration_in_days(deadline - time_now);
            let from_deadline =
                deadline_pressure(days_left, weight_for(&LEAD_DAYS_BY_IMPORTANCE, importance));
            // Whichever reason is more pressing wins. A report due Friday that
            // you set aside Tuesday morning for should rise on Tuesday morning:
            // that is when you decided to do it.
            let pressure = from_deadline.max(planned_pressure.unwrap_or(0.0));
            return weight_for(&WEIGHT_BY_IMPORTANCE, importance) * pressure;
        }

        let age_days = duration_in_days(time_now - self.created);
        let weight = match (self.importance, self.time_importance) {
            (Some(importance), _) => weight_for(&WEIGHT_BY_IMPORTANCE, importance),
            (None, Some(horizon)) => weight_for(&WEIGHT_BY_HORIZON, horizon),
            (None, None) if !self.sessions.is_empty() => {
                weight_for(&WEIGHT_BY_IMPORTANCE, ASSUMED_IMPORTANCE)
            }
            (None, None) => return MALFORMED_SCORE,
        };

        // Undated but planned: the sessions are the only timing the task has,
        // so they drive the pressure. Without this a task blocked out for this
        // afternoon scored as if it were brand new — bottom of the list, on the
        // very day time was set aside for it.
        if let Some(pressure) = planned_pressure {
            return weight * pressure;
        }

        // Undated and unplanned: ripen with age, over the horizon the user
        // gave it, or the middle rate for a task that only carries an
        // importance (a shape the UI doesn't produce, but a reasonable one).
        let ripen_days = match self.time_importance {
            Some(horizon) => weight_for(&RIPEN_DAYS_BY_HORIZON, horizon),
            None => RIPEN_DAYS_BY_HORIZON[1],
        };
        weight * ripeness(age_days, ripen_days)
    }

    /// A small per-task multiplier in `[1.0, 1.0 + JITTER)`, constant for a
    /// given `shuffle_seed`.
    ///
    /// This keeps the intentional gentle shuffle described in
    /// `DOCUMENTATION.md` §14.4 — the list shouldn't look frozen — while
    /// actually delivering one. The original read the current millisecond *at
    /// the moment of the call*, so every task scored in the same millisecond
    /// got the identical multiplier and nothing was shuffled at all; when a
    /// rebuild straddled a millisecond boundary, an arbitrary subset jumped by
    /// up to 10% instead.
    ///
    /// The seed is a **rebuild counter**, deliberately not the clock. Keyed on
    /// the time, any list that re-sorts every frame — the planner's backlog
    /// tray does — reshuffled once a second, so cards crawled out from under
    /// the pointer as you reached for one. A counter shuffles exactly when
    /// §14.4 says it should: when the list is actually rebuilt.
    ///
    /// It is deliberately small: enough to keep near-ties moving, never enough
    /// to reorder tasks that genuinely differ in priority.
    fn tie_break_jitter(&self, shuffle_seed: u64) -> f32 {
        // splitmix64, so the id and the seed are properly mixed rather than
        // merely added — adjacent ids must not produce adjacent factors.
        let mut mixed = self
            .id
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ shuffle_seed.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        mixed ^= mixed >> 30;
        mixed = mixed.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        mixed ^= mixed >> 27;
        mixed = mixed.wrapping_mul(0x94D0_49BB_1331_11EB);
        mixed ^= mixed >> 31;

        // Top 24 bits, scaled into [0, 1).
        let unit = (mixed >> 40) as f32 / (1u64 << 24) as f32;
        1.0 + unit * JITTER
    }
    pub fn calendar_item_color(&self) -> usize {
        calendar_item_color(self.is_event, self.importance, self.time_importance)
    }

    /// The instant this item occupies on the planner's timeline, if any: an
    /// event sits at its `deadline`, a task at the earliest time set aside for
    /// it (falling back to its deadline). `None` for an unplanned,
    /// deadline-less task — those live in the planner's tray instead.
    pub fn planner_anchor(&self) -> Option<DateTime<Local>> {
        if self.is_event {
            self.deadline
        } else {
            self.sessions.iter().map(|s| s.start).min().or(self.deadline)
        }
    }

    /// True when the item has been given time on the planner, as opposed to
    /// merely having a due date. Events count as planned once they have a
    /// length; a task counts once it has at least one session.
    pub fn is_planned(&self) -> bool {
        if self.is_event {
            self.deadline.is_some() && self.duration_minutes.is_some()
        } else {
            !self.sessions.is_empty()
        }
    }

    /// Total minutes of this task's planned sessions, across all days.
    pub fn planned_minutes(&self) -> u32 {
        self.sessions.iter().map(|s| s.minutes).sum()
    }

    /// Minutes of the estimate not yet covered by sessions, if the task has an
    /// estimate. `Some(0)` is meaningful: fully planned.
    pub fn remaining_minutes(&self) -> Option<u32> {
        self.duration_minutes
            .map(|estimate| estimate.saturating_sub(self.planned_minutes()))
    }

    /// True while the task still belongs in the planner's tray: it has no
    /// sessions at all, or its estimate isn't yet covered by the sessions it
    /// has. A 2-hour task with one hour booked is still half a card.
    pub fn wants_planning(&self) -> bool {
        if self.is_event {
            return false;
        }
        self.sessions.is_empty() || self.remaining_minutes().is_some_and(|left| left > 0)
    }
}

/// Which of the six palette entries an item wears.
///
/// A free function rather than a method because an archived item wants the same
/// answer and is no longer an `Active` — the planner draws a completed task's
/// old blocks in the colour it had in life (`archive::Archived::color_id`).
pub fn calendar_item_color(
    is_event: bool,
    importance: Option<u8>,
    time_importance: Option<u8>,
) -> usize {
    if is_event {
        5
    } else if let Some(importance) = importance {
        importance as usize
    } else if let Some(horizon) = time_importance {
        // The horizon indices are load-compatible with the old 3-level
        // urgency scale, which forced "whenever" to take index 3 — but as
        // the *least* pressing tier it wears the calmest colour, not the
        // "highly important" one that index would buy it.
        if horizon >= HORIZON_WHENEVER { 0 } else { horizon as usize }
    } else {
        0
    }
}

/// Fold the pre-sessions single slot (`planned_start` + `duration_minutes`)
/// into a session, once, at load. Nothing else reads `planned_start`; after
/// this it stays `None` and the next save writes the migrated shape.
pub fn migrate_legacy_plans(items: &mut [Active]) {
    for item in items.iter_mut() {
        let Some(start) = item.planned_start.take() else { continue };
        if item.is_event || !item.sessions.is_empty() {
            continue;
        }
        let minutes = item
            .duration_minutes
            .unwrap_or(crate::planner::DEFAULT_BLOCK_MINUTES)
            .max(crate::planner::MIN_BLOCK_MINUTES);
        item.sessions.push(Session { start, minutes });
        // The block's length was also the best available estimate, and
        // `duration_minutes` keeps meaning that for tasks.
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

/* The archived counterpart of `Active` used to live here, as `InActive`: the
 * same item with most of it missing (no sessions, no estimate, no horizon) and
 * no record of *how* it left. It is now `archive::Archived`, which keeps the
 * whole item and says whether it was finished or dropped — see `archive.rs`
 * for why that is the difference between a receipt and a record. */

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

/* Writing and paging the archive lived here too — `save_inactive` and
 * `read_lines_range`, the latter reverse-scanning the whole log past `offset`
 * lines on every "Show more". Both are `archive::ArchiveLog`'s job now: it
 * reads the log once and keeps it, which is what searching and summarising it
 * need and what retires the quadratic paging. */

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
            sessions: Vec::new(),
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
        // Horizon tasks map by time_importance.
        assert_eq!(active(None, Some(2), false, None).calendar_item_color(), 2);
        // "Whenever" is index 3 for save-compatibility but wears the calmest
        // colour, not the highly-important one.
        assert_eq!(active(None, Some(HORIZON_WHENEVER), false, None).calendar_item_color(), 0);
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

    /// An undated task of the given horizon, created `age` days ago.
    fn undated(horizon: u8, age: f64, now: DateTime<Local>) -> Active {
        Active {
            created: now - Duration::minutes((age * 1440.0) as i64),
            ..active(None, Some(horizon), false, None)
        }
    }

    /// A task with one planned session starting at `start`.
    fn planned(importance: Option<u8>, start: DateTime<Local>, minutes: u32) -> Active {
        Active {
            sessions: vec![Session { start, minutes }],
            ..active(importance, None, false, None)
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
        for horizon in 0..=3u8 {
            let weight = WEIGHT_BY_HORIZON[horizon as usize];
            let fresh = undated(horizon, 0.0, now).base_score(now);
            let ripening = undated(horizon, RIPEN_DAYS_BY_HORIZON[horizon as usize] as f64, now).base_score(now);
            let ancient = undated(horizon, 3650.0, now).base_score(now);

            assert!(fresh < ripening && ripening < ancient, "horizon {horizon} should ripen");
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
        let ripest_weight = WEIGHT_BY_HORIZON[2];
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
    fn a_planned_task_rises_as_its_slot_approaches() {
        let now = noon();
        // A task dragged out on the planner gets a slot but no deadline — a plan
        // is not a due date. Without the planned-slot term it scored purely on
        // age, so a task blocked out for this afternoon sat at the bottom of the
        // list on the very day time was set aside for it.
        let planned_at = |hours_ahead: i64| planned(Some(2), now + Duration::hours(hours_ahead), 60);

        let tomorrow = planned_at(24).base_score(now);
        let this_evening = planned_at(6).base_score(now);
        let imminent = planned_at(1).base_score(now);

        assert!(tomorrow < this_evening, "{tomorrow} !< {this_evening}");
        assert!(this_evening < imminent, "{this_evening} !< {imminent}");
    }

    #[test]
    fn a_slipped_plan_does_not_escalate_like_a_missed_deadline() {
        let now = noon();
        let slipped = |days_ago: i64| planned(Some(2), now - Duration::days(days_ago), 60);

        // A plan you didn't keep tops out at the task's weight and stays there.
        // A missed *deadline* keeps climbing past it — the difference between
        // "I meant to do that" and "that was due".
        let weight = WEIGHT_BY_IMPORTANCE[2];
        assert!((slipped(1).base_score(now) - weight).abs() < 0.001);
        assert_eq!(slipped(1).base_score(now), slipped(100).base_score(now));
        assert!(dated(2, -1.0, now).base_score(now) > slipped(100).base_score(now));
    }

    #[test]
    fn planning_a_dated_task_lifts_it_on_the_day() {
        let now = noon();
        // Due in three days, and set aside for right now. The plan is the more
        // pressing of the two reasons, so it wins — that is when you decided to
        // do it.
        let due_friday = dated(3, 3.0, now);
        let due_friday_planned_now = Active {
            sessions: vec![Session { start: now + Duration::minutes(30), minutes: 60 }],
            ..due_friday.clone()
        };
        assert!(
            due_friday_planned_now.base_score(now) > due_friday.base_score(now),
            "planning a task for now should lift it"
        );

        // But a plan never *lowers* a task: the deadline still applies if it is
        // the more pressing of the two.
        let due_today_planned_next_week = Active {
            sessions: vec![Session { start: now + Duration::days(7), minutes: 60 }],
            ..dated(3, 0.0, now)
        };
        assert!(
            (due_today_planned_next_week.base_score(now) - dated(3, 0.0, now).base_score(now)).abs()
                < 0.001
        );
    }

    #[test]
    fn a_planned_task_with_no_importance_is_still_scored() {
        let now = noon();
        // Nothing but a slot: weight falls back to the middle of the scale
        // rather than the task being treated as corrupt.
        let bare = planned(None, now, 60);
        let score = bare.base_score(now);
        assert!((score - WEIGHT_BY_IMPORTANCE[ASSUMED_IMPORTANCE as usize]).abs() < 0.001);
        assert!(score < MALFORMED_SCORE);
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
    fn the_most_pressing_session_drives_planned_pressure() {
        let now = noon();
        // One session this afternoon, one next week: the afternoon one is what
        // matters now, and adding a far-off second session must never *lower*
        // the score below what the near one justifies.
        let near_only = planned(Some(2), now + Duration::hours(2), 60);
        let both = Active {
            sessions: vec![
                Session { start: now + Duration::hours(2), minutes: 60 },
                Session { start: now + Duration::days(7), minutes: 60 },
            ],
            ..active(Some(2), None, false, None)
        };
        assert!((both.base_score(now) - near_only.base_score(now)).abs() < 0.001);
    }

    #[test]
    fn whenever_surfaces_eventually_but_out_argues_nothing() {
        let now = noon();
        // The parking-lot tier still ripens...
        let fresh = undated(HORIZON_WHENEVER, 0.0, now).base_score(now);
        let old = undated(HORIZON_WHENEVER, 365.0, now).base_score(now);
        assert!(fresh < old, "{fresh} !< {old}");
        // ...but even fully ripe it stays under a ripened month-horizon task,
        // let alone anything dated.
        let month = undated(0, 365.0, now).base_score(now);
        assert!(old < month, "{old} !< {month}");
        assert!(old < dated(0, 0.0, now).base_score(now));
    }

    #[test]
    fn sessions_report_planned_and_remaining_time() {
        let now = noon();
        let mut task = active(Some(2), None, false, None);
        task.duration_minutes = Some(120);
        assert_eq!(task.planned_minutes(), 0);
        assert_eq!(task.remaining_minutes(), Some(120));
        assert!(task.wants_planning(), "an unplanned estimated task is a tray card");

        task.sessions.push(Session { start: now, minutes: 45 });
        assert_eq!(task.planned_minutes(), 45);
        assert_eq!(task.remaining_minutes(), Some(75));
        assert!(task.wants_planning(), "45m of a 2h estimate planned: still half a card");

        task.sessions.push(Session { start: now + Duration::days(1), minutes: 90 });
        assert_eq!(task.remaining_minutes(), Some(0), "over-planning clamps to zero");
        assert!(!task.wants_planning(), "estimate covered: the card is done");

        // No estimate: one session is enough to leave the tray.
        let mut unsized_task = active(Some(2), None, false, None);
        assert!(unsized_task.wants_planning());
        unsized_task.sessions.push(Session { start: now, minutes: 30 });
        assert!(!unsized_task.wants_planning());
        assert_eq!(unsized_task.remaining_minutes(), None);
    }

    #[test]
    fn legacy_single_slots_migrate_into_sessions() {
        let now = noon();
        let mut items = vec![
            // A pre-sessions planned task: slot + length.
            Active {
                planned_start: Some(now),
                duration_minutes: Some(90),
                ..active(Some(2), None, false, None)
            },
            // A pre-sessions planned task that somehow lost its length.
            Active {
                planned_start: Some(now),
                ..active(Some(2), None, false, None)
            },
            // An event with a stray planned_start (hand-edited): dropped, an
            // event's time is its deadline.
            Active {
                planned_start: Some(now),
                ..active(None, None, true, Some(now))
            },
        ];
        migrate_legacy_plans(&mut items);

        assert_eq!(items[0].sessions, vec![Session { start: now, minutes: 90 }]);
        assert_eq!(items[0].duration_minutes, Some(90), "the length stays as the estimate");
        assert_eq!(items[1].sessions.len(), 1);
        assert!(items[1].sessions[0].minutes >= crate::planner::MIN_BLOCK_MINUTES);
        assert!(items[2].sessions.is_empty());
        for item in &items {
            assert_eq!(item.planned_start, None, "the legacy field is spent");
        }

        // Running the migration again must not duplicate anything.
        migrate_legacy_plans(&mut items);
        assert_eq!(items[0].sessions.len(), 1);
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
            soon.importance_score(now, 0) > later.importance_score(now, 0),
            "nearer event should score higher"
        );
    }

    #[test]
    fn jitter_is_small_bounded_and_actually_varies_per_task() {
        let now = noon();
        let factors: Vec<f32> = (1..=64u64)
            .map(|id| Active { id, ..active(Some(2), None, false, None) }.tie_break_jitter(7))
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
        let task = Active { id: 7, ..active(Some(2), None, false, None) };

        // Constant for a given seed, so a list that re-sorts every frame — the
        // planner's backlog tray — holds still between rebuilds instead of
        // crawling out from under the pointer.
        assert_eq!(task.tie_break_jitter(4), task.tie_break_jitter(4));
        // ...and different in the next rebuild, which is the gentle shuffle.
        assert_ne!(task.tie_break_jitter(4), task.tie_break_jitter(5));
    }

    #[test]
    fn jitter_cannot_reorder_tasks_that_genuinely_differ() {
        let now = noon();
        // One importance step is a factor of two; the jitter is 8%. It should
        // never be able to flip a real difference in priority.
        let lower = dated(2, 0.0, now);
        let higher = dated(3, 0.0, now);
        for seed in 0..64 {
            assert!(higher.importance_score(now, seed) > lower.importance_score(now, seed));
        }
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

