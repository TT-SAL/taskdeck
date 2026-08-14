# TaskDeck — Technical Documentation

> A native desktop calendar / task-deck application written in Rust, rendered with
> `egui` on a `wgpu` backend. Displays a long vertically-scrolling calendar, a task
> priority list, a live weather forecast, a scratch notepad, and rich theming.

- **Crate name:** `task_deck`
- **Binary name:** `TaskDeck`
- **Edition:** Rust 2024
- **Platform:** Cross-platform (Windows, macOS, Linux) from one source tree, with no
  platform-specific build steps. The three Windows-only touches are `cfg`-gated and
  inert elsewhere: the `.ico` resource (`embed-resource` no-ops off Windows), the
  console-hiding `windows_subsystem = "windows"` in release, and the taskbar icon
  (`WindowAttributesExtWindows::with_taskbar_icon`). Developed on Windows 11; built and
  run on macOS (Apple silicon). See §15 for the platform-specific notes.

---

## 1. Table of Contents

1. Overview & feature tour
2. Technology stack
3. Build, run, and the data directory
4. Module map
5. Runtime architecture (startup → event loop → frame)
6. Data model & persistence formats
7. The priority / importance scoring model
8. The calendar pipeline (`summarize_calendar` → custom widgets → animation)
9. Weather subsystem
10. Color schemes & background images
11. Configuration reference (`userconfig.toml`)
12. Custom calendar widgets reference
13. Glossary of state flags
14. Design decisions & deliberate trade-offs
15. Platform notes (Windows / macOS / Linux)
16. The day planner

---

## 2. Overview & Feature Tour

TaskDeck is a single-window desktop "wall calendar". The central panel is laid out
left-to-right as three regions:

| Region | Source | Description |
|--------|--------|-------------|
| **Left** | `show_tasks` | Scrollable list of deadline-less / prioritized **tasks**, sorted by an importance score. Hovering a card reveals ✓ (complete) and ✗ (delete) buttons. |
| **Center** | `show_calendar` | A virtualized, weeks-long calendar grid (7 columns). Each day cell shows up to 3 items with times. Rows animate (scale + fade) based on scroll velocity. Clicking a day opens a day-detail popup. |
| **Right** | `show_weather_forecast` | A 2- or 3-day weather forecast (12 two-hour slots/day) with SVG icons, **or** a free-text notepad when 3-day mode is off. |

Top menu bar: **New Task**, **New Event**, **Planner**, **Archived**, **Settings**, **Quit**
(+ optional FPS readout). The **day planner** (§16) is a second view of a single day — a
timeline you block time out on, and a tray of everything still waiting for a slot.

Additional features:
- **Events vs Tasks:** events are pinned to a date/time; tasks may have a deadline+importance, or no deadline and an "urgency" (time-importance) that grows over time.
- **Archive:** completed/deleted items are appended to a JSONL log and viewable with pagination ("Show more").
- **Weather coordinate picker:** an interactive Blue-Marble world map with zoom/pan, click-to-pick, and ~200 labeled city markers.
- **Color schemes:** user-editable 6-color palettes used to tint calendar items; palettes can be **auto-generated from the current background image** via k-means clustering in CIE-Lab space.
- **Settings:** background image, startup monitor, fullscreen, FPS counter, number of weeks, background tint %, weather coordinates, 3-day weather toggle.
- **Day planner:** drag on a day's timeline to block out time, drag unplanned tasks in from a backlog tray, and move/resize blocks. Records *when you will do* something separately from *when it is due* — see §16.
- **Idle sleep:** when unfocused and idle for 10 s, the redraw loop stops to save power.

---

## 3. Technology Stack

| Concern | Crate(s) |
|---------|----------|
| GUI (immediate mode) | `egui` 0.33, `egui_extras` (SVG), `epaint`, `emath` |
| GPU rendering | `wgpu` 27, `egui-wgpu` |
| Windowing / event loop | `winit` 0.30, `egui-winit` |
| Async bootstrap | `pollster` (blocks on the async adapter/device setup) |
| Dates / times | `chrono` (with `serde`) |
| Serialization | `serde`, `serde_json` (tasks, schemes, notepad), `toml` + `toml_edit` (config) |
| HTTP (weather) | `reqwest` (blocking, `rustls-tls`; `default-features = false` keeps system OpenSSL out of the Linux build) |
| Images | `image` (backgrounds, world map, icon) |
| Palette generation | `kmeans_colors`, `palette` (Lab/sRGB conversion) |
| Reverse file reading | `rev_lines` (archive pagination) |
| Atomic file writes | `tempfile` (`NamedTempFile::persist`) |
| Allocator | `mimalloc` (set as `#[global_allocator]`) |
| Build | `embed-resource` (embeds `resources.rc` → `icon.ico`), `chrono` (stamps `BUILD_DATE`) |

The release profile is aggressively tuned for a small, fast binary: `opt-level=3`,
`lto="fat"`, `codegen-units=1`, `strip="symbols"`, `panic="abort"`.

---

## 4. Build, Run & the Data Directory

### Building

```sh
cargo build --release
```

`build.rs`:
- compiles `resources.rc` (embeds `icon.ico` as the Windows executable icon);
- injects `BUILD_DATE` (UTC `YYYY-MM-DD`) as a compile-time env var, used in the window title
  (`TaskDeck    -   Ver.<BUILD_DATE>`).

### Asset layout

| Path | Purpose |
|------|---------|
| `<root>/images/` | User background images (`*.jpg/png`); the Settings dropdown lists this directory. Names are resolved through `AppDirs::image_path` (final component only — no traversal out of `images/`). |
| `<root>/taskdeck_data/` | Tasks, archive, notepad, colour schemes, config (see below). |
| `weather_svgs_2/` | Weather icon SVGs, **embedded at compile time** via `include_image!`. |
| `fonts/` | TTF fonts, **embedded at compile time** (`FSEX300`, `DejaVuSans`, `Anton`, `SpaceMono`, `LexendGiga`, `FacultyGlyphic`). |
| `1920px-Blue_Marble_2002.png`, `icon.png`, `noback.png` | Embedded at compile time. |

Only the first two exist at runtime; everything else is baked into the binary, so the
executable plus those two folders is the whole install.

### Directory resolution (`paths::AppDirs`)

`AppDirs::resolve()` runs **once**, first thing in `main`, and every later read or write
goes through the `AppDirs` it returns (threaded explicitly — no globals, no re-derivation
per save). Both folders are created if missing. The `<root>` is:

1. `$TASKDECK_HOME`, if set.
2. The project root, when running from `target/{debug,release}` — detected by the parent
   being named `target` **and** containing a `Cargo.toml`, so an installed binary that
   happens to sit two levels deep doesn't adopt an unrelated folder.
3. `<exe_dir>`, if it already contains `taskdeck_data/` or `images/` (an existing install).
4. `<exe_dir>`, if it is writable and not inside a macOS `.app` bundle — the portable
   layout, created on first run. Writability is probed by actually creating a temp file,
   since permission bits alone miss read-only mounts, sandboxes and ACLs.
5. Otherwise `paths::user_data_dir()`: `%APPDATA%\TaskDeck`,
   `~/Library/Application Support/TaskDeck`, or `$XDG_DATA_HOME/taskdeck`
   (`~/.local/share/taskdeck`).

> **Why not the working directory?** It used to be a mix: `taskdeck_data` was resolved from
> the executable, but `images/` and `userconfig.toml` were resolved from the **working
> directory**. Those coincide when you double-click an executable on Windows and nowhere
> else — the working directory is `/` when launched from Finder or the Dock on macOS, and
> the shell's directory when started from a terminal. The config and the backgrounds could
> therefore land somewhere other than the tasks, or nowhere at all.

Files inside `taskdeck_data/`:

| File | Format | Written by |
|------|--------|-----------|
| `read_at_startup.json` | JSON array of `Active` | `tasks::oversafe_activesave` (atomic) |
| `archived.jsonl` | newline-delimited `InActive` | `tasks::save_inactive` (append) |
| `colorschemes.json` | JSON map `u32 → ColorScheme` | `color::save_colorschemes` (atomic) |
| `notepad_text.json` | JSON string | `utilities::save_notepad_text` (atomic) |
| `userconfig.toml` | TOML | `initialization` + `toml_edit` writers |

---

## 5. Runtime Architecture

### 5.1 Startup (`main.rs`)

```
main → pollster::block_on(run())
run():
  1. EventLoop::new(); create an EventLoopProxy (used to wake UI from the weather thread)
  1b. paths::AppDirs::resolve()  → data/images dirs (created if missing), see §4
  2. get_check_and_set_config(&dirs.config_file()) → Config (reads + normalizes userconfig.toml)
  3. tasks::read_at_startup()    → Vec<Active>   (corrupt file → quarantine + empty set, see below)
  4. dirs.background_options()   → names in images/
  5. color::read_colorschemes()  → HashMap<u32, ColorScheme> (inserts default if empty;
                                    corrupt file → quarantine + default scheme)
  6. utilities::read_notepad_text()
  7. get_weather(coords, proxy)  → spawns the background weather thread, returns WeatherService
  8. build TaskAppConfig → TaskApp::new(...)
  9. task_app.summarize_calendar()   (initial calendar build / sort)
  10. App::new(task_app, ...) → event_loop.run_app(&mut app)
```

**Corrupt-file recovery.** Steps 3 and 5 must not abort the boot. If `read_at_startup.json` or
`colorschemes.json` is unreadable or fails to parse, `tasks::quarantine_corrupt_file` renames the bad
file aside (`<name>.corrupt-<timestamp>`, preserved for manual recovery) and startup continues from an
empty active set / the default colour scheme. The recovery message(s) are passed to `TaskApp` via
`TaskAppConfig::startup_error` and shown in the existing error window once the UI is up. The notepad
load already degrades gracefully via `unwrap_or`.

### 5.2 The two top-level structs

- **`App` (`initialization.rs`)** implements `winit::ApplicationHandler`. It owns the GPU
  surface/device (`AppState`), the window, idle/sleep bookkeeping, and the `TaskApp`. It is the
  *platform shell*.
- **`TaskApp` (`ui.rs`)** owns all application state and the egui UI. Its `ui(&Context)` method
  draws one frame. It is the *application*.

### 5.3 `AppState` (`initialization.rs`)

Created in `App::set_window` (called from `resumed`). Sets up:
- the wgpu `Instance`, built from the **window's display handle**
  (`InstanceDescriptor::new_with_display_handle`) — needed for the GL/EGL backend on Linux,
  which enumerates adapters via the X11/Wayland display; Windows and macOS work from the
  window handle alone. The instance is owned by `AppState` so it outlives its surface.
- a `HighPerformance` wgpu adapter + device,
- a surface preferring `Bgra8Unorm`, falling back through `Rgba8Unorm` and the sRGB variants
  to whatever the surface reports (some Linux GL / software adapters offer no `Bgra8Unorm`;
  `egui-wgpu` selects an sRGB-aware shader from the format it is given). `present_mode =
  AutoNoVsync` — see §14.1.
- the `egui_winit::State` and `egui-wgpu::Renderer`.

The window is sized in **logical** units (`with_inner_size(LogicalSize)`), and centred on the
target monitor using **physical** arithmetic — the logical size scaled by the monitor's scale
factor, compared against the monitor's physical rect. The surface is then sized from
`window.inner_size()` (physical) rather than re-deriving it. Mixing the two spaces is what
put the window half off-screen on HiDPI displays.

### 5.4 The frame (`App::handle_redraw`)

1. **Acquire the surface texture** (handling `Outdated/Lost/Timeout/Occluded/Validation`).
2. Take egui input from winit.
3. **Idle detection:** if there were no input events, no repaint requested, the window is
   unfocused and the cursor is outside, and ≥10 s have elapsed → set `in_sleep = true`.
4. `Context::run_ui` → `task_app.ui(ui)`.
5. Process viewport commands (incl. `Close`), tessellate, upload textures, encode the render
   pass (clears to white), submit, present, free freed textures.

> **Order matters in steps 1–2.** Every failure arm in the acquire abandons the frame, and
> `take_egui_input` is destructive: it hands over the accumulated input and clears it. With
> the take first, an abandoned frame silently swallowed whatever had accumulated — pending
> clicks and keystrokes, and `max_texture_side`, which `egui-winit` delivers exactly once on
> the first take. Losing the latter left egui on its conservative 2048 default forever, so
> loading any background image wider than that panicked. macOS reports `Outdated` on the
> first acquire almost every launch; Windows generally does not, which is why it only
> showed up in the port.

Points-per-pixel comes from `full_output.pixels_per_point` — the value egui actually laid the
frame out with — and is used for both `tessellate` and the `ScreenDescriptor`. It is
deliberately **not** pushed back into the context: `Context::set_pixels_per_point` sets egui's
*zoom factor*, which `egui-winit` then multiplies by the native scale factor again. The old
`ScaleFactorChanged` handler did exactly that, squaring a 2× display into 4× so the UI drew
into a quarter of the window. `ScaleFactorChanged` now only reconfigures the surface.

### 5.5 Event handling (`App::window_event`)

egui gets first crack at every event (`on_window_event`); if it consumes the event, the match
is skipped. Otherwise:
- `CloseRequested` → exit; `Resized` / `ScaleFactorChanged` → reconfigure surface;
- `Focused` / `CursorEntered` / `CursorMoved` / `CursorLeft` → clear sleep, redraw, and
  **request another redraw**.

`RedrawRequested` redraws and, **unless asleep, immediately requests another redraw** — i.e.
while awake the app renders in a continuous loop (see Performance notes in `CODE_REVIEW.md`).

### 5.6 Cross-thread wake-up

The weather thread holds an `EventLoopProxy<()>`. After a successful fetch it calls
`proxy.send_event(())`; `App::user_event` then calls `window.request_redraw()` so the new
forecast is picked up.

---

## 6. Data Model & Persistence

### `Active` (`tasks.rs`) — a live task or event

```rust
struct Active {
    id: u64,                      // stable identity; what delete/complete/lookup key on (see below)
    importance: Option<u8>,       // 0..=4 ("Not"→"Lethally" important); Some only for deadline tasks
    time_importance: Option<u8>,  // 0..=2 (urgency); Some only for deadline-less tasks
    name: String,                 // cosmetic only — may repeat and be edited freely
    created: DateTime<Local>,
    deadline: Option<DateTime<Local>>,   // when it is DUE
    is_event: bool,               // events render with a distinct palette color (index 5)
    planned_start: Option<DateTime<Local>>,  // when it will be WORKED ON (planner; tasks only)
    duration_minutes: Option<u32>,           // how long that block runs
}
```

The last two are the day planner's, and are covered in §16 — including why "due" and "planned"
are separate fields rather than one. Both are `#[serde(default)]`, so pre-planner save files
load unchanged.

**Identity.** Items are keyed by `id`, not `name`: delete/complete/lookup and the calendar day
popup all operate on the id, so duplicate or renamed names are harmless. `id` is a monotonic `u64`
handed out by `TaskApp::add_active_thing` from `TaskApp::next_id`. `id == 0` is an "unassigned"
sentinel: items loaded from a pre-id or hand-edited save (the field is `#[serde(default)]`) are
backfilled at startup by `tasks::assign_missing_ids`, which preserves any existing ids and seeds
`next_id` past the current maximum. New ids persist on the next save.

Three valid shapes:
| Kind | `is_event` | `importance` | `time_importance` | `deadline` |
|------|-----------|--------------|-------------------|------------|
| Event | true | None | None | **Some** |
| Deadline task | false | Some | None | **Some** |
| Urgency task | false | None | Some | None |

### `InActive` (`tasks.rs`) — an archived item

Same fields minus `time_importance`, plus `inactivated: DateTime<Local>`. Carries the originating
`Active::id` (also `#[serde(default)]` for legacy rows). Produced by `Active::to_inactive()` when a
task is completed.

### Persistence functions

- `read_at_startup` / `oversafe_activesave` — load/save the active set. Saving is **atomic**:
  serialize → write to a temp file in the same dir → `fsync` → `persist` (rename).
- `save_inactive` — append one JSON line to `archived.jsonl`.
- `read_lines_range(offset, limit)` — reads the archive **newest-first** using `rev_lines`,
  skipping `offset` lines and taking `limit`; powers the paginated Archive window.

---

## 7. Importance / Priority Scoring (`Active::importance_score`)

Tasks in the left list are sorted by a numeric score that grows as a deadline approaches (or as
an undated task ages). The branch chosen depends on which fields are populated:

- **Deadline task** (`importance` + `deadline`): `score = f(importance, days_until_deadline)`.
  Importance 3–4 use exponential curves (`1.2^…`, `1.17^…`); 0–2 use linear curves. Higher
  importance ⇒ steeper growth.
- **Urgency task** (`time_importance`, no deadline): `score = g(time_importance, days_since_creation)`.
  Urgency 2 is exponential; 0–1 linear. Score grows with age.
- **Event-like** (`deadline` only, both importances `None`): `score = 1e9 / (hours_to_event+1)`.
- **Malformed** (none of the above): `score = 1e9` (intended to surface broken entries).

A small random multiplier derived from the current millisecond is applied as a tie-breaker, giving
the list a gentle intentional shuffle between rebuilds.

`summarize_calendar` sorts tasks by their `importance_score(...)` as an `f32` (highest first),
evaluating the score once per task per rebuild and comparing with `partial_cmp`. (It previously
cast the score to `u16`, which saturated large scores — see `CODE_REVIEW.md` B3.)

`Active::calendar_item_color()` maps an item to a palette index 0–5: events → 5, else
`importance` → 0–4, else `time_importance` → 0–2, else 0.

---

## 8. The Calendar Pipeline

### 8.1 `summarize_calendar` (model build)

Runs at startup and after any add/delete/complete and on date rollover. Steps:

1. Partition `active_things` into events and tasks; sort events by deadline, tasks by score.
2. Compute `deadline_tasks` (tasks that have a deadline) — these are the ones placeable on the grid.
3. **Bucket** the events and the deadline-tasks by day via `tasks::bucket_by_deadline_day`
   (`HashMap<NaiveDate, Vec<&Active>>`, borrowing — no clones). This makes each cell an O(1) lookup,
   so the whole build is **O(days + items)** instead of the old O(days × items) per-day scan. Each
   bucket preserves the source order (events by deadline, tasks by score), so the "take 3" selection
   below is unchanged.
4. Find the Monday of the current week; iterate `calendar_weeks_to_show × 7` days.
5. For each day, look up that day's events and deadline-tasks; choose up to **3** (events first),
   sorted by exact time → the cell `preview: Vec<PreviewItem { name, time, color_id }>`.
6. Also build the **full** day list (`items: Vec<DayItem { id, name, time, is_event }>`) for the day
   popup — the `id` lets the popup's complete/delete buttons act on the right item.
7. Record per-row month-boundary labels in `row_contains_month_switch`.

Output is cached in `self.calendar_elements: Vec<DayCell>`, where
`DayCell { day_number, preview, items, is_today, date, label }` — named fields replacing the former
opaque positional 6-tuple.

### 8.2 `show_calendar` (view + virtualization + animation)

- **Virtualization:** only rows intersecting the scroll clip-rect are built; rows above/below are
  replaced by `add_space` of the exact row height, so 100s–1000s of weeks stay cheap to render.
- **Per-row animation:** `row_anim[row]` eases toward 1.0 when visible and 0.0 when not. Visible
  rows are scaled (0.8→1.0) and their fill/stroke alpha and inner margin are interpolated. A
  velocity model (`smoothed_scroll_velocity → animation_intensity`) speeds up / slows down the
  reveal based on how fast the user is scrolling.
- **Cell content** dispatches on item count (0→`DayNumber`, 1→`DayHeader`, 2→`+MiddleHeader`,
  3→`+BottomHeaderRotated`, 4+→`+ButtonHeaderRotated` with an overflow "…" button).
- **Click vs drag:** a manual press/drag state machine (`PressState`, `DRAG_THRESHOLD_POINTS`)
  distinguishes a tap (opens the day popup) from a scroll-drag (ignored). It is disabled while any
  modal flag is set. The events are inspected in place inside `ctx.input(|i| …)` (not cloned per
  frame).

### 8.3 Day popup

Opens for `expanded_day`. Lists the full day in styled "pill" frames; hovering a row reveals
complete/delete (tasks) or delete (events). Bottom bar: Close, **Event+**, **Task+** (which
pre-fill the date fields from the selected day).

---

## 9. Weather Subsystem (`weather.rs`)

- **`WeatherService`**: `data: Arc<RwLock<Vec<Vec<WeatherData>>>>`, `version: Arc<AtomicU64>`,
  and a command `Sender`. `Drop` sends `Stop` to the thread.
- **Background thread** (`get_weather`): builds a 10 s-timeout blocking `reqwest::Client`, then
  loops:
  - fetch from Open-Meteo (`forecast_days=3`, hourly temp/weather_code/is_day, `timezone=auto`)
    with up to 3 retries and exponential backoff;
  - on success, write `data`, bump `version`, and wake the UI via the proxy;
  - wait up to `REFRESH_INTERVAL` (600 s) on the command channel, or apply a new coordinate.
- **Data shaping** (`fix_and_cache_weather_data`, in `ui.rs`): the raw hourly data is reshaped
  into 3 days × 12 two-hour slots, averaging consecutive hours' temperature, taking the **worse**
  (max) weather code, and treating the slot as "day" if either hour was day. If the raw shape
  isn't the expected 24 hourly buckets (each with at least 3 days), `weather_is_broken_flag` is
  set; the forecast grids are then replaced by a "WEATHER IS BROKEN" notice, while the notepad (when
  3-day weather is off) stays available regardless.
- **Icons** (`icon_for_wmo`): maps WMO codes → one of the embedded SVGs, choosing day/night
  variants where available. The big comment block documents the `weather_svgs_2` naming scheme.
- **`CITIES`**: a static list (~200 entries) of `name/lat/lon` used as map markers.

---

## 10. Color Schemes & Backgrounds (`color.rs`)

- **`ColorScheme`**: `{ name, colors: [[u8;4];6], is_user_configurable }`. Six RGBA colors index
  the calendar item tints by `calendar_item_color()`.
- **`generate_colorscheme(dirs, image_name)`**: resolves the name with `AppDirs::image_path` (keeps
  only the final path component, so the load can't escape `images/`), loads it, downsamples to 200×200,
  drops near-transparent pixels, converts to CIE-Lab, runs **k-means** (`get_kmeans_hamerly`, k=6,
  deterministic seed 42), sorts clusters by a visual-significance heuristic
  (`population*0.6 + saturation*0.2 + |L-50|*0.2`), and emits 6 colors at fixed alpha 80.
  Requires ≥500 usable pixels, else returns `None`.
- Persistence mirrors tasks: atomic temp-file write to `colorschemes.json`.
- The **editor** (in `ui.rs`) lets the user color-pick each of the six swatches and **drag to
  reorder** them; Save commits the edited scheme back into the map.
- **`set_background`** (in `ui.rs`) shrinks a picture whose longest side exceeds
  `max_texture_side` (uniformly, so it isn't stretched) before uploading it.
  `Context::load_texture` *panics* on an oversized image, and this is a full-window backdrop —
  detail beyond the GPU limit isn't visible anyway.

---

## 11. Configuration Reference — `taskdeck_data/userconfig.toml`

`get_check_and_set_config` reads the file (falling back to line-by-line parsing if TOML parsing
fails), clamps/validates each field, then writes the normalized values back to disk via
`write_normalized_config`. That writer uses `toml_edit`, so it **preserves existing comments, key
order, and unknown keys** and writes each value with its real TOML type (integers/float-arrays, not
strings). A missing or unparseable file falls back to a fresh document (same self-heal as before).

| Key | Type | Default | Validation |
|-----|------|---------|-----------|
| `start_in_fullscreen` | bool | `false` | `text_2_bool_lazy` (string contains `t`) |
| `coordinates` | `[f32; 2]` (lat, lon) | `[0.0, 0.0]` | must parse to exactly 2 floats |
| `background` | string | `""` | filename within `images/` |
| `enable_fps_counter` | bool | `false` | |
| `window_size_startup` | `[f32; 2]` | `[1280, 720]` | rejected if either dim `< 200` |
| `calendar_weeks_to_show` | usize | `100` | clamped `CALENDAR_WEEKS_MIN..=MAX` (`6..=520`, ~10 years) |
| `background_image_tint_percent` | u32 | `30` | clamped `1..=100` |
| `selected_monitor_name` | string | `""` | matched against `available_monitors()`; Settings shows "No monitors detected" (no crash) if the list is empty |
| `selected_colorscheme_id` | u32 | `0` | clamped `0..=200000` |
| `three_day_weather` | bool | `false` | |
| `ui_scale_percent` | u32 | `0` (automatic) | `0` = fit to window, else clamped `UI_SCALE_MIN..=MAX` (`40..=100`) |

Runtime setting changes go through one shared helper, `TaskApp::write_config_value(key, value)`
(read → parse → set typed value → write), wrapped by `persist_config_value(key, value)` which routes
any write failure to the error window instead of dropping it. The boolean toggles and the background
picker call `persist_config_value` directly; the setters that also mutate live state
(`set_calendar_weeks`, `set_background_tint`, `set_weather_coordinates`, `set_selected_monitor_name`,
`set_colorscheme`) do their side-effect and then call it. Both the startup writer and these setters
share the same mechanism and value types, so the file no longer round-trips numbers as strings.

**Apply timing.** Most settings apply live. `set_calendar_weeks` updates `calendar_weeks_to_show`
and re-runs `summarize_calendar` immediately (committed on Enter / focus-loss, not per keystroke, to
avoid rebuilding the calendar on every character); the clamp bounds are the shared
`CALENDAR_WEEKS_MIN/MAX` constants so the live value matches what a restart would load. The **startup
monitor** is the exception — the window binds to a monitor at launch, so that choice only takes
effect after a restart; the UI says "(applies after restart)" and the ♲ button restarts the app.
`restart_self` spawns a fresh copy and `exit`s only on a successful spawn; if locating the exe or
spawning fails it reports the error and keeps the current process running (no panic, no respawn loop).

---

## 12. Custom Calendar Widgets (`calendarwidgets.rs`)

Each implements `egui::Widget` with a fixed `60.0` height and draws via the painter. They share a
visual language: a rounded "notch" around the day number, two-line wrapped item text, and small
"hour mark" pills drawn with an **unclipped painter** so they can spill outside the cell.

| Widget | Used when a day has… | Notable detail |
|--------|----------------------|----------------|
| `DayNumber` | 0 items | Just the day number (top-left). |
| `DayHeader` | the 1st item | Number + 2-line title + top hour-mark; custom rounded top-right polygon. |
| `MiddleHeader` | the 2nd item | Plain rounded rect; optional bottom hour-mark. |
| `RotatedNumberOnly` | filler for 0–2 item days | Day number rotated 180° in the bottom-right. |
| `BottomHeaderRotated` | the 3rd item (exactly 3) | Rotated number + title + top & bottom hour-marks. |
| `ButtonHeaderRotated` | the 3rd slot (4+ items) | Same as above plus a "…" overflow button. |

> These widgets are pixel-tuned with many magic offsets; they assume the ~160×215 cell size.

---

## 13. Glossary of `TaskApp` State Flags

| Flag | Meaning |
|------|---------|
| `new_task_flag` / `new_event_flag` | Show the create-task / create-event modal. |
| `error_flag` + `error_text` | Show the (top-most) error modal. |
| `display_archive_flag` | Show the Archive window (paginated). |
| `expand_calendar_day_flag` + `expanded_day` | Show the day-detail popup for a cell index. |
| `planner_flag` + `planner_day` | Show the day planner, and which day it is on. The day is separate from `expanded_day` (a cell index) so the planner can step past the end of the calendar's range. |
| `planner_drag` / `planner_selection` / `planner_naming` | In-flight timeline gesture, selected block, and the block whose title is being typed. |
| `settings_flag` | Show Settings. |
| `color_picker_flag` / `edit_colorscheme_flag` / `rename_colorscheme_flag` | Color-scheme manager sub-modals. |
| `user_wants_to_complete_task_flag` + `confirm_complete_task` | Pending "mark complete?" confirmation. |
| `user_wants_to_delete_task_flag` + `confirm_delete_task` | Pending "delete?" confirmation. |
| `user_wants_to_delete_colorscheme_flag` | Pending scheme deletion. |
| `coordinates_map_flag` | Show the world-map coordinate picker. |
| `should_save_textbox_text` | Notepad has unsaved edits. Flushed by a ~2 s wall-clock debounce (`last_textbox_edit_time`) and force-flushed on exit via `flush_pending_saves` (`App::exiting`). |
| `weather_is_broken_flag` | Weather data wasn't in the expected shape. |
| `hovered_calendar_cell` / `press_origin` | Calendar hover + click/drag tracking. |

When any modal flag is set, `hovered_calendar_cell` is cleared at the end of `ui()` so the
calendar doesn't show a hover state behind a modal.

---

## 14. Design Decisions & Deliberate Trade-offs

These are choices that look like problems at first glance but are intentional. They are recorded
here (rather than in `CODE_REVIEW.md`) so that a future reader — or a future review pass — does not
"fix" them and regress something the maintainer wants. **Please read this section before proposing
changes in these areas.**

### 14.1 Uncapped, forced-repaint render loop

`present_mode = AutoNoVsync` (`AppState` in `initialization.rs`) together with an unconditional
`window.request_redraw()` after every `RedrawRequested` while awake means the app renders as fast
as the GPU allows, with no frame cap, whenever it is focused/active.

This is **wanted**, for two reasons:

1. The uncapped frame rate is a feature. Seeing the calendar run at very high fps is part of the
   appeal; capping it (`Fifo` / `AutoVsync`) is explicitly *not* desired.
2. The "active drain" window barely exists in practice. The app lives on a secondary monitor and is
   idle the vast majority of the time; when it is unfocused with the cursor away, the 10 s
   idle-sleep stops the redraw loop entirely (`in_sleep`). So it only renders flat-out during the
   rare moments of direct interaction — which is exactly when the smoothness is wanted.

The forced repaint is also **load-bearing for the animations**. The row animations are hand-rolled
(`row_anim` advanced by a `dt` taken from egui's `i.time`, once per *drawn* frame) and never call
egui's repaint scheduler. Without the forced `request_redraw()`, frames would only arrive on
discrete input events and animations would freeze mid-transition. Reactive / "repaint on input
only" rewrites have broken exactly this in the past.

Related: `RendererOptions { predictable_texture_filtering: true }` and the `AutoNoVsync` present
mode were chosen so the app behaves consistently across different GPUs. If the render loop is ever
revisited, revisit these together — but the loop itself is correct for this project's goals.

### 14.2 Single-file `ui.rs` / large `TaskApp`

`TaskApp` holds ~70 fields and `ui()` is one very long method in a ~2,600-line `ui.rs`. This is
deliberate: on a solo project, keeping the whole application in one file makes it easier to hold the
entire thing in your head. **Splitting `ui.rs` into submodules is not wanted.**

The one self-contained refinement that would still be welcome (without splitting the file) is
collapsing the many parallel `*_flag` booleans into a single `enum Modal { None, NewTask, … }`, so
that "two modals open at once" becomes unrepresentable. That is tracked as an open item in
`CODE_REVIEW.md`; the file structure itself is not.

### 14.3 Hand-tuned magic numbers in the calendar widgets & animation

The custom calendar widgets (`calendarwidgets.rs`) and the calendar animation are pixel-tuned with
many literal offsets, against an assumed ~160×215 cell. These numbers are the product of extended
hand-tuning that produced an animation the maintainer is happy with, and the literals are the
accepted price of that result. **Do not "clean up" or parameterize the animation/widget magic
numbers** — they are hard to re-derive and easy to break.

(The separate, lower-stakes note about *static side-panel/dialog* spacers not adapting to non-100%
DPI or arbitrary window sizes is tracked in `CODE_REVIEW.md`. It is largely moot while the app runs
fullscreen on a chosen monitor, and even then the animation/widget code stays untouched.)

### 14.4 Random tie-break shuffle in `importance_score`

`Active::importance_score` multiplies the final score by a small random factor in `[1.0, 1.1)`
derived from the current millisecond. This is **intentional**: it gives the task list a gentle
shuffle between rebuilds rather than a frozen order, and is not a bug to remove.

When the priority sort was changed to compare `f32` directly (see §7 and `CODE_REVIEW.md` B3), the
score is now evaluated **once per task per rebuild** and stored, so the shuffle is preserved while
the comparator stays consistent within a single sort.

### 14.5 UI scale instead of a responsive layout

The three columns are laid out at fixed point sizes summing to
`initialization::DESIGN_WIDTH_POINTS` (1920) — 300 for the task list, 1224 for the 7×160 calendar
grid with its 14pt gutters, and the rest for weather/notepad. That is exactly a 1920×1080 monitor
at 100% display scaling.

Anything narrower in *points* pushed the weather column off the right edge. That includes every
HiDPI Mac display (a 3024px Retina panel is 1512 points) and Windows at 125% or 150% scaling.

`TaskApp::apply_ui_scale` handles this by adjusting egui's **zoom factor**, which changes how many
points the window is worth, rather than by making the layout responsive. This is deliberate, and
follows §14.3: the widget geometry stays exactly as hand-tuned, and one scalar makes it fit any
window. The automatic value is the largest scale ≤ 100% at which the design width still fits, so on
a 1920×1080/100% setup the zoom is exactly 1.0 and **nothing changes**. `ui_scale_percent` in the
config pins it manually; `0` means automatic.

The computation is a fixed point, not a feedback loop: the window's physical width and its native
scale factor are both independent of the zoom, so `points × zoom` is constant and re-running it on
the next frame gives the same answer.

---

## 15. Platform Notes

Behaviour that differs per OS, and why.

| Area | Note |
|------|------|
| **Windows-only code** | `windows_subsystem = "windows"` (release), `embed-resource` compiling `resources.rc` (a no-op elsewhere), and `with_taskbar_icon` — all `cfg`-gated. Other platforms take the window icon, or, on macOS, the bundle icon. |
| **Data location** | Portable next to the executable where that is writable, else the platform's per-user data directory. See §4. |
| **macOS `.app` bundles** | Detected via the `Contents/MacOS` layout; data then always goes to `~/Library/Application Support/TaskDeck` rather than inside the (signed, possibly read-only) bundle. |
| **HiDPI** | See §5.3, §5.4 and §14.5 — window placement, the points-per-pixel path, and the UI scale. Effectively macOS-only in practice, but Windows at >100% scaling exercises the same code. |
| **Fullscreen key** | `F11` everywhere; additionally `Ctrl`+`Cmd`+`F` on macOS, where the system keeps `F11` for Mission Control and never delivers it to the app. |
| **Surface format** | `Bgra8Unorm` preferred, with fallbacks — see §5.3. |
| **wgpu backend** | Instance built with the window's display handle so Linux GL/EGL can enumerate adapters. |
| **TLS** | `rustls`, so no system OpenSSL is needed to build on Linux. |
| **Monitor names** | winit reports e.g. `Monitor #41057` on macOS rather than a friendly name. The Settings dropdown shows whatever winit gives it, and copes with an empty list (see `CODE_REVIEW.md` A9). |
| **Not addressed** | No `.app` bundle, `.dmg`, or Linux packaging is produced by the build; `cargo build --release` yields a plain executable on every platform. |

---

## 16. The Day Planner

A second view of a single day: a timeline you lay time out on, plus a tray of everything
waiting to be given a slot. Opened from the **Planner** menu button (today) or the day popup's
**Plan day** button (that day). `planner.rs` holds the model and geometry; `ui.rs` draws it.

### 16.1 The central idea: due ≠ planned

A calendar answers *when is this due*. A planner answers *when will I do it*. Those are
different facts about the same task — a report due Friday can be written on Tuesday morning —
and conflating them is what makes most task apps annoying to plan with.

So `Active` gained a second time field:

| Field | Meaning |
|-------|---------|
| `deadline` | when the item is **due** (unchanged; still what the calendar and the priority score use) |
| `planned_start` | when the user set aside time to **work on** it |
| `duration_minutes` | how long that block runs |

All three are `#[serde(default)]`, and `serde_json` ignores unknown fields, so save files
round-trip through a pre-planner build unchanged.

**Events are the exception.** An event's `deadline` *is* when it happens, so events are planned
by moving that; `planned_start` stays `None` for them. `Active::planner_anchor` and
`Active::is_planned` encapsulate that asymmetry so callers don't re-derive it.

The visible consequence: a task planned for Tuesday and due Friday appears **twice in the
week** — as a work block on Tuesday and as a due marker on Friday. That is the point, not a
bug. `planner::placement_for` is asked per-day rather than answering once, which is what makes
it possible.

### 16.2 Placement

`planner::Placement` is what an item looks like on a given day:

| Variant | Drawn as | Produced by |
|---------|----------|-------------|
| `Block { start, minutes }` | filled rectangle spanning its time | a task's `planned_start`, or an event with a `duration_minutes` |
| `Marker { at, due }` | thin pill | an event with no length yet (`due: false`), or a task's due time (`due: true`) |

An item with no placement on any day (an unplanned, deadline-less task) is what the backlog tray
shows. Note that the marker/block split is also the migration path: every event from before the
planner existed shows up as a marker, and dragging its bottom edge gives it a length.

### 16.3 Interaction

| Gesture | Result |
|---------|--------|
| Drag on empty timeline | Creates a block and opens its title for typing. The header's **Drag creates** toggle picks event or task. |
| Drag a backlog card onto the timeline | Sets `planned_start`; the deadline is untouched. |
| Drag a block | Moves it, keeping the grab point under the pointer. |
| Drag a block's bottom edge | Resizes it. |
| Click | Selects, revealing ✓ complete / ✗ delete / ↩ back-to-unplanned. |
| Double-click | Re-opens the title for editing. |
| `Esc` | Leaves the title editor; a second press closes the planner. |

Everything snaps to `SNAP_MINUTES` (15) and is clamped inside the day by `clamp_block`, which is
shared by create, move, and resize so all three agree on what a legal block is.

**Due markers are deliberately not draggable.** A deadline is a fact about the task; dragging it
on a planner would silently rewrite it while the user thought they were planning. Clicking one
still selects it, and the task can be dragged in from the tray to give it a *planned* time.

### 16.4 Structure

`planner.rs` is pure — no egui, no `TaskApp`. It owns the parts that are easy to get subtly
wrong and hard to see in a screenshot: time↔pixel mapping (`TimelineGeometry`), snapping and
clamping, the gesture→block arithmetic (`Drag` + `preview`), the side-by-side packing of
overlapping blocks (`lay_out`), and the day summary (`summarize`). All of it is unit-tested;
`ui.rs` decides only *which* gesture a press begins and draws the result.

Two consequences worth keeping:

- **`preview` serves both the live preview and the commit.** The commit path calls it *after*
  taking the gesture out of state, so what the user sees under the pointer and what gets saved
  cannot disagree.
- **The preview flows through `lay_out` like a real block**, so neighbours move aside live while
  a block is dragged over them.

`lay_out` groups placements into clusters of transitively-overlapping items and gives each the
first column free at its start time; every member of a cluster reports the same column count so
they line up. A block only costs a column while it actually overlaps — two back-to-back
half-hours share one.

`summarize` **unions** overlapping blocks rather than summing them, so the header's "planned"
figure answers "how much of my day is committed", not "how many block-hours exist".

### 16.5 State

The planner keeps no cached model: `planner_entries()` rebuilds from `active_things` every
frame, so it cannot drift out of sync with the calendar the way a second copy would. The only
persistent state is the flag, the day being shown, the selection, the in-flight gesture
(`planner_drag`), and the title being typed. `planner_flag` is listed in `any_modal_open()`.

---

*See [`CODE_REVIEW.md`](CODE_REVIEW.md) for an analysis of problems, risks, and suggested improvements.*
