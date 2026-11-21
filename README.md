# TaskDeck

TaskDeck is a desktop calendar that is meant to stay on screen. The idea was to make something that behaves like the calendar hanging on a wall: always visible, easy to read from across the room, and quiet until you actually need it. It runs happily on a spare monitor and winds its own rendering down when you are not using it, so leaving it open all day costs almost nothing.

I keep mine fullscreen on a third monitor.

<img width="1914" height="1076" alt="taskdeck_example_main" src="https://github.com/user-attachments/assets/6c94e2a4-e513-4287-9d75-c2429afe3af0" />

## What it does

The window is laid out in three columns.

**Tasks** sit on the left, ordered by how much they need your attention. A task with a deadline carries a severity — how bad missing it would be — and works its way up the list as the date closes in. A task without one carries a horizon instead: *within days*, *a week*, *a month*, or *whenever*, and ripens over that timescale, so things do not quietly settle to the bottom and get forgotten.

**The calendar** runs down the middle. It scrolls through as many weeks as you ask it to (a handful, or years of them) and animates as you move. Each day shows the events and deadlines that land on it. A name too long for its card wraps as far as the card goes and then ends in `…`; hover the day to read the names in full. Click a day and it opens in the planner, where you can see the whole of it and change it.

**Weather and notes** share the right column. You get a two or three day forecast from Open-Meteo, and when you switch the third day off, that space turns into a notepad for whatever you want kept in front of you.

## Opening a day

Click any day on the calendar — or the **Planner** button in the menu (or `P`), which opens the day you were last on. Either way you get the same window, which is the only view of a day there is: the date and weekday across the top, everything still waiting for a slot in a tray down the left, and the day itself as a timeline.

Drag on the timeline to block out time; the block appears where you dragged and you name it right there. Double-click instead if half an hour is what you meant. Drag a task in from the tray to decide when you will actually do it. Drag blocks to move them, drag their bottom edge to make them longer. Overlapping blocks sit side by side so a double-booking is obvious, and a red line shows where you are in the day. The **New** switch at the top right decides what a drag makes: a *task* — time set aside to work on something — an *event*, something that happens at that time — or a *routine*, time that is simply spoken for — once, or every week.

The tray leads with what is **due by this day and has no time set aside for it** — the things a day plan should start from — and the rest of the backlog follows underneath, most pressing first. There is a field at the top for adding a task without leaving the day: type a name, press Enter, and it is waiting in the tray to be dragged onto an hour.

Click anything, on the timeline or in the tray, and the row along the bottom shows what it is, when it runs, how long it takes, when it is due, and how much it matters — every one of them editable right there — plus buttons to complete it, delete it, book it more time, or send time back. Arrow keys step through the days, `T` goes to today, `1` `2` `3` pick what a drag makes, `Esc` closes, and the confirmation you get before anything is deleted answers to `Enter` and `Esc` too.

The planner comes back to the day you left it on, so looking something up and returning does not send you back to today.

Naming a new block is typing and `Enter`. If you dragged one out by mistake, `Esc` — it takes the block away with the name, no dialog to answer. On something that already existed `Esc` is gentler: it just puts the old name back.

**Routines** are the third thing the **New** switch makes, and they are for the parts of a day that are simply spoken for: sleeping, cooking, the commute, the gym — and "tomorrow at two I walk the dog", which is the same kind of thing that happens to happen once. Draw one the way you draw anything else and it lands on that day only. The footer's **Repeats** row is where you say which weekdays it should come back on, with **All** for the daily ones and **Once** to take it back to a single day.

A routine is deliberately quiet. It is not in the task list, because nobody is owed it and there is no ✓ that would ever mean anything. It is not on the calendar, because your going to bed is not news. But it *is* on the day, and it is in the day's figure — which is the point. A planner that cannot see the nine hours you were always going to spend on sleep and meals will happily tell you a day with four free hours has thirteen. The masthead counts it separately: `2h 30m planned · 3 blocks · 9h routine`.

Moving or resizing a routine's block changes the rule, so it moves on every day it falls on — you do not reschedule Wednesday's sleep, you change what time you go to bed. Nothing is stored per day, so a routine costs one record however many years you page through.

**When the day runs late**, the masthead says so — *1h 20m behind* — and offers **Reflow** (`R`): everything still booked slides down past now, in the order it was in, around the events and routines that are actually fixed. A plan written in clock times breaks the moment one thing runs long; this is the cheap repair, and the reasoning behind the proper one is in [`DOCUMENTATION.md` §20](DOCUMENTATION.md). The phone view offers the same button.

**How long something takes** is the task's own number, and the planner spends it. Say the physics homework takes two hours — you can say so before deciding when to do it — and dragging its card out lands a two-hour block, not a default half-hour to be stretched by hand. Book only one of those hours and the card stays in the tray reading *takes 2h · 1h booked*; drag it out again — today, tomorrow, whenever — and it lands the missing hour. A task can be planned in as many blocks across as many days as you like, and it is still one task with one ✓, which frees them all. **＋ Block** in the bottom row books another slice without a trip through the tray.

**When something is due** is set in the bottom row too. `due …` opens a small editor that sets, changes, or clears the deadline of any task — so "due Friday, and I'll do it Tuesday, takes two hours" is one drag and one dialog, in either order. With a deadline the task's knob is *severity* (how bad is missing it — the electric bill is lethal, homework merely high); without one it is the *horizon* (how soon it should roughly happen). The footer always shows whichever question applies.

The important part is what it records. A deadline is when something is **due**; planned blocks are when you will **work on** it. Those are different, so TaskDeck stores them separately: a report due Friday that you plan to write on Tuesday shows as a block on Tuesday and still shows as due on Friday. A task you drag out gets time and no deadline — you have said when you will do it, not when it is owed — and it still rises up the task list as its slot comes round. Dragging blocks around never moves a deadline: a due date is a fact about the task, changed only where changing it looks like changing it.

## On your phone

TaskDeck can serve the day to your phone while it runs. Turn on **Phone view** in Settings and
it shows a link (and a QR code); open that on a phone on the same Wi-Fi — or over
[Tailscale](https://tailscale.com) from anywhere — and you get an **agenda** of your days as one
scrolling list over your own background picture, a **day** view as a timeline, the **weather** on
the same hours as your plan, the **tray** of unplanned tasks, and a sheet for whatever you tap — a
row in the agenda, a block on the timeline, a card in the tray. The sheet leads with the verbs:
**✓ Done**, **＋ Book** at the first free hour, **→ Tomorrow**, each one tap with an Undo behind it
rather than a dialog; below them, times and lengths and due dates are rows of chips rather than
pickers. Tray cards carry ✓ and ＋ themselves. Hold a block to drag it to another hour. The one
button under the scrubber cycles the three views. When the day is running late, **Reflow** is
there too, and your notepad is a tap away, to read or to change. Every
change lands in the desktop app the moment you make it, through exactly the same code a drag on
the planner uses, so there is nothing to sync and nothing to merge. Out of reach of the desktop,
an edit made on the page is kept on the phone — shown as waiting, not as done — and sent, in
order, when the desktop answers again; over a secure link (Tailscale's HTTPS, see `SERVER.md`)
the page itself also opens offline and shows the last day it saw, which a plain `http://` LAN
link cannot offer. Add the page to your home screen and it behaves like an app.

**The weather view** is a forecast for the place you picked on the map in Settings, or — tap
*Use my location* — for wherever the phone is. The phone asks Open-Meteo about its own place
itself, and about the desk's place too whenever the desk cannot be reached, so the forecast keeps
updating with the desk off; where the phone is stays on the phone and is never sent to the desk.
It is drawn for a calendar rather than for its own sake.
It opens with the answer in one line: *Rain from 14:00 · during Write the term report*. Under the
temperature there is a chart of the next twenty-four hours — the sky along the top, the
temperature through the middle, the chance of rain rising from the baseline — and beneath it, on
the same hours, **your day**: every block and every event you have booked, so you can see the rain
land on the walk to the shops. Drag along the chart to read any hour exactly. Then sunrise, sunset,
UV and wind, and the week as seven rows on one temperature scale. Tap a day to move the chart to
it. The last forecast is kept on the phone, so the view still reads with the desktop switched off,
and says how old it is.

The same settings section gives you a **calendar feed** link. Subscribe to it from Google
Calendar or your phone's own calendar app and your events, due dates, day plan and routines show
up there too — read-only, and still there when the desktop is off.

The link carries a key: anyone holding it can edit your calendar, so share it like a password.
**New key** in Settings retires every old link.

The phone view exists while TaskDeck is running. If you want it — and your calendar — available
when your computer is off, put the board on a machine that is always on: **`taskdeck-server`** is
the same program with no window, built from this repository, that serves the phone page and the
feed from a spare desktop, a mini PC or a Raspberry Pi. Point the desktop app at it (Settings →
Server) and it keeps working exactly as before over a copy of the server's board, sending every
change to the server the moment it is made — and queuing changes while the server is unreachable,
to send them in order when it is back. [`SERVER.md`](SERVER.md) is the setup, start to finish.

## The archive

Nothing you finish or delete is thrown away. It goes to the **Archive**, which is a record of what became of your work rather than a bin you can look inside.

The reason it can be is the same distinction the planner is built on. TaskDeck knows when something was **due** and, separately, when you set time aside to **do** it — and the moment you tick something off is the only moment those two facts can be checked against each other. So the archive keeps the whole task: its blocks, its estimate, its dates. Each row then says what happened, in one line:

> ✓ **Write the physics homework**
> finished 2d early · 2h estimated, 3h over 2 sittings · 12d on the board

Across the top is what the rows you are looking at add up to — how many you finished, how many you dropped, how many of your deadlines you actually met, how many hours you booked, and how long a thing typically sits on the board before it leaves it. Search by name, narrow to finished or dropped, tasks or events, and the summary follows whatever you are looking at.

Because nothing is lost, anything in the archive can be **put back**: a task you ticked off by mistake returns with its plan, its due date and its estimate intact. **Forget** is there for the rare thing you want gone for good, and it is the only button in TaskDeck that asks twice.

The archive also shows up where the work happened. Open a past day in the planner and the hours you spent are still drawn on the timeline, outlined rather than filled, behind whatever is still live. A day you have finished no longer goes blank.

A few details worth pointing out:

- Drop any image into the `images` folder and pick it as your background from Settings. TaskDeck can also read that image and build a colour palette from it, which it uses to tint the items on the calendar. It ships with a few palettes to start from — `EMBER`, `TIDE`, `MOSS`, `DUSK`, and a `DISCO` with a `MILD DISCO` for the rest of the week — alongside the untinted `COLORSCHEME ZERO` it opens on. Those five are built in and stay as they are; duplicate one to get a copy that is yours to edit.
- Set your weather location by clicking it on a world map instead of typing in coordinates. Around two hundred cities are marked to get you close.

## Keys

One rule: the topmost open thing owns the keyboard. Every window closes on the key that opened it, and on `Esc`. Nothing fires while you are typing, so the notepad is safe.

| | |
|---|---|
| `P` `A` `S` | Planner · Archive · Settings |
| `T` `E` | New task · new event, with the caret already in the name |
| In the planner | `←` `→` days, `T` today, `1` `2` `3` task/event/routine, `Enter` rename, `U` un-book, `R` reflow, `Del` delete |
| In the archive | `/` search |
| `F11` | Fullscreen (`Ctrl`+`Cmd`+`F` on macOS) |

The tasks on the left are ordered by score, with a small random nudge between near-ties so nothing sits permanently fourth and forgotten. That nudge is fixed for the whole day and turns over at midnight — the list holds still while you work, and looks slightly different tomorrow.

## Getting started

If there is a prebuilt release on the Releases tab, download and extract it. Otherwise build it yourself (below).

TaskDeck keeps two folders:

- `images/` holds the background pictures you can choose from.
- `taskdeck_data/` holds your tasks, notes, colour schemes, and settings.

Run one copy at a time. Two copies pointed at the same `taskdeck_data/` each keep their own picture of your tasks and write all of it whenever anything changes, so whichever saves last replaces the other's work — TaskDeck notices and says so at startup rather than letting it happen quietly. If you want the same board on more than one computer, that is what `taskdeck-server` is for: one copy owns the files and the others are its clients (see *On your phone*, and [`SERVER.md`](SERVER.md)).

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

Two executables are written to `target/release`: `TaskDeck`, the app, and `taskdeck-server`, the board with no window for an always-on machine (see [`SERVER.md`](SERVER.md); `cargo build --release --bin taskdeck-server` builds it alone). No system libraries are needed beyond a working graphics driver for the app; the server needs none at all.

## Settings

Almost everything is adjustable from the in-app Settings panel: the background image and how strongly it is tinted, which monitor the window opens on, fullscreen on or off, how many weeks the calendar covers, the UI scale, your weather location, the two or three day forecast toggle, an optional frame-rate readout, an optional frame-rate *cap* for laptops on battery (off by default — the calendar is meant to run as fast as it can), the phone view (on or off, its port, the link and the calendar feed), and the server this copy is a client of, if any. Your choices are saved to `taskdeck_data/userconfig.toml`.

The layout wants about 1920 points of width, which is what a 1920×1080 monitor gives you at 100% display scaling. On a display that offers less — a HiDPI Mac screen, or Windows at 125% or 150% scaling — the UI scale setting shrinks everything to fit rather than letting the weather column fall off the edge. Left at `0` it works this out for itself; set it to a percentage to pin it.

Fullscreen toggles with `F11`, or `Ctrl`+`Cmd`+`F` on macOS (where the system keeps `F11` for itself).

## Operating system support

TaskDeck runs on Windows, macOS, and Linux from the same source, with no platform-specific build steps. It is developed on Windows 11 and has been built and run on macOS (Apple silicon); the Linux paths are written to the same conventions but have had less exercise, so bug reports are welcome.

On macOS the binary runs as-is. Bundling it as a `TaskDeck.app` also works — TaskDeck detects that it is inside a bundle and keeps your data in `~/Library/Application Support/TaskDeck` rather than writing inside the (possibly signed, possibly read-only) bundle.

## Roadmap

- Scrolling upward to look back over past events.
- Making a day plan that survives one thing running long — a plan is "an hour on this today", a schedule is "at 14:00", and the planner currently makes you write the second when you only know the first. The first step, **Reflow**, is in; floating sessions — the structural fix — are next. The reasoning and the order to do it in are in [`DOCUMENTATION.md` §20](DOCUMENTATION.md), and the shape they will take in §20.5.

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
