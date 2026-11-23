//! Property tests of `apply` (core.md §18.5, brief b2-apply): over boards that
//! pass `check` and commands that `parse_command` accepts, `apply` never
//! panics, leaves a board that passes `check` with its archive in archive
//! order, changes nothing when it refuses or finds a benign repeat, and gives
//! equal results and bytes for equal inputs.

use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;
use taskdeck_core::apply::{Applied, Ctx, Refusal, apply};
use taskdeck_core::board::{ArchiveRow, Block, BoardData, Item, ItemStamps, Kind, Mark, Outcome, Recurrence, RowBlock, Stamp, Subscription};
use taskdeck_core::canon::Record;
use taskdeck_core::command::{BlockMove, CreateEvent, CreateRoutine, CreateTask, LogCommand, NewBlock, parse_command};
use taskdeck_core::time::Instant;

const L: u64 = 1 << 40;

fn at(minutes: i64) -> Instant {
    Instant::from_parts(minutes * 60, 0).expect("in range")
}

/// An id of the small pool the boards and commands share: devices 0 to 2,
/// low parts 0 to 4 (0 only reaches `restore` and the parser).
fn id() -> impl Strategy<Value = u64> {
    (0u64..3, 0u64..5).prop_map(|(d, n)| d * L + n)
}

/// An id a command mints: mostly above the generated marks.
fn fresh() -> impl Strategy<Value = u64> {
    prop_oneof![3 => (1u64..4, 5u64..9).prop_map(|(d, n)| d * L + n), 1 => id()]
}

fn instant() -> impl Strategy<Value = Instant> {
    (0i64..6).prop_map(at)
}

fn stamp() -> impl Strategy<Value = Stamp> {
    (1u64..30, 0u16..4).prop_map(|(seq, device_no)| Stamp { seq, device_no })
}

fn name() -> impl Strategy<Value = String> {
    "[a-z]{1,4}"
}

fn len() -> impl Strategy<Value = u32> {
    prop_oneof![Just(15u32), Just(1440u32), 15u32..1441]
}

#[derive(Clone, Debug)]
struct RawItem {
    id: (u64, u64),
    kind: u8,
    importance: Option<u8>,
    blocks: Vec<((u64, u64), Stamp)>,
    stamps: [Stamp; 5],
}

#[derive(Clone, Debug)]
struct RawRow {
    id: (u64, u64),
    kind: u8,
    archived_at: i64,
    dropped: bool,
    sids: Vec<(u64, u64)>,
}

fn raw_item() -> impl Strategy<Value = RawItem> {
    (
        (0u64..3, 1u64..5),
        0u8..3,
        proptest::option::of(0u8..5),
        proptest::collection::vec(((0u64..3, 1u64..5), stamp()), 0..3),
        [stamp(), stamp(), stamp(), stamp(), stamp()],
    )
        .prop_map(|(id, kind, importance, blocks, stamps)| RawItem { id, kind, importance, blocks, stamps })
}

fn raw_row() -> impl Strategy<Value = RawRow> {
    ((0u64..3, 0u64..5), 0u8..3, 0i64..4, any::<bool>(), proptest::collection::vec((0u64..3, 1u64..5), 0..3))
        .prop_map(|(id, kind, archived_at, dropped, sids)| RawRow { id, kind, archived_at, dropped, sids })
}

fn kind_of(k: u8) -> Kind {
    match k {
        0 => Kind::Task,
        1 => Kind::Event,
        _ => Kind::Routine,
    }
}

const REC: Recurrence = Recurrence {
    days: 3,
    anchor: match chrono::NaiveDate::from_ymd_opt(2026, 1, 5) {
        Some(d) => d,
        None => panic!("a date"),
    },
    start_minutes: 60,
    minutes: 30,
};

/// A board that passes `check`: ids distinct, live sids distinct, the
/// archive in archive order, marks at or above every id.
fn board() -> impl Strategy<Value = BoardData> {
    (
        proptest::collection::vec(raw_item(), 0..10),
        proptest::collection::vec(raw_row(), 0..5),
        proptest::collection::vec(((0u64..3, 1u64..5), any::<bool>()), 0..3),
        0u64..4,
        proptest::collection::vec((0u16..3, 0u64..3, 0u64..3), 0..3),
    )
        .prop_map(|(raw_items, raw_rows, raw_subs, notes_rev, extra)| {
            let mut b = BoardData::empty();
            let mut items: BTreeMap<u64, Item> = BTreeMap::new();
            let mut live_sids = BTreeSet::new();
            for r in raw_items {
                let id = r.id.0 * L + r.id.1;
                if items.contains_key(&id) {
                    continue;
                }
                let kind = kind_of(r.kind);
                let mut blocks: Vec<Block> = Vec::new();
                for ((d, n), s) in r.blocks {
                    let sid = d * L + n;
                    if live_sids.insert(sid) {
                        blocks.push(Block { sid, start: at(1), minutes: 30, stamp: s });
                    }
                }
                blocks.sort_by_key(|x| x.sid);
                let [estimate, levels, nm, repeat, time] = r.stamps;
                items.insert(
                    id,
                    Item {
                        id,
                        kind,
                        name: "i".to_owned(),
                        created: at(0),
                        deadline: (kind == Kind::Event).then(|| at(2)),
                        importance: r.importance,
                        time_importance: None,
                        duration_minutes: None,
                        recurrence: (kind == Kind::Routine).then_some(REC),
                        blocks,
                        stamps: ItemStamps { estimate, levels, name: nm, repeat, time },
                    },
                );
            }
            b.items = items.into_values().collect();
            let mut rows: Vec<ArchiveRow> = raw_rows
                .into_iter()
                .map(|r| {
                    let kind = kind_of(r.kind);
                    let mut sids: Vec<u64> = r.sids.iter().map(|&(d, n)| d * L + n).collect();
                    sids.sort_unstable();
                    sids.dedup();
                    ArchiveRow {
                        id: if r.id.1 == 0 { 0 } else { r.id.0 * L + r.id.1 },
                        kind,
                        name: "r".to_owned(),
                        created: at(0),
                        deadline: None,
                        importance: None,
                        time_importance: None,
                        duration_minutes: None,
                        recurrence: (kind == Kind::Routine).then_some(REC),
                        blocks: sids.into_iter().map(|sid| RowBlock { sid, start: at(3), minutes: 15 }).collect(),
                        archived_at: at(r.archived_at),
                        outcome: if r.dropped { Outcome::Dropped } else { Outcome::Finished },
                    }
                })
                .collect();
            rows.sort_by_key(|r| (r.archived_at, r.id, r.to_canonical()));
            b.archive = rows;
            let mut subs: BTreeMap<u64, Subscription> = BTreeMap::new();
            for ((d, n), enabled) in raw_subs {
                let id = d * L + n;
                subs.insert(id, Subscription { id, name: "s".to_owned(), url: format!("https://e.example/{id}"), color: [0; 4], enabled });
            }
            b.subscriptions = subs.into_values().collect();
            b.notes_rev = notes_rev;
            // The marks: every id's low part, plus a little room.
            let mut marks: BTreeMap<u16, (u64, u64)> = BTreeMap::new();
            let mut see = |id: u64, sid: bool| {
                let (h, n) = (u16::try_from(id / L).expect("high part"), id % L);
                let m = marks.entry(h).or_default();
                if sid {
                    m.1 = m.1.max(n);
                } else {
                    m.0 = m.0.max(n);
                }
            };
            for i in &b.items {
                see(i.id, false);
                i.blocks.iter().for_each(|x| see(x.sid, true));
            }
            for r in &b.archive {
                if r.id != 0 {
                    see(r.id, false);
                }
                r.blocks.iter().for_each(|x| see(x.sid, true));
            }
            b.subscriptions.iter().for_each(|s| see(s.id, false));
            for (d, i, k) in extra {
                let m = marks.entry(d).or_default();
                m.0 += i;
                m.1 += k;
            }
            b.marks = marks
                .into_iter()
                .filter(|&(_, (i, k))| (i, k) != (0, 0))
                .map(|(device_no, (item, block))| Mark { device_no, item, block })
                .collect();
            b
        })
}

fn command() -> impl Strategy<Value = LogCommand> {
    let creates = prop_oneof![
        (fresh(), name(), proptest::option::of(instant()), proptest::option::of(0u8..5), proptest::option::of(len()),
         proptest::collection::vec((fresh(), instant(), len()), 0..3))
            .prop_map(|(id, name, deadline, importance, duration_minutes, blocks)| {
                let mut blocks: Vec<NewBlock> = blocks.into_iter().map(|(sid, start, minutes)| NewBlock { sid, start, minutes }).collect();
                blocks.sort_by_key(|b| b.sid);
                blocks.dedup_by_key(|b| b.sid);
                LogCommand::CreateTask(CreateTask { id, name, deadline, importance, time_importance: None, duration_minutes, blocks })
            }),
        (fresh(), name(), instant(), proptest::option::of(len()), proptest::option::of(0u8..4)).prop_map(|(id, name, start, minutes, t)| {
            LogCommand::CreateEvent(CreateEvent { id, name, start, minutes, importance: None, time_importance: t })
        }),
        (fresh(), name(), 0u8..128, 0i32..1426, len()).prop_map(|(id, name, days, start_minutes, minutes)| {
            LogCommand::CreateRoutine(CreateRoutine { id, name, days, anchor: REC.anchor, start_minutes, minutes })
        }),
        (id(), name()).prop_map(|(id, name)| LogCommand::Rename { id, name }),
        (id(), instant()).prop_map(|(id, at)| LogCommand::Complete { id, at }),
        (id(), instant()).prop_map(|(id, at)| LogCommand::Delete { id, at }),
        id().prop_map(|id| LogCommand::Forget { id }),
    ];
    let blocks = prop_oneof![
        (id(), id(), instant(), len()).prop_map(|(id, sid, start, minutes)| LogCommand::MoveBlock { id, sid, start, minutes }),
        proptest::collection::vec((id(), id(), instant(), 0u32..1441), 1..4).prop_map(|m| {
            let mut seen = BTreeSet::new();
            let moves = m
                .into_iter()
                .filter(|x| seen.insert(x.1))
                .map(|(id, sid, start, minutes)| BlockMove { id, sid, start, minutes })
                .collect();
            LogCommand::MoveBlocks { moves }
        }),
        (id(), fresh(), instant(), len()).prop_map(|(id, sid, start, minutes)| LogCommand::AddBlock { id, sid, start, minutes }),
        proptest::collection::vec((id(), id()), 1..4).prop_map(|mut p| {
            p.sort_by_key(|x| x.1);
            p.dedup_by_key(|x| x.1);
            LogCommand::RemoveBlocks { blocks: p }
        }),
        (id(), len()).prop_map(|(id, minutes)| LogCommand::SetEstimate { id, minutes }),
        (id(), proptest::option::of(instant()), proptest::option::of(0u8..5))
            .prop_map(|(id, deadline, seed)| LogCommand::SetDeadline { id, deadline, seed }),
        (id(), 0u8..5).prop_map(|(id, level)| LogCommand::SetSeverity { id, level }),
        (id(), 0u8..4).prop_map(|(id, level)| LogCommand::SetHorizon { id, level }),
        (id(), 0u8..128).prop_map(|(id, days)| LogCommand::SetRepeat { id, days, anchor: REC.anchor }),
        (id(), 0i32..1426, len()).prop_map(|(id, start_minutes, minutes)| LogCommand::MoveRoutine { id, start_minutes, minutes }),
    ];
    let rest = prop_oneof![
        (id(), instant(), len()).prop_map(|(id, start, minutes)| LogCommand::SetEventSpan { id, start, minutes }),
        ("[a-z ]{0,6}", 0u64..5).prop_map(|(text, base)| LogCommand::SetNotes { text, base }),
        (id(), instant(), fresh()).prop_map(|(id, archived_at, fallback)| LogCommand::Restore { id, archived_at, fallback }),
        (id(), instant()).prop_map(|(id, archived_at)| LogCommand::ForgetArchived { id, archived_at }),
        (fresh(), name(), 0u64..4).prop_map(|(id, name, u)| LogCommand::AddSubscription {
            id,
            name,
            url: format!("https://e.example/{}", L + u),
            color: [1; 4]
        }),
        id().prop_map(|id| LogCommand::RemoveSubscription { id }),
        (id(), name()).prop_map(|(id, name)| LogCommand::RenameSubscription { id, name }),
        (id(), any::<u8>()).prop_map(|(id, c)| LogCommand::SetSubscriptionColor { id, color: [c; 4] }),
        (id(), any::<bool>()).prop_map(|(id, enabled)| LogCommand::SetSubscriptionEnabled { id, enabled }),
        prop_oneof![
            any::<u8>().prop_map(|c| LogCommand::SetPalette { colors: [[c; 4]; 6] }),
            (-9000i32..9001, -18000i32..18001).prop_map(|(lat, lon)| LogCommand::SetHome { lat, lon }),
            Just(LogCommand::SetPicture { picture: None }),
        ],
    ];
    // Only what `parse_command` accepts reaches `apply`.
    prop_oneof![creates, blocks, rest]
        .prop_filter_map("parses", |c| parse_command(&c.to_canonical()).ok().filter(|p| *p == c))
}

/// A run of entries: each command with a context whose seq rises.
fn entries() -> impl Strategy<Value = Vec<(LogCommand, u16, u64, i64)>> {
    proptest::collection::vec((command(), 1u16..3, 0u64..100, 0i64..5), 1..7)
}

fn ctx_for(k: usize, device_no: u16, back: u64, at_min: i64) -> Ctx {
    let seq = 30 + u64::try_from(k).expect("index");
    Ctx { seq, device_no, at: at(at_min), based_on: seq - 1 - back % seq }
}

type Outcome1 = Result<Applied, Refusal>;

fn run(board: &mut BoardData, list: &[(LogCommand, u16, u64, i64)]) -> Vec<(Outcome1, Vec<u8>)> {
    let mut out = Vec::new();
    for (k, (c, d, back, m)) in list.iter().enumerate() {
        let r = apply(board, c, &ctx_for(k, *d, *back, *m));
        out.push((r, board.to_canonical_bytes()));
    }
    out
}

proptest! {
    // spec: CORE-GEN-008
    // spec: CORE-DATA-006
    // spec: CORE-DATA-003
    // spec: CORE-APPLY-002
    // spec: CORE-APPLY-004
    #[test]
    fn apply_keeps_the_invariants(start in board(), list in entries()) {
        prop_assert!(start.check().is_ok(), "the generator makes checked boards: {:?}", start.check());
        let mut b = start;
        for (k, (c, d, back, m)) in list.iter().enumerate() {
            let before = b.to_canonical_bytes();
            let r = apply(&mut b, c, &ctx_for(k, *d, *back, *m));
            let after = b.to_canonical_bytes();
            prop_assert!(b.check().is_ok(), "{:?} after {:?}", b.check(), c);
            prop_assert!(b.archive.windows(2).all(|w| (w[0].archived_at, w[0].id) <= (w[1].archived_at, w[1].id)));
            match r {
                Err(_) => prop_assert_eq!(&after, &before, "a refusal changes nothing"),
                Ok(a) if a.benign => {
                    prop_assert_eq!(&after, &before, "a benign repeat changes nothing");
                    prop_assert!(a.notices.is_empty() && a.changes == Default::default());
                }
                Ok(_) => {}
            }
        }
    }

    // spec: CORE-CTX-002
    // spec: CORE-CONF-007
    #[test]
    fn equal_inputs_give_equal_results(start in board(), list in entries()) {
        let mut one = start.clone();
        let mut two = start;
        prop_assert_eq!(run(&mut one, &list), run(&mut two, &list));
        prop_assert_eq!(one.to_canonical_bytes(), two.to_canonical_bytes());
    }
}
