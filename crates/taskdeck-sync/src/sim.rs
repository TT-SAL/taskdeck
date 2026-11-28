//! The simulator (engine.md §31), compiled only with the feature `sim`.
//!
//! `SplitMix64` (engine.md §31.2) is what the fake sealer and the simulator
//! draw every random value from (ENG-FAKE-002). `run` drives `n` devices, each
//! an `Engine<ToyMachine, FakeSealer>` with a model runtime (`world`), against
//! one or two model relays (`relay`), on one virtual clock, and checks the six
//! invariants of §31.7 at the end (`check`, ENG-SIM-002). `FaultSet`, `OpSet`
//! and `SimStats` carry the fields stage `engine` gave them (the spec change
//! of §31.5, §31.6 and §37 names them).

mod splitmix;
pub use splitmix::SplitMix64;

/// The model relay (engine.md §31.3).
pub mod relay;

mod check;
mod counted;
mod config;
#[cfg(test)]
mod script;
mod store;
mod trace;
mod world;

use std::collections::BTreeMap;

/// A simulator run's configuration (engine.md §31.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimConfig {
    /// The number of machines, 2 to 4. Machine 0 creates the board, the last
    /// one follows the phone pattern of §29.4.
    pub devices: u8,
    /// `T_f` in days of virtual time (the workload and the faults run until
    /// then; the settle of 7,200,000 ms follows).
    pub days: u32,
    pub faults: FaultSet,
    pub ops: OpSet,
    /// Per machine: whether its build supports rules version 2 from the start
    /// (`ToyMachine::new(true)`, the fake's supported rules `[1, 2]`). A
    /// missing entry is `false`.
    pub rules2: Vec<bool>,
}

/// The faults a run injects (engine.md §31.6), one field per row of the
/// table, in the unit the table states. Every fault stops at `T_f`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct FaultSet {
    /// loss: parts per million of messages dropped, either direction (0 to 200,000).
    pub loss_ppm: u32,
    /// duplication: parts per million of messages delivered twice (0 to 50,000).
    pub dup_ppm: u32,
    /// reordering and delay: the largest extra delay drawn per message, ms (0 to 5,000).
    pub delay_ms: u32,
    /// disconnection: sessions closed per virtual hour, over all devices (0 to 4).
    pub disconnects_per_hour: u32,
    /// crash: parts per million of persists at whose boundary the process stops
    /// (0 to 20,000, that is 1 per 50 persists).
    pub crash_ppm: u32,
    /// clock skew: the largest offset of a device's `wall_ms`, ms (0 to
    /// 7,200,000); each device's offset is drawn in `−skew_ms..=skew_ms` and
    /// jumps to a fresh draw about once per virtual day.
    pub skew_ms: i64,
    /// relay rollback: rollbacks per run (0 to 2).
    pub rollbacks: u32,
    /// relay fork: whether one fork happens (0 or 1 per run).
    pub fork: bool,
    /// relay withholding: whether one device is withheld from (0 or 1 per run).
    pub withhold: bool,
    /// injected frame: frames with a bad signature or another board's id (0 to 3 per run).
    pub injected: u32,
}

/// The operations a run's workload starts (engine.md §31.5), one count or
/// switch per operation. Counts are per run, at drawn times before `T_f`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct OpSet {
    /// Revoke a device with `replace`, then re-pair the machine and resubmit its unsent edits (§24.5, §28.5).
    pub revoke: u32,
    /// Answer every crash halt's Skip offer (`here`) with `Operation::Skip` (§17).
    pub skip: bool,
    /// Update the builds without rules 2 to one with rules 2 (a restart with a new
    /// build id), so that the upgrade proposal (§25.3) activates it; `Mul` follows.
    pub rules_upgrade: bool,
    /// `Operation::Purge` (§28.2).
    pub purge: u32,
    /// `Operation::Move` to the second model relay, the others typing the move code (§28.3).
    pub move_relay: bool,
    /// `Operation::RestoreTo` a drawn offered target (§28.1).
    pub restore: u32,
    /// A recovery-key change with the current sheet, sealed as the recovery identity (§26.1).
    pub recovery_key: u32,
    /// Poison entries: a toy `Panic` sealed by a faulty build with a member's
    /// keys and appended unchecked (the "faulty build", §15). Poison configurations only.
    pub poison: u32,
}

/// A run's result (engine.md §31.1).
#[derive(Clone, Debug)]
pub struct SimReport {
    pub seed: u64,
    pub trace_sha256: [u8; 32],
    pub violations: Vec<Violation>,
    pub stats: SimStats,
}

/// A broken invariant (engine.md §31.7, §37).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    pub invariant: u8,
    pub line: u64,
    pub detail: String,
}

/// Counts that show what a run exercised (engine.md §37).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SimStats {
    /// Events delivered, by the event's name (the trace's `ev`).
    pub events: BTreeMap<&'static str, u64>,
    /// Effects emitted, by the effect's name (the trace's `fx`).
    pub effects: BTreeMap<&'static str, u64>,
    /// Notifications emitted, by name.
    pub notifications: BTreeMap<&'static str, u64>,
    /// The reference log's head seq at the end (entries appended, genesis included).
    pub entries: u64,
    /// `Enqueue`s sent; answered `Enqueued`; answered `EnqueueRefused`.
    pub enqueued: u64,
    pub enqueue_ok: u64,
    pub enqueue_refused: u64,
    /// Process starts, the first included; crashes at a persist boundary.
    pub starts: u64,
    pub crashes: u64,
    /// Devices paired (joined through an engine).
    pub paired: u64,
    /// Alarms raised, by kind.
    pub alarms: BTreeMap<&'static str, u64>,
    /// Faults injected, by kind (the trace's `fault` lines).
    pub faults: BTreeMap<&'static str, u64>,
    /// Operations started and finished `Ok`, by kind.
    pub ops_started: BTreeMap<&'static str, u64>,
    pub ops_ok: BTreeMap<&'static str, u64>,
    /// Devices halted, and devices stopped, at the end.
    pub halted_at_end: u64,
    pub stopped_at_end: u64,
    /// Trace lines written.
    pub trace_lines: u64,
    /// `ScheduleTick`s asking for a tick at most 1 ms after the call (a busy timer).
    pub ticks_due_now: u64,
    /// Logins the model runtime made itself because a token was stale (§11.9).
    pub token_renewals: u64,
}

/// Runs the simulator (engine.md §31.1).
pub fn run(seed: u64, config: &SimConfig) -> SimReport {
    world::run(seed, config, false).0
}

/// Runs the simulator and also returns the trace's bytes (engine.md §31.9).
pub fn run_traced(seed: u64, config: &SimConfig) -> (SimReport, Vec<u8>) {
    world::run(seed, config, true)
}

pub use config::NAMES;

impl SimConfig {
    /// A named configuration: `quiet`, `gate`, a single fault (`loss`, `dup`,
    /// `delay`, `disconnect`, `crash`, `skew`, `rollback`, `fork`, `withhold`,
    /// `inject`), a single operation (`revoke`, `skip`, `rules`, `purge`,
    /// `move`, `restore`, `recovery`), or `poison`.
    pub fn named(name: &str) -> Option<SimConfig> {
        config::named(name)
    }

    /// The JSON object of this configuration, for a regression file (§31.8).
    pub fn to_json(&self) -> String {
        config::to_json(self)
    }
}
