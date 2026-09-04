# TaskDeck — Code Review: Problems & Suggested Improvements

Companion to [`DOCUMENTATION.md`](DOCUMENTATION.md). This document tracks **problems worth fixing**
and concrete suggestions for each. It is meant to be a working to-do list for hardening the app, not
a description of how it works (that's the documentation's job).

**How to read this:**
- Findings are grouped by category and tagged with a rough priority. Each cites the relevant
  function/symbol rather than a line number, since line numbers drift as the code changes.
- **Intentional design choices are *not* listed here as problems.** Things that look like issues but
  are deliberate (the uncapped/forced-repaint render loop, the single-file `ui.rs`, the hand-tuned
  calendar magic numbers, the random tie-break shuffle in scoring) are documented in
  [`DOCUMENTATION.md` §14](DOCUMENTATION.md). Read that section before "fixing" anything in those
  areas.
- Resolved items are summarized in the changelog at the bottom rather than kept inline, so this list
  stays focused on open work.

---

## A. Correctness & Crash Risks (high priority)

_All items in this section are resolved — see the changelog._

---

## B. Performance & Power (high priority for an "always-on" calendar)

### B4. Per-page archive read is O(n) → O(n²) overall  *(resolved — see the changelog)*

`read_lines_range` re-opened and reverse-scanned the whole `archived.jsonl` past `offset` lines on
every "Show more", and counted *raw* lines while rendering *parsed* ones, so any unparseable line
skipped or duplicated rows across a page boundary.

Both are **retired rather than patched**, as this entry anticipated: the archive redesign removed
line-offset paging entirely. `archive::ArchiveLog` reads the log **once, whole**, and keeps it —
which is not a concession but the enabling move, since search, month grouping and the summary
figures are all impossible against a fifteen-row window onto a file. Unparseable lines are counted,
surfaced in the window, and preserved verbatim on rewrite. See `DOCUMENTATION.md` §17.2.

The half of the old plan that has **not** happened is dissolving the archive into the *calendar
grid*, which was tied to upward scroll (still on the README roadmap). The planner got it instead:
opening a day draws the blocks of archived items behind what is still live (§17.4), which puts the
record where the work happened without needing the grid to scroll backwards.


---

## C. Robustness & Data Integrity (medium)

_All items in this section are resolved — see the changelog._

---

## D. Architecture & Maintainability (medium)

> Note: the single-file `ui.rs` structure and the hand-tuned calendar magic numbers are deliberate —
> see [`DOCUMENTATION.md` §14](DOCUMENTATION.md). The items below are self-contained changes that do
> **not** require splitting the file or touching the animation/widget tuning.

### D6. Modal state as parallel `*_flag` booleans  *(partially resolved)*
`TaskApp` tracks modal state as ~14 independent booleans (the day planner added one). The "any modal
open" disjunctions are now centralized (see changelog), but the booleans themselves remain.
- **Reframed — a flat `enum Modal` is *not* a faithful fix.** Inspecting the render gates shows the
  modals intentionally **nest/stack**: the colour-scheme manager is a stack
  (`color_picker → edit_colorscheme → rename|delete-confirm`, each gated on its child not being open),
  the complete/delete **confirmations render over** the day popup, and **Task+/Event+ open over** the
  day popup. A single `enum Modal { … }` assumes mutual exclusivity and would change that behaviour.
- **Fix (deferred):** model a modal **stack** (e.g. `Vec<Modal>` or a small primary-modal enum plus an
  orthogonal confirmation overlay), not a flat enum. Best sequenced with the UI/archive redesign so the
  modal model is designed against the new screens rather than retrofitted. Until then, `any_modal_open()`
  is the single place that knows the full set.
- **The archive redesign took the first step.** It was the sequencing point named above, and rather
  than adding to the pile (`display_archive_flag` + a forget-confirmation boolean) the archive owns
  an `ArchiveView` struct holding its own open state, filter, selection and pending confirmation.
  Net: three loose `TaskApp` fields removed, none added. Not the stack D6 ultimately wants — the
  other screens still keep their booleans — but it is the shape they should move to, and it is now
  demonstrated in-tree rather than only described here. See `DOCUMENTATION.md` §13.

### D7. Static side-panel/dialog spacers assume a fixed DPI / window size  *(mitigated, not removed)*
The side panels and dialogs are laid out with absolute `add_space` spacers, which won't adapt to
non-100% DPI scaling or arbitrary window sizes.

- **Mitigated:** the *global* consequence — the layout not fitting the window at all — is handled by
  the UI-scale fit (`TaskApp::apply_ui_scale`, `DOCUMENTATION.md` §14.6), which scales the whole UI
  so the fixed design width always fits. That was load-bearing for the macOS port, where no Retina
  display offers 1920 points.
- **Remaining:** the spacers are still absolute, so individual dialogs can't reflow *within* a
  column, and a very small window scales down rather than rearranging. Only worth revisiting if
  freely-resizable use becomes a goal — and even then, leave the calendar **animation/widget** magic
  numbers alone (§14.3); this is about the static dialog layout only.

---

## E. Smaller issues & polish (low)

- **E8. ✅ FIXED — Windows-only assumptions.** See the changelog (and the platform work it pulled in).
- **E10. ✅ FIXED — Deprecated egui layout APIs.** See the changelog.
- **E11. Phone view and server follow-ups** (`DOCUMENTATION.md` §21–22). Open, none urgent:
  - ✅ *resolved* — `TaskApp::execute_phone_command` had no unit test: it is now three lines over
    `Board::apply`, and `board.rs` tests every command against a temporary directory.
  - The desktop **PHONE** and **SERVER** settings sections (link, QR, feed, Open here, the two
    server fields) are verified by reading, not by driving: capturing the egui window needs a
    screen-recording permission the dev environment does not have.
  - On the phone, resizing a block and drawing out a new one go through the sheet; hold-to-drag
    only moves. Both could be layered on without changing anything on the wire.
  - The week strip and week view cost a second seven-day snapshot per change (cached by week
    and version). Measured at 500 items: 5 ms and 160 KB; a lighter entries-only answer is the
    fix if that ever shows.
  - ✅ *resolved* — the server bound every interface with no way to narrow it: `phone_bind_address`
    (and `--bind` on the server) binds one address alone; see the changelog.
  - Five limits found by the session's adversarial reviews and left as they are: a client that
    declares a body and never sends it parks one of the two `tiny_http` workers, which set no
    socket timeout — two such connections stop the phone view until they close (a token-holder or
    LAN peer only; a different HTTP server would be the fix); a hostile `Content-Length` is
    leaked rather than drained (the drain would abort the process), and each leak keeps a
    connection thread and a file descriptor — hundreds of them exhaust the descriptors and stop
    the phone view until it is toggled or the app restarted; sessions are addressed by position,
    so two devices removing blocks of the same task during an outage can land a queued edit on
    the wrong block, silently; a DST gap can nudge one reflowed block onto the next; the archive's
    unreadable-line count stays on the server and is not shown to clients.
  - Three things could not be exercised here and are verified only by reading: the service
    worker's offline shell (the embedded pane refuses to fetch `/sw.js`; it needs a real secure
    origin), the page's `online` event (the pane cannot toggle connectivity), and
    `deploy/install.sh` (no Linux box, no root — `bash -n` and `sh -n` only). Each is small and
    the first real run will say.

---

## F. What the code already does well

Worth preserving — don't regress these while hardening:

- **Atomic file writes** for the critical JSON files (temp file → fsync → persist → fsync the
  directory) — a save is never half-applied, and survives a power cut. Note the boundary: this is
  crash safety, not mutual exclusion between two running copies, which is a separate guard
  (`DOCUMENTATION.md` §4.1).
- **Weather threading** is clean: `RwLock` for data + `AtomicU64` version flag + a command channel
  with graceful `Drop`/`Stop`, plus an `EventLoopProxy` to wake the UI. The UI only re-shapes the
  data when the version actually changes (`last_weather_version`), so it's not re-cloning every
  frame. Backoff-with-retries is a nice touch.
- **Calendar virtualization** keeps a very long calendar cheap to *render*; the model rebuild is now
  O(days + items) via date bucketing as well (B2, resolved).
- **Defensive config loading** with a line-by-line fallback when TOML parsing fails, plus clamping of
  every numeric field.
- **k-means palette generation** in Lab space with a deterministic seed is a genuinely nice feature.
- **Release profile** is thoughtfully tuned for size/speed.

---

## G. Suggested priority order (open items)

The app is a working, complete product; these are hardening steps, ordered by payoff-to-risk.

1. **Linux verification** — the code is written to Linux conventions (XDG data dir, GL/EGL display
   handle, surface-format fallback, rustls so no system OpenSSL) but has not been built or run
   there. Nothing in it is expected to fail; it simply hasn't been exercised. This now matters
   more than it did: `taskdeck-server` is meant to live on a Linux box (`SERVER.md`), and
   `deploy/install.sh` and the systemd unit have only been read, never run.
2. **D6 (remainder)** — model a modal **stack** to replace the remaining `*_flag` booleans (a flat
   enum isn't faithful — see D6). The archive now shows the target shape in-tree.
3. **D7 (remainder)** — reflow within dialogs; the global fit problem is solved by the UI scale.

_(B4, E8 and E10 are resolved.)_

### Not problems, but worth a decision some day

- **Uncapped render loop on a laptop.** *(decided — the optional cap is in; see the changelog.)*
  §14.1 records the uncapped, forced-repaint loop as deliberate, and the reasoning holds for a
  desktop with a spare monitor. On battery the same loop renders flat-out whenever the window is
  focused — measured at ~140–190 fps on an M4 MacBook. `frame_cap_fps` (Settings → Window → Frame
  rate) paces it when set, and is `0` — uncapped — by default.
- **Packaging.** `cargo build --release` produces a bare executable on all three platforms. A macOS
  `.app`, a `.dmg`, or a Linux desktop entry are all outside the build; `paths` already handles the
  bundle case if one is ever made.

---

## Changelog — Resolved

Fixes already landed (newest first). Kept here as history so the open list above stays focused.

- **The re-review of those fixes, and what it found** (four reviewers over the changed files,
  twenty-one candidates, one confirmed by the workflow before its quota ran out, the rest
  re-read by hand):
  - *Temporary ids were reused across launches.* Every start numbered new items from 2⁶² again
    while `outbox.json` — and the sender's remap memory — could still name that very id from the
    last run, so a new item's edits were re-pointed at an old one. The board now numbers on from
    past the highest temporary id the outbox still names (`Outbox::highest_temporary_id`), and a
    folder that becomes a board of its own — local mode, or copied to a server — renumbers any
    temporary items it holds (`Board::adopt_temporaries`), so a server never issues ids in the
    clients' range. (main.rs, server_main.rs, board.rs, sync.rs; tests)
  - *A timed-out command was applied twice.* When the board did not answer within eight seconds
    the request key was released, so the retry — same key — was carried out again while the
    original was still in the queue. The key now stays taken and the worker's reply channel is
    parked with it (`Slot::Late`): a repeat answers `503 still working on that change` until the
    board's late answer arrives, and then that answer is the reply. (phone.rs; tested)
  - *A queued Restore or ForgetArchived was never remapped*, since `Command::item_id` left them
    out; a task finished and put back offline came back as a `410`. Both are addressed like live
    items now. And the desktop stamps a command with its own clock *before* keeping the copy for
    the server (`Command::stamped`), so a phone edit served by a replica carries `at`/`from` too.
  - *A set-aside that failed halfway left an empty board*: the first file already renamed away,
    the folder then opened as "its own". What moved is moved back on failure, and the error names
    anything that could not be. `forget_replica` sets aside an outbox it cannot read rather than
    deleting it.
  - *Smaller:* a client `at` more than a day ahead is capped; a re-filed archive row lands where its
    time puts it, not at the top (`ArchiveLog::record` inserts in order); a Restore whose save and
    re-file both fail keeps the item live in memory and says so; the Escape-undo of a just-made
    block is silent when the block is already gone; a board fetched before a same-frame edit is
    skipped while anything is pending; the pulse thread closes the park under its own lock, so a
    wait can no longer be parked for nobody; `percent_decode` no longer slices a `str` mid-character.
  - *On the page:* a 401 keeps the edit and the gate says so instead of dropping it; snapshot
    room is made before a write and the outbox's room comes first; an answer never overwrites a
    newer cached copy of the same day; a replay whose head cannot be dropped stops instead of
    sending it for ever; the sheet redraw after a replay survives a load that was overtaken.
  Recorded, not fixed: the hostile-length leak is bounded only by file descriptors — a LAN peer
  sending hundreds of such one-shot connections can exhaust them and stop the phone view until it
  is toggled or the app restarted; that is the same trust boundary as the slowloris limit above,
  and a different HTTP server would be the fix for both. 189 tests.
- **An adversarial review of the session's work, and what it found** (eight independent
  reviewers over the uncommitted diff, sixty-three candidates, each re-read by hand; the
  verification stage of the workflow itself ran out of quota, so the verdicts below are this
  author's, with the code as evidence). Fixed, in order of what they would have cost:
  - *A server restart stalled or spun every client.* The board's version counter started at
    zero per process; a client that had seen 900 asked `/api/wait?version=900`, was answered at
    once (`900 != 1`), fetched the board, threw it away as older than its last ack, and asked
    again — two requests a loop, for as long as it took the count to pass 900. The server now
    seeds the counter from the clock (`Board::seed_version`, milliseconds), the sender takes the
    server's number as it comes rather than the maximum, and the listener treats a count that
    went backwards as a fresh start. (sync.rs, server_main.rs, main.rs)
  - *Once, not at least once.* A reply lost to a timeout, a reset, or a `503` from a board that
    answered after eight seconds made both clients re-send a command the server had applied:
    two *Milk* tasks. Every command now travels under `X-TaskDeck-Request`, the same key on every
    retry, generated when it is queued and kept in the outbox with it; the serving side keeps its
    last 256 replies by key (`phone::Replies`) and answers a repeat with the first reply. Verified
    over curl (second POST returns the same reply, one item) and from the page (a queued edit
    carries its key and replays once). Keyless senders are applied as before.
  - *A folder that stopped being a replica stayed one.* Clearing `server_url` left `.client-of`
    and `outbox.json` behind, so a later return to the server overwrote a week of local edits with
    no set-aside and replayed a stale outbox — against a *different* server, too. Local mode now
    removes the marker and sets a non-empty outbox aside (`sync::forget_replica`, said in the error
    window); a first contact sets the outbox aside with the board files; and a set-aside that fails
    halfway no longer marks the folder or writes the server's board over it — the desktop runs on
    its own files and says why. (main.rs, sync.rs; tests)
  - *Notes typed on a client vanished mid-word* when the server's board arrived: `replace` took the
    server's notes while the debounce still held keystrokes. Unsaved local notes now stay; the
    debounced `SetNotes` sends them, last writer wins as everywhere else. (ui.rs)
  - *A `500` from the server dropped the edit.* The server's own I/O failure (`BoardError::failed`)
    was treated as a verdict on the command; every `5xx` is now kept like a `503`, on the desktop,
    the listener and the page. (sync.rs, phone.html; `a_failing_server_keeps_the_edits_as_well`)
  - *A hostile `Content-Length` aborted the process.* `tiny_http` drains an unread body on drop
    with one allocation of the declared size, so `Content-Length: 18446744073709551615` on a public
    route was a capacity-overflow panic under `panic = "abort"`. A declared body over 8 MiB is now
    neither read nor drained nor dropped — the request is leaked, costing that one connection.
    Verified live: the server survives and keeps answering. (phone.rs)
  - *A date off the calendar panicked the board.* `+262142-12-31` parses; adding a day to it does
    not. Every day and instant off the wire is now checked against 1970–9999 (`sane_day`,
    `sane_instant`, `parse_local_input`, `snapshot_command`) before anything adds to it.
    (board.rs, phone.rs; tests)
  - *An unreadable archive was served as an empty one*, and every client then wrote that emptiness
    over its own cache. `Board::state` is a `Result`; `/api/board` answers `500` instead.
  - *A server started over files it could not read* — a folder copied in as root — quarantined the
    calendar and served an empty board in its place. `Board::open` no longer quarantines a
    permission-denied file, and `taskdeck-server` refuses to start over one and names the `chown`.
    `--print-link` reads the settings without creating or normalising the file
    (`read_config_only`), so running it as root before the first start no longer leaves a
    root-owned `userconfig.toml` the service cannot write its key into. (server_main.rs,
    initialization.rs, board.rs, SERVER.md)
  - *`userconfig.toml` was rewritten with a truncating write* — the one file holding the key. Both
    writers now go through a temporary file renamed into place. (initialization.rs)
  - *Reflow compacted the whole day.* The slide started every block at the cursor, so `R` on a day
    with nothing behind pulled the afternoon up to now, contradicting the key's own comment. A block
    ahead now stays where it was put unless one landing in front pushes it on; nothing is pulled
    earlier. (planner.rs; expectations and `reflow_moves_nothing_when_nothing_is_behind`)
  - *A queued Reflow slid the day from the server's clock*, hours later. `Reflow` carries `from`,
    the client's own now, and the server slides as the client did. `Complete` and `Delete` carry
    `at` for the same reason: the archive row a client makes and the one the server makes share a
    key, so a Restore queued offline is not refused for naming a row the server never had.
  - *Restore removed the row before the live set was saved*; a failed save left the item in
    neither file. The row goes back when the save fails. A restored row wearing a temporary id, or
    `u64::MAX`, is renumbered; `next_item_id` saturates. `Forget` of a missing id is `410`, not a
    save. `Add` with nothing to rank by seeds the quick-add's horizon instead of a score the
    scorer calls malformed. (board.rs; tests for each)
  - *Sender and UI disagreed about temporary ids for one frame*: a command queued after the ack
    but before the drain went out under the temporary id and was refused. The sender remembers
    every remap for the run and re-points arrivals, and wakes the UI after a `Remapped`; the drain
    also re-points an in-flight drag and rebuilds the task list. Pending is counted from `queue()`
    rather than from the sender's file, closing the window in which a board without the just-made
    edit could be delivered. (sync.rs, ui.rs)
  - *An outbox that could not be written said nothing.* `Status.storage_error` is sticky until a
    write succeeds and the menu bar says *outbox not saved* whether or not the server is in touch.
  - *Quit lost the last second.* The sync engine is shut down on exit and before a restart: it files
    what it holds and returns without sending (`SyncHandle::shutdown`, tested under three seconds
    against a black hole).
  - *Smaller, all real:* a wait parked after the pulse thread's final sweep would never be answered
    (answered at once now); `HEAD` on the page and the feed answered `404`; a stale bearer header
    in front of the right one hid it; an IPv6 bind produced links without brackets; a hand-set
    token with `+` or `&` broke every link (percent-encoded now); `--help` did not list
    `--version`.
  - *On the page:* Discard during a replay was undone by the replay writing its stale copy back
    (storage is re-read before each drop); two loads in flight could show the wrong day and poison
    the offline cache for another (loads are sequenced, the cache keyed by the answer's own day);
    a redraw during the 350 ms hold left a drag armed for ever (a detached node lets go); the
    sheet's start, length and weekday controls sent values captured when the sheet was drawn
    (read at the tap now); snapshots were kept for every day ever viewed (the last fourteen are);
    a sheet opened before the first load threw (a word instead); Enter and the blur after it could
    rename twice, and Escape could commit on Chrome (settled once).
  - *Docs:* README no longer promises the offline page on a plain-`http` LAN link; §22.2 and
    SERVER.md §3 say the server build compiles the graphics crates and the link drops them, not
    that they are absent; SERVER.md §2 says `tailscale serve` and a tailnet-only bind do not go
    together as written; §4 and §7 add the `chown` a root copy needs; §5 says why the boot log can
    lack the Tailscale link; §7 no longer calls a backup taken mid-completion consistent; §4's
    outbox row names `Queued`; §11.1 counts six sections; §21.5 says *Tray* and three tabs;
    `install.sh` no longer exits 0 with an empty link.
  Recorded, not fixed (E11): a client that never sends its declared body parks a `tiny_http`
  worker with no timeout — two such connections stop the phone view until they close; sessions
  are addressed by position, so two devices removing blocks of one task during an outage can land
  an edit on the wrong block; a DST gap can nudge one reflowed block onto the next; the archive's
  unreadable-line count does not travel to clients. 187 tests.
- **A bind address for the phone view** (`initialization.rs`, `phone.rs`, `server_main.rs`,
  `ui.rs`, E11): `phone_bind_address` in `userconfig.toml` — `"0.0.0.0"` by default, every
  interface as before — binds one address alone when it names one, so a server box that also
  sits on a LAN it should not serve can listen on its Tailscale address only (`SERVER.md` §2).
  `taskdeck-server --bind ADDR` overrides it for one run and refuses anything that is not an
  address (exit 2); a value in the file that is not an address falls back to every interface
  rather than to no phone view (`clean_bind_address`, tested). The links shown — on the sheet,
  in the log, by `--print-link` — follow the bind (`phone::addresses_for`, tested). File only;
  not on the settings sheet, whose status line says *on 127.0.0.1:7373 only* when it is set.
  Verified live: bound to `127.0.0.1`, loopback answers `200` and the machine's LAN address is
  refused (curl exit 7); bound to the default, both answer. 175 tests.
- **Release gate**: `cargo build --release` builds both binaries warning-free — `TaskDeck`
  16.3 MB, `taskdeck-server` 2.0 MB (unchanged from the first measurement, before this
  session's later additions); `--version` and `--help` read as documented.
- **Client-mode drain read, nothing to change** (`ui.rs` `drain_sync_events`): a server board
  arriving while a title editor, a due editor or a confirmation is open on an item the server
  no longer has ends in the same place a phone edit already does — the next gesture on it is a
  `410` shown in the error window, never an edit applied to the wrong item; a `Remapped` event
  re-points every such slot to the real id first; the listener delivers no board while edits are
  pending and none older than the last acknowledged version; `Board::replace` keeps the id
  counter at or above `CLIENT_ID_FLOOR`, so a creation made right after a replace still wears a
  temporary id.
- **The phone's refusal notice survives a reload** (`phone.html`): `state.notice` lived only in
  memory, so a reload before **OK** lost the one trace of an edit made on the phone that is not
  on the board. It is now kept in `localStorage` (`taskdeck-notice`) until acknowledged, and a
  second refusal is appended rather than replacing an unread first. Verified live: an injected
  refused edit, replayed at load, shown after a further reload, cleared from storage by OK.
- **Archive paths read, one gap pinned** (`board.rs`, `archive.rs`): `retire` writes the record
  before removing the item, `Restore` and `ForgetArchived` load the log themselves and answer
  `410` for a row that is gone, `to_active` is the exact inverse of `retire` (rule, kind, sessions,
  estimate all come back), and unreadable lines are counted, never discarded — all already under
  test except the routine round trip and the never-loaded log, which
  `a_routine_comes_back_as_a_routine_and_the_log_loads_itself_for_a_restore` now holds: a deleted
  everyday routine restored on a fresh board is a routine again, back on its days, and forgetting
  a row that is not there is `410` on an unloaded log too. 173 tests. `taskdeck-server`'s startup
  lines were checked against SERVER.md §5's description (data dir, port, item count, a phone
  link per address, the feed path) and match; nothing changed.
- **Floating sessions, designed and deliberately not built** (`DOCUMENTATION.md` §20.5): the
  smallest safe shape — `Session.floating` defaulted, `start` kept as the order key, one
  `Board::implied_day` pass reusing `planner::reflow`, `Float` plus a flag on `MoveBlock` and an
  optional start on `AddBlock` — with the tests that hold each line and the order of work that
  keeps every step green. Not started, on §20.3's own reasoning: the data change is cheap and
  inert, the drawing change is where the decisions are, and those should be made after living with
  Reflow. The desk and the phone both draw through one placement rule today; step 2 of the note
  is what keeps it that way.
- **Names are bounded** (`board.rs`, `phone.html`): nothing on the desktop or the wire limited a
  name — a client could rename a task to 80 KB of text and every calendar cell would try to draw
  it. `NAME_MAX_CHARS` (200, counted in characters) is checked by one helper, `checked_name`, in
  `Add`, `QuickAdd`, `Rename` and `Create`, with one message; the phone page's three name inputs
  carry `maxlength` to match, so the limit is met at the keyboard rather than as an error. The
  desktop's own dialogs go through the same commands and hear the same message.
  `a_name_is_trimmed_and_bounded_everywhere_a_name_comes_in` pins all four ways in. 172 tests.
- **The feed's text escaping, checked** (`phone.rs`): already RFC 5545 §3.3.11 — backslash,
  semicolon, comma escaped, newline as `\n`, CR dropped — and §3.1 folding at 75 octets on
  character boundaries, both under test (`the_feed_writes_every_layer`,
  `long_lines_fold_on_character_boundaries`). Nothing to change.
- **Snapshot cost measured at 500 items** (`taskdeck-server`, debug build, loopback): a board
  of 500 tasks with a block each, built through the API in 7 s (about 14 ms per create, most of
  it the atomic 190 KB save). Then `/api/state` for 1 / 7 / 14 days answered in 4 / 5 / 6 ms
  at 129 / 161 / 197 KB, `/api/board` in 3 ms (135 KB), the feed in 2 ms (93 KB). The size is
  dominated by `items`, which carries every live item in full so the sheet needs no second
  request (§21.4) — at a realistic thirty items that is a few kilobytes, and nothing here is
  worth a lighter endpoint yet. Scratch board kept at `td_perf` in the session scratchpad.
- **A level off the scale is refused at `Add`** (`board.rs`): `SetSeverity` and `SetHorizon`
  checked their level against the label tables, but `Add` — which the desktop's dialogs and any
  client may send — stored whatever `importance` / `time_importance` it was given. Nothing
  panicked — `weight_for` and `calendar_item_color` both clamp, the latter since the severity-9
  incident its comment records — but the item was saved wearing a level no table has: scored at
  the top weight, painted in the event colour, its footer showing the top label for a level it
  does not have.
  Refused with the setters' own messages; `a_level_off_the_scale_is_refused_at_add` pins it.
  171 tests.
- **The request-body cap fits the longest notes** (`phone.rs`): `MAX_BODY_BYTES` was a flat
  64 KiB while the board accepts notes of 20 000 characters — up to 80 KB in UTF-8 — so a long
  non-ASCII note was refused as "too large" (400) by the transport instead of by the board's own
  message. The cap is now sized from `NOTES_MAX_CHARS × 4` plus 4 KiB for the envelope, and
  `the_body_cap_admits_the_longest_notes_the_board_does` serialises 20 000 four-byte characters
  to prove it fits. `SERVER.md` §8 now points at `deploy/install.sh` for an update.
- **A `503` no longer drops a desktop edit** (`sync.rs`): the sender treated every non-auth
  refusal as the server's verdict on the edit and dropped it — including `503`, which is the
  server's own *not now* (`phone::ask`: shutting down, or the board not answering in eight
  seconds) and says nothing about the edit. Now kept and retried like an unreachable server,
  with the menu bar saying why; the listener backs off on a `503` wait the same way instead of
  sleeping the full 30 s "wrong key" pause. The phone page already did this; the two clients now
  draw the same three lines (network, key, busy). The mock server in the tests grew a `Mood`
  (`Normal` / `RefuseKey` / `Busy`), and `a_busy_server_keeps_the_edits_too` pins it. 169 tests.
- **Phone outbox, second look** (`phone.html`): five things the first cut got wrong or left
  unsaid, each verified live against a stopped-and-restarted `taskdeck-server` — (1) a kept edit
  left its control looking applied (the length select on 45m under a heading still saying 1h
  30m); the sheet is now redrawn when an edit is kept, exactly as it is after a success, and the
  item's sheet carries a *1 change to this is waiting to be sent — what is shown here is from
  before it* line (`waitingFor`); (2) after a replay that sent something, an open sheet is redrawn
  once (`replay` reports `sent`), so it shows the board as it now is — seen going from *takes 1h*
  to *takes 45m* with the line gone; (3) a `401` left the sheet over the token field and the
  banner over the gate heading; `showGate` now closes the sheet, hides the banner and says in the
  gate how many edits are kept and go with the first load after the new link — seen with a wrong
  key and one waiting edit, which then went through on the right key; (4) a refusal notice listing
  every refused edit could grow without bound; capped at three plus *and N more*; (5) a browser
  that allows no storage made `queueEdit` say "Kept" about an edit it had lost; `keepOutbox` now
  answers whether it kept, and the toast says the change is lost when it did not. Also, a
  `restore`'s label now uses the archived row's name from the Done list rather than a bare id.
- **`deploy/install.sh`**: SERVER.md §5 as one idempotent POSIX `sh` script — refuses to run as
  non-root or without `systemctl` or a built `target/release/taskdeck-server`; creates the
  `taskdeck` system user and `/var/lib/taskdeck/taskdeck_data` only if missing; never touches
  existing data beyond `chown`; installs binary and unit; `enable --now`, or `restart` if already
  running (an update); then prints the phone link via `--print-link` as the service user, or the
  last 20 journal lines if the service did not stay up. Checked with `bash -n` and `sh -n`; not
  run here (no Linux box, no root) — first real run is on the user's server.
- **Clippy on the new modules**: `board.rs`, `sync.rs`, `phone.rs` and `server_main.rs` carry no
  warnings; the one new line elsewhere that did (`ArchiveLog::replace_with`'s sort) now uses
  `sort_by_key(Reverse(..))`. The 41 remaining warnings are all in pre-existing code (`ui.rs`
  23, `weather.rs` 6, `initialization.rs` 5, …) and are left as they were — collapsible `if`s,
  float precision, same-type casts: a style sweep across the older modules is a diff of its own,
  not a rider on this work.
- **A phone-side outbox** (`phone.html`): an edit the server cannot be reached for (network
  failure or `503`) is kept in `localStorage` (`taskdeck-outbox`) with a label in words, and the
  banner says *N changes waiting to be sent: …* with **Retry** and **Discard**. Nothing is
  applied on the phone — it has no board — so waiting edits are shown as waiting, not done. Every
  `load()` replays the queue first, in order; `send()` flushes older edits before a new one and
  queues the new one behind them if they still cannot go; a refusal is dropped and shown in the
  banner until acknowledged (`state.notice`); `401` stops the replay and shows the gate. No
  temporary ids: a later phone edit can only name an item a snapshot has shown, so nothing can
  refer to a queued create. Verified live against `taskdeck-server`: two edits queued with the
  server stopped (a quick-add and a length change), a third injected that the server had to
  refuse, replayed on Retry — the server holds the new task and the new length, the refusal is
  reported once, the outbox is empty. Not verified: a real phone's `online` event (the pane
  cannot toggle it).
- **Docs re-read against the code** (`DOCUMENTATION.md` §4, §21–22, `README.md`): the phone
  server spawns two workers and a pulse thread, not one; it wakes its owner through `Wake`, not
  the winit proxy; commands go through `Board::apply`, not the old setters, and the phone's
  "complete list" is that `match`; the route table gained `/sw.js` and `/api/board`; times
  resolve on the board owner's clock; the server binary is an eighth of the desktop's size, not a
  third; a refused key keeps edits queued alongside a network failure; the data-directory table
  names the `.client-of` marker; README's Settings paragraph names the Server section.
- **Board edge cases pinned down by tests** (`board::tests`): nothing a command makes runs past
  midnight (a create at 24:00, a move across it, an event lengthened over it, a routine's rule,
  and a length below a quarter hour are all pulled back or refused as the timeline would); a
  pre-id, pre-sessions save opens migrated — id backfilled, the single slot a session, its length
  the estimate — and a corrupt one is quarantined with the message the desktop shows; a restored
  item whose old id is taken gets a fresh one and the counter clears both. 168 tests.
- **A week view on the phone.** The seven-day snapshot the strip's dots already fetch is drawn
  as seven columns at the day's hour scale, laid out by the server (lanes, ghosts, markers) —
  no second placement rule on the page. `W` / the **Week** button toggles; ‹ › step a week; a
  column tap opens the day; the choice is remembered. Verified in the phone-sized pane.
- **The board extracted, a server binary, and the desktop as a client (`board.rs`,
  `server_main.rs`, `sync.rs`; `DOCUMENTATION.md` §22, `SERVER.md`).**
  - **The data and every mutation left `TaskApp`.** `Board::apply(Command)` is the one way the
    data changes; the GUI's setters are one-line wrappers, the phone handler is `apply` plus two
    queries, and the untestable command handler noted under E11 is now covered command by
    command against a temporary directory, reopening it to check the saves (10 tests). Every
    direct field write in `ui.rs` (footer combos, weekday row, rename) became a command.
  - **`taskdeck-server`**: a second `[[bin]]` with no egui/wgpu/winit; `phone.rs` takes a `Wake`
    callback instead of an `EventLoopProxy` so both hosts share it unchanged. Refuses to start on
    a contended data directory rather than warning — a server has no "cannot open my own
    calendar" excuse.
  - **Client mode**: replica fetched at startup (cache on failure), every edit applied locally
    and queued, sender + listener threads, a persisted outbox with temporary ids (≥ 2⁶²) remapped
    on acknowledgement, refusals dropped and reported. The listener never delivers while edits
    are pending nor a board older than the sender's last acked version, which is what stops a
    fetch racing an edit from briefly showing the board without it.
  - **Verified live** with a server and a client on one machine: both directions within ~2 s;
    four offline edits including a create+rename under a temporary id and a completion the
    server had to refuse; replay in order on reconnect with the real id following through and no
    temporary ids left. 162 tests pass.
  - **The first connection cannot lose a calendar.** Pointing a desktop at a server before its
    folder was copied there used to be a silent overwrite of the local board by the server's.
    Now the first successful contact sets the local files aside, dated, and says so with the
    way back; a marker (`.client-of`) makes later starts plain refreshes; and a *failed* first
    contact keeps the folder a board of its own rather than declaring it a replica the server
    would overwrite later. Verified live in all three orders (down → up → up again).
  - **The engine is tested against a stand-in server** (`sync::tests`): a `tiny_http` mock on an
    ephemeral port records what it is sent; the sender is shown to flush in order, remap a
    temporary id onto the acknowledged one in the commands that follow, and tell the UI; the
    listener to deliver the fetched board. A second mock that answers 401 shows a wrong key
    keeping every edit queued and the status saying "check the server key" — the first cut
    treated 401 like any refusal and would have dropped edits over a typo in the settings.
  - **Known gaps:** the SERVER settings section and menu-bar indicator are verified by reading
    (same reason as §21's settings); switching modes needs a restart by design; the phone page
    keeps the last snapshot of a day for reading when the server is unreachable, but does not
    queue edits of its own. The offline **service worker** (`phone_sw.js`, `/sw.js`) is written
    to the standard and served correctly (the page fetches it with the right type), but its
    registration could not be exercised here: the embedded browser pane fails every
    service-worker script fetch with "an unknown error", and no real browser was connected.
    Worth one check on a phone over Tailscale HTTPS: open the page, turn the server off, reopen
    the page — it should show the day as of the last visit.
- **The phone listens instead of polling.** `GET /api/wait?version=N` is *parked* on a `Pulse`
  (mutex + condvar + the parked requests) that `save_active_things`, the notepad save and a
  scheme change publish to; one thread per server answers what is due, so a change at the desk
  reaches the page within a second, a quiet phone costs one request per 25 s, and the wait never
  touches the UI thread. The first cut parked the *worker* instead, and a live test showed the
  consequence: two abandoned waits plus two live ones held all four workers and a command queued
  for the full timeout. Parking the request fixes it; `Drop` interrupts so the pulse thread
  answers everything and leaves, then unblocks each worker (`tiny_http` queues unblocks, so a
  busy worker still finds its own). Tested (`Parked::take_due`, publish/interrupt, the thread's
  exit) and verified live.
- **Review pass over the day's work.** Four fixes: the phone was sent `Color32` bytes, which are
  premultiplied, so every translucent scheme colour arrived darkened (EMBER's amber as brown) — it
  now gets the scheme's own bytes; a phone ✓ or delete dismissed *any* open desktop confirmation,
  now only one asking about the same item (`dismiss_retire_confirmations_for`); `behind_minutes`
  summed overlapping blocks where the planned figure beside it unions them — it clips and unions
  through `summarize` now; and the masthead's wording lived twice (`planner_day_summary`,
  `phone::day_summary_text`) — it is `planner::summary_text`, tested, called from both.
- **An optional frame cap (`frame_cap_fps`, `App::schedule_next_frame`).** The "worth a decision"
  note above, decided as it suggested: a config value, uncapped by default, that defers only the
  self-chasing `request_redraw` (`ControlFlow::WaitUntil` + `new_events`). Input still draws at
  once and the `dt`-driven animations are untouched; §14.1 gained a paragraph saying so. Live in
  Settings → Window, applied on the next frame.
- **Reflow (`planner::reflow`, `DOCUMENTATION.md` §16.8) — §20.3's first move, built.** When today's
  booked work is behind the now-line the masthead says by how much and offers one verb (`R`, and
  the same chip on the phone): slide the remaining work past now, in its existing order, around
  event and routine blocks. Pure and tested (order kept, anchors stepped over including stacked
  ones, off-grid anchor ends snap up, end-of-day clamp, empty anchors ignored). Applied through
  one setter that ends in the usual rebuild + save. Verified live: four blocks moved to 04:30
  onward in order, stepping over a 07:00 routine.
- **A second error no longer overwrites an unread first.** `show_error` appends under the message
  already showing (the open C4 note): the first is usually the explanation of the second, and a
  failed save silently replaced by the failed config write that followed it was the worst case.
  Bounded at `ERROR_TEXT_CAP` with a one-line "more errors followed", the same message twice in a
  row shown once, and the window body now scrolls rather than growing past the screen.
- **The phone view (`phone.rs`, `phone.html`, `DOCUMENTATION.md` §21).** The app serves its own
  day to a phone and takes edits back — through the same setters a desktop gesture uses, so there
  is no second copy of the truth and §4.1's two-writers case is never created. Review-relevant
  notes:
  - **Everything that can be pure is, and is tested:** routing and query parsing, authorisation
    (header / bearer / query, constant-time compare, empty token opens nothing), the command JSON
    shapes, the day snapshot (lane packing over live entries *and* ghosts, the masthead figure),
    the tray split, the ICS feed (every layer, escaping, 75-octet folding, floating local time for
    rules), the manifest, the QR matrix. 14 new tests; 138 pass.
  - **Verified end to end** against a scratch data directory: every command over HTTP, its effect
    on `read_at_startup.json` and `archived.jsonl`, the 400/401/404/410 paths, and the page in a
    phone-sized viewport with sheet-initiated edits round-tripping to the timeline.
  - **Commands are served from `App::user_event`**, not only inside the frame — `handle_redraw`
    bails before the frame when minimized/occluded and the idle sleep stops frames, and a phone
    edit must land in all of those states.
  - **Restarts retry the bind** for a few seconds: the old socket closes on the worker thread a
    moment after the server is dropped, and the worker is deliberately not joined (it may be
    waiting on a reply only the dropping thread can give).
  - **Setters grew day-parameterised cores** (`plan_item_on`, `add_session_on`, `create_item_on`)
    with the desktop wrappers unchanged in behaviour; `remove_session` now clears the block
    selection only when the selection was that task's.
  - **Known gap, same as the planner's:** the *desktop* Settings section (link, QR, feed) is
    verified by reading, not by driving — capturing the egui window needs a screen-recording
    permission the dev environment does not have. The two long notes there use embedded newlines
    inside `settings_note`, which egui wraps as separate lines.
  - **Drag on the phone is hold-to-move only** (verified with synthetic pointer events: lift after
    the hold, live time while moving, saved on release, a plain tap still opens the sheet).
    Resizing and drawing out new blocks stay in the sheet.
  - **Also on the phone:** a week strip with the calendar's three-dot budget per day (one extra
    seven-day request, cached by week and version), the desktop's ranked task list as a second
    tab of the Tasks sheet (`Snapshot::ranked` is `list_tasks` as drawn), the notepad — read,
    and replaced whole with an explicit Save (`Command::SetNotes`, detabbed like the desktop) —
    and a **Done** tab: the archive's last `DONE_ROWS_MAX` rows with their verdicts and a ↩ that
    restores through `restore_archived`, addressed by the archive's own `ArchiveKey` (its
    `DateTime<Local>` round-trips through JSON exactly; tested).
  - **Not done:** a unit test of `execute_phone_command` itself, which lives on `TaskApp` and
    needs the struct constructed (the same reason the weather reshape is untested).

- **Robustness pass, and the difference between atomic and exclusive.**
  - **An out-of-range `importance` crashed the app on startup, unrecoverably.**
    `calendar_item_color` returned `importance as usize` with no clamp, and the calendar indexes the
    six-entry palette with it directly at nine call sites. `importance` is a `u8` straight out of a
    JSON file — the UI writes 0–4, but a hand-edited save (or one from a future build with more
    levels) can hold anything, and a value of `9` panicked on the next frame that drew that day.
    For a calendar that redraws continuously that means it could not be opened at all: quarantining
    a *corrupt* file was handled, a *valid* file with an unexpected number in it was not. The
    scoring tables next door had guarded against exactly this from the start (`weight_for`); the
    colour path had not. Clamped at the source so every caller is safe by construction, with a test
    over all 256 values. Verified by reproduction: panics before, starts after.
  - **Two instances silently overwrote each other.** Every save is atomic, which makes it
    crash-safe and does nothing about a second copy of the program: both keep their own picture of
    the task list and write all of it, so the later save wins and the other's work is gone. An OS
    lock on `taskdeck_data/.lock`, held for the process lifetime, now warns at startup — warns
    rather than refuses, because a false positive means "cannot open my own calendar". See
    `DOCUMENTATION.md` §4.1.
  - **The atomic rename was not durable.** Contents were fsynced and the swap was atomic, but the
    *directory entry* could still be in the page cache when the power went — so the save was lost
    anyway. `tasks::sync_directory` flushes the parent after each `persist` (Unix; NTFS orders it
    for us).
  - **A ✓ could eat the task.** `retire_active_thing` removed the item and *then* wrote the archive
    row, so a failed archive write left the item gone from the live set with no record anywhere. It
    files first and only removes if that worked; a ✓ that says why it did nothing is a far better
    failure than one that quietly loses work.
  - **`unwrap` on a float comparator.** The palette generator sorted clusters with
    `partial_cmp(..).unwrap()`, which panics on a NaN score — and answering "equal" instead would
    trip the sort's own total-order check. `total_cmp` orders every float there is.
  - **The hover tip showed only the three items the cell already showed**, which is the one thing it
    was no use for. It is built from the live set on hover and names the whole day.

- **The archive threw away everything that made it worth keeping (B4, and more).** Rebuilt as
  `archive.rs` + a ledger window + planner ghosts; the whole design is `DOCUMENTATION.md` §17.
  - **The record was a receipt.** `InActive` dropped `sessions`, `duration_minutes` and
    `time_importance` — so the app deleted what you had *intended* at the exact moment it became
    checkable against what happened, and anything put back would have returned as a bare name the
    scorer reads as `MALFORMED_SCORE`. `Archived` keeps the whole item plus an `Outcome`.
  - **Deleting archived nothing.** Only completing wrote a row; a task you abandoned vanished
    without trace, and the README's "completed and deleted items are not thrown away" was simply
    false. One `retire_active_thing` handles both endings now, and the README is true.
  - **B4, both halves.** Line-offset paging is gone rather than fixed: the log is read once, whole,
    and kept. Unparseable lines are counted, shown, and preserved verbatim on rewrite instead of
    being silently dropped and mis-paging their neighbours. `rev_lines` is no longer a dependency.
  - **A legacy event claimed to have been finished.** Rows predating `outcome` default to
    `Finished`, events among them — a real log read back with a dentist appointment wearing a ✓,
    and counted it under "finished". `was_finished()` lets the kind of thing decide first.
  - **New because the record is lossless:** a verdict line per row (deadline vs finish, estimate vs
    booked), summary figures over whatever is on screen, search and filters, **restore**, a
    confirmed permanent **forget**, and ghosts of a day's spent hours on the planner timeline.
  - **D6, one step.** The window's state is an `ArchiveView` struct, not four more `TaskApp` fields.

- **The task list reordered under the pointer, and a planned task scored as if it were new.**
  Both fell out of the scoring rebuild below.
  - **The shuffle was keyed on the clock.** Making the tie-break jitter actually work (see below)
    meant it re-rolled every second — and `planner_backlog_items` re-sorts **every frame**, so the
    tray's cards crawled out from under the pointer while you reached for one. The jitter now takes
    an explicit `TaskApp::shuffle_seed`, a counter bumped once per `summarize_calendar`, so the
    order changes when the list is genuinely rebuilt and holds still otherwise — which is what
    `DOCUMENTATION.md` §14.4 meant all along.
  - **A planned slot now carries pressure.** A task dragged out on the planner gets a slot and no
    deadline, so under the previous model it scored purely on age: blocked out for this afternoon,
    and sitting at the bottom of the list on the very day time had been set aside for it. Pressure
    now also comes from `planned_start` — same curve, short lead (`PLANNED_LEAD_DAYS`), capped at
    1.0 because a plan you didn't keep is not a missed deadline. A dated task takes the **greater**
    of its deadline's and its slot's pressure, so planning something for this morning lifts it this
    morning and a plan can never lower a task. This reverses the "the planner does not affect the
    score" note added a commit earlier; the reasoning is in `DOCUMENTATION.md` §7.
  - **The planner now says what it did.** The inspector spells out `due <when>` or `no deadline`
    for tasks, with a hover explaining that a slot is when you will work on something and a
    deadline is when it is owed. Nothing in the block itself conveyed that, and it was a fair thing
    to be confused by.

- **Priority scoring was inverted, and the branches weren't comparable.** Rebuilt as
  `weight × pressure`; the model, the tables and the resulting numbers are in
  `DOCUMENTATION.md` §7.
  - **The bug.** In the deadline branches, the variable named `days_since_creation` was actually
    days *remaining*, and every curve grew with it. Measured: a "lethally important" task scored
    **633 thirty days out and 26.8 when a week overdue**. Sorted highest-first, that means
    deadlines **sank as they approached** and the most overdue task in the list sat at the bottom —
    the exact opposite of what the README promises. This had presumably been true for a long time
    and is invisible unless you tabulate the curves, which is what caught it.
  - **The scales.** The four branches were mutually incommensurable: linear curves topping out near
    17, exponentials reaching 1e38, and two 1e9 sentinels. Importance 3–4 buried every other task
    regardless of timing, and an undated task's score grew without bound (2659 after 90 days).
  - **The replacement.** One bounded formula for every task. Weights double per importance level, so
    one step of importance is exactly one doubling of time pressure — that is what makes them
    comparable. Dated pressure halves per `lead` days of remaining time, is exactly 1.0 at the
    deadline, and doubles daily once overdue up to a cap; undated pressure ripens towards the weight
    and stops. Maximum real score is 64. Continuous, monotone in time, and it cannot overflow —
    far-future deadlines underflow towards zero instead of saturating to `+inf` (which is what E9
    was patching around; `MAX_SCORE_EXPONENT` is gone).
  - **Constants are the policy**, in two small tables meant to be edited. The lead times also bound
    how long importance out-argues urgency (`lead × log2(weight)`, about a month at the top level);
    if the list ever feels too importance-driven or too deadline-driven, that is the knob.
  - **Tie-break jitter rewritten** — it never shuffled anything. See `DOCUMENTATION.md` §14.4.
  - **Resolution order simplified**: a deadline decides the model whenever there is one, with a
    middling importance assumed if absent. Two previously-`1e9` shapes (dated-without-importance,
    important-without-deadline) are now scored sensibly instead of being pinned to the top as
    "broken"; only a task with no deadline, no importance and no urgency is treated as corrupt.
  - 16 tests cover the invariants — rises toward the deadline, exact weight at the deadline, the
    overdue cap, ripening, cross-model comparisons, finiteness at absurd distances, and that the
    jitter varies per task but can't reorder genuine differences.
  - _Not done: `planned_start` deliberately does not affect the score — when you intend to do
    something isn't how much it matters. If a planned task should rest until its slot, that's a
    policy change to make explicitly._

- **Fixedsys rendered at fractional pixel sizes (long-standing "crisp here, broken there").**
  Fixedsys Excelsior's outlines trace a bitmap font's pixel grid, so it is sharp only when a
  glyph's em box lands on whole pixels — sharpest at multiples of 16px. A ruler rendering one
  string at every size confirmed it: 16.00px and 32.00px crisp, 17.19px and 18.75px smeared. Two
  changes: the **automatic** UI scale is now rounded *down* to a step that makes points-per-pixel a
  multiple of `PPP_QUANTUM` (0.25) — 1.5625 became 1.5, at which every even point size is a whole
  pixel — and `set_styles` runs each named size through `snap_font_points`, which snaps to the
  16px grid when close and to the nearest whole pixel otherwise. `apply_ui_scale` re-runs
  `set_styles` when the scale changes, since the snapped sizes depend on it. An explicit
  `ui_scale_percent` is left alone. Unit-tested (4 tests); details in `DOCUMENTATION.md` §14.6.
  _Note: inline `FontId::new(…)` call sites are not snapped — only the named text styles are. The
  scale quantization helps them all, but exact sizes are only guaranteed for the styles._
- **Planner buttons were unclickable, and planner-created tasks had no importance.**
  The ✓/✗/↩ strip was drawn *inside* the block, and the block's own click-and-drag target is
  registered over the same pixels — egui hit-tests the most recently added widget first, so the
  buttons lost every click. `planner_timeline` now registers all interactions **before** painting,
  so anything drawn afterwards (the in-place title editor) wins its clicks, and the controls moved
  to a `planner_inspector` row under the header, which a 15-minute block had no room for anyway.
  The inspector is also where **importance** (or urgency) is now editable, which a task dragged out
  on the timeline previously had no way to set.
- **A fresh install had no usable colour schemes.** `colorschemes.json` starting empty meant the
  manager opened on `COLORSCHEME ZERO` alone — six fully transparent entries, so it looked broken
  rather than empty. `ColorScheme::builtin_schemes()` adds `EMBER`, `TIDE`, `MOSS` and `DUSK`
  behind it (ZERO stays id 0, so nobody's existing appearance changes). See `DOCUMENTATION.md` §10
  for how the ramps are laid out.
- **Day planner added** (`planner.rs` + `TaskApp::show_planner`). A timeline view of one day with a
  backlog tray, drag-to-create, drag-to-plan, move and resize. Documented in
  [`DOCUMENTATION.md` §16](DOCUMENTATION.md); the notes here are the review-relevant ones.
  - **Model.** `Active` gains `planned_start` and `duration_minutes`, both `#[serde(default)]` so
    save files round-trip through a pre-planner build. `planned_start` is deliberately *not* the
    deadline: due and planned are different facts, and overloading `deadline` would have changed
    what the priority score and calendar mean. Events keep using `deadline` (an event's deadline
    *is* when it happens); the asymmetry is encapsulated in `planner_anchor` / `is_planned`.
  - **Pure core.** Everything easy to get wrong and hard to see in a screenshot — time↔pixel
    mapping, snapping, clamping, the gesture→block arithmetic, overlap packing, the day summary —
    lives in `planner.rs` with no egui dependency and is unit-tested (21 new tests; 42 total).
    `ui.rs` only decides which gesture a press begins.
  - **One source of truth for a drag.** `planner::preview` serves both the live preview and the
    commit-on-release (the commit calls it after `take()`ing the gesture), so what the user sees
    under the pointer cannot disagree with what is saved.
  - **No cached model.** `planner_entries()` rebuilds from `active_things` every frame, so the
    planner cannot drift out of sync with the calendar the way a second copy would. At realistic
    item counts this is free; if the active set ever grows large it is the obvious first thing to
    memoize (it is called twice per frame — once for the header summary, once for the timeline).
  - **Known gap: the gestures are not covered end-to-end.** The arithmetic behind them is, but
    "press here, drag there, release" is only verifiable by hand — synthetic input needs macOS
    Accessibility permission, which the dev environment doesn't have. Worth re-checking by hand
    after any change to `handle_planner_gestures`.
  - **Deliberate: due markers don't drag.** A deadline is a fact about the task; letting a planner
    gesture rewrite it would be a silent data change while the user thought they were planning.

- **E8 — cross-platform build (Windows / macOS / Linux), plus the bugs that hid behind it.**
  Gating the Windows-only calls was a two-line fix; running the result on macOS surfaced five real
  defects, three of which are latent on Windows too. New `DOCUMENTATION.md` §15 collects the
  platform-specific behaviour.
  - **The `cfg` gate itself.** `winit::platform::windows::WindowAttributesExtWindows` and
    `with_taskbar_icon` are now `#[cfg(windows)]`. `embed-resource` and `windows_subsystem` already
    no-op'd elsewhere. `cargo build` and `cargo build --release` are clean on macOS.
  - **DPI applied twice (macOS/HiDPI crash-free but unusable).** `ScaleFactorChanged` called
    `ctx.set_pixels_per_point(scale_factor)`, but that sets egui's *zoom factor*, which `egui-winit`
    multiplies by the native scale factor again — 2× became 4×, and the UI was laid out for a
    quarter of the window. The handler now only reconfigures the surface, and the frame takes its
    points-per-pixel from `full_output.pixels_per_point` for both `tessellate` and the
    `ScreenDescriptor`. `AppState::scale_factor` is gone (it existed only to feed that path).
  - **Input dropped on an abandoned frame → oversized-background panic.** `handle_redraw` took the
    egui input *before* acquiring the surface texture, and every failure arm of the acquire returns.
    Since `take_egui_input` clears what it hands over, an abandoned frame swallowed pending clicks
    and keystrokes — and `max_texture_side`, which is delivered exactly once, on the first take.
    egui therefore kept its 2048 default forever and **panicked on any background image wider than
    2048px** (the repo's own sample image is 3000×2000). macOS reports `Outdated` on the first
    acquire almost every launch; Windows usually doesn't, which is why only images >2048 wide broke
    there. The acquire now happens first. Belt-and-braces, `set_background` downscales anything past
    the limit instead of trusting it.
  - **Window geometry mixed logical and physical units.** The centring maths compared a *logical*
    window size against a *physical* monitor rect, putting the window partly off-screen on any
    HiDPI display, and the surface was sized from the configured (logical) numbers rather than the
    window's actual physical size. Both now work in one space. `window_icon()` also replaces two
    `unwrap()`s on a cosmetic decode, and the monitor lookup falls back primary → first → none
    instead of `available_monitors().nth(0).unwrap()`.
  - **Layout wider than any Retina display.** The fixed three-column layout needs 1920 points; a
    3024px Mac panel is 1512. Handled by scaling the UI to fit rather than re-tuning the widget
    geometry — see `DOCUMENTATION.md` §14.6 and the new `ui_scale_percent` setting (`0` =
    automatic). A 1920×1080/100% Windows setup computes a zoom of exactly 1.0, so it is unchanged.
  - **Also:** `Bgra8Unorm` is now preferred-with-fallbacks rather than `expect`ed (some Linux GL and
    software adapters don't offer it); the wgpu instance is built with the window's display handle
    so Linux GL/EGL can enumerate adapters; `reqwest` uses `rustls-tls` with
    `default-features = false`, so a Linux build needs no system OpenSSL; and `F11` is joined by
    `Ctrl`+`Cmd`+`F` on macOS, which never delivers `F11` to the app.
- **Paths: one resolved root instead of a working-directory/executable mix.** `taskdeck_data` was
  resolved from the executable while `images/` and `userconfig.toml` were resolved from the
  **working directory** — which is the executable's folder only when you double-click on Windows.
  Launched from Finder, the Dock, or a terminal in another folder, the config and backgrounds went
  somewhere else than the tasks. New `paths::AppDirs` resolves one root **once** in `main`
  (`$TASKDECK_HOME` → cargo project root → existing install → writable exe dir → per-user data
  directory), creates both folders, and is threaded explicitly through every reader and writer:
  `read_at_startup`, `oversafe_activesave`, `save_inactive`, `read_lines_range`,
  `quarantine_corrupt_file`, `save_colorschemes`/`read_colorschemes`,
  `save_notepad_text`/`read_notepad_text`, `get_check_and_set_config`, `generate_colorscheme`, and
  `set_background` now take the directory rather than re-deriving it from an exe path. `AppDirs`
  replaces `TaskApp::exe_file_path`, and `AppDirs::image_path` replaces `utilities::safe_image_path`
  (same traversal defence, tests moved with it). `tasks::get_data_dir` is gone. The writability
  probe creates a real temp file, and a macOS `.app` bundle is never written into.
  6 new unit tests (`paths::tests`); 21 pass in total.
- **First-run notepad seeded with invalid JSON.** `read_notepad_text` created a missing
  `notepad_text.json` containing `{}`, then immediately failed to parse it as a `String` — so every
  fresh install opened with "There was something wrong with …notepad_text.json!" as the notepad's
  contents. It now seeds `""`. (The three seed-a-missing-file paths also stopped `expect`-ing on the
  write, which turned an unwritable data directory into a panic.)
- **E10 — deprecated egui layout APIs migrated (no visual/behaviour change).** The 6 remaining build
  warnings are cleared by moving off the deprecated `Ui` methods to the `UiBuilder` API:
  - the 5 `Ui::allocate_ui_at_rect(rect, add)` calls (calendar cell in `show_calendar`, the map area
    and footer in the coordinate picker, the colour swatch in the scheme editor, and the overflow
    button in `calendarwidgets::ButtonHeaderRotated`) → `scope_builder(UiBuilder::new().max_rect(rect), add)`;
  - the 1 `Ui::child_ui_with_id_source(rect, layout, row, None)` (the per-row calendar child) →
    `new_child(UiBuilder::new().id_salt(row).max_rect(rect).layout(layout))`.
  These are **provably equivalent** rather than re-tuned: in egui 0.33 `allocate_ui_at_rect` is
  literally `scope_builder(UiBuilder::new().max_rect(rect), add)`, and the `None` `ui_stack_info`
  resolves to `UiStackInfo::default()` — exactly what `UiBuilder::new()` already carries — so the
  hand-tuned calendar/dialog layout (§14.3) is untouched. Note the deprecation hint pointed at
  `allocate_new_ui`, but that is *itself* deprecated in favour of `scope_builder`, so we target
  `scope_builder` directly to avoid swapping one warning for another. Build is now warning-free;
  `cargo test --lib` still green (17 passed).
- **E2 / E6 / E9 — polish.**
  - **E2:** removed 21 exact-duplicate entries from `CITIES` (`weather.rs`); 289 → 268 unique cities,
    the de-cluttered map shows each once. Verified the unique-name set is otherwise unchanged.
  - **E6:** cleared the unused-binding warnings (the `device_id`/`position` `WindowEvent` destructures
    → `{ .. }`) and did the trivial `ComboBox::from_id_source` → `from_id_salt` rename. Build warnings
    15 → 6; the remaining 6 are egui layout-API deprecations, now tracked as **E10**.
  - **E9:** _(superseded — the exponential curves it clamped no longer exist; see the scoring rebuild
    at the top of this changelog.)_ `importance_score` clamps the exponential exponent to a shared `MAX_SCORE_EXPONENT`, so the
    `1.2^…`/`1.17^…`/`1.15^…` curves saturate to a large **finite** `f32` instead of overflowing to
    `+inf` for far-future deadlines. Unit-tested (`…_stays_finite_for_far_future_deadline`).
- **D6 (partial) — one `any_modal_open()` predicate.** The two hand-maintained "is any modal open"
  flag disjunctions (one gating the calendar tap/drag machine, one clearing the hovered cell) had
  **drifted apart** — and *both* omitted `coordinates_map_flag` and `edit_colorscheme_flag`, so
  calendar input leaked behind the map picker and the colour-scheme editor. Both are replaced by a
  single `TaskApp::any_modal_open()` listing all 13 modal flags, fixing the drift/omissions and giving
  one place to update when a modal is added. The booleans themselves (and the flat-`enum` idea, now
  reframed) remain open under D6.
- **D2 / D4 / E1 — named calendar model, single coordinate field, real bool parse.**
  - **D2:** the opaque `calendar_elements` 6-tuple is now `Vec<DayCell>`, with
    `DayCell { day_number, preview: Vec<PreviewItem>, items: Vec<DayItem>, is_today, date, label }`.
    `show_calendar`, the day popup, and the "find today" lookup all use named fields instead of
    `day.0`/`.2`/`.4`. This is what the coming completed-tasks overlay will extend.
  - **D4:** `TaskApp`'s two loose `latitude`/`longitude` fields collapse into one
    `coordinates: [f32; 2]` (the type already used by config, the weather API, and `float_pair_array`).
    The weather service keeps its own copy by necessity (it runs on the worker thread); that's the only
    remaining duplicate and it's updated explicitly in `set_weather_coordinates`.
  - **E1:** `text_2_bool_lazy` (true for any string containing `t`) is replaced by `parse_config_bool`
    (case-insensitive `true`/`1`/`yes`/`on`, else `false`). Unit-tested.
- **C5 / C6 / C7 — path sanitization, restart error handling, DST-safe date entry.**
  - **C5:** the unsound `name.replace("..", "")` in `set_background` and `generate_colorscheme` is
    replaced by one shared `utilities::safe_image_path(name)`, which keeps only the final path
    component (`Path::file_name`) — defeating `..`, absolute paths, drive prefixes, and embedded
    separators — and returns `None` for names with no file component. Unit-tested.
  - **C6:** `restart_self` no longer panics. It's now `&mut self`, reports a failure to locate the exe
    or spawn the child via the error window, and `exit`s **only** on a successful spawn — so a failed
    restart leaves the running process intact instead of crashing or looping.
  - **C7:** `parse_time_input` resolves local times with `.earliest()` (so a fall-back **ambiguous**
    time picks the earlier instant instead of failing) and, for a spring-forward **gap**, nudges one
    hour forward to land just past it. The `Single` path is unchanged (covered by the existing test);
    the DST branches are tz-dependent so aren't unit-tested.
- **B5 — no per-frame clone of the input event vector.** `show_calendar`'s tap-vs-drag state machine
  used `ui.ctx().input(|i| i.events.clone())` every frame. It now inspects `&i.events` in place inside
  the `input(|i| …)` closure (the closure only mutates `self` / reads `visible_cells`, no re-entrant
  `ctx` calls). Behaviour is identical. Also dropped the unused `MouseWheel { unit, delta, modifiers }`
  destructure in the same arm to `{ .. }`, clearing 3 of the E6 warnings (build now at 15).
- **B2 — calendar rebuild is no longer O(days × items), and the week count is sanely clamped.**
  `summarize_calendar` now buckets events and deadline-tasks by day **once**
  (`tasks::bucket_by_deadline_day` → `HashMap<NaiveDate, Vec<&Active>>`, borrowing, no clones) and does
  an O(1) lookup per cell, so the whole build is O(days + items) instead of re-scanning every item for
  every day. Buckets preserve source order (events by deadline, tasks by score), so the "take 3"
  preview selection is unchanged; deadline-less items are skipped from the grid but still retained in
  `active_things` (A4 behaviour preserved). Separately, `CALENDAR_WEEKS_MAX` dropped from `20000`
  (~385 years, ~140k cells, a 20k-float `row_anim`) to `520` (~10 years) — a one-line change to the
  shared constant introduced in C3, so the startup loader and the live setter still agree. Unit test
  covers the bucketing (grouping + per-day order). Existing configs above the new cap clamp down on
  next load.
- **A9 — Settings no longer panics on an empty monitor list.** The startup-monitor row in
  `show_settings` used to index `monitor_options[selected_monitor_index]` directly, which panics when
  the list is empty (winit's `MonitorHandle::name()` can return `None` for every monitor, or a
  headless/remote setup reports none). The row now renders a disabled "No monitors detected"
  placeholder when the list is empty and otherwise resolves the selection with `position(...)` /
  `.get(...)` instead of raw indexing. (Found while doing C3.)
- **C3 — restart-only settings now apply live or are labelled.** `set_calendar_weeks` now updates
  `self.calendar_weeks_to_show` and re-runs `summarize_calendar` (+ `sync_calendar_caches`)
  immediately, so the new week count takes effect without a restart. It commits on Enter / focus-loss
  (not per keystroke, since each apply rebuilds the calendar), reflects the clamped value back into
  the field, and restores the field on invalid input. The clamp bounds are shared
  `initialization::CALENDAR_WEEKS_MIN/MAX` constants, so the live value and the startup-loaded value
  clamp identically (and B2 can change the range in one place). The **startup monitor** genuinely
  needs a restart (the window binds to a monitor at launch), so it's now labelled "(applies after
  restart)" with a hover hint on the ♲ button rather than silently saving.
- **C2 / C4 / D5 — unified, typed config writer with surfaced errors.** Both writers now use
  `toml_edit` and agree on value types. Startup persistence moved from `toml::to_string(&Config)`
  (which stripped comments and reordered keys) to `write_normalized_config`, which updates only the
  owned keys in-place and **preserves comments / ordering / unknown keys**; values are written with
  their real TOML types (integers, float arrays) instead of strings (**C2**). The runtime setters'
  duplicated read-parse-set-write blocks collapsed into one helper,
  `TaskApp::write_config_value(key, value)`, wrapped by `persist_config_value` (**D5**); the
  `toggle_*` and `update_background_config` methods were removed in favour of direct
  `persist_config_value` calls. Write failures (config setters **and** the notepad save) now route to
  the error window instead of `let _ = …` (**C4**). Coordinate/window pairs share
  `utilities::float_pair_array`. Tests: `write_normalized_config` round-trip (comment preserved, typed
  values) and `float_pair_array`. _Remaining (noted under the old C4): the error channel is still a
  single `error_text`/`error_flag`, so a second error can overwrite an unread first — a small queue
  would help now that more write errors surface._
- **C1 — stable item ids replace name-as-primary-key.** `Active`/`InActive` gained a `u64 id`
  (`#[serde(default)]`); delete/complete/lookup and the calendar day popup now key on `id` instead of
  `name` (`delete_active_thing(id)`, `complete_active_thing(id)`, `confirm_*: Option<u64>`, and the
  popup's `all_str` tuple carries the id). `name` is now purely cosmetic, so the creation-time
  `name_is_unique` gate was removed — duplicate names are allowed and a hand-edited duplicate no
  longer deletes/completes both. New ids come from `TaskApp::next_id` via `add_active_thing`; legacy/
  hand-edited saves (id `0`) are backfilled at startup by `tasks::assign_missing_ids` (preserves
  existing ids, seeds `next_id` past the max). Unit tests cover the backfill (mixed/empty). _Follow-up:
  a rename **UI** is now unblocked by the data model but not yet built._
- **A8 — startup deserialization no longer aborts the app.** `main.rs` now handles the `Err` from
  `read_at_startup` and `read_colorschemes` instead of `.unwrap()`-ing. A corrupt/unreadable file is
  quarantined by `tasks::quarantine_corrupt_file` (renamed to `<name>.corrupt-<timestamp>`, kept for
  manual recovery); boot continues from an empty active set / the default colour scheme. Recovery
  messages are threaded through `TaskAppConfig::startup_error` and shown in the existing error window
  once the UI is up (joined if both files failed). Unit tests cover the rename-aside path and the
  no-file-present path.
- **A1 — notepad save-on-exit.** `App::exiting` now calls `TaskApp::flush_pending_saves()` on
  event-loop shutdown (covers the Quit button and the window X; not a hard kill / panic-abort).
- **A2 — frame-count autosave replaced with a wall-clock debounce.** Persists ~2 s after the last
  edit via `last_textbox_edit_time: Option<Instant>`, independent of frame rate.
- **A3 — weather reshape panic on short/partial responses.** `fix_and_cache_weather_data` now
  validates inner (per-day) lengths (`any(|hour| hour.len() < 3)`) as well as the outer 24, tripping
  `weather_is_broken_flag` instead of indexing out of bounds.
- **A4 — `summarize_calendar` deadline unwraps.** All event deadline access is now `Option`-safe
  (sort on `e.deadline`, filter with `map_or`, format with `.map(…).unwrap_or_default()`). A
  deadline-less event is quarantined (never placed) but retained in `active_things`. (Deliberately
  not surfaced via the error window — `summarize_calendar` re-runs on every edit and would re-pop the
  modal.)
- **A5 — background-texture `unwrap`.** The background draw is wrapped in `if let Some(texture)` and
  skipped when absent.
- **A6 — notepad hidden while weather is broken.** `show_weather_forecast` was restructured so the
  "WEATHER IS BROKEN" notice only replaces the forecast grids; the notepad now renders unconditionally
  when 3-day weather is off, reachable at startup and while the network is down.
- **A7 — `expanded_day` index outliving its calendar.** The day popup reads via
  `self.calendar_elements.get(index)` and dismisses itself (`else` branch clears the flags) instead of
  panicking on a stale index.
- **B3 — priority sort `as u16` saturation.** Each task's score is evaluated once into a
  `(f32, Active)` pair (preserving the intentional shuffle — §14.4 — and giving a consistent
  comparator) and sorted on the `f32` with `partial_cmp` (NaN → `Equal`). No `u16` cast.
- **E5 — day picker `1..=31`.** The day `ComboBox` now ranges over `utilities::days_in_month(year,
  month)` and clamps `day_input` into range each frame, so "Feb 31" can't be entered.
- **E7 — no tests (partial).** Added `#[cfg(test)]` unit tests for `ordinal_suffix`, `days_in_month`,
  `parse_time_input` (valid + impossible dates), `calendar_item_color`, and `importance_score` (the
  `1e9` branch and event-distance ordering, with bounds that tolerate the random multiplier). Later
  work added tests for `quarantine_corrupt_file`, `assign_missing_ids` (C1), `write_normalized_config`
  + `float_pair_array` (C2), and `bucket_by_deadline_day` (B2 — the calendar bucketing, extracted into
  a pure helper so it's testable without constructing `TaskApp`). Run with `cargo test --lib`. Still
  uncovered: the weather reshape (a `TaskApp` method needing the struct constructed first).

### Reclassified as intentional (moved to `DOCUMENTATION.md` §14)

Previously listed here as "won't fix / by design"; now documented as deliberate design decisions so
they aren't re-raised as problems:

- Uncapped, forced-repaint render loop + `AutoNoVsync` / `predictable_texture_filtering` (was B1, E4)
  → §14.1.
- Single-file `ui.rs` / large `TaskApp` (was D1) → §14.2. (The modal-`enum` refinement survives as
  the open item **D6**.)
- Hand-tuned calendar/widget magic numbers (was D3) → §14.3. (The static-dialog DPI note survives as
  the open item **D7**.)
- Random tie-break shuffle in `importance_score` (was E3) → §14.4.
