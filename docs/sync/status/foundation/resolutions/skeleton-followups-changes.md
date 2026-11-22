# Exact spec changes for the skeleton follow-ups (items 1 to 23)

Source: `docs/sync/status/foundation/resolutions/skeleton-followups.md`. Item 24 needs no spec change. Where the
skeleton already took a reading, the spec follows the skeleton (that file's opening paragraph), except item 6,
where the skeleton follows the spec. Names and signatures quoted from the skeleton come from
`crates/taskdeck-core/src/`, `crates/taskdeck-seal/src/`, `crates/taskdeck-sync/src/`, `crates/taskdeck-device/src/lib.rs`
and `crates/taskdeck-android/src/lib.rs`. No change touches relay.md or README.md.

## core.md

### TR4-FOLLOW-1 (items 1, 4 and 5)

**Section:** §20 Public API, the read-side block: replace the comment line `// ---------- read side (§14), …` and
the six lines `pub mod planner; … pub mod weather; …` (today lines 1587 to 1593) with the text below, and replace
the `pub mod snapshot { … }` block (today lines 1605 to 1613) with the second text below. `pub mod read` and
`pub mod reminders` stay as they are.

```rust
// ---------- read side (§14), copied modules with the zone parameter; every zone parameter, here and in planner, archive, ics and the snapshot builders, is &(impl Zone + ?Sized) or Z: Zone + ?Sized (§5.2)
// Each module carries exactly the items below: those of §14.2's table and of today's src/, without UI-only items and file I/O.
// "today's fields" and "today's variants" are those of the type of that name in today's src/, with §14.2's changes.
pub mod planner {
    pub const DAY_MINUTES: i32 = 24 * 60;
    pub const SNAP_MINUTES: i32 = 15;
    pub const MIN_BLOCK_MINUTES: u32 = 15;
    pub const DEFAULT_BLOCK_MINUTES: u32 = 30;
    pub enum Placement { /* today's variants */ }
    impl Placement { pub fn start(&self) -> i32; pub fn end(&self) -> i32; }
    pub struct DayPlacement { /* today's fields; session: Option<u64>, a sid */ }
    pub struct Placeable<'a> { /* today's fields, borrowed from an &'a Item */ }
    impl<'a> Placeable<'a> { pub fn of(item: &'a Item) -> Self; }
    pub fn placements_for(item: &Item, day: chrono::NaiveDate, zone: &(impl Zone + ?Sized)) -> Vec<DayPlacement>;
    pub fn placements_of(item: Placeable, day: chrono::NaiveDate, zone: &(impl Zone + ?Sized)) -> Vec<DayPlacement>;
    pub fn appears_on(item: &Item, day: chrono::NaiveDate, zone: &(impl Zone + ?Sized)) -> bool;
    pub struct Lane { /* today's fields */ }
    pub fn lay_out(placements: &[Placement]) -> Vec<Lane>;
    pub struct DaySummary { /* today's fields */ }
    pub fn summarize(placements: &[Placement]) -> DaySummary;
    pub fn summary_text(planned: DaySummary, routine: DaySummary, done: DaySummary) -> String;
    pub fn format_minutes(minutes: i32) -> String;
    pub fn format_duration(minutes: u32) -> String;
    pub fn drop_length_for(item: &Item) -> u32;
    pub fn duration_options(current: Option<u32>) -> Vec<u32>;
    pub enum BacklogGroup { /* today's variants */ }
    pub fn backlog_group(deadline: Option<Instant>, day: chrono::NaiveDate, zone: &(impl Zone + ?Sized)) -> BacklogGroup;
    pub fn relative_due(deadline: Instant, now: Instant) -> String;
    pub struct Flowable<K> { /* today's fields */ }
    pub fn behind_minutes(blocks: &[Placement], now: i32) -> i32;
    pub fn reflow<K: Copy>(blocks: &[Flowable<K>], anchors: &[(i32, i32)], now: i32) -> Vec<(K, i32)>;
}   // now_marker, snap, clamp_block and resolve_on_day are in time, not here (§14.2)
pub mod tasks {
    pub const WEEKDAY_NAMES: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    pub const EVERY_DAY: u8 = 0b0111_1111;
    pub const WEEKDAYS: u8 = 0b0001_1111;
    pub const IMPORTANCE_LABELS: [&str; 5];                       // today's labels
    pub const HORIZON_LABELS: [&str; 4] = ["Within a month", "Within a week", "Within days", "Whenever"];
    pub const HORIZON_DISPLAY_ORDER: [u8; 4] = [2, 1, 0, 3];
    pub const ROUTINE_COLOR_INDEX: usize = 0;
    pub const PALETTE_LEN: usize = 6;
    pub const HORIZON_WHENEVER: u8 = 3;
    pub const WEIGHT_BY_IMPORTANCE: [f32; 5] = [1.0, 2.0, 4.0, 8.0, 16.0];
    pub const LEAD_DAYS_BY_IMPORTANCE: [f32; 5] = [0.5, 1.0, 2.0, 4.0, 8.0];
    pub const WEIGHT_BY_HORIZON: [f32; 4] = [1.0, 2.0, 4.0, 0.5];
    pub const RIPEN_DAYS_BY_HORIZON: [f32; 4] = [30.0, 10.0, 3.0, 90.0];
    pub const PLANNED_LEAD_DAYS: f32 = 0.5;
    pub const OVERDUE_DOUBLING_DAYS: f32 = 1.0;
    pub const OVERDUE_PRESSURE_CAP: f32 = 4.0;
    pub const ASSUMED_IMPORTANCE: u8 = 2;
    pub const MALFORMED_SCORE: f32 = 1.0e6;
    pub const JITTER: f32 = 0.08;
    pub fn weekday_bit(day: chrono::NaiveDate) -> u8;
    pub fn calendar_item_color(is_event: bool, importance: Option<u8>, time_importance: Option<u8>) -> usize;
    impl Recurrence { pub fn falls_on(&self, day: chrono::NaiveDate) -> bool; pub fn repeats(&self) -> bool;
                      pub fn includes(&self, index: u32) -> bool; pub fn summary(&self) -> String;
                      pub fn summary_dated(&self) -> String; }
    impl Item { pub fn importance_score(&self, now: Instant, seed: u64) -> f32;
                pub fn tie_break_jitter(&self, seed: u64) -> f32;
                pub fn is_routine(&self) -> bool;
                pub fn calendar_item_color(&self) -> usize;
                pub fn planner_anchor(&self) -> Option<Instant>;
                pub fn is_planned(&self) -> bool;
                pub fn planned_minutes(&self) -> u32;
                pub fn remaining_minutes(&self) -> Option<u32>;
                pub fn wants_planning(&self) -> bool; }
}
pub mod archive {
    pub enum Timing { /* today's variants */ }
    impl ArchiveRow { pub fn verdict(&self, zone: &(impl Zone + ?Sized)) -> String;
                      pub fn timing(&self) -> Timing;
                      pub fn was_finished(&self) -> bool;
                      pub fn was_judged(&self) -> bool;
                      pub fn mark(&self) -> &'static str;
                      pub fn lifetime(&self) -> chrono::TimeDelta;
                      pub fn booked_minutes(&self) -> u32;
                      pub fn sittings(&self) -> usize;
                      pub fn color_id(&self) -> usize;
                      pub fn is_routine(&self) -> bool; }
    pub fn format_span(span: chrono::TimeDelta) -> String;
    pub fn format_total(minutes: u64) -> String;
    pub struct Filter { /* today's fields */ }
    pub enum OutcomeFilter { /* today's variants */ }
    pub enum KindFilter { /* today's variants */ }
    impl Filter { pub fn admits(&self, row: &ArchiveRow) -> bool; pub fn is_open(&self) -> bool; }
    pub struct Summary { /* today's fields */ }
    impl Summary { pub fn headline(&self) -> String; }
    pub fn summarize<'a>(rows: impl IntoIterator<Item = &'a ArchiveRow>) -> Summary;
    pub struct MonthRun { /* today's fields */ }
    pub fn group_by_month(rows: &[&ArchiveRow], zone: &(impl Zone + ?Sized)) -> Vec<MonthRun>;   // rows newest first
}
pub mod ics {
    pub struct Occurrence { /* today's fields */ }
    impl Occurrence { pub fn is_busy(&self) -> bool; }
    pub struct Parsed { /* today's fields */ }
    pub struct Limits { /* today's fields */ }
    impl Default for Limits {}
    pub fn parse(text: &str, from: chrono::NaiveDate, until: chrono::NaiveDate, limits: Limits,
                 zone: &(impl Zone + ?Sized)) -> Result<Parsed, String>;
}
pub mod overlay {
    pub struct Covered { /* today's fields */ }
    impl Covered { pub fn holds(&self, day: chrono::NaiveDate) -> bool; }
    pub struct OverlayEvent { /* today's fields */ }
    pub const LONG_EVENT_MINUTES: i32 = 12 * 60;
    impl OverlayEvent { pub fn minutes(&self) -> i32; pub fn is_background(&self) -> bool; pub fn is_busy(&self) -> bool; }
    pub enum FetchOutcome { /* today's variants */ }
    pub fn placeholder_name(id: u64) -> String;
    pub fn name_from_url(url: &str) -> Option<String>;
    pub fn better_name(subscription: &Subscription, overlay: &Overlay) -> Option<String>;
    pub fn is_unnamed(subscription: &Subscription) -> bool;
    pub struct FetchStatus { /* today's fields; at: chrono::DateTime<chrono::FixedOffset> (§14.6) */ }
    pub struct Overlay { /* today's fields */ }
    impl Overlay { pub fn covering(self, covers: Option<Covered>) -> Overlay;
                   pub fn sealed(events: Vec<OverlayEvent>, status: Vec<FetchStatus>) -> Overlay;
                   pub fn digest(&self) -> u64;
                   pub fn anchors_on(&self, day: chrono::NaiveDate) -> Vec<(i32, i32)>;
                   pub fn events_on(&self, day: chrono::NaiveDate) -> impl Iterator<Item = &OverlayEvent>;
                   pub fn forget(&mut self, subscription: u64);
                   pub fn status_for(&self, subscription: u64) -> Option<&FetchStatus>; }
}
pub mod weather {
    // The response types that today's code keeps private are public here, so that build_report can be called.
    pub struct WeatherResponse { /* today's fields */ }
    pub struct HourlyData { /* today's fields */ }
    pub struct CurrentData { /* today's fields */ }
    pub struct DailyData { /* today's fields */ }
    pub struct Hour { /* today's fields */ }
    pub struct Day { /* today's fields */ }
    pub struct Now { /* today's fields */ }
    pub struct Report { /* today's fields */ }
    pub fn build_report(json: &WeatherResponse, coordinates: [f32; 2]) -> Report;
    pub fn sky_symbol(code: i32, is_day: bool) -> &'static str;
    pub struct City { /* today's fields */ }
    pub fn nearest_city(latitude: f32, longitude: f32) -> Option<&'static City>;
    pub static CITIES: &[City];                                   // today's table
}
```

```rust
pub mod snapshot {
    pub struct SnapshotInput<'a, Z: Zone + ?Sized> { /* §14.3 */ }
    pub struct Snapshot { /* today's fields, plus notes_base: u64 */ }
    pub struct Look { pub id: String, pub aspect: f32, pub top: String }
    pub struct Dials { pub blur: u32, pub light: u32 }
    pub struct DoneRow { /* today's fields */ }
    pub struct Labels { /* today's fields */ }
    impl Labels { pub fn current() -> Self; }
    pub struct DaySnapshot { /* today's fields */ }
    pub struct SubscribedSnapshot { /* today's fields */ }
    pub struct EntrySnapshot { /* today's fields */ }
    pub struct GhostSnapshot { /* today's fields */ }
    pub struct Tray { /* today's fields */ }
    pub struct TrayCard { /* today's fields */ }
    pub struct ItemDetail { /* today's fields */ }
    pub struct RepeatDetail { /* today's fields */ }
    pub struct SessionDetail { /* today's fields */ }
    pub fn snapshot<Z: Zone + ?Sized>(input: &SnapshotInput<Z>) -> Snapshot;
    pub fn answer_snapshot<Z: Zone + ?Sized>(input: &SnapshotInput<Z>) -> serde_json::Value;
    pub fn build_day(items: &[Item], ghosts: &[(&ArchiveRow, &RowBlock)], day: chrono::NaiveDate, now: Instant,
                     zone: &(impl Zone + ?Sized)) -> DaySnapshot;                       // §14.3
    pub fn build_tray(items: &[Item], day: chrono::NaiveDate, now: Instant, shuffle_seed: u64,
                      zone: &(impl Zone + ?Sized)) -> Tray;                             // §14.3 item 4
    pub fn item_detail(item: &Item, zone: &(impl Zone + ?Sized)) -> ItemDetail;        // §14.3 item 6
    pub fn done_rows<'a>(rows: impl IntoIterator<Item = &'a ArchiveRow>, zone: &(impl Zone + ?Sized)) -> Vec<DoneRow>;   // newest first, §14.3 items 7 and 8
    pub fn with_subscribed(days: Vec<DaySnapshot>, subscriptions: &[Subscription], overlay: &Overlay,
                           zone: &(impl Zone + ?Sized)) -> Vec<DaySnapshot>;             // §14.3 item 11
}
```

### TR4-FOLLOW-2 (item 2)

**Section:** §14.2 The queries, the table. Replace the row

`| `planner::now_marker(day, now)` | `now_marker(day, now, zone)` | yes | §5.5 |`

with these two rows:

| Today | In the core | Zone | Notes |
|---|---|---|---|
| `planner::now_marker(day, now)` | `time::now_marker(day, now, zone)` | yes | §5.5; in `time`, not `planner` (§20) |
| `planner::snap`, `clamp_block`, `resolve_on_day` (planner.rs:296-311 for the last) | `time::snap(m)`, `time::clamp_block(start, minutes)`, `time::resolve_on_day(day, minutes, zone)` | `resolve_on_day` only | §5.4, §5.5; in `time`, not `planner` (§20) |

§20 already has the four functions in `pub mod time` with these signatures; it does not change.

### TR4-FOLLOW-3 (item 3)

**Section:** §12.3 The intents, the table. Replace the `create` row with:

| `op` | Fields (Rust type; default) | Sent by | Compiles to (§13.4) |
|---|---|---|---|
| `create` | `kind` `board::Kind` (JSON `"task"`, `"event"` or `"routine"`); `name` String; `day` date; `start` i32; `minutes` u32 | phone, desk | `create_task`, `create_event` or `create_routine` |

**Section:** §20 Public API, `pub mod intent`. Replace the `Intent` line with:

```rust
    pub enum Intent { /* one variant per row of §12.3, fields as its table; Create { kind: super::board::Kind, name: String, day: chrono::NaiveDate, start: i32, minutes: u32 } */ }
```

### TR4-FOLLOW-4 (item 15)

The code `core/gone` keeps its apply-refusal meaning (§16 "Apply refusals", cited by §9, §10.6 and `bridge.md`
§8.3's `reason(code)`). The compile error for a missing item gets the name `core/item_gone`.

**Section:** §13.4 Per intent, first paragraph. Replace

`"Missing" means no live item in `view` has `id` (`core/gone`, 410 "That item is no longer on the board.").`

with

`"Missing" means no live item in `view` has `id` (`core/item_gone`, 410 "That item is no longer on the board.").`

**Section:** §16 Error catalogue, table "Intent and compile errors": the row of the 410 item code becomes
`core/item_gone` (the whole table, in the README's columns, is in TR4-FOLLOW-5). No other spec cites the compile
code: `bridge.md` §8.3's `reason(code)` and `engine.md` §5.1's `SmRefusal` comment cite the apply refusal and stay.

### TR4-FOLLOW-5 (item 16, with item 15's new name)

**Section:** §16 Error catalogue, table "Intent and compile errors". The paragraph above the table stays. Replace the
table with:

| Code | Meaning (status) | Raised when | What the caller does |
|---|---|---|---|
| `core/intent_json` | the parser's message, e.g. `` not a time: `<text>` `` (400) | §12.2; §13.3 G3 | show it |
| `core/query` | "That is a query, not a command." (400) | `snapshot`, `feed`, `board` | the bridge answers queries itself |
| `core/name_long` | "A name is at most 200 characters." (400) | G2 | show it |
| `core/name_empty` | `add`: "It needs a name."; `quick_add`: "A task needs a name."; `rename`: "A name cannot be empty." (400) | §13.4 | show it |
| `core/event_time` | "An event needs a time." (400) | `add` | show it |
| `core/date_range` | "That date is out of range (1970–9999)." (400) | G4, G5 | show it |
| `core/severity` | "No such severity." (400) | `add`, `set_severity` | show it |
| `core/horizon` | "No such horizon." (400) | `add`, `set_horizon` | show it |
| `core/item_gone` | "That item is no longer on the board." (410) | missing (§13.4) | show it; refresh |
| `core/block_gone` | "That block is no longer there." (410) | `move_block`, `remove_block` | show it; refresh |
| `core/bad_time` | "That time doesn't exist on this day (daylight saving)." (400) | `create`, `move_block`, `add_block` | show it |
| `core/finish_kind` | "Only a task can be finished — an event or a routine is deleted instead." (400) | `complete` | show it |
| `core/block_kind` | "Only a task can be given a block of time." (400) | `add_block` | show it |
| `core/unplan_kind` | "Only a task can be unplanned." (400) | `unplan` | show it |
| `core/length_range` | "A length is between fifteen minutes and a day." (400) | `set_estimate` | show it |
| `core/deadline_kind` | "Only a task has a due date; an event's time is its time." (400) | `set_deadline` | show it |
| `core/severity_kind` | "Only a task has a severity." (400) | `set_severity` | show it |
| `core/horizon_kind` | "Only a task has a horizon." (400) | `set_horizon` | show it |
| `core/repeat_kind` | "Only a routine repeats." (400) | `set_repeat` | show it |
| `core/notes_long` | "Those notes are too long for the notepad." (400) | `set_notes` | show it; keep the draft |
| `core/archive_gone` | "That is no longer in the archive." (410) | `restore`, `forget_archived` | show it; refresh |
| `core/calendars_full` | "That is already 20 calendars, which is as many as this keeps." (400) | `add_subscription` | show it |
| `core/url_empty` | "An address is needed." (400) | `checked_url` | show it |
| `core/url_long` | "That address is too long." (400) | `checked_url` | show it |
| `core/url_http` | "Use https — a calendar link is a password, and http sends it in clear." (400) | `checked_url` | show it |
| `core/url_scheme` | "An address should start with https://" (400) | `checked_url` | show it |
| `core/url_host` | "That address has no host in it." (400) | `checked_url` | show it |
| `core/url_duplicate` | "That calendar is already subscribed to." (400) | `add_subscription` | show it |
| `core/calendar_gone` | "That calendar is no longer subscribed to." (410) | subscription intents | show it; refresh |
| `core/cap_items` | "That is as many items as a board can hold." (400; new) | creating intents, `restore` | show it |
| `core/cap_blocks` | "That is as many blocks as a board can hold." (400; new) | `create` (task), `add_block` | show it |
| `core/cap_archive` | "The archive is full." (400; new) | `complete`, `delete` | show it |
| `core/place` | "That place is off the map." (400; new) | `set_home` | show it |
| `core/picture` | "That picture could not be shared." (400; new) | `set_picture` | a fault of the uploading code: log it |

The message is the text inside the quotation marks, without them; "(new)" in the paragraph above the table now reads
as "new" inside the status parentheses. Replace "except where marked new" in that paragraph with "except where the
status parentheses say new".

## seal.md

### TR4-FOLLOW-6 (item 6)

The parameter is `generation` (`gen` is a reserved word in edition 2024). The skeleton follows the spec here: its
`r#gen` becomes `generation`.

**Section:** §26 Public API, signatures. Replace the two signatures with:

```rust
pub fn login_message(origin: &str, board: &BoardId, dev: &DevId, generation: u32,
                     relay_nonce: &[u8], client_nonce: &[u8; 32]) -> Result<Vec<u8>, SealError>;
pub fn sign_login(keys: &DeviceKeys, origin: &str, board: &BoardId, generation: u32, relay_nonce: &[u8],
                  rng: &mut dyn Entropy) -> Result<([u8; 32], [u8; 64]), SealError>;   // (client_nonce, sig)
```

The wire field of §16.1's layout keeps its name; only the Rust parameter is renamed.

### TR4-FOLLOW-7 (item 7)

**Section:** §26 Public API, blobs and snapshots. Replace the `open_snapshot_declared` signature with:

```rust
pub fn open_snapshot_declared<'a>(parts: &[&[u8]], sha256: &[u8; 32], size: u64, s: u64, writer: &DevId,
                                  roots: &dyn Fn(u32) -> Option<&'a EpochRoot>, board: &BoardId)
                                  -> Result<(SnapshotBody, u32, u64), SealError>;  // §20.5 declared mode: (body, epoch, inflated)
```

### TR4-FOLLOW-8 (item 8)

**Section:** §26 Public API, ids and small values. Replace the `SealError` line with:

```rust
pub enum SealError { /* one unit variant per code of §22, named the CamelCase of the code's name:
                        seal/frame_format is FrameFormat, seal/bad_version is BadVersion, seal/ctl_genesis is CtlGenesis */ }
```

### TR4-FOLLOW-9 (item 9)

**Section:** §17.10 State machines. Replace the first paragraph ("Both roles are sans-IO machines. … passed with each
event.") with:

Both roles are sans-IO machines. Inputs arrive as events; the machine answers with effects. Time is a monotonic
millisecond count (`u64`) passed with the inputs that need it: E's `Start` (the call `Existing::start`), N's
`CodeEntered` and `Tick` on both; no other event carries a time. N's machine is made by `New::new()`, in `Idle`, and
draws nothing until `Slot{A}`. §26 gives the events and effects with their fields.

**Section:** §17.10, table E. Replace the `Idle` row with:

| State | Event | Guard | Effects | Next |
|---|---|---|---|---|
| `Idle` | `Start{membership, head, relay origin, own dev, now}` (the call `Existing::start`) | E is current and unhalted | draw `s`, `esk_E`, `r_E`; `RegisterTicket{tid, slot A}`; `ShowCode{code}` | `Posted` |

**Section:** §17.10, table N. Replace the two `Idle` rows with:

| State | Event | Guard | Effects | Next |
|---|---|---|---|---|
| `Idle` | `CodeEntered{code, host, board_id, door, name, now}` (the relay address as `relay.md` parses it; `name` the new device's name, §5.5) | §17.2 check passes | derive keys; `FetchSlot{tid, ticket_key, A}` | `AwaitA` |
| `Idle` | `CodeEntered` | check fails | `AskAgain{code}` | `Idle` |

**Section:** §17.10, the line after table N. Replace "`start` is the time of `Start` on E and of `CodeEntered` on N."
with "`start` is the `now` of `Start` on E and of the `CodeEntered` that passes the check on N."

**Section:** §26 Public API, pairing. Replace the `pub mod pair { … }` block with:

```rust
pub mod pair {
    pub fn new_code(rng: &mut dyn super::Entropy) -> Result<(PairSecret, String), super::SealError>;
    pub fn parse_code(text: &str) -> Result<PairSecret, super::SealError>;
    pub struct PairSecret { /* secret */ }
    pub struct Existing { /* secret: E's machine, §17.10 */ }
    pub struct New { /* secret: N's machine, §17.10 */ }
    pub enum ExistingEvent {
        SlotB { bytes: Vec<u8> }, UserConfirmed,
        Registered { h: u64, chain: super::Chain, snapshot: super::SnapAnnounce, added_seq: u64, token: String,
                     roots: Vec<(u32, super::EpochRoot)>, host: String, door: [u8; 5] },
        RegisterFailed { code: super::SealError }, Cancel, Tick { now: u64 } }
    pub enum ExistingEffect {
        RegisterTicket { tid: [u8; 16], slot_a: Vec<u8> }, ShowCode { code: String }, PostSlot { slot: Vec<u8> },
        ShowSas { digits: String }, NeedSnapshotAndRegister { device: super::DeviceAdded }, Done { dev: super::DevId },
        DeleteTicket { tid: [u8; 16] }, Abort { code: super::SealError } }
    pub enum NewEvent {
        CodeEntered { code: String, host: String, board_id: super::BoardId, door: [u8; 5], name: String, now: u64 },
        Slot { bytes: Vec<u8> }, SasEntered { digits: String }, Snapshot { parts: Vec<Vec<u8>> },
        Entries { frames: Vec<(u64, Vec<u8>)> }, Frames { frames: Vec<(u64, Vec<u8>)> }, Cancel, Tick { now: u64 } }
    pub enum NewEffect {
        FetchSlot { tid: [u8; 16], ticket_key: zeroize::Zeroizing<[u8; 32]>, label: u8 },
        AskAgain { code: super::SealError }, PostSlot { slot: Vec<u8> }, AskSas, ShowAccepted,
        FetchSnapshot { token: String, blob: super::BlobId, parts: u32 }, ReadLog { token: String, from: u64 },
        FetchFrames { token: String, seqs: Vec<u64> },
        Joined { device_no: u16, keys: super::DeviceKeys, bundle: Bundle, snapshot: super::SnapshotBody },
        Abort { code: super::SealError } }
    pub struct Bundle { pub board_id: super::BoardId, pub host: String, pub door: [u8; 5],
                        pub roots: Vec<(u32, super::EpochRoot)>, pub head: super::Head, pub snapshot: super::SnapAnnounce,
                        pub added_seq: u64, pub token: String }               // slot D, §17.8; ZeroizeOnDrop
    impl Existing { pub fn start(membership: &super::Membership, head: super::Head, relay_origin: &str,
                                 own: &super::DevId, now: u64, rng: &mut dyn super::Entropy) -> (Self, Vec<ExistingEffect>);
                    pub fn on(&mut self, ev: ExistingEvent, rng: &mut dyn super::Entropy) -> Vec<ExistingEffect>; }
    impl New { pub fn new() -> Self;                                          // Idle; draws nothing (§17.10)
               pub fn on(&mut self, ev: NewEvent, rng: &mut dyn super::Entropy) -> Vec<NewEffect>; }
}
```

### TR4-FOLLOW-10 (item 10)

**Section:** §16.2 Admin operations, the table. Replace the `compact` row with:

| `compact` | `u64 below_seq ‖ u64 snap_entry_seq ‖ blob_id[16] ‖ u32 n ‖ n × u64 keep_seq` | 36 + 8n | `blob_id` is the blob the `snap` entry at `snap_entry_seq` announces (its `SnapAnnounce.blob`, §11.5, §26), which the relay records as `compact_snap`; `keep_seq` ascending: the control entries and the recorded Skip targets (§13.1 `skips`) below `below_seq` that stay (`engine.md` §20.2); `below_seq` at most `head_seq − 16,384` (`relay.md` §12.8) |

(The row keeps the table's column count; only the parenthesis changes.)

### TR4-FOLLOW-11 (item 11)

`Genesis.wraps` is a `Vec<[u8; 96]>`, like `EpochRotated.wraps`; §12.2's constraint "exactly two" stays the rule.

**Section:** §26 Public API, entries. After the line `// one struct per item of §12, fields as its table; plus:`
insert:

```rust
pub struct Genesis { pub salt: [u8; 16], pub recovery: RecoveryPublic, pub device: GenesisDevice, pub epoch: u32,
                     pub commit: [u8; 32], pub wraps: Vec<[u8; 96]>, pub rules: u32, pub hash_version: u32,
                     pub state_hash: [u8; 32], pub snapshot: Option<GenesisSnapshot> }
                     // wraps: a Vec, as EpochRotated.wraps; it holds exactly two (§12.2), and one with any other count fails §12.2
pub struct GenesisDevice { pub dev: DevId, pub sign_pk: [u8; 32], pub hpke_pk: [u8; 32], pub name: String }   // §12.2 `device`
pub struct GenesisSnapshot { pub blob: BlobId, pub sha256: [u8; 32], pub size: u64, pub parts: u32, pub inflated: u64 }  // §12.2 `snapshot`
```

### TR4-FOLLOW-12 (item 22)

**Section:** §8 Frame, the clause list. Replace SEAL-FRAME-004 with (same id, the code now named):

- **SEAL-FRAME-004** (seal) A sealer MUST refuse to produce a frame with `ctr = 0`, returning `seal/frame_format` and no frame.

**Section:** §22 Error catalogue. Replace the `seal/frame_format` row with:

| Code | Meaning | Raised when | What the caller does |
|---|---|---|---|
| `seal/frame_format` | not a well-formed frame, or a frame asked for with `ctr = 0` | §14.2 step 1; `seal_frame` with `ctr = 0` (SEAL-FRAME-004) | class `Skip`; tampering alarm (`engine.md`); from `seal_frame`: a fault of the caller, nothing is sealed |

## engine.md

### TR4-FOLLOW-13 (item 12)

**Section:** §37 Public API. Delete the two root lines

```rust
pub struct PendingMark { pub entry_id: [u8; 16], pub provisional_seq: u64, pub op: &'static str,
                         pub target: Option<u64>, pub doomed: Option<&'static str>, pub acknowledged: bool }
pub struct NoticeEntry { pub kind: NoticeKind, pub seq: u64, pub index: u32, pub record: Vec<u8>, pub dismissed: bool }
```

and after the `pub use ui::{…};` statement (which keeps `PendingMark` and `NoticeEntry`) insert:

```rust
// PendingMark and NoticeEntry live in ui and are re-exported above; their fields:
// ui::PendingMark { pub entry_id: [u8; 16], pub provisional_seq: u64, pub op: &'static str,
//                   pub target: Option<u64>, pub doomed: Option<&'static str>, pub acknowledged: bool }
// ui::NoticeEntry { pub kind: NoticeKind, pub seq: u64, pub index: u32, pub record: Vec<u8>, pub dismissed: bool }
```

### TR4-FOLLOW-14 (item 13)

**Section:** §37 Public API, `pub mod sim`. Replace `pub struct FaultSet { /* §31.6 */ }`, `pub struct OpSet { /* §31.5 */ }`
and `pub struct SimStats { /* counts per event and effect kind */ }` with:

```rust
                                     pub struct FaultSet {}   // empty in the skeleton: stage engine names its fields (the faults of §31.6) through a spec change
                                     pub struct OpSet {}      // empty in the skeleton: stage engine names its fields (the operations of §31.5) through a spec change
                                     pub struct SimStats {}   // empty in the skeleton: stage engine names its counts (per event and effect kind) through a spec change
```

### TR4-FOLLOW-15 (item 14)

**Section:** §5.3 The toy state machine, the canonical-bytes table. Replace the `marks` row with:

| Field | Width | Rule |
|---|---|---|
| marks | 10 × `n_marks` | `u16 device_no ‖ u64 n`, strictly ascending by `device_no`, `device_no` from 1 to 8,191, `n` from 1 to 2^40 − 1 |

**Section:** §5.3, the sentence after the table. Replace its start "`load` refuses with `engine/toy_format` any input that
violates this table, an item id …" with "`load` refuses with `engine/toy_format` any input that violates this table
(a mark whose `device_no` is outside 1 to 8,191 or whose `n` is outside 1 to 2^40 − 1 included), an item id …"; the
rest of the sentence stays.

**Section:** §5.3, the paragraph after the commands table. Replace "`target` is the key (as `u64`) or the id." with
"`target` is the key (as `u64`) or the id, and none for `Fail` and `Panic`."

**Section:** §5.3, after the numbered `apply` list (after "`Panic`: calls `panic!` with a constant message."), insert
the paragraph:

Every refusal `apply` returns, whatever its code and step, carries `op = op_name(command)` and
`target = target(command)`, with `base` and `current` absent.

**Section:** §5.3 "Other methods", the `rebind` sentence. After "… and returns the commands with the final `next_n`."
insert: "`rebind` returns `engine/toy_format` when `origin_state` is not exactly 8 bytes, and `engine/toy_bad_id` when
a mint's `n` would exceed 2^40 − 1, and in either case no commands."

**Section:** §33 Error catalogue. Replace the two rows:

| Code | Meaning | Raised when | What the caller does |
|---|---|---|---|
| `engine/toy_format` | toy state bytes invalid, or a toy origin state that is not 8 bytes | §5.3 `load`, `rebind` | simulator only |
| `engine/toy_bad_id` | minted id out of range or not above the mark, or a rebind mint above 2^40 − 1 | §5.3 `apply`, `rebind` | simulator only |

No new clause: ENG-TOY-001 already requires §5.3 exactly.

### TR4-FOLLOW-16 (item 20)

The fake's treatment of a later rules version is stated in engine.md (the fake's spec), not in seal.md, whose §14.3
keeps only the row for version 1 and its sentence that a later version extends the table through a spec change.

**Section:** §6.3 The fake sealer, after the paragraph "The fake has a genesis helper …" insert the paragraph:

`seal.md` §14.3 defines only rules version 1. A fake whose supported rules (TR4-FOLLOW-17) hold a version above 1
treats that version as allowing exactly (`ver`, `suite`) = (1, 1) and state-hash version 1, the values of version 1,
in its `classify` (`seal.md` §14.2 step 8) and in the beat it composes (`seal.md` §11.4).

**Section:** §6.3, the clause list. After ENG-FAKE-002 add:

- **ENG-FAKE-003** (engine) `FakeSealer` MUST treat every supported rules version above 1 as allowing exactly (`ver`, `suite`) = (1, 1) and state-hash version 1, in its receipt checks and in the beats it composes.

### TR4-FOLLOW-17 (item 21)

None of the three is public. `create` takes no time and stamps seq 1 with `at_ms = 0`; a fake made by `create` or
`join` supports the rules `[1]`; the simulator, which is in the same crate, reaches other values through
crate-private functions that §37 does not list.

**Section:** §6.3 The fake sealer, the paragraph "The fake has a genesis helper …": after
"… with the toy empty state;" insert "the plaintext of seq 1 carries `at_ms = 0`;", and at the end of the paragraph
add: "A fake made by `create` or `join` supports the rules `[1]`. The simulator, in the same crate, gives a fake another
genesis time or another list of supported rules through crate-private functions; they are not part of §37's API."

**Section:** §37 Public API, `pub mod fake`. Replace the `impl FakeSealer` lines with:

```rust
                                      impl FakeSealer { pub fn create(seed: u64, name: &str) -> (Self, Vec<u8>, Vec<u8>);   // seq 1 at at_ms = 0; supported rules [1]
                                                        pub fn join(seed: u64, name: &str, bundle: FakeBundle) -> Self; }    // supported rules [1]
                                      // the genesis time and the supported rules have crate-private setters only (§6.3)
```

### TR4-FOLLOW-18 (item 23)

`open_snapshot` keeps `&self` (as the trait and the fake have it); the sealer keeps the opened history behind
shared access.

**Section:** §6.1 The trait, contracts table, row `seal_snapshot`, `open_snapshot`. Replace the sentence
"`open_snapshot` checks the body's history (`seal.md` §20.5 step 7), keeps it for the runtime (`device.md` §15.7), and
returns the material, the sealed header's epoch and the inflated length." with:

"`open_snapshot` checks the body's history (`seal.md` §20.5 step 7), keeps it for the runtime (`device.md` §15.7), and
returns the material, the sealed header's epoch and the inflated length; since it takes `&self`, a sealer keeps that
history behind shared access (interior mutability), and the kept history is what its next `seal_snapshot` and
`export_identity` see."

The trait signature does not change:

```rust
    fn open_snapshot(&self, parts: &[&[u8]], expected: &SnapInfo, writer: &Dev, exact: bool)
        -> Result<(SnapshotMaterial, u32, u64), SealerError>;   // (material, header epoch, inflated length)
```

**Section:** §6.3 The fake sealer, "Control history." paragraph. Replace "The fake keeps in memory one history
record …" with "The fake keeps in memory, behind shared access because `open_snapshot` takes `&self` (§6.1), one
history record …"; the rest of the paragraph stays.

## device.md

### TR4-FOLLOW-19 (item 19)

**Section:** §25 Public API, key stores. Replace the four struct lines and the `AndroidKeyStore` comment (today lines
2107 to 2111) with:

```rust
#[cfg(windows)] pub struct DpapiKeyStore;
#[cfg(windows)] impl KeyStore for DpapiKeyStore {}
#[cfg(target_os = "macos")] pub struct KeychainKeyStore;
#[cfg(target_os = "macos")] impl KeyStore for KeychainKeyStore {}
#[cfg(feature = "test-trust")] pub struct FileKeyStore { pub dir: std::path::PathBuf }
#[cfg(feature = "test-trust")] impl KeyStore for FileKeyStore {}
#[cfg(feature = "test-trust")] pub struct MemoryKeyStore { /* map */ }
#[cfg(feature = "test-trust")] impl KeyStore for MemoryKeyStore {}
// AndroidKeyStore lives in taskdeck-android (§12.5) and implements KeyStore there (bridge.md §21).
```

**Section:** §25, the store and its file layer. Replace the two lines `pub struct OsFiles;` and
`#[cfg(feature = "test-trust")] pub struct CrashFiles { … }` with:

```rust
pub struct OsFiles;
impl FileLayer for OsFiles {}
#[cfg(feature = "test-trust")] pub struct CrashFiles { pub inner: OsFiles, pub crash_at: u64 }
#[cfg(feature = "test-trust")] impl FileLayer for CrashFiles {}
```

**Section:** §25, adapters. Replace the two lines `pub struct CoreMachine { … }` and `pub struct SealSealer { … }` with:

```rust
pub struct CoreMachine { /* over taskdeck-core */ }
pub struct SealSealer { /* over taskdeck-seal */ }
// impl taskdeck_sync::StateMachine for CoreMachine and impl taskdeck_sync::Sealer for SealSealer are stage device's
// to write; the skeleton carries the two structs without them.
```

(The `impl Backend` part of item 19 is under bridge.md, TR4-FOLLOW-22.)

## bridge.md

### TR4-FOLLOW-20 (item 17)

**Section:** §12.4 Dependencies and their verification, the table. Replace the `taskdeck-android` row with:

| Where | Dependencies |
|---|---|
| `taskdeck-android` | `jni` 0.22.4, `serde`, `serde_json`, `base64`, `sha2`, `zeroize`; with `backend-device` (the default feature, §11.1) also `taskdeck-core`, `taskdeck-device`, `chrono` (for `PageFacade::answer_snapshot` and `PageFacade::today`, §21), `rustls-platform-verifier` (for its Android initialiser); all at the workspace's pins |

§21's two `PageFacade` signatures stay as they are (`from: chrono::NaiveDate`, `-> chrono::NaiveDate`).

### TR4-FOLLOW-21 (item 18)

**Section:** §21 Public API. Replace the line `pub mod reminders {                                                          // §15.2` with:

```rust
#[cfg(feature = "backend-device")] pub mod reminders {                       // §15.2; it names taskdeck_core, which the crate has only with backend-device
```

The module's body (`record_json`) does not change.

### TR4-FOLLOW-22 (item 19, the `Backend` part)

**Section:** §21 Public API. After the line `#[cfg(feature = "backend-device")] impl DeviceBackend { … }` insert:

```rust
// impl Backend for EchoBackend (stage android-shell, §6.7) and impl Backend for DeviceBackend (stage android) are
// those stages' to write; the skeleton carries the two structs without them.
```

## For the lead

1. **Item 9, `pair::New`'s constructor.** Neither the spec nor the skeleton has one. Recommendation, written as
   TR4-FOLLOW-9: `New::new() -> Self`, taking nothing, in `Idle`, drawing nothing (every input arrives with
   `CodeEntered`, and §17.10's draws start at `Slot{A}`). The skeleton then adds `New::new()` (clippy's
   `new_without_default` may also ask for `impl Default for New`). TR4-FOLLOW-9 also takes the skeleton's reading
   that `CodeEntered` carries `name`, which I read as the new device's name (§5.5), since slot B's `DeviceAdded`
   needs it. Please confirm that reading.
2. **Item 11, `Genesis.wraps`.** Written as TR4-FOLLOW-11: `Vec<[u8; 96]>`, as the skeleton has and like
   `EpochRotated`, with §12.2's "exactly two" kept as the constraint the parser enforces. The alternative,
   `[[u8; 96]; 2]`, would enforce it in the type but would change the skeleton.
3. **Item 13, `FaultSet`, `OpSet`, `SimStats`.** Written as TR4-FOLLOW-14: empty structs in §37, and stage `engine`
   names their fields through a spec change. No field names are guessed from §31.5 and §31.6.
4. **Item 14, the toy's bounds.** Written as TR4-FOLLOW-15. The refusal fields (`op` and `target` for every code,
   `base` and `current` absent) and `rebind`'s two errors follow the skeleton. The `load` bounds on a mark
   (`device_no` 1 to 8,191, `n` 1 to 2^40 − 1) are the stricter reading, and the toy does not check them yet:
   `taskdeck-sync/src/toy.rs`'s `load` must add the two checks, and `engine-toy.json` gets cases for them.
5. **Item 20, a later rules version in the fake.** Stated in engine.md §6.3 with the new clause ENG-FAKE-003
   (TR4-FOLLOW-16), not in seal.md: seal.md §14.3 stays normative for version 1 only.
6. **Item 21, the fake's time and supported rules.** None is made public (TR4-FOLLOW-17): §37 says that `create`
   uses `at_ms = 0` and that a fake supports `[1]`, and the crate-private `create_at` and `set_supported_rules`
   stay out of the API. If a simulator outside the crate ever needs them, that is a later spec change.
7. **Item 22, `ctr = 0`.** The code is `seal/frame_format` (TR4-FOLLOW-12), as the fake answers: SEAL-FRAME-004's text
   names it, with no new code.
8. **Item 23, `open_snapshot`'s receiver.** It keeps `&self`, and §6.1 and §6.3 say that the sealer keeps the history
   behind shared access (TR4-FOLLOW-18), as the trait and the fake have it. One thing to check: the fake's
   `RefCell` makes `FakeSealer` `!Sync`. If `Engine` or `device.md` ever needs `S: Sealer + Sync`, `SealSealer` would
   need a `Mutex` instead. Nothing I read requires `Sync`.
9. **Item 15, the new name.** I chose `core/item_gone` for the compile error (410), because the apply refusal
   `core/gone` is the one cited elsewhere (`bridge.md` §8.3's `reason(code)`, `engine.md` §5.1). The skeleton has no
   `core/` code strings yet, so nothing in the code changes.
10. **Item 17.** Per that item, `chrono` goes into `taskdeck-android`'s manifest under `backend-device`, and then
    `PageFacade::answer_snapshot` and `PageFacade::today` go into the skeleton.
11. **Item 19, who writes the `Backend` impls.** The item says "stage `device`'s and `android`'s". But bridge.md §6.7
    gives the echo backend to `android-shell`, so TR4-FOLLOW-22 gives `impl Backend for EchoBackend` to
    `android-shell` and `impl Backend for DeviceBackend` to `android`. Please confirm, or name `android` for both.
