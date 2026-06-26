# TaskDeck — Code Review: Problems & Suggested Improvements

Companion to [`DOCUMENTATION.md`](DOCUMENTATION.md). This document tracks **problems worth fixing**
and concrete suggestions for each. It is meant to be a working to-do list for hardening the app, not
a description of how it works (that's the documentation's job).

**How to read this:**
- Findings are grouped by category and tagged with a rough priority. Each cites the relevant
  function/symbol rather than a line number, since line numbers drift as the code changes.
- **Intentional design choices are *not* listed here as problems.** Things that look like issues but
  are deliberate (the uncapped/forced-repaint render loop, the single-file `ui.rs`, the hand-tuned
  calendar magic numbers, the random tie-break shuffle in scoring) are documented in
  [`DOCUMENTATION.md` §14](DOCUMENTATION.md). Read that section before "fixing" anything in those
  areas.
- Resolved items are summarized in the changelog at the bottom rather than kept inline, so this list
  stays focused on open work.

---

## A. Correctness & Crash Risks (high priority)

### A8. Startup deserialization aborts the app before the window opens
`main.rs` loads the active set and colour schemes with `tasks::read_at_startup(&exe_file_path).unwrap()`
and `color::read_colorschemes(&exe_file_path).unwrap()`. If either file is corrupt or unreadable
(invalid JSON, partial write from outside the app, missing/return-Err data directory), the `unwrap`
panics during boot, before any window or error UI exists — the app simply fails to start with no
explanation. The notepad load already does the right thing (`read_notepad_text(...).unwrap_or(...)`),
so the handling is inconsistent.
- **Why it matters:** a single bad byte in `read_at_startup.json` or `colorschemes.json` makes the
  app un-launchable, and there's no in-app path to recover. This is the same data-integrity class as
  A4 (now fixed), but at the file-parse layer rather than the invariant layer.
- **Fix:** handle the `Err` instead of unwrapping — e.g. rename the bad file aside
  (`*.corrupt-<timestamp>`), start from an empty/default set, and surface the problem in the existing
  error window once the UI is up. Apply the same treatment to both files.

---

## B. Performance & Power (high priority for an "always-on" calendar)

### B2. `calendar_weeks_to_show` clamp allows absurd values
Clamped to `6..=20000` in `get_check_and_set_config` (`initialization.rs`). 20,000 weeks ≈ **385
years** = ~140,000 day cells. `summarize_calendar` allocates that many cells and, for **each day**,
linearly filters **all** events and tasks (`O(days × items)`); `row_anim` also grows to 20,000
floats. Rendering is virtualized, but the *model rebuild* on every add/delete/date-rollover is not,
so it becomes very slow at the top of the range.
- **Fix:** clamp to something sane (e.g. `6..=520`, ~10 years), and/or bucket items by date once
  (`HashMap<NaiveDate, Vec<…>>`) so each day's lookup is `O(1)`/`O(items)` instead of re-scanning
  every item per day.

### B4. Per-page archive read is O(n) → O(n²) overall
`read_lines_range` (`tasks.rs`) re-opens and reverse-scans the whole `archived.jsonl`, skipping
`offset` lines on every "Show more". Fine for small archives, quadratic for large ones.
- **Sub-issue (new): pagination can mis-page on unparseable lines.** It `.skip(offset)` over *raw*
  lines, then parses with `filter_map(... .ok())`. The UI advances `offset` by the number of *raw*
  lines consumed, but renders only the *parsed* rows — so any unparseable archived line causes rows
  to be skipped or duplicated across pages.
- **Fix:** keep the `RevLines` iterator (or a byte offset) alive across pages, or read forward with a
  persisted cursor; count consumed lines consistently with what's displayed.

### B5. Cloning all input events every calendar frame
`show_calendar` runs its press/drag state machine off `ui.ctx().input(|i| i.events.clone())`, cloning
the full event vector each frame.
- **Fix:** inspect events in place without cloning, or drive the tap-vs-drag decision from egui's
  `Response` drag/click APIs (the map picker already does this).

---

## C. Robustness & Data Integrity (medium)

### C1. Name is the de-facto primary key
Tasks/events are identified solely by `name`: uniqueness is enforced at creation (`name_is_unique`)
and delete/complete filter by name (`delete_active_thing`, `complete_active_thing`). Consequences:
you can't have two items called "Meeting", you can't rename one, and any duplicate that slips in (a
hand-edited data file bypasses the creation-time check) would delete/complete **both**.
- **Fix:** add a stable `id` (UUID or a monotonic counter) to `Active`/`InActive`; key all
  delete/complete/lookup operations on it; keep `name` purely cosmetic and freely editable.

### C2. Config has two writers that disagree on types
`get_check_and_set_config` rewrites the whole file via `toml::to_string(&Config)` on **every
startup**, which strips comments and reorders keys. The runtime `toml_edit` setters, meanwhile, write
some numbers as **strings** (e.g. `set_calendar_weeks` writes the input verbatim; tint and
colourscheme id likewise). It's self-healing only because `read_config` stringifies everything before
re-parsing — brittle and surprising.
- **Fix:** pick one mechanism. Prefer `toml_edit` throughout (preserves the file) and write typed
  values (`toml_edit::value(n as i64)`), or stop rewriting the file at startup unless it actually
  changed.

### C3. Settings that silently require a restart give no feedback
`set_calendar_weeks` writes the new week count to disk but never updates `self.calendar_weeks_to_show`
or re-runs `summarize_calendar`, so the change only appears after a restart, with no UI hint. The
monitor selection is similar (it applies via a hard process `restart_self`, see C6).
- **Fix:** apply live where feasible (re-run `summarize_calendar` after updating the field), or label
  the field "(applies after restart)".

### C4. Silent error swallowing on writes
Many writes discard their `Result` with `let _ = …`: the config setters (`set_calendar_weeks`,
`set_background_tint`, the toggles, …) and **the notepad save** (`save_textbox_text`, also the
exit-time `flush_pending_saves`). A failure (disk full, permissions, file locked) is invisible — for
the notepad this can silently lose the user's notes.
- **Related (new):** the error channel itself is a single `error_text` + `error_flag`, so even if
  these were surfaced, a second error would overwrite the first before it's seen. A small queue (or
  at least "don't overwrite an unread error") would help if write-errors start being surfaced.
- **Fix:** at minimum log; ideally route failures to the existing error window like the task-save
  path does.

### C5. Weak path-traversal sanitization
`name.replace("..", "")` in `set_background` (`ui.rs`) and `generate_colorscheme` (`color.rs`). This
is easy to bypass in principle (absolute paths, odd separators) and also mangles legitimate names
containing `..`. Risk is low because the names come from a directory listing, but the approach is
unsound.
- **Fix:** take only the file-name component, or canonicalize the resolved path and verify it stays
  within `images/`.

### C6. `restart_self` can spin-loop and panics on failure
`restart_self` spawns a fresh process and `exit(0)`s (used by the monitor "♲" button). If startup
fails repeatedly the user could get a respawn loop, and the spawn itself is `.expect("Failed to
restart!")` — a failed spawn panics rather than reporting.
- **Fix:** apply the monitor change without a full restart if the platform allows; otherwise guard
  against repeated immediate restarts and handle the spawn error gracefully.

### C7. Date entry fails on DST-gap / ambiguous local times
`parse_time_input` resolves the local time with `Local.from_local_datetime(&naive).single()`, which
returns `None` for a non-existent local time (spring-forward gap) or an ambiguous one (fall-back
overlap). The result is a generic "Problem with date" for a date that looks perfectly valid to the
user.
- **Fix:** fall back to `.earliest()` / `.latest()` instead of `.single()`, or give a clearer message
  explaining the DST gap. (Rare, low priority, but the failure is confusing.)

---

## D. Architecture & Maintainability (medium)

> Note: the single-file `ui.rs` structure and the hand-tuned calendar magic numbers are deliberate —
> see [`DOCUMENTATION.md` §14](DOCUMENTATION.md). The items below are self-contained changes that do
> **not** require splitting the file or touching the animation/widget tuning.

### D2. `calendar_elements` tuple is opaque
`Vec<(u8, Vec<(String,String,usize)>, Vec<(String,String,bool)>, bool, NaiveDate, String)>` — a
six-element tuple accessed positionally (`day.0`, `day.2`, `day.4`) all over `show_calendar` and the
day popup.
- **Fix:** introduce named structs (`DayCell`, `CalendarItem`) for readability and to prevent
  positional mix-ups. Self-contained; stays inside `ui.rs`.

### D4. Duplicated coordinate state
`latitude` / `longitude` (live, editable) duplicate `coordinates` from config and the values held by
the weather service. Three copies must be kept in sync by hand.
- **Fix:** a single source of truth for the current coordinates.

### D5. Duplicated TOML-setter boilerplate
`toggle_fullscreen_option`, `toggle_fps_option`, `toggle_num_weather_days`, `set_calendar_weeks`,
`set_background_tint`, … are near-identical read-parse-set-write blocks.
- **Fix:** one generic `set_config_value(key, value)` helper. (Pairs naturally with C2/C4 — fix the
  typing and error-handling once, in the shared helper.)

### D6. Many parallel `*_flag` booleans must be kept consistent by hand
`TaskApp` tracks modal state as a dozen independent booleans (`new_task_flag`, `settings_flag`,
`display_archive_flag`, …) plus the big disjunction that clears `hovered_calendar_cell` when "any
modal is open". Nothing structurally prevents two modals being open at once.
- **Fix:** a single `enum Modal { None, NewTask, NewEvent, Settings, Archive, DayPopup(usize), … }`
  makes invalid combinations unrepresentable. This is a self-contained change *inside* `ui.rs` (it is
  **not** a reason to split the file — see §14.2).

### D7. Static side-panel/dialog spacers assume a fixed DPI / window size
The side panels and dialogs are laid out with absolute `add_space` spacers, which won't adapt to
non-100% DPI scaling or arbitrary window sizes. Largely moot while the app runs fullscreen on a
chosen monitor.
- **Fix:** only worth revisiting if multi-DPI or freely-resizable use becomes a goal — and even then,
  leave the calendar **animation/widget** magic numbers alone (§14.3); this is about the static
  dialog layout only.

---

## E. Smaller issues & polish (low)

- **E1. `text_2_bool_lazy`** (`initialization.rs`) returns `true` for any string containing `t`. It
  happens to work for `"true"`/`"false"`, but `"east"`, `"set"`, etc. would also be `true`. Use a
  real bool parse with a sensible default.
- **E2. Duplicate cities** in `CITIES` (`weather.rs`): Mumbai/Delhi/Bangalore/Ahmedabad,
  Copenhagen/Aarhus/Aalborg/Odense, and several others appear 2–3×. Cosmetic, but clutters the map.
- **E6. Unused bindings / dead code:** several `device_id`, a `_map_response`, an unused
  `MouseWheel { unit, delta, modifiers }` destructure, etc. — these are the bulk of the current
  compiler warnings. Clean up (or `_`-prefix) to get back to a quiet build.
- **E8. Windows-only assumptions** (`winit::platform::windows`, `with_taskbar_icon`,
  `windows_subsystem`) aren't feature-gated; the crate won't compile on other platforms despite
  mostly-portable logic. Gate the Windows-specific calls behind `#[cfg(windows)]` if cross-platform
  builds ever matter.
- **E9. (new) `importance_score` saturates to `+inf` at the extreme top end.** For a high-importance
  deadline far in the future, the exponential curve (`1.2^…`, `1.17^…`) overflows `f32` to `+inf`, so
  all such tasks compare equal — a much milder echo of the old `as u16` flattening (B3), now at the
  `inf` end. Only reachable with extreme inputs and bounded further once B2 caps the week count, so
  very low priority. If it ever matters: clamp the exponent, use `f64`, or a saturating-but-finite
  curve.

---

## F. What the code already does well

Worth preserving — don't regress these while hardening:

- **Atomic file writes** for the critical JSON files (temp file → fsync → persist) — good durability
  against partial writes.
- **Weather threading** is clean: `RwLock` for data + `AtomicU64` version flag + a command channel
  with graceful `Drop`/`Stop`, plus an `EventLoopProxy` to wake the UI. The UI only re-shapes the
  data when the version actually changes (`last_weather_version`), so it's not re-cloning every
  frame. Backoff-with-retries is a nice touch.
- **Calendar virtualization** keeps a very long calendar cheap to *render* (the rebuild cost is the
  open concern — see B2).
- **Defensive config loading** with a line-by-line fallback when TOML parsing fails, plus clamping of
  every numeric field.
- **k-means palette generation** in Lab space with a deterministic seed is a genuinely nice feature.
- **Release profile** is thoughtfully tuned for size/speed.

---

## G. Suggested priority order (open items)

The app is a working, complete product; these are hardening steps, ordered by payoff-to-risk.

1. **A8** — stop a corrupt data file from making the app un-launchable (quarantine + recover).
2. **C1** — stable item IDs, so duplicate/rename/delete behave correctly.
3. **B2** — sane `calendar_weeks_to_show` clamp and/or date-bucketed lookups (fixes the rebuild
   blow-up at the top of the range).
4. **C2 / C4 / D5** — unify the config writer (typed `toml_edit`), surface write errors, and collapse
   the duplicated setter boilerplate into one helper. These three are best done together.
5. **C3** — apply restart-only settings live, or label them.
6. **B4 / B5** — archive pagination cursor (and the mis-page sub-issue), and the per-frame event
   clone.
7. **C5 / C6 / C7** — path sanitization, restart-loop/spawn-error handling, DST date entry.
8. **D2 / D4 / D6 / D7** — named calendar structs, single coordinate source of truth, modal `enum`,
   DPI-aware dialog layout.
9. **E-series polish** (E1, E2, E6, E8, E9), and extend the unit tests (see changelog E7) to the
   calendar bucketing and weather reshape.

---

## Changelog — Resolved

Fixes already landed (newest first). Kept here as history so the open list above stays focused.

- **A1 — notepad save-on-exit.** `App::exiting` now calls `TaskApp::flush_pending_saves()` on
  event-loop shutdown (covers the Quit button and the window X; not a hard kill / panic-abort).
- **A2 — frame-count autosave replaced with a wall-clock debounce.** Persists ~2 s after the last
  edit via `last_textbox_edit_time: Option<Instant>`, independent of frame rate.
- **A3 — weather reshape panic on short/partial responses.** `fix_and_cache_weather_data` now
  validates inner (per-day) lengths (`any(|hour| hour.len() < 3)`) as well as the outer 24, tripping
  `weather_is_broken_flag` instead of indexing out of bounds.
- **A4 — `summarize_calendar` deadline unwraps.** All event deadline access is now `Option`-safe
  (sort on `e.deadline`, filter with `map_or`, format with `.map(…).unwrap_or_default()`). A
  deadline-less event is quarantined (never placed) but retained in `active_things`. (Deliberately
  not surfaced via the error window — `summarize_calendar` re-runs on every edit and would re-pop the
  modal.)
- **A5 — background-texture `unwrap`.** The background draw is wrapped in `if let Some(texture)` and
  skipped when absent.
- **A6 — notepad hidden while weather is broken.** `show_weather_forecast` was restructured so the
  "WEATHER IS BROKEN" notice only replaces the forecast grids; the notepad now renders unconditionally
  when 3-day weather is off, reachable at startup and while the network is down.
- **A7 — `expanded_day` index outliving its calendar.** The day popup reads via
  `self.calendar_elements.get(index)` and dismisses itself (`else` branch clears the flags) instead of
  panicking on a stale index.
- **B3 — priority sort `as u16` saturation.** Each task's score is evaluated once into a
  `(f32, Active)` pair (preserving the intentional shuffle — §14.4 — and giving a consistent
  comparator) and sorted on the `f32` with `partial_cmp` (NaN → `Equal`). No `u16` cast.
- **E5 — day picker `1..=31`.** The day `ComboBox` now ranges over `utilities::days_in_month(year,
  month)` and clamps `day_input` into range each frame, so "Feb 31" can't be entered.
- **E7 — no tests (partial).** Added `#[cfg(test)]` unit tests for `ordinal_suffix`, `days_in_month`,
  `parse_time_input` (valid + impossible dates), `calendar_item_color`, and `importance_score` (the
  `1e9` branch and event-distance ordering, with bounds that tolerate the random multiplier). Run with
  `cargo test --lib`. Still uncovered: calendar bucketing and the weather reshape (both `TaskApp`
  methods needing the struct constructed first).

### Reclassified as intentional (moved to `DOCUMENTATION.md` §14)

Previously listed here as "won't fix / by design"; now documented as deliberate design decisions so
they aren't re-raised as problems:

- Uncapped, forced-repaint render loop + `AutoNoVsync` / `predictable_texture_filtering` (was B1, E4)
  → §14.1.
- Single-file `ui.rs` / large `TaskApp` (was D1) → §14.2. (The modal-`enum` refinement survives as
  the open item **D6**.)
- Hand-tuned calendar/widget magic numbers (was D3) → §14.3. (The static-dialog DPI note survives as
  the open item **D7**.)
- Random tie-break shuffle in `importance_score` (was E3) → §14.4.
