//! Reproductions from the verify pass of d-compile.

use taskdeck_core::apply::{Ctx, apply};
use taskdeck_core::board::{ArchiveRow, BoardData, Item, ItemStamps, Kind, Mark, Outcome, Stamp};
use taskdeck_core::canon::Record;
use taskdeck_core::command::parse_command;
use taskdeck_core::compile::{CompileCtx, compile};
use taskdeck_core::ids::IdMinter;
use taskdeck_core::intent::parse_intent;
use taskdeck_core::overlay::Overlay;
use taskdeck_core::time::{FixedZone, Instant};

const S: Stamp = Stamp { seq: 1, device_no: 0 };

fn at(m: i64) -> Instant {
    Instant::from_parts(1_773_100_800 + m * 60, 0).expect("i")
}

/// A board that passes `check()` just under the size cap.
fn board_at_cap() -> BoardData {
    let mut b = BoardData::empty();
    b.items = vec![Item {
        id: 1,
        kind: Kind::Task,
        name: "t".into(),
        created: at(0),
        deadline: None,
        importance: None,
        time_importance: None,
        duration_minutes: None,
        recurrence: None,
        blocks: vec![],
        stamps: ItemStamps { estimate: S, levels: S, name: S, repeat: S, time: S },
    }];
    b.marks = vec![Mark { device_no: 0, item: 1, block: 0 }];
    let name = "\u{1}".repeat(200);
    let mk = |k: i64| ArchiveRow {
        id: 0,
        kind: Kind::Task,
        name: name.clone(),
        created: at(0),
        deadline: None,
        importance: None,
        time_importance: None,
        duration_minutes: None,
        recurrence: None,
        blocks: vec![],
        archived_at: Instant::from_parts(1_000_000 + k, 0).expect("i"),
        outcome: Outcome::Finished,
    };
    b.archive = vec![mk(0)];
    let one = b.to_canonical().len();
    b.archive = vec![mk(0), mk(1)];
    let per = b.to_canonical().len() - one;
    let n = 2 + (61_865_984 - b.to_canonical().len() - 5_000) / per;
    b.archive = (0..n as i64).map(mk).collect();
    let len = b.to_canonical().len();
    b.notes = "a".repeat(61_865_984 - len - 100);
    assert!(b.to_canonical().len() <= 61_865_984);
    assert!(b.check().is_ok());
    b
}

// spec: CORE-COMP-008
#[test]
fn compiled_add_at_the_size_cap_is_not_refused_by_apply() {
    let b = board_at_cap();
    let overlay = Overlay::default();
    let cx = CompileCtx { view: &b, zone: &FixedZone(0), now: at(720), device_no: 1, overlay: &overlay };
    let mut m = IdMinter::new(1);
    let before = m;
    let intent = parse_intent(r#"{"op":"add","name":"Write something long enough","importance":1}"#).unwrap();
    match compile(&intent, &cx, &mut m) {
        Ok(out) => {
            let c = out.command.expect("cmd");
            assert_eq!(parse_command(&c.to_canonical()).as_ref(), Ok(&c));
            let r = apply(&mut b.clone(), &c, &Ctx { seq: 1000, device_no: 1, at: at(720), based_on: 999 })
                .map(|_| ())
                .map_err(|e| e.code);
            assert_eq!(r, Ok(()), "CORE-COMP-008");
        }
        Err(e) => {
            // Compile refuses it on `view` (G6), minting nothing (CORE-COMP-004).
            assert_eq!(e.code, "core/cap");
            assert_eq!(m, before);
        }
    }
}

// spec: CORE-COMP-008
#[test]
fn compile_still_accepts_a_shrinking_command_at_the_size_cap() {
    let b = board_at_cap();
    let overlay = Overlay::default();
    let cx = CompileCtx { view: &b, zone: &FixedZone(0), now: at(720), device_no: 1, overlay: &overlay };
    let mut m = IdMinter::new(1);
    let intent = parse_intent(r#"{"op":"delete","id":1}"#).unwrap();
    let c = compile(&intent, &cx, &mut m).expect("compiles").command.expect("cmd");
    let r = apply(&mut b.clone(), &c, &Ctx { seq: 1000, device_no: 1, at: at(720), based_on: 999 })
        .map(|_| ())
        .map_err(|e| e.code);
    assert_eq!(r, Ok(()));
}
