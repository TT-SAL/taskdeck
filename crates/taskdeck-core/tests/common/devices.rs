//! Devices that compile random intents against their own views, for the
//! determinism and rebase property tests (core.md §18.6).
//!
//! A `Step` is drawn by proptest without a board; `intent_of` turns it into
//! an `Intent` against the view the device holds when the step runs, picking
//! the ids, sids, archive keys and subscriptions it names from that view, so
//! that most intents compile and many touch what another device touched.

use proptest::prelude::*;
use taskdeck_core::apply::{Applied, Ctx, Refusal, apply};
use taskdeck_core::board::{BoardData, Kind, PictureRef};
use taskdeck_core::command::LogCommand;
use taskdeck_core::compile::{CompileCtx, Compiled, compile};
use taskdeck_core::ids::IdMinter;
use taskdeck_core::intent::{Intent, WireTime};
use taskdeck_core::overlay::Overlay;
use taskdeck_core::time::{Instant, Zone};

/// The first instant of every run: the fixtures' `now` (core.md §18.5).
pub fn start_instant() -> Instant {
    Instant::parse_canonical("2026-10-20T08:15:00.000000000Z").expect("an instant")
}

/// One thing a device does: perhaps take the confirmed state as its view,
/// then compile one intent and, when it gives a command, apply it to its own
/// view (its pending state) and send it.
#[derive(Clone, Debug)]
pub struct Step {
    pub device: usize,
    pub sync: bool,
    /// Minutes the clock moves on before this step.
    pub advance: u32,
    pub op: u8,
    pub pick: usize,
    pub pick2: usize,
    /// Days after 2026-10-19 (around a fold of Berlin's, 2026-10-25, and
    /// New York's, 2026-11-01).
    pub day: u32,
    pub start: i32,
    pub minutes: u32,
    pub text: String,
    pub naive: bool,
    pub flag: bool,
    pub level: u8,
}

pub fn a_step() -> impl Strategy<Value = Step> {
    // Edits of live items (ops 3 to 16) and `add` (0) weigh more, and small
    // picks most, so that devices often touch the same item and group.
    let op = prop_oneof![2 => 0u8..28, 2 => 3u8..17, 1 => Just(0u8)];
    let pick = prop_oneof![4 => 0usize..2, 1 => any::<usize>()];
    let start = prop_oneof![4 => (0i32..96).prop_map(|q| q * 15), 1 => -30i32..1500];
    let minutes = prop_oneof![15u32..241, 0u32..1500];
    (
        (0usize..3, prop::bool::weighted(0.15), 0u32..600),
        (op, pick, any::<usize>()),
        (0u32..16, start, minutes),
        ("[a-zé\t ]{0,8}", any::<bool>(), any::<bool>(), 0u8..8),
    )
        .prop_map(|((device, sync, advance), (op, pick, pick2), (day, start, minutes), (text, naive, flag, level))| {
            Step { device, sync, advance, op, pick, pick2, day, start, minutes, text, naive, flag, level }
        })
}

/// The `k`-th entry (modulo the length), from the start when `front` and
/// from the end otherwise: the legacy ids come first and every view holds
/// them, the ids the devices minted come last; small `k` lands where devices
/// collide.
fn pick<T>(list: &[T], k: usize, front: bool) -> Option<&T> {
    if list.is_empty() {
        None
    } else if front {
        list.get(k % list.len())
    } else {
        list.get(list.len() - 1 - k % list.len())
    }
}

/// An intent of every op of §12.3, its ids taken from `view`.
pub fn intent_of(step: &Step, view: &BoardData, now: Instant) -> Intent {
    let day = chrono::NaiveDate::from_ymd_opt(2026, 10, 19).expect("a date") + chrono::Days::new(u64::from(step.day));
    let front = step.pick2.is_multiple_of(2);
    let item = pick(&view.items, step.pick, front).map_or(1, |i| i.id);
    let task_block = {
        let tasks: Vec<_> = view.items.iter().filter(|i| !i.blocks.is_empty()).collect();
        pick(&tasks, step.pick, front).map(|i| (i.id, pick(&i.blocks, step.pick2 / 2, true).map_or(1, |b| b.sid)))
    };
    let row = pick(&view.archive, step.pick, front).map(|r| (r.id, r.archived_at.to_chrono().fixed_offset()));
    let sub = pick(&view.subscriptions, step.pick, front).map_or(1, |s| s.id);
    let wire = || -> Option<WireTime> {
        if step.flag && step.level == 0 {
            return None;
        }
        Some(if step.naive {
            let minutes = step.start.rem_euclid(1440);
            let (hour, minute) = (u32::try_from(minutes / 60).expect("hour"), u32::try_from(minutes % 60).expect("minute"));
            let local = day.and_hms_opt(hour, minute, 0).expect("a time");
            WireTime::Naive(local, local.format("%Y-%m-%dT%H:%M").to_string())
        } else {
            let offset = chrono::FixedOffset::east_opt(i32::from(step.level) * 3600 - 7200).expect("an offset");
            let shift = i64::from(step.start) * 60;
            WireTime::Exact(now.plus_seconds(shift).unwrap_or(now).to_chrono().with_timezone(&offset))
        })
    };
    let name = step.text.clone();
    let level = step.level;
    match step.op {
        0 => Intent::Add {
            name,
            deadline: wire(),
            importance: Some(level % 5),
            is_event: step.flag,
            time_importance: Some(level % 4),
        },
        1 => Intent::QuickAdd { name, deadline: wire() },
        2 => Intent::Create {
            kind: [Kind::Task, Kind::Event, Kind::Routine][usize::from(level % 3)],
            name,
            day,
            start: step.start,
            minutes: step.minutes,
        },
        3 => Intent::Rename { id: item, name },
        4 => Intent::Complete { id: item, at: wire() },
        5 => Intent::Delete { id: item, at: wire() },
        6 => Intent::Forget { id: item },
        7 => {
            let (id, sid) = task_block.unwrap_or((item, 1));
            Intent::MoveBlock { id, session: step.flag.then_some(sid), day, start: step.start, minutes: step.minutes }
        }
        8 => Intent::AddBlock { id: item, day, start: step.start, minutes: step.flag.then_some(step.minutes) },
        9 => {
            let (id, session) = task_block.unwrap_or((item, 1));
            Intent::RemoveBlock { id, session }
        }
        10 => Intent::Unplan { id: task_block.map_or(item, |t| t.0) },
        11 => Intent::SetEstimate { id: item, minutes: step.minutes },
        12 => Intent::SetDeadline { id: item, deadline: wire() },
        13 => Intent::SetSeverity { id: item, level: level % 6 },
        14 => Intent::SetHorizon { id: item, level: level % 5 },
        15 => Intent::SetRepeat { id: item, days: level.wrapping_mul(37) & 0x7f, day },
        16 => Intent::Reflow { day, from: step.flag.then_some(step.start) },
        17 => Intent::SetNotes { text: step.text.repeat(3), base: step.flag.then_some(view.notes_rev) },
        18 => match row {
            Some((id, archived_at)) => Intent::Restore { id, archived_at },
            None => Intent::Forget { id: item },
        },
        19 => match row {
            Some((id, archived_at)) => Intent::ForgetArchived { id, archived_at },
            None => Intent::Forget { id: item },
        },
        20 => Intent::AddSubscription {
            name,
            url: format!("https://calendar.example/{}.ics", step.pick % 7),
            color: step.flag.then_some([level, 2, 3, 255]),
        },
        21 => Intent::RemoveSubscription { id: sub },
        22 => Intent::RenameSubscription { id: sub, name },
        23 => Intent::SetSubscriptionColor { id: sub, color: [level, 9, 9, 255] },
        24 => Intent::SetSubscriptionEnabled { id: sub, enabled: step.flag },
        25 => Intent::SetPalette { colors: [[level, 1, 2, 255]; 6] },
        26 => Intent::SetHome { lat: f64::from(step.start) / 20.0, lon: f64::from(step.minutes) / 10.0 },
        _ => Intent::SetPicture {
            picture: step.flag.then_some(PictureRef {
                blob: [level; 16],
                epoch: 1,
                parts: 1,
                sha256: [7; 32],
                size: 1000,
            }),
        },
    }
}

/// A device: its `device_no`, zone, minter, and its view (the confirmed
/// state at `head` with its own commands since applied to it).
pub struct Device {
    pub no: u16,
    pub zone: Box<dyn Zone>,
    pub minter: IdMinter,
    pub view: BoardData,
    pub head: u64,
}

impl Device {
    pub fn new(no: u16, zone: Box<dyn Zone>, view: &BoardData, head: u64) -> Device {
        Device { no, zone, minter: IdMinter::new(no), view: view.clone(), head }
    }

    /// Compiles the step's intent against the view at `now`; on a command,
    /// applies it to the view with the provisional `seq` (the pending state)
    /// and returns it with the context it is sent with.
    pub fn compile_step(&mut self, step: &Step, now: Instant, seq: u64) -> Option<(LogCommand, Ctx)> {
        let intent = intent_of(step, &self.view, now);
        let overlay = Overlay::default();
        let cx = CompileCtx { view: &self.view, zone: &*self.zone, now, device_no: self.no, overlay: &overlay };
        let Ok(Compiled { command: Some(command), .. }) = compile(&intent, &cx, &mut self.minter) else {
            return None;
        };
        let ctx = Ctx { seq, device_no: self.no, at: now, based_on: self.head };
        let _ = apply(&mut self.view, &command, &ctx);
        Some((command, ctx))
    }
}

/// One entry applied to a replica: its result, and the board bytes unchanged
/// when it was refused (CORE-APPLY-002).
pub fn apply_checked(board: &mut BoardData, command: &LogCommand, ctx: &Ctx) -> Result<Applied, Refusal> {
    let before = board.to_canonical_bytes();
    let result = apply(board, command, ctx);
    match &result {
        Err(_) => assert_eq!(board.to_canonical_bytes(), before, "a refusal changed the board"),
        Ok(a) if a.benign => assert_eq!(board.to_canonical_bytes(), before, "a benign repeat changed the board"),
        Ok(_) => {}
    }
    result
}
