//! Property tests of task a-base (core.md §18.5): the parsers never panic,
//! loading changes nothing, `encode` then `parse` is the identity, a minter
//! never repeats, and every zone finds an instant again from its wall time.

use proptest::prelude::*;
use serde_json::Value;
use taskdeck_core::board::{
    ArchiveRow, Block, BoardData, Home, Item, ItemStamps, Kind, Mark, Outcome, PictureRef, Recurrence, RowBlock, Stamp,
    Subscription,
};
use taskdeck_core::canon::{self, Record};
use taskdeck_core::ids::{self, IdMinter};
use taskdeck_core::time::{self, FixedZone, Instant, LocalTime, TableZone, Zone};

const BOARD_BYTES: &str = include_str!("vectors/board-bytes.json");
const MAX_INT: i64 = (1 << 53) - 1;
const CODES: [&str; 5] = ["core/too_large", "core/not_json", "core/schema", "core/not_canonical", "core/invariant"];

/// The accepted board texts of `board-bytes.json`: seeds for mutation.
fn seeds() -> Vec<Vec<u8>> {
    let v: Value = serde_json::from_str(BOARD_BYTES).expect("vectors");
    v["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .filter(|c| c.get("error").is_none())
        .filter_map(|c| c["board_text"].as_str().map(|t| t.as_bytes().to_vec()))
        .filter(|t| t.len() < 20_000)
        .collect()
}

#[derive(Clone, Debug)]
enum Edit {
    Flip(usize, u8),
    Insert(usize, u8),
    Delete(usize),
    Truncate(usize),
}

fn edit() -> impl Strategy<Value = Edit> {
    let byte = prop_oneof![any::<u8>(), prop::sample::select(b"{}[]\",:-0123456789eEtfnu\\ ".to_vec())];
    prop_oneof![
        (any::<usize>(), byte.clone()).prop_map(|(p, b)| Edit::Flip(p, b)),
        (any::<usize>(), byte).prop_map(|(p, b)| Edit::Insert(p, b)),
        any::<usize>().prop_map(Edit::Delete),
        any::<usize>().prop_map(Edit::Truncate),
    ]
}

fn mutate(mut bytes: Vec<u8>, edits: &[Edit]) -> Vec<u8> {
    for e in edits {
        let n = bytes.len().max(1);
        match *e {
            Edit::Flip(p, b) if !bytes.is_empty() => bytes[p % n] = b,
            Edit::Insert(p, b) => bytes.insert(p % (bytes.len() + 1), b),
            Edit::Delete(p) if !bytes.is_empty() => {
                bytes.remove(p % n);
            }
            Edit::Truncate(p) => bytes.truncate(p % (bytes.len() + 1)),
            _ => {}
        }
    }
    bytes
}

fn known(r: Result<(), taskdeck_core::CoreError>) -> bool {
    r.map_or_else(|e| CODES.contains(&e.code), |()| true)
}

proptest! {
    // (the canonical parser and the board-bytes parser on random input)
    // spec: CORE-GEN-007
    #[test]
    fn parsers_never_panic_on_random_bytes(bytes in prop::collection::vec(any::<u8>(), 0..400)) {
        prop_assert!(known(canon::parse(&bytes, 1000).map(|_| ())));
        prop_assert!(known(BoardData::from_canonical_bytes(&bytes).map(|_| ())));
    }

    // (the same on mutated canonical boards)
    // spec: CORE-GEN-007
    #[test]
    fn parsers_never_panic_on_mutated_boards(seed in any::<prop::sample::Index>(), edits in prop::collection::vec(edit(), 1..6)) {
        let seeds = seeds();
        let bytes = mutate(seed.get(&seeds).clone(), &edits);
        prop_assert!(known(canon::parse(&bytes, 100_000).map(|_| ())));
        let loaded = BoardData::from_canonical_bytes(&bytes);
        prop_assert!(known(loaded.as_ref().map(|_| ()).map_err(Clone::clone)));
        if let Ok(board) = loaded {
            prop_assert_eq!(board.to_canonical_bytes(), bytes);
        }
    }

    // (encode then parse is the identity)
    // spec: CORE-ENC-001
    // spec: CORE-ENC-008
    #[test]
    fn encode_then_parse_is_the_identity(v in value()) {
        let bytes = canon::encode(&v).expect("no null and no float");
        prop_assert_eq!(canon::parse(&bytes, bytes.len()).expect("canonical"), v);
        prop_assert!(!bytes.iter().any(|&b| b == b'\n' || b == b'\r' || b == b'\t'), "control bytes are escaped");
    }

    // (loading changes nothing)
    // spec: CORE-DATA-005
    // spec: CORE-DATA-002
    #[test]
    fn loading_changes_nothing(board in board()) {
        prop_assert_eq!(board.check(), Ok(()));
        let bytes = board.to_canonical_bytes();
        let loaded = BoardData::from_canonical_bytes(&bytes).expect("a generated board loads");
        prop_assert_eq!(&loaded, &board);
        prop_assert_eq!(loaded.to_canonical_bytes(), bytes);
    }

    // (one minter never yields the same number twice)
    // spec: CORE-ID-004
    #[test]
    fn a_minter_never_repeats(device_no in 1u16..=8191, ops in prop::collection::vec(op(), 1..60)) {
        let mut m = IdMinter::new(device_no);
        let mut view = BoardData::empty();
        let (mut items, mut blocks) = (Vec::new(), Vec::new());
        for o in ops {
            match o {
                Op::Item => {
                    let mark = view.mark(device_no).item;
                    if let Some(id) = m.mint_item(&view) {
                        let (h, n) = ids::split(id);
                        prop_assert_eq!(h, device_no);
                        prop_assert!(n > mark);
                        items.push(id);
                    }
                }
                Op::Block => {
                    let mark = view.mark(device_no).block;
                    if let Some(id) = m.mint_block(&view) {
                        let (h, n) = ids::split(id);
                        prop_assert_eq!(h, device_no);
                        prop_assert!(n > mark);
                        blocks.push(id);
                    }
                }
                Op::View(item, block) => {
                    view.marks = if (item, block) == (0, 0) { vec![] } else { vec![Mark { device_no, item, block }] };
                }
            }
        }
        for list in [&mut items, &mut blocks] {
            let n = list.len();
            list.sort_unstable();
            list.dedup();
            prop_assert_eq!(list.len(), n);
        }
    }

    // (a table zone finds every instant again from its local wall time)
    // spec: CORE-TIME-005
    // spec: CORE-TIME-002
    #[test]
    fn a_table_zone_finds_the_instant_again(t in instant(), z in table_zone()) {
        prop_assert!(found_again(t, &z));
    }

    // (the same for fixed zones and for the process zone)
    // spec: CORE-TIME-002
    #[test]
    fn fixed_and_chrono_zones_find_the_instant_again(t in instant(), o in -86_399i32..=86_399) {
        prop_assert!(found_again(t, &FixedZone(o)));
        let local = time::ChronoZone(chrono::Local);
        prop_assert!(found_again(t, &local));
        // local_day and local_minutes agree with chrono's own reading.
        let wall = t.to_chrono().with_timezone(&local.0).naive_local();
        prop_assert_eq!(time::local_day(t, &local), wall.date());
        prop_assert_eq!(time::local_minutes(t, &local), i32::try_from(wall.time().num_seconds_from_midnight() / 60).expect("i32"));
    }

    // (snap and clamp_block stay on the grid inside the day)
    // spec: CORE-TIME-004
    #[test]
    fn clamp_block_lies_in_the_day(start in any::<i64>(), minutes in any::<i64>()) {
        let (s, len) = time::clamp_block(start, minutes);
        prop_assert!(s >= 0 && s % 15 == 0 && len >= 15 && len % 15 == 0);
        prop_assert!(i64::from(s) + i64::from(len) <= 1440);
    }

    // (every instant's text is 30 bytes and parses back)
    // spec: CORE-ENC-007
    #[test]
    fn instant_text_round_trips(t in instant()) {
        let text = t.to_canonical();
        prop_assert_eq!(text.len(), 30);
        prop_assert_eq!(Instant::parse_canonical(&text), Some(t));
        prop_assert_eq!(Instant::from_chrono(t.to_chrono()), Some(t));
    }
}

use chrono::Timelike;

fn found_again(t: Instant, z: &(impl Zone + ?Sized)) -> bool {
    let wall = t.to_chrono().naive_utc() + chrono::Duration::seconds(i64::from(z.offset_at(t)));
    match z.from_local(wall) {
        LocalTime::None => false,
        LocalTime::Single(a) => a == t,
        LocalTime::Ambiguous(a, b) => a < b && (a == t || b == t || (a < t && t < b)),
    }
}

#[derive(Clone, Debug)]
enum Op {
    Item,
    Block,
    View(u64, u64),
}

fn op() -> impl Strategy<Value = Op> {
    let mark = prop_oneof![0u64..50, Just((1 << 40) - 2), Just((1 << 40) - 1)];
    prop_oneof![4 => Just(Op::Item), 4 => Just(Op::Block), 1 => (mark.clone(), mark).prop_map(|(i, b)| Op::View(i, b))]
}

fn value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        any::<bool>().prop_map(Value::Bool),
        (-MAX_INT..=MAX_INT).prop_map(Value::from),
        prop_oneof![Just(0i64), Just(MAX_INT), Just(-MAX_INT), Just(-1)].prop_map(Value::from),
        any::<String>().prop_map(Value::String),
    ];
    leaf.prop_recursive(8, 64, 6, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..6).prop_map(Value::Array),
            prop::collection::btree_map(any::<String>(), inner, 0..6).prop_map(|m| Value::Object(m.into_iter().collect())),
        ]
    })
}

fn instant() -> impl Strategy<Value = Instant> {
    prop_oneof![
        (0i64..=253_402_300_799, 0u32..1_000_000_000),
        (prop_oneof![Just(0i64), Just(253_402_300_799)], prop_oneof![Just(0u32), Just(999_999_999)]),
    ]
    .prop_map(|(s, n)| Instant::from_parts(s, n).expect("in range"))
}

fn table_zone() -> impl Strategy<Value = TableZone> {
    let offset = prop_oneof![-86_399i32..=86_399, (-26i32..=26).prop_map(|h| h * 1800)];
    (offset.clone(), prop::collection::btree_map(-1_000_000_000_000i64..300_000_000_000_000, offset, 0..8))
        .prop_map(|(initial, t)| TableZone::new(initial, t.into_iter().collect()).expect("ascending"))
}

fn text(max: usize) -> impl Strategy<Value = String> {
    prop::collection::vec(any::<char>(), 0..max).prop_map(String::from_iter)
}

fn stamp() -> impl Strategy<Value = Stamp> {
    (prop_oneof![0u64..100, Just((1 << 53) - 1)], 0u16..=8191).prop_map(|(seq, device_no)| Stamp { seq, device_no })
}

fn device() -> impl Strategy<Value = u16> {
    prop_oneof![Just(0u16), Just(1), Just(2), Just(8191)]
}

fn recurrence() -> impl Strategy<Value = Recurrence> {
    (0u8..=127, 0i64..=2_932_896, 0i32..=1439, 0u32..=1440).prop_map(|(days, d, start_minutes, minutes)| Recurrence {
        days,
        anchor: chrono::NaiveDate::from_ymd_opt(1970, 1, 1).expect("date") + chrono::Days::new(d.unsigned_abs()),
        start_minutes,
        minutes,
    })
}

#[derive(Clone, Debug)]
struct Shape {
    kind: Kind,
    name: String,
    created: Instant,
    deadline: Option<Instant>,
    importance: Option<u8>,
    time_importance: Option<u8>,
    duration_minutes: Option<u32>,
    recurrence: Recurrence,
    device: u16,
    blocks: Vec<(u16, Instant, u32, Stamp)>,
}

fn shape() -> impl Strategy<Value = Shape> {
    (
        prop_oneof![Just(Kind::Task), Just(Kind::Event), Just(Kind::Routine)],
        text(8),
        instant(),
        prop::option::of(instant()),
        (prop::option::of(0u8..=4), prop::option::of(0u8..=3), prop::option::of(0u32..=1440)),
        recurrence(),
        device(),
        prop::collection::vec((device(), instant(), 0u32..=1440, stamp()), 0..4),
    )
        .prop_map(|(kind, name, created, deadline, (importance, time_importance, duration_minutes), recurrence, device, blocks)| Shape {
            kind,
            name,
            created,
            deadline,
            importance,
            time_importance,
            duration_minutes,
            recurrence,
            device,
            blocks,
        })
}

/// A board that passes `check`: ids of a few devices numbered in turn, sids
/// likewise, every list sorted, marks at the highest numbers.
fn board() -> impl Strategy<Value = BoardData> {
    (
        prop::collection::vec((shape(), prop::array::uniform5(stamp())), 0..5),
        prop::collection::vec((shape(), instant(), any::<bool>(), any::<bool>()), 0..5),
        prop::collection::vec((device(), text(6), [any::<u8>(); 4], any::<bool>()), 0..4),
        prop::collection::vec(prop::collection::vec(any::<u8>(), 1..12), 0..4),
        (text(20).prop_map(|t| t.replace('\t', " ")), prop_oneof![Just(0u64), 1u64..(1 << 53)]),
        (prop::array::uniform6(prop_oneof![Just([0u8; 4]), any::<[u8; 4]>()]), -9000i32..=9000, -18000i32..=18000),
        prop::option::of((any::<[u8; 16]>(), 1u32..=u32::MAX, 67u64..=67_108_864, any::<[u8; 32]>())),
    )
        .prop_map(|(items, rows, subs, unreadable, (notes, notes_rev), (palette, lat, lon), picture)| {
            let mut n = 0u64;
            let mut sid = 0u64;
            let next = |counter: &mut u64, h: u16| {
                *counter += 1;
                ids::compose(h, *counter).expect("id")
            };
            let mut board = BoardData::empty();
            for (s, st) in items {
                let blocks = s
                    .blocks
                    .iter()
                    .map(|&(h, start, minutes, stamp)| Block { sid: next(&mut sid, h), start, minutes, stamp })
                    .collect::<Vec<_>>();
                let mut item = Item {
                    id: next(&mut n, s.device),
                    kind: s.kind,
                    name: s.name,
                    created: s.created,
                    deadline: s.deadline,
                    importance: s.importance,
                    time_importance: s.time_importance,
                    duration_minutes: s.duration_minutes,
                    recurrence: (s.kind == Kind::Routine).then_some(s.recurrence),
                    blocks,
                    stamps: ItemStamps { estimate: st[0], levels: st[1], name: st[2], repeat: st[3], time: st[4] },
                };
                item.blocks.sort_by_key(|b| b.sid);
                board.items.push(item);
            }
            board.items.sort_by_key(|i| i.id);
            for (s, archived_at, pre_id, dropped) in rows {
                let mut blocks: Vec<RowBlock> = s
                    .blocks
                    .iter()
                    .map(|&(h, start, minutes, _)| RowBlock { sid: next(&mut sid, h), start, minutes })
                    .collect();
                blocks.sort_by_key(|b| b.sid);
                board.archive.push(ArchiveRow {
                    id: if pre_id { 0 } else { next(&mut n, s.device) },
                    kind: s.kind,
                    name: s.name,
                    created: s.created,
                    deadline: s.deadline,
                    importance: s.importance,
                    time_importance: s.time_importance,
                    duration_minutes: s.duration_minutes,
                    recurrence: (s.kind == Kind::Routine).then_some(s.recurrence),
                    blocks,
                    archived_at,
                    outcome: if dropped { Outcome::Dropped } else { Outcome::Finished },
                });
            }
            board.archive.sort_by_key(|a| (a.archived_at, a.id, a.to_canonical()));
            for (k, (h, name, color, enabled)) in subs.into_iter().enumerate() {
                let id = next(&mut n, h);
                board.subscriptions.push(Subscription { id, name, url: format!("https://example.com/{k}.ics"), color, enabled });
            }
            board.subscriptions.sort_by_key(|s| s.id);
            board.unreadable = unreadable;
            board.unreadable.sort();
            board.notes = notes;
            board.notes_rev = notes_rev;
            board.palette = palette;
            board.home = Home { lat, lon };
            board.picture = picture.map(|(blob, epoch, size, sha256)| PictureRef {
                blob,
                epoch,
                parts: u32::try_from(size.div_ceil(1_048_576)).expect("parts"),
                sha256,
                size,
            });
            // Marks: the highest numbers per device.
            let mut marks: Vec<Mark> = Vec::new();
            let mut raise = |id: u64, block: bool| {
                let (h, low) = ids::split(id);
                let at = match marks.iter().position(|m| m.device_no == h) {
                    Some(at) => at,
                    None => {
                        marks.push(Mark { device_no: h, item: 0, block: 0 });
                        marks.len() - 1
                    }
                };
                let m = &mut marks[at];
                if block { m.block = m.block.max(low) } else { m.item = m.item.max(low) }
            };
            for i in &board.items {
                raise(i.id, false);
                i.blocks.iter().for_each(|b| raise(b.sid, true));
            }
            for r in &board.archive {
                if r.id != 0 {
                    raise(r.id, false);
                }
                r.blocks.iter().for_each(|b| raise(b.sid, true));
            }
            board.subscriptions.iter().for_each(|s| raise(s.id, false));
            marks.sort_by_key(|m| m.device_no);
            board.marks = marks;
            board
        })
}
