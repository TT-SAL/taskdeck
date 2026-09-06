//! Calendars somebody else keeps.
//!
//! A subscription is an https address that answers with an iCalendar file. The
//! events in it are drawn beside the board's own and are **never part of it**:
//! they do not become `Active`, they never reach the archive, nothing about
//! them can be edited, and no command creates one. They are context. "There is
//! a meeting at two" is a reason not to plan work at two; it is not a task.
//!
//! That line is the whole design. The moment a fetched event became an item,
//! this would need identity mapping across refreshes, tombstones for events
//! deleted upstream, a policy for an edited import, and an archive filling with
//! things nobody did — the machine `DOCUMENTATION.md` §21.1 refuses to build
//! for the outbound direction. Keeping the two apart costs one extra shape and
//! buys the invariant back whole.
//!
//! The module has two halves, and the split matters:
//!
//! - **The list** (`Subscription`) is *authored*. It is board data, it lives in
//!   `subscriptions.json` beside the other board files, and it changes only
//!   through `Board::apply`, so it replicates to clients like everything else.
//! - **The overlay** (`Overlay`) is *derived*. It is what the addresses said
//!   last time anyone asked. Losing it costs a refresh, not a calendar.
//!
//! Who fetches follows the same rule as everything else here: **whichever
//! process owns the board**. A standalone desktop fetches its own; the server
//! fetches; a desktop that is a client of a server never does, and receives the
//! overlay with the board it is a replica of. Two fetchers would be two clocks,
//! two copies of an address that is itself the credential, and two answers to
//! what is on Tuesday.

use std::{
    error::Error,
    fs,
    io::{Read, Write},
    path::Path,
    sync::{
        Arc, RwLock,
        atomic::{AtomicU64, Ordering},
        mpsc::{Receiver, RecvTimeoutError, Sender, channel},
    },
    thread,
    time::Duration,
};

use chrono::{Local, NaiveDate, TimeDelta};
use serde::{Deserialize, Serialize};

use crate::{ics, phone::Wake, planner};

/* ─────────────────────────────── The bounds ──────────────────────────────── */

/// The file the list lives in, beside the other board files.
pub const SUBSCRIPTIONS_FILE: &str = "subscriptions.json";
/// The overlay's cache. Derived data, kept only so a restart draws the
/// calendars it drew before rather than an empty week for ten minutes.
pub const OVERLAY_FILE: &str = "subscribed_cache.json";

/// The colours a new subscription is given, in turn: six that read clearly
/// against the calendar without borrowing the scheme's meaning.
pub const DEFAULT_COLORS: [[u8; 4]; 6] = [
    [ 90, 140, 200, 255],
    [200, 120,  90, 255],
    [120, 175, 110, 255],
    [175, 120, 190, 255],
    [200, 175,  90, 255],
    [110, 175, 185, 255],
];

/// Most calendars one board may subscribe to. Past this the settings sheet
/// stops offering to add another; the number is a wall calendar's, not a
/// mail server's.
pub const SUBSCRIPTIONS_MAX: usize = 20;
/// Longest summary kept off a feed. Same reason and same number as
/// `board::NAME_MAX_CHARS`: it is one line on a calendar cell.
pub const SUMMARY_MAX_CHARS: usize = 200;
/// Most events one overlay holds across every subscription. A feed that spells
/// out a decade of a daily standup is not malice, only a calendar server being
/// literal — and either way the wall shows one week.
pub const EVENTS_MAX: usize = 5_000;
/// Longest location kept. A room, not an address book: the longest in a real
/// university feed is thirty-four characters.
pub const LOCATION_MAX_CHARS: usize = 120;
/// Longest description kept. It is only ever a hover, and a calendar server
/// will happily send a page of meeting notes.
pub const DESCRIPTION_MAX_CHARS: usize = 300;
/// Longest failure text kept against a subscription. The words come from
/// somebody else's server and are shown at the desk.
pub const WHY_MAX_CHARS: usize = 200;

/// Most bytes read from one address. A calendar is text; anything past this is
/// not one, and reading it would be somebody else deciding how much of this
/// machine's memory to use.
pub const MAX_ICS_BYTES: u64 = 8 * 1024 * 1024;
/// How long one fetch may take before it is given up on.
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(20);
/// How often the calendars are read again.
pub const REFRESH_EVERY: Duration = if cfg!(test) { Duration::from_millis(200) } else { Duration::from_secs(600) };
/// How far back and forward the window reaches. Not the calendar's display
/// range, which is ten years: expanding a daily rule across ten years for
/// every subscription is hundreds of thousands of occurrences to hold, to
/// compare on every refresh, and to hand to a client.
pub const WINDOW_BACK_DAYS: i64 = 60;
pub const WINDOW_FORWARD_DAYS: i64 = 400;

/// The span the calendars were actually read for, both ends inclusive.
///
/// Outside it the overlay is empty — and an empty day is indistinguishable on
/// the wire from a day with nothing on it. That is a wrong answer to the only
/// question a calendar is asked, so the span travels with the overlay and out
/// to the phone, which can then say *nothing of yours* where it would
/// otherwise have said *nothing*.
///
/// Carried from the fetch that used it, never re-derived from a later clock: a
/// desk that has just started up honestly reports the window its cache was
/// written with, until the first refresh lands seconds later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Covered {
    pub from: NaiveDate,
    pub until: NaiveDate,
}

impl Covered {
    /// Whether a day is one the calendars were read for.
    pub fn holds(&self, day: NaiveDate) -> bool {
        self.from <= day && day <= self.until
    }
}

/* ────────────────────────── The list, which is authored ──────────────────── */

/// One subscribed calendar, as the user set it up.
///
/// The `url` is usually the credential as well as the address — a secret link
/// from Google or Apple with no password behind it. It lives with the board,
/// which is why the board's folder is the thing to keep private, and why a
/// client receives it rather than each machine holding its own copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subscription {
    pub id: u64,
    pub name: String,
    pub url: String,
    /// RGBA, straight rather than through the colour scheme. The colour says
    /// *which calendar*, not how urgent — so it does not follow a scheme
    /// change, and that is deliberate.
    pub color: [u8; 4],
    pub enabled: bool,
}

/// Whether `url` is an address this will fetch, and why not when it is not.
///
/// https only. A calendar link is a bearer credential in a query string, and
/// sending one in clear over a LAN or a café's network is the one mistake this
/// can prevent for free. `webcal://` is Apple's scheme for exactly this file
/// and is rewritten rather than refused, because that is what a user will
/// paste.
pub fn checked_url(url: &str) -> Result<String, String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("An address is needed.".to_string());
    }
    if url.chars().count() > 2000 {
        return Err("That address is too long.".to_string());
    }
    let url = match url.strip_prefix("webcal://") {
        Some(rest) => format!("https://{rest}"),
        None => url.to_string(),
    };
    if url.starts_with("http://") {
        return Err("Use https — a calendar link is a password, and http sends it in clear.".to_string());
    }
    if !url.starts_with("https://") {
        return Err("An address should start with https://".to_string());
    }
    let host = url.get("https://".len()..).unwrap_or_default();
    let host = host.split(['/', '?', '#']).next().unwrap_or_default();
    if host.is_empty() || !host.contains('.') {
        return Err("That address has no host in it.".to_string());
    }
    Ok(url)
}

/* ───────────────────────── The overlay, which is derived ─────────────────── */

/// One occurrence off a subscribed calendar, already cut to a single day.
///
/// The same shape `ics::Occurrence` comes out in, plus which subscription it
/// came from. Day and minutes rather than an instant because that is what
/// every reader wants: the calendar cell, the planner's timeline and the
/// phone's day all think in minutes from midnight.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverlayEvent {
    pub subscription: u64,
    pub day: NaiveDate,
    pub start: i32,
    pub end: i32,
    #[serde(default)]
    pub all_day: bool,
    #[serde(default)]
    pub free: bool,
    pub summary: String,
    /// Where it is. Drawn on its own line: a course feed puts the room here in
    /// a dozen characters while the summary runs to a hundred, so this is the
    /// half that reads at a glance (§23).
    #[serde(default)]
    pub location: String,
    /// What a hover or a long press says. Never painted: a calendar server
    /// will send a page of notes and a block is three lines tall.
    #[serde(default)]
    pub description: String,
}

/// Past this, an event stops being an appointment and starts being a
/// description of the day.
///
/// Found in real data rather than reasoned out: a university feed carries
/// "CS-C3240, Machine Learning, Lähiopetus 1.9.–30.11." as an event running
/// 08:00 to 20:00 on every teaching day — a course *period*, written as a
/// twelve-hour block because iCalendar gave the exporter nowhere else to put
/// it. Anchoring reflow on that leaves nowhere to put any work at all, which
/// is the same failure an all-day band would cause and for the same reason.
/// Twelve hours is where the line falls, and the number is measured rather
/// than chosen. Across three real feeds — two university timetables and a
/// student club's — every genuine appointment is 90 to 600 minutes and every
/// course-period marker is exactly 720. Nothing at all sits between 600 and
/// 720, so the line goes in the gap.
///
/// It was six hours first, which was a guess made against one feed, and the
/// club's calendar showed what the guess cost: a board game night runs 16:00
/// to 22:00 and a tournament noon to eight, and all of them were being drawn
/// as scenery for a day that was in fact taken. Half a day is the honest
/// threshold — past twelve hours there is no morning or evening left to plan
/// into, which is the whole reason a span stops being an appointment.
pub const LONG_EVENT_MINUTES: i32 = 12 * 60;

impl OverlayEvent {
    /// How long it runs.
    pub fn minutes(&self) -> i32 {
        (self.end - self.start).max(0)
    }

    /// Whether this describes the day rather than taking an hour out of it: an
    /// all-day band, or something long enough to amount to one.
    ///
    /// Drawn as the ground the day sits on — full width, behind everything,
    /// and out of the column packing so a real lecture beside it still gets
    /// the width it needs.
    pub fn is_background(&self) -> bool {
        self.all_day || self.minutes() >= LONG_EVENT_MINUTES
    }

    /// Whether this is time the day has to be planned around.
    ///
    /// Not a background span, and not something the owner marked
    /// `TRANSPARENT` — this app's own feed writes that on a due marker, and a
    /// household may well subscribe TaskDeck to a calendar TaskDeck feeds.
    pub fn is_busy(&self) -> bool {
        !self.is_background() && !self.free && self.end > self.start
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum FetchOutcome {
    Ok {
        events: usize,
        problems: Vec<String>,
        /// The name the feed gives itself (`X-WR-CALNAME`), when it gives one.
        /// A subscription that is still wearing its placeholder takes this the
        /// first time it is read — nobody wants a wall calendar labelled
        /// "Calendar 150" when the file itself says what it is.
        #[serde(default)]
        title: Option<String>,
    },
    Failed {
        why: String,
    },
}

/// The name a subscription is given before anything better is known.
///
/// Only reached when the address has no host worth reading, which in practice
/// means never — `checked_url` has already refused anything without one.
pub fn placeholder_name(id: u64) -> String {
    format!("Calendar {id}")
}

/// A name taken from the address itself: the host, without `www.` and without
/// the port.
///
/// Better than "Calendar 150" and available immediately, which matters because
/// half the feeds in the world send no `X-WR-CALNAME` at all — a university's
/// Sisu feed does not, so the alternative was two calendars in the settings
/// sheet distinguishable only by an id nobody chose.
pub fn name_from_url(url: &str) -> Option<String> {
    let host = url.split("://").nth(1)?;
    let host = host.split(['/', '?', '#']).next()?;
    // A userinfo prefix belongs to nobody's calendar name.
    let host = host.rsplit('@').next()?;
    let host = host.split(':').next()?;
    let host = host.strip_prefix("www.").unwrap_or(host);
    (!host.is_empty()).then(|| host.to_string())
}

/// A better name for this subscription than the one it is wearing, if there is
/// one and nobody has typed one.
///
/// The feed's own `X-WR-CALNAME` first, and the address's host after it. Only
/// while the name is one nobody chose, so this settles rather than fighting a
/// rename every ten minutes, and it runs on every refresh rather than only at
/// the moment of subscribing — which is what lets a calendar added before any
/// of this existed pick up a real name on its next read.
pub fn better_name(subscription: &Subscription, overlay: &Overlay) -> Option<String> {
    if !is_unnamed(subscription) {
        return None;
    }
    let from_feed = match overlay.status_for(subscription.id).map(|status| &status.outcome) {
        Some(FetchOutcome::Ok { title: Some(title), .. }) if !title.trim().is_empty() => {
            Some(title.trim().to_string())
        }
        _ => None,
    };
    let better = from_feed.or_else(|| name_from_url(&subscription.url))?;
    (better != subscription.name).then_some(better)
}

/// Whether this subscription is still wearing a name nobody chose — the
/// placeholder, or the one taken off its address.
///
/// Both count, so a feed that *does* send a name is still allowed to introduce
/// itself later; only a name a person typed is left alone.
pub fn is_unnamed(subscription: &Subscription) -> bool {
    subscription.name == placeholder_name(subscription.id)
        || Some(&subscription.name) == name_from_url(&subscription.url).as_ref()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchStatus {
    pub subscription: u64,
    pub at: chrono::DateTime<Local>,
    pub outcome: FetchOutcome,
}

/// What the subscribed calendars said, last time anyone asked.
///
/// **Derived, not authored.** It does not pass through `apply`, and losing it
/// costs a refresh rather than a calendar.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Overlay {
    #[serde(default)]
    pub events: Vec<OverlayEvent>,
    /// How each subscription's last fetch went. **Kept apart from the events
    /// on purpose**: this carries the time of the last attempt, so it changes
    /// on every single refresh by construction. Folding it into the digest
    /// would make every refresh a change, which is the one thing the digest
    /// exists to prevent.
    #[serde(default)]
    pub status: Vec<FetchStatus>,
    /// The span the last fetch read for. **Kept out of the digest** for the
    /// same reason as `status`: it slides forward every midnight, so folding it
    /// in would make every refresh a change and wake every parked phone.
    /// `None` before anything has been fetched, and from a cache written by a
    /// version that did not record it — the cache is only rewritten when the
    /// *events* change, so after a restart with a stable feed this arrives with
    /// the first refresh rather than off the disk. Until it does, the phone
    /// draws no edge at all, which is the old behaviour and not a wrong one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub covers: Option<Covered>,
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fold(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

impl Overlay {
    /// The one door in, from anywhere: a fetch, the cache on disk, or a server
    /// over the wire. Bounds the strings and the counts, drops an event that
    /// ends before it starts, and **sorts** — see `digest`.
    /// The span, added after sealing. A builder rather than a third parameter
    /// on `sealed`, which has twenty callers and does not otherwise care.
    pub fn covering(mut self, covers: Option<Covered>) -> Overlay {
        self.covers = covers;
        self
    }

    pub fn sealed(events: Vec<OverlayEvent>, status: Vec<FetchStatus>) -> Overlay {
        let mut events: Vec<OverlayEvent> = events
            .into_iter()
            .filter(|event| event.end >= event.start)
            .map(|mut event| {
                event.start = event.start.clamp(0, planner::DAY_MINUTES);
                event.end = event.end.clamp(event.start, planner::DAY_MINUTES);
                event.summary = event.summary.chars().take(SUMMARY_MAX_CHARS).collect();
                event.location = event.location.chars().take(LOCATION_MAX_CHARS).collect();
                event.description = event.description.chars().take(DESCRIPTION_MAX_CHARS).collect();
                event
            })
            .take(EVENTS_MAX)
            .collect();
        // Sorted so a feed free to list its events in a different order each
        // time does not read as a change.
        events.sort_by(|a, b| {
            (a.day, a.start, a.end, a.subscription, &a.summary)
                .cmp(&(b.day, b.start, b.end, b.subscription, &b.summary))
        });
        let status = status
            .into_iter()
            .map(|mut status| {
                if let FetchOutcome::Failed { why } = &mut status.outcome {
                    *why = why.chars().take(WHY_MAX_CHARS).collect();
                }
                status
            })
            .collect();
        Overlay { events, status, covers: None }
    }

    /// Is this the same calendar as last time?
    ///
    /// FNV-1a over the events only. Not a cryptographic hash and never stored:
    /// it answers one question, in memory, about data this process just built.
    /// Computed rather than cached in a field, because a cached one would come
    /// back as zero from the wire and from the cache file and quietly compare
    /// equal to nothing.
    pub fn digest(&self) -> u64 {
        let mut hash = FNV_OFFSET;
        for event in &self.events {
            hash = fold(hash, &event.subscription.to_le_bytes());
            hash = fold(hash, &event.day.format("%Y%m%d").to_string().into_bytes());
            hash = fold(hash, &event.start.to_le_bytes());
            hash = fold(hash, &event.end.to_le_bytes());
            hash = fold(hash, &[event.all_day as u8, event.free as u8]);
            hash = fold(hash, event.summary.as_bytes());
            // Where it is and what it says are part of what the calendar told
            // us, so a room that moved is a change. Leaving them out meant a
            // cache written before this field existed was never displaced by
            // a fetch that had it — the events matched, so nothing looked
            // different, and the rooms stayed blank until something else on
            // the day happened to move.
            hash = fold(hash, event.location.as_bytes());
            hash = fold(hash, event.description.as_bytes());
        }
        hash
    }

    /// The spans a subscribed calendar claims on `day`, for `planner::reflow`.
    ///
    /// **An all-day event is not an anchor.** One would claim the whole day and
    /// leave reflow nowhere to put anything, and "I am at a conference" is not
    /// the same claim as "there is a meeting at two". Neither is a
    /// `TRANSPARENT` marker, which is what this app's own feed writes on a due
    /// date — reading our own politeness back as somebody's meeting would wall
    /// off the day with our own deadlines.
    pub fn anchors_on(&self, day: NaiveDate) -> Vec<(i32, i32)> {
        self.events
            .iter()
            .filter(|event| event.day == day && event.is_busy())
            .map(|event| (event.start, event.end))
            .collect()
    }

    pub fn events_on(&self, day: NaiveDate) -> impl Iterator<Item = &OverlayEvent> {
        self.events.iter().filter(move |event| event.day == day)
    }

    /// Drop everything one subscription contributed. Used the moment one is
    /// removed or switched off, so the timeline and the anchors are right on
    /// the next frame rather than after a round trip — and so a client, which
    /// never fetches, is right at all.
    pub fn forget(&mut self, subscription: u64) {
        self.events.retain(|event| event.subscription != subscription);
        self.status.retain(|status| status.subscription != subscription);
    }

    pub fn status_for(&self, subscription: u64) -> Option<&FetchStatus> {
        self.status.iter().find(|status| status.subscription == subscription)
    }
}

/* ───────────────────────────────── On disk ───────────────────────────────── */

/// Read the subscription list. A missing file is an empty list, which is what
/// every board that has never subscribed to anything looks like.
pub fn read(data_dir: &Path) -> Result<Vec<Subscription>, Box<dyn Error>> {
    let path = data_dir.join(SUBSCRIPTIONS_FILE);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path)?;
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    Ok(serde_json::from_str(&text)?)
}

/// Write the list whole, the way every other board file is written: a
/// temporary file in the same directory, fsynced, renamed over the target, and
/// then the directory flushed (§4.1).
pub fn save(list: &[Subscription], data_dir: &Path) -> Result<(), Box<dyn Error>> {
    write_atomically(data_dir, SUBSCRIPTIONS_FILE, &serde_json::to_string_pretty(list)?)
}

/// The overlay cache. `None` for anything unreadable: it is derived data, and
/// a refresh replaces it in a moment.
pub fn read_overlay(data_dir: &Path) -> Option<Overlay> {
    let text = fs::read_to_string(data_dir.join(OVERLAY_FILE)).ok()?;
    let overlay: Overlay = serde_json::from_str(&text).ok()?;
    // `sealed` keeps only what it is handed, so the span has to be handed back
    // to it — otherwise every restart quietly forgets how far the last fetch
    // looked and the phone stops being able to say so.
    Some(Overlay::sealed(overlay.events, overlay.status).covering(overlay.covers))
}

/// Cache the overlay so a restart draws the calendars it drew before rather
/// than an empty week until the first fetch returns. Written like the rest,
/// and a failure is the caller's to shrug at.
pub fn save_overlay(overlay: &Overlay, data_dir: &Path) -> Result<(), Box<dyn Error>> {
    write_atomically(data_dir, OVERLAY_FILE, &serde_json::to_string(overlay)?)
}

fn write_atomically(dir: &Path, name: &str, text: &str) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(dir)?;
    let mut temp = tempfile::NamedTempFile::new_in(dir)?;
    temp.write_all(text.as_bytes())?;
    temp.as_file_mut().sync_all()?;
    temp.persist(dir.join(name))?;
    crate::tasks::sync_directory(dir);
    Ok(())
}

/* ─────────────────────────────── The fetching ────────────────────────────── */

/// Read one address and parse what it answers with.
///
/// Everything about this call is bounded before it starts: a timeout, a
/// declared-length refusal, and a read that stops at `MAX_ICS_BYTES` whatever
/// the server claims. What comes back is a stranger's text, so it goes to
/// `ics::parse`, which refuses rather than guesses and cannot panic on it.
fn fetch_one(client: &reqwest::blocking::Client, url: &str, from: NaiveDate, until: NaiveDate) -> Result<ics::Parsed, String> {
    let response = client
        .get(url)
        .header(reqwest::header::ACCEPT, "text/calendar, text/plain;q=0.9, */*;q=0.5")
        .send()
        .map_err(|error| shorten(&error.to_string()))?;

    let status = response.status();
    if !status.is_success() {
        return Err(format!("the calendar answered {}", status.as_u16()));
    }
    if response.content_length().is_some_and(|length| length > MAX_ICS_BYTES) {
        return Err("that calendar is too large to read".to_string());
    }

    // Read at most one byte past the cap, so a server that lies about its
    // length is refused rather than believed.
    let mut body = Vec::new();
    response
        .take(MAX_ICS_BYTES + 1)
        .read_to_end(&mut body)
        .map_err(|error| shorten(&error.to_string()))?;
    if body.len() as u64 > MAX_ICS_BYTES {
        return Err("that calendar is too large to read".to_string());
    }

    // Not `String::from_utf8`: a calendar with one bad byte in a description
    // is still a calendar, and refusing the file over it would be the
    // archive's old mistake in a new place.
    let text = String::from_utf8_lossy(&body);
    ics::parse(&text, from, until, ics::Limits::default()).map_err(|why| shorten(&why))
}

fn shorten(text: &str) -> String {
    text.chars().take(WHY_MAX_CHARS).collect()
}

/// Read every enabled subscription once, and say what they came to.
///
/// One address failing costs only itself: the others are still read, and the
/// failure is recorded against the one that failed so the desk can say which.
///
/// **A calendar that could not be read keeps the events it last gave.** A
/// server that is down, a tunnel that dropped, a laptop that woke on the wrong
/// network — none of those is somebody cancelling a meeting, and blanking the
/// day over one is the same mistake as treating an outage as a deletion, which
/// this program refuses to make everywhere else. The failure is on the status
/// line at the desk; the meetings stay where they were.
pub fn fetch_all(client: &reqwest::blocking::Client, list: &[Subscription], previous: &Overlay) -> Overlay {
    let today = Local::now().date_naive();
    let from = today
        .checked_sub_signed(TimeDelta::try_days(WINDOW_BACK_DAYS).unwrap_or_default())
        .unwrap_or(today);
    let until = today
        .checked_add_signed(TimeDelta::try_days(WINDOW_FORWARD_DAYS).unwrap_or_default())
        .unwrap_or(today);

    let mut events = Vec::new();
    let mut status = Vec::new();
    // Whether anything was actually read this time round. A fetch that failed
    // carries the last events forward, but those were read for an *older*
    // window — so claiming today's would say "I looked out to here" about days
    // nobody has ever looked at.
    let mut read_something = false;
    for subscription in list.iter().filter(|s| s.enabled) {
        let at = Local::now();
        let outcome = match fetch_one(client, &subscription.url, from, until) {
            Ok(parsed) => {
                read_something = true;
                let count = parsed.occurrences.len();
                events.extend(parsed.occurrences.into_iter().map(|occurrence| OverlayEvent {
                    subscription: subscription.id,
                    day: occurrence.day,
                    start: occurrence.start,
                    end: occurrence.end,
                    all_day: occurrence.all_day,
                    free: occurrence.free,
                    summary: without_repeated_location(&occurrence.name, &occurrence.location),
                    location: occurrence.location,
                    description: occurrence.description,
                }));
                FetchOutcome::Ok { events: count, problems: parsed.problems, title: parsed.name }
            }
            Err(why) => {
                events.extend(
                    previous.events.iter().filter(|event| event.subscription == subscription.id).cloned(),
                );
                FetchOutcome::Failed { why }
            }
        };
        status.push(FetchStatus { subscription: subscription.id, at, outcome });
    }
    let covers = if read_something { Some(Covered { from, until }) } else { previous.covers };
    Overlay::sealed(events, status).covering(covers)
}

/// Characters a calendar server puts between a course and the room it is in.
const TRAILING_SEPARATORS: &str = "-\u{2013}\u{2014},\u{00b7}|@:;/\u{2022}";

/// The room, taken off the end of the summary when the feed says it twice.
///
/// University feeds write the whole itinerary into SUMMARY and then repeat its
/// last part in LOCATION: "MS-C1350, Partial Differential Equations, Luento -
/// L01 - U4 NORDEA - U142" with LOCATION "U4 NORDEA - U142". Both are shown —
/// the room on its own line, because that is the part you are walking towards —
/// and printing it twice costs the line that would have shown the course.
///
/// Only a suffix is taken, and only when something is left: a summary that *is*
/// the room ("U142") keeps its name rather than becoming blank.
fn without_repeated_location(summary: &str, location: &str) -> String {
    let summary = summary.trim();
    let location = location.trim();
    if location.chars().count() < 2 {
        return summary.to_string();
    }
    let Some((cut, _)) = summary.char_indices().rev().nth(location.chars().count() - 1) else {
        return summary.to_string();
    };
    if summary[cut..].to_lowercase() != location.to_lowercase() {
        return summary.to_string();
    }
    let kept = summary[..cut]
        .trim_end_matches(|c: char| c.is_whitespace() || TRAILING_SEPARATORS.contains(c));
    if kept.is_empty() { summary.to_string() } else { kept.to_string() }
}

/* ──────────────────────────────── The service ────────────────────────────── */

enum FeedCommand {
    /// The list changed: fetch now rather than at the next tick. Nobody types
    /// an address and then waits ten minutes to find out whether it was right.
    Subscriptions(Vec<Subscription>),
    Stop,
}

/// The fetching, on its own thread — the weather pattern of §5.6, for the same
/// reason.
///
/// The board loop is one thread answering one request at a time. A fetch that
/// hangs on somebody's slow calendar server must not be that thread's problem,
/// so it is not on it: this owns a thread, publishes behind a lock, counts a
/// version the reader compares, takes commands down a channel, and wakes the
/// UI the way everything else does.
pub struct Feeds {
    overlay: Arc<RwLock<Overlay>>,
    pub version: Arc<AtomicU64>,
    tx: Sender<FeedCommand>,
}

impl Feeds {
    /// The overlay as it stands. A poisoned lock answers with an empty
    /// overlay rather than taking the window down: a calendar that is not
    /// drawn is a smaller loss than a wall that is not.
    pub fn overlay(&self) -> Overlay {
        self.overlay.read().map(|held| held.clone()).unwrap_or_default()
    }

    /// The list changed. Fetches at once.
    pub fn set_subscriptions(&self, list: Vec<Subscription>) {
        let _ = self.tx.send(FeedCommand::Subscriptions(list));
    }
}

impl Drop for Feeds {
    fn drop(&mut self) {
        let _ = self.tx.send(FeedCommand::Stop);
    }
}

/// Start the service. Only ever called by a process that owns its board.
pub fn start(list: Vec<Subscription>, cached: Overlay, wake: Wake) -> Feeds {
    let overlay = Arc::new(RwLock::new(cached));
    let version = Arc::new(AtomicU64::new(0));
    let (tx, rx) = channel::<FeedCommand>();

    let shared = Arc::clone(&overlay);
    let counter = Arc::clone(&version);
    thread::Builder::new()
        .name("taskdeck-calendars".to_string())
        .spawn(move || run(list, rx, shared, counter, wake))
        .ok();

    Feeds { overlay, version, tx }
}

fn run(mut list: Vec<Subscription>, rx: Receiver<FeedCommand>, overlay: Arc<RwLock<Overlay>>, version: Arc<AtomicU64>, wake: Wake) {
    let client = match reqwest::blocking::Client::builder()
        .timeout(FETCH_TIMEOUT)
        // `checked_url` refuses http and says why: a calendar link is a
        // password, and http sends it in clear. Without these two lines that
        // promise is the *calendar server's* to keep, not ours — reqwest
        // follows ten redirects by default and `https_only` is off, so one
        // `302` to `http://` puts the credential on the wire in clear, and one
        // to `http://127.0.0.1/` points the fetcher at whatever else is
        // listening on this machine. Three hops is more than any real feed
        // needs and fewer than a loop.
        .redirect(reqwest::redirect::Policy::limited(3))
        .https_only(true)
        .user_agent(concat!("TaskDeck/", env!("CARGO_PKG_VERSION")))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            eprintln!("taskdeck: no calendar client ({error}); subscriptions are off this run");
            return;
        }
    };

    loop {
        // An empty list costs nothing: no request, no timer, just a wait for
        // somebody to add one.
        if list.iter().any(|subscription| subscription.enabled) {
            let last = overlay.read().map(|held| held.clone()).unwrap_or_default();
            let fresh = fetch_all(&client, &list, &last);
            let changed = overlay
                .read()
                .map(|held| held.digest() != fresh.digest() || held.status != fresh.status)
                .unwrap_or(true);
            if changed {
                if let Ok(mut held) = overlay.write() {
                    *held = fresh;
                }
                version.fetch_add(1, Ordering::Relaxed);
                wake();
            }
        }

        match rx.recv_timeout(REFRESH_EVERY) {
            Ok(FeedCommand::Subscriptions(fresh)) => {
                // Everything a removed or switched-off calendar contributed
                // goes at once, so the day is right before the fetch returns.
                if let Ok(mut held) = overlay.write() {
                    for gone in list.iter().filter(|old| {
                        !fresh.iter().any(|new| new.id == old.id && new.enabled && new.url == old.url)
                    }) {
                        held.forget(gone.id);
                    }
                }
                list = fresh;
                version.fetch_add(1, Ordering::Relaxed);
                wake();
            }
            Ok(FeedCommand::Stop) => break,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).expect("a real date")
    }

    fn event(subscription: u64, day: NaiveDate, start: i32, end: i32, summary: &str) -> OverlayEvent {
        OverlayEvent {
            subscription,
            day,
            start,
            end,
            all_day: false,
            free: false,
            summary: summary.to_string(),
            location: String::new(),
            description: String::new(),
        }
    }

    #[test]
    fn an_address_must_be_https_and_have_a_host() {
        assert_eq!(checked_url(" https://cal.example.com/x.ics "), Ok("https://cal.example.com/x.ics".to_string()));
        // Apple's own scheme for this exact file: rewritten, since that is
        // what a person will paste.
        assert_eq!(checked_url("webcal://cal.example.com/x.ics"), Ok("https://cal.example.com/x.ics".to_string()));
        // The link is the credential, so plain http is refused rather than
        // quietly sending it across a café's network.
        assert!(checked_url("http://cal.example.com/x.ics").unwrap_err().contains("https"));
        assert!(checked_url("").is_err());
        assert!(checked_url("cal.example.com/x.ics").is_err());
        assert!(checked_url("https://").is_err());
        assert!(checked_url("https://localhost/x.ics").is_err(), "no dot, no host");
        assert!(checked_url(&format!("https://a.com/{}", "x".repeat(3000))).is_err());
    }

    #[test]
    fn a_calendar_is_named_after_its_host_until_something_better_turns_up() {
        // Half the feeds in the world send no X-WR-CALNAME — a university's
        // Sisu feed does not — and "Calendar 150" tells nobody anything.
        assert_eq!(name_from_url("https://sisu.helsinki.fi:443/ilmo/x.ics").as_deref(), Some("sisu.helsinki.fi"));
        assert_eq!(name_from_url("https://www.example.com/a.ics").as_deref(), Some("example.com"));
        assert_eq!(name_from_url("https://user:pw@cal.example.com/a.ics").as_deref(), Some("cal.example.com"));
        assert_eq!(name_from_url("not a url").as_deref(), None);

        let host_named = Subscription {
            id: 150,
            name: "sisu.helsinki.fi".into(),
            url: "https://sisu.helsinki.fi:443/ilmo/x.ics".into(),
            color: [1, 2, 3, 255],
            enabled: true,
        };
        // A name nobody chose, so a feed that does introduce itself still may.
        assert!(is_unnamed(&host_named));
        assert!(is_unnamed(&Subscription { name: placeholder_name(150), ..host_named.clone() }));
        // A name a person typed is left alone.
        assert!(!is_unnamed(&Subscription { name: "Uni".into(), ..host_named }));
    }

    #[test]
    fn an_overlay_is_bounded_and_sorted_however_it_was_handed_over() {
        let long = "x".repeat(SUMMARY_MAX_CHARS + 40);
        let overlay = Overlay::sealed(
            vec![
                event(1, day(2026, 9, 10), 600, 660, &long),
                event(1, day(2026, 9, 9), 540, 600, "earlier"),
                // Ends before it starts: not an event.
                event(1, day(2026, 9, 11), 600, 300, "backwards"),
                // Past the end of a day: clamped, not refused.
                event(1, day(2026, 9, 12), 0, 99_999, "long"),
            ],
            Vec::new(),
        );
        let days: Vec<NaiveDate> = overlay.events.iter().map(|e| e.day).collect();
        assert_eq!(days, vec![day(2026, 9, 9), day(2026, 9, 10), day(2026, 9, 12)]);
        assert_eq!(overlay.events.iter().find(|e| e.day == day(2026, 9, 12)).map(|e| e.end), Some(planner::DAY_MINUTES));
        assert!(overlay.events.iter().all(|e| e.summary.chars().count() <= SUMMARY_MAX_CHARS));
    }

    #[test]
    fn the_digest_ignores_the_order_a_feed_listed_things_in_and_ignores_the_clock() {
        // This is the whole of "a refresh that changed nothing must not move
        // the board version".
        let one = Overlay::sealed(
            vec![event(1, day(2026, 9, 10), 600, 660, "A"), event(1, day(2026, 9, 10), 540, 600, "B")],
            Vec::new(),
        );
        let other = Overlay::sealed(
            vec![event(1, day(2026, 9, 10), 540, 600, "B"), event(1, day(2026, 9, 10), 600, 660, "A")],
            Vec::new(),
        );
        assert_eq!(one.digest(), other.digest());

        // The same events fetched at a different moment are the same events.
        let later = Overlay::sealed(
            one.events.clone(),
            vec![FetchStatus {
                subscription: 1,
                at: Local::now(),
                outcome: FetchOutcome::Ok { events: 2, problems: Vec::new(), title: None },
            }],
        );
        assert_eq!(one.digest(), later.digest(), "the time of the last attempt is not a change");

        // A real change is a change.
        let changed = Overlay::sealed(vec![event(1, day(2026, 9, 10), 600, 660, "A renamed")], Vec::new());
        assert_ne!(one.digest(), changed.digest());

        // Including a room that moved. Left out of the digest, a cache written
        // before the field existed is never displaced by a fetch that has it:
        // the events match, so nothing looks different, and the room stays
        // blank. Found in use rather than in a test.
        let mut relocated = one.events.clone();
        if let Some(first) = relocated.first_mut() {
            first.location = "Chemicum, sali A110".into();
        }
        assert_ne!(one.digest(), Overlay::sealed(relocated, Vec::new()).digest());
    }

    #[test]
    fn an_all_day_band_and_a_free_marker_are_drawn_but_never_planned_around() {
        let mut all_day = event(1, day(2026, 9, 10), 0, planner::DAY_MINUTES, "Conference");
        all_day.all_day = true;
        let mut free = event(1, day(2026, 9, 10), 540, 600, "due: Tax return");
        free.free = true;
        let busy = event(1, day(2026, 9, 10), 720, 780, "Dentist");
        let overlay = Overlay::sealed(vec![all_day, free, busy], Vec::new());

        // All three are shown...
        assert_eq!(overlay.events_on(day(2026, 9, 10)).count(), 3);
        // ...and exactly one of them takes an hour of the day away.
        assert_eq!(overlay.anchors_on(day(2026, 9, 10)), vec![(720, 780)]);
        assert!(overlay.anchors_on(day(2026, 9, 11)).is_empty());
    }

    #[test]
    fn forgetting_a_subscription_takes_its_events_and_its_status_with_it() {
        let mut overlay = Overlay::sealed(
            vec![event(1, day(2026, 9, 10), 600, 660, "mine"), event(2, day(2026, 9, 10), 700, 760, "theirs")],
            vec![
                FetchStatus { subscription: 1, at: Local::now(), outcome: FetchOutcome::Ok { events: 1, problems: Vec::new(), title: None } },
                FetchStatus { subscription: 2, at: Local::now(), outcome: FetchOutcome::Failed { why: "no".into() } },
            ],
        );
        overlay.forget(2);
        assert_eq!(overlay.events.len(), 1);
        assert_eq!(overlay.events.first().map(|e| e.subscription), Some(1));
        assert!(overlay.status_for(2).is_none());
        assert!(overlay.status_for(1).is_some());
    }

    /// A calendar server of our own, so the whole path — request, body cap,
    /// parse, overlay — is exercised without reaching the network.
    fn serving(body: &'static str) -> (u16, std::thread::JoinHandle<()>) {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("a port");
        let port = server.server_addr().to_ip().expect("an address").port();
        let handle = std::thread::spawn(move || {
            if let Ok(request) = server.recv() {
                let response = tiny_http::Response::from_string(body).with_header(
                    tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"text/calendar"[..]).expect("a header"),
                );
                let _ = request.respond(response);
            }
        });
        (port, handle)
    }

    #[test]
    fn a_calendar_is_fetched_parsed_and_laid_over_the_day() {
        // The one test that joins the two halves: an address, a real body, and
        // an overlay with an anchor in it at the end.
        let today = Local::now().date_naive();
        let body: &'static str = Box::leak(
            format!(
                "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nX-WR-CALNAME:Work\r\nBEGIN:VEVENT\r\nUID:a@test\r\n\
                 SUMMARY:Dentist\r\nDTSTART:{}T140000\r\nDTEND:{}T150000\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
                today.format("%Y%m%d"),
                today.format("%Y%m%d")
            )
            .into_boxed_str(),
        );
        let (port, handle) = serving(body);

        let client = reqwest::blocking::Client::builder().timeout(FETCH_TIMEOUT).build().expect("a client");
        let list = vec![Subscription {
            id: 7,
            name: "Work".into(),
            // http, not https: `checked_url` guards what a person may type,
            // and this is a loopback server standing in for one.
            url: format!("http://127.0.0.1:{port}/cal.ics"),
            color: [1, 2, 3, 255],
            enabled: true,
        }];
        let overlay = fetch_all(&client, &list, &Overlay::default());
        let _ = handle.join();

        assert_eq!(overlay.events.len(), 1, "{:?}", overlay.status);
        let only = overlay.events.first().expect("one");
        assert_eq!((only.summary.as_str(), only.start, only.end), ("Dentist", 14 * 60, 15 * 60));
        assert_eq!(only.subscription, 7);
        // And it is an hour the day has to be planned around.
        assert_eq!(overlay.anchors_on(today), vec![(14 * 60, 15 * 60)]);
        assert!(matches!(
            overlay.status_for(7).map(|s| &s.outcome),
            Some(FetchOutcome::Ok { events: 1, .. })
        ));
    }

    #[test]
    fn an_address_that_answers_with_rubbish_fails_only_itself() {
        let (port, handle) = serving("<html>not a calendar</html>");
        let client = reqwest::blocking::Client::builder().timeout(FETCH_TIMEOUT).build().expect("a client");
        let list = vec![Subscription {
            id: 3,
            name: "Broken".into(),
            url: format!("http://127.0.0.1:{port}/x.ics"),
            color: [1, 2, 3, 255],
            enabled: true,
        }];
        let overlay = fetch_all(&client, &list, &Overlay::default());
        let _ = handle.join();

        assert!(overlay.events.is_empty());
        // Recorded against the one that failed, in words a person can read.
        assert!(
            matches!(overlay.status_for(3).map(|s| &s.outcome), Some(FetchOutcome::Failed { .. })),
            "{:?}",
            overlay.status
        );
    }

    #[test]
    fn a_span_long_enough_to_describe_the_day_is_drawn_but_never_planned_around() {
        // Straight from a real university feed: a course *period* exported as
        // an event running 08:00–20:00 on every teaching day. Treated as busy
        // it leaves reflow nowhere to put any work at all, which is exactly
        // the failure an all-day band would cause.
        let today = Local::now().date_naive();
        let period = event(1, today, 8 * 60, 20 * 60, "CS-C3240, Machine Learning, Lähiopetus 1.9.–30.11.");
        let lecture = event(1, today, 10 * 60 + 15, 12 * 60, "MOLE-101, Biokemia");
        let overlay = Overlay::sealed(vec![period, lecture], Vec::new());

        assert_eq!(overlay.events_on(today).count(), 2, "both are still shown");
        assert_eq!(
            overlay.anchors_on(today),
            vec![(10 * 60 + 15, 12 * 60)],
            "only the lecture takes an hour out of the day"
        );
        let long = overlay.events.iter().find(|e| e.minutes() >= LONG_EVENT_MINUTES).expect("the period");
        assert!(long.is_background() && !long.is_busy());

        // The other side of the line, and the reason it moved. These are real
        // durations off a student club's feed: an evening that ends at ten and
        // a tournament that runs noon to eight are things you are *at*, not
        // scenery for a day that is otherwise free.
        for (start, end, what) in [
            (16 * 60, 22 * 60, "board game night, six hours"),
            (12 * 60, 20 * 60, "tournament, eight hours"),
            (14 * 60, 24 * 60, "the long one, ten hours"),
            (9 * 60, 13 * 60, "lab, four hours"),
        ] {
            assert!(event(1, today, start, end, what).is_busy(), "{what} is an appointment");
        }
        // And a day carved out whole still is not.
        assert!(event(1, today, 0, 24 * 60, "conference").is_background());
    }

    #[test]
    fn two_meetings_at_the_same_hour_are_given_a_column_each() {
        // Drawn full width before this, so two overlapping meetings landed in
        // exactly the same rectangle and neither could be read.
        let today = Local::now().date_naive();
        let overlay = Overlay::sealed(
            vec![
                event(1, today, 600, 660, "Standup"),
                event(2, today, 615, 675, "Interview"),
                // Later, and clear of both: it gets the width back.
                event(1, today, 800, 860, "Alone"),
            ],
            Vec::new(),
        );
        let placements: Vec<planner::Placement> = overlay
            .events
            .iter()
            .map(|e| planner::Placement::Block { start: e.start, minutes: (e.end - e.start) as u32 })
            .collect();
        let lanes = planner::lay_out(&placements);

        let overlapping: Vec<&planner::Lane> = lanes.iter().take(2).collect();
        assert_eq!(overlapping.iter().map(|l| l.columns).collect::<Vec<_>>(), vec![2, 2]);
        assert_ne!(
            overlapping.first().map(|l| l.column),
            overlapping.get(1).map(|l| l.column),
            "two meetings at the same hour must not share a column"
        );
        assert_eq!(lanes.get(2).map(|l| l.columns), Some(1), "the one that overlaps nothing keeps the width");
    }

    #[test]
    fn a_calendar_that_cannot_be_read_keeps_the_events_it_last_gave() {
        // An outage is not a cancellation. Blanking the day because a tunnel
        // dropped is the same mistake as treating an outage as a deletion,
        // which this program refuses to make anywhere else.
        let today = Local::now().date_naive();
        let previous = Overlay::sealed(vec![event(4, today, 600, 660, "Standup")], Vec::new());
        let client = reqwest::blocking::Client::builder().timeout(Duration::from_millis(300)).build().expect("a client");
        let list = vec![Subscription {
            id: 4,
            name: "Work".into(),
            url: "http://127.0.0.1:1/gone.ics".into(),
            color: [1, 2, 3, 255],
            enabled: true,
        }];
        let overlay = fetch_all(&client, &list, &previous);

        assert_eq!(overlay.events.len(), 1, "yesterday's answer is still the best answer there is");
        assert_eq!(overlay.events.first().map(|e| e.summary.as_str()), Some("Standup"));
        // And the desk is told, so the staleness is visible rather than silent.
        assert!(matches!(overlay.status_for(4).map(|s| &s.outcome), Some(FetchOutcome::Failed { .. })));
    }

    #[test]
    fn a_calendar_that_is_switched_off_is_not_even_asked_for() {
        let client = reqwest::blocking::Client::builder().timeout(FETCH_TIMEOUT).build().expect("a client");
        let list = vec![Subscription {
            id: 1,
            name: "Off".into(),
            // Nothing listens here; if it were asked, this would take the
            // timeout and come back Failed.
            url: "http://127.0.0.1:1/none.ics".into(),
            color: [1, 2, 3, 255],
            enabled: false,
        }];
        let overlay = fetch_all(&client, &list, &Overlay::default());
        assert!(overlay.events.is_empty());
        assert!(overlay.status.is_empty(), "a calendar that is off is not a calendar that failed");
    }

    #[test]
    fn the_list_and_the_cache_round_trip_through_a_folder() {
        let dir = tempfile::tempdir().expect("a temp dir");
        assert!(read(dir.path()).expect("an empty folder reads").is_empty());
        assert!(read_overlay(dir.path()).is_none());

        let list = vec![Subscription {
            id: 1,
            name: "Work".into(),
            url: "https://cal.example.com/w.ics".into(),
            color: [10, 20, 30, 255],
            enabled: true,
        }];
        save(&list, dir.path()).expect("saves");
        assert_eq!(read(dir.path()).expect("reads back"), list);

        let overlay = Overlay::sealed(vec![event(1, day(2026, 9, 10), 600, 660, "Standup")], Vec::new());
        save_overlay(&overlay, dir.path()).expect("saves");
        assert_eq!(read_overlay(dir.path()).expect("reads back").digest(), overlay.digest());

        // Derived data: an unreadable cache is nothing to report.
        fs::write(dir.path().join(OVERLAY_FILE), "{not json").expect("writes");
        assert!(read_overlay(dir.path()).is_none());
    }

    #[test]
    fn the_span_the_calendars_were_read_for_survives_the_cache() {
        // `sealed` keeps only what it is handed, so a restart is exactly where
        // this gets lost, and losing it makes the phone call known days unknown.
        let dir = tempfile::tempdir().expect("a temp dir");
        let covers = Covered { from: day(2026, 7, 7), until: day(2027, 10, 10) };
        let overlay = Overlay::sealed(vec![event(1, day(2026, 9, 4), 600, 660, "Lecture")], Vec::new())
            .covering(Some(covers));
        save_overlay(&overlay, dir.path()).expect("writes");
        assert_eq!(read_overlay(dir.path()).and_then(|back| back.covers), Some(covers));

        // And a cache written before the field existed still reads.
        let older = r#"{"events":[],"status":[]}"#;
        fs::write(dir.path().join(OVERLAY_FILE), older).expect("writes");
        let back = read_overlay(dir.path()).expect("reads");
        assert_eq!(back.covers, None);
    }

    #[test]
    fn a_fetch_that_read_nothing_claims_no_more_than_the_last_one_did() {
        // The events carried forward were read for the window of whenever they
        // last arrived. A desk that has been off a week and comes back with the
        // feed still unreachable must not claim it has looked a week further
        // ahead than anybody has.
        let stale = Covered { from: day(2026, 1, 1), until: day(2026, 6, 1) };
        let previous = Overlay::sealed(vec![event(1, day(2026, 2, 2), 600, 660, "Lecture")], Vec::new())
            .covering(Some(stale));
        let client = reqwest::blocking::Client::builder().timeout(Duration::from_millis(300)).build().expect("a client");
        let list = vec![Subscription {
            id: 1,
            name: "Timetable".into(),
            url: "http://127.0.0.1:1/gone.ics".into(),
            color: DEFAULT_COLORS[0],
            enabled: true,
        }];
        let overlay = fetch_all(&client, &list, &previous);
        assert_eq!(overlay.covers, Some(stale), "an unread window is not a read one");
        // And the events it could not refresh are still there.
        assert_eq!(overlay.events.len(), 1);
    }

    #[test]
    fn the_span_is_not_part_of_what_counts_as_a_change() {
        // It slides forward every midnight. Folding it into the digest would
        // make every refresh a change and wake every parked phone, which is the
        // one thing the digest exists to prevent.
        let events = vec![event(1, day(2026, 9, 4), 600, 660, "Lecture")];
        let monday = Overlay::sealed(events.clone(), Vec::new())
            .covering(Some(Covered { from: day(2026, 7, 7), until: day(2027, 10, 10) }));
        let tuesday = Overlay::sealed(events, Vec::new())
            .covering(Some(Covered { from: day(2026, 7, 8), until: day(2027, 10, 11) }));
        assert_eq!(monday.digest(), tuesday.digest());
    }

    #[test]
    fn a_day_is_inside_the_span_at_both_of_its_ends() {
        // Inclusive at both ends, matching `ics::parse`. A day out here would
        // grey a day the calendars were read for.
        let covers = Covered { from: day(2026, 7, 7), until: day(2027, 10, 10) };
        assert!(covers.holds(day(2026, 7, 7)));
        assert!(covers.holds(day(2027, 10, 10)));
        assert!(!covers.holds(day(2026, 7, 6)));
        assert!(!covers.holds(day(2027, 10, 11)));
    }

    #[test]
    fn a_room_the_feed_names_twice_is_only_shown_once() {
        // Both real university feeds do this, with different separators.
        assert_eq!(
            without_repeated_location(
                "MS-C1350, Partial Differential Equations - Luento - L01 - U4 NORDEA - U142",
                "U4 NORDEA - U142",
            ),
            "MS-C1350, Partial Differential Equations - Luento - L01",
        );
        assert_eq!(
            without_repeated_location("KEK101, Atomit - Luennot - Chemicum, sali A110", "Chemicum, sali A110"),
            "KEK101, Atomit - Luennot",
        );
        // Case and stray space are the feed's business, not the reader's.
        assert_eq!(without_repeated_location("Seminar \u{00b7} Exactum  ", " exactum"), "Seminar");
    }

    #[test]
    fn a_summary_that_is_only_the_room_keeps_its_name() {
        // Trimming here would leave the event with nothing to be called.
        assert_eq!(without_repeated_location("U142", "U142"), "U142");
        assert_eq!(without_repeated_location("- U142", "U142"), "- U142");
        // A room named elsewhere in the line is not a suffix and is left alone.
        assert_eq!(without_repeated_location("U142 lecture", "U142"), "U142 lecture");
        // Nothing to take: no location, or one too short to be one.
        assert_eq!(without_repeated_location("Standup", ""), "Standup");
        assert_eq!(without_repeated_location("Room A", "A"), "Room A");
        // A location longer than the whole summary cannot be its tail.
        assert_eq!(without_repeated_location("A110", "Chemicum, sali A110"), "A110");
    }
}
