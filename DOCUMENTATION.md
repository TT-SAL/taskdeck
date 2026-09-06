| `phone_public_url` | string | `""` | the address to **hand out** when something in front of the server owns it — `tailscale serve`, a proxy, a real domain. Empty means build the links from this machine's own addresses. Taken verbatim, trailing slash trimmed; anything that is not an `http(s)` URL with a host falls back to empty. File only, read at start |
# TaskDeck — Technical Documentation

> A native desktop calendar / task-deck application written in Rust, rendered with
> `egui` on a `wgpu` backend. Displays a long vertically-scrolling calendar, a task
> priority list, a live weather forecast, a scratch notepad, and rich theming.

> Sections 2–19, §21 and §22 describe what the program **does**. §20 is the one exception: it is a
> design direction for the planner, marked as such, kept here so the reasoning behind it survives.

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

1. Table of contents
2. Overview & feature tour
3. Technology stack
4. Build, run, and the data directory
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
17. The archive
18. Routines
19. The keyboard
20. Planned ≠ scheduled — where the planner goes next *(Reflow built; the rest designed, §20.5)*
21. The phone view
22. The board, the server, and the desktop as a client
23. Subscribed calendars

---

## 2. Overview & Feature Tour

TaskDeck is a single-window desktop "wall calendar". The central panel is laid out
left-to-right as three regions:

| Region | Source | Description |
|--------|--------|-------------|
| **Left** | `show_tasks` | Scrollable list of deadline-less / prioritized **tasks**, sorted by an importance score. Hovering a card reveals ✓ (complete) and ✗ (delete) buttons. |
| **Center** | `show_calendar` | A virtualized, weeks-long calendar grid (7 columns). Each day cell shows up to 3 items with times. Rows animate (scale + fade) based on scroll velocity. Clicking a day opens the planner on it (§16). |
| **Right** | `show_weather_forecast` | A 2- or 3-day weather forecast (12 two-hour slots/day) with SVG icons, **or** a free-text notepad when 3-day mode is off. |

Top menu bar: **New Task**, **New Event**, **Planner**, **Archive**, **Settings**, **Quit**
(+ optional FPS readout). The **day planner** (§16) is a second view of a single day — a
timeline you block time out on, and a tray of everything still waiting for a slot.

Additional features:
- **Tasks, events and routines:** events are pinned to a date/time; tasks are ranked either by a deadline plus a **severity** (how bad is missing it) or, with no deadline, by a **horizon** (roughly how soon it should happen) that ripens over time; **routines** are weekly standing commitments — sleep, meals, the commute — that occupy the day without being ranked, completable or on the calendar (§18).
- **Archive:** completed *and* deleted items are kept whole — sessions, estimate, dates and all — in an append-only JSONL log, read once into memory and shown as a searchable ledger that says how each one went, with restore and a permanent forget. See §17.
- **Weather coordinate picker:** an interactive Blue-Marble world map with zoom/pan, click-to-pick, a graticule, ~270 city markers, and the nearest of them named for whatever you picked (§9.2).
- **Color schemes:** user-editable 6-color palettes used to tint calendar items; palettes can be **auto-generated from the current background image** via k-means clustering in CIE-Lab space.
- **Settings:** one sheet in four sections — appearance (background picture, its brightness, the colour scheme), window (UI scale, startup monitor, fullscreen, frame-rate readout), calendar (weeks shown), weather (coordinates, two or three day forecast). See §11.1.
- **Day planner:** clicking any calendar day opens it. Drag on the day's timeline to block out time, drag unplanned tasks in from the tray, move/resize blocks, and set a due time without leaving the day. Records *when you will do* something separately from *when it is due* — see §16.
- **Idle sleep:** when unfocused and idle for 10 s, the redraw loop stops to save power.

---

## 3. Technology Stack

| Concern | Crate(s) |
|---------|----------|
| GUI (immediate mode) | `egui` 0.35, `egui_extras` (SVG), `epaint`, `emath` |
| GPU rendering | `wgpu` 29, `egui-wgpu` |
| Windowing / event loop | `winit` 0.30, `egui-winit` |
| Async bootstrap | `pollster` (blocks on the async adapter/device setup) |
| Dates / times | `chrono` (with `serde`) |
| Serialization | `serde`, `serde_json` (tasks, schemes, notepad), `toml` + `toml_edit` (config) |
| HTTP client (weather; the desktop as a client of `taskdeck-server`, §22.3) | `reqwest` (blocking, `rustls-tls`; `default-features = false` keeps system OpenSSL out of the Linux build) |
| HTTP server (phone view, `taskdeck-server`) | `tiny_http` (blocking: two worker threads and one for the long poll — §21.2), `qrcode` (the link as a QR code in Settings; render features off) |
| Images | `image` (backgrounds, world map, icon) |
| Palette generation | `kmeans_colors`, `palette` (Lab/sRGB conversion) |
| Atomic file writes | `tempfile` (`NamedTempFile::persist`) |
| Allocator | `mimalloc` (set as `#[global_allocator]`) |
| Build | `embed-resource` (embeds `resources.rc` → `icon.ico`), `chrono` (stamps `BUILD_DATE`) |

> `rev_lines` was here, used to reverse-scan the archive log a page at a time. The archive
> is read whole and kept in memory now (§17.2), so nothing reads a file backwards any more
> and the dependency is gone.

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
per save). Both folders are created if missing. `AppDirs::locate()` resolves the same `<root>`
and creates nothing: `taskdeck-server --print-link` uses it, so a look at a service's link — often
taken as another user — cannot leave folders behind that the service then cannot write (§22.2).
The `<root>` is:

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
| `archived.jsonl` | newline-delimited `archive::Archived` | `archive::ArchiveLog` (append; atomic whole-file rewrite on restore/forget) |
| `colorschemes.json` | JSON map `u32 → ColorScheme` | `color::save_colorschemes` (atomic) |
| `notepad_text.json` | JSON string | `utilities::save_notepad_text` (atomic) |
| `userconfig.toml` | TOML | `initialization` + `toml_edit` writers |
| `.lock` | empty; the OS lock on it is the content | `paths::claim_data_dir` (§4.1) |
| `outbox.json` | JSON array of `sync::Queued` — each a `command`, the temporary `local_id` it created if any, and the request `key` it is sent under | `sync::Outbox` (atomic) — only when the desktop is a client of a server (§22.4) |
| `.client-of` | the server's URL, plain text | `sync::mark_replica_of` — marks the folder as that server's replica, so a later start refreshes rather than sets aside (§22.3) |
| `subscriptions.json` | JSON array of `subscriptions::Subscription` | `subscriptions::save` (atomic) — the calendars this board subscribes to (§23). Board data: it changes only through `Board::apply` |
| `subscribed_cache.json` | JSON `subscriptions::Overlay` | `subscriptions::save_overlay` (atomic) — **derived**, not board data: what those calendars last said, kept only so a restart is not blank until the first fetch returns (§23) |

### 4.1 Atomicity is not exclusion

Every save here is atomic: serialize, write a temp file beside the real one, `fsync` it, rename over
the top. That makes a save **crash-safe** — what is on disk is always either the whole previous
version or the whole new one, never half of either — and it is worth being clear about what that
does *not* cover.

**A second copy of the program.** Two instances each read the task list at startup, keep their own
picture of it, and write **the whole picture** on every change. Both writes are individually
perfect; the second simply replaces the first, and everything the other instance did since it
started is gone. No amount of atomicity helps, because nothing was ever torn — two programs
disagreed about the truth and the later one won. (`archived.jsonl` is the one exception: appends are
atomic, so two instances *filing* things is safe. Restoring or forgetting rewrites the whole log,
and that clobbers like everything else.)

`paths::claim_data_dir` takes an OS lock on `taskdeck_data/.lock` and holds it for the process
lifetime. An OS lock rather than a PID in a file, because the kernel releases it however the process
dies — there is no stale lock to reason about and no liveness check to get wrong.

A second instance is **warned, not stopped**, through the same startup-error window that reports a
quarantined file. Refusing to start is the stricter answer and the wrong one here: a false positive
means "cannot open my own calendar", which is worse than what it prevents, and some filesystems —
network shares especially — do not lock faithfully. A filesystem that will not lock at all is
treated as no guard rather than as a conflict. (`taskdeck-server` *does* refuse: a server has no
"my own calendar" excuse, §22.2.)

**This rule is what the phone and the server are built on.** Rather than a second copy of the
data to reconcile, there is one process writing each folder and everyone else talks to it: the
phone is a thin client of the running process (§21.1), `taskdeck-server` is that process on a box
with no window, and a desktop pointed at a server keeps a *replica* in its own folder — one
writer there too — with `outbox.json` and the `.client-of` marker beside it (the table above,
§22.1, §22.4). Nothing in this section had to change for any of that; it is the reason the rest
could be simple.

**Durability of the rename.** The rename is atomic but, on a journalling filesystem, the *directory
entry* can still be in the page cache when the power goes: contents fsynced, swap atomic, save lost.
`tasks::sync_directory` flushes the parent directory after each `persist`, which is the step that
makes the guarantee whole — and every writer of the folder takes it: the active set, the archive
rewrite, the notepad, the outbox, the colour schemes and the settings file (§6). Best-effort and
Unix-only — Windows
will not open a directory as a file, and NTFS orders the metadata once the file's own data is down.

---

## 5. Runtime Architecture

### 5.1 Startup (`main.rs`)

```
main → pollster::block_on(run())
run():
  1. EventLoop::new(); create an EventLoopProxy (§5.6)
  1b. paths::AppDirs::resolve()  → data/images dirs (created if missing), see §4;
      paths::claim_data_dir()    → the .lock of §4.1 (a second copy is warned)
  2. get_check_and_set_config(&dirs.config_file()) → Config (reads + normalizes userconfig.toml);
      an empty phone_token is minted and written back (§21.3)
  2b. the phone view's request channel (its server thread sends, the UI thread serves, §21.2),
      then the `Wake` closure around the proxy that the phone server and the sync engine are given
  3. the Board (§22.1) — items, archive, notes, id counter:
       server_url empty  → Board::open(data)   (corrupt file → quarantine + empty set, see below;
                            a folder that was a replica is made its own again, §22.3)
       server_url set    → sync::Remote::board() → Board::from_parts(...) kept as a replica,
                            the local board set aside on a first contact; unreachable → the
                            cache, or the local board on a failed first contact (§22.3);
                            sync::start(...) spawns the sender and the listener
  4. dirs.background_options()   → names in images/
  5. color::read_colorschemes()  → HashMap<u32, ColorScheme> (inserts default if empty;
                                    corrupt file → quarantine + default scheme)
  6. get_weather(coords, proxy)  → spawns the background weather thread, returns WeatherService
  7. build TaskAppConfig (board, sync handle, phone channel, …) → TaskApp::new(...)
  8. task_app.summarize_calendar()   (initial calendar build / sort)
  9. App::new(task_app, ...) → event_loop.run_app(&mut app)
```

**Corrupt-file recovery.** Steps 3 and 5 must not abort the boot. If `read_at_startup.json` or
`colorschemes.json` is unreadable or fails to parse, `tasks::quarantine_corrupt_file` renames the bad
file aside (`<name>.corrupt-<timestamp>`, preserved for manual recovery) and startup continues from an
empty active set / the default colour scheme. One failure is deliberately *not* quarantined: a file
that exists but cannot be read for want of permission is left where it is and named, since renaming
it away would start an empty board over a calendar that is merely owned by someone else
(`taskdeck-server` refuses to start over such a file, §22.2). The recovery message(s) are passed to
`TaskApp` via `TaskAppConfig::startup_error` and shown in the existing error window once the UI is
up. The notepad load already degrades gracefully via `unwrap_or`.

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

Every background thread wakes the UI through the same `EventLoopProxy<()>` (`proxy.send_event(())`):
the phone server and the sync engine through one `Wake` closure around it, built once in `main`; the
weather thread, which predates the closure and needs nothing else from it, through a clone of the
proxy itself. `App::user_event` then serves
the phone view's request queue and drains the sync engine's events (`TaskApp::serve_phone_requests`,
§21.2) before it calls `window.request_redraw()`, so an edit from a phone or a board from the server
lands even while the window is minimized or asleep, and the next frame draws it. The threads:

| Thread | Module | Wakes the UI when |
|--------|--------|-------------------|
| weather | `weather.rs` | a forecast fetch succeeded |
| phone workers (two) | `phone.rs` | a request has been queued for the UI thread to answer (§21.2) |
| phone pulse | `phone.rs` | never — it answers parked long polls itself, on the UI thread's `publish` |
| sync sender | `sync.rs` | a temporary id was mapped to a real one, a command was refused, touch changed (§22.3) |
| sync listener | `sync.rs` | the server's board arrived (§22.3) |

All plain blocking threads, in the house style (§3.4 of `MOBILE.md` records why: no async
runtime). `taskdeck-server` has the same phone threads and no window, so its `Wake` does
nothing and its main thread simply blocks on the queue (§22.2).

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
    recurrence: Option<Recurrence>,          // a routine's weekly rule (below, §18); None otherwise
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
handed out by the `Board` (`Board::next_item_id`, §22.1). `id == 0` is an "unassigned" sentinel:
items loaded from a pre-id or hand-edited save (the field is `#[serde(default)]`) are backfilled
at startup by `tasks::assign_missing_ids`, which preserves any existing ids and seeds the counter
past the current maximum. An id that appears twice — two computers' files pasted together — stays
with its first holder and the second is renumbered the same way, since two items under one id
would send every command for one to the other. New ids persist on the next save. Ids from 2⁶² up
are **temporary**: a desktop that is a client of a server numbers its own creations there until
the server answers with the real id (§22.4), and a board that becomes its own again renumbers any
it still holds (`Board::adopt_temporaries`).

Three valid shapes:
| Kind | `is_event` | `importance` (severity) | `time_importance` (horizon) | `deadline` |
|------|-----------|--------------|-------------------|------------|
| Event | true | None | None | **Some** |
| Dated task | false | Some | kept but dormant | **Some** |
| Undated task | false | kept but dormant | Some | None |

"Kept but dormant": the footer's due editor moves tasks between the two shapes at will, so both
knobs may be present on one task — whichever matches the deadline's presence is live (§7.3), and
the other is remembered for the day the deadline is added or cleared again.

### `Recurrence` (`tasks.rs`) — a standing weekly claim

`{ days: u8 (bit 0 = Monday), start_minutes: i32, minutes: u32 }`, held in
`Active::recurrence`. A **rule**, not a list of instances: one record generates a block for whatever
day is being looked at, so nothing accumulates and there is no per-occurrence editing question. An
item carrying one is a **routine** — the third kind, neither ranked nor completable nor on the
calendar grid. Full rationale in §18.

### `Archived` (`archive.rs`) — an item that has left the board

**Every** field of `Active` that carries meaning, plus `archived_at: DateTime<Local>` and an
`outcome: Outcome` (`Finished` / `Dropped`). It is a faithful copy, not a summary, which is what
makes the verdict line, the summary figures, the planner's ghosts and `to_active()` (restore) all
possible from the one record. `Archived::retire(item, outcome, at)` takes the `Active` **by value** —
retiring ends its life as a live item — and `to_active()` is its exact inverse. Full rationale in
§17.

> **Superseded `InActive` (`tasks.rs`).** That record was the item minus `time_importance`,
> `sessions` and `duration_minutes`, with no outcome field. Three consequences, all now fixed:
> a restored task would have come back as a bare name the scorer reads as `MALFORMED_SCORE`
> (§7.3); the deadline-versus-finish and estimate-versus-booked comparisons were impossible to
> make; and *deleting* wrote nothing at all, so the README's claim that deleted items are kept
> was untrue. Rows in that shape still load — see the wire-compatibility note in §17.1.

### `Command` (`board.rs`) — the one way anything changes

Every change to the data is a `Command` carried out by `Board::apply` (§22.1): a serialisable
enum tagged by `op` in `snake_case` on the wire — `{"op":"move_block","id":17,"session":1,
"day":"2026-09-04","start":870,"minutes":60}` — with times as the phone's own inputs produce them
(§21.3), and a few fields a client fills for the server: `at` on `complete` and `delete`, `from` on
`reflow` (§22.4). The request that carries it may name itself with an `X-TaskDeck-Request` key,
which is what makes a retry a repeat rather than a second application (§22.4).

### Persistence functions

The board's own files below belong to one `Board` per folder (§4.1, §22.1) and nothing else writes
them; `userconfig.toml` is the odd one out, written by `main`, the settings sheet and the server
binary rather than by the board. Each is replaced whole — a temporary file, `fsync`, a rename, and
then the directory flushed (§4.1) — so a power cut at any instant leaves the previous version or
the new one, never half of either. The single exception is the archive's ordinary write, which
appends one line and fsyncs it; the log is rewritten whole only when a row is taken out of it.

- `read_at_startup` / `oversafe_activesave` (`tasks.rs`) — load/save the active set. Saving is
  **atomic**: serialize → write to a temp file in the same dir → `fsync` → `persist` (rename).
- `archive::ArchiveLog` owns `archived.jsonl` entirely: `load` (once, whole), `record` (one
  appended line, placed by its time), `take` (remove one row + atomic whole-file rewrite). §17.2.
- `utilities::save_notepad_text` — the notepad, atomically, through `apply(SetNotes)` (§21.5).
- `sync::Outbox` — `outbox.json`, a JSON array of `Queued { command, local_id, key }`, rewritten
  atomically after every change while the desktop is a client (§22.4).
- `initialization::write_normalized_config` / `write_config_value` — `userconfig.toml`, also
  through a temporary file, since it holds the phone view's key (§11).

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

1. Partition the board's items (`board.items`, §22.1) into events and tasks; sort events by deadline, tasks by score.
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
   the board's items directly, so only the count is still owed.
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
  Within the cell: the hour top-left, the temperature under it against the right edge, and the sky
  centred on the floor of the cell with the two readings laid over its top corners — that overlap
  is the arrangement, not an accident of it. `WEATHER_GRID_WIDTH` is derived from the cell size and
  is what the notepad below measures itself against.
- **The column's vertical gaps were hiding inside the old cell.** `WEATHER_LABEL_GAP` (weekday to
  its grid) and `WEATHER_DAY_GAP` (grid to the next weekday) were both a single `add_space(75.0)`,
  and looked like nothing of the sort on screen: the old cell was a `Frame` laid out **bottom-up**,
  which anchors its content to the bottom of an available rect that the (overflowing) column had
  already exhausted, so each grid was painted the better part of a cell-height *above* where the
  layout had placed it and swallowed most of the space above it. The 75s were tuned against that.
  Painting each cell honestly at the rect it is allocated left all of it showing at once and pushed
  every grid far down the column — so the gaps are now the numbers the old arrangement actually
  measured. The weekday itself is centred on `WEATHER_GRID_WIDTH` rather than pushed into place by
  a per-call-site `add_space`; there had been two different guesses at the same position.
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
  Requires ≥500 usable pixels, else returns `None`. The manager's *Generate from the background*
  button is disabled while the images folder is empty, and a `None` is reported in the error
  window rather than swallowed.
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
fails; even a line that cannot be read, such as one holding an unclosed quote, is skipped),
clamps/validates each field (`nan` and `inf` do not count as numbers for the float pairs), then
writes the normalized
values back to disk via `write_normalized_config`. That writer uses `toml_edit`, so it **preserves
existing comments, key order, and unknown keys** and writes each value with its real TOML type
(integers/float-arrays, not strings). A missing or unparseable file falls back to a fresh document
(same self-heal as before).

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
| `phone_server_enabled` | bool | `false` | serve the phone view (§21) while the app runs |
| `phone_server_port` | u16 | `7373` | `phone::PORT_MIN` (1024) or above; anything else falls back to the default |
| `phone_bind_address` | string | `"0.0.0.0"` | an IP address (trimmed) to listen on alone; anything that is not one falls back to every interface (§21.7). File only — not on the settings sheet — and read at start: a change takes effect at the next start |
| `phone_token` | string | `""` → minted | the key in the phone's link; `main` mints one on the first start and keeps it. **A credential**: anyone holding the link can edit the calendar |
| `frame_cap_fps` | u32 | `0` (uncapped) | `0` = the uncapped loop of §14.1, else clamped `FRAME_CAP_MIN..=MAX` (`15..=360`) |
| `server_url` | string | `""` | a `taskdeck-server` to keep the board on, `http://host:port`; empty means the board lives here (§22) |
| `server_token` | string | `""` | that server's `phone_token`. **A credential** |

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

Six named sections — **APPEARANCE**, **WINDOW**, **CALENDAR**, **WEATHER**, **PHONE** (§21.7),
**SERVER** (§22.3) — each a two-column
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
visual language: a rounded "notch" around the day number, wrapped item text, and small "hour mark"
pills drawn with an **unclipped painter** so they can spill outside the cell.

| Widget | Used when a day has… | Notable detail |
|--------|----------------------|----------------|
| `DayNumber` | 0 items | Just the day number (top-left). |
| `DayHeader` | the 1st item | Number + title + top hour-mark; custom rounded top-right polygon. |
| `MiddleHeader` | the 2nd item | Plain rounded rect; optional bottom hour-mark. |
| `RotatedNumberOnly` | filler for 0–2 item days | Day number rotated 180° in the bottom-right. |
| `BottomHeaderRotated` | the 3rd item (exactly 3) | Rotated number + title + top & bottom hour-marks. |
| `ButtonHeaderRotated` | the 3rd slot (4+ items) | Same as above plus a "…" overflow button. |

> These widgets are pixel-tuned with many magic offsets; they assume the ~160×215 cell size.

### 12.1 Fitting a name onto a card — `fit_text_rows`

A card is about thirteen characters to a row and a name is any length, so something has to give.
Worse, a card is **not a rectangle**: the day number is punched out of one corner, so the row beside
it is narrower than the rows below.

All four card widgets used to solve this separately, and every copy was wrong:

| Fault | Consequence |
|-------|-------------|
| Only the *first* row was measured; the remainder was poured into the second unchecked | A long name ran off the side of the card and was clipped mid-glyph |
| Anything past two rows was dropped | A truncated name looked like the whole name — no ellipsis, no sign it continued |
| A word wider than a row never fit row one | Row one was left **empty** and the whole word went to row two, where it overflowed |
| Two copies had no "row one is full" flag | Later *shorter* words were still appended to row one — the name printed **out of order** |
| One of those appended with `push_str(word)`, no separator | The rest printed **run together**: `overdueprojectretro` |

`fit_text_rows(ui, text, font, widths)` replaces all four. `widths` carries **one entry per row**, in
order, which is what lets the indented first row and the full-width rows below it each be measured
against their own space. It breaks at spaces where it can, inside a word where it must (a name with
no space in it still has to be cut somewhere), and ends in `…` when anything was left over — paid
for out of the row's own width, not hung past its end.

Two details worth keeping:

- **The row *count* is measured too** (`rows_between`), not assumed. How many rows a card has room
  for depends on the height of the day number's galley, which depends on the font and the UI scale.
  A first attempt hardcoded three and the last row straddled the bottom edge of the card, painted in
  half. The 16-point Anton numeral's galley is also a few points taller than its glyphs, and giving
  that leading back is what buys `DayHeader` its third row.
- **The layout is a pure function of a per-character advance** (`fit_rows_by`), so the rules are
  tested against a known ten-unit character rather than against whatever the font measures. Summing
  advances is how egui's own layouter measures a row, so this agrees with what is painted.

It is also cheaper than what it replaces, which laid out the whole accumulated line into a fresh
`Galley` once per word per card. This takes the font lock once and costs one pass over the name.

### 12.2 The hover tip

A name that ends in `…` still has to be readable somehow. Hovering a cell names **every** item on
that day, in time order, with their times.

It is built from the board's items on hover, not from the cell's three-item `preview`. Reading the
preview would have the tip answer its own question with "the same three, again" — and storing the
full list on the `DayCell` instead is exactly what the day popup used to do (a second, fully-cloned
copy of every dated item, rebuilt on every `summarize_calendar`), which was removed for good reason.
One scan of one hovered day costs nothing. `CELL_TIP_MAX_ROWS` is a ceiling so a pathological day
cannot produce a tip taller than the screen, not a space constraint; past it the rest are counted.

The tip's hit area is registered **after** the cards are drawn — egui hit-tests the most recently
added widget first — and senses only hover, so the day click still belongs to the calendar's own
press/drag handling.

---

## 13. Glossary of `TaskApp` State Flags

| Flag | Meaning |
|------|---------|
| `new_task_flag` / `new_event_flag` | Show the create-task / create-event modal. |
| `error_flag` + `error_text` | Show the (top-most) error modal. |
| `archive_view` | **Not a flag** — a struct (`ArchiveView`) holding the archive window's whole state: `open`, the filter, the selected row, the pending forget confirmation, the one-frame search focus. See the D6 note below. |
| `archive` | The `ArchiveLog` itself: the session's copy of `archived.jsonl`, loaded lazily and kept (§17.2). |
| `planner_ghosts` | The archived blocks that fall on `planner_day`, cached per day rather than rebuilt per frame (§17.4). |
| `planner_flag` + `planner_day` | Show the day planner, and which day it is on. The day is a date, not a calendar cell index, so the planner can step past the end of the calendar's range and a rebuild underneath it can't repoint it. |
| `planner_drag` / `planner_selection` / `planner_naming` | In-flight timeline gesture, selected item, and the item whose title is being typed. |
| `planner_naming_created` / `planner_naming_focus` | Whether the item under the title editor was created by the gesture that opened it (Escape then removes it), and the one-frame request for keyboard focus (§16.3.5). |
| `planner_selected_session` | Which of the selection's sessions was clicked, when a work block was — what the footer's per-block controls act on. |
| `planner_due_edit` | Task whose deadline is open in the footer's due editor (§16.3.4). |
| `planner_create_kind` | What a drag or double-click on empty timeline makes: `Task`, `Event` or `Routine` — the `1` / `2` / `3` keys (§16.3, §18). |
| `dialog_wants_focus` | One-frame request to put the caret in a create dialog's name field, so `T` / `E` open something you can type into straight away (§19). |
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
| `board` | **Not a flag** — the `Board` (§22.1): the live items, the archive log and the notepad, with `apply(Command)` as the one way to change them. Everything below that names an item names one of its. |
| `sync` | **Not a flag** — `Some(SyncHandle)` when this copy is a client of a `taskdeck-server` (§22.3): the queue for commands, the events to drain, the status the menu bar shows. `None` when the board lives here. |
| `phone_enabled` / `phone_port` / `phone_bind` / `phone_token` | The phone view's setting, port, bind address and key (§21.7) — the first three from `userconfig.toml`, the key minted once by `main`. |
| `phone_server` / `phone_error` / `phone_retry_until` | The phone view's server (§21.2): running, why not, and until when a restart keeps retrying the bind. `phone_pulse` is the change counter the page watches, published from `phone_changed`. |
| `phone_addresses` / `phone_addresses_checked` | The addresses the settings sheet shows links for (`phone::addresses_for`), re-asked of the kernel every `PHONE_ADDRESS_TTL` rather than every frame. |
| `phone_port_input` / `server_url_input` / `server_token_input` / `frame_cap_input` | The settings sheet's editable copies of a setting, applied — and written to the file — when the field settles, not per keystroke. The server fields take effect at the next start (§22.3). |
| `frame_cap_fps` | The optional frame cap (§14.1); `FRAME_CAP_UNCAPPED` by default. |

When any modal flag is set, `hovered_calendar_cell` is cleared at the end of `ui()` so the
calendar doesn't show a hover state behind a modal. `any_modal_open` is `planner_flag ||
modal_over_planner()`; the second half is the same list without the planner, and is what the
planner's own keyboard shortcuts stand down for (§16.3).

> **On the flag count (`CODE_REVIEW.md` D6).** The archive redesign was the sequencing point that
> review named, and it takes the first step: the archive owns `ArchiveView` rather than adding
> `display_archive_flag` + `archive` + `offset` + a forget-confirmation boolean to the pile. Net
> change is three loose fields removed and none added. It is not the modal *stack* D6 ultimately
> asks for — the remaining screens still keep their booleans — but it is the shape the rest should
> move to: a screen owns its own state, and `any_modal_open` remains the single place that knows
> the full set.

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

**The optional cap does not change any of this.** `frame_cap_fps` (§11; Settings → Window → Frame
rate) is `0` — uncapped — by default, and uncapped is the design. Set, it paces only the
self-chasing loop: `App::schedule_next_frame` defers the `request_redraw` that follows a frame until
the frame's share of a second has passed (`ControlFlow::WaitUntil`, answered in `new_events`).
Frames still arrive continuously, only slower, so the `dt`-driven animations are untouched, and
input still draws immediately — the `CursorMoved` and friends arms call `handle_redraw` themselves
and are not gated. It exists for the one case §14.1's reasoning does not cover: a laptop on
battery, where a calendar drawn two hundred times a second is a fan.

### 14.2 Single-file `ui.rs` / large `TaskApp`

`TaskApp` holds a great many fields and `ui()` is one very long method in a `ui.rs` of close to
eight thousand lines. This is deliberate: on a solo project, keeping the whole *window* in one file
makes it easier to hold the entire thing in your head. **Splitting `ui.rs` into submodules is not
wanted.** What has its own module is what has no window in it: the board (`board.rs`), the phone
view (`phone.rs`), the client engine (`sync.rs`) and the headless server (`server_main.rs`, §21–22)
— one of them ships as a binary with no window at all, which is the test of the split.

The one self-contained refinement that would still be welcome (without splitting the file) is
replacing the many parallel `*_flag` booleans with a modal **stack** — not a flat enum, since the
modals deliberately nest (`CODE_REVIEW.md` D6) — so that "two modals open at once" is expressible
only where it is meant. That is tracked there as an open item; the file structure itself is not.

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
**intentional**: it gives the task list a gentle shuffle from day to day rather than a frozen
order, and is not a bug to remove.

The score is evaluated **once per task per rebuild** and stored, so the shuffle is captured a
single time and the comparator stays consistent within a sort (see §7 and `CODE_REVIEW.md` B3).

`tie_break_jitter` hashes the task's **id** with a seed, giving each task its own factor in
`[1.0, 1.08)`. The magnitude is deliberately below one importance step (a factor of two), so it can
shuffle near-ties without ever reordering tasks that genuinely differ in priority — which is
asserted by a test.

**The seed is the day** (`TaskApp::shuffle_seed`, derived from `self.date`), and it took two wrong
answers to get there:

| Seed | What went wrong |
|------|-----------------|
| the current millisecond | Every task scored in the same millisecond got the *same* factor, which changes no ordering at all; when a rebuild straddled a boundary, an arbitrary subset jumped by up to 8%. Effect: "nothing, occasionally something arbitrary". |
| a rebuild counter | `summarize_calendar` runs after **every mutation**, so booking a block, dragging a card or nudging a deadline reshuffled the whole task list beside the planner while the user worked. |

Both answered "how often should this move?" with "whenever something happens" rather than by asking
what the jitter is *for*. What it is for is keeping a task that is perpetually fourth from being
permanently ignored — a fairness argument, whose natural period is a day. So the list is fixed for
as long as anyone is looking at it and turns over at midnight, which is also exactly what a calendar
on a wall does.

Deriving it from the date rather than storing it also removes the question of where to bump it:
there is no seed state, and `self.date` is refreshed every frame with a day-rollover check that
already calls `summarize_calendar`.

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
button — or `P` — which opens it on the day it was last left (§19.3). `planner.rs` holds the model and geometry; `ui.rs` draws it.

It is the *only* per-day view. A read-only popup used to open on a day click and hand over to
the planner with a **Plan day** button; §16.7 records why that went and where each of its parts
landed.

### 16.1 The central idea: due ≠ planned

A calendar answers *when is this due*. A planner answers *when will I do it*. Those are
different facts about the same task — a report due Friday can be written on Tuesday morning —
and conflating them is what makes most task apps annoying to plan with.

> The same argument has a next line — *planned ≠ scheduled* — which this planner does not yet
> draw. §20 is what that would mean and why it is the fix for a day plan that falls apart when
> one thing runs long.

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
shared by create, move, and resize so all three agree on what a legal block is. The board
runs a client's `create` and `move_block` through the same clamp, and `snap` clamps while the value
is still a float, so a start or length a body carries as `i32::MIN` or `u32::MAX` becomes a
whole-day block at midnight rather than an integer overflow.

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
events (`refilter_tasks`), so the footer was the only place this was reachable. Rows already in the
log from before that fix are handled at the reading end too — see `was_finished()` in §17.1.

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

The planner keeps no cached model: `planner_entries()` rebuilds from the board's items every
frame, so it cannot drift out of sync with the calendar the way a second copy would. The only
persistent state is the flag, the day being shown, the selection (`planner_selection` plus
`planner_selected_session`, so the footer knows *which block* its per-block controls act on),
the in-flight gesture (`planner_drag`), the title being typed, the create kind, the quick-add
field, and the task under the due editor (`planner_due_edit`). `planner_flag` is listed in
`any_modal_open()`.

The body's controls read the board's items and write them back through one setter each
(`plan_item`, `add_session`, `remove_session`, `unplan_item`, `set_item_duration`,
`set_item_deadline`, …), every one of which is a one-line wrapper that builds a `Command` for
`Board::apply` (§22.1): the board validates and saves, and `TaskApp::apply` then rebuilds the
calendar and the task list — so there is no path that changes a task without the calendar and the
task list agreeing about it a frame later, and none that a phone or a server cannot take too.

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

### 16.8 Reflow — when the day runs late

The first move of §20.3, built as that section asked: **a button, not a model change.**

A plan made of clock times is over-specified (§20.1): "an hour on the report today" is known,
"that hour is 14:00" almost never is, and it is the second claim that breaks and takes everything
below it with it. Until sessions can float, the cheap answer is to notice the day running late and
offer one verb.

- **The figure.** `planner::behind_minutes` sums, over today's *work* blocks, the part of each that
  lies before the now-line — `1h 20m behind` in the masthead, in the same amber the phone view uses
  for today. Only task sessions count: an event that has passed *happened*, and a routine that has
  passed was never owed. Zero on any day but today, which has no now, and the figure and its button
  are simply absent then.
- **The verb.** **Reflow** (`R`) calls `planner::reflow`, which slides the day's task sessions down
  past now **in their existing order**, around the *anchors* — event and routine blocks, the spans
  that are genuinely fixed (§20.2). A block still ahead moves only when one landing in front of it pushes it on — nothing is
  ever pulled earlier, so a day with nothing behind is left exactly alone, which is exactly the cascade "my schedule exploded"
  describes. Blocks keep their length; this is a slide, not a re-plan.
- **Nothing is dropped.** A day that runs out of room has its last blocks clamped inside it and left
  overlapping — an over-booked day should *look* over-booked (§20.4), not be quietly tidied.
- **It is one command.** `Command::Reflow`, applied by the board like every other edit, rewrites
  `Session::start` for the blocks that changed and saves; the phone view offers the same button,
  the desktop planner shows the result a frame later, and a desktop that is a client stamps the
  command with its own `from` so the server slides the day as it slid here (§22.4).

`reflow` and `behind_minutes` are pure and tested: order preserved, anchors stepped over (including
one that begins inside the anchor just stepped past), now rounded up to the snap grid, the
end-of-day clamp, empty anchors ignored.

Living with this is the experiment §20.3 asks for before committing to floating sessions: if the
button is pressed every afternoon, the flow model is what is wanted; if it is never pressed, the
data change was not worth making.

---

## 17. The Archive

`archive.rs` (model + store, pure and unit-tested), plus the window and the ghosts in `ui.rs`.

### 17.1 The central idea: the record is the item

The app's one real claim is that **due is not the same as planned** (§16.1): a deadline says when
something is *owed*, `sessions` say when you will *sit down and do it*, and the two are stored
separately because they are genuinely different facts.

Those two facts only ever become *checkable against each other* at one moment — the moment the
thing is finished. That was precisely the moment the old archive discarded them. `InActive` kept a
name, a created date, a deadline and a timestamp; the sessions, the estimate and the horizon went
on the floor. So the app recorded the most interesting thing it knows — what you intended — and
deleted it at the exact instant it became possible to check.

`Archived` keeps the whole item, plus one fact the old record could not express: **how it left**.

```rust
pub enum Outcome { Finished, Dropped }
```

Three things follow, and they are the whole reason for the redesign:

| Follows from | What it buys |
|--------------|--------------|
| deadline + `archived_at` | **The verdict.** Was the date met, and by how much. |
| `duration_minutes` + `sessions` | The other half of it: was the *estimate* any good. |
| nothing is lost | **Restore.** `to_active()` is the exact inverse of `retire()`. |
| sessions survive | **Ghosts.** The planner can draw what a day was actually spent on (§17.4). |

**Deleting archives too.** Completing and deleting used to differ in kind — one wrote a row, the
other simply dropped the item. A task you gave up on left no trace it had existed, and the README's
"completed and deleted items are not thrown away" was false. They are one act with different
outcomes now (`Command::Complete` and `Command::Delete`, both through `Board::retire`), and the
ledger is what tells them apart.

The one path that still removes without recording is `forget_active_thing`, used for Escape on a
just-created, not-yet-named block (§16.3.5). It is seconds old, the only thing in it is the slot an
accidental drag gave it, and *"untitled, dropped a moment after it was written down"* is noise, not
history.

**An event is never finished.** It is a time that arrives and passes on its own, which is why the
planner offers no ✓ on one (§16.3) — an event reaches the archive only by being deleted. Rows
written before `outcome` existed all default to `Finished`, events among them, so `was_finished()`
lets the *kind* decide first and the stored outcome only break the tie. Without that, a real log
read back with a dentist appointment wearing a ✓ next to it, and the "N finished" figure counted
appointments as work done.

**Wire compatibility.** Old lines still load. `outcome` defaults to `Finished` — correct, because
only the complete path ever wrote one — and the missing fields default to empty. The timestamp
keeps its old JSON name:

```rust
#[serde(rename = "inactivated")]
pub archived_at: DateTime<Local>,
```

Renaming it on disk would produce a log the previous build cannot read at all, which is not a trade
worth making for a nicer field name. A test asserts the wire name, so nobody "tidies" it later.

### 17.2 The store: read once, keep it

One append-only JSONL file, read **whole** into memory the first time something asks to see it, and
kept for the session.

What that replaces (`CODE_REVIEW.md` B4): `read_lines_range(offset, limit)` re-opened
`archived.jsonl` and reverse-scanned past `offset` lines on every "Show more" — O(n) a page, O(n²)
to walk the log — and it counted *raw* lines while rendering *parsed* ones, so a single unreadable
line silently skipped or duplicated rows across a page boundary. Both are gone rather than patched,
because line-offset paging is gone.

Reading it whole is not a concession, it is the enabling move: you cannot compute *"29 of 34
deadlines met"*, or search by name, or group by month, from a fifteen-row window onto a file.

| Operation | Cost | How often |
|-----------|------|-----------|
| `load` | O(n), once per session | first time anything opens the archive or the planner |
| `record` | one appended line + `fsync` | every completion / deletion |
| `take` (restore, forget) | O(n) whole-file atomic rewrite | only when the user deliberately asks |

The rewrite uses the same discipline as `oversafe_activesave`: temp file in the same directory →
flush → `fsync` → `persist` (rename). If it fails, the removed row is put **back** into memory, so
the window and the file cannot disagree.

`record` mirrors into memory only when the log is already loaded — pushing onto an unloaded log
would have the next `load` read the same row off disk and hold it twice. Entries are sorted by
`archived_at` descending rather than merely reversed: appends happen in time order, but a
hand-edited or hand-merged log has no such guarantee, and the month runs depend on the order being
real.

**Unreadable lines are kept verbatim** and written back out on a rewrite, at the head of the file.
Failing to parse a line is not grounds for deleting it. Nor is failing to read it as text: a
line that is not UTF-8 is set aside as the bytes it is, so one stray byte does not close the
ledger. The count is shown in the window rather than swallowed — a number that is not zero is a
fact about the user's own data.

### 17.3 The window: a ledger with a footer

Built like the planner rather than like the fixed 500×800, hardcoded-Dracula, three-column
`Grid` + "Show more" it replaces: sized from the viewport, a masthead that says what the rows add
up to, a body you read, a footer that acts on the selection.

- **Masthead** — `Summary::headline()` over the *filtered* rows: `41 finished · 6 dropped · 3
  events removed · 29 of 34 deadlines met · 52h booked · typically 4d on the board`. Only
  **finished, dated tasks** are judged: a dropped task withdrew rather than failed, and an event
  never made a promise. The median, not the mean, so one task forgotten for two years doesn't
  become the headline figure for how you work.
- **Filters** — a name search, `Everything / Finished / Dropped`, `Both / Tasks / Events`. The
  outcome pair keys on `was_finished()`, so between them they still cover every row (an event falls
  under "dropped": it was taken off the calendar).
- **Ledger** — rows under month headings, newest first, each with a date spine, the ✓/✗ mark in the
  item's own palette colour, the name, and `Archived::verdict()`.
- **Footer** — reserved whether or not anything is selected, so selecting doesn't shift the ledger
  under the pointer. Carries the full timestamps the rows deliberately don't spell out, plus **↩
  Put it back** and **Forget**.

**Only Forget asks.** Restoring is undone by ticking the thing off again, one button away.
Forgetting is the single irreversible act in the app, so it is the single one with a confirmation.

**Escape in two steps**: clears the filter if the filter is doing anything, closes the window
otherwise — a search you are half way through is not something the window should close over. It
stands down for the forget confirmation (captured *before* the body runs, for the reason §16.3
gives) and for the error window, which answers Escape itself.

`archive_window` is a **free function** over `(&ArchiveLog, &mut ArchiveView)`, not a method. The
window reads hundreds of rows *and* offers buttons that mutate the log, which cannot both hold
`&mut self`. Borrowing the two fields separately means the rows are borrowed rather than cloned
every frame, and it forces the buttons to return an `ArchiveAction` instead of acting mid-draw — so
every change to the archive happens in one place, after the frame is laid out.

**Restore and the id.** The id comes back with the record, but it may not be free: the counter is
seeded past the highest id in the *live* set, so archiving the highest-numbered item and restarting
leaves the counter behind it, and a legacy row carries the `0` sentinel. `Board::apply(Restore)`
pushes the counter past whatever came back and hands out a fresh id if the old one is taken, is
`0`, or lies in the clients' temporary range (§22.4).

### 17.4 Ghosts: the archive inside the calendar

Open a day in the planner and the blocks of archived items that fall on it are drawn behind
whatever is still live — outlined over a dark wash rather than filled, with a ✓ or ✗ and the name.

This is what the lossless record buys the rest of the app. A day used to show only what was still
coming: tick the morning's work off and the morning went blank, as though it had never been spent.
The masthead's day summary gains a `· 2h done` alongside `planned`, counted separately — folding it
into "planned" would make a finished day look like a day with everything still ahead of it.

Three deliberate limits:

- **Blocks only, never due markers.** A ghost answers *"what did this day go on"*, and the deadline
  of something already dealt with is not part of that answer. Events therefore never appear at all:
  they have no sessions, and a deleted appointment is not something a day was spent on.
- **Not interactive.** A ghost is a record, and drawing it as a solid grabbable block would be a
  lie the pointer immediately exposes. It is kept out of the entry list the gesture handling reads,
  so `handle_planner_gestures` is untouched by any of this.
- **In the lane packing all the same.** `lay_out` runs over live placements *and* ghosts, then the
  lanes are split back apart. An hour you already spent behaves like an hour that is booked: a new
  block dropped on top of finished work lands beside it, exactly as it would beside a live one.

`planner_ghosts` is a cache rebuilt when the day changes and whenever the archive does — on open,
on day step, on retire, restore and forget. Deriving it per frame would walk the whole log inside a
loop that redraws continuously, for an answer that stands still. `placements_of` in `planner.rs`
takes the fields rather than an `Active`, so the placement rule has one implementation rather than
a second copy drifting alongside it in the archive.

---

## 18. Routines

`Active::recurrence` (`tasks.rs`), `Placeable` / `placements_of` (`planner.rs`), and the footer's
weekday row (`ui.rs`).

### 18.1 The third thing an item can be

The model had two kinds and needed three. A **task** is work you owe: ranked in the left column,
completable, archived when it is done. An **event** is news: it goes on the wall calendar because
somebody — usually future-you — needs to see that it is happening.

Sleeping is neither, and so is cooking, the commute, and the gym. Try it in the old model and both
answers are wrong:

| | Ranked list | Wall calendar | Wants a ✓ | Recurs |
|---|---|---|---|---|
| as a **task** | **yes — climbs to the top**, because a session booked for today is exactly what `PLANNED_LEAD_DAYS` lifts | no | **yes**, and finishing dinner files a row saying you completed an hour of work | no |
| as an **event** | no | **yes** — a daily block eats one of the three slots in every cell | no | no |

So a routine is its own thing, and it is defined by what it is *excluded* from:

| Excluded from | By | Why |
|---|---|---|
| the ranked task list | `refilter_tasks` | nobody is owed it, so there is nothing to rank |
| the planner's tray | `wants_planning` | it is not waiting for a slot; it *is* a slot |
| the priority score | `base_score` (early `0.0`) | a guard, not a policy — see below |
| the calendar grid | having no `deadline` | it is not news |
| the ✓ | the footer | there is no state in which sleeping every night is *done* |

The `base_score` guard is worth the two lines: a routine's shape — no deadline, no importance, no
horizon — is precisely the one the scorer calls `MALFORMED_SCORE` and shoves to the top of the list
so a corrupt entry gets noticed (§7.3). Two filters already keep routines away from it, and a guard
is cheaper than trusting both to hold forever.

### 18.2 Why it earns its place: the day's figure

The strongest argument for routines is not convenience, it is that **the planner's load figure is a
lie without them**. If the nine hours you were always going to spend on sleep and meals are not in
the day, a day with four genuinely free hours reports thirteen.

Which is also why the masthead counts them apart rather than folding them into "planned":

```
2h 30m planned · 3 blocks · 1 due · 9h routine · 2h done
```

Folding them in would make every day read as full; leaving them out is the failure above. Each part
is conditional, so an ordinary day still shows two or three.

### 18.3 A rule, not instances

```rust
pub struct Recurrence {
    pub days: u8,          // bit 0 = Monday … bit 6 = Sunday
    pub start_minutes: i32,
    pub minutes: u32,
}
```

One record generates a block for whatever day is being looked at (`placements_of`), and nothing is
stored per occurrence. Three things follow:

- **Nothing accumulates.** Sleeping every night for five years is one line of JSON.
- **There is no "this occurrence or all of them?"** — the question every calendar app has to ask,
  and the single largest source of complexity in recurring events. There is only the rule, so
  moving or resizing a block edits the rule and it moves on every day it falls on. You do not
  reschedule Wednesday's sleep; you change what time you go to bed. The footer names the days while
  you make the gesture so the reach of it is on screen.
- **Daylight saving is a non-issue.** The times are minutes from midnight, not `DateTime`s, so
  "23:00" means 23:00 on every day it lands on, including the ones where the clocks changed.
  Compare `Session`, which stores an absolute instant and needs `resolve_on_day` to step over a
  spring-forward gap.

`placements_of` checks the rule **first** and answers alone. A hand-edited save that gives a routine
a deadline and sessions as well gets the rule, not three copies of the item on its own day.

### 18.4 Once is a perfectly good answer

An empty `days` mask is **not** a broken rule — it is a one-off, and it is what a new routine
starts as.

"Tomorrow at two I walk the dog" is the same *kind* of thing as sleeping every night: time that is
spoken for, owed to nobody, with no ✓ that would mean anything and no business on the wall calendar.
The only difference is how often it comes round, and "once" is a perfectly good answer. So repeating
is a **property** of this kind rather than its definition, and the footer's row is labelled
`Repeats:` — all seven off reads "just this day".

A non-repeating rule needs to know *which* day, which is `Recurrence::anchor`. For a repeating rule
the anchor is merely where it was drawn and is ignored; `#[serde(default)]` covers routines written
before the field existed, all of which repeat.

**Unticking the last weekday re-anchors to the day being shown**, not to the anchor it had. Clearing
the last day while looking at Thursday should leave the block on Thursday; falling back to whichever
day it was first drawn on would make it vanish out from under the person who just unticked
something. That is why `toggle` takes the shown day.

> **A naming tension worth knowing about.** With one-off as the default, the switch's `Routine`
> label promises something the default is not. The word still describes what the facility is *for*,
> and "does not repeat" living inside the repeat control is what every calendar does — but if a
> better word turns up, this is the thing it would fix. `Block` is taken (a task's session is a
> block, and `＋ Block` adds one), which rules out the obvious candidate.

### 18.5 Which days, and what a drag means

A routine's one knob is *which days*, and it sits in the same footer slot a dated task's **severity**
and an undated task's **horizon** occupy — it is the same kind of thing, the single question that
kind of item asks (§7.3).

Seven letter toggles, not a preset combo: "Mon · Wed · Fri" is as ordinary as "every day", and a
preset list either omits it or grows a "Custom…" that opens the toggles anyway. **All** is beside
them because the case the feature exists for is daily.

**A new routine happens once, on the day you drew it.** A gesture should do what you watched it do
— drawing a block on Wednesday and silently filling in the next six days, or even every future
Wednesday, is a surprise you only find by stepping to Thursday. It is the same rule the deadline
follows: changed only where changing it looks like changing it (§16.1). **All** makes the daily case
one further click, **Once** takes a repeating one back to a single day.

### 18.6 How it is drawn

A routine's block is deliberately recessive: a quieter fill, a thinner outline in a washed-out
accent, and a `↻` in front of the time. Eight hours of sleep rendered as loud as an hour of real
work would make every day look full of nothing.

It is still **solid**, though, which is the distinction from an archive ghost (§17.4): a ghost is a
record and takes no gestures, while a routine is live and you can pick it up. Its palette entry is
`ROUTINE_COLOR_INDEX` — the calmest one — and the planner's timeline is the only place that colour
is ever seen, since routines never reach the grid.

### 18.7 In the archive

Deleting a routine files it like anything else, with the rule kept (`Archived::recurrence`), so
restoring brings back the arrangement rather than a nameless task the scorer reads as corrupt. Its
row reads:

```
✗  go to sleep
   routine dropped  ·  every day at 23:00 for 8h  ·  40d on the board
```

It counts under `Summary::routines`, apart from finished and dropped work, for the same reason
events do: a standing arrangement you stopped keeping is not a task you failed. `was_finished()`
excludes routines alongside events.

Ghosts explicitly **do not** generate from an archived rule (`rebuild_planner_ghosts` passes
`recurrence: None`): a dropped routine would otherwise haunt every past Tuesday it ever fell on.

### 18.8 What this version deliberately does not do

Recurrence is the feature most likely to metastasize, so the first cut is crude on purpose and these
are the known edges:

- **No blocks across midnight.** A rule is one span inside one day, so sleep 23:00–07:00 cannot be
  drawn as a single eight-hour block; `clamp_block` truncates it at midnight. A marker at 23:00 —
  which is what the day view mostly wants from it — works, and so do two routines meeting at
  midnight. Supporting a wrap means emitting a head and a tail placement on consecutive days and
  teaching the gestures to map the tail back to the rule.
- **No exceptions.** There is no "skip today". The natural escape hatch is for dragging a generated
  block to materialise a one-off override, which is a bigger change than the rule itself.
- **No end date, no monthly or n-weekly rules.** A weekday mask covers routine; anything else is a
  recurring *event*, which is a different feature.

---

## 19. The Keyboard

### 19.1 One rule

**The topmost open thing owns the keyboard.**

With nothing open, the menu bar's letters are live. Open the planner and its own keys take over.
Open something over the planner and that owns them instead (`modal_over_planner`). Every window
closes on the key that opened it *and* on Escape, so nothing is a one-way door.

That rule is what lets `T` mean "new task" on the calendar and "today" in the planner without
ambiguity: they are never live at the same time, and each is the obvious mnemonic on its own screen.

| Where | Key | Does |
|-------|-----|------|
| Calendar | `P` | Planner, **on the day it was last left** |
| | `A` | Archive |
| | `S` | Settings |
| | `T` | New task (caret already in the field) |
| | `E` | New event |
| | `F11` / `⌃⌘F` | Fullscreen (§15) |
| Planner | `←` `→` | Previous / next day |
| | `T` | Today |
| | `1` `2` `3` | What a drag makes: task / event / routine |
| | `Enter` | Rename the selection |
| | `U` | Un-book the selected block, or the whole plan |
| | `R` | Reflow — slide today's remaining work past now (§16.8) |
| | `Del` | Delete the selection (through the confirmation) |
| | `P` / `Esc` | Close |
| Archive | `/` | Search |
| | `Esc` | Clear the filter, then close |
| | `A` | Close |
| Settings | `S` / `Esc` | Close |
| Dialogs | `Enter` / `Esc` | Accept / cancel |
| Phone page (§21.5) | `←` `→` | Previous / next day (or month, in the agenda) |
| | `T` | Today |
| | `W` | Day ↔ agenda |

The shortcuts are named in the menu buttons' hover text and in the planner's own hint line, because
a single-letter shortcut nobody knows about is not a feature.

### 19.2 Two things that are easy to get wrong

**Plain letters and text fields.** Every global key stands down for
`ctx.egui_wants_keyboard_input()`. Without it, typing "please stop and archive" into the notepad
would open the planner, the settings sheet and the archive on the way through. The archive's search
field is deliberately **not** focused when the window opens for the same reason — it would take
every letter shortcut with it, and `A` would type an `a` instead of closing the window it had just
opened. `/` reaches for it instead.

**A toggle must not fire twice in one frame.** A window closes itself *during its own draw*, so by
the end of the frame it looks as though nothing was ever open — and the keypress that closed it is
still in that frame's input. Asking "is anything open?" at the end would have `P` close the planner
and reopen it on the same press, forever. So `ui()` captures `modal_owned_frame` **before** anything
is drawn, and the menu keys stand down for the whole of any frame that began with something open.

This is the third instance of the same pattern in the codebase — `naming_owned_frame` (§16.3.5) and
the archive's `confirm_owned_frame` (§17.3) are the others. Whenever a handler runs after the thing
it defers to has already been drawn, the state it needs is the state at the *start* of the frame.

### 19.3 The planner remembers its day

`open_planner` is called with `self.planner_day` from the menu button and from `P`, so the planner
comes back to the day you left it on. Only clicking a calendar cell moves it, which is the explicit
gesture; `Today` and `T` are one press away inside.

The day was always remembered — `close_planner` never cleared it — it simply was not used, and
every reopen snapped to today. Planning a Thursday is not one visit: you open the day, look at the
week, come back, and each of those trips went through the calendar to get home.

Within a run only. A restart opens on today, which is the right default for a program whose whole
premise is the current date.

---

## 20. Planned ≠ Scheduled

> **Move 1 — Reflow — is built; see §16.8. The rest of this is not.** Every other section of this
> document describes what the program does; this one describes where the planner should go and why,
> so the reasoning survives the conversation it came out of. Nothing here is a defect —
> `CODE_REVIEW.md` is the list of those.

### 20.1 The diagnosis: a plan made of clock times is over-specified

Two complaints come up about day planning, and they are the same complaint:

- *"I struggle to say **this** is when I will be doing that thing."*
- *"If one thing runs long, my whole schedule explodes."*

Dropping a block at 14:00 asserts two things:

1. I will spend an hour on the report today.
2. That hour is 14:00–15:00.

The first is usually known. The second is almost never known — it was invented because the timeline
demanded a coordinate. And the second is the one that breaks, and when it breaks it takes everything
below it with it.

**The schedule explodes because it was carrying information the planner never had.** Every block is
a claim about the clock that its author could not support, and a day made of thirty such claims will
be wrong by lunchtime through no fault of anyone's.

This is the same observation the app already made once. §16.1 separates *when it is due* from *when
I will do it*, because they are different facts and conflating them makes an app annoying to plan
with. The next line of that argument is:

> A **plan** is "an hour on this today". A **schedule** is "at 14:00".

The planner currently makes you write a schedule when all you have is a plan, and then holds you to
it. Everything below follows from taking that seriously.

### 20.2 What the app already has for this

Three pieces are in place, none of them put to this use yet:

| Already there | What it becomes |
|---------------|-----------------|
| Events and routines (§18) | The **anchor set** — the parts of a day that are genuinely fixed, as distinct from work. The dentist is at 10; sleep is at 23. Everything else can move around them. |
| `planner::lay_out` | Already resolves overlapping spans into lanes. Flowing work into the gaps between anchors is the same geometry read the other way round. |
| `Archived::duration_minutes` + `sessions` (§17) | Every finished task carries what it was estimated at and what was booked for it. That is a calibration signal nobody else has lying around. |

The routine work is what makes the rest possible. Before it, "fixed" and "work" were not
distinguishable in the model — a block was a block — so there was nothing to flow *around*.

### 20.3 The moves, in order

Ordered by payoff against cost and confidence, not by ambition.

#### 1. Reflow — a button, not a model change *(built — §16.8)*

The now-line exists. When work is booked before it and has not been ticked off, say so in the
masthead — *"1h 20m behind"* — and offer one action: push everything unfinished down past now, in
order, around the anchors.

That turns "my schedule exploded" into "press reflow". It needs no new fields: it rewrites the
`start` of the shown day's sessions, which every gesture in the planner already does.

**Do this first**, and not only because it is cheap. It is the experiment: living with it says
whether the flow model below is what is actually wanted, before committing to the data change that
flow requires.

#### 2. Floating sessions — the structural fix

A session gets a duration and a **position in the day's order**, and no start time. The planner
flows the floating ones from now, around the anchors, and draws them at *implied* times. Finish
early and everything slides up; finish late and it slides down. It cannot explode, because it was
never rigid.

Pinning stays, and gains a meaning it does not currently have: dragging a floating block onto an
hour **pins** it, and a pinned block now says *"I actually know this one."* The dentist is pinned.
The report is not.

This is the direct answer to "I struggle to say when I will do that thing": it stops asking. You say
what, how much, and roughly in what order, and the clock is derived rather than asserted.

The cost is real and it is in the data model. `Session::start` is an absolute `DateTime<Local>`,
which is exactly the over-specification this removes — so a session becomes either pinned to an
instant or floating on a day with a rank. That is a migration, and `#[serde(default)]` will carry
old saves through (everything existing is pinned), but it touches the placement rule, the gestures
and the day summary at once. Worth doing after reflow has earned it.

#### 3. Estimates as a range

`1–2h`, not `2h`. Book the optimistic end and draw the pessimistic end as a lighter tail on the
block.

Uncertainty becomes visible instead of being laundered into a single false number, and — more
useful — a day where every tail runs long *looks* over-committed instead of quietly being so. One
extra field beside `duration_minutes`, one extra rectangle in `paint_planner_entry`.

#### 4. Calibration from the archive, and not from a timer

The archive already holds the estimate and what was booked against it. Be careful what that
measures, though: **booked is what was planned, not what was spent.** Holding one plan up against
another plan says little.

The honest signal already in the log is **re-booking**. A task that needed `＋ Block` twice was
under-estimated, and `Archived::sittings()` records it for nothing. So the finding to surface is of
the form *"tasks you re-book run about 1.8× their estimate"* — and eventually the tray could offer
the corrected number rather than the typed one.

**A timer is the trap here.** It is the obvious answer to "estimation is hard", and it measures the
right thing — and it is a per-task ritual that gets abandoned inside a fortnight, in an application
built to be quiet until you actually need it. The re-booking signal is worse data that costs nothing
and is already being collected, which is what makes it the better feature.

### 20.4 What to resist

**Auto-scheduling the whole day.** Once flow exists it is tempting to have the app lay out all eight
hours optimally. That is the rigid schedule again, written by a machine: wrong in exactly the same
way, and worse to live with because it was not even chosen. The point of flowing is to stop
asserting the clock, not to assert it more confidently.

**Filling a day up.** The same instinct, one step earlier. Show committed against open honestly —
which is what the masthead's separate `routine` figure started (§18.2) — and let an over-booked day
look over-booked. A planner that quietly accepts sixteen hours of commitments in a day is not
helping.

**Treating a slipped plan as a failure.** The scorer already gets this right and it is worth
keeping: a missed *deadline* escalates, a missed *plan* tops out at the task's own weight (§7.1),
because "I meant to do that" is not the same thing as "that was due". Anything built here should
inherit that stance rather than nagging about a plan that did not survive the day.

### 20.5 How move 2 fits the Board

*A design note, written after §22 existed and before any of this was built. It says what the
smallest safe shape is, which tests already hold which lines, and where the risk actually sits —
so that when reflow has earned it (§20.3), the work starts from here and not from a blank page.*

**The algorithm already exists.** `planner::reflow(blocks, anchors, now)` slides a set of blocks
past a cursor, in their order, around fixed spans, snapping up and never dropping anything. A
floating session is nothing more than a block that is *always* reflowed — from now on today, from
the morning on any other day — rather than only when the button is pressed. Move 2 is therefore
not a new planner; it is running the one from §16.8 continuously over a subset, and drawing the
result.

**One field, defaulted.** `Session` gains `#[serde(default)] floating: bool`. Every save on disk
loads as pinned, which is what every session is today, and every path that reads `Session::start`
— the calendar summary, the archive, the feed, the scorer's `sessions.is_empty()`, the phone
snapshot — keeps reading it unchanged. That is the whole migration, and
`a_legacy_save_loads_migrated_and_a_corrupt_one_is_quarantined` is the test that holds it.

**`start` stays, and becomes the order key.** The tempting design gives a floating session a
`rank` and no time. It is the wrong one here: dozens of readers want an instant, the feed *must*
write one (a subscriber cannot flow), and "position in the day's order" is exactly what sorting by
`start` already gives — `reflow` sorts its input by `start` today. So a floating session keeps a
`start`, meaning *the last implied time*, and its place in the order is where that time sorts.
Dragging a floating block above another sets its `start` just before the other's and leaves it
floating; the flow then draws both at implied times. No second concept of order is introduced.

**One new place in the code.** `Board::implied_day(day, now)` — take the day's floating sessions
in `start` order; the anchors are the day's events, routines *and pinned blocks* (a pinned block is
now a claim, §20.3); the cursor is `now` for today and the earliest floating `start` otherwise;
call `reflow`; answer `(item, session) → start`. Everything that draws a day reads placements
through it — the desktop planner, `phone::snapshot` (for the day and for the week's seven), and the
calendar cell summary — so there is still one placement rule (§21.4's argument). Nothing is saved
by drawing. Implied times are written back into `start` only when a command runs on that day
(`Board::apply` already has the day in hand for `MoveBlock`, `AddBlock`, `Reflow`), so the stored
order key never drifts far from the drawn one and the feed is at most one edit stale.

**Two commands, and a flag on a third.** `Float { id, session }` unpins; `MoveBlock` gains
`#[serde(default)] floating: bool` — absent, it pins, which is what every existing sender means by
a move; `AddBlock` with no `start` books a floating session at the end of the day's order. The
desktop's drag pins, as it does now; a modifier or the sheet's own toggle floats. That is the
complete wire surface; `commands_parse_from_the_wire_with_either_time_shape` grows two cases.

**What the readers show.** The snapshot's entry carries `floating`; the page draws a floating block
with an open left edge and no time in its label, and its sheet says *flows from 14:20* rather than
*starts 14:20*. The feed writes the implied time as an ordinary `⏱` block: a calendar app cannot
flow and should not pretend to. The masthead's *behind* figure (`behind_minutes`) excludes floating
blocks — a block that slides is never behind, which is the point of it — so the Reflow button
stops appearing for a day that is all float, and stays for the pinned work that can still run late.

**Which existing tests hold which line.** `reflow_*` in `planner.rs` hold the slide (order kept,
anchors stepped, overlapping anchors passed in one sweep, an over-booked day clamped and left
overlapping); `blocks_move_book_remove_and_unplan` and `nothing_runs_past_midnight` hold the
session gestures and the midnight rule for pinned blocks, and must pass untouched when the flag is
added; `a_day_lays_out_live_entries_and_ghosts_together` holds the phone's lanes and will need one
floating entry added to it; `the_feed_writes_every_layer` holds the feed's shape.

**Where the risk is.** Not in the data — the field is inert until set. It is in *drawing*: the day
must now be laid out as a whole before any block of it can be placed, where today
`placements_for(item, day)` answers per item. Every caller of `placements_for` on the drawing side
(the planner, the calendar summary, the snapshot) has to move to the day-level pass, and a caller
missed draws a floating block at its stale stored time — wrong, but not lost, and visible at once.
The order of work that keeps each step green: (1) the field and the three commands, with tests,
nothing drawn differently; (2) `implied_day` used by the snapshot and the planner; (3) the
visuals and the sheet; (4) `behind_minutes` excluding float. Steps 1 and 2 are a day each; 3 is
where the design decisions are and where living with Reflow first pays.

---

## 21. The Phone View

`phone.rs` (the server, the wire shapes, the day snapshot and the feed — pure where it can be,
and tested), `phone.html` (the page, embedded with `include_str!`), and the command handler plus
the **PHONE** settings section in `ui.rs`.

### 21.1 The central idea: a thin client of the one live process

The goal was to see *and edit* the calendar on a phone. [`MOBILE.md`](MOBILE.md) weighs five
ways of getting there; this is the one that was built, and the reason is §4.1.

Every save writes the whole active set, and two writers clobber each other — the documented
data-loss case. Any sync to a phone calendar (Google, CalDAV, a synced file) therefore creates a
**second copy of the truth** that has to be reconciled: identity mapping across systems,
tombstones, a conflict policy, and a translation into a schema that has no word for severity,
horizon, estimate or rule. All of that machinery exists to solve a problem that is only there
because a second copy was made.

So no second copy is made. While TaskDeck runs it serves one web page, and the phone is a thin
client of the running process: a tap on the phone becomes a [`Command`], travels over a channel
to the thread that owns the board, and is applied through **the same `Board::apply` a desktop
gesture goes through** (§22.1) — the desktop's setters, `plan_item`, `add_session`,
`set_item_deadline`, `retire_active_thing`, …, are one-line wrappers that build the same command.
The calendar, the task list, the planner and the disk agree about a phone edit a frame later,
exactly as they would about a drag. Nothing the phone does is a second way of changing a task.

The price is stated rather than hidden: **the phone view lives only while the process that owns
the board runs** — the desktop app, or `taskdeck-server` on a box that is always on (§22). For a
wall calendar on a spare monitor the first is the usual state, and the feed (§21.6) covers the
read-only case when it is not.

### 21.2 Threads: the weather pattern, again

`PhoneServer::start` binds `0.0.0.0:<port>` with `tiny_http` and spawns two plain worker
threads (and a third for the long poll, below). A worker never touches `TaskApp`. Per request it
parses a `Command`, sends `PhoneRequest { command, reply }` down a channel, calls the `Wake` it
was given — on the desktop a closure around the same `EventLoopProxy` the weather thread uses, on
`taskdeck-server` nothing at all — and blocks on the reply for up to eight seconds.

The UI thread drains the queue in **two** places, and the first one matters:

- `App::user_event` — the proxy wake. `handle_redraw` returns *before running the frame* when the
  window is minimized or the surface is occluded, and the idle sleep stops frames altogether
  (§5.4). Serving from the wake itself means a phone edit lands in all of those states. Nothing
  a command does needs egui.
- the top of `TaskApp::ui` — catches anything that arrived since, before the frame draws it.

**The long poll parks requests, not threads.** `GET /api/wait?version=N` is handed to the `Pulse`
— a mutex-and-condvar holding the version number and a list of parked requests — and the worker
is free at once. One thread per server (`answer_parked`) answers every parked request the moment
the board's owner publishes a newer version (every save the board makes, the notepad save, a
colour scheme change) or its 25 s deadline passes. That is what lets a drag at the desk show on the phone
within a second while the phone asks nothing in between; the wait never touches the UI thread; and
an *abandoned* wait costs nothing. The first cut parked the worker itself, and a phone reloading
its page a few times — each reload abandoning a wait the server cannot see was abandoned — parked
every worker for the full timeout with commands queued behind them. Two workers serve everything
else. The page keeps a slow poll underneath as a net.

Dropping the `PhoneServer` sets a flag, **interrupts** the pulse so its thread answers every parked
request with the version unchanged and leaves — marking the pulse *closed* under the same lock a
park takes, so a wait that arrives after the sweep is answered at once by its worker rather than
parked for a thread that is gone — and calls `Server::unblock` once per worker.
`tiny_http` *queues* each unblock, so a worker that is busy at that moment still finds its unblock
on its next `recv`; each then leaves, and the socket closes with the last `Arc`. They are
deliberately **not joined**: a worker may be mid-request, waiting for a reply that only the thread
dropping the server can produce.

**Restarts wait for the socket.** A new port, a new key, or off-and-on cannot bind the same port
until that worker has let go, which is a moment later on another thread. `tend_phone_server`
therefore retries a failed bind quietly every `PHONE_RETRY_EVERY` for `PHONE_RESTART_WINDOW`
after a restart, and only reports a failure that outlasts the window. It runs at the top of every
frame and every wake, so nothing needs to be scheduled.

### 21.3 The wire

A few public routes, which carry no data, and the authorised ones:

| Route | What |
|-------|------|
| `GET /` | the page. Public — it carries no data, and a home-screen shortcut that opens `/` has to load before it can present its key |
| `GET /icon-192.png`, `GET /icon-512.png`, `GET /icon.png`, `GET /sw.js`, `GET /manifest.webmanifest` | public; the service worker (§21.5) is only honoured from a secure origin; the manifest's `start_url` keeps the token it was asked with, because iOS gives a home-screen app storage of its own |
| `GET /api/state?from=YYYY-MM-DD&days=N` | the `Snapshot` (§21.4); `N` is clamped to 31 |
| `GET /api/wait?version=N` | long poll: answers `{version}` the moment the version moves past `N`, or after 25 s unchanged |
| `GET /api/board` | the whole board — items, archive, notes, version — for a desktop that keeps a replica of it (§22.3) |
| `POST /api/command` | one `Command`, as JSON tagged by `op`; a query sent here is refused (`400`). `X-TaskDeck-Request: <key>` names the request, the same on every retry, so a repeat is answered with the first reply rather than applied again (§22.4) |
| `GET /calendar.ics` | the feed (§21.6) |

**Everything textual is gzipped when the client offers to take it**, which is every browser. The
decision lives in one place — `Encoding`, taken off `Accept-Encoding` once per request — rather than
at the two dozen sites a response is built, because it is a property of the transport and not of any
particular answer. Measured against the real board: the page 127,656 → 37,142 bytes, a 31-day
snapshot 25,255 → 3,840, the feed 1,874 → 539. A body under 1,400 bytes is sent as it is: gzip adds
a header and a checksum, so a one-sentence error comes out *longer* than it went in and every hop
still pays to decode it. The page and the service worker never change between builds, so they are
packed once at the best setting and kept; everything else is packed per request at the fast one.
A body that would grow is never compressed.

**The icon is served at the sizes a home screen asks for.** The source is 882×882 and 660 KB, which
was most of what a first install moved — and Android wants 192 and 512, so it was paying for a
picture it immediately threw most of away. Scaled once per size on first request (a resize on the
request path would otherwise be paid every time by the client least able to afford it), it is 50 KB
and 257 KB. The service worker's shell no longer holds any of them: the page references no image of
its own, and the only thing that ever looks at the icon is the operating system, at install.

The key travels as the `X-TaskDeck-Token` header (the page), as a bearer token, or as `?token=`
in the query — the only place a calendar app subscribing to the feed can put it. Comparison is
constant-time out of habit. The token is 160 bits from `/dev/urandom` spelled in a 32-letter
alphabet that survives a QR code and a phone keyboard; on a platform without `/dev/urandom` the
standard library's per-process hash seed is stirred with the clock and the pid — not a CSPRNG,
but unguessable from outside the machine, which is the threat.

**The bind is the defence, and it starts closed.** `phone_bind_address` is `127.0.0.1` on a fresh
install, because `tailscale serve` — the deployment SERVER.md describes — hands requests to exactly
there, so the ordinary setup needs nothing wider. Every interface is a door nothing walks through
except strangers: measured on a real server, **two TCP connections that declare a body and never
send it hold both worker threads for as long as they stay open**, because `tiny_http` 0.12 sets no
socket read timeout (verified: the crate contains no `set_read_timeout`) and `EqualReader::drop`
drains a body that will never arrive. The token is untouched by this — nothing is read, nothing is
written, every route still answers 401 — but the phone view stops answering, and on `0.0.0.0` the
attacker is anyone on the café or campus network the laptop joined. A durable fix needs a patched
`tiny_http`; the bind removes the attacker instead, and the server says at startup when it is open
wide.

**Every answer carries three guards.** `Referrer-Policy: no-referrer` is the load-bearing one: the
token is in the query string, so without it any request out of the page would hand the whole link
to somewhere else in the `Referer` header. `X-Content-Type-Options: nosniff`, and a
`Content-Security-Policy` that a one-file page can afford — `default-src 'none'` with `'self'` and
inline allowed back, `frame-ancestors 'none'` so it cannot be framed, `form-action 'none'` so an
injected form has nowhere to post. `worker-src 'self'` is not decoration: without it the service
worker falls back to `script-src`, is refused, and the offline shell goes with it — found by
loading the page rather than by reading the policy.

**A command must be offered as `application/json`**, and that is the only thing stopping a page the
phone visits from writing to the board with a leaked token. A cross-origin form or `fetch` may send
`text/plain`, `multipart/form-data` or `application/x-www-form-urlencoded` with nobody's permission;
asking for JSON puts the request in the class that needs a CORS preflight, and this server answers
no preflight at all. There is no cookie here to be `SameSite` — the token rides in the URL, and a
token that has leaked once should not also be a write key for every page the phone opens.

**Calendar fetches are pinned to https and three redirects.** `checked_url` refuses `http` and says
why — a calendar link is a password. Without `.https_only(true)` and an explicit
`redirect::Policy` that promise would be the *calendar server's* to keep: `reqwest` follows ten
redirects by default and permits an https→http downgrade, so one `302` puts the credential on the
wire in clear, and one to `http://127.0.0.1/` points the fetcher at whatever else is listening here.

**Times on the wire are what the page's native inputs produce**: a day is `YYYY-MM-DD`, a time
of day is minutes from midnight, a deadline is a naive local `YYYY-MM-DDTHH:MM` (what
`<input type="datetime-local">` yields; RFC 3339 with an offset is accepted too). Everything is
resolved on the clock of the machine that owns the board through `planner::resolve_on_day`, so a
phone in another zone plans in the calendar's zone, not its own.

Every mutating command **validates first and then changes the board**. The only logic of its own
is turning a bad request into a message: a mistyped time on the phone is the phone's to hear
about (`400`), not the desk's (an error window). An item that has gone — finished from the desk,
or the phone looking at a stale snapshot — answers `410`, which the page treats as "refresh and
let go". This `match` (`Board::apply`, §22.1) is the complete list of what a phone may do; the
desktop's `TaskApp::execute_phone_command` only adds what a window has to do around it, such as
dismissing a confirmation for an item the phone just finished.

### 21.4 The snapshot: laid out, not described

`GET /api/state` answers with everything the page needs to draw a day and its tray in one round
trip, and it is **laid out already**: each entry carries its lane (`column`/`columns`) from
`planner::lay_out`, run over live placements *and* archive ghosts together exactly as the desktop
does (§17.4), so a block booked from the phone lands beside finished work rather than on top of
it. The day figure is the desktop masthead's, computed by the same rules (§18.2). The page draws
and never decides — which is what keeps one implementation of the placement rule rather than a
second one drifting in JavaScript.

`items` carries the full detail of every live item (deadline, estimate, booked, remaining,
severity, horizon, rule, sessions) so the sheet that opens on a tap needs no second request.
`version` is bumped by the board on every save it makes (`Board::touch`, the one funnel) so the
page can tell a changed day from a redraw.

**`days` is clamped to `MAX_SNAPSHOT_DAYS`, and clamped silently** — 31, the longest calendar
month, because the phone's agenda pages by month and a request, a cache key and a slot on the
month rail should all be the same unit. Asking for more returns 31 days with a 200 and no marker
of any kind: a client meeting a server's limit is not a client error, and a `400` would break a
page that sensibly asks for as much as it can use. **So a client counts `days.length` and resumes
from the last date it actually received**, never from the number it asked for. The ceiling stops
at 31 rather than higher because `build_day` rescans the whole archive for every day it builds and
`ArchiveLog` holds every line ever written; past roughly ninety days per request, or a few thousand
archive rows, the archive wants bucketing by session date once per snapshot before the ceiling
moves again.

**`known` is the span the subscribed calendars were actually read for**, both ends inclusive —
`WINDOW_BACK_DAYS` back and `WINDOW_FORWARD_DAYS` forward of whenever the fetch happened. It exists
because outside that span a day is byte-identical on the wire to a day with nothing on it, and
answering "nothing on this day" about a day nobody has looked into is a wrong answer to the only
question a calendar is asked. With the span the phone can say *nothing of yours* instead.

Three rules keep it honest. It is **carried from the fetch that used it**, never re-derived from a
later clock, so a desk that has just started up reports the window its cache was written with until
the first refresh lands seconds later. A refresh in which **nothing was read** keeps the previous
span rather than claiming a new one — the events carried forward through an outage were read for an
older window, and a desk that has been off a week must not claim to have looked a week further
ahead than anyone has. And it is **absent entirely when nothing is subscribed**, because a board
with no calendars has no ignorance to declare, and every day on it that looks free is free.

It is deliberately **not** part of `Overlay::digest`, for the reason `status` is not: the span
slides forward every midnight, and folding it in would make every refresh a change and wake every
parked phone, which is the one thing the digest exists to prevent.

### 21.5 The page

One file, no dependencies, dark, sized for a thumb — and laid out around two rules. **The masthead
is what you read, and the bar at the bottom is everything you touch.** And **a day is a plate**: one
material at one radius with a lit edge and a shadow, floating in ten pixels of ground, rather than a
change of grey. The three dark tokens this replaced differed by 1.08:1 and 1.11:1 — twenty-two rules
spending two tokens on a distinction nobody could see. At the bottom of the luminance scale a
boundary is drawn by an edge and a shadow, not by a ratio; the plate's own ratio is still 1.11:1 and
that is the point. A day with nothing on it gets no plate: it is a void of page colour with its date
and the word *free* on it, so the ribbon reads as a rhythm of solid days and open ones.

**One monospace column runs the whole length of the page.** The masthead's day number, every day
heading's date, every agenda row's time and every hour label in the day view share a left edge at
page x = 23 — ten pixels of page gutter, ten of plate padding, and the row's three-pixel accent bar
— with content starting at 77. It is not a rule or a fill, just alignment and `tabular-nums`, so
`09:15` and `11:00` are the same width and every colon lines up. The desktop has called this the
date spine since it was written and runs its whole planner in monospace; the phone never picked it
up, and `ui-monospace` is a system face that costs nothing.

**The picture behind it.** The desk's own background image, cropped to a phone's shape, scaled to
462×1000 and darkened, served from `/bg-<hash>.jpg`. Three numbers decide it. Decode cost scales
with **megapixels, not bytes** — about 45 MP/s on a desk machine and eight to twenty times slower on
an old phone in battery saver — so the desktop's 3000×2000 original would be one to nearly three
seconds on every cold open; at 0.46 MP it is a tenth of a second, and 927 KB becomes 48. The name
carries the content's hash, so the URL is immutable and cached for a year rather than re-fetched
with everything else that is `no-store`. And it is behind the token, because a personal photograph
is a stronger reason to ask for the key than the app's own icon was.

The darkening is **solved, not set**. Nothing on the picture may be bright enough to swallow the
smallest text that sits on it, expressed as a ceiling on the 99.9th percentile of relative
luminance — one blown pixel is where a room name goes to die. A highlight roll-off in linear light
leaves the shadows almost untouched and crushes the top end, then one scale factor lands the peak on
the ceiling; since luminance is a linear combination of linear channels, that factor is arithmetic
rather than a search. It is then checked against what actually comes out of the JPEG encoder, whose
ringing pushes highlights back up, and corrected. The desk's own `background_image_tint_percent` is
a **floor** on the darkening and never a ceiling: the phone's type is 11 to 14px where a wall
calendar's is a heading, so it may need to go darker than the desk asked and never lighter.

**Text is made legible by a halo on the glyphs, not by a card under them.** A card is the obvious
way to put text on a photograph and also the way to hide the photograph. Instead the text carries a
soft dark shadow for the falloff and — where the browser can draw a stroke *behind* the fill — a
stroke that becomes a halo rather than a thicker letter. `paint-order: stroke fill` is the
load-bearing half and sits behind `@supports`, because without it a stroke paints over the fill and
fattens the text into a blob; the shadow alone carries anything that lacks it. Deliberately **not**
`backdrop-filter`, which blurs the backdrop per element per frame, and **not**
`filter: drop-shadow`, which forces a filter surface per element — either across a scrolling list is
a stall this page has already paid for once. The whole treatment is scoped to `html.has-photo`,
because on a flat ground a halo is invisible and still costs paint.

**A row's ground is its hue at a fixed luminance**, not its hue at a fixed alpha. `groundOf` scales
the accent's three channels in linear light onto a five-step ladder, which moves value and leaves
hue and saturation exactly alone — so a slot stays its own colour and gains weight. That is the ramp
`color.rs` has been sending in `RAMP_ALPHA` all along, whose own comment calls opacity "half of what
makes a step read as louder than the one below it", and which the phone was discarding by
hard-coding one alpha for every slot. `color.rs` also records the bug that causes: two colours
"differed by a hue nudge at nearly the same lightness, and on a small calendar pill over a photo
they were one colour."

The ceiling of that ladder is a contrast budget, and it is what makes the type legible. A room line
takes the accent mixed 55% toward the page's ink rather than the raw accent, and the numbers moved
accordingly: measured on the real board, a clock time went from **3.49:1 to 5.92:1** and a room line
from 2.28:1 to **8.95:1**. The times are the one thing this app exists to deliver and they were the
least legible text on the screen, at 11px, outdoors, at battery-saver brightness. Nothing in the masthead is
tappable. That started as ergonomics and turned out to be structural: navigation used to be four
buttons in the top row, which on a phone held in one hand is the furthest thing from a thumb, while
the three easiest targets on the screen were spent on Tray, Notes and ＋ New — of which only the
last is used often. Two properties fall out of the rule rather than having to be maintained. The
masthead becomes a fixed height per view (76px in both), so a mode change re-measures to the same
number and the two IntersectionObservers are not torn down and rebuilt; and every control left
under a resting thumb is reversible — Tray, Today, the mode toggle and ＋ New only move you or open
something you can cancel.

The masthead holds the day and its figure in the day view, and in the agenda one muted line naming
the day the buttons act on (**Today · Sunday 6 September**, with **· 1h 20m behind** in amber when
it is, §16.8) above the glance line. The body is the day as a timeline (routines dashed and
recessive, ghosts outlined, due times as flagged markers, a now-line on today) or the agenda ribbon.

The bar is two rows. A **scrubber** — the seven-tap week strip in the day view, the seven-cell month
rail in the agenda — and a **deck** of four: Tray, Today, the mode toggle, ＋ New. `‹ ›` are gone
from the screen entirely: the scrubber reaches three days or three months either way in one tap,
which is further than a step and nearer the hand. They survive as what `←` `→` and the swipe call.
`T` and `W` work as before. The week strip is **centred on the shown day**, three either way, rather
than anchored to Monday — a Monday-anchored week offers a Sunday reader seven days that have already
happened.

Two things moved out of the bar. **Notes** is a third of the width for something opened once a week;
it is now a button in the Tray sheet's footer, beside the tasks it is about. And **Reflow** is not
reversible, so it does not sit under a resting thumb: it is a chip at the top of the Tray sheet, on
all three tabs, and the deck only points at it — the Tray button takes an amber underline when the
day is behind. The badge on that button keeps meaning exactly one thing, how much is waiting, urgent
when something is due; otherwise an empty tray on a late day would have shown an urgent zero.

**Every sheet ends in a way out that is on screen.** The footer is sticky, so Close is reachable
however long the list above it is — a Close at the bottom of 88vh of scrolling tasks is not a way
out of anything one-handed, and the ＋ New sheet had none at all. The grip at the top is a real
button: tap it to close, or drag it down past 60px. Anything in between springs back, because a
half-committed drag should not decide.

Under the figure, the shown day's week as seven taps, Monday first like the calendar, each with up
to three dots for what the wall calendar would show on it — events and due dates, in their own
colours, the same budget a calendar cell has. The dots are drawn from the same store the agenda
fills, so the week costs no request of its own; a day the page has not been told about yet simply
has no dots.

**The agenda** (`W`) is the other view, and the one the page opens in — a continuous list of days,
each a heading with a count and the day's things beneath it in clock order: the start over the end
in the left column, the name, and the room under it when the event came from a subscribed calendar.
It is one run, not a week: it grows a month at a time downward on its own as you reach the end, and
upward when you tap **▲ Earlier**.

The asymmetry is deliberate. A sentinel at the bottom is safe because appending never moves what
you are reading; a sentinel at the top would fire on every cold open, since the run starts at the
reading position, and an automatic *prepend* is the one mutation that can land in the middle of a
fling. A tap has no fling in flight.

**The masthead answers the question before anything is read.** The line under the buttons says one
of five things: what you are in and when it ends (*AG Board Game Night — until 03:30*), how long
until the next thing when it is close (*In 40m — KEK101 · 15:15 · Chemicum, sali A110*), how long
you are free for when it is not (*Free for 4h 10m — then MS-C1350 at 10:15*), that a task falls due
rather than a place to be, or that there is nothing left today and what tomorrow starts with. A
filled dot means something is on or nearly on; a hollow one means you are free. It is drawn from
today's snapshot, which is already in hand, and re-drawn every minute and whenever the app comes
back to the front — *in 25 min* is true for one minute, and a phone out of a pocket must not show a
twenty-minute-old answer for as long as a round trip takes, or on a dead network for ever.

It replaces the day figure in this view, deliberately. "1h planned · 2 blocks" is a fact about a day
rather than an answer about it, and on a day made entirely of lectures it read *nothing on this day*,
because the figure counts only the board's own things. The names are shortened to the part anyone
actually uses — a university feed writes `MS-C1350, Partial Differential Equations, Lähiopetus
1.9.–7.12.2026 - Luento - L01`, and the answer to *what am I in* is `MS-C1350`. Long spans and
all-day bands take no part in it: they describe a day rather than occupying it, so they cannot say
when you are free.

The big date comes off the masthead here, because the rail immediately below already names the month
in the same words, and the room it frees is where the answer goes. (It also could not fit: a flex
item will not shrink below its content without `min-width: 0`, so `Wednesday` at 26px had been
running off the right edge of the screen next to four buttons, in both views, since the masthead was
written.)

**Nothing about scrolling changes anything.** It is a reading motion: it never moves the shown day,
never sends a command, and is never reachable from `load()` — which runs from the long poll, a
sixty-second timer, the tab becoming visible and the network returning, and would otherwise fetch a
page a minute with the phone in a pocket. In this view the shown day is pinned to **today**, so
**＋ New**, **Reflow** and **Tray** all mean today and the masthead says so in words. That is not a
limitation working around a problem; a scroll position quietly retargeting an edit is how work gets
booked on the wrong date. To act on another day, tap its heading, which opens it in the day view.

A **month rail** replaces the week strip inside this view: seven cells, the month you are reading
in the middle, a tap on an end cell three months away. Pure scrolling is O(distance) in
thumb-flicks — a day is about 250px and a screen holds three, so a flick is roughly a week and next
February is twenty flicks — and the rail makes anywhere within half a year two taps for seven nodes
and one function.

Three numbers bound it. The **DOM holds four months** and settles back to three: whole months are
dropped from the far end at idle, never while a finger is down, and never from below unless the
reader is two screens clear of it — trimming the bottom near the bottom clamps the scroll offset
and throws the list a screenful. The **cache holds six months** as one blob per month under
`taskdeck-month-*`, deliberately outside the `taskdeck-snap-` prefix so the day cache's scan of the
whole store does not grow, and bounded to what the boot actually reads back so a month scrolled
past once is never written and left. **One request is ever in flight**, through a promise chain:
every one of them serialises on the desk's UI thread anyway, and a fan-out would only starve the
fetch for the day someone is looking at.

Two things hold the reading position still. Everything that inserts or removes days above it goes
through one helper that **measures** a surviving day's real screen position before and after the
change and puts back the difference — which is right whether the engine's own scroll anchoring
compensated fully, partly or not at all, and cannot land on top of an adjustment the engine already
made. And every programmatic scroll is `auto` and never `smooth`, because anchor adjustments are
skipped for the whole duration of a scripted animation.

A day past the far edge of the subscribed calendars (§21.4's `known`) is drawn with a dashed rule
and says **nothing of yours** rather than *nothing*, with one card at the end saying where the line
is. This is not a gate — it was one, and the gate was wrong twice over: a page is a whole month, so
the boundary falls *inside* one and there is no step to stop at, and a rail jump steps over it
without asking. What a reader needs is to know why those days look empty, and each of them says so
for itself.

The day view was the only view until the week became a seven-column time grid, and the grid did
not survive meeting a real week. The arithmetic is unkind: at 380 device-independent pixels a
column is 51 wide, and a lecture called *MS-C1350, Partial Differential Equations, L01* in a room
called *U4 NORDEA - U142* has nothing to say in 51 pixels. Turning the grid on its side does not
help — 24 hours across 380px gives a 90-minute lecture 28 pixels, and clipping the night away to
07:00–22:00 only gets to 44. A grid spends its width on *when*, which seven columns cannot afford;
a list spends it on *what*, and the clock survives as two small numbers. The day view keeps its
hour scale, because one column can pay for it. **Notes** in the bottom bar opens the desktop notepad's text, and **Save** replaces it
whole (`Command::SetNotes`, tabs removed as on the desktop) — explicit rather than per keystroke,
so the desk and the phone cannot fight over a sentence.

Tapping anything opens a **sheet**; **holding** a block lifts it, and it then follows the finger
snapped to the quarter hour and is moved on release — the same `move_block` the sheet's time
input sends, so a routine's block moves its rule and an event's block moves the event, as on the
desktop. A finger that moves before the hold elapses is a scroll and is left to the page; once a
block is lifted a non-passive `touchmove` listener keeps the page still under it, and the click
that follows the release is swallowed so it does not open the sheet. Precise times still go
through the sheet: on a phone, `<input type="time">` and a length picker are a better aim than a
finger on a 15-minute block. The sheet is the footer's controls in a different arrangement — starts / length for the block; due, severity or horizon,
takes, **＋ Block on this day**, unplan, ✓ Done for a task; when / for on an event; the seven
weekday toggles with **All** and **Once** on a routine; Delete on everything. The two
destructive verbs confirm, as on the desktop. The **Tray** sheet has three tabs: **Unplanned** —
the desktop tray, leading with what is **due by this day**, then the backlog, with the quick-add
field on top (a name, and a due date if **due…** is opened — the same two shapes the desktop
makes) — **All**, every task in the order the desktop's left column ranks them
(`Snapshot::ranked` is `list_tasks` as drawn, jitter and all) — and **Done**, the last
`DONE_ROWS_MAX` rows of the archive with their verdict lines and a **↩** that puts one back
(`Command::Restore`, addressed by the archive's own key, through `restore_archived` exactly as the
ledger's button is). The undo for a ✓ tapped on the wrong row; the ledger itself stays at the
desk. **New** makes a task, event or routine at a chosen time, and tapping empty timeline opens
it pre-filled with that hour.

The page listens on `/api/wait` while visible (§21.2), refetches after every edit and on
returning to the foreground, and polls once a minute as a net; it never redraws a sheet someone
is typing into.

**Offline.** The page keeps the last snapshot of each day it showed in the phone's own storage,
and a small service worker (`phone_sw.js`, served at `/sw.js`) keeps the page shell and the icon
— network-first, so an updated page arrives whenever it can; the API, the feed and the manifest
are never cached. With the server unreachable the page therefore still opens and still shows the
day, with a banner saying "as of 12:05" and why. A service worker needs a secure context — HTTPS,
or localhost — so that half works over Tailscale's HTTPS (`tailscale serve`, `SERVER.md` §2) and
not over a plain-http LAN address, where the browser refuses the worker and the page behaves as
it did before. Unreachable, it says so and offers a retry; refused (`401`, after a new key), it
asks for the link again.

**Edits made meanwhile are kept, not applied.** An edit the server cannot be reached for goes
into a small outbox in the phone's storage — a network failure; a request that runs out of time
(fifteen seconds for an ordinary one, forty-five for the long poll, so a mobile link that drops
mid-request cannot hang the page); or a `5xx`, the server's own trouble. The banner says so —
*2 changes waiting to be sent: add "Milk", change the length of "Learn some Rust"* — with
**Retry** and **Discard**. The page has no board of its own to apply them to, so a waiting edit is
shown as waiting rather than as done: that is the honest half of the desktop's outbox (§22.4),
which does apply locally because it has a `Board`. A sheet kept open through this is redrawn when
an edit is kept, so a control does not sit there looking applied, and the item's sheet carries a
line saying how many edits to it are waiting and that what it shows is from before them. A key the
server no longer accepts shows the gate, with a line saying how many edits are kept and go with
the first load after the new link.

**Replay.** The waiting edits are sent in order the next time the server answers — before any
newer edit, so what was done first arrives first — each under the request key it was given when
it was kept, so a lost answer never applies an edit twice (§22.4). Replay is tried at every load:
the retry button, the network coming back, returning to the foreground, the minute poll, the long
poll's first answer after an outage. One the server refuses is dropped and said in the banner
until acknowledged, never retried for ever; the notice is kept in the phone's storage, so a reload
before **OK** does not lose the only trace of an edit that was made here and is not on the board.
When the edits have gone through, an open sheet is redrawn once more to show the board as it now
is. A queued create needs no temporary id: a later edit can only name an item the page has seen
in a snapshot, so nothing queued after a create can refer to it.

### 21.6 The feed

`/calendar.ics` is the live set as iCalendar, four layers each tagged with a `CATEGORIES`:
events as themselves, a task's deadline as a short `⚑ due:` event at the due time, each session
as a `⏱` block, and routines as weekly `RRULE`s. Instants are written in UTC. A routine is
written as a **floating** local time — no zone — with its rule, which is precisely what the
model says it is: 23:00 means 23:00 on every day it lands on, clocks changing or not (§18.3).

Subscribe to it from Google Calendar ("From URL") or the phone's own calendar app and the
calendar shows up in a native app, in widgets, with the desktop off — read-only, and refreshed
on the subscriber's schedule. It is read-only *by construction*: nothing is written back from
it, so it can be subscribed to from anywhere without a conflict story.

### 21.7 Settings

The **PHONE** section: a switch, the port, and — while the server is up — the link for each
address this machine has, a **Copy** button, **Open here** (the platform's own opener — `open`,
`xdg-open`, `cmd /C start` — for a look at the view in the desktop browser), the link as a QR
code, the feed link, and **New key**. Off by default: a listening socket is a change in the app's posture and should be chosen.

Addresses come from `phone::local_addresses`, which enumerates no interfaces: it asks the kernel
which local address it *would* use to reach a private-range destination (the LAN interface) and
to reach `100.100.100.100` (the Tailscale interface when a tailnet is up, the LAN one again when
not — deduplicated). Connecting a UDP socket sends nothing. Over Tailscale the transport is
encrypted end to end, so plain HTTP inside the tunnel is fine and the TLS-certificate question
never comes up. The link is the key — the sheet says to share it like a password — and it should
not be port-forwarded to the open internet.

The server binds every interface by default (`phone_bind_address = "0.0.0.0"`), because the phone
is sometimes on the LAN and sometimes on the tailnet and the token guards the door either way.
Setting the key to one address in `userconfig.toml` — the Tailscale one, say — binds that address
alone; the links shown are then for that address only (`phone::addresses_for`). A key that is not
an address falls back to every interface rather than to no phone view at all. There is no control
for it on the sheet: it is a posture decided once, in the file, read at start — a change there
takes effect at the next start, like the server fields (§22.3).

### 21.8 What this deliberately does not do

- **Sync to a phone calendar app in both directions.** The feed is one-way on purpose. Two-way
  is `MOBILE.md`'s Proposal B, costed there; the reason it was not built first is that most of
  its unique value is view value the feed already delivers.
- **Work with the desktop closed — on its own.** By construction (§21.1). What lifts that limit
  is moving the board to a machine that is always on: `taskdeck-server` (§22) serves this same
  page and feed with no window at all, and the desktop becomes one of its clients.
- **Resize by dragging, or drag out a new block.** Hold-to-drag moves; lengths and new blocks go
  through the sheet, where a picker is a better aim than a finger. Both could be layered on
  without changing anything on the wire.
- **Become a second TaskDeck.** The page is a companion. Scoring, the calendar grid, the archive
  window and colour schemes stay at the desk.

---

## 22. The Board, the Server, and the Desktop as a Client

`board.rs` (the data and every way it changes), `server_main.rs` (the `taskdeck-server`
binary), `sync.rs` (the desktop as a client), and [`SERVER.md`](SERVER.md) (setting a server up).

### 22.1 The central idea: one board, many hosts

Everything TaskDeck knows — the live items, the archive, the notepad, the id counter — is a
[`Board`](src/board.rs), and there is exactly one way to change it: `Board::apply(Command)`.
A [`Command`] is a small, named, serialisable intent — *move block 2 of task 17 to 14:30*,
*put back the row archived at 16:20* — and the variants of that one enum are the complete list of
things that can happen to the data. The desktop's every gesture, the phone's every tap and the
server's every request end up in the same `match`, which validates, changes, and saves.

That was a refactor of what §16.6 already described (every setter ending in `summarize_calendar`
+ `save_active_things`); `TaskApp` now holds a `Board` and delegates, and the setters it kept
(`plan_item`, `add_session`, `set_item_deadline`, …) are one-line wrappers that build a command.
Nothing the user sees changed, and the command handler the review left untested (§21) is now the
best-tested thing in the tree: `board.rs` exercises every command against a temporary directory,
including reopening it to see what was saved.

The reason for the refactor is what it makes possible. A board that knows nothing about windows
can be **hosted** anywhere:

| Host | What it is | Where the board's files live |
|------|------------|------------------------------|
| `TaskDeck`, alone | the desktop app as it always was | this machine's `taskdeck_data/` |
| `taskdeck-server` | the board with no window: the phone page, the feed, and the client API, on a box that is always on | that box's `taskdeck_data/` |
| `TaskDeck`, as a **client** | the desktop app with `server_url` set: the full GUI over a *replica* of a server's board | the server's; this machine keeps a cache |

The two-writers rule of §4.1 is satisfied by construction in every arrangement: a board's files
have exactly one process writing them, and `taskdeck-server` refuses to start if another TaskDeck
holds the folder's lock.

### 22.2 `taskdeck-server`

A second binary from the same crate (`[[bin]] taskdeck-server`, `src/server_main.rs`). It links
the one library crate, so the build compiles egui, wgpu and winit like the desktop's does — but
nothing in the server calls them, the release link drops them, and the result is a quarter of the
desktop's size (3.7 MB against 16) with no graphics driver needed on the box. It was 2 MB until
§23: reading subscribed calendars put an HTTPS client on its reachable path for the first time, and
rustls and its crypto are most of the difference. It resolves its data directory exactly as the desktop does (`paths::AppDirs`),
reads `phone_server_port`, `phone_bind_address`, `phone_token` and `selected_colorscheme_id`
from the same `userconfig.toml` (`--port` and `--bind` override the first two for one run), mints
a token on first start, and runs one loop: take a request from the phone
server's queue, answer it — queries from the board, commands through `apply` — and publish the
version if anything changed. Everything the phone view is (§21) it serves unchanged, because
§21's `phone.rs` was written against the board and a `Wake` callback rather than against
`TaskApp` and a winit proxy. It stops on `SIGTERM` like any service; the saves inside `apply` are
atomic, so a stop at any instant leaves the files whole. `--print-link` prints the data directory
and the phone and feed links without taking the lock or binding the port, so it can be run beside
the service to read its link. It also draws the first link as a QR code, so a phone camera can take
it off the screen rather than someone typing a thirty-two character token; that appears only when
stdout and stderr are both terminals, so piping the output stays a clean parseable link and a
`NO_COLOR` environment suppresses it. The startup banner does the same on the same terms, which
means under systemd it never does. It creates no folders either (`AppDirs::locate`, §4), so a look
taken as the wrong user leaves nothing behind that the service cannot then write. `--port N`
overrides the port for one run. The arguments are read as OS strings by a small `parse_args`:
a flag it does not know, a value it cannot read, or an
argument that is not text is refused with the flag named and exit code 2, and `--help` or
`--version` answer before anything after them is judged.

Its power draw is the machine's idle draw; the server itself wakes for milliseconds per request
and blocks the rest of the time. `SERVER.md` is the setup: a headless Linux box, Tailscale, a
systemd unit, a nightly copy of the folder.

### 22.3 The desktop as a client (`sync.rs`)

With `server_url` and `server_token` set, the desktop's board is a **replica**:

- **At startup** it fetches the server's board (`GET /api/board`: items, archive, notes, version)
  and builds its `Board` from that, writing it to its own `taskdeck_data/` as a cache. If the
  server cannot be reached within a few seconds it opens the cache instead — the last board it
  saw — and says so in the error window; it is usable at once and edits queue.
- **The first time**, the folder is a board of its own, not yet anyone's replica, and writing the
  server's board over it is how a calendar gets lost — `server_url` set before the folder was
  copied to the server. So on first contact the desktop **sets the local files aside**, dated
  (`read_at_startup.json.local-20260904-121500` and so on; empty ones are not kept), says so in
  the error window with the way back (copy them to the server, or clear `server_url` and rename
  them), and writes a marker (`.client-of`, holding the server URL) that makes every later start
  a plain refresh. A *first* contact that fails is not treated as "offline": the folder is still
  its own board, so the desktop runs on it and tries again next start, rather than turning it
  into a replica the server would overwrite later. A set-aside that fails halfway is undone —
  what moved is moved back — and the desktop runs on its own files rather than on a folder half
  emptied.
- **Leaving, and coming back.** Clearing `server_url` makes the folder a board of its own again
  (`sync::forget_replica`): the marker goes, an outbox of edits meant for that server is set
  aside rather than replayed against whoever comes next, and items created offline under
  temporary ids are renumbered as the board's own (`Board::adopt_temporaries`). A later return
  to a server is therefore a first contact again, set-aside and all — never a silent overwrite
  of what was done locally in between. Switching to a different server is the same cycle.
- **Every edit applies locally first**, through the same `apply`, so the UI never waits on the
  network; the command is then queued for the server. Online, it is sent within milliseconds; the
  server's version moves; the listener fetches the board; the replica is replaced. The user sees
  their own edit, then the server's confirmation of it, indistinguishably.
- **Two threads, in the house style** (§5.6). The *sender* owns the outbox and flushes it in
  order, retrying every few seconds while the server is unreachable. The *listener* long-polls
  `/api/wait` (§21.2) and, when the version moves, fetches the board and hands it to the UI — but
  never while edits are still waiting to go out, and never a board older than the last reply the
  sender received, so a fetch racing an edit cannot show the board without it. A board that
  arrives but cannot be read — two computers on different builds — is fetched again after a pause
  that doubles up to thirty seconds, not at once: the next wait would answer immediately, the
  version having still moved. Both wake the UI through the same `Wake` the phone server uses; the
  UI drains their events where it drains the phone's queue.
- **The menu bar says where things stand**: `● server` in touch with nothing waiting,
  `↑ 2 sending…`, `○ offline · 3 waiting`; the hover names the server and the last failure. One
  more word is about this disk, not the network: `· outbox not saved` stays until a write of
  `outbox.json` succeeds, whether or not the server is in touch, because an outbox that is not
  being written is an outbox that is empty at the next start. Settings → **SERVER** holds the two
  fields; they take effect at the next start, because the board is fetched and the engine started
  before there is a window, and a restart answers "what happens to the edits in between" honestly
  — the outbox goes with you: on the way out (quit, or the restart button) the engine files what
  it still holds and stops before the process ends (`SyncHandle::shutdown`).

### 22.4 Offline, and why it is safe

Edits made with the server unreachable wait in `taskdeck_data/outbox.json` — commands, in order,
written atomically after every change — and are replayed in order when the server answers again.
Two things make that a replay rather than a merge:

**Commands, not files.** *"Move block 2 of task 17 to 14:30"* applies cleanly to whatever the
server has when it arrives. Nobody's whole board ever overwrites anybody's; a conflict is decided
per command, last writer wins, at replay time — the right trade for one person and two devices.

**Temporary ids.** Something created offline gets an id from a range the server never issues
(`CLIENT_ID_FLOOR`, 2⁶²; `Board::number_from`) — and, at each start, from past the highest such
id the outbox still names (`Outbox::highest_temporary_id`), so an id from the last run is never
handed out again while a remap for it can still arrive. When its creation is replayed the server
answers with the real id; every later queued command that named the temporary one is re-pointed
(`Outbox::remap`, using `Command::item_id` / `set_item_id` — which address an archive row by the
same id, so a task finished and put back offline follows too), the sender remembers the mapping
for the rest of the run and re-points anything queued after the answer but before the UI drained
it, the replica's item is renumbered, and the UI's selection, title editor, confirmations and an
in-flight drag follow it — so a block named right after it was made, offline, is still the block
under the editor when it comes back as #29.

A command the server refuses — a ✓ for a task that was finished from the phone meanwhile, a block
of an item since deleted — is **dropped and reported** in the error window (*"a change made here
could not be applied on the server — complete #13: That item is no longer on the board (410)"*),
never retried forever and never lost in silence. Only three things keep a command queued: a
network failure; a key the server does not accept (`401`/`403`) — that is a setting to fix, not
an edit to lose, so the sender goes offline with *check the server key* and tries again; and any
`5xx` — the server's own trouble: shutting down, its board not answering within eight seconds, its
disk refusing a save — which is treated as unreachable. The phone page (§21.5) draws the same
three lines. The replay was verified live, more than once: edits queued against a stopped server,
among them one the server had to refuse, replayed in order on restart, the created item taking its
real id and its rename following it, the desktop ending with the server's exact board
(`CODE_REVIEW.md` has each run).

**Once, not at least once.** A reply can be lost — a timeout, a reset, a `503` from a board that
answered after the eight seconds — and a client that retries would then apply the command twice:
two *Milk* tasks, two blocks. So every command travels under a key (`X-TaskDeck-Request`), the
same on every retry of that command, and the serving side keeps its last thousand replies by key
(`phone::Replies`): a repeat is answered with the first reply and the board hears nothing.
The key is generated when the command is queued (`sync::SyncHandle::queue`, the page's `send`)
and kept in the outbox with it, so a replay after a restart is a repeat too. When the board has
not answered within the eight seconds a worker waits, the key is not freed: the worker parks its
reply channel with the key, the client hears `503`, and a repeat is answered *still working on
that change* (`503` again) until the board's late answer arrives — at which point that answer is
the reply. The outcome of a timed-out command is unknown, not "not applied", and the code says so.

**Two things the replay cannot mend.** A session is addressed by its position in a task's list
(`MoveBlock { session }`), so an edit queued against block 1 while the server, meanwhile, removed
block 0 lands on what is now block 1 — silently. And a Reflow carries the client's `from`, so it
slides the day as the client saw it, but the blocks it slides are whatever the server has by
then. Both are the price of last-writer-wins per command, and both need two devices editing the
same task's blocks during an outage; they are recorded here rather than solved.

### 22.5 What this deliberately does not do

- **Merge.** Two edits to the same field from two devices resolve last-writer-wins per command.
  Field-level merging is a different, larger machine, and the case it serves — two people editing
  one calendar at once — is not this app's.
- **Switch modes live.** Going from a local board to a server's, or back, is a restart, for the
  reason in §22.3.
- **Serve phones from the client.** It can — the client's own phone server still works, and a
  phone edit there is queued to the server like any other — but the phone should be pointed at
  the server, which is on when the desktop is not.
- **Encrypt or authenticate beyond the token.** The transport is Tailscale's; see §21.7.

---

## 23. Subscribed Calendars

### 23.1 The central idea: context, not commitments

A subscription is an https address that answers with an iCalendar file — a secret link from
Google, Apple or a work calendar. What comes back is drawn beside the day and is **never part of
the board**: nothing fetched becomes an `Active`, nothing reaches the archive, nothing can be
edited, and no `Command` creates one.

That line is the whole design, and it is the same argument §21.1 makes in the other direction. The
moment an imported event became an item, this would need identity mapping across refreshes,
tombstones for events deleted upstream, a policy for an edited import, and an archive filling with
things nobody did. Keeping the two apart costs one extra shape and buys the invariant back whole.

What an overlay event *is* for: **"there is a meeting at two, so do not plan work at two."** That is
answered by drawing it, and by `planner::reflow` stepping around it — which needed no new concept,
because reflow already takes anchors and a subscribed hour is exactly one.

### 23.2 Two halves, and why the split matters

- **The list** (`subscriptions::Subscription`) is *authored*. It lives in `subscriptions.json`
  beside the other board files, and it changes only through `Board::apply` — five commands:
  `add_subscription`, `remove_subscription`, `rename_subscription`, `set_subscription_color`,
  `set_subscription_enabled`. So it replicates to a client like every other change.
- **The overlay** (`subscriptions::Overlay`) is *derived*. It is what the addresses last said.
  Losing it costs a refresh, not a calendar. It is cached to `subscribed_cache.json` only so a
  restart draws the calendars it drew before rather than an empty week for ten minutes.

The URL is usually the credential as well as the address, which is why it lives with the board
rather than in each machine's `userconfig.toml`, and why the board's folder is the thing to keep
private. Plain `http://` is refused outright: sending that link in clear over a café's network is
the one mistake this can prevent for free. `webcal://` is rewritten rather than refused, because
that is what a person will paste.

### 23.3 Who fetches

**Whichever process owns the board** — the same rule as everything else here.

| This copy | Fetches? | Where its overlay comes from |
|-----------|----------|------------------------------|
| A desktop on its own board | yes | its own `subscriptions::Feeds` thread |
| `taskdeck-server` | yes | the same |
| A desktop that is a client (§22.3) | **no** | with the board, in `BoardState` |
| The phone | **no** | in the snapshot |

Two fetchers would be two clocks, two copies of a secret address, and two answers to what is on
Tuesday. It is expressed in the type rather than in a branch: `TaskApp::calendars` is an `Option`,
and it is `None` exactly when `sync_handle` is `Some`.

The fetching is on its own thread in the weather pattern of §5.6, for the same reason: the board
loop answers one request at a time, and a hang on somebody's slow calendar server must not be its
problem. Adding this put the **first outbound network call** in `taskdeck-server`, which until now
made none at all.

### 23.4 Rules the code keeps

- **A refresh that changed nothing changes nothing.** `Overlay::digest` is FNV-1a over the sorted
  events and deliberately excludes the fetch status, which carries the time of the last attempt and
  would otherwise make every refresh a change — waking every parked phone and making every client
  refetch the whole board on a ten-minute timer. `Board::adopt_overlay` moves the version only on a
  real difference.
- **A calendar that could not be read keeps the events it last gave.** A server that is down or a
  tunnel that dropped is not somebody cancelling a meeting. The failure shows on the status line at
  the desk; the meetings stay. This is the same refusal to read an outage as a deletion that §22.4
  makes about edits.
- **A span that describes the day is never an anchor.** An all-day event would claim 00:00–24:00
  and leave reflow nowhere to put the day's work; so would anything long enough to amount to the
  same. "I am at a conference" is not the claim "there is a meeting at two", so both are drawn and
  planned straight through — the all-day one as a chip above the timeline, the long one as a
  backdrop behind it. The line is `LONG_EVENT_MINUTES`, **twelve hours**, and it is measured rather
  than reasoned out: a university feed exports a course *period* as an event running 08:00–20:00 on
  every teaching day, because iCalendar gave it nowhere else to put one, and treated as busy that
  made the whole working day unplannable. Across three real feeds — two university timetables and a
  student club's — every genuine appointment runs 90 to 600 minutes and every period marker runs
  exactly 720, with nothing at all in between, so the line goes in the gap. It was six hours first,
  a guess made against one feed, and the club's calendar showed the cost: a board game night from
  16:00 to 22:00 and a tournament from noon to eight were both being drawn as scenery for a day
  that was in fact taken. Past twelve hours there is no morning or evening left to plan into, which
  is the whole reason a span stops being an appointment.
- **A backdrop takes no part in the column packing either.** Subscribed events that overlap each
  other are laid out side by side so both stay readable, but a twelve-hour period marker sitting
  behind three lectures would otherwise squeeze every one of them into half a column.
- **`TRANSP:TRANSPARENT` is drawn and not obeyed.** This app's own feed writes it on a due marker,
  and a household may well subscribe TaskDeck to a calendar TaskDeck feeds; reading our own
  politeness back as somebody's meeting would wall off the day with our own deadlines.
- **Imported events never reach our own feed** (§21.6). Subscribing a calendar app to both would
  otherwise loop.
- **A window, not the whole calendar.** Sixty days back and four hundred forward. The wall shows up
  to ten years, and expanding a daily rule across ten years for every subscription is hundreds of
  thousands of occurrences to hold, to compare on every refresh and to hand to a client.

### 22.6 Opening it quickly

A calendar you pull out of a pocket to check a time is judged on one number: how long from tapping
the icon to seeing the day. On an old phone in battery saver that was five seconds, and all three
causes were ours.

- **The day is painted from `localStorage` before the network is asked.** `load()` used the cached
  day only in its `catch`, so a working-but-slow connection meant waiting out the whole round trip
  in front of a blank screen. It now draws what it knows first and replaces it when the answer
  arrives. No "as of 12:05" banner while a request is in flight — nothing has failed, and saying so
  would be noise; that banner stays the `catch` branch's.
- **The service worker serves the shell from disk rather than the network.** It was network-first,
  which spends a round trip to be handed back a file that only changes when the binary is rebuilt.
  Cache-first with a background refresh means a rebuilt page appears one launch late, which is the
  right way round for something opened to check a time.
- **The icon is cacheable; nothing else is.** 660 KB of the 750 KB a cold open moved was one PNG,
  re-fetched every time because every response was `no-store`. It is part of the program rather
  than part of the board, so it gets a week. The board and anything carrying the token still get
  `no-store`, and the shell is the service worker's to keep, because it knows how to replace it
  safely and a `max-age` does not.

Together: about 750 KB on every open becomes about 5 KB, and the first paint stops waiting for any
of it. The remaining fetch is the day itself, which arrives behind an already-drawn screen.

### 23.5 Where they are drawn

Three surfaces, each honest about whose the event is.

- **The planner's timeline**: a dashed outline with a bar down the left in the calendar's own
  colour, no fill, and no response registered at all — nothing about it can be dragged, tapped or
  ticked off, so nothing about it should invite the attempt. Overlapping ones are laid out side by
  side by `planner::lay_out` among *themselves*, so they never take width from the day's own blocks.
  The text uses the height the block has rather than one line and an ellipsis, with `LOCATION` on
  its own last line. That is not cosmetic: a course feed's summary runs past a hundred characters
  and ends with the room, so an ellipsis eats precisely the part you were looking for.
- **The month grid**: after the day's own items and never instead of them, in whatever room is left
  of the cell's three slots, prefixed `◇` and drawn in the calendar's own **hue** rather than a
  palette index — `PreviewItem::subscribed` carries the override. A lecture is worth knowing about;
  a task is worth doing, and the cell says which is which. The **weight** is still the scheme's,
  taken from its events slot: a subscription's colour is authored opaque because the swatch in the
  settings sheet has to be, and painting it as stored put a solid block among washes the schemes
  hold at a third of that on purpose, so the background photo can show through. One opaque rectangle
  in that company does not read as *somebody else's* — it reads as broken. Under COLORSCHEME ZERO,
  whose six entries are fully transparent, a subscribed event is untinted like everything else.
- **The phone**: the same, laid out server-side so the page draws and never decides, with all-day
  bands as chips above the timeline. A flat ground rather than a fill, and a bar down the left in
  the calendar's colour. It was diagonal stripes for one afternoon; stripes read as *cancelled* or
  *disabled*, which is the wrong thing to say about a lecture you have to be at. Inside the block
  the name is clamped to three lines and the room is a separate, unclampable line under it, so the
  half-width block a clash gives you loses words off the *course title* and never the room. In the
  week list the same event is one row: time, name, room.

The colour is the subscription's own and deliberately does **not** follow the colour scheme: it
says which calendar, not how urgent.

**A room named twice is shown once.** Both real university feeds write the whole itinerary into
`SUMMARY` and then repeat its tail in `LOCATION` — `…Luento - L01 - U4 NORDEA - U142` with
`LOCATION:U4 NORDEA - U142`. Drawing both spends a line saying nothing, so
`without_repeated_location` takes the room off the end of the summary, along with whatever `-`,
`·` or `,` was joining them. Only a suffix, only case-insensitively, and only when a name is left
over: an event whose summary *is* the room keeps it.

### 23.6 The parser (`ics.rs`), and why it is ours

A fetched file is **input nobody in this repository wrote**, so `ics.rs` is built the other way
round from the feed writer it mirrors: everything is bounded before it is read, an unreadable event
is skipped and *counted* rather than argued with, and only a file that is structurally not a
calendar is refused whole. It knows nothing of egui, threads or HTTP, so it can be tested against a
`&str` and nothing else.

It is hand-written, and adds **no dependency**. The survey behind that: `ical` is archived;
`icalendar` is a good tokenizer that reads no `DURATION`, no `VTIMEZONE`, and recurses through
components with no depth limit — a file of repeated `BEGIN:` lines is a stack overflow that
`panic = "abort"` cannot catch; `rrule` carries 174 panic-shaped sites outside its tests and hauls
in `regex` and `chrono-tz`, whose prebuilt table is seven megabytes of generated Rust for a server
binary that is two. And `chrono-tz` would not even answer the question: Outlook writes
`TZID:W. Europe Standard Time`, which is not an IANA name, while the `VTIMEZONE` block RFC 5545
requires the file to carry is right there. So zones are resolved from the file's own definition.

What it reads: folded lines in CRLF or LF, a byte order mark, quoted parameters, `DTSTART`/`DTEND`/
`DURATION`, `LOCATION`, `DESCRIPTION`, `VALUE=DATE` all-day events with RFC 5545's exclusive end,
`EXDATE`, `RDATE`,
`RECURRENCE-ID` overrides and cancellations, `TRANSP`, `X-WR-CALNAME`, and `RRULE` restricted to
`FREQ=DAILY|WEEKLY|MONTHLY|YEARLY` with `INTERVAL`, `COUNT`, `UNTIL`, `BYDAY` (including `-1FR`),
`BYMONTHDAY`, `BYMONTH` and `BYSETPOS`. What it refuses rather than half-honours: `BYYEARDAY`,
`BYWEEKNO`, sub-daily frequencies, and `EXRULE`. A rule it cannot read leaves its event out and
says so — an event drawn on the wrong day is worse than an event not drawn.

Every bound is a counted budget in `ics::Limits`, so the cost is a function of those numbers and
the window, never of what the file claims about itself. The two that matter most: the component
walk is **iterative with a depth cap**, and every date step in the recurrence walk is `checked_`,
because `board.rs` already carries the scar of chrono panicking past its last representable day.

---

*See [`CODE_REVIEW.md`](CODE_REVIEW.md) for an analysis of problems, risks, and suggested improvements.*
