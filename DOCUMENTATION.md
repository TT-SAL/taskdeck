# TaskDeck — Technical Documentation

> A native desktop calendar / task-deck application written in Rust, rendered with
> `egui` on a `wgpu` backend. Displays a long vertically-scrolling calendar, a task
> priority list, a live weather forecast, a scratch notepad, and rich theming.

- **Crate name:** `task_deck`
- **Binary name:** `TaskDeck`
- **Edition:** Rust 2024
- **Platform:** Windows-first (uses `winit::platform::windows`, embeds an `.ico`, hides the
  console with `windows_subsystem = "windows"` in release). Most logic is cross-platform,
  but it is not currently built/tested for other OSes.

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

---

## 2. Overview & Feature Tour

TaskDeck is a single-window desktop "wall calendar". The central panel is laid out
left-to-right as three regions:

| Region | Source | Description |
|--------|--------|-------------|
| **Left** | `show_tasks` | Scrollable list of deadline-less / prioritized **tasks**, sorted by an importance score. Hovering a card reveals ✓ (complete) and ✗ (delete) buttons. |
| **Center** | `show_calendar` | A virtualized, weeks-long calendar grid (7 columns). Each day cell shows up to 3 items with times. Rows animate (scale + fade) based on scroll velocity. Clicking a day opens a day-detail popup. |
| **Right** | `show_weather_forecast` | A 2- or 3-day weather forecast (12 two-hour slots/day) with SVG icons, **or** a free-text notepad when 3-day mode is off. |

Top menu bar: **New Task**, **New Event**, **Archived**, **Settings**, **Quit** (+ optional FPS readout).

Additional features:
- **Events vs Tasks:** events are pinned to a date/time; tasks may have a deadline+importance, or no deadline and an "urgency" (time-importance) that grows over time.
- **Archive:** completed/deleted items are appended to a JSONL log and viewable with pagination ("Show more").
- **Weather coordinate picker:** an interactive Blue-Marble world map with zoom/pan, click-to-pick, and ~200 labeled city markers.
- **Color schemes:** user-editable 6-color palettes used to tint calendar items; palettes can be **auto-generated from the current background image** via k-means clustering in CIE-Lab space.
- **Settings:** background image, startup monitor, fullscreen, FPS counter, number of weeks, background tint %, weather coordinates, 3-day weather toggle.
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
| HTTP (weather) | `reqwest` (blocking) |
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

### Asset layout (relative to the working directory at runtime)

| Path | Purpose |
|------|---------|
| `images/` | User background images (`*.jpg/png`); the Settings dropdown lists this directory. |
| `weather_svgs_2/` | Weather icon SVGs, **embedded at compile time** via `include_image!`. |
| `fonts/` | TTF fonts, **embedded at compile time** (`FSEX300`, `DejaVuSans`, `Anton`, `SpaceMono`, `LexendGiga`, `FacultyGlyphic`). |
| `1920px-Blue_Marble_2002.png`, `icon.png`, `noback.png` | Embedded at compile time. |

> ⚠️ Backgrounds in `images/` and the config/data files are loaded **relative to the current
> working directory**, not the executable path. See §5 for the asymmetry with `taskdeck_data`.

### Data directory resolution (`tasks::get_data_dir`)

Data is stored under a `taskdeck_data/` folder, resolved by:
1. `<exe_dir>/taskdeck_data` if it exists (production layout); else
2. `<exe_dir>/../../taskdeck_data` (the dev layout, i.e. project root when running from `target/debug/`).

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
  2. get_check_and_set_config()  → Config (reads + normalizes userconfig.toml)
  3. tasks::read_at_startup()    → Vec<Active>
  4. enumerate images/ dir       → background_options
  5. color::read_colorschemes()  → HashMap<u32, ColorScheme> (inserts default if empty)
  6. utilities::read_notepad_text()
  7. get_weather(coords, proxy)  → spawns the background weather thread, returns WeatherService
  8. build TaskAppConfig → TaskApp::new(...)
  9. task_app.summarize_calendar()   (initial calendar build / sort)
  10. App::new(task_app, ...) → event_loop.run_app(&mut app)
```

### 5.2 The two top-level structs

- **`App` (`initialization.rs`)** implements `winit::ApplicationHandler`. It owns the GPU
  surface/device (`AppState`), the window, idle/sleep bookkeeping, and the `TaskApp`. It is the
  *platform shell*.
- **`TaskApp` (`ui.rs`)** owns all application state and the egui UI. Its `ui(&Context)` method
  draws one frame. It is the *application*.

### 5.3 `AppState` (`initialization.rs`)

Created in `App::set_window` (called from `resumed`). Sets up:
- a `HighPerformance` wgpu adapter + device,
- a `Bgra8Unorm` surface with `present_mode = AutoNoVsync`,
- the `egui_winit::State` and `egui-wgpu::Renderer`.

### 5.4 The frame (`App::handle_redraw`)

1. Take egui input from winit.
2. **Idle detection:** if there were no input events, no repaint requested, the window is
   unfocused and the cursor is outside, and ≥10 s have elapsed → set `in_sleep = true`.
3. Acquire the surface texture (handling `Outdated/Lost/Timeout/OutOfMemory`).
4. `begin_pass` → `task_app.ui(ctx)` → `end_pass`.
5. Process viewport commands (incl. `Close`), tessellate, upload textures, encode the render
   pass (clears to white), submit, present, free freed textures.

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
    importance: Option<u8>,       // 0..=4 ("Not"→"Lethally" important); Some only for deadline tasks
    time_importance: Option<u8>,  // 0..=2 (urgency); Some only for deadline-less tasks
    name: String,                 // also the de-facto unique identity (see §7)
    created: DateTime<Local>,
    deadline: Option<DateTime<Local>>,
    is_event: bool,               // events render with a distinct palette color (index 5)
}
```

Three valid shapes:
| Kind | `is_event` | `importance` | `time_importance` | `deadline` |
|------|-----------|--------------|-------------------|------------|
| Event | true | None | None | **Some** |
| Deadline task | false | Some | None | **Some** |
| Urgency task | false | None | Some | None |

### `InActive` (`tasks.rs`) — an archived item

Same fields minus `time_importance`, plus `inactivated: DateTime<Local>`. Produced by
`Active::to_inactive()` when a task is completed.

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
3. Find the Monday of the current week; iterate `calendar_weeks_to_show × 7` days.
4. For each day, collect that day's events and deadline-tasks; choose up to **3** (events first),
   sorted by exact time → `chosen_str: Vec<(name, "HH:MM", color_id)>`.
5. Also build the **full** day list (`all_str: Vec<(name, "HH:MM", is_event)>`) for the day popup.
6. Record per-row month-boundary labels in `row_contains_month_switch`.

Output is cached in `self.calendar_elements: Vec<(day_u8, chosen[3], full_list, is_today, NaiveDate, day_label)>`.

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
  modal flag is set.

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
  isn't the expected 24 buckets, `weather_is_broken_flag` is set and the UI shows
  "WEATHER IS BROKEN".
- **Icons** (`icon_for_wmo`): maps WMO codes → one of the embedded SVGs, choosing day/night
  variants where available. The big comment block documents the `weather_svgs_2` naming scheme.
- **`CITIES`**: a static list (~200 entries) of `name/lat/lon` used as map markers.

---

## 10. Color Schemes & Backgrounds (`color.rs`)

- **`ColorScheme`**: `{ name, colors: [[u8;4];6], is_user_configurable }`. Six RGBA colors index
  the calendar item tints by `calendar_item_color()`.
- **`generate_colorscheme(image_name)`**: loads an image from `images/`, downsamples to 200×200,
  drops near-transparent pixels, converts to CIE-Lab, runs **k-means** (`get_kmeans_hamerly`, k=6,
  deterministic seed 42), sorts clusters by a visual-significance heuristic
  (`population*0.6 + saturation*0.2 + |L-50|*0.2`), and emits 6 colors at fixed alpha 80.
  Requires ≥500 usable pixels, else returns `None`.
- Persistence mirrors tasks: atomic temp-file write to `colorschemes.json`.
- The **editor** (in `ui.rs`) lets the user color-pick each of the six swatches and **drag to
  reorder** them; Save commits the edited scheme back into the map.

---

## 11. Configuration Reference — `taskdeck_data/userconfig.toml`

`get_check_and_set_config` reads the file (falling back to line-by-line parsing if TOML parsing
fails), clamps/validates each field, then **re-serializes the normalized `Config` back to disk**.

| Key | Type | Default | Validation |
|-----|------|---------|-----------|
| `start_in_fullscreen` | bool | `false` | `text_2_bool_lazy` (string contains `t`) |
| `coordinates` | `[f32; 2]` (lat, lon) | `[0.0, 0.0]` | must parse to exactly 2 floats |
| `background` | string | `""` | filename within `images/` |
| `enable_fps_counter` | bool | `false` | |
| `window_size_startup` | `[f32; 2]` | `[1280, 720]` | rejected if either dim `< 200` |
| `calendar_weeks_to_show` | usize | `100` | clamped `6..=20000` |
| `background_image_tint_percent` | u32 | `30` | clamped `1..=100` |
| `selected_monitor_name` | string | `""` | matched against `available_monitors()` |
| `selected_colorscheme_id` | u32 | `0` | clamped `0..=200000` |
| `three_day_weather` | bool | `false` | |

Runtime setting changes are written back through `toml_edit` (preserving the rest of the doc),
e.g. `update_background_config`, `toggle_fullscreen_option`, `set_calendar_weeks`,
`set_weather_coordinates`, `set_selected_monitor_name`, `set_colorscheme`.

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

*See [`CODE_REVIEW.md`](CODE_REVIEW.md) for an analysis of problems, risks, and suggested improvements.*
