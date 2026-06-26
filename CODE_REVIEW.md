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

_All items in this section are resolved — see the changelog._

---

## B. Performance & Power (high priority for an "always-on" calendar)

### B4. Per-page archive read is O(n) → O(n²) overall  *(deferred — archive redesign)*
> **Deferred:** this is archive-subsystem work, and the archive is slated for a redesign that
> dissolves it into the calendar (a "show completed tasks" overlay + upward scroll). That redesign
> would replace line-offset pagination with **date-ranged** reads, retiring this issue's framing
> rather than fixing it in place — so don't patch `read_lines_range` now.

`read_lines_range` (`tasks.rs`) re-opens and reverse-scans the whole `archived.jsonl`, skipping
`offset` lines on every "Show more". Fine for small archives, quadratic for large ones.
- **Sub-issue (new): pagination can mis-page on unparseable lines.** It `.skip(offset)` over *raw*
  lines, then parses with `filter_map(... .ok())`. The UI advances `offset` by the number of *raw*
  lines consumed, but renders only the *parsed* rows — so any unparseable archived line causes rows
  to be skipped or duplicated across pages.
- **Fix:** keep the `RevLines` iterator (or a byte offset) alive across pages, or read forward with a
  persisted cursor; count consumed lines consistently with what's displayed.


---

## C. Robustness & Data Integrity (medium)

_All items in this section are resolved — see the changelog._

---

## D. Architecture & Maintainability (medium)

> Note: the single-file `ui.rs` structure and the hand-tuned calendar magic numbers are deliberate —
> see [`DOCUMENTATION.md` §14](DOCUMENTATION.md). The items below are self-contained changes that do
> **not** require splitting the file or touching the animation/widget tuning.

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

- **E2. Duplicate cities** in `CITIES` (`weather.rs`): Mumbai/Delhi/Bangalore/Ahmedabad,
  Copenhagen/Aarhus/Aalborg/Odense, and several others appear 2–3×. Cosmetic, but clutters the map.
- **E6. Unused bindings / dead code:** several `device_id`, a `_map_response`, etc. — the bulk of the
  current compiler warnings (15 remain). Clean up (or `_`-prefix) to get back to a quiet build. (The
  `MouseWheel { unit, delta, modifiers }` destructure was cleared as part of B5.)
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
- **Calendar virtualization** keeps a very long calendar cheap to *render*; the model rebuild is now
  O(days + items) via date bucketing as well (B2, resolved).
- **Defensive config loading** with a line-by-line fallback when TOML parsing fails, plus clamping of
  every numeric field.
- **k-means palette generation** in Lab space with a deterministic seed is a genuinely nice feature.
- **Release profile** is thoughtfully tuned for size/speed.

---

## G. Suggested priority order (open items)

The app is a working, complete product; these are hardening steps, ordered by payoff-to-risk.

1. **D6** — modal `enum` to replace the parallel `*_flag` booleans.
2. **D7** — DPI-aware dialog layout (largely moot while fullscreen; lowest priority).
3. **E-series polish** (E2, E6, E8, E9), and extend the unit tests (see changelog E7) to the
   weather reshape.

_(B4 is deferred pending the archive redesign — see B4.)_

---

## Changelog — Resolved

Fixes already landed (newest first). Kept here as history so the open list above stays focused.

- **D2 / D4 / E1 — named calendar model, single coordinate field, real bool parse.**
  - **D2:** the opaque `calendar_elements` 6-tuple is now `Vec<DayCell>`, with
    `DayCell { day_number, preview: Vec<PreviewItem>, items: Vec<DayItem>, is_today, date, label }`.
    `show_calendar`, the day popup, and the "find today" lookup all use named fields instead of
    `day.0`/`.2`/`.4`. This is what the coming completed-tasks overlay will extend.
  - **D4:** `TaskApp`'s two loose `latitude`/`longitude` fields collapse into one
    `coordinates: [f32; 2]` (the type already used by config, the weather API, and `float_pair_array`).
    The weather service keeps its own copy by necessity (it runs on the worker thread); that's the only
    remaining duplicate and it's updated explicitly in `set_weather_coordinates`.
  - **E1:** `text_2_bool_lazy` (true for any string containing `t`) is replaced by `parse_config_bool`
    (case-insensitive `true`/`1`/`yes`/`on`, else `false`). Unit-tested.
- **C5 / C6 / C7 — path sanitization, restart error handling, DST-safe date entry.**
  - **C5:** the unsound `name.replace("..", "")` in `set_background` and `generate_colorscheme` is
    replaced by one shared `utilities::safe_image_path(name)`, which keeps only the final path
    component (`Path::file_name`) — defeating `..`, absolute paths, drive prefixes, and embedded
    separators — and returns `None` for names with no file component. Unit-tested.
  - **C6:** `restart_self` no longer panics. It's now `&mut self`, reports a failure to locate the exe
    or spawn the child via the error window, and `exit`s **only** on a successful spawn — so a failed
    restart leaves the running process intact instead of crashing or looping.
  - **C7:** `parse_time_input` resolves local times with `.earliest()` (so a fall-back **ambiguous**
    time picks the earlier instant instead of failing) and, for a spring-forward **gap**, nudges one
    hour forward to land just past it. The `Single` path is unchanged (covered by the existing test);
    the DST branches are tz-dependent so aren't unit-tested.
- **B5 — no per-frame clone of the input event vector.** `show_calendar`'s tap-vs-drag state machine
  used `ui.ctx().input(|i| i.events.clone())` every frame. It now inspects `&i.events` in place inside
  the `input(|i| …)` closure (the closure only mutates `self` / reads `visible_cells`, no re-entrant
  `ctx` calls). Behaviour is identical. Also dropped the unused `MouseWheel { unit, delta, modifiers }`
  destructure in the same arm to `{ .. }`, clearing 3 of the E6 warnings (build now at 15).
- **B2 — calendar rebuild is no longer O(days × items), and the week count is sanely clamped.**
  `summarize_calendar` now buckets events and deadline-tasks by day **once**
  (`tasks::bucket_by_deadline_day` → `HashMap<NaiveDate, Vec<&Active>>`, borrowing, no clones) and does
  an O(1) lookup per cell, so the whole build is O(days + items) instead of re-scanning every item for
  every day. Buckets preserve source order (events by deadline, tasks by score), so the "take 3"
  preview selection is unchanged; deadline-less items are skipped from the grid but still retained in
  `active_things` (A4 behaviour preserved). Separately, `CALENDAR_WEEKS_MAX` dropped from `20000`
  (~385 years, ~140k cells, a 20k-float `row_anim`) to `520` (~10 years) — a one-line change to the
  shared constant introduced in C3, so the startup loader and the live setter still agree. Unit test
  covers the bucketing (grouping + per-day order). Existing configs above the new cap clamp down on
  next load.
- **A9 — Settings no longer panics on an empty monitor list.** The startup-monitor row in
  `show_settings` used to index `monitor_options[selected_monitor_index]` directly, which panics when
  the list is empty (winit's `MonitorHandle::name()` can return `None` for every monitor, or a
  headless/remote setup reports none). The row now renders a disabled "No monitors detected"
  placeholder when the list is empty and otherwise resolves the selection with `position(...)` /
  `.get(...)` instead of raw indexing. (Found while doing C3.)
- **C3 — restart-only settings now apply live or are labelled.** `set_calendar_weeks` now updates
  `self.calendar_weeks_to_show` and re-runs `summarize_calendar` (+ `sync_calendar_caches`)
  immediately, so the new week count takes effect without a restart. It commits on Enter / focus-loss
  (not per keystroke, since each apply rebuilds the calendar), reflects the clamped value back into
  the field, and restores the field on invalid input. The clamp bounds are shared
  `initialization::CALENDAR_WEEKS_MIN/MAX` constants, so the live value and the startup-loaded value
  clamp identically (and B2 can change the range in one place). The **startup monitor** genuinely
  needs a restart (the window binds to a monitor at launch), so it's now labelled "(applies after
  restart)" with a hover hint on the ♲ button rather than silently saving.
- **C2 / C4 / D5 — unified, typed config writer with surfaced errors.** Both writers now use
  `toml_edit` and agree on value types. Startup persistence moved from `toml::to_string(&Config)`
  (which stripped comments and reordered keys) to `write_normalized_config`, which updates only the
  owned keys in-place and **preserves comments / ordering / unknown keys**; values are written with
  their real TOML types (integers, float arrays) instead of strings (**C2**). The runtime setters'
  duplicated read-parse-set-write blocks collapsed into one helper,
  `TaskApp::write_config_value(key, value)`, wrapped by `persist_config_value` (**D5**); the
  `toggle_*` and `update_background_config` methods were removed in favour of direct
  `persist_config_value` calls. Write failures (config setters **and** the notepad save) now route to
  the error window instead of `let _ = …` (**C4**). Coordinate/window pairs share
  `utilities::float_pair_array`. Tests: `write_normalized_config` round-trip (comment preserved, typed
  values) and `float_pair_array`. _Remaining (noted under the old C4): the error channel is still a
  single `error_text`/`error_flag`, so a second error can overwrite an unread first — a small queue
  would help now that more write errors surface._
- **C1 — stable item ids replace name-as-primary-key.** `Active`/`InActive` gained a `u64 id`
  (`#[serde(default)]`); delete/complete/lookup and the calendar day popup now key on `id` instead of
  `name` (`delete_active_thing(id)`, `complete_active_thing(id)`, `confirm_*: Option<u64>`, and the
  popup's `all_str` tuple carries the id). `name` is now purely cosmetic, so the creation-time
  `name_is_unique` gate was removed — duplicate names are allowed and a hand-edited duplicate no
  longer deletes/completes both. New ids come from `TaskApp::next_id` via `add_active_thing`; legacy/
  hand-edited saves (id `0`) are backfilled at startup by `tasks::assign_missing_ids` (preserves
  existing ids, seeds `next_id` past the max). Unit tests cover the backfill (mixed/empty). _Follow-up:
  a rename **UI** is now unblocked by the data model but not yet built._
- **A8 — startup deserialization no longer aborts the app.** `main.rs` now handles the `Err` from
  `read_at_startup` and `read_colorschemes` instead of `.unwrap()`-ing. A corrupt/unreadable file is
  quarantined by `tasks::quarantine_corrupt_file` (renamed to `<name>.corrupt-<timestamp>`, kept for
  manual recovery); boot continues from an empty active set / the default colour scheme. Recovery
  messages are threaded through `TaskAppConfig::startup_error` and shown in the existing error window
  once the UI is up (joined if both files failed). Unit tests cover the rename-aside path and the
  no-file-present path.
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
  `1e9` branch and event-distance ordering, with bounds that tolerate the random multiplier). Later
  work added tests for `quarantine_corrupt_file`, `assign_missing_ids` (C1), `write_normalized_config`
  + `float_pair_array` (C2), and `bucket_by_deadline_day` (B2 — the calendar bucketing, extracted into
  a pure helper so it's testable without constructing `TaskApp`). Run with `cargo test --lib`. Still
  uncovered: the weather reshape (a `TaskApp` method needing the struct constructed first).

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
