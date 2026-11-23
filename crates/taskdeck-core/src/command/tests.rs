//! Unit tests of `command` (core.md §9). The vectors and property tests are in
//! `tests/commands.rs`.

use super::*;

const OPS: [&str; 29] = [
    "create_task",
    "create_event",
    "create_routine",
    "rename",
    "complete",
    "delete",
    "forget",
    "move_block",
    "move_blocks",
    "add_block",
    "remove_blocks",
    "set_estimate",
    "set_deadline",
    "set_severity",
    "set_horizon",
    "set_repeat",
    "move_routine",
    "set_event_span",
    "set_notes",
    "restore",
    "forget_archived",
    "add_subscription",
    "remove_subscription",
    "rename_subscription",
    "set_subscription_color",
    "set_subscription_enabled",
    "set_palette",
    "set_home",
    "set_picture",
];

// spec: CORE-CMD-002
#[test]
fn exactly_the_29_ops_of_9_3_have_a_reader() {
    for op in OPS {
        assert!(reader_for(op).is_some(), "{op}");
    }
    for op in ["", "Rename", "rename ", "create", "set_note", "move_block\u{0}", "import", "noop"] {
        assert!(reader_for(op).is_none(), "{op:?}");
    }
}

#[test]
fn the_limit_is_the_seal_cap() {
    assert_eq!(COMMAND_LIMIT, 257_979);
}

// spec: CORE-CMD-001
#[test]
fn the_schema_rules_are_checked_apart_from_step_5() {
    let at = |start_minutes, minutes| LogCommand::MoveRoutine { id: 1, start_minutes, minutes };
    assert!(at(1425, 15).check_schema_rules().is_ok());
    assert!(at(0, 1440).check_schema_rules().is_ok());
    assert_eq!(at(1425, 16).check_schema_rules(), Err(schema()));
    let pic = |size, parts| LogCommand::SetPicture {
        picture: Some(PictureRef { blob: [0; 16], epoch: 1, parts, sha256: [0; 32], size }),
    };
    assert!(pic(1_048_576, 1).check_schema_rules().is_ok());
    assert!(pic(1_048_577, 2).check_schema_rules().is_ok());
    assert_eq!(pic(1_048_577, 1).check_schema_rules(), Err(schema()));
    assert!(LogCommand::SetPicture { picture: None }.check_schema_rules().is_ok());
}
