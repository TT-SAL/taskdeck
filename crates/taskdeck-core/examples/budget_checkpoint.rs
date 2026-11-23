//! CORE-PERF-003 (core.md §17): reads `n` from standard input, imports the
//! 10,000-row fixture and encodes its canonical bytes once, then `n` times
//! loads a board from those bytes (`BoardData::from_canonical_bytes`: parsed
//! and checked). `ci.sh` times it with `n` and with 0 and takes the
//! difference.

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
    let bytes = budget::fixture_board().to_canonical_bytes();
    for _ in 0..n {
        black_box(budget::checkpoint_workload(black_box(&bytes)));
    }
}
