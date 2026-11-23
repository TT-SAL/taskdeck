//! The budget job of stage seal (SYNC_PLAN.md §3.3, §6.3): 256 entries
//! verified and opened within 100 ms, the median of five runs, in a release
//! build.
//!
//!   cargo run --release --locked -p taskdeck-seal --example budget
//!
//! A board is built through the public API (8 devices, one rotation, 512
//! accepted control entries), then 256 `cmd` entries by three authors with
//! payloads of 300 bytes. `classify` runs over the 256 in log order, each
//! from the membership the one before returned; five runs, the median. The
//! program prints
//!
//!   seal budget: median <n> us, limit 100000 us, baseline <b> us
//!
//! and exits 1 when the median is over the limit, or more than 25% over the
//! baseline in `budget-baseline.txt` (one integer, microseconds; 0 means no
//! baseline yet). It also prints, without judging them, the same measurement
//! on a membership with 100,000 control entries (the cost of the copy of the
//! state `classify` returns, a low finding of task S4) and the time to read a
//! backup whose history holds 2,000 records (the projection of §21.6 is
//! private; `read_backup` is the public call that runs it).
//!
//! The board's genesis and devices come from the integration tests' `Sim`
//! (`tests/common/board.rs`, which uses the public API only); the entries
//! after it are sealed and classified here directly, so that building
//! 100,000 control entries costs one `classify` each.

#[path = "../tests/common/mod.rs"]
mod common;
#[path = "../tests/common/board.rs"]
mod board;

use std::collections::BTreeMap;
use std::time::Instant;

use board::{Sim, Who};
use taskdeck_seal::*;

const LIMIT_US: u64 = 100_000;
const ENTRIES: usize = 256;
const RUNS: usize = 5;

/// The board after its genesis: the state, the chain, the roots, the
/// history, and the sealing of further entries.
struct Log<'s> {
    sim: &'s Sim,
    m: Membership,
    chains: Vec<Chain>,
    roots: BTreeMap<u32, EpochRoot>,
    history: Vec<HistoryRecord>,
    ctl_count: usize,
    home_gen: u32,
    now: i64,
}

impl<'s> Log<'s> {
    fn new(sim: &'s Sim) -> Self {
        let ctl_count = sim.history.len();
        Log {
            sim,
            m: Membership::from_canonical_bytes(&sim.m.to_canonical_bytes()).unwrap(),
            chains: sim.chains.clone(),
            roots: sim.roots.iter().map(|(e, r)| (*e, EpochRoot::from_bytes(*r))).collect(),
            history: sim.history.iter().map(|h| HistoryRecord::from_bytes(&h.to_bytes()).unwrap()).collect(),
            ctl_count,
            home_gen: 1,
            now: sim.now,
        }
    }

    fn seq(&self) -> u64 {
        self.chains.len() as u64 - 1
    }

    /// Device `i` seals an entry of `kind` with `payload` (a `ctl` payload is
    /// built from `items`) at the head.
    fn seal(&self, i: usize, kind: Kind, payload: Vec<u8>, items: &[CtlItem]) -> Vec<u8> {
        let keys = &self.sim.devs[i];
        let dev = keys.public().dev;
        let ctr = self.m.devices().iter().find(|d| d.dev == dev).map(|d| d.last_ctr + 1).unwrap();
        let epoch = self.m.epoch();
        let key = derive_epoch_key(&self.roots[&epoch], &self.sim.board, epoch, KeyKind::Entry);
        let entry_id = new_entry_id(&mut OsEntropy).unwrap();
        let signer = Signer::Device(keys);
        let payload = match kind {
            Kind::Ctl => ctl_payload(items, &signer, &self.sim.board, ctr, &entry_id, self.now).unwrap(),
            _ => payload,
        };
        let head = self.seq();
        let plain = Plain { entry_id, kind, at_ms: self.now, based_on: Head { seq: head, chain: self.chains[head as usize] }, payload };
        seal_frame(&signer, &self.sim.board, ctr, epoch, &key, &plain, &mut OsEntropy).unwrap()
    }

    /// Classifies `frame` at the next seq against the current state.
    fn classify(&self, frame: &[u8], m: &Membership, seq: u64, prev: Chain) -> Receipt {
        let roots = |e: u32| self.roots.get(&e);
        let chain_at = |b: u64| self.chains.get(b as usize).copied();
        let none = |_: u64| None;
        let input = ReceiptInput {
            seq,
            frame,
            prev_chain: prev,
            membership: m,
            roots: &roots,
            chain_at: &chain_at,
            retained_from: 1,
            own: None,
            supported_rules: &[1],
            genesis: None,
            skip_targets: &none,
        };
        classify(&input).unwrap()
    }

    /// Seals, classifies and appends one entry, which must be accepted.
    fn append(&mut self, i: usize, kind: Kind, payload: Vec<u8>, items: &[CtlItem]) -> Vec<u8> {
        let frame = self.seal(i, kind, payload, items);
        let seq = self.seq() + 1;
        let r = self.classify(&frame, &self.m, seq, self.chains[seq as usize - 1]);
        assert!(matches!(r.class, Class::Accept), "seq {seq}: {:?}", r.code);
        if kind == Kind::Ctl {
            self.history.push(HistoryRecord::from_accepted(seq, &frame, r.opened.as_ref().unwrap()).unwrap());
            self.ctl_count += 1;
        }
        self.m = r.membership;
        self.chains.push(r.chain);
        self.now += 1000;
        frame
    }

    /// A `Fence` by device `i`, the board's next home generation.
    fn fence(&mut self, i: usize) {
        self.home_gen += 1;
        let item = CtlItem::Fence(Fence { host: "relay.example.org".into(), door: [1, 2, 3, 4, 5], home_gen: self.home_gen });
        self.append(i, Kind::Ctl, Vec::new(), &[item]);
    }

    /// Control entries by the eight devices in turn until the board has
    /// accepted `n` of them.
    fn control_entries_to(&mut self, n: usize) {
        let mut i = 0;
        while self.ctl_count < n {
            self.fence(i % 8);
            i += 1;
        }
    }

    /// 256 `cmd` entries by devices 0, 1 and 2 in turn, 300-byte payloads;
    /// returns them with the state, seq and chain before the first.
    fn commands(&mut self) -> (Vec<u8>, u64, Chain, Vec<Vec<u8>>) {
        let before = self.m.to_canonical_bytes();
        let seq0 = self.seq();
        let chain0 = self.chains[seq0 as usize];
        let frames = (0..ENTRIES)
            .map(|k| {
                let mut payload = vec![b'x'; 300];
                payload[..8].copy_from_slice(&(k as u64).to_be_bytes());
                self.append(k % 3, Kind::Cmd, payload, &[])
            })
            .collect();
        (before, seq0, chain0, frames)
    }
}

/// The median of `RUNS` runs of `classify` over `frames` in log order, each
/// from the membership the one before returned, in microseconds.
fn measure(log: &Log<'_>, before: &[u8], seq0: u64, chain0: Chain, frames: &[Vec<u8>]) -> u64 {
    let mut times: Vec<u64> = (0..RUNS)
        .map(|_| {
            let mut m = Membership::from_canonical_bytes(before).unwrap();
            let mut prev = chain0;
            let t = Instant::now();
            for (k, f) in frames.iter().enumerate() {
                let r = log.classify(f, &m, seq0 + 1 + k as u64, prev);
                assert!(matches!(r.class, Class::Accept) && r.opened.is_some(), "entry {k}: {:?}", r.code);
                m = r.membership;
                prev = r.chain;
            }
            t.elapsed().as_micros() as u64
        })
        .collect();
    times.sort_unstable();
    times[RUNS / 2]
}

/// The median of `RUNS` reads of a nightly backup of the board as it stands
/// (its whole history in the file), in microseconds.
fn backup_read(log: &Log<'_>) -> (usize, u64) {
    let board_bytes = b"{\"cards\":[]}".to_vec();
    let membership = log.m.to_canonical_bytes();
    let board_hash: [u8; 32] = <sha2::Sha256 as sha2::Digest>::digest(&board_bytes).into();
    let seq = log.seq();
    let content = BackupContent::Nightly {
        state_hash: state_hash(1, &board_hash, &membership),
        membership,
        board: board_bytes,
        head: Head { seq, chain: log.chains[seq as usize] },
        hash_version: 1,
    };
    let input = BackupInput {
        board: log.sim.board,
        seq,
        written_ms: log.now,
        history: &log.history,
        content,
        roots: log.roots.iter().map(|(e, r)| (*e, EpochRoot::from_bytes(*r.to_bytes()))).collect(),
        read_token: Some("rt1.budget".into()),
        meta: b"{\"v\":1}".to_vec(),
        pinned_recovery: log.sim.rec.last().unwrap().public(),
    };
    let file = write_backup(&log.sim.devs[0], input, &mut OsEntropy).unwrap();
    let rk = log.sim.rec.last().unwrap();
    let mut times: Vec<u64> = (0..RUNS)
        .map(|_| {
            let t = Instant::now();
            let v = read_backup(&file, &log.sim.board, rk).unwrap();
            let us = t.elapsed().as_micros() as u64;
            drop(v);
            us
        })
        .collect();
    times.sort_unstable();
    (log.history.len(), times[RUNS / 2])
}

fn baseline() -> u64 {
    include_str!("../budget-baseline.txt").trim().parse().expect("budget-baseline.txt: one integer")
}

fn main() {
    // The board of the budget: 8 devices, one rotation, 512 control entries.
    let mut sim = Sim::new(9001, 8);
    sim.rotate(Who::D(0));
    let mut log = Log::new(&sim);
    log.control_entries_to(512);
    let (before, seq0, chain0, frames) = log.commands();
    let median = measure(&log, &before, seq0, chain0, &frames);
    let base = baseline();
    println!("seal budget: median {median} us, limit {LIMIT_US} us, baseline {base} us");

    // Not judged: the history read back at 2,000 records, and the same
    // measurement with 100,000 control entries.
    log.control_entries_to(2_000);
    let (records, read_us) = backup_read(&log);
    println!("seal budget (not judged): read_backup with {records} history records: median {read_us} us");
    let t = Instant::now();
    log.control_entries_to(100_000);
    let build_s = t.elapsed().as_secs_f64();
    let (before, seq0, chain0, frames) = log.commands();
    let big = measure(&log, &before, seq0, chain0, &frames);
    println!(
        "seal budget (not judged): {ENTRIES} entries on a membership with {} control entries ({} bytes): median {big} us (built in {build_s:.1} s)",
        log.ctl_count,
        before.len()
    );

    let mut failed = false;
    if median > LIMIT_US {
        println!("seal budget: FAILED: the median is over the limit");
        failed = true;
    }
    if base > 0 && u128::from(median) * 100 > u128::from(base) * 125 {
        println!("seal budget: FAILED: the median is more than 25% over the baseline");
        failed = true;
    }
    if failed {
        std::process::exit(1);
    }
}
