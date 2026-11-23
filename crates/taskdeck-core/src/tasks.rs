//! Scoring and the item and rule queries, copied from today's `src/tasks.rs`
//! (core.md §14.2, §20). Presentation only: the score is `f32`, ported
//! operation by operation, and nothing here is stored or hashed (core.md
//! §14.1).

use chrono::Datelike;

use crate::board::{Item, Kind, Recurrence};
use crate::time::{self, Instant};

/// Short weekday names, Monday first — the order `chrono`'s
/// `num_days_from_monday` counts in, so a `Recurrence` bit and a name share an
/// index.
pub const WEEKDAY_NAMES: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// Every weekday set.
pub const EVERY_DAY: u8 = 0b0111_1111;

/// Weekdays only — Monday to Friday.
pub const WEEKDAYS: u8 = 0b0001_1111;

/// Labels for a dated task's **severity** — how bad missing the date is —
/// indexed by `Item::importance`.
pub const IMPORTANCE_LABELS: [&str; 5] =
    ["Not important", "Mildly important", "Important", "Highly important", "Lethally important"];

/// Labels for an undated task's **horizon** — roughly how soon it should
/// happen — indexed by `Item::time_importance`. The index order is a
/// serialization fact (the first three are load-compatible with the old
/// urgency scale, "whenever" is appended); `HORIZON_DISPLAY_ORDER` is how a
/// list presents them, soonest first.
pub const HORIZON_LABELS: [&str; 4] = ["Within a month", "Within a week", "Within days", "Whenever"];

/// Presentation order for `HORIZON_LABELS`: soonest first, the parking lot last.
pub const HORIZON_DISPLAY_ORDER: [u8; 4] = [2, 1, 0, 3];

/// Palette entry a routine wears: the calmest one.
///
/// A routine is the backdrop a day is planned *around*, not something competing
/// for attention in it, and it never reaches the calendar grid — the planner's
/// timeline is the only place its colour is ever seen.
pub const ROUTINE_COLOR_INDEX: usize = 0;

/// Number of entries in a colour scheme, and therefore the range every palette
/// index has to stay inside.
pub const PALETTE_LEN: usize = 6;

/// Index of the "whenever" horizon: the appended parking-lot tier.
pub const HORIZON_WHENEVER: u8 = 3;

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
 * The tables below are the whole policy.
 */

/// How much each importance level counts for, indexed by `Item::importance`.
/// Doubling per level means one step of importance is worth exactly one
/// doubling of time pressure, which is what makes the two comparable.
pub const WEIGHT_BY_IMPORTANCE: [f32; 5] = [1.0, 2.0, 4.0, 8.0, 16.0];

/// How far ahead of its deadline each importance level starts to feel urgent:
/// the task is at half pressure this far out, and full pressure at the deadline.
/// More important work is noticed earlier, which is the real difference between
/// "lethally important" and "not important" — both are equally due on the day.
///
/// These also bound how long importance out-argues urgency. A task out-ranks a
/// trivial one sitting at *its* deadline for `lead × log2(weight)` days —
/// about a month for the top level.
pub const LEAD_DAYS_BY_IMPORTANCE: [f32; 5] = [0.5, 1.0, 2.0, 4.0, 8.0];

/* ── The horizon: an undated task's whole input ──────────────────────────
 *
 * A dated task asks two questions: when is it due (the deadline), and how bad
 * is missing that (the 5-level importance — severity). An undated task asks
 * only one: *roughly how soon should this happen?* That answer is the
 * horizon, and it drives both how fast the task ripens and how much it weighs
 * once ripe.
 *
 * The horizon is stored in `Item::time_importance`, and the first three
 * indices are **load-compatible** with the old 3-level "urgency" scale (same
 * weights: 1, 2, 4). That is why the arrays are ordered month → week → days
 * with "whenever" appended at index 3, even though the UI presents them
 * soonest-first.
 */

/// How much a ripened task of each horizon counts for, indexed by
/// `Item::time_importance`: within a month / a week / days / whenever.
///
/// "Whenever" weighs half of the lowest dated tier — a parking-lot idea should
/// eventually surface, but never by out-arguing anything with a real claim.
pub const WEIGHT_BY_HORIZON: [f32; 4] = [1.0, 2.0, 4.0, 0.5];

/// How long a task of each horizon takes to ripen: it reaches half its weight
/// this long after being created, and approaches full weight from there.
pub const RIPEN_DAYS_BY_HORIZON: [f32; 4] = [30.0, 10.0, 3.0, 90.0];

/// How far ahead of a *planned* block a task starts to feel pressing. Short by
/// design: a plan says "do it at this time", so it should climb into view over
/// the hours before its block rather than days ahead the way a deadline does.
pub const PLANNED_LEAD_DAYS: f32 = 0.5;

/// Once a deadline is missed, pressure keeps doubling this often. It is the
/// same for every importance level: being late is late, and the weight already
/// says how much this particular lateness matters.
pub const OVERDUE_DOUBLING_DAYS: f32 = 1.0;

/// Ceiling on overdue pressure, reached two days late.
///
/// Without it a task forgotten for a year would out-score everything else by
/// astronomical margins and the list below it would be meaningless. Being
/// maximally late is worth two steps of importance, no more.
pub const OVERDUE_PRESSURE_CAP: f32 = 4.0;

/// Importance assumed for a dated task that has none. Not reachable from the
/// UI, but a legacy save can produce it, and treating it as the middle of the
/// scale is far better than treating it as broken.
pub const ASSUMED_IMPORTANCE: u8 = 2;

/// Score for an item with nothing to go on: no deadline, no importance, no
/// urgency. Deliberately far above any real score (the maximum is
/// `16 × 4 = 64`) so a corrupt entry surfaces at the top of the list where it
/// will be noticed and fixed, rather than hiding at the bottom.
pub const MALFORMED_SCORE: f32 = 1.0e6;

/// Size of the tie-break jitter: each score is multiplied by a factor in
/// `[1.0, 1.0 + JITTER)`. See `Item::tie_break_jitter`.
pub const JITTER: f32 = 0.08;

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

/// `to − from` as fractional days.
///
/// Minutes rather than whole hours, which truncated: a task due in 90 minutes
/// and one due in 110 scored identically for the whole hour between. A
/// difference of two instants, so no zone.
fn duration_in_days(from: Instant, to: Instant) -> f32 {
    time::between(from, to).num_minutes() as f32 / (24.0 * 60.0)
}

/// The bit `day`'s weekday occupies in a `Recurrence::days` mask.
pub fn weekday_bit(day: chrono::NaiveDate) -> u8 {
    1 << day.weekday().num_days_from_monday()
}

/// Which of the six palette entries an item wears. **Always** in
/// `0..PALETTE_LEN`.
///
/// A free function rather than a method because an archive row wants the same
/// answer (`ArchiveRow::color_id`).
///
/// The clamp is the load-bearing part: `importance` is a `u8` from outside,
/// and the calendar indexes the six-entry palette with this directly.
pub fn calendar_item_color(is_event: bool, importance: Option<u8>, time_importance: Option<u8>) -> usize {
    let index = if is_event {
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
    };
    index.min(PALETTE_LEN - 1)
}

impl Recurrence {
    /// Whether the rule fires on `day`.
    pub fn falls_on(&self, day: chrono::NaiveDate) -> bool {
        if !self.repeats() {
            return day == self.anchor;
        }
        self.days & weekday_bit(day) != 0
    }

    /// Whether this comes round again, or is just the one day.
    pub fn repeats(&self) -> bool {
        self.days != 0
    }

    /// Whether the weekday at `index` (0 = Monday) is included.
    pub fn includes(&self, index: u32) -> bool {
        self.days & (1 << index) != 0
    }

    /// "just this day" / "every day" / "weekdays" / "Mon · Wed · Fri".
    pub fn summary(&self) -> String {
        match self.days {
            0 => "just this day".to_string(),
            EVERY_DAY => "every day".to_string(),
            WEEKDAYS => "weekdays".to_string(),
            0b0110_0000 => "weekends".to_string(),
            _ => WEEKDAY_NAMES
                .iter()
                .enumerate()
                .filter(|(index, _)| self.includes(*index as u32))
                .map(|(_, name)| *name)
                .collect::<Vec<_>>()
                .join(" · "),
        }
    }

    /// The same, for somewhere that has no "this day" to refer to — the archive
    /// ledger, which names the date instead (chrono's `%-d %b %Y`).
    pub fn summary_dated(&self) -> String {
        if self.repeats() {
            self.summary()
        } else {
            let a = self.anchor;
            format!("just {} {} {}", a.day(), time::month_abbrev(a.month()), a.year())
        }
    }
}

impl Item {
    /// Priority of this task right now. Higher sorts first.
    ///
    /// See the notes above for the model. In short: `weight × pressure`, where
    /// a dated task's pressure doubles as its deadline approaches and an
    /// undated task's ripens with age. `seed` is the local day
    /// (`read::shuffle_seed`) — see `tie_break_jitter`.
    pub fn importance_score(&self, now: Instant, seed: u64) -> f32 {
        self.base_score(now) * self.tie_break_jitter(seed)
    }

    /// The score without the tie-break jitter — the part that is a pure
    /// function of the task and the time, and so the part worth testing.
    pub(crate) fn base_score(&self, time_now: Instant) -> f32 {
        // A routine is not ranked, because it is not owed. The shape a routine
        // has (no deadline, no importance, no horizon) is exactly the one the
        // scorer treats as corrupt and shoves to the top of the list, and a
        // guard is cheaper than trusting two filters to never let one through.
        if self.is_routine() {
            return 0.0;
        }

        // Pressure from planned blocks, if the task has any. The *most
        // pressing* block decides: with work booked for this afternoon and
        // more for Thursday, this afternoon is what matters now. Capped at
        // 1.0: a block that has come and gone is a plan you didn't keep,
        // which is not the same thing as a missed deadline and shouldn't
        // escalate like one.
        let planned_pressure = self
            .blocks
            .iter()
            .map(|block| deadline_pressure(duration_in_days(time_now, block.start), PLANNED_LEAD_DAYS).min(1.0))
            .fold(None, |best: Option<f32>, pressure| Some(best.map_or(pressure, |b| b.max(pressure))));

        // A deadline is the strongest thing a task can tell us, so it decides
        // the model whenever there is one. An importance is used if set, and
        // assumed otherwise; a `time_importance` alongside a deadline is
        // ignored rather than given its own precedence puzzle.
        if let Some(deadline) = self.deadline {
            let importance = self.importance.unwrap_or(ASSUMED_IMPORTANCE);
            let days_left = duration_in_days(time_now, deadline);
            let from_deadline = deadline_pressure(days_left, weight_for(&LEAD_DAYS_BY_IMPORTANCE, importance));
            // Whichever reason is more pressing wins. A report due Friday that
            // you set aside Tuesday morning for should rise on Tuesday morning:
            // that is when you decided to do it.
            let pressure = from_deadline.max(planned_pressure.unwrap_or(0.0));
            return weight_for(&WEIGHT_BY_IMPORTANCE, importance) * pressure;
        }

        let age_days = duration_in_days(self.created, time_now);
        let weight = match (self.importance, self.time_importance) {
            (Some(importance), _) => weight_for(&WEIGHT_BY_IMPORTANCE, importance),
            (None, Some(horizon)) => weight_for(&WEIGHT_BY_HORIZON, horizon),
            (None, None) if !self.blocks.is_empty() => weight_for(&WEIGHT_BY_IMPORTANCE, ASSUMED_IMPORTANCE),
            (None, None) => return MALFORMED_SCORE,
        };

        // Undated but planned: the blocks are the only timing the task has,
        // so they drive the pressure. Without this a task blocked out for this
        // afternoon scored as if it were brand new — bottom of the list, on the
        // very day time was set aside for it.
        if let Some(pressure) = planned_pressure {
            return weight * pressure;
        }

        // Undated and unplanned: ripen with age, over the horizon the user
        // gave it, or the middle rate for a task that only carries an
        // importance.
        let ripen_days = match self.time_importance {
            Some(horizon) => weight_for(&RIPEN_DAYS_BY_HORIZON, horizon),
            None => RIPEN_DAYS_BY_HORIZON[1],
        };
        weight * ripeness(age_days, ripen_days)
    }

    /// A small per-task multiplier in `[1.0, 1.0 + JITTER)`, constant for a
    /// given `seed`.
    ///
    /// The seed is **the day**, deliberately neither the clock nor a rebuild
    /// counter. Keyed on the time, any list that re-sorts every frame
    /// reshuffled once a second and cards crawled out from under the pointer.
    /// Keyed on a rebuild counter it moved on every edit instead. The jitter
    /// exists so that a task which is perpetually fourth is not permanently
    /// ignored, and that argument has a period of about a day.
    ///
    /// It is deliberately small: enough to keep near-ties moving, never enough
    /// to reorder tasks that genuinely differ in priority.
    pub fn tie_break_jitter(&self, seed: u64) -> f32 {
        // splitmix64, so the id and the seed are properly mixed rather than
        // merely added — adjacent ids must not produce adjacent factors.
        let mut mixed = self.id.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ seed.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        mixed ^= mixed >> 30;
        mixed = mixed.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        mixed ^= mixed >> 27;
        mixed = mixed.wrapping_mul(0x94D0_49BB_1331_11EB);
        mixed ^= mixed >> 31;

        // Top 24 bits, scaled into [0, 1).
        let unit = (mixed >> 40) as f32 / (1u64 << 24) as f32;
        1.0 + unit * JITTER
    }

    /// True when this is a standing commitment rather than a task or an
    /// event: its `kind` is `Routine` (core.md §6.2).
    pub fn is_routine(&self) -> bool {
        self.kind == Kind::Routine
    }

    /// The item's palette index.
    pub fn calendar_item_color(&self) -> usize {
        if self.is_routine() {
            return ROUTINE_COLOR_INDEX;
        }
        calendar_item_color(self.kind == Kind::Event, self.importance, self.time_importance)
    }

    /// The instant this item occupies on the planner's timeline, if any: an
    /// event sits at its `deadline`, a task at the earliest time set aside for
    /// it (falling back to its deadline). `None` for an unplanned,
    /// deadline-less task — those live in the planner's tray instead.
    pub fn planner_anchor(&self) -> Option<Instant> {
        if self.is_routine() {
            // A rule has no single instant — it has one per week, forever.
            return None;
        }
        if self.kind == Kind::Event {
            self.deadline
        } else {
            self.blocks.iter().map(|b| b.start).min().or(self.deadline)
        }
    }

    /// True when the item has been given time on the planner, as opposed to
    /// merely having a due date. Events count as planned once they have a
    /// length; a task counts once it has at least one block.
    pub fn is_planned(&self) -> bool {
        if self.is_routine() {
            // A routine is nothing but plan.
            return true;
        }
        if self.kind == Kind::Event {
            self.deadline.is_some() && self.duration_minutes.is_some()
        } else {
            !self.blocks.is_empty()
        }
    }

    /// Total minutes of this task's planned blocks, across all days.
    pub fn planned_minutes(&self) -> u32 {
        self.blocks.iter().map(|b| b.minutes).sum()
    }

    /// Minutes of the estimate not yet covered by blocks, if the task has an
    /// estimate. `Some(0)` is meaningful: fully planned.
    pub fn remaining_minutes(&self) -> Option<u32> {
        self.duration_minutes.map(|estimate| estimate.saturating_sub(self.planned_minutes()))
    }

    /// True while the task still belongs in the planner's tray: it has no
    /// blocks at all, or its estimate isn't yet covered by the blocks it
    /// has. A 2-hour task with one hour booked is still half a card.
    pub fn wants_planning(&self) -> bool {
        // Neither an event nor a routine is waiting for a slot: an event's time
        // is its deadline, and a routine's is its rule.
        if self.kind == Kind::Event || self.is_routine() {
            return false;
        }
        self.blocks.is_empty() || self.remaining_minutes().is_some_and(|left| left > 0)
    }
}

#[cfg(test)]
mod tests;
