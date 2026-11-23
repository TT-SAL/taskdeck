//! CORE-PERF-001 (core.md §17): reads `n` from standard input, imports the
//! 10,000-row fixture once, then builds its recorded 31-day snapshot `n`
//! times. `ci.sh` times it with `n` and with 0 and takes the difference.

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
    let request = budget::snapshot_request();
    for _ in 0..n {
        black_box(budget::snapshot_workload(black_box(&board), &request));
    }
}
