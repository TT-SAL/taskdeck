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
| **Center** | `show_calendar` | A virtualized, weeks-long calendar grid (7 columns). Each day cell shows up to 3 items with times. Rows animate (scale + fade) based on scroll velocity. Clicking a day opens the planner on it (§16). |
| **Right** | `show_weather_forecast` | A 2- or 3-day weather forecast (12 two-hour slots/day) with SVG icons, **or** a free-text notepad when 3-day mode is off. |

Top menu bar: **New Task**, **New Event**, **Planner**, **Archived**, **Settings**, **Quit**
(+ optional FPS readout). The **day planner** (§16) is a second view of a single day — a
timeline you block time out on, and a tray of everything still waiting for a slot.

Additional features:
- **Events vs Tasks:** events are pinned to a date/time; tasks are ranked either by a deadline plus a **severity** (how bad is missing it) or, with no deadline, by a **horizon** (roughly how soon it should happen) that ripens over time.
- **Archive:** completed/deleted items are appended to a JSONL log and viewable with pagination ("Show more").
- **Weather coordinate picker:** an interactive Blue-Marble world map with zoom/pan, click-to-pick, a graticule, ~270 city markers, and the nearest of them named for whatever you picked (§9.2).
- **Color schemes:** user-editable 6-color palettes used to tint calendar items; palettes can be **auto-generated from the current background image** via k-means clustering in CIE-Lab space.
- **Settings:** one sheet in four sections — appearance (background picture, its brightness, the colour scheme), window (UI scale, startup monitor, fullscreen, frame-rate readout), calendar (weeks shown), weather (coordinates, two or three day forecast). See §11.1.
- **Day planner:** clicking any calendar day opens it. Drag on the day's timeline to block out time, drag unplanned tasks in from the tray, move/resize blocks, and set a due time without leaving the day. Records *when you will do* something separately from *when it is due* — see §16.
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
struct Session { start: DateTime<Local>, minutes: u32 }   // one planned block of work

struct Active {
    id: u64,                      // stable identity; what delete/complete/lookup key on (see below)
    importance: Option<u8>,       // 0..=4 severity ("Not"→"Lethally" important); dated tasks
    time_importance: Option<u8>,  // 0..=3 horizon (month/week/days/whenever); undated tasks
    name: String,                 // cosmetic only — may repeat and be edited freely
    created: DateTime<Local>,
    deadline: Option<DateTime<Local>>,   // when it is DUE (tasks); when it HAPPENS (events)
    is_event: bool,               // events render with a distinct palette color (index 5)
    sessions: Vec<Session>,       // when it will be WORKED ON (planner; tasks only)
    planned_start: Option<DateTime<Local>>,  // LEGACY single slot; folded into sessions at load
    duration_minutes: Option<u32>,           // event: block length. task: total ESTIMATE
}
```

`sessions` and `duration_minutes` are the day planner's, covered in §16 — including why "due"
and "planned" are separate fields, and why a task carries a *list* of sessions rather than one
slot. All planner fields are `#[serde(default)]`, so older save files load unchanged;
`tasks::migrate_legacy_plans` runs at startup and folds a pre-sessions `planned_start` +
`duration_minutes` pair into a single `Session` (the length also stays as the estimate, which is
what it was in practice). The legacy field is `None` from then on and nothing else reads it.

**Identity.** Items are keyed by `id`, not `name`: delete/complete/lookup and the calendar day
popup all operate on the id, so duplicate or renamed names are harmless. `id` is a monotonic `u64`
handed out by `TaskApp::add_active_thing` from `TaskApp::next_id`. `id == 0` is an "unassigned"
sentinel: items loaded from a pre-id or hand-edited save (the field is `#[serde(default)]`) are
backfilled at startup by `tasks::assign_missing_ids`, which preserves any existing ids and seeds
`next_id` past the current maximum. New ids persist on the next save.

Three valid shapes:
| Kind | `is_event` | `importance` (severity) | `time_importance` (horizon) | `deadline` |
|------|-----------|--------------|-------------------|------------|
| Event | true | None | None | **Some** |
| Dated task | false | Some | kept but dormant | **Some** |
| Undated task | false | kept but dormant | Some | None |

"Kept but dormant": the footer's due editor moves tasks between the two shapes at will, so both
knobs may be present on one task — whichever matches the deadline's presence is live (§7.3), and
the other is remembered for the day the deadline is added or cleared again.

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

Tasks in the left list are sorted by one number, highest first. That number is always

```
score = weight × pressure
```

**`weight`** is how much the task matters — fixed, chosen by the user. **`pressure`** is how much
it matters *right now* — it moves with the clock. Separating the two is what makes the scale mean
anything: both factors are bounded, so scores from different kinds of task are directly
comparable, a far-off deadline cannot drown out an imminent one, and nothing can overflow.

### 7.1 The two models

The knob a task carries answers a different question depending on whether it is dated. With a
deadline, timing comes from the date, and the knob is **severity**: how bad is missing it —
paying the electric bill on time is lethally important, a piece of homework merely highly.
Without one, there is nothing for a severity to rank against; what the user actually knows is
*roughly how soon it should happen*, so the knob is a **horizon** — a soft timescale, stored in
`time_importance` and worn openly as one ("within a week" ripens over ten days).

| Kind | Pressure |
|------|----------|
| **Dated task** (`deadline` set) | `2^(-days_left / lead)` — halves for every `lead` days of remaining time, is exactly `1.0` **at** the deadline, and keeps doubling once overdue (`OVERDUE_DOUBLING_DAYS`, capped at `OVERDUE_PRESSURE_CAP`). |
| **Undated task** (`time_importance` horizon, no deadline) | `1 - 2^(-age / ripen)` — rises from 0 towards 1 as the task sits, reaching half at `ripen` days. It approaches the task's weight and stops, so an undated task can rise into view but never shouts down something that is actually due. |
| **Planned task** (any `sessions`, §16) | Same curve as a deadline but on the *most pressing session* and with a short `PLANNED_LEAD_DAYS` (0.5), **capped at 1.0**. A plan says "do it at this time", so the task climbs over the hours before its slot — with work booked this afternoon and more on Thursday, this afternoon is what counts. A session that came and went is a plan you didn't keep, which is not a missed deadline and doesn't escalate like one. |

Both are continuous and monotonically increasing with the passage of time, which is the property
the list ordering rests on.

### 7.2 The tables (this is the whole policy)

| `importance` | Label | weight | lead |
|---|---|---|---|
| 0 | Not important | 1 | 0.5 d |
| 1 | Mildly important | 2 | 1 d |
| 2 | Important | 4 | 2 d |
| 3 | Highly important | 8 | 4 d |
| 4 | Lethally important | 16 | 8 d |

| `time_importance` | Label | weight | ripen |
|---|---|---|---|
| 0 | Within a month | 1 | 30 d |
| 1 | Within a week | 2 | 10 d |
| 2 | Within days | 4 | 3 d |
| 3 | Whenever | ½ | 90 d |

The horizon's index order is a **serialization fact**: 0–2 are load-compatible with the old
three-level "urgency" scale (identical weights and curves, new names), and "whenever" is
appended at index 3 even though it is the slowest tier. `HORIZON_DISPLAY_ORDER` presents them
soonest-first in every combo, and `calendar_item_color` maps index 3 to the calmest palette
slot rather than the "highly important" one its index would buy. "Whenever" weighs half the
lowest dated tier: a parking-lot idea should surface eventually, never by out-arguing anything
with a real claim.

Weights double per level, so **one step of importance is worth exactly one doubling of time
pressure** — that is what makes the two commensurable. Lead times set how early a task starts to
be felt, and with the weights they also bound how long severity out-argues a horizon: a task
out-ranks a trivial one sitting at *its own* deadline for `lead × log2(weight)` days — about a
month at the top level. Lengthening a lead time lengthens that dominance too.

What the numbers come out as:

| days to deadline | imp 0 | imp 1 | imp 2 | imp 3 | imp 4 |
|---|---|---|---|---|---|
| 30 | 0.00 | 0.00 | 0.00 | 0.04 | 1.19 |
| 14 | 0.00 | 0.00 | 0.03 | 0.71 | 4.76 |
| 3 | 0.02 | 0.25 | 1.41 | 4.76 | 12.34 |
| 0 (due) | 1.00 | 2.00 | 4.00 | 8.00 | 16.00 |
| −2 (late) | 4.00 | 8.00 | 16.00 | 32.00 | 64.00 |

Being maximally late is worth two steps of importance and no more, so a trivial task a week
overdue reads as about as pressing as an important one due today — a nag, not an emergency.

### 7.3 Which model applies

1. **A deadline decides**, whenever there is one — but pressure is the **greater** of the deadline's
   and the sessions', so a task due Friday that you set aside Tuesday morning for rises on
   Tuesday morning: that is when you decided to do it. A plan never *lowers* a task.
   `importance` is used if set and `ASSUMED_IMPORTANCE` (2) assumed otherwise — the due editor
   seeds it when it dates a task, so the gap is only reachable from a hand-edited save, and
   middling beats broken. A `time_importance` alongside a deadline is dormant (§6), not given
   its own precedence rule.
2. Else, if there are **sessions**, the most pressing one supplies the pressure. The weight comes
   from `importance`, else `time_importance`, else `ASSUMED_IMPORTANCE`.
3. Else **`time_importance`** → the ripening model.
4. Else **`importance` with no deadline** → the ripening model at the middle rate, carrying the
   importance weight.
5. Else nothing to go on → `MALFORMED_SCORE` (1e6), far above any reachable real score (the
   maximum is 16 × 4 = 64), so a corrupt entry surfaces at the top where it gets noticed.

### 7.4 What this replaced

The previous model was **inverted**: `days_since_creation` in the deadline branches was actually
*days remaining*, and every curve grew with it. Measured, a "lethally important" task scored 633
thirty days out and 26.8 when a week overdue — so deadlines **sank as they approached** and the
most overdue task in the list sat at the bottom, the exact opposite of what the README describes.
The branches were also mutually incommensurable (linear curves topping out near 17 against
exponentials reaching 1e38 and 1e9 sentinels), so importance 3–4 buried everything else regardless
of timing, and an undated task's score grew without bound — 2659 after 90 days.

`summarize_calendar` sorts by the `f32` directly (highest first), evaluating the score once per
task per rebuild so the comparator stays consistent. (It once cast to `u16`, which saturated large
scores — `CODE_REVIEW.md` B3.)

`Active::calendar_item_color()` maps an item to a palette index 0–5: events → 5, else
`importance` → 0–4, else `time_importance` → 0–2 (index 3, "whenever", wears 0), else 0.

> **A plan changes *when* a task is pressing, never *how much* it matters.** Sessions feed
> pressure, never weight, and only ever raise a score (the deadline and the sessions are combined
> with `max`). Scheduling something cannot make it more important than the user said it was — but
> on the afternoon you set aside for it, it will be at the top of the list, which is the point.
>
> This matters most for a task dragged out on the planner: it has a slot and **no deadline**, so the
> slot is the only timing it has. Without this term such a task scored purely on age and sat at the
> bottom of the list on the very day time was set aside for it.

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
6. Record `item_count`, the day's total. The cell's layout is chosen by it (0/1/2/3/4+), and the
   4+ case draws an overflow marker. This used to be a second, fully-cloned `Vec<DayItem>` of every
   dated item, built so the day popup could list it; the planner that replaced the popup reads
   `active_things` directly, so only the count is still owed.
7. Record per-row month-boundary labels in `row_contains_month_switch`.

Output is cached in `self.calendar_elements: Vec<DayCell>`, where
`DayCell { preview, item_count, is_today, date, label }` — named fields replacing the former
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
  distinguishes a tap (opens the planner on that day) from a scroll-drag (ignored). It is disabled
  while any modal flag is set. The events are inspected in place inside `ctx.input(|i| …)` (not
  cloned per frame). The tap reads the *date* out of the cell rather than remembering its index, so
  a rebuild underneath the planner can't leave it showing the wrong day.

### 8.3 There is no day popup

Clicking a day used to raise a read-only popup listing it, with a **Plan day** button that handed
over to the planner. Two windows answered for one day and only one of them could change it, so the
popup is gone and the tap opens the planner directly. §16.7 records what became of each of its
parts.

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
- **Cells are allocated at exactly `WEATHER_CELL` and painted**, not laid out from their contents.
  Laid out, a cell was as wide as its widest line — so a slot reading `-34` was twenty points wider
  than one reading `7`, its column grew to fit, and **the grid changed shape when the weather did**.
  The temperature is right-aligned against a fixed edge for the same reason: it is the one thing in
  the cell whose width isn't known in advance, so it grows towards an edge rather than pushing one.
  `WEATHER_GRID_WIDTH` is derived from the cell size and is what the notepad below measures itself
  against.
- **`CITIES`**: a static list (~200 entries) of `name/lat/lon` used as map markers.

### 9.1 The notepad (`show_notepad`)

The bottom of the same column, whenever the third day of weather is off. It is drawn as **one more
cell of the weather column** — the same hairline stroke, the same 15pt corner, no fill of its own —
with a `NOTES` heading and, while the two-second autosave debounce is still pending, a quiet
`unsaved` beside it.

Its width is `WEATHER_GRID_WIDTH` less its own margins, so the card ends exactly where the cells
above it do; its height is the column's remaining space (`available_height()` measured in the
column's own vertical ui) less a bottom margin, clamped. Both were literals before, and both were
wrong: a dark slab, wider than the forecast it sat under and reaching the bottom edge of the window.

Three things about laying this out are worth keeping, and all three were bugs first:

- **A `Frame` inherits the layout it is placed in**, and this one is placed in a row. Without an
  explicit `ui.vertical` inside it, the heading and the writing area were laid out *side by side* —
  the note wrapped at two characters in what was left over and hugged the right edge of the card.
  This is also what made `available_height()` look useless from inside the frame, and the height a
  constant for a while: the figure was the row's, not the column's.
- **The scroll area pins both height bounds.** With `max_height` alone it shrinks to its content and
  the card closes up like a fan when the note is short.
- **The writing area is given the height, not the card.** `ui.set_height` on the card's own ui makes
  the heading row inherit it and centre itself down the middle of the note. The field asks for as
  many rows as the space fits, computed from the real row height of the face it is set in, so an
  empty note is the same rectangle as a full one.

**Tabs are turned into spaces** on load and on every edit (`utilities::detab`). The app is set in
Fixedsys, which has no tab glyph, and epaint asks the font for one rather than handling `\t` itself
— so every tab was painted as a missing-glyph box. The field is also no longer styled as a code
editor, which is what used to insert them: `Tab` now leaves the field, as it does everywhere else,
and only a paste can still bring one in.

---

### 9.2 The coordinate picker

An equirectangular Blue Marble (1920×960) drawn into a rect whose width comes from the viewport
and whose height is half of it — anything else stretches the world. Scroll zooms about the
pointer, a drag of either button pans, a click picks, and **Use this** applies the coordinates and
closes; **Cancel** puts back what `coordinates_before_picker` remembers, because the map edits the
live coordinates (that is what makes the crosshair follow the click) and otherwise there would be
no way out of the window that wasn't a change.

**Why it used to shake.** The window was pinned to `fixed_size(1500×770)` around a 1440×712 map
plus a footer, both laid out at absolute rects derived from `available_rect_before_wrap()`. The
content came to a few points more than the window: every frame the window tried to grow, the fixed
size pulled it back, the available rect moved, and the map was drawn somewhere slightly different.
Sizing the content from the *viewport* and letting the window size itself to the content breaks the
loop — nothing in the chain now reads a value the chain produces.

The map paints a graticule every thirty degrees (equator and prime meridian a shade brighter),
a crosshair across the full width and height at the chosen point rather than a dot — at this scale
a dot is a pixel of noise over a photograph of a planet, and the lines say *which latitude* and
*which longitude* — and city markers that are dim until the pointer is near one.

`weather::nearest_city` puts a name to a point picked by eye. It compares **great-circle** angles,
not the difference of the two coordinates: a degree of longitude is 111km at the equator and
nothing at all near the pole, and the flat comparison also breaks completely at the antimeridian
(it answers "Tijuana" for a point beside New Zealand). Both cases are tested.

## 10. Color Schemes & Backgrounds (`color.rs`)

- **`ColorScheme`**: `{ name, colors: [[u8;4];6], is_user_configurable }`. Six RGBA colors index
  the calendar item tints by `calendar_item_color()`. `is_user_configurable` is false for exactly
  the built-ins (`is_builtin()` is its inverse, named for what it actually means at the call
  sites): they can be selected and duplicated, never edited, renamed, or deleted.
- **`generate_colorscheme(dirs, image_name)`**: resolves the name with `AppDirs::image_path` (keeps
  only the final path component, so the load can't escape `images/`), loads it, downsamples to 200×200,
  drops near-transparent pixels, converts to CIE-Lab, runs **k-means** (`get_kmeans_hamerly`, k=6,
  deterministic seed 42), sorts clusters by a visual-significance heuristic
  (`population*0.6 + saturation*0.2 + |L-50|*0.2`), and emits 6 colors at fixed alpha 80.
  Requires ≥500 usable pixels, else returns `None`.
- **`builtin_schemes()`**: the schemes every install has. `COLORSCHEME ZERO` (six fully
  transparent entries) stays id 0, so the untinted default look is unchanged, followed by
  `EMBER`, `TIDE`, `MOSS`, `DUSK`, and the two that go round the colour wheel instead of along a
  hue — `DISCO` and `MILD DISCO`, where the steps are told apart by hue alone and the urgency order
  is carried by the alpha curve. Each ramps quiet→loud across palette slots 0–4 (least to
  most important) with slot 5 — events — deliberately outside the ramp, so a glance at the
  calendar reads as urgency and events stand apart. A ramp is written as five plain RGB triples
  and takes its alpha from the shared `RAMP_ALPHA` curve, so no scheme can disagree with the
  others about opacity, and a palette is edited as five colours rather than twenty numbers.

  **Every step has to be tellable from the one below it at a glance.** The first version of these
  wasn't: EMBER's amber and burnt orange differed by a hue nudge at nearly the same lightness, and
  on a small calendar pill over a photograph they were one colour — steps three and four of the
  urgency scale were indistinguishable in practice. Each step now moves on hue, lightness *and*
  alpha at once, so none of them depends on a single axis being noticed. Three tests hold the
  line: adjacent steps at least `MIN_STEP_DISTANCE` (60, summed per-channel) apart, events that
  far from every step of the ramp, and alpha strictly rising up the ramp.
- **`install_builtins(&mut schemes, &mut selected_id) -> bool`**: puts the built-ins on their
  reserved ids (`builtin_schemes()[i]` ⇒ id `i`) and reports whether the map changed, so `main`
  saves only when it did. It runs on **every** startup. Seeding them once into an empty map — the
  old behaviour — meant anyone who already had a scheme never saw them, corrections to a palette
  could never reach an existing install, and a `colorschemes.json` written before the built-ins
  existed stayed a one-entry file forever. A scheme of the user's own sitting on a reserved id
  (what a pre-built-ins install looks like) is **moved to a free id, never overwritten**, and
  `selected_id` follows it — `main` then writes that id back to the config, or the next run would
  select the built-in that took its place.
- Persistence mirrors tasks: atomic temp-file write to `colorschemes.json`.
- The **manager** (in `ui.rs`) lists the schemes in two labelled sections, **Built in** and
  **Yours**, each sorted by id — `HashMap` iteration order is arbitrary *and differs between
  runs*, so an unsorted list reshuffled itself at every launch. Edit / Rename / Delete are shown
  disabled rather than hidden on a built-in: a button column that grows and shrinks as the
  selection moves is harder to aim at than one that greys out. Each row paints the palette beside
  the name, and the **name is laid out to the width the swatches leave**, cut with an ellipsis
  (`LayoutJob` + `TextWrapping { max_rows: 1, overflow_character }`) — a generated scheme is named
  after the picture it came from, and painted as a plain string it ran straight under the palette
  it was labelling. Each list is `SCHEME_ROW_PITCH × rows` tall, gap included: sizing by height
  alone left egui's default spacing to pile up and pushed the last scheme below a fold nobody
  expects in a seven-item list.
- The **editor** (in `ui.rs`) is a labelled sheet rather than a row of anonymous squares: five
  swatches under **URGENCY** with a `least → most` legend, one under **EVENTS**, and an **ON THE
  CALENDAR** strip that paints the whole palette over a dark ground. Both views are needed and
  they answer different questions — egui's colour button shows colour over a checkerboard, so
  alpha reads as alpha while you edit it, which is exactly what you cannot judge the result from
  when the colours are translucent tints meant for a photograph. A click opens the picker and a
  drag swaps two swatches: egui resolves click and drag targets separately, so the button (which
  senses clicks only) takes the click while the drag falls through to the rect underneath. The
  edit is live on the calendar behind the window; Save commits it into the map, Cancel restores.
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

Runtime setting changes go through one shared helper, `initialization::write_config_value(path, key,
value)` (read → parse → set typed value → write), which `TaskApp::write_config_value` forwards to and
`persist_config_value(key, value)` wraps to route any write failure to the error window instead of
dropping it. It is public because startup needs it too: `main` can have to correct a setting — a
selected colour scheme whose id had to move (§10) — before there is any `TaskApp` to route it through. The boolean toggles and the background
picker call `persist_config_value` directly; the setters that also mutate live state
(`set_calendar_weeks`, `set_background_tint`, `set_weather_coordinates`, `set_selected_monitor_name`,
`set_colorscheme`) do their side-effect and then call it. Both the startup writer and these setters
share the same mechanism and value types, so the file no longer round-trips numbers as strings.

**Apply timing.** Most settings apply live. Two apply when their control *settles* — the drag stops
or the field loses focus — rather than per step, and for the same reason in both cases: the step is
expensive enough to fight back. `set_calendar_weeks` rebuilds the calendar model for up to ten years
of days; `set_ui_scale` changes what every point in the window is measured in, so applying it
mid-drag moves the slider out from under the pointer. Background brightness is the opposite case —
one multiply in the draw — so it follows the slider live and only the *write to disk* waits. The
clamp bounds are the shared `CALENDAR_WEEKS_MIN/MAX` and `UI_SCALE_MIN/MAX` constants, so a live
value always matches what a restart would load. The **startup monitor** is the one setting that
cannot take effect where it is made — the window binds to a monitor at launch — so the sheet says
"takes effect at the next start" and offers **Restart now**. `restart_self` spawns a fresh copy and
`exit`s only on a successful spawn; if locating the exe or spawning fails it reports the error and
keeps the current process running (no panic, no respawn loop).

### 11.1 The settings sheet

Four named sections — **APPEARANCE**, **WINDOW**, **CALENDAR**, **WEATHER** — each a two-column
`Grid` of `settings_row(label, contents)`, plus a footer with the author line and **Done**. All the
sections share `SETTINGS_LABEL_COLUMN`, so the controls line up down the whole sheet rather than per
section, and the body scrolls past `SETTINGS_MAX_BODY_HEIGHT` instead of being cut off.

What it replaced is worth recording as a shape to avoid: a single-column `Grid` used purely as a
spacer, every row a `horizontal_centered` with a pair of `end_row()`s after it for padding, inside a
window pinned to `fixed_size(400×300)` holding far more than that. Nothing aligned with anything and
the last rows fell out of the bottom of the window.

Controls were also chosen for what they are: numbers that have a range are sliders and drag values
rather than text fields that parse and clamp on focus loss, and the UI scale is a **Fit to window**
checkbox plus a percentage slider rather than a text field where `0` secretly meant "automatic".
Escape closes the sheet, unless the map picker is stacked over it or a field has the keyboard.

One latent crash went with it: the background row indexed `background_options[selected_index]`
directly, so an index left over from a picture that had since been deleted from `images/` panicked
the moment Settings opened. The index is clamped to the list, and an empty list says so.

**The colour-scheme manager** (opened from *Colour scheme → Manage*) is built the same way: the two
labelled lists from §10 on the left, the verbs on the right, **Done** below. Each row paints the
palette as six swatches beside the name — over a dark base, because these are translucent tints
meant to sit on a photograph and on nothing at all they read as nothing. A list of names alone was
guesswork: `DUSK` and `Scheme from "lake.jpg"` are evocative, not informative, and the only way to
find out what either did to the calendar was to select it and look.

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
| `planner_flag` + `planner_day` | Show the day planner, and which day it is on. The day is a date, not a calendar cell index, so the planner can step past the end of the calendar's range and a rebuild underneath it can't repoint it. |
| `planner_drag` / `planner_selection` / `planner_naming` | In-flight timeline gesture, selected item, and the item whose title is being typed. |
| `planner_naming_created` / `planner_naming_focus` | Whether the item under the title editor was created by the gesture that opened it (Escape then removes it), and the one-frame request for keyboard focus (§16.3.5). |
| `planner_selected_session` | Which of the selection's sessions was clicked, when a work block was — what the footer's per-block controls act on. |
| `planner_due_edit` | Task whose deadline is open in the footer's due editor (§16.3.4). |
| `planner_create_kind` | What a drag or double-click on empty timeline makes: `Task` or `Event` (§16.3). |
| `planner_quick_add_input` | The tray's quick-add field. |
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
calendar doesn't show a hover state behind a modal. `any_modal_open` is `planner_flag ||
modal_over_planner()`; the second half is the same list without the planner, and is what the
planner's own keyboard shortcuts stand down for (§16.3).

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

`Active::importance_score` multiplies the final score by a small random factor. This is
**intentional**: it gives the task list a gentle shuffle between rebuilds rather than a frozen
order, and is not a bug to remove.

The score is evaluated **once per task per rebuild** and stored, so the shuffle is captured a
single time and the comparator stays consistent within a sort (see §7 and `CODE_REVIEW.md` B3).

**The implementation was rewritten because it never actually shuffled.** It read the current
millisecond *at the moment of the call*, so every task scored within the same millisecond — which,
at these list sizes, is all of them — got an identical multiplier, and multiplying every score by
the same number changes no ordering whatsoever. The only time it did anything was when a rebuild
happened to straddle a millisecond boundary, at which point an arbitrary subset of the list jumped
by up to 10% relative to the rest. So the effect was "nothing, occasionally something arbitrary".

`tie_break_jitter` now hashes the task's **id** with a **rebuild counter** (`TaskApp::shuffle_seed`,
bumped once per `summarize_calendar`), giving each task its own factor in `[1.0, 1.08)`. The
magnitude is deliberately below one importance step (a factor of two), so it can shuffle near-ties
without ever reordering tasks that genuinely differ in priority — which is asserted by a test.

**The seed is a counter and not the clock, deliberately.** Keyed on the time, it reshuffled once a
second — and the planner's backlog tray re-sorts *every frame*, so its cards crawled out from under
the pointer as you reached for one. A counter shuffles exactly when §14.4 says it should: when the
list is actually rebuilt.

### 14.5 Plans stay out of the calendar grid

A day cell shows at most three items (§12) and is read from across the room. That budget belongs to
what is **due**; filling it with what you *intend to do* would crowd out the thing the calendar
exists to tell you. So `summarize_calendar` buckets on `deadline` alone and the planner keeps plans
on its own timeline, where there is room for them.

The follow-on — a task dragged out in the planner has no deadline and therefore never appears in
the calendar — is accepted, not overlooked. It is still visible in the task list, and its slot
drives its priority there (§7). See §16.4.

### 14.6 UI scale instead of a responsive layout

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

**The scale is quantized, and that is about the font.** TaskDeck is set in Fixedsys Excelsior,
whose outlines trace a bitmap font's pixel grid: it is sharp when a glyph's em box lands on whole
pixels — sharpest at a multiple of 16px — and visibly smeared when it doesn't. Rendering one string
at 16.00px and 17.19px side by side settles the question immediately. An unconstrained fit produced
scales like 0.78125, i.e. 1.5625 points-per-pixel, at which almost no size in the app is a whole
number of pixels; that is the long-standing "crisp in some places, broken-ish in others" look.

Two things address it:

- `apply_ui_scale` rounds the automatic scale **down** to whatever step makes points-per-pixel a
  multiple of `PPP_QUANTUM` (0.25). Rounding down only ever makes the UI smaller than strictly
  required, so the layout still fits. On a 2× display the earlier 1.5625 becomes 1.5, at which
  every even point size is a whole pixel.
- `set_styles` runs each named text size through `snap_font_points`, which returns the point size
  whose *pixel* height is a whole number — preferring a multiple of the 16px grid when one is
  within `FIXEDSYS_GRID_TOLERANCE`, else the nearest whole pixel. Because the answer depends on
  the scale, `apply_ui_scale` re-runs `set_styles` whenever points-per-pixel actually changes.

An explicit `ui_scale_percent` is **not** quantized: a number the user typed is a number they meant.
At 1× — a Windows display at 100% — the common sizes are already whole pixels and nothing moves
except `Heading`, which takes 32px over 30px because it is within tolerance of the grid.

---

## 15. Platform Notes

Behaviour that differs per OS, and why.

| Area | Note |
|------|------|
| **Windows-only code** | `windows_subsystem = "windows"` (release), `embed-resource` compiling `resources.rc` (a no-op elsewhere), and `with_taskbar_icon` — all `cfg`-gated. Other platforms take the window icon, or, on macOS, the bundle icon. |
| **Data location** | Portable next to the executable where that is writable, else the platform's per-user data directory. See §4. |
| **macOS `.app` bundles** | Detected via the `Contents/MacOS` layout; data then always goes to `~/Library/Application Support/TaskDeck` rather than inside the (signed, possibly read-only) bundle. |
| **HiDPI** | See §5.3, §5.4 and §14.6 — window placement, the points-per-pixel path, and the UI scale. Effectively macOS-only in practice, but Windows at >100% scaling exercises the same code. |
| **Fullscreen key** | `F11` everywhere; additionally `Ctrl`+`Cmd`+`F` on macOS, where the system keeps `F11` for Mission Control and never delivers it to the app. |
| **Surface format** | `Bgra8Unorm` preferred, with fallbacks — see §5.3. |
| **wgpu backend** | Instance built with the window's display handle so Linux GL/EGL can enumerate adapters. |
| **TLS** | `rustls`, so no system OpenSSL is needed to build on Linux. |
| **Monitor names** | winit reports e.g. `Monitor #41057` on macOS rather than a friendly name. The Settings dropdown shows whatever winit gives it, and copes with an empty list (see `CODE_REVIEW.md` A9). |
| **Not addressed** | No `.app` bundle, `.dmg`, or Linux packaging is produced by the build; `cargo build --release` yields a plain executable on every platform. |

---

## 16. The Day Planner

The view of a single day: a timeline you lay time out on, plus a tray of everything waiting to
be given a slot. Opened by **clicking any day on the calendar**, or from the **Planner** menu
button (which opens it on today). `planner.rs` holds the model and geometry; `ui.rs` draws it.

It is the *only* per-day view. A read-only popup used to open on a day click and hand over to
the planner with a **Plan day** button; §16.7 records why that went and where each of its parts
landed.

### 16.1 The central idea: due ≠ planned

A calendar answers *when is this due*. A planner answers *when will I do it*. Those are
different facts about the same task — a report due Friday can be written on Tuesday morning —
and conflating them is what makes most task apps annoying to plan with.

So `Active` carries planning as its own fields:

| Field | Meaning |
|-------|---------|
| `deadline` | when the item is **due** (still what the calendar and the priority score use) — and, since the footer's due editor, an *editable attribute*: set, changed, or cleared on any task without recreating it |
| `sessions` | the blocks of time set aside to **work on** it — a list, because "when will I do it" is not always one answer: two hours due Friday may be an hour on Tuesday and an hour on Thursday |
| `duration_minutes` | how long the task **takes in total** — the estimate the tray spends (§16.3.2) |

All are `#[serde(default)]`, and `serde_json` ignores unknown fields, so older save files load
unchanged (`migrate_legacy_plans` folds the old single-slot shape into a session, §6).

**Events are the exception.** An event's `deadline` *is* when it happens, so events are planned
by moving that; `sessions` stays empty and `duration_minutes` is simply the block's length.
`Active::planner_anchor` and `Active::is_planned` encapsulate that asymmetry so callers don't
re-derive it.

The visible consequence: a task planned for Tuesday and Thursday and due Friday appears **three
times in the week** — a work block on each planned day and a due marker where it is owed. That
is the point, not a bug. `planner::placements_for` is asked per-day and answers with a list,
which is what makes it possible.

### 16.2 Placement

`planner::placements_for(item, day)` returns everything an item puts on a day — each element a
`DayPlacement { session: Option<usize>, placement }`, where `session` is the index of the
task's session the block belongs to (`None` for event blocks and due markers). That index is
the address every gesture edits, so a task planned twice on one day is two independently
grabbable blocks.

| `Placement` variant | Drawn as | Produced by |
|---------|----------|-------------|
| `Block { start, minutes }` | filled rectangle spanning its time | each of a task's sessions on the day, or an event with a `duration_minutes` |
| `Marker { at, due }` | thin pill | an event with no length yet (`due: false`), or a task's due time (`due: true`) |

A due marker is shown even when a session shares its day — "worked on Friday morning, due Friday
17:00" is exactly the day where seeing the deadline next to the work matters most. An item with
no placement on any day is what the tray shows. The marker/block split is also the migration
path: every event from before the planner existed shows up as a marker, and dragging its bottom
edge gives it a length.

### 16.3 Interaction

The window is a masthead, a body, and a footer:

```
 ◀ ▶     August 15th, 2026 · today
 Today   SATURDAY       3h 30m planned · 4 blocks · 2 due   New: [Task] Event   ✕
 ─────────────────────────────────────────────────────────────────────────────────────
  Unplanned        │  06 ───────────────────────────────────────────────────────
  + add a task     │  07 ───────────────────────────────────────────────────────
  DUE TODAY (2)    │  ...
  ▸ card           │
  BACKLOG (5)      │
  ▸ card           │
 ─────────────────────────────────────────────────────────────────────────────────────
  Task  Write the report  13:00–15:00  takes [2h ▾]  due Fri 21 Aug 17:00 ▾  ✎  Severity: …
```

The three groups share one row and one centre line. An earlier version stacked the right-hand
pair into a column, which does not centre as a block inside a centred row — egui aligns it from
the row's middle and it grows downwards from there, so the toggle sat below the bottom of the
50-point weekday it was meant to sit beside.

The masthead pays for its own margin (`PLANNER_EDGE_MARGIN`) at the top and down both sides. The
window frame's inner margin alone is a few points, which put the day stepper and the ✕ hard into
the corners of the window; a button whose edge *is* the window's edge reads as an accident. In
the right-hand group the space is added **first**, because that row is laid out right-to-left, so
the first thing added is the gap against the edge.

The date-over-weekday block is the calendar day popup's headline, carried over unchanged —
`format_date`'s two strings, the second in 50-point Anton, with the same `add_space(-9.0)`
between them. It is the one thing from that window worth keeping, and it does the naming a
window caption would have done, so the planner has no title bar.

| Gesture | Result |
|---------|--------|
| Drag on empty timeline | Creates an item of the masthead's **New** kind (Task or Event) and opens its title for typing. |
| Double-click empty timeline | The same, at `DEFAULT_BLOCK_MINUTES` — most of what goes on a day is half an hour of something, and aiming a precise drag for it is work the app can do instead. |
| Drag a tray card onto the timeline | **Adds a session** of the card's remaining length (§16.3.2); the deadline is untouched, and the card stays in the tray until its estimate is covered. |
| Drag a block | Moves that block, keeping the grab point under the pointer. Its siblings stay put — every gesture is addressed by `(task, session)`. |
| Drag a block's bottom edge | Resizes that block. The grip is a shaded strip with two bars, `PLANNER_RESIZE_HANDLE` tall, and brightens under the pointer. |
| `due … ▾` in the footer | Opens the **due editor** (§16.3.4): set, change, or clear the selected task's deadline. |
| `takes …` in the footer | The task's total estimate (§16.3.2); `for …` on an event is its block length. |
| `＋ Block` in the footer | Books another session on the shown day, after the last block, for the remaining estimate — so splitting work across days never needs the card to leave the tray and come back. |
| `↩ Remove block` / `↩ Unplan` | One adaptive un-book button: frees the clicked block when one is selected, the whole plan otherwise. Cheap and unconfirmed either way — the time goes back onto the card. |
| Click anything (timeline or tray) | Selects it; the **footer** shows what it is, when it runs, how long it takes, its deadline, and its severity or horizon. |
| Double-click a block | Re-opens the title for editing. |
| `Enter` in the title editor | Keeps the typed name and closes the editor. So does clicking anything else, or leaving the day — everything except Escape. On a **just-created** item it also clears the selection, because that Enter ends the whole drag-name-done gesture (§16.3.5). |
| `Esc` in the title editor | Throws the edit away. On a **just-created** item that removes the item too, unconfirmed — the undo for a block dragged out by accident. On a rename it only puts the old name back. |
| Type in the tray's quick-add | Enter makes an undated, unplanned task and keeps the field focused — a brain-dump is several tasks, not one. |
| `←` `→` `T` | Previous day, next day, today. |
| `Enter` `U` `Del` | Rename / un-book (same adaptive rule as the button) / delete the selection (delete still asks, and the dialog answers to `Enter` / `Esc`). |
| `Esc` | Backs out one level: the title editor first (§16.3.5), then the planner. |

Shortcuts stand down whenever a widget has focus (`Context::egui_wants_keyboard_input`), a
window is stacked over the planner (`TaskApp::modal_over_planner`), or either editor — the due
editor or the in-place title editor — is open. Crucially that includes **the frame an editor
closes on**, which is why `show_planner` samples both flags *before* running the body: an editor
clears its own flag the moment Enter finishes it, and the very same Enter is still in this
frame's input, so a check made afterwards answers "nothing is open" on exactly the frame that
matters. Escape is exempt and read first, so backing out still works while typing. An editor's
key must never leak into the window it was typed over.

**`New:` is two kinds now.** `planner::CreateKind` decides which time field a create fills in: a
`Task` gets its first session and no due date of its own, an `Event`'s slot *is* its deadline.
There used to be a third kind, `Deadline`, which existed only because a due time could be
*created* but never attached; the due editor (§16.3.4) made it an attribute, and the mode went
away.

Everything snaps to `SNAP_MINUTES` (15) and is clamped inside the day by `clamp_block`, which is
shared by create, move, and resize so all three agree on what a legal block is.

The controls live in a footer row rather than inside the block, for two reasons: a
15-minute block has no room for three buttons, and — more subtly — the block's own drag target
is registered over the same pixels, so buttons drawn inside it were unclickable. egui hit-tests
the *most recently added* widget first, so anything that must win a click has to be added after
the block-sized drag target. `planner_timeline` therefore registers all interactions **before**
painting; the in-place title editor, drawn afterwards, gets its clicks.

The same rule caught the **resize handle**, which is worth recording because the trap is not
obvious in the reading order. The handle is a strip inside the block's own rect, and
`handle_planner_gestures` registered it *first* and the body second — so the body, being the more
recent, took every press on it. Dragging the bottom edge moved the block instead of lengthening
it, and resizing was simply unreachable. The body is now registered first and the handle second.
When two interaction rects overlap, the one that must win goes **last**. The footer is also
where a task's **severity or horizon** is set — one combo, asking whichever question the task's
datedness makes meaningful (§7.1). It sits *below* the timeline because the row is only occupied some of the
time: at the bottom, an empty one costs nothing and a full one doesn't push the day the user is
aiming at. With nothing selected it carries the gesture hints, none of which announce themselves.

**Only tasks can be completed.** The footer's ✓ is hidden for events. Completing means "this is
done, file it in the archive", and an event is not work you finish — it is a time that arrives
and passes whether or not you were there. Offering the ✓ on one asked a question with no answer,
and filed dentist appointments in the archive as things the user had *done*. An event that
shouldn't be there is deleted; the task list never offered the ✓ on one, because it never shows
events (`refilter_tasks`), so the footer was the only place this was reachable.

**Due markers are deliberately not draggable.** A deadline is a fact about the task; dragging it
on a planner would silently rewrite it while the user thought they were planning. Clicking one
still selects it — and deliberate changes go through the due editor, where changing a deadline
looks like changing a deadline.

### 16.3.1 The tray

Every task that still **wants planning** (`Active::wants_planning`): no sessions at all, or an
estimate not yet covered by the sessions it has. A two-hour task with one hour booked is still
half a card — it reads `takes 2h · 1h booked` and dropping it books the missing hour. In two
groups (`planner::backlog_group`):

- **Due by this day** — deadline on or before `planner_day`. "On or before", not "on": something
  due Wednesday and still unplanned belongs at the top of Friday's tray too.
- **Backlog** — owed later, or not owed at all.

Within each group the order is the task list's own score (§7), so the most pressing thing to
schedule is at the top. The first group is the tray's reason for existing: *owed today and with
no time set aside for it* is the one list a day planner should lead with, and it is exactly what
the day popup used to show as a flat list you could not act on. Here every row is a card to drag
onto an hour.

### 16.3.2 How long something takes: the estimate and the budget it funds

For a task, `Active::duration_minutes` is the **total estimate** — "the physics homework takes
two hours" — and the sessions spend it. The three numbers the planner works with:

| Number | Where it lives | Meaning |
|---|---|---|
| estimate | `duration_minutes` | how long the work takes in total (`takes` in the footer) |
| booked | `Σ sessions[i].minutes` (`planned_minutes`) | how much of it has a slot |
| remaining | `remaining_minutes` | what the tray card is still worth |

`planner::drop_length_for` spends the remainder: an untouched 2-hour card lands a 2-hour block;
with an hour booked, the next drop lands the missing hour; and the card leaves the tray only when
the estimate is covered (`wants_planning`). "Split it across two days" is therefore just dragging
the same card twice — or once plus `＋ Block`. Each block's own length is edited by its bottom
edge; the footer's `takes` picker edits the estimate. (For an event, with no sessions, the same
field is simply the block's length and the picker reads `for`.)

An estimate survives unplanning — giving up on the slots is not forgetting how long the work
takes — and a create drag's length doubles as the first estimate, since the drag said exactly
that.

`planner::duration_options` folds the item's current length into the preset list, so a length
dragged out by hand — 1h 05m — reads back as the selection rather than as the nearest preset that
picking anything would round it to.

### 16.3.4 The due editor

The footer's `due … ▾` button opens a small modal: the shared date/time combos
(`display_date_entering`), seeded from the current deadline — or from the shown day at 17:00 for
a task without one — and **Set** / **No deadline** / **Cancel** (`Enter` / `Esc` answer it).
Committing goes through `set_item_deadline`, which also seeds a middling severity onto a task
gaining its first deadline, since a dated task is ranked by severity and the user hasn't said
yet; the footer's combo is right there to adjust it. Clearing the deadline returns the task to
its horizon, which was kept, not erased (§6).

This is the verb whose absence shaped the old model. A due date could only be *created* — as its
own item, on the right day, through a mode switch — never attached to the task it described, and
never changed afterwards at all. "Due Friday, worked on Tuesday, takes two hours" was a
create-unplan-replan-resize dance; it is now one drag (the block, which sets the estimate) and
one dialog (the deadline).

### 16.3.3 Type scale

`PLANNER_NAME_SIZE` / `PLANNER_META_SIZE` / `PLANNER_FINE_SIZE` (17 / 15 / 13 points) are used
throughout the planner instead of a literal per call site. The app sets Body at 18 points and
Button at 22 (`set_styles`) because it is meant to be read from across the room; the planner had
drifted to 11–14, which is a different application's typography and looked it next to a 22-point
combo box. The three sit under the body size — a timeline is denser than a task card, and a
15-minute block has to fit its own name — without dropping into the footnote range.

`PLANNER_TWO_LINE_BLOCK` is the height at which a block stops sharing one row with its name and
puts it on a second line: the time line plus a `PLANNER_NAME_SIZE` name plus the block's margins.
It is a constant rather than a literal because it is derived from the type scale and has to move
when that does.

### 16.3.5 Naming, and undoing an accident

Creating on the timeline opens the title editor in place, and the two keys that close it mean
opposite things:

| Key | A just-created item | A rename |
|---|---|---|
| `Enter` (or clicking away, or leaving the day) | keeps it, placeholder name if you typed none | keeps the new name |
| `Esc` | **removes the item** | puts the old name back |

**Enter on a just-created item also deselects it.** Drag, name, Enter is one gesture and Enter is
its end; leaving the new block lit up with a row of controls aimed at it answers a question
nobody asked. Two conditions, both necessary: only for `Enter` (losing focus by *clicking* must
not clear a selection the click has just made — the commit runs after the click is handled), and
only when `planner_naming_created` says the item is new (a rename keeps the selection you chose
deliberately).

Escape deleting outright is deliberate and unconfirmed. The item is seconds old, the only thing
in it is the slot an accidental drag gave it, and Escape is the key everyone reaches for to undo
the last thing they did; making that path go through a selection and a confirmation dialog is
what made an accidental drag annoying. Anything older is only ever deleted through the footer,
which asks. `planner_naming_created` is the whole distinction, and the footer spells the keys out
while the editor is open, because "Escape discards" is not guessable.

Committing an *unnamed* block still keeps it, under a placeholder. The block holds a real
decision — the time it was dragged out on — so blurring away from it is not the same statement as
Escape; if you meant neither, Escape says so.

**Exactly one thing hosts the editor.** A task can put several things on one day — a block per
session, plus its due marker — and they all carry the task's id, so "is this the item being
named?" does not pick one of them: every match drew its own editor, and once they shared a stable
widget id egui reported the clash outright. `planner_timeline` elects a single host per frame
(`naming_host`), preferring the block actually selected and falling back to the item's first
placement on the day. A task with *nothing* on the shown day has no block to elect, so its tray
card hosts the field instead (`BacklogCard::hosts_name_editor`, via `planner::appears_on`) —
before that, renaming such a task from the footer set the naming state and then showed the field
nowhere at all.

Two mechanics this rests on, both of which were bugs first:

- **The editor claims focus once, not every frame.** `Response::lost_focus` is a *live query*
  into egui's focus memory, not a flag baked when the widget was built. Re-requesting focus on
  every frame — which the editor did — put focus back the instant Enter made the field surrender
  it, so the query always answered "no" and **Enter could not finish an edit at all**.
  `planner_naming_focus` makes it a one-frame request.
- **The field's id is keyed to the item**, not to where it sits. Blocks move as neighbours
  re-flow around them, and an id derived from position would change with it — dropping focus,
  and with it the edit, mid-word. That only mattered once focus stopped being re-requested every
  frame, which had been masking it.

### 16.4 "Plan" is an adjective, not a noun

There is no plan *object*. A plan is the `sessions` on the task itself, so a task is never split
into records and there is never a second thing to complete, delete, or keep in sync — even
planned across three evenings, homework due Thursday is **one** `Active`: one card in the task
list, one ✓ to finish it (which releases every block), appearing on the planner as a work block
per session and a due marker where it is owed. Sessions stay deliberately dumb — a start and a
length, no name, no completion state of their own; the moment they grow either, they are
sub-tasks, and the one-✓ property is gone.

Which fields get filled in is the only difference between how a task was made:

| Created via | `deadline` | `sessions` |
|---|---|---|
| New Task dialog | set by the user, or none | empty (drag it in from the tray later) |
| Tray quick-add | empty | empty — a name and nothing else, ready to place |
| Planner drag, **Task** | empty — a plan is not a due date; the due editor adds one when there is one | the dragged slot (whose length is also the first estimate) |
| Planner drag, **Event** | the slot (an event's deadline *is* when it happens) | unused by events |

**Plans deliberately do not appear in the calendar grid.** `summarize_calendar` buckets on
`deadline` alone, so a task shows in the grid on the day it is *owed* and nowhere else. A day cell
holds at most three items (§12) and is the always-on view read from across the room; filling that
budget with "what I intend to do" would crowd out "what is actually due". Plans belong to the
planner, which has a whole timeline for them.

The consequence, which is intended and not an oversight: a task dragged out on the planner has no
deadline until the due editor gives it one, so it **does not appear in the calendar at all**. It
lives in the task list — where its sessions drive its priority (§7) — and on the planner's
timeline. A dragged-out *event* does appear in the calendar, because its slot is its deadline;
so does any task the moment it is given a due date.

### 16.5 Structure

`planner.rs` is pure — no egui, no `TaskApp`. It owns the parts that are easy to get subtly
wrong and hard to see in a screenshot: time↔pixel mapping (`TimelineGeometry`), snapping and
clamping, the gesture→block arithmetic (`Drag` + `preview`, where every move/resize is addressed
by `(task, session)` via `Drag::target`), the per-day placement list (`placements_for`), the
side-by-side packing of overlapping blocks (`lay_out`), and the day summary (`summarize`). All
of it is unit-tested; `ui.rs` decides only *which* gesture a press begins and draws the result.

Two consequences worth keeping:

- **`preview` serves both the live preview and the commit.** The commit path calls it *after*
  taking the gesture out of state, so what the user sees under the pointer and what gets saved
  cannot disagree.
- **The preview flows through `lay_out` like a real block**, so neighbours move aside live while
  a block is dragged over them.

`lay_out` groups placements into clusters of transitively-overlapping items and gives each the
first column free at its start time; every member of a cluster reports the same column count so
they line up. A block only costs a column while it actually overlaps — two back-to-back
half-hours share one. **Markers take a column too**, and `planner_entry_rect` honours it: it used
to draw them full width regardless, which painted a due marker straight over the title of the
block it happened to fall inside. Where nothing overlaps, the cluster is one column wide and a
marker still spans the timeline. Item text is drawn through a painter clipped to its own rect, so
a long title cannot spill into the neighbour it is sharing the hour with.

`summarize` **unions** overlapping blocks rather than summing them, so the masthead's "planned"
figure answers "how much of my day is committed", not "how many block-hours exist".

The body's height is **measured, not constant**: `ui.available_height()` after the masthead is
drawn. The masthead's height depends on the metrics of a 50-point face and on the UI scale, and a
constant that disagreed with either would push the footer off the bottom of the window on someone
else's display.

**How much of the day is on screen** is the product of two numbers, and both are set for it. The
window is the viewport less `PLANNER_WINDOW_INSET` — wider than it is tall, so a strip of calendar
down each side says what is behind the window while every point of height buys more minutes — with
ceilings high enough not to bind on an ordinary screen. `PLANNER_HOUR_HEIGHT` is the other half of
the trade: taller hours are easier to aim a 15-minute block at, shorter ones fit more day. At 48 the
snap step is still a dozen points and a full day is 1152, so roughly eighteen hours are visible at
once and opening on the working hour shows the rest of the day without a scroll.

### 16.6 State

The planner keeps no cached model: `planner_entries()` rebuilds from `active_things` every
frame, so it cannot drift out of sync with the calendar the way a second copy would. The only
persistent state is the flag, the day being shown, the selection (`planner_selection` plus
`planner_selected_session`, so the footer knows *which block* its per-block controls act on),
the in-flight gesture (`planner_drag`), the title being typed, the create kind, the quick-add
field, and the task under the due editor (`planner_due_edit`). `planner_flag` is listed in
`any_modal_open()`.

The body's controls read `active_things` and write it back through one setter each (`plan_item`,
`add_session`, `remove_session`, `unplan_item`, `set_item_duration`, `set_item_deadline`, …),
every one of which ends in `summarize_calendar` + `save_active_things` — so there is no path that
changes a task without the calendar and the task list agreeing about it a frame later.

### 16.7 What became of the day popup

Clicking a calendar day used to raise `calendar_day_popup`: the day's date and weekday, a scrolled
list of everything due on it in pill frames with hover complete/delete, and a bottom bar of
**Close · Plan day · Event+ · Task+**. It was a second window describing a day the planner already
drew better, and its only route to changing anything was to close itself and open the planner.

| Popup part | Where it went |
|------------|---------------|
| The day text | The planner's masthead, unchanged (§16.3). |
| The list of the day | The timeline. Everything the list held has a `Placement` on that day, so nothing is lost — and the timeline adds *when you will do it*, which the list could not show. |
| Per-row ✓ / ✗ | The footer, on the selection. |
| **Plan day** | Gone: the day click *is* the plan-day click. |
| **Event+** | Drag with **New: Event**. |
| **Task+** | The tray's quick-add, plus the footer's due editor when the task is owed on this day. |

Two things fell out of removing it. `DayCell` no longer carries `items: Vec<DayItem>` — a second,
fully-cloned copy of every dated item, rebuilt on every `summarize_calendar` for the popup to
read — only the `item_count` the cell layout actually dispatches on. And the open day is a
`NaiveDate` rather than the popup's `expanded_day` cell index, so a calendar rebuild underneath it
(a midnight rollover, a reduced week count) can no longer leave it pointing at a different day;
the popup needed an explicit bounds check and closed itself when that happened.

---

*See [`CODE_REVIEW.md`](CODE_REVIEW.md) for an analysis of problems, risks, and suggested improvements.*
