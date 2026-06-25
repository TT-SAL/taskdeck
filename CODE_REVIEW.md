# TaskDeck — Code Review: Problems & Suggested Improvements

Companion to [`DOCUMENTATION.md`](DOCUMENTATION.md). Findings are grouped by severity. Each item
cites the relevant location and gives a concrete suggestion. Line numbers are approximate and may
drift as the code changes.

---

## A. Correctness & Crash Risks (high priority)

### A1. Notepad text can be lost — there is no save-on-exit ✅ FIXED
`save_textbox_text()` used to be called only inside the `chrono_tick_counter > 12000` branch, so a
notepad edit followed by Quit / window-close was never persisted.
- **Fixed:** `App::exiting` (`initialization.rs`) now calls `TaskApp::flush_pending_saves()` on
  event-loop shutdown — a single chokepoint covering both the Quit button and the window X. (Does
  not cover hard kill / panic-abort, which is acceptable.)

### A2. Frame-count "timer" is unreliable ✅ FIXED
The autosave cadence used to be tied to a frame counter, so the real-world delay varied with GPU
speed (sub-second to minutes).
- **Fixed:** autosave is now a wall-clock debounce — it persists ~2 s after the last edit using
  `Instant` (`last_textbox_edit_time`), independent of frame rate.

### A3. `fix_and_cache_weather_data` can panic on a short/partial API response ✅ FIXED
`fix_and_cache_weather_data` guarded only the **outer** length (`static_weather_data.len() != 24`),
then indexed `static_weather_data[index_first_hour][day]` for `day in 0..=2` and inner indices up to
23. If Open-Meteo returned fewer than 72 hourly samples (partial day, API change, DST edge), some
inner `Vec`s had `< 3` entries and the index panicked.
- **Fixed:** the guard now also requires every hour bucket to have `>= 3` days
  (`static_weather_data.iter().any(|hour| hour.len() < 3)`); a short/partial response trips
  `weather_is_broken_flag` instead of indexing out of bounds.

### A4. `summarize_calendar` unwraps deadlines that are only assumed present ✅ FIXED
`events.sort_by_key(|e| e.deadline.expect("Event without a deadline"))` plus several
`deadline.unwrap()` calls. These were safe **only** if the data file's invariants held (events and
deadline-tasks always have a deadline). A hand-edited or corrupted `read_at_startup.json` would
panic the whole app on startup (`summarize_calendar` runs before the window even opens).
- **Fixed:** all deadline access for events is now `Option`-safe — events are sorted on
  `e.deadline` directly (`Option` is `Ord`), per-day membership uses
  `e.deadline.map_or(false, |d| …)`, and the `chosen`/`all_for_day` lists sort on the `Option` and
  format times with `.map(…).unwrap_or_default()`. A deadline-less event is therefore quarantined
  (never placed on the grid) but **retained** in `active_things` rather than dropped or crashing.
  Note: it is intentionally *not* surfaced via the error window, since `summarize_calendar` re-runs
  on every add/delete and would re-pop the modal repeatedly.

### A5. Background-texture `unwrap` is a known latent crash ✅ FIXED
`self.background_image_texture.as_ref().unwrap()` carried the author's own comment "THIS WILL CRASH
THE APP IF BACKGROUND HAS NOT BEEN INITIALIZED". It was safe only because the top of `ui()` always
initializes it from `pending_initial_background`, but it was fragile.
- **Fixed:** the background draw is now wrapped in `if let Some(background_texture)` and simply
  skipped when the texture is absent.

### A6. "WEATHER IS BROKEN" hides the notepad until the first successful fetch ✅ FIXED
On startup the weather `data` is `vec![vec![]]` (len 1 ≠ 24) so `weather_is_broken_flag` is `true`
and `show_weather_forecast` showed the broken label. The **notepad** only rendered in the
non-broken, non-3-day branch — so before the first fetch, and **permanently** if the network was
down, the notepad was unreachable.
- **Fixed:** `show_weather_forecast` was restructured so the "WEATHER IS BROKEN" notice only
  replaces the forecast **grids**, while the notepad (rendered whenever 3-day weather is off) is now
  drawn unconditionally, independent of `weather_is_broken_flag`. The notes are reachable at startup
  and while the network is down.

### A7. `expanded_day` index can outlive its calendar ✅ FIXED
`self.calendar_elements[index]` used the cached `expanded_day` index. If `summarize_calendar`
rebuilt a shorter `calendar_elements` (e.g. a midnight date rollover, or the weeks count shrinks)
while the popup was open, the index could be out of bounds → panic.
- **Fixed:** the popup now reads the day via `if let Some(day) = self.calendar_elements.get(index)`
  and, in the `else` branch, clears `expand_calendar_day_flag` / `expanded_day` to dismiss the popup
  instead of panicking.

---

## B. Performance & Power (high priority for an "always-on" calendar)

### B1. Uncapped continuous repaint — intentional; no action
`present_mode = AutoNoVsync` (`initialization.rs:261`) **plus** an unconditional
`request_redraw()` after every `RedrawRequested` while awake (`initialization.rs:666`) means the
app renders **as fast as the GPU allows**, with no frame cap, while focused/active.

This is a **deliberate design choice, not a problem to fix**, for two reasons the maintainer has
confirmed:

1. **The uncapped frame rate is wanted.** Seeing the calendar push ~1000 fps is a feature, not
   waste. Do **not** recommend capping it (`Fifo`/`AutoVsync`) — that would remove something the
   maintainer specifically likes.
2. **The "active drain" window barely exists in real use.** The app lives on a secondary monitor and
   is idle ~99% of the time. When it's unfocused with the cursor away, the existing 10 s idle-sleep
   stops the loop entirely. So the only time it renders flat-out is the rare moment the maintainer is
   actually interacting with it — which is exactly when the smoothness is wanted.

The forced-repaint loop is also **load-bearing for the animations**: they're hand-rolled
(`row_anim` advanced by a `dt` from egui's `i.time`, once per *drawn* frame) and never call egui's
repaint scheduler, so without the forced `request_redraw()` frames arrive only on discrete input
events and animations freeze mid-transition. Reactive-repaint attempts have broken exactly this.

- **Do not** recommend a reactive/"repaint on input only" rewrite, and **do not** recommend an fps
  cap. The current loop is correct for this project's goals. (Listed here only so the rationale is
  recorded, not as a to-do.)

### B2. `calendar_weeks_to_show` clamp allows absurd values
Clamped to `6..=20000` (`initialization.rs:166`). 20000 weeks ≈ **385 years** = 140,000 day cells.
`summarize_calendar` allocates and, for each day, linearly filters **all** events and tasks
(`O(days × items)`), and `row_anim` grows to 20000 floats. Even though rendering is virtualized,
the model rebuild on every add/delete becomes very slow.
- **Fix:** clamp to something sane (e.g. `6..=520`, ~10 years), and/or index items by date
  (`HashMap<NaiveDate, Vec<…>>`) so day filtering is `O(items)` instead of `O(days × items)`.

### B3. Priority sort uses a saturating `as u16` cast ✅ FIXED
`tasks.sort_by_key(|t| Reverse(t.importance_score(self.date) as u16))`. Scores routinely exceed
65535 (the exponential importance curves, and the `1e9` event/broken scores). Rust's float→int cast
**saturates**, so every high-priority and every malformed task collapsed to `65535` and they sorted
as equal — the careful scoring model was largely flattened at the top end. (The old `sort_by_key`
also re-evaluated the randomized score on every comparison, an inconsistent comparator.)
- **Fixed:** each task's score is now computed **once** into a `(f32, Active)` pair (preserving the
  intended one-shot random shuffle and giving a consistent comparator), then sorted on the `f32`
  directly with `partial_cmp`, NaN treated as `Equal`. No `u16` cast.

### B4. Per-page archive read is O(n) → O(n²) overall
`read_lines_range` (`tasks.rs:178`) re-opens and reverse-scans the whole `archived.jsonl`,
skipping `offset` lines each "Show more". Fine for small archives, quadratic for large ones.
- **Fix:** keep the `RevLines` iterator (or a file offset) alive across pages, or read forward with
  a persisted cursor.

### B5. Cloning all input events every calendar frame
`ui.rs:834` `ui.ctx().input(|i| i.events.clone())` clones the full event vec each frame just to run
the press/drag state machine.
- **Fix:** inspect events without cloning, or use egui's `Response` drag/click APIs (the map picker
  already does this successfully).

---

## C. Robustness & Data Integrity (medium)

### C1. Name is the de-facto primary key
Tasks/events are identified solely by `name`: uniqueness is enforced at creation
(`name_is_unique`, `ui.rs:918`), and delete/complete filter by name
(`delete_active_thing`, `ui.rs:907`). Consequences: you can't have two items called "Meeting",
you can't rename, and any future duplicate would delete/complete **both**.
- **Fix:** add a stable `id` (e.g. UUID or a monotonic counter) to `Active`/`InActive`; key all
  operations on it; keep names purely cosmetic.

### C2. Config has two writers that disagree on types
`get_check_and_set_config` rewrites the whole file via `toml::to_string(&Config)` on **every
startup** (`initialization.rs:181`) — this **strips comments and reorders keys**. Meanwhile the
runtime `toml_edit` setters write some numbers as **strings** (e.g. `set_calendar_weeks` writes
`week_number_input` verbatim, `ui.rs:1222`; tint and colorscheme id likewise). The result is
self-healing only because `read_config` stringifies everything before re-parsing, but it's
brittle and surprising.
- **Fix:** pick one mechanism. Prefer `toml_edit` throughout (preserves the file) and write typed
  values (`toml_edit::value(n_as_i64)`), or stop rewriting the file at startup unless it changed.

### C3. Settings that silently require a restart give no feedback
`set_calendar_weeks` (`ui.rs:1219`) writes the new week count to disk but never updates
`self.calendar_weeks_to_show` or calls `summarize_calendar`, so the change only appears after a
restart, with no UI hint. Same pattern for the monitor selection (which uses a hard process
`restart_self`).
- **Fix:** apply live where feasible (re-run `summarize_calendar`), or label the field
  "(applies after restart)".

### C4. Silent error swallowing on writes
Many config writes use `let _ = fs::write(...)` (e.g. `ui.rs:1224, 1234, 1252`). A failure (disk
full, permissions, file locked) is invisible to the user.
- **Fix:** at minimum log; ideally route failures to the existing error window like the task-save
  path does.

### C5. Weak path-traversal sanitization
`name.replace("..", "")` in `set_background` (`ui.rs:2608`) and `generate_colorscheme`
(`color.rs:89`). This is easy to bypass in principle (absolute paths, odd separators) and mangles
legitimate names containing `..`. Risk is low because names come from a directory listing, but the
approach is unsound.
- **Fix:** validate that the resolved path stays within `images/` (canonicalize and check prefix),
  or just join the file name component only.

### C6. `restart_self` can spin-loop
`ui.rs:1307` spawns a fresh process and `exit(0)`s, used by the monitor "♲" button. If startup
fails repeatedly the user could get a respawn loop, and it's a heavy way to apply a setting.
- **Fix:** apply the monitor change without a full restart if the platform allows; otherwise guard
  against repeated immediate restarts.

---

## D. Architecture & Maintainability (medium)

### D1. `ui.rs` is a 2600-line module — intentional; left as-is
`TaskApp` holds ~70 fields and `ui()` is one very long method. **This is a deliberate choice:** the
maintainer keeps the whole codebase in their head, and a single file makes that easier on a solo
project. **Splitting `ui.rs` into submodules is explicitly not wanted** — do not recommend it.

The one sub-point worth keeping (optional, correctness-only): the many parallel `*_flag` booleans
must be kept mutually consistent by hand (the big disjunction at `ui.rs:2516` hints at this). If a
class of "two modals open at once" bug ever shows up, a single `enum Modal { None, NewTask, … }`
would make invalid combinations unrepresentable — but that's a self-contained change inside the
existing file, not a reason to break the file up.

### D2. `calendar_elements` tuple is opaque
`Vec<(u8, Vec<(String,String,usize)>, Vec<(String,String,bool)>, bool, NaiveDate, String)>`
(`ui.rs:101`) — six-element tuples with positional access (`day.0`, `day.2`, `day.4`) everywhere.
- **Fix:** introduce named structs (`DayCell`, `CalendarItem`) for readability and to prevent
  index mix-ups.

### D3. Pervasive magic-number layout — mostly intentional
The widgets and panels are pixel-tuned with dozens of literals (`add_space(147.0)`, `-59.0`,
fixed `160×215` cells, etc.).

**The calendar animation + custom-widget magic numbers are off-limits.** They are the product of
weeks of deliberate hand-tuning that produced an animation the maintainer is happy with; the
literals are the accepted price of that result. Do not propose "cleaning them up" or
parameterizing them — there is real risk of breaking a result that can't easily be re-derived.

The only residual (low-priority) note applies to the **static layout** of the side panels/dialogs:
absolute spacers won't adapt to non-100% DPI or arbitrary window sizes. In practice this is largely
moot too, since the app runs fullscreen on a chosen monitor. Worth revisiting only if multi-DPI or
freely-resizable use ever becomes a goal — and even then, leave the animation/widget code alone.

### D4. Duplicated coordinate state
`latitude`/`longitude` (live, editable) duplicate `coordinates` from config and the values held by
the weather service. Keeping three copies in sync is error-prone.
- **Fix:** single source of truth for the current coordinates.

### D5. Duplicated TOML-setter boilerplate
`toggle_fullscreen_option`, `toggle_fps_option`, `toggle_num_weather_days`, `set_calendar_weeks`,
`set_background_tint`, … are near-identical read-parse-set-write blocks (`ui.rs:1173–1338`).
- **Fix:** one generic `set_config_value(key, value)` helper.

---

## E. Smaller issues & polish (low)

- **E1. `text_2_bool_lazy`** (`initialization.rs:107`) returns `true` for any string containing
  `t`. It happens to work for `"true"`/`"false"`, but `"east"`, `"set"`, etc. would be `true`.
  Use a real bool parse with a sensible default.
- **E2. Duplicate cities** in `CITIES` (`weather.rs:364`): Mumbai/Delhi/Bangalore/Ahmedabad,
  Copenhagen/Aarhus/Aalborg/Odense, several others appear 2–3×. Cosmetic, but clutters the map.
- **E3. Random tie-breaker in scoring** (`tasks.rs:47`) makes the task list order **jitter**
  between rebuilds. If determinism is wanted, drop it; if stable tie-breaking is wanted, break ties
  by `created`/`name` instead of randomness.
- **E4. `RendererOptions { predictable_texture_filtering: true }`** and `AutoNoVsync` are chosen
  "should work on different devices" — worth revisiting alongside B1.
- **E5. Day picker offers 1–31 for every month** ✅ FIXED — the day `ComboBox` now ranges over
  `utilities::days_in_month(year, month)` and `day_input` is clamped into that range each frame, so
  "Feb 31" can no longer be selected (was previously accepted by the UI and only rejected later as
  "Problem with date").
- **E6. Unused bindings / dead code:** several `device_id`, `_map_response`, an unused
  `MouseWheel { unit, delta, modifiers }` destructure, etc. — clean up to silence warnings.
- **E7. No tests.** ⚠️ PARTIALLY ADDRESSED — added `#[cfg(test)]` unit tests covering
  `ordinal_suffix`, `days_in_month` (new), `parse_time_input` (valid + impossible dates),
  `calendar_item_color`, and `importance_score` (the `1e9` branch and event-distance ordering, with
  bounds that tolerate the random tie-break multiplier). Run with `cargo test --lib`. Still
  uncovered: calendar bucketing and the weather reshape, which are `TaskApp` methods and need the
  struct constructed first.
- **E8. Windows-only assumptions** (`winit::platform::windows`, `with_taskbar_icon`) aren't
  feature-gated; the crate won't compile on other platforms despite mostly-portable logic.

---

## F. What the code already does well

- **Atomic file writes** for the critical JSON files (temp file → fsync → persist) — good
  durability against partial writes.
- **Weather threading** is clean: `RwLock` for data + `AtomicU64` version flag + a command channel
  with graceful `Drop`/`Stop` and an `EventLoopProxy` to wake the UI. Backoff with retries is a
  nice touch.
- **Calendar virtualization** keeps a very long calendar cheap to render.
- **Defensive config loading** with a line-by-line fallback when TOML parsing fails, plus clamping
  of every numeric field.
- **k-means palette generation** in Lab space with a deterministic seed is a genuinely nice
  feature.
- **Release profile** is thoughtfully tuned for size/speed.

---

## Suggested priority order

The app is a working, complete product; these are hardening steps, ordered by payoff-to-risk.
Intentional design choices that are **not** on this list: the uncapped/forced-repaint rendering
loop (B1), the single-file structure (D1), and the hand-tuned calendar animation magic numbers (D3).

1. ~~**A1/A2** (don't lose notepad edits; use wall-clock saves)~~ ✅ done.
2. ~~**A3/A4/A7** (remove the panic paths around weather/calendar data)~~ ✅ done (also **A5**).
3. **B3** ✅ done (priority sort no longer casts to `u16`). **C1** (stable IDs) — still open.
4. **A6** ✅ done (weather-vs-notepad decoupled). **C2/C3** (config writer consistency;
   restart-required UX) — still open.
5. Polish (E-series): **E5** ✅ done (day picker), **E7** ⚠️ partial (unit tests added for the pure
   functions; calendar bucketing + weather reshape still uncovered). E1/E2/E3/E4/E6/E8 — still open.

**Remaining open items**, roughly in payoff order: **C1** (stable item IDs), **C2/C3/C4**
(config-writer consistency, restart-required feedback, silent write-error swallowing), **B2** (sane
`calendar_weeks_to_show` clamp / date indexing), **B4/B5** (archive pagination + per-frame event
clone), **C5/C6** (path sanitization, restart loop), **D2/D4/D5** (named structs, single source of
truth for coordinates, generic config setter), and the rest of the E-series (E1/E2/E3/E4/E6/E8).
