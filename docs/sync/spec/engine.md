# TaskDeck sync spec: engine

*The replica engine: confirmed and pending state, the outbox, send and reconcile, receipt checks mapped to actions, refusals and halts, poison pills, Skip, checkpoints, bases and state hashes, snapshots and bootstrap, compaction, integrity evidence and the stop, determinism mismatch, beats, epochs and re-sealing, rules upgrades, timed windows, the clock-skew guard, the operations, duties with no always-on device, what the user interfaces are told, and the deterministic simulator. Owning stage: `engine`. Conventions, notation and the glossary are in [`README.md`](README.md); this spec does not restate them.*

<!-- OUTLINE (draft pass 1)
1 Scope · 2 Terms · 3 Rules for every implementation · 4 The model · 5 The state-machine trait and the toy state machine
6 The sealer trait and the fake sealer · 7 Replica state and persistent records · 8 Confirmed and pending state · 9 Enqueue
10 Send · 11 Sessions, reconcile and echoes · 12 Receive · 13 Classes, refusals and notices · 14 Control entries and banners
15 Poison pills · 16 Halts · 17 Skip · 18 Checkpoints, bases and state hashes · 19 Snapshots and bootstrap · 20 Compaction
21 Integrity evidence and the stop · 22 Determinism mismatch and adoption · 23 Beats · 24 Epochs and re-sealing
25 Rules upgrades · 26 Timed windows · 27 The clock · 28 Operations · 29 Duties without an always-on device
30 What the user interfaces are told · 31 The simulator · 32 The fifteen V2 scenarios
33 Error catalogue · 34 Limits · 35 Test vectors · 36 Interfaces · 37 Public API · 38 Open questions
-->

## 1. Scope

This spec fixes everything the replica engine (`crates/taskdeck-sync/`) decides: which events it consumes, which effects it emits and in what order, what it persists and when, how it turns the receipt classes of `seal.md` §14 into actions, and every duty a device takes when no device is always on. It realises:

- SYNC.md §4.3 (client rules 1 to 6), §4.5 (trial apply, rules upgrades, the halted view, the clock), §4.6 (Skip, the naming hold, the persistent refusal and notice lists), §4.7 (the view, catch-up, snapshots, compaction duty, bootstrap, purge), §4.8 (integrity checks, evidence, stop and show, restore tail, determinism mismatch), §4.2 (what the outcomes and the three halts mean for a replica), §5.3 (the engine's part of pairing: the bundle snapshot and the `DeviceAdded` entry), §5.4 (revocation, rotation, races, re-sealing, a returning revoked device, reset trust), §5.5 (the 72-hour recovery-key change window), §6.4 and §6.6 (restore to a point), §6.7 (move board, the move code, following a move), §9 (bootstrap from the genesis);
- the verifier findings V1-1, V1-10, V1-11, V1-16; V2-3, V2-5 (the `based_on` the engine carries), V2-6, V2-7, V2-8, V2-9, V2-10, V2-13 (purge), V2-14, V2-15, V2-16 (the persistent lists), V2-17, V2-20 as modified, V2-21; V3-2, V3-6 (the data behind the phone's sync line, refusals and marks), V3-14; the fifteen V2 scenarios S1 to S15 (§32);
- SYNC_PLAN.md §2 (no hub, no always-on device, limited administration from the phone), §3.4 (security rules), §6.1 item 3, §6.5 (the `engine` stage, its simulator and its gate), §6.7 (what `device` wires in), §6.9 and §6.10 (what the two apps show);
- the rulings S5 to S8, S10, S13 to S15 of `docs/sync/status/foundation.md`, and decision D6 (the traits live in this crate).

The `engine` stage builds the engine with no I/O of its own, generic over a state machine and a sealer, plus a toy state machine, a fake sealer and a seeded simulator (§31).

Non-goals, each owned elsewhere:

- every byte of frames, envelopes, control items, membership, the receipt checks themselves, the chain and the pairing machines: `seal.md`;
- board data, log commands, `apply`, the compile step, notices and refusal records as records, canonical bytes: `core.md`;
- routes, socket messages, relay storage, budgets and relay-side checks: `relay.md`;
- files and their byte encodings, fsync, transport, login and tokens, key stores, blobs on disk, backups and their schedule, which operations each platform offers: `device.md`;
- the screens and their wording: stages `desktop` and `android` (from §30's data).

There is no hub and no always-on device (SYNC_PLAN.md §2): every duty falls to whichever device is awake (§29).

## 2. Terms

Terms used here that README §9 does not define.

| Term | Meaning |
|---|---|
| runtime | The code around the engine (`taskdeck-device`, or the simulator) that turns effects into I/O and I/O into events. |
| input | One event together with the local wall clock and monotonic clock at which the runtime delivers it (§4.2). |
| log head | `(L, C_L)`: the highest seq whose frame is durable in this device's log. |
| class head | `K`: the highest seq whose frame has been classified by the sealer, with `M_K`. Equal to the log head except while a page is being processed. |
| board head | `B`: the highest seq whose entry has been applied (or skipped, or refused) by the state machine; the confirmed state is the state at `B`. Below the class head only under the crash halt (§16). |
| cursor | The seq after which the engine reads next: the log head. |
| record | One outbox record (§7.4): a compiled entry of this device that has not yet reached the board head. |
| unapplied record | A record whose entry has not yet been applied to the confirmed state. |
| provisional seq | The seq the pending view gives an unapplied record: `B + i` for the `i`-th unapplied record (§8.2). |
| doomed | An unapplied record whose application to the pending view is refused or panics (§8.3); shown as "will be refused". |
| eligible | A device that may take duties now (§29.1). |
| stop | One of the conditions of §21 and §22 under which the engine sends nothing and, for evidence, applies nothing new; distinct from a halt. |
| evidence | The data that proves an integrity alarm (§21.1). |
| usable snapshot | A snapshot acknowledged by a device other than its writer and mismatched by none, as this device knows (§19.3). |
| due | A snapshot, beat or duty whose condition holds at this device now. |
| rank | A device's position in the duty order of §29.2. |
| log clock `τ_s` | `M_s.clock_ms` (`seal.md` §13.5): the largest `at_ms` of the accepted entries up to `s`. |
| attempt | One run of the apply pipeline after a `Start`, or after a rebuild that follows a caught panic (§15). |
| re-sealable | A refusal code after which an own `cmd` entry is sealed again under a new counter (§24.3). |
| op | A user operation the engine runs over several entries and relay calls (§28). |

## 3. Rules for every implementation

These hold for all code in `crates/taskdeck-sync/` except its tests and benches.

- **ENG-GEN-001** (engine) The `taskdeck-sync` crate MUST declare `#![forbid(unsafe_code)]` at its root.
- **ENG-GEN-002** (engine) The `taskdeck-sync` crate MUST compile for `wasm32-unknown-unknown` with its default features and with the feature `sim`.
- **ENG-GEN-003** (engine) The engine MUST NOT read a clock, the file system, the network, environment variables, thread-local state or a random source, and MUST NOT spawn threads; every time value it uses MUST come from an input (§4.2) and every random byte from the sealer (§6.1).
- **ENG-GEN-004** (engine) The crate's clippy configuration MUST list `std::time::SystemTime::now`, `std::time::Instant::now`, `std::thread::spawn`, `std::fs::*` and `std::net::*` under `disallowed-methods`, and clippy MUST pass with these as errors.
- **ENG-GEN-005** (engine) The crate's non-development dependencies MUST be exactly `sha2` and `zeroize` at the workspace's pinned versions; it MUST NOT depend on `taskdeck-core` or `taskdeck-seal` (decision D6).
- **ENG-GEN-006** (engine) Every decision of the engine MUST be a function of its persisted records, its inputs and the results of the trait methods it calls: it MUST NOT iterate a `HashMap` or `HashSet` where the order can change an effect, and MUST NOT use `f32` or `f64` values.
- **ENG-GEN-007** (engine) The engine MUST compute SHA-256 only through the `sha2` crate and MUST NOT contain its own implementation of any hash, MAC, cipher or signature.
- **ENG-GEN-008** (engine) The engine MUST NOT parse a `cmd` payload, a `beat` or a `snap` payload, or a control item from a frame before the sealer has classified that frame `Accept` (SYNC_PLAN.md §3.4: nothing parsed before it is authenticated).
- **ENG-GEN-009** (engine) The engine MUST check every count and length in an event (entries per page, frames per batch, frame sizes, record counts) against its limit in §34 before it allocates for it, and MUST treat an event that exceeds one as a protocol error of the runtime (`engine/runtime_protocol`).
- **ENG-GEN-010** (engine) The engine MUST hold secret bytes (the identity export of §6.1) only inside `zeroize::Zeroizing` values and MUST NOT copy them into any other value, log, notice, status or error.
- **ENG-GEN-011** (engine) The engine MUST NOT compare a secret value itself; every comparison of a move code, a key or a MAC MUST be the sealer's (`seal.md` SEAL-GEN-007).
- **ENG-GEN-012** (engine, device) Every call of a state-machine or sealer method that can run code derived from replicated data (`apply`, `trial_apply`, `parse`, `load`, `canonical_bytes`, `upgrade`, `reset`, `classify`, `open_snapshot`) MUST run inside `std::panic::catch_unwind`, and a caught panic MUST be handled as §15 states for `apply` and as a local fault (`engine/local_fault`, §21.4) for every other method.
- **ENG-GEN-013** (engine) No error, status, notice or alarm the engine produces MUST contain a board text (an item name, notes, a subscription name or URL), a token or a key; only seqs, ids, device names, key fingerprints, codes and counts.

## 4. The model

### 4.1 Shape

```text
            ┌──────────────── runtime (device, or the simulator) ────────────────┐
 inputs ──▶ │  Input { wall_ms, mono_ms, event }  ─▶  Engine<SM, S>::handle  ─▶  Vec<Effect>  │ ──▶ I/O
            └────────────────────────────────────────────────────────────────────────┘
```

`Engine<SM, S>` holds one board's replica. `SM: StateMachine` (§5) is the board's state machine (`taskdeck-core` through an adapter, or the toy of §5.3); `S: Sealer` (§6) is the frame and membership layer (`taskdeck-seal` through an adapter, or the fake of §6.3). The engine is a deterministic function: `handle(&mut self, input) -> Vec<Effect>`. It never blocks, never waits and never calls the runtime; it learns the outcome of every effect from a later event.

- **ENG-MODEL-001** (engine) The engine MUST change its state only inside `handle`, and the effects one call returns MUST be a function of the engine's state before the call and the input.
- **ENG-MODEL-002** (engine) Two engines constructed from equal records, equal configuration and equal trait implementations, fed equal input sequences, MUST return equal effect sequences.

### 4.2 Inputs

```rust
pub struct Input<C> { pub wall_ms: i64, pub mono_ms: u64, pub event: Event<C> }
```

`wall_ms` is the device's local clock in milliseconds since 1970-01-01T00:00:00Z when the runtime delivers the event; `mono_ms` is a monotonic millisecond count that never decreases within one process and restarts at 0 or any value with each `Start`. `C` is the state machine's command type.

| Event | Fields | Sent by the runtime when |
|---|---|---|
| `Start` | `loaded: Loaded` (§7.10) | the process opens the board, after reading every record |
| `Shutdown` | | the process is about to exit |
| `Tick` | | at or after the time of the latest `ScheduleTick`, and whenever it likes; extra ticks are harmless |
| `Persisted` | `id: u64` | the persist effect `id` is durable (§4.4) |
| `PersistFailed` | `id: u64` | the persist effect `id` could not be made durable |
| `LocalRead` | `req: u64, records: Vec<LogRecord>` or `base: Checkpoint` | a `ReadLog` or `ReadBase` effect completed |
| `Enqueue` | `ticket: u64, command: C, at_ms: i64, origin_state: Vec<u8>, hold: bool` | the facade compiled an intent with `now = at_ms` (§9) |
| `ReleaseHold` | | the user finished naming a held create (§9.3) |
| `Op` | `ticket: u64, op: Operation` | the user started an operation (§28) |
| `PairRegister` | `ticket: u64, device: NewDevice` | the existing-device pairing machine emitted `NeedSnapshotAndRegister` (`seal.md` §17.10) |
| `Resubmit` | `ticket: u64, commands: Vec<C>, from_device_no: u16, origin_state: Vec<u8>` | a re-paired identity imports an old identity's unsent commands (§24.5) |
| `User` | `action: UserAction` (§30.4) | the user acknowledged, confirmed or chose something |
| `Connected` | `push: bool` | a session to the relay is open (socket, or HTTPS reachability) |
| `Welcome` | `Welcome` (§11.1) | a socket welcome, or a `hello` response, arrived |
| `Page` | `req: Option<u64>, page: Page` (§12.1) | a read answer (`req` set) or a push (`req` absent) arrived |
| `AppendResults` | `req: u64, results: Vec<AppendResult>` | an append answer arrived (`relay.md` §9.6) |
| `AdminResult` | `req: u64, result: Result<AdminOk, RelayRefusal>` | an admin operation answered |
| `BlobResult` | `req: u64, result: Result<BlobOk, RelayRefusal>` | a blob upload, download or listing finished |
| `RelayRefused` | `req: Option<u64>, refusal: RelayRefusal` | a whole request was refused (`relay.md` §4.2), or the relay answered `fenced`, `revoked` or a budget |
| `Second` | `session: [u8; 16]` | the relay sent `second` (`relay.md` §11.3) |
| `Displaced` | `session: [u8; 16]` | the relay sent `displaced` |
| `Disconnected` | `unreachable: bool, paused_until_ms: Option<i64>` | the session ended; `unreachable` per `relay.md` §14.5; `paused_until_ms` for a quota pause |

- **ENG-MODEL-003** (engine) The engine MUST use `wall_ms` only where §27 and the clauses that name it allow (composing `at_ms`, offering actions, the skew guard, local day boundaries, duty waits), and `mono_ms` only for timeouts and cadences of §34.
- **ENG-MODEL-004** (engine) The engine MUST treat an input whose `mono_ms` is lower than the previous input's within one `Start` as `engine/runtime_protocol` and MUST ignore it.

### 4.3 Effects

| Effect | Fields | What the runtime does |
|---|---|---|
| `Persist` | `id: u64, op: PersistOp` (§7.11) | performs the write in the serial order of §4.4, then sends `Persisted{id}` |
| `ReadLog` | `req: u64, from: u64, to: u64` | reads the durable log records `from..=to` and sends `LocalRead` |
| `ReadBase` | `req: u64, seq: u64` | reads the base checkpoint at `seq` and sends `LocalRead` |
| `Connect` | `relay: RelayTarget` | logs in if it has no valid token (`device.md`), opens a socket if the relay pushes, else uses HTTPS; sends `Connected`, then `Welcome` |
| `Disconnect` | | closes the session; sends nothing |
| `Hello` | | sends `hello` over HTTPS (or nothing over a live socket, whose welcome is already in) and sends `Welcome` |
| `Read` | `req: u64, after: u64, limit: u16` | sends a read (socket message or `GET …/log`) and sends `Page{req}` |
| `Append` | `req: u64, frames: Vec<Vec<u8>>` | sends one batch of `relay.md` §9.2 and sends `AppendResults{req}` |
| `Admin` | `req: u64, op: AdminRequest` (§11.6) | fetches an admin nonce, has the sealer sign (`seal.md` §16.2), sends the request, sends `AdminResult` |
| `Blob` | `req: u64, op: BlobRequest` (§19.6) | performs the blob routes of `relay.md` §10.7 and sends `BlobResult` |
| `ScheduleTick` | `at_mono_ms: u64` | delivers a `Tick` at or after that time; a later `ScheduleTick` replaces an earlier one |
| `Notify` | `Notification` (§30.3) | passes it to the facade; it changes nothing durable |

- **ENG-EFFECT-001** (engine, device) The runtime MUST execute every effect other than `Persist` in the order the engine emitted it, without waiting for any `Persist` to complete.
- **ENG-EFFECT-002** (engine) The engine MUST give every `Persist`, `ReadLog`, `ReadBase`, `Read`, `Append`, `Admin` and `Blob` effect an id unique within the process (`id` or `req`, from one counter starting at 1 at `Start`), and MUST ignore an answer whose id it did not issue or already received.
- **ENG-EFFECT-003** (engine) The engine MUST have at most one `Read` and at most one `Append` or frame-carrying `Admin` outstanding at a time.

### 4.4 Durability

Persist effects form one serial queue. The runtime makes each one durable (`device.md` fsync rules) before it starts the next, and reports `Persisted` in the same order. The engine emits an effect that depends on a write only after that write's `Persisted` has arrived. The dependencies are the table of §7.12.

- **ENG-PERSIST-001** (engine, device) The runtime MUST start writing a `Persist` effect only after every earlier `Persist` effect of the same engine is durable, and MUST send the `Persisted` events in the order of the effects.
- **ENG-PERSIST-002** (engine) The engine MUST NOT emit an effect listed in the "later effect" column of §7.12 before the `Persisted` of every write its row names has arrived.
- **ENG-PERSIST-003** (engine) On `PersistFailed`, the engine MUST stop with the local fault `engine/persist_failed` (§21.4), MUST emit no further `Persist`, `Append`, `Admin` or `Blob` effect until the next `Start`, and MUST treat every unconfirmed write as not done.
- **ENG-PERSIST-004** (engine, device) A runtime that crashes or is killed MUST, at the next `Start`, present every record whose `Persisted` it sent and MAY present records whose write was in progress only if they are complete and valid (`seal.md` §19.4 torn tails are dropped by `device.md`).

### 4.5 One call, in order

Within one `handle` call, the engine processes the event, then re-evaluates in this order and appends the effects each step produces:

1. the session (§11);
2. receive and apply (§12 to §17);
3. the pending view, if a rebase is due (§8.4);
4. operations in progress (§28);
5. duties (§29);
6. the send loop (§10);
7. checkpoint cadence (§18.1);
8. the next `ScheduleTick` (the earliest deadline among §34's cadences and timeouts that is armed);
9. `Notify` effects for every changed part of §30.

- **ENG-MODEL-005** (engine) The engine MUST evaluate the nine steps of §4.5 in order after every event and MUST emit at most one `ScheduleTick` per call, the last one before the `Notify` effects.

## 5. The state-machine trait and the toy state machine

### 5.1 The trait

The engine crate defines the trait (decision D6). `taskdeck-device` implements it over `taskdeck-core` (`core.md` §8 to §11, §20); the toy of §5.3 implements it for the simulator.

```rust
pub struct Ctx { pub seq: u64, pub device_no: u16, pub at_ms: i64, pub based_on: u64 }

pub struct Applied<N> { pub benign: bool, pub notices: Vec<N> }

pub struct SmRefusal {
    pub code: &'static str,          // e.g. "core/gone", "toy/fail"
    pub op: Option<&'static str>,
    pub target: Option<u64>,
    pub base: Option<u64>,           // set_notes conflicts (core.md §10.7)
    pub current: Option<u64>,
}
pub struct SmError { pub code: &'static str }

pub enum Coalesce<C> { Merge(C), CancelBoth, Keep }

pub trait StateMachine {
    type State: Clone;
    type Command: Clone;
    type Notice: Clone;

    fn supported_rules(&self) -> &[u32];
    fn hash_versions(&self) -> &[u32];

    fn load(&self, bytes: &[u8]) -> Result<Self::State, SmError>;
    fn canonical_bytes(&self, state: &Self::State) -> Vec<u8>;
    fn board_hash(&self, state: &Self::State) -> [u8; 32];
    fn rules(&self, state: &Self::State) -> u32;
    fn upgrade(&self, state: &mut Self::State, v: u32) -> Result<(), SmError>;

    fn parse(&self, payload: &[u8]) -> Result<Self::Command, SmError>;
    fn encode(&self, command: &Self::Command) -> Vec<u8>;
    fn op_name(&self, command: &Self::Command) -> &'static str;
    fn target(&self, command: &Self::Command) -> Option<u64>;

    fn apply(&self, state: &mut Self::State, command: &Self::Command, ctx: &Ctx)
        -> Result<Applied<Self::Notice>, SmRefusal>;
    fn trial_apply(&self, state: &Self::State, command: &Self::Command, ctx: &Ctx)
        -> Result<Applied<Self::Notice>, SmRefusal>;
    fn reset(&self, target: &Self::State, current: &Self::State) -> Self::State;

    fn hold(&self, command: &Self::Command) -> Option<u64>;
    fn coalesce(&self, held: &Self::Command, next: &Self::Command) -> Coalesce<Self::Command>;
    fn rebind(&self, commands: &[Self::Command], from_device_no: u16, to_device_no: u16,
              origin_state: &[u8]) -> Result<(Vec<Self::Command>, Vec<u8>), SmError>;

    fn notice_concerns(&self, notice: &Self::Notice, device_no: u16) -> bool;
    fn notice_bytes(&self, notice: &Self::Notice) -> Vec<u8>;
}
```

**Contracts.**

| Method | Contract |
|---|---|
| `supported_rules` | the rules versions this build applies, ascending, at least `[1]` (`core.md` `SUPPORTED_RULES`) |
| `hash_versions` | the state-hash versions this build computes, ascending; `[1]` in this build |
| `load` | parses canonical state bytes exactly as stored, with no normalisation (`core.md` §6.14, V2-19); an error for any other input |
| `canonical_bytes` | the canonical state bytes; `load(canonical_bytes(s))` gives a state with the same bytes |
| `board_hash` | `SHA-256(canonical_bytes(state))`; an implementation may cache, never change, the value |
| `rules` | the state's active rules version (`BoardData.rules`) |
| `upgrade` | `core.md` §11.2: refuses a version that is not newer or not supported, else activates it; leaves the state unchanged on error |
| `parse` | the strict command parser (`core.md` §9.1); called only on an opened, accepted `cmd` payload |
| `encode` | the canonical payload of a command; `parse(encode(c))` returns `c` |
| `apply` | pure and deterministic in `(state, command, ctx)` (`core.md` CORE-CTX-002); on `Err` the state is unchanged; may panic only through a bug |
| `trial_apply` | `apply` on a copy; never changes `state` |
| `reset` | the state a `ResetTo` installs: `target`'s data with the id high-water marks of `current` kept (elementwise maximum) and any revision counter of `current` raised above both (§28.1) |
| `hold` | `Some(id)` when the command creates the item `id` and may be held while it is named (V2-17); else `None` |
| `coalesce` | for a held create and the next command: `Merge(c)` when `next` renames the held item (`c` is the create carrying the new name), `CancelBoth` when `next` forgets it, `Keep` otherwise |
| `rebind` | rewrites commands minted by `from_device_no` for `to_device_no`: every id the commands create with high part `from_device_no` is re-minted from `origin_state`, and every later reference to it is rewritten; returns the commands and the new origin state (§24.5) |
| `notice_concerns` | whether a notice is to be kept by the device with this `device_no` (`core.md` `Notice::concerns`) |
| `notice_bytes` | the canonical bytes of a notice record (`core.md` §10.7) |

- **ENG-SM-001** (engine) The engine MUST call `apply` only with a `Ctx` whose `device_no` is the author's `device_no` in `M_{s−1}`, whose `at_ms` and `based_on` are the entry's plaintext fields, and whose `seq` is the entry's seq for the confirmed state and its provisional seq (§8.2) for the pending view.
- **ENG-SM-002** (engine) The engine MUST call `upgrade` with version `v` immediately after it accepts the control entry that activates `v` and before it applies any entry above that seq.
- **ENG-SM-003** (engine) The engine MUST treat a `parse` failure of an accepted `cmd` payload as a refusal of that entry on every device with the parser's code, exactly as `seal.md` §14.4 maps it to `Refuse`.
- **ENG-SM-004** (engine) The engine MUST keep a state machine's `Notice` values produced while applying to the pending view only for display and MUST record only notices produced while applying to the confirmed state (`core.md` §10.7).

### 5.2 Hash of a state

The engine composes the state hash (ruling S7, §18.3) from `board_hash` and the sealer's membership bytes. Only hash version 1 exists.

### 5.3 The toy state machine

`ToyMachine` lives in the engine crate behind the feature `sim`. Two independent implementations from this section give equal bytes and equal results.

**State.** `rules: u32`; `keys`: a map from `u8` key to `(value: i64, stamp_seq: u64, stamp_dev: u16)`; `items`: a map from `u64` id to `name: u32`; `marks`: a map from `u16` device number to `u64` (the highest item number minted by that device).

**Canonical bytes** (all integers big-endian):

| Field | Width | Rule |
|---|---|---|
| `magic` | 4 | `"TOY1"` |
| `rules` | 4 | u32, a supported version |
| `n_keys` | 2 | u16, 0 to 256 |
| keys | 19 × `n_keys` | `u8 key ‖ i64 value ‖ u64 stamp_seq ‖ u16 stamp_dev`, strictly ascending by `key` |
| `n_items` | 4 | u32, 0 to 10,000 |
| items | 12 × `n_items` | `u64 id ‖ u32 name`, strictly ascending by `id` |
| `n_marks` | 2 | u16, 0 to 8,191 |
| marks | 10 × `n_marks` | `u16 device_no ‖ u64 n`, strictly ascending by `device_no`, `n ≥ 1` |
| **total** | **16 + 19k + 12i + 10m** | no trailing bytes |

`load` refuses with `toy/format` any input that violates this table, an item id whose high part `h = id div 2^40` is 0 or above 8,191, whose low part `n = id mod 2^40` is 0, or whose `n` exceeds the mark of `h`. The empty state is `rules = 1` and no keys, items or marks (16 bytes).

**Commands** (one `u8` op, then the fields):

| op | Name | Fields | Width | Rules versions |
|---|---|---|---|---|
| 1 | `Set` | `u8 key ‖ i64 value` | 10 | 1, 2 |
| 2 | `Add` | `u8 key ‖ i64 delta` | 10 | 1, 2 |
| 3 | `Del` | `u8 key` | 2 | 1, 2 |
| 4 | `Create` | `u64 id ‖ u32 name` | 13 | 1, 2 |
| 5 | `Name` | `u64 id ‖ u32 name` | 13 | 1, 2 |
| 6 | `Forget` | `u64 id` | 9 | 1, 2 |
| 7 | `Fail` | `u8 code` | 2 | 1, 2 |
| 8 | `Panic` | | 1 | 1, 2 |
| 9 | `Mul` | `u8 key ‖ i64 factor` | 10 | 2 |

`parse` refuses with `toy/parse` an unknown op or a payload whose length is not the op's width; it accepts op 9 under any rules (the rules check is `apply`'s). `op_name` is the name in lowercase; `target` is the key (as `u64`) or the id.

**`apply`**, in this order, stopping at the first failure:

1. `ctx.device_no` from 1 to 8,191, `ctx.seq ≥ 2`, `ctx.based_on < ctx.seq`; else `toy/bad_ctx`.
2. The op is allowed by `rules` (op 9 only under 2); else `toy/rules`.
3. Per op:
   - `Set`: if the key exists with `stamp_seq > ctx.based_on`, `stamp_dev ≠ ctx.device_no` and `stamp_dev ≠ 0`, one notice `{seq: ctx.seq, author: ctx.device_no, other: stamp_dev, other_seq: stamp_seq, key}`; then `value` is set and the stamp becomes `(ctx.seq, ctx.device_no)`.
   - `Add` and `Mul`: the key must exist (else `toy/gone`, target the key); the checked sum or product must not overflow (else `toy/overflow`); the value and stamp are set as for `Set`, without a notice.
   - `Del`: an absent key is a benign repeat; else the key is removed.
   - `Create`: `h = id div 2^40` must equal `ctx.device_no` and `n = id mod 2^40` must be from 1 to 2^40 − 1 and above `marks[h]` (0 when absent) (else `toy/bad_id`); fewer than 10,000 items (else `toy/cap`); then the item is inserted and `marks[h] = n`.
   - `Name`: the item must exist (else `toy/gone`); its name is set.
   - `Forget`: an absent item is a benign repeat; else it is removed.
   - `Fail`: refused with `toy/fail`.
   - `Panic`: calls `panic!` with a constant message.

**Other methods.** `supported_rules` is `[1]`, or `[1, 2]` when the toy is constructed with `rules2 = true`. `hash_versions` is `[1]`. `upgrade(v)`: `v > rules` and `v` supported, else `toy/unknown_rules`; then `rules = v`. `reset(target, current)` is `target` with `rules = current.rules` and every mark the maximum of the two. `hold` is `Some(id)` for `Create`. `coalesce(Create{id, name}, next)` is `Merge(Create{id, name: m})` for `Name{id, name: m}` with the same `id`, `CancelBoth` for `Forget{id}` with the same `id`, else `Keep`. The origin state is the toy minter, `u64 next_n` (8 bytes, big-endian). `rebind` maps, in order, every `Create` whose id has high part `from_device_no` to `to_device_no × 2^40 + next_n` (then `next_n += 1`), and rewrites `Name` and `Forget` ids by that mapping. A notice's bytes are `u64 seq ‖ u16 author ‖ u16 other ‖ u64 other_seq ‖ u8 key` (21 bytes); it concerns `author` and `other`.

- **ENG-TOY-001** (engine) `ToyMachine` MUST encode, parse, apply, reset, coalesce and rebind exactly as §5.3 states, and MUST produce the bytes of the vectors `engine-toy.json` (§35).
- **ENG-TOY-002** (engine) `ToyMachine` and `FakeSealer` MUST be compiled only with the cargo feature `sim`, and no default feature of `taskdeck-sync` MUST enable it.

## 6. The sealer trait and the fake sealer

### 6.1 The trait

`taskdeck-device` implements the sealer over `taskdeck-seal` (`seal.md` §26); the fake of §6.3 implements it for the simulator. The sealer owns every secret: the device keys, the epoch roots it holds, the genesis anchor when it has one, and the roots it drew for its own unsent rotations. It also owns the only random source the engine uses.

```rust
pub type Dev = [u8; 16];
pub type Hash32 = [u8; 32];
pub struct Head { pub seq: u64, pub chain: Hash32 }
pub enum Kind { Cmd = 1, Ctl = 2, Beat = 3, Snap = 4 }
pub enum Class { Skip, Tamper, Refuse, Accept, NeedFrames(Vec<(u64, Hash32)>) }
pub enum Halt { Commitment, UnknownRules(u32) }
pub enum Verdict { Ack, Mismatch }

pub struct BeatInfo { pub rules: Vec<u32>, pub envelopes: Vec<(u8, u8)>, pub hash_versions: Vec<u32>,
                      pub state: Option<(u64, Hash32, u32)>, pub acks: Vec<(u64, Verdict)> }
pub struct SnapInfo { pub seq: u64, pub hash: Hash32, pub hash_version: u32, pub blob: [u8; 16],
                      pub sha256: Hash32, pub size: u64, pub parts: u32, pub inflated: u64, pub epoch: u32 }

pub enum RecoveryMode { Direct, Propose, Confirm, Cancel }
pub enum UpgradeMode { Normal, Override }
pub enum CtlItem {
    Genesis,
    DeviceAdded { dev: Dev, name: String, key_fp: String, bootstrap_seq: u64 },
    DeviceRevoked { dev: Dev },
    DeviceReinstated { dev: Dev },
    EpochRotated { epoch: u32, purge_before: Option<u64> },
    RecoveryKeyChange { mode: RecoveryMode, key_fp: Option<String> },
    Fence { host: String, door: [u8; 5], home_gen: u32 },
    ResetTo { seq: u64, state_hash: Hash32, hash_version: u32 },
    Skip { seq: u64, frame_hash: Hash32 },
    RulesUpgrade { rules: u32, mode: UpgradeMode },
}
pub enum CtlIntent {
    Rotate { revoke: Vec<Dev>, reinstate: Vec<Dev>, purge_before: Option<u64> },
    AddDevice { dev: Dev, sign_pk: Hash32, hpke_pk: Hash32, name: String, transcript: Hash32, bootstrap_seq: u64 },
    RecoveryChange { mode: RecoveryMode, sign_pk: Option<Hash32>, hpke_pk: Option<Hash32> },
    Fence { host: String, door: [u8; 5], home_gen: u32 },
    ResetTo { seq: u64, state_hash: Hash32, hash_version: u32 },
    Skip { targets: Vec<(u64, Hash32)> },
    RulesUpgrade { rules: u32, mode: UpgradeMode },
}
pub enum Body { Cmd(Vec<u8>), Ctl(CtlIntent), Beat(BeatInfo), Snap(SnapInfo) }
pub struct PlainEntry { pub entry_id: [u8; 16], pub at_ms: i64, pub based_on: Head, pub body: Body }

pub struct Opened { pub entry_id: [u8; 16], pub kind: Kind, pub at_ms: i64, pub based_on: Head,
                    pub payload: Vec<u8>, pub items: Vec<CtlItem>, pub beat: Option<BeatInfo>,
                    pub snap: Option<SnapInfo> }
pub struct Receipt<M> {
    pub class: Class, pub code: Option<&'static str>, pub chain: Hash32,
    pub author: Dev, pub ctr: u64, pub frame_hash: Hash32,
    pub own_dev_foreign: bool, pub based_on_unchecked: bool,
    pub opened: Option<Opened>, pub membership: M, pub keys_changed: bool, pub halt: Option<Halt>,
}
pub struct ClassifyInput<'a, M> {
    pub seq: u64, pub frame: &'a [u8], pub prev_chain: Hash32, pub membership: &'a M,
    pub chain_at: &'a dyn Fn(u64) -> Option<Hash32>, pub retained_from: u64,
    pub own_sealed: &'a dyn Fn(u64) -> Option<Hash32>,
    pub skip_targets: &'a dyn Fn(u64) -> Option<&'a [u8]>,
}
pub struct Sealed { pub frame: Vec<u8>, pub frame_hash: Hash32, pub epoch: u32, pub keys_changed: bool }
pub struct SnapshotMaterial { pub seq: u64, pub chain: Hash32, pub hash_version: u32,
                              pub state_hash: Hash32, pub board: Vec<u8>, pub membership: Vec<u8> }
pub struct SealedBlob { pub id: [u8; 16], pub bytes: Vec<u8>, pub sha256: Hash32, pub parts: u32,
                        pub inflated: u64, pub epoch: u32 }
pub struct SealerError { pub code: &'static str }

pub trait Sealer {
    type Membership: Clone;

    fn own_dev(&self) -> Dev;
    fn view(&self, m: &Self::Membership) -> MembershipView;          // §6.2
    fn membership_bytes(&self, m: &Self::Membership) -> Vec<u8>;
    fn membership_from_bytes(&self, bytes: &[u8]) -> Result<Self::Membership, SealerError>;
    fn supported_envelopes(&self) -> Vec<(u8, u8)>;

    fn chain_zero(&self) -> Hash32;
    fn chain_step(&self, prev: &Hash32, seq: u64, frame: &[u8]) -> Hash32;
    fn frame_hash(&self, frame: &[u8]) -> Hash32;
    fn frame_author(&self, frame: &[u8]) -> Result<(Dev, u64), SealerError>;  // §14.2 step 1, then dev and ctr

    fn classify(&mut self, input: &ClassifyInput<'_, Self::Membership>)
        -> Result<Receipt<Self::Membership>, SealerError>;

    fn new_entry_id(&mut self) -> Result<[u8; 16], SealerError>;
    fn seal(&mut self, m: &Self::Membership, ctr: u64, entry: &PlainEntry) -> Result<Sealed, SealerError>;
    fn open_own(&self, m: &Self::Membership, frame: &[u8]) -> Result<[u8; 16], SealerError>;
    fn discard_pending(&mut self, entry_id: &[u8; 16]) -> bool;
    fn forget_roots_below(&mut self, epoch: u32) -> bool;

    fn seal_snapshot(&mut self, m: &Self::Membership, material: &SnapshotMaterial)
        -> Result<SealedBlob, SealerError>;
    fn open_snapshot(&self, parts: &[&[u8]], expected: &SnapInfo, writer: &Dev, exact: bool)
        -> Result<SnapshotMaterial, SealerError>;

    fn sync_fingerprint(&self, head: &Head) -> String;
    fn move_code(&self, fence_seq: u64, fence_hash: &Hash32) -> String;
    fn check_move_code(&self, fence_seq: u64, fence_hash: &Hash32, typed: &str) -> Result<(), SealerError>;
    fn export_identity(&self) -> zeroize::Zeroizing<Vec<u8>>;
}
```

**Contracts.**

| Method | Contract |
|---|---|
| `own_dev` | this device's `dev` |
| `view` | the read-only view of §6.2, computed from the membership value alone |
| `membership_bytes`, `membership_from_bytes` | the canonical membership bytes of `seal.md` §13.2 and their strict parser (SEAL-MEM-001, SEAL-MEM-002) |
| `supported_envelopes` | the (`ver`, `suite`) pairs this build opens, ascending; `[(1, 1)]` in this build |
| `chain_zero`, `chain_step`, `frame_hash` | `seal.md` §15.1 and §8.2 |
| `frame_author` | the header's `dev` and `ctr` after `seal.md` §14.2 step 1, for the 409 and own-dev paths; an error when step 1 fails |
| `classify` | `seal.md` §14 over the inputs of §14.1, with `M_{s−1} = input.membership`, the sealer's own roots, its own X25519 key, its build's supported rules and, for `seq = 1`, its genesis anchor; returns `seal.md` §14.4's outcome; `keys_changed` is true when it learned a root; it never panics (SEAL-GEN-011) |
| `new_entry_id` | 16 fresh random bytes (SEAL-PLAIN-002) |
| `seal` | builds the plaintext of `seal.md` §10.1 with the entry's id, kind, `at_ms`, `based_on` and the body's payload; for `Body::Ctl` it composes the items from the intent and `m` at this moment (wraps to the current devices of `m` and its current recovery key, a fresh root for a rotation, kept as pending under the entry id, the inner signature); seals under `m`'s current epoch (SEAL-ENV-003) with counter `ctr`; returns the frame, its hash, the epoch and whether its secrets changed |
| `open_own` | opens a frame this device signed, under any root it holds, and returns its `entry_id`; an error for any other frame |
| `discard_pending` | forgets the pending root of an own rotation that was refused (SEAL-MEM-016); true when a secret changed |
| `forget_roots_below` | deletes every epoch root below `epoch` (purge, §28.2); true when a secret changed |
| `seal_snapshot`, `open_snapshot` | `seal.md` §20; `exact = false` is the rejoin mode of §19.7, which takes `inflated` as an upper bound (§36) |
| `sync_fingerprint`, `move_code`, `check_move_code` | `seal.md` §15.3 and §15.4, the check in constant time |
| `export_identity` | the bytes of the sealed identity file (`seal.md` §19.2 kind 7): keys, roots, pending roots, anchor |

- **ENG-SEALER-001** (engine) The engine MUST classify the entry at seq `s` only with `membership` equal to `M_{s−1}` as its own earlier classifications produced it, `prev_chain` equal to `C_{s−1}`, and `retained_from` equal to the lowest seq whose chain value it holds (§7.2).
- **ENG-SEALER-002** (engine) The engine MUST supply to `own_sealed` exactly the frame hashes of §11.3: the frames it sealed, by counter, and during a restored-copy reconcile the frames of §11.3 step 4.
- **ENG-SEALER-003** (engine) The engine MUST supply to `skip_targets` only frames whose hash it has checked equals the `frame_hash` the Skip item names (§17.2).
- **ENG-SEALER-004** (engine) The engine MUST persist `export_identity()` (§7.11 `WriteIdentity`) after every sealer call that reports a changed secret, before the effects §7.12 makes depend on it.
- **ENG-SEALER-005** (engine) The engine MUST NOT ask the sealer to seal an entry with counter `c` twice: once a frame for `c` exists in a record, the engine MUST reuse its bytes for every retry (SEAL-FRAME-005, SYNC.md §4.3 rule 2).

### 6.2 The membership view

```rust
pub enum DeviceStatus { Current, Revoked }
pub struct DeviceView { pub dev: Dev, pub device_no: u16, pub status: DeviceStatus, pub name: String,
                        pub key_fp: String, pub added_seq: u64, pub added_by: Dev, pub bootstrap_seq: u64,
                        pub status_seq: u64, pub status_by: Dev, pub last_ctr: u64, pub beat_ms: i64,
                        pub rules: Vec<u32>, pub envelopes: Vec<(u8, u8)>, pub hash_versions: Vec<u32> }
pub struct MembershipView {
    pub board_id: [u8; 16], pub rules: u32, pub epoch: u32, pub epoch_seq: u64, pub epoch_clock_ms: i64,
    pub clock_ms: i64, pub purge_seq: u64, pub home_gen: u32, pub home_seq: u64,
    pub home_host: String, pub home_door: [u8; 5], pub devices: Vec<DeviceView>,
    pub recovery_dev: Dev, pub recovery_fp: String,
    pub pending_recovery: Option<(Dev, String, u64, i64)>,     // proposer, new key's fingerprint, seq, start_ms
    pub pending_override: Option<(u32, Dev, u64)>,             // rules, voter, seq
    pub last_reset: Option<(u64, u64)>,                        // at_seq, target
    pub skips: Vec<(u64, Hash32)>, pub ctl_seqs: Vec<u64>,
}
```

Each field is the field of the same name of `seal.md` §13.1; `key_fp` and `recovery_fp` are `seal.md` §6.8's text. `devices` is ascending by `dev`.

### 6.3 The fake sealer

`FakeSealer` lives in the engine crate behind the feature `sim`. It is `seal.md` §5, §8 to §15 and §20 with exactly the substitutions below, and nothing else changed: every check of `seal.md` §14.2, every shape, schema constraint and transition of `seal.md` §11 to §13, the chain of §15.1, the canonical membership bytes of §13.2, the padding of §10.2 and the frame, body and plaintext layouts of §8.1, §9.1 and §10.1 hold as written. "H" is SHA-256; the ASCII literals below carry no terminator and are not context strings of `seal.md` §4.

| `seal.md` | Fake |
|---|---|
| randomness (`Entropy`) | SplitMix64 (§31.2) seeded at construction; bytes are drawn as consecutive big-endian `u64` outputs, the last one truncated |
| Ed25519 key | `sk` = 32 random bytes; `sign_pk = H("FAKE-PK" ‖ sk)`; `hpke_pk = H("FAKE-HPKE" ‖ sk)` |
| a signature over `m` by `sign_pk` | `H("FAKE-SIG" ‖ sign_pk ‖ m) ‖ H("FAKE-SIG2" ‖ sign_pk ‖ m)` (64 bytes); verification recomputes it from the registered `sign_pk` and compares the 64 bytes |
| recovery key, `board_id`, `dev_R` | a fake key pair as above; `board_id = H("FAKE-BOARD" ‖ rsign_pk ‖ salt)[0..16]`; `dev_R = H("FAKE-REC" ‖ board_id ‖ rsign_pk)[0..16]` |
| `K_entry,e`, `K_snap,e` | `H("FAKE-KEY" ‖ board_id ‖ u32 e ‖ R_e)`, `H("FAKE-SNAPKEY" ‖ board_id ‖ u32 e ‖ R_e)` |
| AEAD seal of `p` under key `k` with AD `a` | `p ‖ H("FAKE-AEAD" ‖ k ‖ a ‖ p)[0..16]`; the nonce is drawn and carried, and not used; opening recomputes and compares the tag |
| commitment `c_e` | `H("FAKE-EPOCH" ‖ board_id ‖ u32 e ‖ R_e)` |
| wrap of `R_e` to a recipient with id `i` and `hpke_pk` `p` | 96 bytes: `i[16] ‖ 32 zero bytes ‖ (R_e XOR H("FAKE-WRAP" ‖ p ‖ u32 e)) ‖ H("FAKE-WRAPTAG" ‖ R_e ‖ u32 e)[0..16]`; unwrapping XORs and checks the tag and `c_e` |
| JSON payloads of `ctl`, `beat`, `snap` | the binary encodings below, parsed strictly (exact lengths, no trailing bytes) with the constraints of `seal.md` §11 and §12 |
| inner control signature | the fake signature over `"FAKE-CTL" ‖ board_id ‖ dev ‖ u64 ctr ‖ entry_id ‖ i64 at_ms ‖ H(items bytes)` |
| snapshot body compression | none: the body of `seal.md` §20.4 is stored as it is, and `inflated` is its length |
| sync fingerprint, move code | `seal.md` §15.3 and §15.4's `x` and `mc` computed as written; the text is the decimal seq, a space and the lowercase hex of `x[0..6]` for the fingerprint, and the lowercase hex of `mc` (10 characters) for the move code, typed input compared after removing spaces and hyphens and lowercasing |

**Fake payload encodings.**

`ctl` payload: `u8 n_items` (1 to 16) ‖ items ‖ `sig[64]`. Each item is `u8 type` and its fields:

| type | Item | Fields |
|---|---|---|
| 1 | `Genesis` | `salt[16] ‖ rsign_pk[32] ‖ rhpke_pk[32] ‖ dev[16] ‖ sign_pk[32] ‖ hpke_pk[32] ‖ vb8(name) ‖ commit[32] ‖ wrap[96] ‖ wrap[96] ‖ u32 rules ‖ u32 hash_version ‖ state_hash[32]` |
| 2 | `DeviceAdded` | `dev[16] ‖ sign_pk[32] ‖ hpke_pk[32] ‖ vb8(name) ‖ transcript[32] ‖ u64 bootstrap_seq` |
| 3 | `DeviceRevoked` | `dev[16]` |
| 4 | `DeviceReinstated` | `dev[16]` |
| 5 | `EpochRotated` | `u32 epoch ‖ commit[32] ‖ u8 n ‖ n × wrap[96] ‖ u64 purge_before` (0 means absent) |
| 6 | `RecoveryKeyChange` | `u8 mode` (1 direct, 2 propose, 3 confirm, 4 cancel), then `sign_pk[32] ‖ hpke_pk[32]` unless the mode is cancel |
| 7 | `Fence` | `vb8(host) ‖ door[5] ‖ u32 home_gen` |
| 8 | `ResetTo` | `u64 seq ‖ state_hash[32] ‖ u32 hash_version` |
| 9 | `Skip` | `u64 seq ‖ frame_hash[32]` |
| 10 | `RulesUpgrade` | `u32 rules ‖ u8 mode` (1 normal, 2 override) |

`beat` payload: `u8 n ‖ n × u32` (rules) ‖ `u8 n ‖ n × (u8 ver ‖ u8 suite)` ‖ `u8 n ‖ n × u32` (hash versions) ‖ `u8 has_state` (0 or 1) ‖ if 1: `u64 S ‖ hash[32] ‖ u32 hash_version` ‖ `u8 n_acks` ‖ `n_acks × (u64 entry ‖ u8 verdict)` (1 ack, 2 mismatch).

`snap` payload (116 bytes): `u64 seq ‖ hash[32] ‖ u32 hash_version ‖ blob[16] ‖ sha256[32] ‖ u64 size ‖ u32 parts ‖ u64 inflated ‖ u32 epoch`.

The fake has a genesis helper `FakeSealer::create(seed, name) -> (FakeSealer, frame_1, board_bytes)` that draws, in this order, the salt, the recovery key, the device keys, `dev`, `R_1` and the entry id, and builds seq 1 exactly as `seal.md` §12.2 and §13.3 with the toy empty state; and a pairing helper `FakeSealer::join(seed, name, bundle: FakeBundle) -> FakeSealer` that takes a new device's keys and the roots the existing device hands over, standing in for `seal.md` §17.

- **ENG-FAKE-001** (engine) `FakeSealer` MUST implement every check, transition and layout of `seal.md` §8 to §15 and §20 that §6.3 does not substitute, and MUST produce the bytes of the vectors `engine-fake.json` (§35).
- **ENG-FAKE-002** (engine) `FakeSealer` MUST draw every random byte from its seeded SplitMix64 generator in the order its calls are made, so that equal seeds and equal call sequences give equal frames.

## 7. Replica state and persistent records

The engine defines what it persists as records with the fields below; `device.md` owns the files, their byte encodings and fsync (seal file kinds 1 to 8, plus kinds 9 and 10 requested in §36). Each record type has one owner file:

| Record | File (`seal.md` §19.2 kind) | Written |
|---|---|---|
| `LogRecord` | 1 `log.tdl` | appended per page |
| `Checkpoint` | 2 `checkpoint.tdk` | rewritten whole (§18.1) |
| base `Checkpoint` | 3 `base-<seq>.tdk` | one file per base (§18.2) |
| `HashRecord` | 4 `hashes.tdk` | appended |
| `OutboxOp` | 5 `outbox.tdo` | appended; rewritten when compacted |
| identity | 7 | rewritten whole (sealer bytes) |
| `NoticeOp` | 8 | appended; rewritten when compacted |
| `Meta` | 9 (new) | rewritten whole |
| `Marker` | 10 (new) | rewritten whole, or removed |

### 7.1 In-memory state

| Part | Content |
|---|---|
| log head | `(L, C_L)` |
| chain index | `C_s` for every seq from `retained_from` to `L` |
| class head | `K` and `M_K` |
| board head | `B`, the confirmed state at `B`, `M_B`, `C_B` |
| pending view | the state of §8.2, its unapplied records and doomed flags, `view_version` |
| outbox | the records of §7.4, in order |
| registry | §7.3 |
| meta | §7.8 |
| session | §11 |

`L ≥ K ≥ B` always. `K = L` except while a page is in progress (§12.3). `K = B` except under the crash halt (§16.2).

### 7.2 `LogRecord`

| Field | Type | Meaning |
|---|---|---|
| `seq` | u64 | the entry's seq |
| `chain` | [32] | `C_seq` as this device computed it |
| `frame` | bytes, 670 to 262,144 | the frame exactly as the relay served it |

The log holds the contiguous seqs from `log_floor` to `L`. `log_floor` is 1 for the device that created the board and `S + 1` for a device that bootstrapped from a snapshot at `S`; only retention (§7.13) and purge (§28.2) raise it. `retained_from` is `log_floor − 1` when the device holds that seq's chain value (from the snapshot, or from the record it just truncated), else `log_floor`.

### 7.3 `Checkpoint` and the registry

| Field | Type | Meaning |
|---|---|---|
| `board_seq` | u64 | `B` |
| `board_chain` | [32] | `C_B` |
| `board` | bytes | `SM::canonical_bytes` of the confirmed state |
| `membership` | bytes | `M_B` canonical bytes |
| `log_floor` | u64 | §7.2 |
| `registry` | `Registry` | below |

**`Registry`** (engine data that the log alone does not give a device that bootstrapped later):

| Field | Type | Meaning |
|---|---|---|
| `snapshots` | list of `SnapRecord`, ascending by `entry_seq`, at most 64 | the `snap` entries this device accepted, with their verdicts |
| `reports` | list of `(S, dev, hash, hash_version, report_seq)`, ascending by `(S, dev)`, at most 1,024 | state-hash reports from beats and `snap` entries for aligned seqs (§22.1) |
| `pending_verdicts` | list of `(entry_seq, Verdict)`, ascending, at most 64 | verdicts this device has not yet sent in a beat |
| `last_entry` | list of `(dev, at_ms, seq)`, ascending by `dev` | the newest accepted entry of every author |
| `restored` | `Option<(u64, u64)>` | the counter range of a restored-copy reconcile in progress (§11.3) |

**`SnapRecord`:** `entry_seq: u64`, `author: Dev`, `info: SnapInfo`, `entry_clock: i64` (`τ` after the `snap` entry), `verdicts: list of (dev, Verdict, beat_seq)` (from other devices' beats, at most 16, ascending by `dev`), `own: Option<Verdict>`.

### 7.4 Outbox records

Every entry this device authors goes through the outbox, in the order it was enqueued: commands, control entries, beats and `snap` entries alike, because they share one counter sequence (SYNC.md §4.3).

| Field | Type | Meaning |
|---|---|---|
| `entry_id` | [16] | from `Sealer::new_entry_id`, kept for life, re-seals included (SEAL-PLAIN-002) |
| `order` | u64 | the record's position: one more than the largest `order` ever used |
| `at_ms` | i64 | the composer's `wall_ms` at enqueue (§27.3); for `cmd` the compile clock |
| `based_on` | `Head` | `(B, C_B)` at enqueue (SEAL-PLAIN-003, `core.md` §19) |
| `kind` | `Kind` | |
| `body` | `Body` | for `cmd` the encoded command; for `ctl` the intent; for `beat`, `snap` their info |
| `origin_state` | bytes | for `cmd`: the opaque origin state (the core's `IdMinter`) handed in with it, persisted in the same write (`core.md` §19) |
| `via` | `Via` | `Append`, `Register { dev, sign_pk }`, `RecoveryRegister { dev_r, sign_pk }`, `Fence { host, door, home_gen }` (§11.6) |
| `hold` | bool | held while named (§9.3) |
| `ctr` | `Option<u64>` | the counter, set when sealed |
| `frame` | `Option<bytes>` | the sealed frame |
| `frame_hash` | `Option<[32]>` | `H(frame)`, or the hash of the relay's frame matched by `entry_id` (§11.4) |
| `epoch` | `Option<u32>` | the epoch it was sealed under |
| `ack` | `Option<(u64, [32])>` | `(seq, chain)` the relay acknowledged |
| `seen` | `Option<u64>` | the seq at which this device classified it |
| `op` | `Option<u64>` | the ticket of the operation that owns it (§28) |
| `reseals` | u32 | how often it was sealed again (§24.3) |

`OutboxOp` is `Put(record)` (the newest `Put` of an `entry_id` replaces earlier ones) or `Drop(entry_id)`. A record leaves the outbox (`Drop`) when its entry reaches the board head (applied, refused by `apply`, or refused by the sealer with a code that is not re-sealable), or when the naming hold cancels it.

- **ENG-STATE-001** (engine) The engine MUST keep each record in the outbox until its entry has been applied to, or finally refused at, the confirmed state, or the naming hold cancelled it, and MUST NOT drop a record because of an acknowledgement alone.

### 7.5 `HashRecord`

`{seq: S, hash_version: u32, hash: [32]}` for every aligned `S` (§18.3). When the file holds several records for one `S`, the last one in file order is the device's hash at `S`.

### 7.6 `Marker`

`{first: u64, last: u64, attempt: u64}`: the entries `first..=last` are being applied in attempt `attempt` (§15). At most one marker exists.

### 7.7 `NoticeOp`

`Add { record: Vec<u8> }` (a notice or refusal record of `core.md` §10.7, canonical bytes, with its kind) or `Dismiss { seq: u64, index: u32 }` (the `index`-th record, 0-based, whose `seq` equals `seq`, is dismissed by the user).

### 7.8 `Meta`

| Field | Type | Meaning |
|---|---|---|
| `attempt` | u64 | the current attempt (§15) |
| `last_failed` | `Option<(u64, u64)>` | `(seq, attempt)` of the latest entry that failed to apply |
| `build_id` | string | the runtime's build id at the last `Start` |
| `halt` | `Option<HaltRecord>` | §16 |
| `stops` | list of `StopRecord`, at most 16 | §21, §22 |
| `alarms` | list of `AlarmRecord`, at most 256 | §21.1 |
| `banners` | list of `Banner`, at most 256 | §14.2 |
| `relay` | `RelayTarget { host, door, home_gen }` | where this device syncs |
| `prev_session` | `Option<[16]>` | the session id of the previous process (§11.5) |
| `conflicts` | u32 | consecutive unmatched 409 conflicts (§11.4) |
| `clock` | `Option<(i64, i64)>` | the user's last clock confirmation: `(wall_ms, skew_ms)` (§27.2) |
| `last_beat_ms` | i64 | `wall_ms` of the last beat this device composed |
| `duties` | `DutyState` | §29.3 |
| `ops` | list of `OpRecord`, at most 16 | operations in progress (§28) |
| `move_tries` | `(u32, i64)` | wrong move codes in a row, and the `wall_ms` until which entry is locked (§28.3) |

### 7.9 Bases

A base is a `Checkpoint` written at an aligned seq and kept 400 days (§18.2). Bases are read on demand (`ReadBase`).

### 7.10 `Loaded`

What `Start` carries, read by the runtime from the files:

| Field | Meaning |
|---|---|
| `checkpoint` | the newest checkpoint, or absent before the first |
| `bases` | the seq and `board_chain` of every base, ascending |
| `chains` | `(seq, chain)` of every log record, ascending |
| `tail` | every log record above `checkpoint.board_seq` |
| `hashes` | every `HashRecord` |
| `outbox` | every `OutboxOp`, in file order |
| `notices` | every `NoticeOp`, in file order |
| `meta` | `Meta`, or absent before the first |
| `marker` | the marker, or absent |
| `build_id` | the running build's id |
| `session` | the 16-byte session id this process sends in its socket hello (`relay.md` §11.3) |
| `bootstrap` | for a new replica, one of `Genesis { board, frame_1 }` (the creating device, §19.8) or `Join { material, roots_ok, added_seq, relay }` (a paired device, §19.8); absent otherwise |

- **ENG-STATE-002** (engine) At `Start`, the engine MUST rebuild the confirmed state, `M_B` and `C_B` from `checkpoint` (or `bootstrap`), then classify and apply the `tail` records above `B` in seq order under the marker rules of §15 before it emits any `Append`, `Admin` or `Blob` effect.
- **ENG-STATE-003** (engine) At `Start`, the engine MUST rebuild the outbox by replaying `outbox` in file order, the newest `Put` of each `entry_id` winning and a `Drop` removing it.

### 7.11 Persist operations

| `PersistOp` | Fields | Meaning |
|---|---|---|
| `AppendLog` | `records: Vec<LogRecord>` | append, contiguous with the log head |
| `SetMarker` | `Marker` | replace the marker |
| `ClearMarker` | | remove the marker |
| `WriteCheckpoint` | `Checkpoint` | replace the checkpoint |
| `WriteBase` | `Checkpoint` | create the base at its `board_seq` |
| `DeleteBases` | `seqs: Vec<u64>` | delete those bases |
| `TruncateLogBelow` | `seq: u64` | drop every log record below `seq` |
| `AppendHashes` | `Vec<HashRecord>` | append |
| `Outbox` | `Vec<OutboxOp>` | append, in order |
| `RewriteOutbox` | `Vec<OutboxRecord>` | replace the file with one `Put` per live record, in order |
| `Notices` | `Vec<NoticeOp>` | append |
| `RewriteNotices` | `Vec<NoticeOp>` | replace the file |
| `WriteMeta` | `Meta` | replace |
| `WriteIdentity` | `Zeroizing<Vec<u8>>` | replace the identity file with the sealer's export |

A single `Persist` may carry several ops; the runtime writes them in order and reports one `Persisted`.

- **ENG-PERSIST-005** (engine) The engine MUST emit `TruncateLogBelow`, `DeleteBases` and `RewriteOutbox` only in the cases §7.13, §28.2 and §9.1 name, and never because of a relay answer (SYNC.md §4.3 rule 6, V2-21).

### 7.12 What must be durable first

| # | Write | Later effect that waits for its `Persisted` |
|---|---|---|
| D1 | `Outbox(Put)` of a newly enqueued or coalesced record, or the `Drop` of a cancelled one | `Notify(Enqueued)` or `Notify(EnqueueRefused)` for its ticket (§9.1) |
| D2 | `Outbox(Put)` carrying a record's frame, plus `WriteIdentity` when sealing changed a secret | the `Append` or `Admin` that transmits that frame |
| D3 | `AppendLog` of a page's records and its `SetMarker` | applying any of those entries to the board, committing their membership states, any `Notify` derived from them, any `Persist` of a checkpoint, base, hash, notice, outbox `Drop` or meta that reflects them |
| D4 | `WriteIdentity` after `classify` reported `keys_changed` | any `WriteCheckpoint` or `WriteBase` whose `board_seq` is at or above that entry |
| D5 | `WriteMeta` with a new `attempt` | applying any entry in that attempt (§15) |
| D6 | `WriteMeta` with `last_failed` or `halt` set | applying the failed entry again |
| D7 | `WriteBase` at a purge rotation's seq | that purge's `TruncateLogBelow`, `DeleteBases` and `forget_roots_below` (§28.2) |
| D8 | `WriteCheckpoint` with `board_seq ≥ x` | `TruncateLogBelow(y)` for any `y > x` |
| D9 | `Outbox(Put)` clearing a record's frame for a re-seal | sealing that record again |
| D10 | `WriteMeta` with an operation's new stage | the relay call of that stage (§28) |

- **ENG-PERSIST-006** (engine) The engine MUST respect every dependency of the table of §7.12.

### 7.13 Retention

- The log keeps every record whose seq is at or above the oldest kept base, and every record of the last 16,384 seqs.
- A base is deleted when its `τ` is more than 34,560,000,000 ms (400 days) of log clock below `τ_B`, except that the newest two bases are always kept.
- Hash records are kept for the seqs the log keeps.
- Notice and refusal records: at most 1,000 kept; when there are more, the oldest dismissed ones are dropped by a `RewriteNotices`, then the oldest.
- The outbox is rewritten when it holds more than `2 × live + 64` ops.

- **ENG-STATE-004** (engine) The engine MUST delete log records, bases, hashes and notices only by the rules of §7.13 and of a purge (§28.2).

## 8. Confirmed and pending state

### 8.1 The confirmed state

The confirmed state is the state machine's state after the entries up to the board head `B`, in seq order, each applied, refused or skipped as §13 says (SYNC.md §4.7). It changes only by §12.4 (applying the next entry), §17.3 (a Skip replay), §28.1 (a `ResetTo`), §19.8 (a bootstrap) and §22.3 (an adoption). `B` never decreases.

### 8.2 The pending view

The pending view is what the user interfaces show: the confirmed state with this device's unapplied `cmd` records applied on top, in outbox order.

1. Let `U` be the outbox records of kind `cmd` whose entry has not reached the board head (their `seen` is absent or above `B`), in `order`.
2. Start from a copy `P` of the confirmed state.
3. For the `i`-th record of `U` (`i` from 1), let `ctx = {seq: B + i, device_no: d, at_ms: r.at_ms, based_on: min(r.based_on.seq, B)}`, where `d` is this device's `device_no` in `M_B`. Apply `r`'s command to `P` under `catch_unwind`:
   - success: keep the change; the record is not doomed; its notices are shown with it and never recorded (ENG-SM-004);
   - a refusal: the record is **doomed** with the refusal's code; `P` is unchanged;
   - a panic: the record is doomed with `engine/trial_panic`, and `P` is rebuilt from the confirmed state by re-applying the records before it that were not doomed.
4. `P` is the pending view. `view_version`, a counter that starts at 1 at every `Start`, is raised by one.

A device whose own record has no `device_no` in `M_B` (a revoked device keeps its number; only a device that has not joined has none) shows the confirmed state alone.

- **ENG-PEND-001** (engine) The pending view MUST equal the result of the steps of §8.2 over the confirmed state and the outbox, with the provisional seqs `B + i` and the `Ctx` fields that §8.2 states.
- **ENG-PEND-002** (engine) A record whose application to the pending view is refused or panics MUST be marked doomed with that code and MUST leave the pending view as if it were absent.
- **ENG-PEND-003** (engine) The engine MUST raise `view_version` by one each time the pending view changes, and MUST start it at 1 at every `Start`.

### 8.3 Doomed records

"Doomed" is known exactly when the most recent computation of §8.2 refused the record. It is re-decided at every rebase, so a record can stop being doomed when the confirmed state changes. A doomed record is still sent: the log, not the pending view, decides its fate, and its refusal is then recorded (§13.3). The interfaces mark it "will be refused" with its code (V3-6).

- **ENG-PEND-004** (engine) The engine MUST send a doomed record like any other and MUST NOT drop it because it is doomed.

### 8.4 Rebase

The pending view is recomputed in full (a **rebase**) when:

1. a read run ends: a page arrives with `more` unset and the log head equals the page's head (SYNC.md §4.7: "rebased per batch");
2. during a long catch-up, after every 16 pages applied since the last rebase;
3. after a Skip replay, a reset, a bootstrap, an adoption, a `ResetTo` or a rules upgrade;
4. after a record leaves the outbox other than by being applied (a cancelled hold, a re-seal that changes order, a resubmit).

An enqueue does not rebase: the new record is applied on top of the current view (§9.1 step 4), with the next provisional seq.

- **ENG-PEND-005** (engine) The engine MUST rebase in each of the four cases of §8.4 and MUST NOT rebase after each page of a run of more than one page except as case 2 states.

### 8.5 The view of a halted device

A halted device (§16) shows its confirmed state frozen at `B` plus every unapplied record of its own, acknowledged or not, by the same computation (V2-6): its records stay in the outbox until they reach the board head (ENG-STATE-001), so its own sent edits do not vanish from its screen. The interfaces label the view "frozen since" the halt's `since_ms` and seq (§30).

## 9. Enqueue

### 9.1 Steps

`Enqueue { ticket, command, at_ms, origin_state, hold }` is what the facade sends after it compiled an intent against the engine's pending view (`core.md` §13) with `now = at_ms`. The facade compiles and enqueues with no other input to the engine in between, so the view it compiled against is the view the engine holds.

1. **Refusals.** Answer `Notify(EnqueueRefused { ticket, code })` and stop when: the device has not joined (`engine/not_joined`); it is revoked (`engine/revoked`, §24.4); composing is blocked by the clock (`engine/clock_unconfirmed`, §27.2); `|at_ms − wall_ms| > 60,000` (`engine/runtime_protocol`). A halt or a stop does not refuse: editing goes on locally (SYNC.md §4.8, V2-9), and a halted device holds what it queues (§16.4).
2. **Naming hold.** If the last outbox record is held, apply §9.3 first.
3. **Trial apply.** Apply the command to a copy of the pending view under `catch_unwind` with `ctx = {seq: B + |U| + 1, device_no: d, at_ms, based_on: B}`. A panic answers `EnqueueRefused { ticket, code: "engine/trial_panic" }`; a refusal answers `EnqueueRefused` with the refusal's code. Nothing is queued (SYNC.md §4.3 rule 1, V2-14).
4. **Record.** Draw `entry_id = Sealer::new_entry_id()`; build the record `{entry_id, order, at_ms, based_on: (B, C_B), kind: cmd, body: encode(command), origin_state, via: Append, hold}`; emit `Persist(Outbox[Put(record)])`; apply the command to the pending view (the trial's result) and raise `view_version`.
5. **Answer.** When that `Persisted` arrives, emit `Notify(Enqueued { ticket, entry_id, provisional_seq })`. The facade's call returns only then, so the edit is durable before the call returns (SYNC.md §4.3 rule 1, §4.1).

- **ENG-ENQ-001** (engine) The engine MUST refuse an `Enqueue` whose trial apply panics, without queuing it, with `engine/trial_panic`.
- **ENG-ENQ-002** (engine) The engine MUST refuse an `Enqueue` whose trial apply is refused, without queuing it, with the refusal's code.
- **ENG-ENQ-003** (engine) Each new record MUST receive a fresh 128-bit `entry_id` from `Sealer::new_entry_id`.
- **ENG-ENQ-004** (engine) The engine MUST emit `Notify(Enqueued)` for a ticket only after the `Persisted` of the write that holds its record and its `origin_state`.
- **ENG-ENQ-005** (engine) A record MUST carry `based_on = (B, C_B)` and the `at_ms` of its `Enqueue`, and the engine MUST NOT change either when it seals or re-seals it.
- **ENG-ENQ-006** (engine) The engine MUST refuse an `Enqueue` with the code of the first condition of §9.1 step 1 that holds.

### 9.2 Entries the engine composes

Control entries, beats and `snap` entries are enqueued by operations (§28) and duties (§29) with `at_ms = wall_ms`, `based_on = (B, C_B)` and no trial apply (they do not change the board). A control entry's items are composed when it is sealed (§10.2), from the membership of that moment.

### 9.3 The naming hold

A record may be held only when `Enqueue.hold` is true and `SM::hold(command)` is `Some`. A held record is not sealed while all of these hold: it is the last record of the outbox; it is still held; and less than 30,000 ms of `mono_ms` have passed since it was enqueued or last coalesced (V2-17). Holds end at every `Start`, on `ReleaseHold`, at the timeout, and as below.

When an `Enqueue` arrives while the last record `r` is held, the engine computes `SM::coalesce(r.command, command)`:

| Result | Effect |
|---|---|
| `Merge(m)` | trial-apply `m` in place of `r` (the view without `r`, then `m`); on success `Put` `r` with body `m` and the new `origin_state` (same `entry_id`, `at_ms` and `based_on`), keep the hold and restart its timer, and answer `Enqueued { ticket, entry_id: r.entry_id }`; on refusal answer `EnqueueRefused` and leave `r` unchanged |
| `CancelBoth` | `Drop(r)`; queue nothing; rebase; answer `Enqueued { ticket, entry_id: None }`, meaning "done, nothing left to send" |
| `Keep` | end `r`'s hold, then enqueue `command` by §9.1 |

So a Create then Forget never leaves the device, and a Create then Rename leaves as one Create (SYNC.md §4.6, `core.md` `LogCommand::with_name`).

- **ENG-HOLD-001** (engine) The engine MUST NOT seal a held record while it is the last outbox record, still held, and younger than 30,000 ms of `mono_ms` since it was enqueued or last coalesced.
- **ENG-HOLD-002** (engine) The engine MUST replace a held create by the merged command, keeping its `entry_id`, `at_ms` and `based_on`, when the next enqueued command renames the held item.
- **ENG-HOLD-003** (engine) The engine MUST drop a held create and queue nothing for the next command when that command forgets the held item.
- **ENG-HOLD-004** (engine) Any other `Enqueue`, a `ReleaseHold`, the timeout or a `Start` MUST end the hold.

### 9.4 Offline

Offline use is unlimited. When 10,000 or more records are neither acknowledged nor seen, the status carries `queue_large` (SYNC.md §4.7); nothing is refused.

- **ENG-ENQ-007** (engine) The engine MUST NOT refuse an `Enqueue` because of the outbox's size, and MUST set `queue_large` in the status while 10,000 or more records are neither acknowledged nor seen.

## 10. Send

### 10.1 When the engine sends

The engine sends only while every one of these holds: the session is `Live` (§11.1); no stop of §21 or §22 is in force; the relay target is not fenced (§28.3); the device is not revoked (§24.4); no append pause is in force (§10.4); and no `Append` or frame-carrying `Admin` is outstanding (ENG-EFFECT-003). A halted device sends only the records of a Skip operation it started (§16.4).

### 10.2 The send loop

Run at step 6 of §4.5:

1. Let `R` be the records in `order` with no `ack` and no `seen`, from the first; under a halt, only the records of a Skip operation.
2. **Admin-carried records.** If the first record of `R` has `via ≠ Append`, every earlier record is acknowledged or seen: seal it if it has no frame (step 4), then send it alone as the matching `Admin` (§11.7) once its frame is durable. Stop.
3. **Resends.** Take from the front of `R` the leading records that already have a frame, as long as the batch has at most 16 frames and at most 4,194,373 bytes (`relay.md` §9.2).
4. **New seals.** If the batch has room and the epoch gate (§24.2) is open, take the next records of `R` that have no frame, in order, stopping before a held record (§9.3) and before a record with `via ≠ Append`. For each: `ctr = next_ctr` (§10.3); `Sealer::seal(M_K, ctr, entry)`; set `ctr`, `frame`, `frame_hash`, `epoch`. Persist every newly sealed record (`Outbox[Put…]`, and `WriteIdentity` if a secret changed) in one `Persist`.
5. When that `Persisted` arrives (or at once, if the batch holds only resends), emit `Append { req, frames }`.

- **ENG-SEND-001** (engine) The engine MUST transmit the frames of one batch in `order`, at most 16 frames and at most 4,194,373 bytes per batch.
- **ENG-SEND-002** (engine) The engine MUST assign a record's counter when it seals it, never at enqueue (SYNC.md §4.3 rule 2, V2-17).
- **ENG-SEND-003** (engine) The engine MUST make a newly sealed frame durable before it transmits it (D2), and MUST transmit the identical bytes on every retry.
- **ENG-SEND-004** (engine) The engine MUST seal records in `order`, so that the counters of its frames rise with `order`, and MUST NOT seal a record while an earlier record is unsealed.
- **ENG-SEND-005** (engine) The engine MUST send a record with `via ≠ Append` alone, in its admin operation, and only when every earlier record is acknowledged or seen.

### 10.3 Counters

`next_ctr = 1 + max(a, h, w)`, where `a` is this device's `last_ctr` in `M_K`, `h` the largest `ctr` of a record that has a frame, and `w` the relay's `last_ctr` for this device once a reconcile has verified it (§11.3). The first frame of a device has counter 1.

- **ENG-SEND-006** (engine) The engine MUST seal each new frame with `next_ctr` as §10.3 defines it.

### 10.4 Append results

For each result, in frame order (`relay.md` §9.5, §9.6):

| Result | Action |
|---|---|
| `new`, `dup` `(seq, chain)` | `ack = (seq, chain)`; `Put` the record in the next `Persist` (no effect waits for it); if `seq ≤ L`, run the echo check now (§11.5) |
| `conflict` | §11.4 |
| `gap (last_ctr, last_seq)` | §11.4 |
| refused `429 relay/budget` | pause appends until the next 00:00 UTC of `wall_ms`; `status.paused_until`; the frame stays sealed |
| refused `403 relay/revoked` | §24.4 |
| refused `400 relay/bad_signature`, `400 relay/bad_frame`, `400 relay/wrong_board`, `403 relay/not_author`, `400 relay/unknown_author` | stop with the local fault `engine/own_frame_refused` (§21.4), naming the counter and the code |
| `skipped` | nothing; the frame is resent by the next loop |

- **ENG-RESULT-001** (engine) The engine MUST record a `new` or `dup` result as the record's acknowledgement and MUST NOT treat it as proof that the entry is valid (`relay.md` §9.5).
- **ENG-RESULT-002** (engine) On `429 relay/budget`, the engine MUST send no `Append` until the next 00:00 UTC of its local clock and MUST keep the refused frame for resending.
- **ENG-RESULT-003** (engine) A refusal of one of its own frames for its format, board, signature or author MUST stop the engine with `engine/own_frame_refused` and MUST NOT lead it to change or drop the frame.

### 10.5 Retries

An `Append` whose answer never arrives (a `Disconnected` first) is lost from the engine's view; its frames stay sealed and are resent, identical, after the next reconcile (rule 2). The relay's `dup` result acknowledges a resend (`relay.md` §9.4 T3a), so a frame transmitted before a crash is never applied twice (V2-3 (a)).

- **ENG-SEND-007** (engine) After a `Disconnected`, the engine MUST treat every outstanding request as unanswered and MUST resend unacknowledged frames only after the next reconcile (§11.3) has completed.

## 11. Sessions, reconcile and echoes

### 11.1 Session states

```rust
pub struct Welcome { pub head: Head, pub last_ctr: Option<u64>, pub last_seq: Option<u64>,
                     pub relay_now: i64, pub floor: u64, pub purge_floor: u64, pub fenced: bool,
                     pub fence: Option<(String, [u8; 5], u32)>, pub appends_left: Option<u32>, pub push: bool }
```

| State | Event | Guard | Effects | Next |
|---|---|---|---|---|
| `Offline` | any | a relay target is set, the device is not stopped by `identity_clone` and not revoked | `Connect { relay }` | `Connecting` |
| `Connecting` | `Connected { push }` | | `Hello` when `push` is false (over a socket the welcome follows by itself) | `Connecting` |
| `Connecting` | `Welcome` | the checks of §11.2 pass | the reconcile of §11.3; `Read` when the head is above `L` | `Reconciling` |
| `Reconciling` | `Page` | §12 | as §12 | `Reconciling`, or `Live` when `L` reaches the latest known relay head and the reconcile is complete |
| `Live` | `Page` (push or read) | §12 | as §12 | `Live` |
| `Live` | `Tick` | `push` is false and 30,000 ms of `mono_ms` passed since the last `Hello` while visible, as `device.md` sets the poll rate | `Hello` | `Live` |
| any | `Welcome` | a check of §11.2 fails | the stop of §21 | unchanged |
| any | `Disconnected` | | none; outstanding requests are unanswered (ENG-SEND-007) | `Offline` |

The runtime applies its own backoff before acting on a `Connect` (`device.md`, full jitter from 1 to 60 s, SYNC.md §6.2).

- **ENG-SESS-001** (engine) The engine MUST follow the session table of §11.1, and MUST NOT seal or send a frame in any state but `Live`.

### 11.2 Checks on a welcome

In order:

1. `relay_now` updates the clock measurement (§27.1).
2. If `head.seq < L`: evidence `relay_behind` (the relay holds less than this device's log, V2-9). If `head.seq ≤ L` and the chain this device holds at `head.seq` differs from `head.chain`: evidence `fork`.
3. If any record has `ack.seq > head.seq`: evidence `relay_behind`.
4. If `fenced`: §28.3.
5. The reconcile of §11.3 with `last_ctr` and `last_seq`.

- **ENG-SESS-002** (engine) The engine MUST raise the evidence of §11.2 step 2 or step 3 when a welcome's head is below the log head or below an acknowledged seq, or names a chain value at a seq the device holds that differs from its own.

### 11.3 Reconcile on hello

SYNC.md §4.3 rule 3, V1-16, V2-3. Let `a` be this device's `last_ctr` in `M_K`, `h` the largest counter of a record with a frame, `r = last_ctr` and `q = last_seq` from the welcome.

1. `r < a`: the relay lacks counters this device has classified: evidence `relay_behind`.
2. `a ≤ r ≤ h`: normal. The frames `a + 1` to `r` are stored; the engine reads up to at least `q` before it seals anything new.
3. `r > h`: frames under this device's `dev` exist that this folder never sealed: a restored copy (a backup, a VM snapshot, Time Machine) or another holder of the key. The engine sets `registry.restored = (h + 1, r)` (persisted in the next `WriteMeta`), seals nothing until the log reaches `q`, and during that catch-up:
   - supplies to `own_sealed`, for every counter in `h + 1..=r`, the hash of the frame being classified, so those frames are classified as its own;
   - for every such accepted frame, looks up its `entry_id` among the records: a match is the same entry sent by the earlier copy, and is treated as this device's own entry from then on (seen, then applied and dropped);
   - every record that holds a frame at a counter `≤ r` whose hash differs from the relay's frame at that counter has its frame cleared for re-sealing (D9);
   - when the log reaches `q`: if `a` (now updated) is below `r`, the relay claimed counters it cannot produce: evidence `withheld`; otherwise `restored` is cleared, the status carries the notice `restored_copy { entries }`, and `next_ctr` uses `w = r`.
4. Frames under this device's `dev` at counters above every value covered by steps 2 and 3 that the device did not seal are classified with `own_dev_foreign` (`seal.md` §14.7) and raise §21's `foreign_signer`.

The identity minter for commands (`core.md` §7.2) is raised by the runtime after this catch-up, from the marks the confirmed state now holds (`core.md` §19 requires it of `device.md`).

- **ENG-RECON-001** (engine) On a welcome whose `last_ctr` exceeds every counter it sealed, the engine MUST read the log through `last_seq` before sealing any new frame.
- **ENG-RECON-002** (engine) During a restored-copy reconcile the engine MUST drop, as already sent, every record whose `entry_id` is sequenced and accepted at a counter in the range of §11.3 step 3, and MUST NOT send it again.
- **ENG-RECON-003** (engine) The engine MUST clear for re-sealing every record whose frame sits at a counter the relay holds with a different frame.
- **ENG-RECON-004** (engine) The engine MUST accept a relay `last_ctr` above its own only after it has classified this device's own frames up to that counter, and MUST raise `withheld` when the relay cannot produce them (V1-16).

### 11.4 Conflicts and gaps

SYNC.md §4.3 rule 4. A `conflict` result for the record `x` sealed at counter `c`:

| Case | Action |
|---|---|
| `seq ≠ 0` and `Sealer::open_own(M_K, frame)` returns an `entry_id` that is `x`'s | the relay holds `x` as sealed by an earlier copy: `x.ack = (seq, chain at seq once known)`, `x.frame_hash = H(relay frame)`; `conflicts = 0` |
| `seq ≠ 0` and the `entry_id` is another record `y`'s | `y` is acknowledged at `seq` as above; `x`'s frame is cleared for re-sealing; `conflicts = 0` |
| `seq ≠ 0` and the `entry_id` is no record's | "restored from a copy": alarm `restored_copy` (no stop); `x`'s frame is cleared for re-sealing; `conflicts += 1` |
| `seq ≠ 0` and `open_own` fails | the relay presented a frame this device did not sign: alarm `relay_protocol` naming the counter; `conflicts += 1`; `x` is cleared for re-sealing |
| `seq = 0` | the relay no longer holds that counter's frame (compacted, purged or before its base): `x` is cleared for re-sealing; `conflicts += 1` |

After any conflict the engine re-runs the reconcile (a new `Hello`) before it seals again. When `conflicts` reaches 3, the engine stops with `identity_clone` (§21.1) instead of re-sealing (SYNC.md §4.3 rule 4: "stop after three repeats"); `conflicts` returns to 0 at the next `new` or `dup` result.

A `gap (last_ctr r, last_seq q)` result: if any record holds an acknowledgement for a counter above `r`, evidence `relay_behind` (a counter gap the device holds acks for, SYNC.md §4.8); otherwise the engine resends, identical, from the record with counter `r + 1`.

- **ENG-RECON-005** (engine) On a `conflict` whose frame opens as one of its own records, the engine MUST treat that record as acknowledged at the conflict's seq and MUST NOT re-seal it.
- **ENG-RECON-006** (engine) On a `conflict` that matches no record, the engine MUST re-seal the conflicting record under a new counter after a new reconcile, and MUST stop with `identity_clone` after three such conflicts in a row.
- **ENG-RECON-007** (engine) On a `gap` result, the engine MUST raise `relay_behind` when it holds an acknowledgement for a counter above the relay's `last_ctr`, and otherwise MUST resend its frames from the counter after the relay's `last_ctr`.

### 11.5 Echo check

SYNC.md §4.3 rule 5. For every record with `ack = (s, c)`: when the log holds seq `s`, the frame there must have the record's `frame_hash` and `C_s` must equal `c`. A difference is evidence `unechoed` (V2-10). A relay head below `s` in any welcome or page is evidence `relay_behind` (§11.2).

- **ENG-ECHO-001** (engine) The engine MUST compare every acknowledged record with the frame and chain value at its acknowledged seq as soon as the log holds that seq, and MUST raise `unechoed` on any difference.

### 11.6 One identity per folder, second and displaced

The folder lock and the refusal of cloud-synced folders are `device.md`'s (SYNC.md §4.3 rule 6). The engine handles what the relay reports (`relay.md` §11.3):

| Event | Action |
|---|---|
| `Second { session }` with `session = meta.prev_session` | the previous process of this device was still connected (a restart after a crash): ignored, once; `prev_session` is cleared |
| `Second` otherwise | stop with `identity_clone`: another process with this identity is running |
| `Displaced { session }` | stop with `identity_clone`; no `Connect` until the user chooses `UseThisCopy` (§30.4) |

At `Start` the runtime passes its new session id in `Loaded`; the engine stores the previous one as `prev_session` in the next `WriteMeta`.

- **ENG-SESS-003** (engine) The engine MUST stop with `identity_clone` on a `Displaced`, and on a `Second` whose session is not the previous process's.
- **ENG-SESS-004** (engine) After `identity_clone`, the engine MUST NOT emit `Connect` until the user chooses `UseThisCopy`.

### 11.7 Admin requests

```rust
pub enum AdminRequest {
    Register { frame: Vec<u8>, dev: Dev, sign_pk: Hash32 },                  // relay.md §12.3, frame mode
    RegisterSeq { seq: u64, dev: Dev, sign_pk: Hash32, frame_hash: Hash32 }, // seq mode, after a restore tail
    RecoveryRegister { frame: Vec<u8>, dev_r: Dev, sign_pk: Hash32 },        // §12.12, frame mode
    RecoveryRegisterSeq { seq: u64, dev_r: Dev, sign_pk: Hash32, frame_hash: Hash32 },
    Revoke { dev: Dev }, Reinstate { dev: Dev },                              // §12.4, §12.5
    Fence { host: String, door: [u8; 5], home_gen: u32, frame: Option<Vec<u8>> }, // §12.6
    Seed { home_gen: u32, first_seq: u64, chain_before: Hash32, last_seq: u64, chain_last: Hash32,
           blob_id: [u8; 16], devices: Vec<SeedDevice> },                   // §12.7, on the new relay
    SeedTail { first: u64, frames: Vec<Vec<u8>> },
    Compact { below_seq: u64, snap_entry_seq: u64, keep: Vec<u64> },         // §12.8
    RestoreTail { first_seq: u64, chain_before: Hash32, frames: Vec<Vec<u8>> }, // §12.9
    Purge { before_seq: u64, epoch: u32 },                                    // §12.10
}
pub struct SeedDevice { pub dev: Dev, pub sign_pk: Hash32, pub role: u8, pub status: u8, pub last_ctr: u64 }
pub struct AdminOk { pub seq: Option<u64>, pub chain: Option<Hash32>, pub token: bool, pub deleted: Option<u64>,
                     pub floor: Option<u64>, pub more: Option<bool>, pub purge_floor: Option<u64>,
                     pub done: Option<u64>, pub head: Option<Head>, pub door: Option<[u8; 5]> }
pub struct RelayRefusal { pub status: u16, pub code: String, pub until: Option<i64>,
                          pub head: Option<Head>, pub index: Option<u32> }
```

`Seed` and `SeedTail` go to the new relay of a move (§28.3), every other request to the current target. A `Register`, `RecoveryRegister` or `Fence` with a frame acknowledges that frame on success (`seq`, `chain`), and maps a `409 relay/conflict` or `409 relay/gap` to §11.4. The token a `Register` returns is the new device's and goes to the pairing machine (§28.4), never to the engine.

### 11.8 No relay answer deletes anything

- **ENG-SESS-005** (engine) No relay answer, refusal, close, silence or absence MUST lead the engine to emit `TruncateLogBelow`, `DeleteBases`, an outbox `Drop` of an unapplied record, `forget_roots_below`, or any write that removes a key (SYNC.md §4.3 rule 6, V2-21).

<!-- CONTINUE -->
