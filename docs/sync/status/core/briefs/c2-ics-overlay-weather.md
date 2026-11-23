# Task C, step 2, `c2-ports`: ICS parser, overlay types, weather report types (a port)

Plan: SYNC_PLAN.md §6.2 item 1 and §7 "Porting". You move today's pure code into the core, unchanged in
what it computes. Step C1 (planner, scoring, archive) is in the working copy.

## Fill (from → to)

| Today, under `/home/user/taskdeck/src/` | Core file | Items (exactly those of core.md §20) |
|---|---|---|
| `ics.rs` lines 1–1417 | `src/ics.rs` | `Occurrence` (`is_busy`), `Parsed`, `Limits` with today's `Default`, `parse(text, from, until, limits, zone)` and every private helper it needs |
| `subscriptions.rs` lines 184–484 | `src/overlay.rs` | `Covered`, `OverlayEvent`, `LONG_EVENT_MINUTES`, `FetchOutcome`, `placeholder_name`, `name_from_url`, `better_name`, `is_unnamed`, `FetchStatus`, `Overlay` and its methods |
| `weather.rs` (the report builder, from line 335; types are already copied) | `src/weather.rs` | `build_report`, `sky_symbol`, `nearest_city` (the three bodies left) |
| `utilities.rs` `detab` | `src/compile.rs` | `trim` and `detab` only (core.md §2: lines 36–39); the rest of `compile` is task D |

Not ported: every fetch, file, thread and cache (`fetch_*`, the feeds thread, `save_overlay`, the weather
thread and its statics), `checked_url` (task D), the subscription list itself (it is `board::Subscription`).

| Read | core.md lines |
|---|---|
| §14.6 the ICS parser, overlay and weather types | 1100–1102 |
| §5.2–§5.4 zones | 147–181 |
| §6.8 `Subscription` | 286–294 |
| §20 `ics`, `overlay`, `weather`, and `compile::{trim, detab}` | 1607–1615; 1708–1753 |

## The only changes to make

1. `ics::parse` takes `zone: &(impl Zone + ?Sized)` and uses it wherever today's parser converts a UTC or
   `TZID` time to `Local` (today's `ics.rs` around lines 740–749, 904, 1218) and where it reads an undefined
   `TZID` as local wall time (CORE-READ-004). It must not read the process zone: `chrono::Local` does not
   exist in the library build. Everything else is today's parser, limits included.
2. `FetchStatus.at` is `chrono::DateTime<chrono::FixedOffset>` (today `DateTime<Local>`). Its serde form stays
   RFC 3339 with the offset, so today's `subscribed_cache.json` reads and writes unchanged in both directions:
   prove it with a test over `fixtures/boards/small/subscribed_cache.json` (embedded with `include_str!`).
3. Where today's code names `board::NAME_MAX_CHARS`, `utilities::detab`, `utilities::days_in_month` or
   `subscriptions::Subscription`, use a private constant (200), `crate::compile::detab`, a private copy, and
   `crate::board::Subscription`.
4. `compile::trim` removes exactly the 25 White_Space code points of core.md §2 from both ends, by an explicit
   list (not `str::trim`, whose table follows the toolchain); `compile::detab` replaces each U+0009 by four
   U+0020. Inside the ICS parser keep whatever trimming today's parser does: it is per-device presentation.
5. `Overlay::digest`, `anchors_on`, `events_on`, `sealed`, `covering`, `forget`, `status_for`: today's code,
   byte for byte in what they return (the digest is FNV over the events only).
6. A local time in a fold: `Zone::from_local` gives its instants ascending, and the core takes the earlier
   (core.md §5.4). Today's code took chrono's `.earliest()`, which under chrono 0.4.45's `Local` is the later
   one. Where a ported test expects the later instant of a fold, that is today's quirk: expect the earlier,
   and list the test under `for_lead`. This is the one case where an expected value may change.
7. No function here reads a clock (CORE-GEN-003). If today's code does, in something §20 lists, that is a
   `for_lead` point: say where.

## Tests

- Port today's unit tests of the ported code (`ics.rs` from line 1418, the overlay tests of
  `subscriptions.rs` from 794, `weather.rs` from 1049) into `src/ics/tests.rs`, `src/overlay/tests.rs`,
  `src/weather/tests.rs`: same inputs, same expected values, with an explicit zone where the old test relied
  on the process zone (say which in a comment). Do not change an expected value.
- New: `ics::parse` with two different zones gives the local times each zone implies (a `TZID` event, a UTC
  event, a floating event), including across a daylight-saving transition of a `TableZone`.
- New property test (`tests/ics_props.rs`): `ics::parse` returns and never panics on random and mutated text
  within `Limits::default()`, in bounded time.
- `compile::trim` against each of the 25 code points and against U+200B and U+FEFF, which it must keep.
- Clauses to cite: CORE-READ-004, CORE-COMP-002 (trim), CORE-GEN-003.

## Done

The step gate of `common.md` exits 0 in W; no `unimplemented!()` is left in `ics.rs`, `overlay.rs`,
`weather.rs`, and `compile::{trim, detab}` are filled; you committed in W. Report the commit, what you
ported, what you left out and why, and every `for_lead` point.
