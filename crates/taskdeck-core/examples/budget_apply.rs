//! CORE-PERF-002 (core.md §17): reads `n` from standard input, imports the
//! 10,000-row fixture and builds the 5,000 commands of the budget workload
//! once, then `n` times applies them all to a copy of the fixture. `ci.sh`
//! times it with `n` and with 0 and takes the difference, so each run's time
//! includes the copy of the board (a clone) as well as the 5,000 `apply`s.

// The fixture folders, the zones and the budget workloads, shared with the
// tests (tests/common/budget.rs says how).
#[allow(dead_code)]
mod boards {
    include!("../tests/common/boards.rs");
}
#[allow(dead_code)]
mod zones {
    include!("../tests/common/zones.rs");
}
#[allow(dead_code)]
mod budget {
    include!("../tests/common/budget.rs");
}

use std::hint::black_box;

fn main() {
    let n = budget::read_n();
    let board = budget::fixture_board();
    let commands = budget::apply_commands(&board);
    for _ in 0..n {
        let mut copy = black_box(&board).clone();
        let accepted = budget::apply_workload(&mut copy, black_box(&commands));
        assert_eq!(accepted, commands.len(), "every command of the workload applies");
        black_box(copy);
    }
}
