//! The simulator (engine.md §31), compiled only with the feature `sim`.
//!
//! `SplitMix64` is working code: the fake sealer draws every random byte from
//! it (engine.md §6.3, ENG-FAKE-002). The rest is the skeleton stage
//! `engine` fills: `run` has no body yet, and `FaultSet`, `OpSet` and
//! `SimStats` have no fields yet, because §31.5, §31.6 and §37 name what they
//! hold but not their field names.

mod splitmix;
pub use splitmix::SplitMix64;

/// The model relay (engine.md §31.3).
pub mod relay;

/// A simulator run's configuration (engine.md §31.1).
pub struct SimConfig {
    pub devices: u8,
    pub days: u32,
    pub faults: FaultSet,
    pub ops: OpSet,
    pub rules2: Vec<bool>,
}

/// The faults a run injects (engine.md §31.6).
pub struct FaultSet {}

/// The operations a run's workload may start (engine.md §31.5).
pub struct OpSet {}

/// A run's result (engine.md §31.1).
pub struct SimReport {
    pub seed: u64,
    pub trace_sha256: [u8; 32],
    pub violations: Vec<Violation>,
    pub stats: SimStats,
}

/// A broken invariant (engine.md §31.7, §37).
pub struct Violation {
    pub invariant: u8,
    pub line: u64,
    pub detail: String,
}

/// Counts per event and effect kind (engine.md §37).
pub struct SimStats {}

/// Runs the simulator (engine.md §31.1).
pub fn run(_seed: u64, _config: &SimConfig) -> SimReport {
    unimplemented!()
}
