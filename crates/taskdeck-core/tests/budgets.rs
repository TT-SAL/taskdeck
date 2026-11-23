//! The budget workloads of core.md §17, each run once here for its result,
//! not its time: the crate cannot read a clock, so the times are taken by
//! `ci.sh`, which runs the examples `budget_snapshot`, `budget_apply` and
//! `budget_checkpoint` on the same workloads (`tests/common/budget.rs`) and
//! prints the medians on lines starting `budget:`.

mod common;

use common::budget;
use common::snapshots::{self, Removed, first_difference, request, transform};
use serde_json::Value;
use taskdeck_core::board::BoardData;
use taskdeck_core::command::LogCommand;
use taskdeck_core::time::Zone;

// The snapshot of the budget is the request of the fixture's recorded 31-day
// snapshot, and what it builds equals the recording after §18.5's
// transformation (the fixture has no record §18.7 removes).
// spec: CORE-PERF-001
#[test]
fn the_snapshot_budget_builds_the_recorded_snapshot() {
    let board = budget::fixture_board();
    let wanted = budget::snapshot_request();
    let (_, _, _, text) = snapshots::FILES
        .iter()
        .find(|f| f.0 == budget::SNAPSHOT_ZONE_DIR && f.1 == "archive-10k" && f.2 == budget::SNAPSHOT_FILE)
        .expect("the recorded 31-day snapshot of archive-10k");
    assert_eq!(request(budget::SNAPSHOT_FILE), (wanted.from, wanted.days, wanted.now));
    assert_eq!(board.archive.len(), 10_000, "the 10,000-row fixture");
    let built = serde_json::to_value(budget::snapshot_workload(&board, &wanted)).expect("JSON");
    let recorded: Value = serde_json::from_str(text).expect("recorded JSON");
    let expected = transform(recorded, &board, &Removed::default()).expect("compared");
    assert_eq!(first_difference("", &built, &expected), None);
    assert_eq!(built["days"].as_array().map(Vec::len), Some(31));
    // The zone is the one the fixture was recorded in.
    assert_eq!(wanted.zone.offset_at(wanted.now), 7200);
}

// The 5,000 commands of the budget workload are as §17 defines them, all
// apply, and the archive grows by the 1,000 completed tasks; nothing else of
// the fixture moves but device 2's marks.
// spec: CORE-PERF-002
#[test]
fn the_apply_budget_applies_every_command() {
    let fixture = budget::fixture_board();
    let commands = budget::apply_commands(&fixture);
    assert_eq!(commands.len(), 5000);
    let ops: Vec<&str> = commands.iter().take(5).map(|(c, _)| c.op()).collect();
    assert_eq!(ops, ["create_task", "rename", "move_block", "set_deadline", "complete"]);
    for (k, (command, ctx)) in (2u64..).zip(&commands) {
        assert_eq!((ctx.seq, ctx.based_on, ctx.device_no), (k, k - 1, budget::AUTHOR), "{}", command.op());
    }
    let mut board = fixture.clone();
    assert_eq!(budget::apply_workload(&mut board, &commands), 5000);
    assert_eq!(board.archive.len(), fixture.archive.len() + 1000);
    assert_eq!(board.items, fixture.items);
    assert!(board.check().is_ok());
    // The completed rows carry the moved block, the new name and the deadline.
    let (_, first) = &commands[0];
    let row = board.archive.iter().find(|r| r.id == commands[0].0.target().expect("an id")).expect("archived");
    assert_eq!(row.name, "Budget task 0, renamed");
    assert_eq!(row.blocks.len(), 1);
    assert_eq!(row.blocks[0].minutes, 30);
    let LogCommand::CreateTask(created) = &commands[0].0 else { panic!("a create_task first") };
    assert_eq!(row.blocks[0].start, created.blocks[0].start.plus_seconds(900).expect("in range"));
    assert!(row.deadline.is_some());
    assert_eq!(row.archived_at, first.at);
}

// The checkpoint of the budget: the fixture's canonical bytes load, checked,
// and give the same bytes again (CORE-DATA-005).
// spec: CORE-PERF-003
#[test]
fn the_checkpoint_budget_loads_the_fixtures_bytes() {
    let fixture = budget::fixture_board();
    let bytes = fixture.to_canonical_bytes();
    let loaded: BoardData = budget::checkpoint_workload(&bytes);
    assert_eq!(loaded.to_canonical_bytes(), bytes);
    assert_eq!(loaded.archive.len(), 10_000);
}
