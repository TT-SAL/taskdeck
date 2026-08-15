# TaskDeck

TaskDeck is a desktop calendar that is meant to stay on screen. The idea was to make something that behaves like the calendar hanging on a wall: always visible, easy to read from across the room, and quiet until you actually need it. It runs happily on a spare monitor and winds its own rendering down when you are not using it, so leaving it open all day costs almost nothing.

I keep mine fullscreen on a third monitor.

<img width="1914" height="1076" alt="taskdeck_example_main" src="https://github.com/user-attachments/assets/6c94e2a4-e513-4287-9d75-c2429afe3af0" />

## What it does

The window is laid out in three columns.

**Tasks** sit on the left, ordered by how much they need your attention. A task with a deadline carries a severity — how bad missing it would be — and works its way up the list as the date closes in. A task without one carries a horizon instead: *within days*, *a week*, *a month*, or *whenever*, and ripens over that timescale, so things do not quietly settle to the bottom and get forgotten.

**The calendar** runs down the middle. It scrolls through as many weeks as you ask it to (a handful, or years of them) and animates as you move. Each day shows the events and deadlines that land on it. Click a day and it opens in the planner, where you can see the whole of it and change it.

**Weather and notes** share the right column. You get a two or three day forecast from Open-Meteo, and when you switch the third day off, that space turns into a notepad for whatever you want kept in front of you.

## Opening a day

Click any day on the calendar — or the **Planner** button in the menu, which opens today. Either way you get the same window, which is the only view of a day there is: the date and weekday across the top, everything still waiting for a slot in a tray down the left, and the day itself as a timeline.

Drag on the timeline to block out time; the block appears where you dragged and you name it right there. Double-click instead if half an hour is what you meant. Drag a task in from the tray to decide when you will actually do it. Drag blocks to move them, drag their bottom edge to make them longer. Overlapping blocks sit side by side so a double-booking is obvious, and a red line shows where you are in the day. The **New** switch at the top right decides what a drag makes: a *task* — time set aside to work on something — or an *event*, something that happens at that time.

The tray leads with what is **due by this day and has no time set aside for it** — the things a day plan should start from — and the rest of the backlog follows underneath, most pressing first. There is a field at the top for adding a task without leaving the day: type a name, press Enter, and it is waiting in the tray to be dragged onto an hour.

Click anything, on the timeline or in the tray, and the row along the bottom shows what it is, when it runs, how long it takes, when it is due, and how much it matters — every one of them editable right there — plus buttons to complete it, delete it, book it more time, or send time back. Arrow keys step through the days, `T` goes to today, `Esc` closes, and the confirmation you get before anything is deleted answers to `Enter` and `Esc` too.

**How long something takes** is the task's own number, and the planner spends it. Say the physics homework takes two hours — you can say so before deciding when to do it — and dragging its card out lands a two-hour block, not a default half-hour to be stretched by hand. Book only one of those hours and the card stays in the tray reading *takes 2h · 1h booked*; drag it out again — today, tomorrow, whenever — and it lands the missing hour. A task can be planned in as many blocks across as many days as you like, and it is still one task with one ✓, which frees them all. **＋ Block** in the bottom row books another slice without a trip through the tray.

**When something is due** is set in the bottom row too. `due …` opens a small editor that sets, changes, or clears the deadline of any task — so "due Friday, and I'll do it Tuesday, takes two hours" is one drag and one dialog, in either order. With a deadline the task's knob is *severity* (how bad is missing it — the electric bill is lethal, homework merely high); without one it is the *horizon* (how soon it should roughly happen). The footer always shows whichever question applies.

The important part is what it records. A deadline is when something is **due**; planned blocks are when you will **work on** it. Those are different, so TaskDeck stores them separately: a report due Friday that you plan to write on Tuesday shows as a block on Tuesday and still shows as due on Friday. A task you drag out gets time and no deadline — you have said when you will do it, not when it is owed — and it still rises up the task list as its slot comes round. Dragging blocks around never moves a deadline: a due date is a fact about the task, changed only where changing it looks like changing it.

A few details worth pointing out:

- Drop any image into the `images` folder and pick it as your background from Settings. TaskDeck can also read that image and build a colour palette from it, which it uses to tint the items on the calendar. It ships with a few palettes to start from — `EMBER`, `TIDE`, `MOSS` and `DUSK` — alongside the untinted `COLORSCHEME ZERO` it opens on.
- Set your weather location by clicking it on a world map instead of typing in coordinates. Around two hundred cities are marked to get you close.
- Completed and deleted items are not thrown away. They go to an archive you can page back through.

## Getting started

If there is a prebuilt release on the Releases tab, download and extract it. Otherwise build it yourself (below).

TaskDeck keeps two folders:

- `images/` holds the background pictures you can choose from.
- `taskdeck_data/` holds your tasks, notes, colour schemes, and settings.

Both are created on first run, next to the executable — so keeping the executable in its own folder gives you a self-contained, portable install you can move around. If the executable lives somewhere you are not allowed to write (`/Applications`, `/usr/local/bin`, `C:\Program Files`), TaskDeck falls back to the usual per-user location instead:

| Platform | Fallback location |
|----------|-------------------|
| Windows | `%APPDATA%\TaskDeck` |
| macOS | `~/Library/Application Support/TaskDeck` |
| Linux | `$XDG_DATA_HOME/taskdeck`, or `~/.local/share/taskdeck` |

Set `TASKDECK_HOME` to put the two folders wherever you like instead. Note that they are found relative to the executable, not to the directory you happen to launch from, so starting TaskDeck from a shortcut, the Dock, or another folder all behave the same.

## Building from source

TaskDeck is written in Rust, with egui and wgpu doing the drawing. With a current toolchain installed:

```sh
cargo build --release
```

The executable is written to `target/release`. No system libraries are needed beyond a working graphics driver.

## Settings

Almost everything is adjustable from the in-app Settings panel: the background image and how strongly it is tinted, which monitor the window opens on, fullscreen on or off, how many weeks the calendar covers, the UI scale, your weather location, the two or three day forecast toggle, and an optional frame-rate readout. Your choices are saved to `taskdeck_data/userconfig.toml`.

The layout wants about 1920 points of width, which is what a 1920×1080 monitor gives you at 100% display scaling. On a display that offers less — a HiDPI Mac screen, or Windows at 125% or 150% scaling — the UI scale setting shrinks everything to fit rather than letting the weather column fall off the edge. Left at `0` it works this out for itself; set it to a percentage to pin it.

Fullscreen toggles with `F11`, or `Ctrl`+`Cmd`+`F` on macOS (where the system keeps `F11` for itself).

## Operating system support

TaskDeck runs on Windows, macOS, and Linux from the same source, with no platform-specific build steps. It is developed on Windows 11 and has been built and run on macOS (Apple silicon); the Linux paths are written to the same conventions but have had less exercise, so bug reports are welcome.

On macOS the binary runs as-is. Bundling it as a `TaskDeck.app` also works — TaskDeck detects that it is inside a bundle and keeps your data in `~/Library/Application Support/TaskDeck` rather than writing inside the (possibly signed, possibly read-only) bundle.

## Roadmap

- Scrolling upward to look back over past events.

## Attribution

Weather symbols are from the Yr weather symbols set by Yr / NRK, licensed under the Creative Commons Attribution 4.0 International License (CC BY 4.0).
Source: https://nrkno.github.io/yr-weather-symbols/
License: https://creativecommons.org/licenses/by/4.0/

Images:
- Background photo by Francesco Ungaro (Pexels) — `pexels-francesco-ungaro-1525041.jpg`
- Blue Marble 2002 by NASA (public domain)

Fonts, used under the SIL Open Font License (OFL 1.1):
- Faculty Glyphic — Copyright © The Faculty Glyphic Project Authors
- Anton — Copyright © The Anton Project Authors
- DejaVu Sans — Copyright © DejaVu Fonts
- Lexend Giga — Copyright © The Lexend Project Authors
- Space Mono — Copyright © The Space Mono Project Authors

The full license texts are in the `fonts/LICENSES` directory.
