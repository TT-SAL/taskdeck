# TaskDeck on a Phone — Proposals

> **Status.** Proposal D — TaskDeck serving its own phone view — **is built**, together with the
> read-only calendar feed that is Proposal A's third transport ([`DOCUMENTATION.md` §21](DOCUMENTATION.md)).
> Its one stated limit — the phone goes dark when the desktop is off — was then lifted the way
> §7 anticipated: the board was extracted from the GUI, **`taskdeck-server`** hosts it on an
> always-on machine with no window, and the desktop runs as a client of it with an offline queue
> ([`DOCUMENTATION.md` §22](DOCUMENTATION.md), [`SERVER.md`](SERVER.md)). The phone page has since
> gained subscribed calendars, a continuously scrolling agenda, a weather view with the day's plan
> drawn on the same hours as the rain ([§21.9](DOCUMENTATION.md)), and an offline queue of its own,
> and `deploy/install.sh` sets the server up in one command. This document is kept as the comparison that led there, and as the costed
> design for Proposal B should native two-way calendar sync ever be wanted on top.
>
> Like `DOCUMENTATION.md` §20, this document records design directions and the reasoning behind
> them, so the trade-offs survive the conversation they came out of. It proposes several routes
> to the same goal — *seeing and editing the calendar on a phone* — compares them honestly, and
> ends with a recommendation and an order to build things in.

- **Goal:** the calendar (and ideally the day plan) visible on a phone, away from the desk,
  with at least some ability to change it from there.
- **Non-goal:** a second full TaskDeck. The desktop app stays the home of the data and the
  place where real planning happens.

---

## 1. Table of Contents

1. Table of contents
2. What "my calendar on my phone" actually means
3. Constraints the existing architecture sets
4. Proposal A — one-way ICS publish (*see it anywhere, cheaply*)
5. Proposal B — two-way Google Calendar sync (*the asked-for thing, costed honestly*)
6. Proposal C — CalDAV (*the standards route*)
7. Proposal D — TaskDeck serves its own phone view (*the architecture-fit route*)
8. Proposal E — a quick-capture inbox (*the cheap half of "edit"*)
9. Comparison
10. Recommendation and build order
11. Implementation notes shared by every route

---

## 2. What "my calendar on my phone" actually means

TaskDeck's "calendar" is not one list. Before choosing a transport, it is worth being precise
about **which layers** should reach the phone, because they differ in how well they survive
translation into what a phone calendar app can show:

| Layer | In the model | Maps to a phone calendar as… | Fidelity of the mapping |
|-------|--------------|------------------------------|--------------------------|
| **Events** | `deadline` *is* when it happens, `duration_minutes` its length | an ordinary timed event | **Perfect.** This is exactly what phone calendars are. |
| **Task deadlines** | `deadline` = when it is *owed* (§16.1: due ≠ planned) | a short marker event ("DUE 17:00 — report") | **Good**, with one caveat: a phone calendar will happily let you *drag* it, which TaskDeck deliberately makes a dialog (§16.3, "due markers are not draggable"). |
| **Sessions** (the day plan) | when you will *work on* something; a budget spent from an estimate | events in a second calendar ("TaskDeck plan") | **Fair.** The block itself maps; the machinery behind it — estimate, booked, remaining, one ✓ freeing all blocks — does not exist on the other side. |
| **Routines** | a weekly *rule*, minutes-from-midnight, no per-day instances (§18.3) | a weekly recurring event (`RRULE:FREQ=WEEKLY;BYDAY=…`) | **Good for viewing.** Local-time RRULEs even share the model's DST stance. But any per-occurrence edit on the phone ("skip today") is a concept the rule deliberately does not have. |
| Severity / horizon / score | the ranking model (§7) | — | **Does not map at all.** No phone calendar has these, and nothing should pretend to. |

Two conclusions fall out of the table before any code is written:

1. **Viewing is nearly lossless.** Everything worth *seeing* on the go — events, due dates,
   the day's plan, even routines — exports cleanly to iCalendar. The one-way direction is
   cheap and safe.
2. **Editing is where fidelity dies.** A phone calendar can express "move this block" and
   "new event Saturday 14:00". It cannot express "this takes 2h and only 1h is booked",
   "severity: lethally important", or "this is a rule, not an instance". Any two-way route
   must decide, per edit type, what a phone-side change *means* — and the honest answer for
   several of them is "nothing; ignore it or refuse it".

So the proposals below are really answers to two separable questions: **how does the phone
see the calendar** (A, and the read halves of B/C/D), and **how do changes get back**
(the write halves of B/C/D, and E).

---

## 3. Constraints the existing architecture sets

These shape every proposal; ignoring any one of them produces a design the codebase is
already documented as rejecting.

### 3.1 One writer, whole-picture saves (§4.1)

Every save writes **the entire active set**, atomically, and two processes doing that clobber
each other — the docs are explicit that atomicity is not exclusion, and `.lock` exists to
*warn* about exactly this. The consequence for sync:

> **Any sync engine must live inside the running TaskDeck process**, as a thread beside the
> weather thread — never as a second process writing `read_at_startup.json` directly.

A separate daemon or a file-sync tool that edits the JSON while TaskDeck is open is precisely
the documented data-loss case. In-process, a sync thread hands results to the UI thread
(the `EventLoopProxy` wake already exists for weather, §5.6) and changes flow through the same
setters everything else uses — `summarize_calendar` + `save_active_things` — so the calendar,
the task list, and the disk can never disagree.

The corollary: **when the desktop app is closed, nothing syncs.** That is acceptable — edits
made on the phone queue up wherever they were made (Google, the CalDAV server, the inbox
file) and reconcile at the next startup — but it must be stated, not discovered. *(As built,
the "running TaskDeck process" can be `taskdeck-server` on a box that stays on, and the
desktop's closing then changes nothing — `DOCUMENTATION.md` §22.)*

### 3.2 Ids are per-install; sync needs globally stable identity

`id` is a monotonic `u64` handed out by this install (`assign_missing_ids` seeds it from the
save file). Two installs, or TaskDeck and Google, will both happily use `17`. Every two-way
route needs a stable cross-system identity:

- add `uid: String` to `Active` (`#[serde(default)]`, backfilled at startup exactly like ids
  are today: `taskdeck-<id>@<install-uuid>`, with the install UUID minted once into
  `userconfig.toml`);
- use it as the iCalendar `UID` / Google `iCalUID`, and key the sync-state map on it.

This is worth doing in the same style as `assign_missing_ids` — one backfill pass, old saves
load unchanged — and it is a prerequisite for B and C, useful for A, and irrelevant to D
(which talks to the live process and can keep using `id`).

### 3.3 The data model is richer than iCalendar

Severity, horizon, estimates, the sessions-spend-a-budget mechanic, and rule-not-instance
routines have no wire representation on the other side. `X-TASKDECK-*` properties /
`extendedProperties.private` can round-trip them through a server, but no phone UI will show
or edit them. Every two-way design must therefore treat the phone as a **coarse editor**:
times and titles yes, the model no.

### 3.4 Precedent: blocking threads, rustls, no async runtime

The weather thread is the pattern: a plain thread, blocking `reqwest` with `rustls`, retries
with backoff, results behind an `Arc`, a proxy wake. Every network proposal below fits that
shape; none needs tokio.

### 3.5 The desktop is usually on

The app's premise is a wall calendar on a spare monitor that runs all day (README). That
makes "the phone talks to the running desktop app" (Proposal D) far more viable than it would
be for a laptop-only tool — the server is already running whenever the household is awake.

---

## 4. Proposal A — one-way ICS publish

**What it is.** After every save (and on a debounce, like the notepad's), TaskDeck writes
`taskdeck_data/taskdeck.ics` — a standard iCalendar file containing the layers from §2, each
tagged with a `CATEGORIES` so the phone can tell them apart. The phone *subscribes* to it.
View-only.

**What goes in it** (each layer independently toggleable in Settings):

| Layer | iCalendar shape |
|-------|-----------------|
| Events | `VEVENT` with `DTSTART`/`DTEND` |
| Task due dates | short `VEVENT` at the due time, `SUMMARY: DUE — <name>` |
| Sessions | `VEVENT` per session, `SUMMARY: <name>` (optionally `⏱`-prefixed) |
| Routines | `VEVENT` with `RRULE:FREQ=WEEKLY;BYDAY=…`, local-time `DTSTART` — off by default; sleep on a phone calendar is noise |

`UID` from §3.2 keeps subscriptions stable across regenerations, so the phone updates items
in place instead of duplicating them.

**How the phone gets the file.** Three transports, in increasing freshness:

1. **Subscribe by URL in Google Calendar** ("From URL"). Zero phone setup beyond pasting a
   link — but the file must be reachable over HTTPS from the internet, and Google refreshes
   subscribed calendars **on its own slow schedule** (commonly quoted as up to ~12–24 hours,
   not user-controllable). Fine for events and deadlines; useless for "what does my
   afternoon look like *now*".
2. **Sync the file itself to the phone** — Syncthing (Android; Syncthing-Fork from F-Droid,
   since the official Play Store app was discontinued; Möbius Sync on iOS) — then subscribe
   locally: **ICSx⁵** on Android reads a local/remote ICS into the system calendar on a
   refresh interval you choose; the events then appear in *any* calendar app, Google
   Calendar included.
3. **TaskDeck serves the file itself** — a ~50-line `tiny_http` thread serving one GET —
   reachable from the phone over **Tailscale** (a free WireGuard mesh; no port forwarding,
   nothing exposed to the internet). ICSx⁵ / iOS "subscribed calendar" pull it as often as
   every 15 minutes. This transport is also the first brick of Proposal D.

**What it costs.** The export is a pure function of `Vec<Active>` — a day or two including
tests, using the `icalendar` crate or ~200 lines by hand (the format is line-based text).
No OAuth, no accounts, no server state, no conflict handling, because nothing ever writes
back.

**What it refuses to do.** Edit. Deliberately: one-way means the desktop file is still the
single truth, §3.1 is untouched, and there is no failure mode worse than "the phone is a few
minutes stale".

**Verdict.** The highest value per line of code in this document. Even if a two-way route is
built later, this ships first and keeps working alongside it.

---

## 5. Proposal B — two-way Google Calendar sync

**What it is.** The literal request: TaskDeck talks to the Google Calendar API; the phone
uses the ordinary Google Calendar app; edits flow both ways.

**Shape.** A sync thread beside the weather thread:

- **Auth:** OAuth installed-app flow with the loopback redirect (open browser, catch the code
  on `127.0.0.1`), refresh token stored in `taskdeck_data/`. The `oauth2` crate covers this.
- **Calendars:** create dedicated secondary calendars — `TaskDeck` (events + due markers) and
  optionally `TaskDeck plan` (sessions) — never the user's primary. A dedicated calendar is
  what makes deletion semantics sane and lets the phone toggle layers.
- **Down-sync:** `events.list` with `syncToken` for cheap incremental pulls, polled every few
  minutes (push notifications require a public HTTPS webhook — a non-starter for a desktop
  app; polling is the honest design).
- **Identity:** `iCalUID` = the §3.2 uid; TaskDeck's uid also mirrored into
  `extendedProperties.private` as a belt-and-braces key. A `sync_state.json` maps uid ↔
  Google event id + etag + last-synced stamp.
- **Conflicts:** per-item last-write-wins on modification time, which is enough for one
  person and two devices. Tombstones both ways: a Google-side delete of a TaskDeck-owned
  event retires the item as `Dropped` (the archive already models this); a TaskDeck-side
  retire deletes the Google event.

**What phone edits mean** — the table that makes or breaks this proposal:

| Edit on the phone | Meaning in TaskDeck | Verdict |
|---|---|---|
| Move / resize an **event** | move the event (its deadline *is* its time) | clean |
| Move a **DUE marker** | change the task's deadline | works, but bypasses the "changing a deadline should look like changing a deadline" stance (§16.3). Acceptable: a phone edit is deliberate. |
| Move / resize a **session block** | move that session / change its minutes (and the estimate? resize currently edits only the block — keep that rule) | clean |
| Create an event in the `TaskDeck` calendar | new `Active` event | clean |
| Create an event in `TaskDeck plan` | a session of… which task? | **no answer.** Import as a plain new task with one session named after the title; it will be slightly wrong and editable at the desk. |
| Delete | retire as `Dropped` | clean, thanks to the archive |
| Edit a routine occurrence | per-occurrence exceptions are exactly what §18.3 refused to model | **refuse**: routines export one-way or not at all |

**The costs nobody advertises:**

- **Google Cloud console ceremony.** A project, an OAuth consent screen, and — because
  Calendar scopes are classed *sensitive* — a choice between: staying in "Testing" status,
  where **refresh tokens expire every 7 days** (re-consenting weekly, forever); or publishing
  unverified, which works for a personal app but greets you with a scary "unverified app"
  interstitial; or actual verification, which is a process aimed at companies. For a
  personal tool this is the single most annoying property of the whole proposal, and it is
  ongoing, not one-time. (Policy details drift; verify against current Google docs before
  committing.)
- **Engineering weight.** OAuth + token refresh + incremental sync + tombstones + conflict
  handling + a mapping store is realistically **a few weeks**, and it is the kind of code
  that generates a trickle of edge-case bugs (revoked tokens, 410-gone sync tokens, quota
  hiccups) indefinitely. It would be the most complex subsystem in the app, in an app whose
  documented instinct is to resist metastasizing features (§18.8).
- **Privacy.** The calendar — names of tasks included — lives on Google's servers. That is
  the point for some users and a dealbreaker for others; it is a fact either way.

**What you get for it.** The genuinely best phone *editing* UX available — the native
calendar app, widgets, notifications, sharing — and visibility even when the desktop is off.

**Verdict.** Buildable, and the design above is the right shape if it is built. But it is
the most expensive proposal by a wide margin, the only one with a standing third-party
dependency, and most of its unique value (native app, works when desktop is off) is *view*
value that Proposal A already delivers for ~2% of the cost.

---

## 6. Proposal C — CalDAV

**What it is.** The same two-way idea over the open protocol instead of Google's API.
TaskDeck implements a small CalDAV client (PUT/DELETE per item, `sync-collection` REPORT for
changes); the phone side is native on iOS and **DAVx⁵** on Android; the middle is either
Google's CalDAV endpoint or a self-hosted server (**Radicale** is a famously small one —
a Python package and a config file; Baïkal and Nextcloud are the heavier options).

**Why it tempts:**

- No vendor lock; the phone half is standard.
- Item-per-resource maps cleanly onto per-`uid` sync, and `X-TASKDECK-*` properties can
  round-trip severity/estimates losslessly through the server (invisible on the phone, but
  not destroyed).
- With a server that supports `VTODO`, tasks-with-due-dates could sync as actual *tasks* to
  Tasks.org on Android — the only route in this document where a task arrives on the phone
  as a task rather than as a disguised event.

**Why it loses anyway:**

- **Against Google's endpoint:** the same OAuth ceremony as B (Google's CalDAV requires
  OAuth too), for a strictly less capable API. Dominated by B.
- **Self-hosted:** now there is a server to run, back up, and TLS-certify — a standing
  operational chore attached to a wall calendar. Tailscale removes the exposure problem but
  not the "one more service" problem.
- Rust's CalDAV client ecosystem is thin; expect to hand-roll the WebDAV subset. The edit
  semantics table from B applies unchanged — CalDAV changes the wire, not the meaning
  problem.

**Verdict.** The right choice only if two conditions hold at once: two-way editing is a hard
requirement *and* the data must not live with Google. Otherwise B beats it on phone UX and
A/D beat it on cost.

---

## 7. Proposal D — TaskDeck serves its own phone view

**What it is.** The running desktop app hosts a small HTTP server on a background thread —
`tiny_http` fits the no-async house style — serving a **single-file mobile web page**: today
and the next few days as a scrollable timeline, the tray, and a handful of verbs. The phone
reaches it over the LAN or, from anywhere, over Tailscale. Add it to the home screen and it
is indistinguishable from an app.

**Why this is the architecture-fit route.** Every other two-way proposal fights §3.1 — a
second copy of the truth that must be reconciled. This one has **no second copy**: the phone
is a thin client of the one live process. A `POST /item/17/deadline` lands in the UI thread
(via the existing proxy-wake pattern, applied through the same setters as every gesture) and
`summarize_calendar` + `save_active_things` run exactly as if the edit were made at the desk.
No uid scheme, no mapping store, no tombstones, no conflict policy — none of that machinery
exists because the problem it solves was never created.

It is also the **only route with full fidelity**: the phone page can show severity, horizon,
`takes 2h · 1h booked`, routines as routines — because it is TaskDeck talking to itself, not
a translation into someone else's schema.

**Scope discipline.** The page is a *companion*, not a port. Version one:

- read: today ± N days (timeline with blocks, due markers, routines), the tray;
- write: quick-add a task; complete; move/resize a block; change a deadline (as a picker, not
  a drag — preserving the §16.3 stance even here).

**Security, sized honestly.** Bind to the Tailscale/LAN interface, require a bearer token
minted into `userconfig.toml` and baked into the phone's bookmark URL. Over Tailscale the
transport is WireGuard-encrypted end-to-end, so plain HTTP inside the tunnel is fine and the
TLS-certificate question never comes up. Do not port-forward this to the open internet.

**The honest limits:**

- **Desktop off ⇒ phone dark.** §3.5 argues this is rare for this app, and pairing with
  Proposal A (whose §4 transport 3 is *this same HTTP thread* serving one more file) gives a
  read-only fallback that outlives the desktop being on. *(This limit was lifted after the
  build: the board was extracted from the window and `taskdeck-server` hosts the same page and
  feed on an always-on machine, with the desktop as its client — `DOCUMENTATION.md` §22. The
  reasoning above is left as it was written.)*
- No native notifications/widgets; it is a web page.
- One new skill in the project: a small amount of HTML/JS. Keeping it to a single embedded
  file (`include_str!`, like the fonts and SVGs) keeps the build story unchanged — the
  executable remains the whole install.

**Cost.** The server thread and JSON endpoints are small (the setters already exist; this is
plumbing). The page itself is the real work. Realistically **a week-ish** for the v1 scope
above — an order of magnitude less than B, for more fidelity, at the price of native-app
polish.

---

## 8. Proposal E — a quick-capture inbox

Not a calendar view at all, but it is half of "edit on the go", separable and nearly free:
the commonest mobile need is *capture* — "remember to book the dentist" — not replanning.

`taskdeck_data/inbox.txt`: one task name per line. Anything that can append a line to a
synced file feeds it (Syncthing + any notes widget; a Shortcuts automation on iOS). TaskDeck
polls the file's mtime once a minute *in-process* (the running app is the only JSON writer,
so §3.1 is respected; the inbox file itself is append-only foreign territory, read and then
truncated), and each line becomes an undated, unplanned task — exactly what the tray's
quick-add makes — waiting in the tray at the next planning session.

A parsing convention can come later (`due fri`, `~2h`); version one is names only. If
Proposal D is built, its quick-add endpoint makes this redundant — but E works with zero UI
and ships in an afternoon.

---

## 9. Comparison

| | **A** ICS publish | **B** Google two-way | **C** CalDAV | **D** Own phone view | **E** Inbox |
|---|---|---|---|---|---|
| See calendar on phone | ✔ (mins–hrs stale, by transport) | ✔ live | ✔ live | ✔ live | ✘ |
| Edit from phone | ✘ | times/titles only | times/titles only | ✔ full fidelity | add-only |
| Works with desktop off | ✔ (last export) | ✔ | ✔ | ✘ alone; ✔ with `taskdeck-server` (`DOCUMENTATION.md` §22) | ✔ (queues) |
| Native phone calendar UI | ✔ | ✔ | ✔ | ✘ (web page) | — |
| Respects one-writer rule (§3.1) | trivially | needs the full sync engine | needs the full sync engine | **by construction** | ✔ |
| New moving parts | none | Google project, OAuth, sync state | server to run, OAuth or ops | HTTP thread, token, (Tailscale) | synced text file |
| Standing annoyances | subscription refresh lag | token expiry / unverified-app ceremony | server care & feeding | keep desktop on — or run `taskdeck-server` on a box that stays on | none |
| Data leaves your machines | only if transport 1 | yes (Google) | optional | no | no |
| Effort | **days** | **weeks**, then upkeep | weeks + ops | **~a week** | **hours** |

---

## 10. Recommendation and build order

**Phase 1 — ship A (and E if capture matters).** The ICS export is days of work, mostly
pure functions over `Vec<Active>`, testable like `planner.rs` is. Transport via Syncthing +
ICSx⁵ (or the iOS subscribed-calendar equivalent) gives a phone view fresh to within minutes,
with nothing running anywhere new. This alone is most of the stated goal: *"a way to see my
calendar on my phone"*.

**Phase 2 — build D for the editing half.** It is the only proposal whose design *removes*
the hard problems (identity, conflicts, second copies) instead of solving them, the only one
with full model fidelity, and it fits an app that is on all day by design. Its HTTP thread
also upgrades A's transport for free. Live with it before considering anything heavier —
exactly the "reflow first, floating sessions after it has earned it" logic of §20.3.

**B only if, after living with A + D, the native Google Calendar experience — widgets,
notifications, editing with the desktop off — turns out to be genuinely missed.** Then build
it as designed in §5, on top of the §3.2 uid work, with eyes open about the OAuth upkeep.
C is the B-variant to pick only if Google specifically is unwanted.

What to resist, in the spirit of §20.4: a background sync *daemon* (it is the documented
clobber case), syncing routines two-way (per-occurrence edits reintroduce the complexity
§18.3 exists to refuse), and letting any phone surface grow toward being a second TaskDeck.

*What was actually built: Phase 1's feed and Phase 2's D together, then — rather than B — the
board moved out of the window into `taskdeck-server`, which lifts D's one limit without a second
copy of anything. The case for B above is therefore now only the native-app experience: widgets
and notifications. See the status note at the top.*

---

## 11. Implementation notes shared by every route

- **The hook point exists already.** Every mutation funnels through
  `save_active_things` (`ui.rs`); the ICS export (A) is one more debounced write there, in
  the notepad's pattern. Sync engines (B/C) instead *observe* mutations and talk to their
  thread through channels, weather-style.
- **Threading pattern:** clone of the weather subsystem — plain thread, blocking
  `reqwest`/`tiny_http`, retries with backoff, `Arc` + `EventLoopProxy` wake. No async
  runtime enters the tree.
- **Identity (§3.2):** `uid: String` on `Active`, `#[serde(default)]` + startup backfill,
  install UUID in `userconfig.toml`. Prerequisite for B/C, nice-to-have for A, unused by D.
- **Time and zones:** `DateTime<Local>` serializes with an offset, so instants export
  unambiguously (emit UTC in ICS). Routines are *wall-clock minutes* by design — export as
  local-time `RRULE` with the system `TZID`, which preserves their DST behaviour exactly.
- **Crates:** `icalendar` (or hand-rolled ICS — it is line-based text), `tiny_http` (D and
  A-transport-3), `oauth2` + existing `reqwest` (B/C). All compatible with the
  rustls/no-OpenSSL build stance.
- **New files in `taskdeck_data/`:** `taskdeck.ics` (A), `sync_state.json` + token store
  (B/C), `inbox.txt` (E). All follow the atomic temp-file-rename discipline; the token store
  should be mentioned in the §4 file table if built, since it is a credential.
- **Settings:** one new section — which layers export (events / due / plan / routines), the
  sync toggle, and the phone-view token with a "show QR" affordance if D is built (a QR is
  how a URL with a token gets into a phone without typing it).
