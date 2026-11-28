//! Text through `parse_intent` and `compile` against the fixture board
//! (core.md §12, §13, §18.6): neither panics (CORE-GEN-007), and a compiled
//! command parses from its canonical bytes and applies on its view
//! (CORE-COMP-008). Two control bytes pick the zone's offset and `now`.
#![no_main]

use libfuzzer_sys::fuzz_target;
use taskdeck_core::apply::{Ctx, apply};
use taskdeck_core::command::parse_command;
use taskdeck_core::compile::{CompileCtx, Compiled, compile};
use taskdeck_core::ids::IdMinter;
use taskdeck_core::intent::parse_intent;
use taskdeck_core::overlay::Overlay;
use taskdeck_core::time::FixedZone;
use taskdeck_core_fuzz::{Control, fixture_board, now_plus};

fuzz_target!(|data: &[u8]| {
    let mut control = Control(data);
    let zone = FixedZone((i32::from(control.byte()) - 128) * 900);
    let now = now_plus(i64::from(control.byte()) * 30);
    let Ok(text) = std::str::from_utf8(control.rest()) else { return };
    let Ok(intent) = parse_intent(text) else { return };
    let view = fixture_board();
    let overlay = Overlay::default();
    let cx = CompileCtx { view: &view, zone: &zone, now, device_no: 1, overlay: &overlay };
    let mut minter = IdMinter::new(1);
    if let Ok(Compiled { command: Some(command), .. }) = compile(&intent, &cx, &mut minter) {
        let parsed = parse_command(&command.to_canonical()).expect("a compiled command parses");
        assert_eq!(parsed, command);
        let mut board = view.clone();
        let ctx = Ctx { seq: 2, device_no: 1, at: now, based_on: 1 };
        assert!(apply(&mut board, &command, &ctx).is_ok(), "a compiled command is refused on its view");
    }
});
