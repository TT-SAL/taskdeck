# TaskDeck

TaskDeck is a desktop calendar that is meant to stay on screen. The idea was to make something that behaves like the calendar hanging on a wall: always visible, easy to read from across the room, and quiet until you actually need it. It runs happily on a spare monitor and winds its own rendering down when you are not using it, so leaving it open all day costs almost nothing.

I keep mine fullscreen on a third monitor.

<img width="1914" height="1076" alt="taskdeck_example_main" src="https://github.com/user-attachments/assets/6c94e2a4-e513-4287-9d75-c2429afe3af0" />

## What it does

The window is laid out in three columns.

**Tasks** sit on the left, ordered by how much they need your attention. A task can carry a deadline and an importance level, and it works its way up the list as the deadline gets closer. Tasks without a deadline instead build urgency the longer they go unfinished, so things do not quietly settle to the bottom and get forgotten.

**The calendar** runs down the middle. It scrolls through as many weeks as you ask it to (a handful, or years of them) and animates as you move. Each day shows the events and deadlines that land on it; click a day to open it, read everything that is on it, and add new events or tasks for that date.

**Weather and notes** share the right column. You get a two or three day forecast from Open-Meteo, and when you switch the third day off, that space turns into a notepad for whatever you want kept in front of you.

A few details worth pointing out:

- Drop any image into the `images` folder and pick it as your background from Settings. TaskDeck can also read that image and build a colour palette from it, which it uses to tint the items on the calendar.
- Set your weather location by clicking it on a world map instead of typing in coordinates. Around two hundred cities are marked to get you close.
- Completed and deleted items are not thrown away. They go to an archive you can page back through.

## Getting started

If there is a prebuilt release on the Releases tab, download and extract it. Otherwise build it yourself (below).

Keep the executable next to its two folders:

- `images/` holds the background pictures you can choose from.
- `taskdeck_data/` holds your tasks, notes, colour schemes, and settings.

TaskDeck will create these folders when it can, but it needs somewhere it is allowed to write. Running it from a read-only or restricted location, or removing the folders while it is open, can stop it from working.

## Building from source

TaskDeck is written in Rust, with egui and wgpu doing the drawing. With a current toolchain installed:

```sh
cargo build --release
```

The executable is written to `target/release`. Move it next to the `images` and `taskdeck_data` folders before running it.

## Settings

Almost everything is adjustable from the in-app Settings panel: the background image and how strongly it is tinted, which monitor the window opens on, fullscreen on or off, how many weeks the calendar covers, your weather location, the two or three day forecast toggle, and an optional frame-rate readout. Your choices are saved to `taskdeck_data/userconfig.toml`.

## Operating system support

TaskDeck is developed and tested on Windows 11, and the releases target Windows. Most of the code is not tied to any one platform, so a port elsewhere probably would not take much, but it is not something I have done or tested yet.

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
