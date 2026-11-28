//! Unit tests of `CoreMachine`: each contract of engine.md §5.1 that the
//! engine relies on, against what `taskdeck-core` does.

use taskdeck_core::apply::{Group, Notice, RefusalRecord};
use taskdeck_core::board::BoardData;
use taskdeck_core::command::LogCommand;
use taskdeck_core::compile::{CompileCtx, compile};
use taskdeck_core::ids::{self, IdMinter};
use taskdeck_core::intent::Intent;
use taskdeck_core::overlay::Overlay;
use taskdeck_core::time::{FixedZone, Instant};
use taskdeck_sync::{Coalesce, Ctx, StateMachine};

use super::{CoreMachine, ORIGIN_FORMAT};

const NOW: i64 = 1_700_000_000_000;

fn add(view: &BoardData, minter: &mut IdMinter, name: &str) -> LogCommand {
    let intent =
        Intent::Add { name: name.into(), deadline: None, importance: None, is_event: false, time_importance: None };
    let overlay = Overlay::default();
    let cx = CompileCtx {
        view,
        zone: &FixedZone(0),
        now: Instant::from_at_ms(NOW).expect("in range"),
        device_no: minter.device_no,
        overlay: &overlay,
    };
    compile(&intent, &cx, minter).expect("compiles").command.expect("a command")
}

fn ctx(seq: u64, device_no: u16) -> Ctx {
    Ctx { seq, device_no, at_ms: NOW, based_on: seq - 1 }
}

/// A board with one task created by device 1 at seq 2.
fn board_with_task() -> (BoardData, u64) {
    let sm = CoreMachine::new();
    let mut b = BoardData::empty();
    let mut minter = IdMinter::new(1);
    let c = add(&b, &mut minter, "Write");
    let id = c.target().expect("a create has a target");
    sm.apply(&mut b, &c, &ctx(2, 1)).expect("applies");
    (b, id)
}

#[test]
fn versions_are_cores() {
    let sm = CoreMachine::new();
    assert_eq!(sm.supported_rules(), &[1]);
    assert_eq!(sm.hash_versions(), &[1]);
}

/// engine.md §5.1 `load`, `canonical_bytes`: load of the canonical bytes is the identity.
#[test]
fn load_of_canonical_bytes_is_the_identity() {
    let sm = CoreMachine::new();
    for b in [BoardData::empty(), board_with_task().0] {
        let bytes = sm.canonical_bytes(&b);
        let again = sm.load(&bytes).expect("loads");
        assert_eq!(again, b);
        assert_eq!(sm.canonical_bytes(&again), bytes);
    }
}

/// engine.md §5.1 `load`: no normalisation (V2-19); other bytes are an error.
#[test]
fn load_refuses_bytes_that_are_not_canonical() {
    let sm = CoreMachine::new();
    let bytes = sm.canonical_bytes(&BoardData::empty());
    let mut spaced = b" ".to_vec();
    spaced.extend_from_slice(&bytes);
    assert!(sm.load(&spaced).is_err());
    assert!(sm.load(b"").is_err());
    assert!(sm.load(b"{\"rules\":1,\"rules\":1}").is_err());
}

/// engine.md §5.1 `board_hash`: SHA-256 of the canonical bytes.
#[test]
fn board_hash_is_sha256_of_the_canonical_bytes() {
    use sha2::Digest;
    let sm = CoreMachine::new();
    let (b, _) = board_with_task();
    let want: [u8; 32] = sha2::Sha256::digest(sm.canonical_bytes(&b)).into();
    assert_eq!(sm.board_hash(&b), want);
    assert_eq!(sm.rules(&b), 1);
}

/// engine.md §5.1 `encode`, `parse`: `parse(encode(c)) = c`.
#[test]
fn parse_of_encode_is_the_command() {
    let sm = CoreMachine::new();
    let (b, id) = board_with_task();
    let mut minter = IdMinter::new(1);
    for c in [add(&b, &mut minter, "Read"), LogCommand::Rename { id, name: "Write more".into() }, LogCommand::Forget { id }] {
        let p = sm.encode(&c);
        assert_eq!(sm.parse(&p).expect("parses"), c);
        assert_eq!(sm.op_name(&c), c.op());
        assert_eq!(sm.target(&c), c.target());
    }
    assert!(sm.parse(b"{\"op\":\"nope\"}").is_err());
}

/// engine.md §5.1 `apply`: on `Err` the state is unchanged; `refusal_bytes`
/// is `RefusalRecord::to_canonical` with the refusal's fields (core.md §10.7).
#[test]
fn a_refused_command_leaves_the_state_and_gives_its_refusal_record() {
    let sm = CoreMachine::new();
    let (mut b, id) = board_with_task();
    let before = sm.canonical_bytes(&b);
    let gone = LogCommand::Rename { id: id + 1, name: "x".into() };
    let r = sm.apply(&mut b, &gone, &ctx(3, 1)).expect_err("refused");
    assert_eq!(r.code, "core/gone");
    assert_eq!(r.op, Some("rename"));
    assert_eq!(r.target, Some(id + 1));
    assert_eq!(sm.canonical_bytes(&b), before);
    let bytes = sm.refusal_bytes(3, 1, &r);
    let rec = RefusalRecord::from_canonical(&bytes).expect("a record");
    assert_eq!(
        rec,
        RefusalRecord {
            seq: 3,
            author: 1,
            code: "core/gone".into(),
            op: Some("rename".into()),
            target: Some(id + 1),
            base: None,
            current: None
        }
    );
}

/// engine.md §5.1 `SmRefusal.base`, `current`: a stale `set_notes` carries both.
#[test]
fn a_stale_set_notes_carries_base_and_current() {
    let sm = CoreMachine::new();
    let mut b = BoardData::empty();
    sm.apply(&mut b, &LogCommand::SetNotes { text: "a".into(), base: 0 }, &ctx(2, 1)).expect("applies");
    let r = sm.apply(&mut b, &LogCommand::SetNotes { text: "b".into(), base: 0 }, &ctx(3, 2)).expect_err("stale");
    assert_eq!((r.code, r.base, r.current), ("core/notes_stale", Some(0), Some(1)));
    let rec = RefusalRecord::from_canonical(&sm.refusal_bytes(3, 2, &r)).expect("a record");
    assert_eq!((rec.base, rec.current), (Some(0), Some(1)));
}

/// engine.md §36: `Ctx.at` is `Instant::from_at_ms(at_ms)`; out of its range
/// the adapter refuses with `core/bad_ctx` and changes nothing.
#[test]
fn an_at_ms_outside_the_instant_range_is_bad_ctx() {
    let sm = CoreMachine::new();
    let (mut b, id) = board_with_task();
    let before = b.clone();
    let c = LogCommand::Rename { id, name: "y".into() };
    let bad = Ctx { seq: 3, device_no: 1, at_ms: i64::MIN, based_on: 2 };
    assert_eq!(sm.apply(&mut b, &c, &bad).expect_err("refused").code, "core/bad_ctx");
    assert_eq!(b, before);
}

/// engine.md §5.1 `trial_apply`: `apply` on a copy.
#[test]
fn trial_apply_never_changes_the_state() {
    let sm = CoreMachine::new();
    let (b, id) = board_with_task();
    let c = LogCommand::Rename { id, name: "z".into() };
    let mut applied = b.clone();
    let real = sm.apply(&mut applied, &c, &ctx(3, 1)).expect("applies");
    let trial = sm.trial_apply(&b, &c, &ctx(3, 1)).expect("applies");
    assert_eq!(real, trial);
    assert_ne!(applied, b);
    assert_eq!(b, board_with_task().0);
}

/// engine.md §5.1 `upgrade` (core.md §11.2): a version not newer or not
/// supported is refused, and the state stays.
#[test]
fn upgrade_refuses_what_is_not_newer_and_supported() {
    let sm = CoreMachine::new();
    let mut b = BoardData::empty();
    for v in [0, 1, 2] {
        assert_eq!(sm.upgrade(&mut b, v).expect_err("refused").code, "core/unknown_rules");
        assert_eq!(b, BoardData::empty());
    }
}

/// engine.md §5.1 `reset` (core.md §11.3): target's data, the marks'
/// elementwise maximum, `notes_rev` one above both.
#[test]
fn reset_keeps_both_marks_and_raises_the_notes_revision() {
    let sm = CoreMachine::new();
    let target = BoardData::empty();
    let (mut current, _) = board_with_task();
    sm.apply(&mut current, &LogCommand::SetNotes { text: "n".into(), base: 0 }, &ctx(3, 1)).expect("applies");
    let r = sm.reset(&target, &current).expect("resets");
    assert!(r.items.is_empty());
    assert_eq!(r.marks, current.marks);
    assert_eq!(r.notes_rev, current.notes_rev + 1);
    assert_eq!(r.rules, current.rules);
}

/// Finding f1-11: `reset` also fails, with `core/cap`, at the last notes
/// revision, which engine.md §5.1 does not list among its errors.
#[test]
fn reset_fails_at_the_last_notes_revision() {
    let sm = CoreMachine::new();
    let mut current = BoardData::empty();
    current.notes_rev = (1 << 53) - 1;
    assert_eq!(sm.reset(&BoardData::empty(), &current).expect_err("refused").code, "core/cap");
}

/// engine.md §5.1 `hold`, `coalesce` (V2-17).
#[test]
fn a_held_create_merges_a_rename_and_cancels_with_a_forget() {
    let sm = CoreMachine::new();
    let b = BoardData::empty();
    let mut minter = IdMinter::new(1);
    let create = add(&b, &mut minter, "Draft");
    let id = sm.hold(&create).expect("a create may be held");
    match sm.coalesce(&create, &LogCommand::Rename { id, name: "Final".into() }) {
        Coalesce::Merge(c) => assert_eq!(c, create.clone().with_name("Final".into()).expect("a create")),
        _ => panic!("not merged"),
    }
    assert!(matches!(sm.coalesce(&create, &LogCommand::Forget { id }), Coalesce::CancelBoth));
    assert!(matches!(sm.coalesce(&create, &LogCommand::Forget { id: id + 1 }), Coalesce::Keep));
    assert!(matches!(sm.coalesce(&create, &LogCommand::Complete { id, at: Instant::MIN }), Coalesce::Keep));
    let rename = LogCommand::Rename { id, name: "x".into() };
    assert_eq!(sm.hold(&rename), None);
    assert!(matches!(sm.coalesce(&rename, &LogCommand::Forget { id }), Coalesce::Keep));
}

/// engine.md §5.1 `rebind` (core.md §7.5): the old identity's mints are
/// minted anew from the new identity's origin state (device.md §8.3), and
/// the returned origin state is the minter after them.
#[test]
fn rebind_mints_from_the_origin_state_of_the_new_identity() {
    let sm = CoreMachine::new();
    let b = BoardData::empty();
    let mut old = IdMinter::new(3);
    let create = add(&b, &mut old, "Mine");
    let old_id = create.target().expect("a target");
    let rename = LogCommand::Rename { id: old_id, name: "Ours".into() };
    let origin = CoreMachine::origin_state(&IdMinter::new(5));
    assert_eq!(origin.len(), 18);
    let (out, after) = sm.rebind(&[create, rename], 3, 5, &b, &origin).expect("rebinds");
    let new_id = out[0].target().expect("a target");
    assert_eq!(ids::split(new_id).0, 5);
    assert_eq!(out[1], LogCommand::Rename { id: new_id, name: "Ours".into() });
    let m = CoreMachine::minter(&after).expect("18 bytes");
    assert_eq!(m.device_no, 5);
    assert!(m.next_item > 1);
}

/// Decision f1-6: an origin state of another length, or of another
/// `device_no` than `to_device_no`, is `device/record_format`.
#[test]
fn rebind_refuses_an_origin_state_that_is_not_the_new_identitys() {
    let sm = CoreMachine::new();
    let b = BoardData::empty();
    let origin = CoreMachine::origin_state(&IdMinter::new(4));
    assert_eq!(sm.rebind(&[], 3, 5, &b, &origin).expect_err("refused").code, ORIGIN_FORMAT);
    assert_eq!(sm.rebind(&[], 3, 5, &b, &origin[..17]).expect_err("refused").code, ORIGIN_FORMAT);
}

/// engine.md §5.1 `notice_concerns`, `notice_bytes` (core.md §10.7).
#[test]
fn a_notice_concerns_its_author_and_the_other_device() {
    let sm = CoreMachine::new();
    let n = Notice::Overwrite { seq: 9, author: 2, other: 1, other_seq: 4, item: 7, sid: None, group: Group::Name };
    assert!(sm.notice_concerns(&n, 1) && sm.notice_concerns(&n, 2) && !sm.notice_concerns(&n, 3));
    assert_eq!(Notice::from_canonical(&sm.notice_bytes(&n)).expect("a record"), n);
}
