// The three budget workloads of core.md §17 (CORE-PERF-001 to -003), shared
// by `tests/budgets.rs` (as `common::budget`) and the examples
// `budget_snapshot`, `budget_apply` and `budget_checkpoint`. The examples
// `include!` this file next to `boards.rs` and `zones.rs`, each in a module of
// that name, so `super::boards` and `super::zones` name those files in both.
// (No inner attribute or `//!` here: `include!` does not take them.)
//
// None of this reads a clock: the examples are timed by the shell (ci.sh).

use taskdeck_core::apply::{Ctx, apply};
use taskdeck_core::board::BoardData;
use taskdeck_core::command::{CreateTask, LogCommand, NewBlock};
use taskdeck_core::ids::IdMinter;
use taskdeck_core::legacy::import;
use taskdeck_core::overlay::Overlay;
use taskdeck_core::snapshot::{Dials, Snapshot, SnapshotInput, snapshot};
use taskdeck_core::time::{Instant, TableZone};

/// The recorded 31-day snapshot of the 10,000-row fixture (core.md §17,
/// §18.5): its zone directory and file under `fixtures/snapshots/`.
pub const SNAPSHOT_ZONE_DIR: &str = "europe-berlin";
pub const SNAPSHOT_FILE: &str = "2026-10-05_31d_at-20261020T0815Z.json";

/// Rounds of the apply workload, five commands each (core.md §17).
pub const ROUNDS: u64 = 1000;
/// The author of the apply workload (core.md §17).
pub const AUTHOR: u16 = 2;

fn instant(text: &str) -> Instant {
    Instant::parse_canonical(text).unwrap_or_else(|| panic!("{text} is an instant"))
}

/// The 10,000-row fixture: the board `import` makes of
/// `fixtures/boards/archive-10k` (core.md §17).
pub fn fixture_board() -> BoardData {
    let (board, report) = import(&super::boards::board("archive-10k"));
    board.unwrap_or_else(|| panic!("archive-10k imports: {:?}", report.failure))
}

/// The request of the recorded snapshot `SNAPSHOT_FILE`: what its name holds
/// (from 2026-10-05, 31 days, now 2026-10-20T08:15Z) and what §18.5 builds
/// the input of (the zone of its directory, an empty overlay since the folder
/// has no `subscribed_cache.json`, no look, dials 11 and 39, version 0,
/// `ranked` none), as `tests/snapshot_fixtures.rs` builds it.
pub struct SnapshotRequest {
    pub zone: TableZone,
    pub overlay: Overlay,
    pub from: chrono::NaiveDate,
    pub days: u32,
    pub now: Instant,
}

pub fn snapshot_request() -> SnapshotRequest {
    SnapshotRequest {
        zone: super::zones::of_directory(SNAPSHOT_ZONE_DIR),
        overlay: Overlay::default(),
        from: chrono::NaiveDate::from_ymd_opt(2026, 10, 5).expect("a date"),
        days: 31,
        now: instant("2026-10-20T08:15:00.000000000Z"),
    }
}

/// The input of the snapshot for `board`.
pub fn snapshot_input<'a>(board: &'a BoardData, request: &'a SnapshotRequest) -> SnapshotInput<'a, TableZone> {
    SnapshotInput {
        board,
        overlay: &request.overlay,
        zone: &request.zone,
        now: request.now,
        from: request.from,
        days: request.days,
        version: 0,
        ranked: None,
        look: None,
        dials: Dials { blur: 11, light: 39 },
    }
}

/// The workload of CORE-PERF-001: `snapshot` of 31 days.
pub fn snapshot_workload(board: &BoardData, request: &SnapshotRequest) -> Snapshot {
    snapshot(&snapshot_input(board, request))
}

/// The budget workload of CORE-PERF-002, built beforehand: 1,000 rounds of
/// five commands by `device_no` 2, each with `based_on` one below its `seq`
/// (seqs 2 to 5,001): a `create_task` with one block of 30 minutes, a
/// `rename` of that task, a `move_block` of its block by 15 minutes, a
/// `set_deadline` and its `complete`. Ids come from device 2's minter against
/// the fixture, so each is above device 2's marks. `ctx.at` and the
/// `complete`'s `at` are the snapshot's now; the blocks start on 2026-10-21
/// and the following days.
pub fn apply_commands(board: &BoardData) -> Vec<(LogCommand, Ctx)> {
    let at = instant("2026-10-20T08:15:00.000000000Z");
    let first = instant("2026-10-21T07:00:00.000000000Z");
    let mut minter = IdMinter::new(AUTHOR);
    let mut out = Vec::new();
    for round in 0..ROUNDS {
        let id = minter.mint_item(board).expect("an item id");
        let sid = minter.mint_block(board).expect("a sid");
        let quarter = i64::try_from(round).expect("small") * 900;
        let start = first.plus_seconds(quarter).expect("in range");
        let moved = start.plus_seconds(900).expect("in range");
        let deadline = start.plus_seconds(86_400).expect("in range");
        let seq = 2 + round * 5;
        let commands = [
            LogCommand::CreateTask(CreateTask {
                id,
                name: format!("Budget task {round}"),
                deadline: None,
                importance: None,
                time_importance: None,
                duration_minutes: None,
                blocks: vec![NewBlock { sid, start, minutes: 30 }],
            }),
            LogCommand::Rename { id, name: format!("Budget task {round}, renamed") },
            LogCommand::MoveBlock { id, sid, start: moved, minutes: 30 },
            LogCommand::SetDeadline { id, deadline: Some(deadline), seed: None },
            LogCommand::Complete { id, at },
        ];
        for (k, command) in (0u64..).zip(commands) {
            let ctx = Ctx { seq: seq + k, device_no: AUTHOR, at, based_on: seq + k - 1 };
            out.push((command, ctx));
        }
    }
    out
}

/// The workload of CORE-PERF-002: every command applied in order; returns how
/// many were accepted.
pub fn apply_workload(board: &mut BoardData, commands: &[(LogCommand, Ctx)]) -> usize {
    commands.iter().filter(|(command, ctx)| apply(board, command, ctx).is_ok()).count()
}

/// The workload of CORE-PERF-003: the fixture's canonical bytes parsed and
/// checked.
pub fn checkpoint_workload(bytes: &[u8]) -> BoardData {
    BoardData::from_canonical_bytes(bytes).expect("the fixture's canonical bytes load")
}

/// The integer `n` an example reads from standard input.
pub fn read_n() -> u64 {
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).expect("standard input");
    line.trim().parse().unwrap_or_else(|_| panic!("an integer on standard input, not {line:?}"))
}
