//! The named configurations (engine.md §31.5, §31.6) and their JSON form for
//! regression files (§31.8).

use super::{FaultSet, OpSet, SimConfig};

/// Every name `SimConfig::named` knows.
pub const NAMES: &[&str] = &[
    "quiet", "gate", "loss", "dup", "delay", "disconnect", "crash", "skew", "rollback", "fork", "withhold", "inject",
    "revoke", "skip", "rules", "purge", "move", "restore", "recovery", "poison",
];

/// The days of a run: the gate must stay practical at 1,000 seeds on three
/// systems (task D's brief), so a run is one virtual day plus the settle.
const DAYS: u32 = 1;

fn base(faults: FaultSet, ops: OpSet) -> SimConfig {
    SimConfig { devices: 3, days: DAYS, faults, ops, rules2: vec![false, false, false] }
}

/// Every fault at a moderate rate (the gate's).
fn moderate_faults() -> FaultSet {
    FaultSet {
        loss_ppm: 20_000,
        dup_ppm: 10_000,
        delay_ms: 1_500,
        disconnects_per_hour: 1,
        crash_ppm: 5_000,
        skew_ms: 900_000,
        rollbacks: 1,
        fork: true,
        withhold: true,
        injected: 2,
    }
}

/// Every operation of §31.5.
fn every_op() -> OpSet {
    OpSet {
        revoke: 1,
        skip: true,
        rules_upgrade: true,
        purge: 1,
        move_relay: true,
        restore: 1,
        recovery_key: 1,
        poison: 1,
    }
}

pub(crate) fn named(name: &str) -> Option<SimConfig> {
    let q = FaultSet::default();
    let one = |f: fn(&mut FaultSet)| {
        let mut x = FaultSet::default();
        f(&mut x);
        base(x, OpSet::default())
    };
    let op = |f: fn(&mut OpSet)| {
        let mut x = OpSet::default();
        f(&mut x);
        base(FaultSet::default(), x)
    };
    Some(match name {
        "quiet" => base(q, OpSet::default()),
        "gate" => {
            let mut c = base(moderate_faults(), every_op());
            c.devices = 4;
            c.rules2 = vec![false, true, false, true];
            c
        }
        "loss" => one(|f| f.loss_ppm = 100_000),
        "dup" => one(|f| f.dup_ppm = 50_000),
        "delay" => one(|f| f.delay_ms = 5_000),
        "disconnect" => one(|f| f.disconnects_per_hour = 4),
        "crash" => one(|f| f.crash_ppm = 20_000),
        "skew" => one(|f| f.skew_ms = 7_200_000),
        "rollback" => one(|f| f.rollbacks = 2),
        "fork" => one(|f| f.fork = true),
        "withhold" => one(|f| f.withhold = true),
        "inject" => one(|f| f.injected = 3),
        "revoke" => op(|o| o.revoke = 1),
        "skip" | "poison" => op(|o| {
            o.poison = 1;
            o.skip = true;
        }),
        "rules" => {
            let mut c = op(|o| o.rules_upgrade = true);
            c.rules2 = vec![true, false, false];
            c
        }
        "purge" => op(|o| o.purge = 1),
        "move" => op(|o| o.move_relay = true),
        "restore" => op(|o| o.restore = 1),
        "recovery" => op(|o| o.recovery_key = 1),
        _ => return None,
    })
}

pub(crate) fn to_json(c: &SimConfig) -> String {
    let f = &c.faults;
    let o = &c.ops;
    let rules2: Vec<&str> = c.rules2.iter().map(|b| if *b { "true" } else { "false" }).collect();
    format!(
        concat!(
            "{{\"devices\": {}, \"days\": {}, \"faults\": {{\"loss_ppm\": {}, \"dup_ppm\": {}, \"delay_ms\": {}, ",
            "\"disconnects_per_hour\": {}, \"crash_ppm\": {}, \"skew_ms\": {}, \"rollbacks\": {}, \"fork\": {}, ",
            "\"withhold\": {}, \"injected\": {}}}, \"ops\": {{\"revoke\": {}, \"skip\": {}, \"rules_upgrade\": {}, ",
            "\"purge\": {}, \"move_relay\": {}, \"restore\": {}, \"recovery_key\": {}, \"poison\": {}}}, ",
            "\"rules2\": [{}]}}"
        ),
        c.devices,
        c.days,
        f.loss_ppm,
        f.dup_ppm,
        f.delay_ms,
        f.disconnects_per_hour,
        f.crash_ppm,
        f.skew_ms,
        f.rollbacks,
        f.fork,
        f.withhold,
        f.injected,
        o.revoke,
        o.skip,
        o.rules_upgrade,
        o.purge,
        o.move_relay,
        o.restore,
        o.recovery_key,
        o.poison,
        rules2.join(", "),
    )
}
