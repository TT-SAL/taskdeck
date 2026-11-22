# TaskDeck sync spec: engine

*The replica engine: confirmed and pending state, the outbox, send and reconcile, receipt checks mapped to actions, refusals and halts, poison pills, Skip, checkpoints, bases and state hashes, snapshots and bootstrap, compaction, integrity evidence and the stop, determinism mismatch, beats, epochs and re-sealing, rules upgrades, timed windows, the clock-skew guard, the operations, duties with no always-on device, what the user interfaces are told, and the deterministic simulator. Owning stage: `engine`. Conventions, notation and the glossary are in [`README.md`](README.md); this spec does not restate them.*

## 1. Scope

This spec fixes everything the replica engine (`crates/taskdeck-sync/`) decides: which events it consumes, which effects it emits and in what order, what it persists and when, how it turns the receipt classes of `seal.md` §14 into actions, and every duty a device takes when no device is always on. It realises:

- SYNC.md §4.3 (client rules 1 to 6), §4.5 (trial apply, rules upgrades, the halted view, the clock), §4.6 (Skip, the naming hold, the persistent refusal and notice lists), §4.7 (the view, catch-up, snapshots, compaction duty, bootstrap, purge), §4.8 (integrity checks, evidence, stop and show, restore tail, determinism mismatch), §4.2 (what the outcomes and the three halts mean for a replica), §5.3 (the engine's part of pairing: the bundle snapshot and the `DeviceAdded` entry), §5.4 (revocation, rotation, races, re-sealing, a returning revoked device, reset trust), §5.5 (the 72-hour recovery-key change window), §6.4 and §6.6 (restore to a point), §6.7 (move board, the move code, following a move), §9 (bootstrap from the genesis);
- the verifier findings V1-1, V1-10, V1-11, V1-16; V2-3, V2-5 (the `based_on` the engine carries), V2-6, V2-7, V2-8, V2-9, V2-10, V2-13 (purge), V2-14, V2-15, V2-16 (the persistent lists), V2-17, V2-20 as modified, V2-21; V3-2, V3-6 (the data behind the phone's sync line, refusals and marks), V3-14; the fifteen V2 scenarios S1 to S15 (§32);
- SYNC_PLAN.md §2 (no hub, no always-on device, limited administration from the phone), §3.4 (security rules), §6.1 item 3, §6.5 (the `engine` stage, its simulator and its gate), §6.7 (what `device` wires in), §6.9 and §6.10 (what the two apps show);
- the rulings S5 to S8, S10, S13 to S15, S24, S25 and S27 of `docs/sync/status/foundation.md`, and decision D6 (the traits live in this crate).

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
- **ENG-GEN-012** (engine) Every call of a state-machine or sealer method that can run code derived from replicated data (`apply`, `trial_apply`, `parse`, `load`, `canonical_bytes`, `upgrade`, `reset`, `classify`, `open_snapshot`) MUST run inside `std::panic::catch_unwind`, and a caught panic MUST be handled as §15 states for `apply` and as a local fault (`engine/local_fault`, §21.4) for every other method.
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
| `LocalRead` | `req: u64, result: LocalReadResult` (records, a base, or `Missing`, §37) | a `ReadLog` or `ReadBase` effect completed |
| `Enqueue` | `ticket: u64, command: C, at_ms: i64, origin_state: Vec<u8>, hold: bool` | the facade compiled an intent with `now = at_ms` (§9) |
| `ReleaseHold` | | the user finished naming a held create (§9.3) |
| `Op` | `ticket: u64, op: Operation` | the user started an operation (§28) |
| `PairRegister` | `ticket: u64, device: NewDevice` | the existing-device pairing machine emitted `NeedSnapshotAndRegister` (`seal.md` §17.10) |
| `Resubmit` | `ticket: u64, commands: Vec<C>, from_device_no: u16, origin_state: Vec<u8>` | a re-paired identity imports an old identity's unsent commands, with its own current origin state (§24.5) |
| `User` | `action: UserAction` (§30.4) | the user acknowledged, confirmed or chose something |
| `Connected` | `push: bool` | a session to the relay is open (socket, or HTTPS reachability) |
| `MissedRoots` | `first_epoch: u32, wraps: Vec<[u8; 96]>` | a `login` answer carried `wraps` (`relay.md` §10.3; ruling S24): sent right after that login, before any other input; accepted in every session state and changes none (§24.4) |
| `Welcome` | `Welcome` (§11.1) | a socket welcome, or a `hello` response, arrived |
| `Page` | `req: Option<u64>, page: Page` (§12.1) | a read answer (`req` set) or a push (`req` absent) arrived |
| `AppendResults` | `req: u64, results: Vec<AppendResult>` | an append answer arrived (`relay.md` §9.6) |
| `AdminResult` | `req: u64, result: Result<AdminOk, RelayRefusal>` | an admin operation answered |
| `BlobResult` | `req: u64, result: Result<BlobOk, RelayRefusal>` | a blob upload, download or listing finished |
| `RelayRefused` | `req: Option<u64>, refusal: RelayRefusal` | a whole request was refused (`relay.md` §4.2), or the relay answered `fenced`, `revoked` or a budget; a socket `state` message (fenced, `relay.md` §11.3, §11.4) arrives as `RelayRefused { req: None, 403 relay/fenced }` (`device.md` §10.5) |
| `Second` | `session: [u8; 16]` | the relay sent `second` (`relay.md` §11.3) |
| `Displaced` | `session: [u8; 16]` | the relay sent `displaced` |
| `Disconnected` | `unreachable: bool, paused_until_ms: Option<i64>` | the session ended; `unreachable` per `relay.md` §14.5; `paused_until_ms` for a quota pause |

- **ENG-MODEL-003** (engine) The engine MUST use `wall_ms` only where §27 and the clauses that name it allow (composing `at_ms`, offering actions, the skew guard, local day boundaries, duty waits), and `mono_ms` only for timeouts and cadences of §34.
- **ENG-MODEL-004** (engine) The engine MUST treat an input whose `mono_ms` is lower than the previous input's within one `Start` as `engine/runtime_protocol` and MUST ignore it.

### 4.3 Effects

| Effect | Fields | What the runtime does |
|---|---|---|
| `Persist` | `id: u64, ops: Vec<PersistOp>` (§7.11) | performs the writes, in order, in the serial order of §4.4, then sends `Persisted{id}` |
| `ReadLog` | `req: u64, from: u64, to: u64` | reads the durable log records `from..=to` and sends `LocalRead`; `Missing` when it cannot read every one of them |
| `ReadBase` | `req: u64, seq: u64` | reads the base checkpoint at `seq` and sends `LocalRead`; `Missing` when it cannot read that base |
| `Connect` | `relay: RelayTarget` | logs in if it has no valid token (`device.md`), opens a socket if the relay pushes, else uses HTTPS; sends `Connected`, then `Welcome` |
| `Disconnect` | | closes the session; sends nothing |
| `Hello` | | sends `hello` over HTTPS (also while a socket is open; `relay.md` §10.4) and sends `Welcome` with its fields |
| `Read` | `req: u64, after: u64, limit: u16` | sends a read (socket message or `GET …/log`) and sends `Page{req}` |
| `Append` | `req: u64, frames: Vec<Vec<u8>>` | sends one batch of `relay.md` §9.2 and sends `AppendResults{req}` |
| `Admin` | `req: u64, op: AdminRequest` (§11.7) | fetches an admin nonce, has the sealer sign (`seal.md` §16.2), sends the request, sends `AdminResult` |
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
    pub code: &'static str,          // e.g. "core/gone", "engine/toy_fail"
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
    fn reset(&self, target: &Self::State, current: &Self::State) -> Result<Self::State, SmError>;

    fn hold(&self, command: &Self::Command) -> Option<u64>;
    fn coalesce(&self, held: &Self::Command, next: &Self::Command) -> Coalesce<Self::Command>;
    fn rebind(&self, commands: &[Self::Command], from_device_no: u16, to_device_no: u16,
              view: &Self::State, origin_state: &[u8]) -> Result<(Vec<Self::Command>, Vec<u8>), SmError>;

    fn notice_concerns(&self, notice: &Self::Notice, device_no: u16) -> bool;
    fn notice_bytes(&self, notice: &Self::Notice) -> Vec<u8>;
    fn refusal_bytes(&self, seq: u64, author: u16, refusal: &SmRefusal) -> Vec<u8>;
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
| `reset` | the state a `ResetTo` installs (§28.1), `core.md` §11.3: `target`'s data with the id high-water marks of both kept (elementwise maximum), any revision counter raised one above both, and the rules version of `current`; an error only for a rules version this build lacks, which is the local fault `engine/local_fault` |
| `hold` | `Some(id)` when the command creates the item `id` and may be held while it is named (V2-17); else `None` |
| `coalesce` | for a held create and the next command: `Merge(c)` when `next` renames the held item (`c` is the create carrying the new name), `CancelBoth` when `next` forgets it, `Keep` otherwise |
| `rebind` | `core.md` §7.5: rewrites commands minted by `from_device_no` for `to_device_no`; every id and sid the commands mint with high part `from_device_no` is minted anew from `origin_state`, the new identity's current origin state, against `view`, its pending view, and every later reference to it is rewritten; returns the commands and the origin state after those mints (§24.5) |
| `notice_concerns` | whether a notice is to be kept by the device with this `device_no` (`core.md` `Notice::concerns`) |
| `notice_bytes` | the canonical bytes of a notice record (`core.md` §10.7) |
| `refusal_bytes` | the canonical bytes of the refusal record of `core.md` §10.7 (`RefusalRecord::to_canonical`) with this `seq` and `author` and the refusal's `code`, `op`, `target`, `base` and `current` |

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
| marks | 10 × `n_marks` | `u16 device_no ‖ u64 n`, strictly ascending by `device_no`, `device_no` from 1 to 8,191, `n` from 1 to 2^40 − 1 |
| **total** | **16 + 19k + 12i + 10m** | no trailing bytes |

`load` refuses with `engine/toy_format` any input that violates this table (a mark whose `device_no` is outside 1 to 8,191 or whose `n` is outside 1 to 2^40 − 1 included), an item id whose high part `h = id div 2^40` is 0 or above 8,191, whose low part `n = id mod 2^40` is 0, or whose `n` exceeds the mark of `h`. The empty state is `rules = 1` and no keys, items or marks (16 bytes).

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

`parse` refuses with `engine/toy_parse` an unknown op or a payload whose length is not the op's width; it accepts op 9 under any rules (the rules check is `apply`'s). `op_name` is the name in lowercase; `target` is the key (as `u64`) or the id, and none for `Fail` and `Panic`.

**`apply`**, in this order, stopping at the first failure:

1. `ctx.device_no` from 1 to 8,191, `ctx.seq ≥ 2`, `ctx.based_on < ctx.seq`; else `engine/toy_bad_ctx`.
2. The op is allowed by `rules` (op 9 only under 2); else `engine/toy_rules`.
3. Per op:
   - `Set`: if the key exists with `stamp_seq > ctx.based_on`, `stamp_dev ≠ ctx.device_no` and `stamp_dev ≠ 0`, one notice `{seq: ctx.seq, author: ctx.device_no, other: stamp_dev, other_seq: stamp_seq, key}`; then `value` is set and the stamp becomes `(ctx.seq, ctx.device_no)`.
   - `Add` and `Mul`: the key must exist (else `engine/toy_gone`, target the key); the checked sum or product must not overflow (else `engine/toy_overflow`); the value and stamp are set as for `Set`, without a notice.
   - `Del`: an absent key is a benign repeat; else the key is removed.
   - `Create`: `h = id div 2^40` must equal `ctx.device_no` and `n = id mod 2^40` must be from 1 to 2^40 − 1 and above `marks[h]` (0 when absent) (else `engine/toy_bad_id`); fewer than 10,000 items (else `engine/toy_cap`); then the item is inserted and `marks[h] = n`.
   - `Name`: the item must exist (else `engine/toy_gone`); its name is set.
   - `Forget`: an absent item is a benign repeat; else it is removed.
   - `Fail`: refused with `engine/toy_fail`.
   - `Panic`: calls `panic!` with a constant message.

Every refusal `apply` returns, whatever its code and step, carries `op = op_name(command)` and
`target = target(command)`, with `base` and `current` absent.

**Other methods.** `supported_rules` is `[1]`, or `[1, 2]` when the toy is constructed with `rules2 = true`. `hash_versions` is `[1]`. `upgrade(v)`: `v > rules` and `v` supported, else `engine/toy_unknown_rules`; then `rules = v`. `reset(target, current)` is `Ok` of `target` with `rules = current.rules` and every mark the maximum of the two; it never fails. `hold` is `Some(id)` for `Create`. `coalesce(Create{id, name}, next)` is `Merge(Create{id, name: m})` for `Name{id, name: m}` with the same `id`, `CancelBoth` for `Forget{id}` with the same `id`, else `Keep`. The origin state is the toy minter, `u64 next_n` (8 bytes, big-endian). `rebind` maps, in order, every `Create` whose id has high part `from_device_no` to `to_device_no × 2^40 + n`, with `n = max(next_n, m + 1)` and `m` the mark of `to_device_no` in `view` (0 when absent), then sets `next_n = n + 1`; it rewrites `Name` and `Forget` ids by that mapping and returns the commands with the final `next_n`. `rebind` returns `engine/toy_format` when `origin_state` is not exactly 8 bytes, and `engine/toy_bad_id` when a mint's `n` would exceed 2^40 − 1, and in either case no commands. A notice's bytes are `u64 seq ‖ u16 author ‖ u16 other ‖ u64 other_seq ‖ u8 key` (21 bytes); it concerns `author` and `other`. A refusal's bytes (`refusal_bytes`) are `u64 seq ‖ u16 author ‖ vb8(code) ‖ vb8(op) ‖ u8 has_target ‖ u64 target`, with `op` empty when absent, and `has_target = 1` with the target, or `has_target = 0` with eight zero bytes.

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
    Genesis { hash_version: u32, state_hash: Hash32 },
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
    fn take_missed_wraps(&mut self, m: &Self::Membership, first_epoch: u32, wraps: &[[u8; 96]])
        -> Result<bool, SealerError>;                                // §24.4, ruling S24

    fn seal_snapshot(&mut self, m: &Self::Membership, material: &SnapshotMaterial)
        -> Result<SealedBlob, SealerError>;
    fn open_snapshot(&self, parts: &[&[u8]], expected: &SnapInfo, writer: &Dev, exact: bool)
        -> Result<(SnapshotMaterial, u32, u64), SealerError>;   // (material, header epoch, inflated length)

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
| `classify` | `seal.md` §14 over the inputs of §14.1, with `M_{s−1} = input.membership`, the sealer's own roots, its own X25519 key, its build's supported rules and, for `seq = 1`, its genesis anchor; returns `seal.md` §14.4's outcome; while it holds a missed wrap of epoch `n` (`take_missed_wraps`), an accepted `EpochRotated{n}` that does not wrap to this device has that wrap opened against the rotation's `commit` (`seal.md` §7) as SEAL-RCPT-007 opens an own wrap, returning the commitment halt when it does not open or does not match; when it accepts an `EpochRotated{n}` in an entry of this device whose `entry_id` holds a pending root, it opens its own wrap as every recipient does (SEAL-RCPT-007), keeps the opened root as the root of epoch `n`, and removes that pending root in the same call; `keys_changed` is true when it learned a root or removed a pending one; it never panics (SEAL-GEN-011) |
| `new_entry_id` | 16 fresh random bytes (SEAL-PLAIN-002) |
| `seal` | builds the plaintext of `seal.md` §10.1 with the entry's id, kind, `at_ms`, `based_on` and the body's payload; for `Body::Ctl` it composes the items from the intent and `m` at this moment; for a rotation it draws a fresh root, kept as pending under the entry id, computes its commitment, and wraps the root to each current device and to the current recovery identity of the working copy `W` of `seal.md` §13.6, that is `m` as the entry's earlier items leave it (a device the entry revokes gets no wrap, one it reinstates gets one, and after a `RecoveryKeyChange` the new recovery key gets it), once each, ascending by recipient id; then the inner signature; seals under `m`'s current epoch (SEAL-ENV-003) with counter `ctr`; draws its random values in this order: the root (`EpochRoot::generate`), then the wraps in ascending recipient-id order (`wrap_root`, 32 bytes each, `seal.md` SEAL-WRAP-004), then the frame's nonce (`seal_frame`, SEAL-GEN-012); returns the frame, its hash, the epoch and whether its secrets changed |
| `open_own` | opens a frame this device signed, under any root it holds, and returns its `entry_id`; an error for any other frame |
| `discard_pending` | forgets the pending root of an own rotation that was refused (SEAL-MEM-016); true when a secret changed |
| `forget_roots_below` | deletes every epoch root below `epoch` (purge, §28.2); true when a secret changed |
| `take_missed_wraps` | ruling S24 (§24.4): keeps in memory the wraps of the roots of epochs `first_epoch` to `first_epoch + wraps.len() − 1` that a `login` answer carried, replacing any it holds, and opens at once, by `seal.md` §7 (SEAL-WRAP-002, SEAL-KEY-013), the wrap of each of those epochs whose root it lacks and whose commitment `m` records; `classify` opens the others as it accepts the rotations that set their epochs; true when it learned a root (a changed secret, ENG-SEALER-004); `seal/commitment` when such a wrap does not open or its root does not match the commitment (SEAL-WRAP-003); it never persists the wraps; the fake sealer opens them with its wrap layout (§6.3) |
| `seal_snapshot`, `open_snapshot` | `seal.md` §20. `seal_snapshot` seals under `m`'s current epoch and puts into the body the sealer's own control history up to `material.seq` (`seal.md` §20.4, ruling S27); the engine never supplies or reads it. It refuses a body over 67,108,864 bytes, or one whose sealed bytes would exceed 67,108,864, with `seal/snapshot_large` (`seal.md` SEAL-BLOB-009): §19.4 then uploads and announces nothing, §28.4 reports the pairing failed, and any other caller fails its operation with that code (`OpDone`, §28). `open_snapshot` checks the body's history (`seal.md` §20.5 step 7), keeps it for the runtime (`device.md` §15.7), and returns the material, the sealed header's epoch and the inflated length; since it takes `&self`, a sealer keeps that history behind shared access (interior mutability), and the kept history is what its next `seal_snapshot` and `export_identity` see. With `exact = true` it is `seal.md` §20.5 with every value of `expected`, and the returned epoch and length equal `expected`'s; with `exact = false` it is the declared mode of `seal.md` §20.5 (§19.7) and reads only `expected.seq`, `blob`, `sha256`, `size` and `parts` |
| `sync_fingerprint`, `move_code`, `check_move_code` | `seal.md` §15.3 and §15.4, the check in constant time |
| `export_identity` | the bytes of the sealed identity file (`seal.md` §19.2 kind 7): keys, roots, pending roots, anchor |

- **ENG-SEALER-001** (engine) The engine MUST classify the entry at seq `s` only with `membership` equal to `M_{s−1}` as its own earlier classifications produced it, `prev_chain` equal to `C_{s−1}`, and `retained_from` equal to the lowest seq whose chain value it holds (§7.2).
- **ENG-SEALER-002** (engine) The engine MUST supply to `own_sealed` exactly the frame hashes of §11.3: the frames it sealed, by counter, and during a restored-copy reconcile the frames of §11.3 step 4.
- **ENG-SEALER-003** (engine) The engine MUST supply to `skip_targets` only frames whose hash it has checked equals the `frame_hash` the Skip item names (§17.2).
- **ENG-SEALER-004** (engine) The engine MUST persist `export_identity()` (§7.11 `WriteIdentity`) after every sealer call that reports a changed secret, before the effects §7.12 makes depend on it.
- **ENG-SEALER-005** (engine) The engine MUST NOT ask the sealer to seal an entry with counter `c` twice: once a frame for `c` exists in a record, the engine MUST reuse its bytes for every retry (SEAL-FRAME-005, SYNC.md §4.3 rule 2).
- **ENG-SEALER-006** (engine) When it classifies the entry at seq `s`, the engine MUST supply a `chain_at` that returns `C_b` for every `b` from `retained_from` to `s − 1`, including the values computed earlier in the same dry run (§12.3 step 6), and `None` for every other `b`.
- **ENG-SEALER-007** (device) When it composes an `EpochRotated`, the sealer adapter MUST wrap the new root to exactly the current devices and the current recovery identity of the working copy `W` of `seal.md` §13.6, as the entry's earlier items leave it, once each, ascending by recipient id.
- **ENG-SEALER-008** (device) Within one `seal` call, the sealer adapter MUST draw its random values in the order of §6.1 `seal`: the epoch root, then one wrap per recipient in ascending recipient-id order, then the frame's nonce.
- **ENG-SEALER-009** (device) When it accepts a rotation of this device's own, the sealer adapter MUST remove that entry's pending root in the same `classify` call and keep as the epoch's root the one its own wrap gave, so that the next `export_identity` holds it among the roots and not among the pending roots.

### 6.2 The membership view

```rust
pub enum DeviceStatus { Current, Revoked }
pub struct DeviceView { pub dev: Dev, pub sign_pk: Hash32, pub device_no: u16, pub status: DeviceStatus, pub name: String,
                        pub key_fp: String, pub added_seq: u64, pub added_by: Dev, pub bootstrap_seq: u64,
                        pub status_seq: u64, pub status_by: Dev, pub last_ctr: u64, pub beat_ms: i64,
                        pub rules: Vec<u32>, pub envelopes: Vec<(u8, u8)>, pub hash_versions: Vec<u32> }
pub struct RecoveryView { pub dev_r: Dev, pub sign_pk: Hash32, pub hpke_pk: Hash32, pub since_seq: u64,
                          pub until_seq: u64, pub last_ctr: u64 }
pub struct PendingRecovery { pub proposer: Dev, pub sign_pk: Hash32, pub hpke_pk: Hash32, pub key_fp: String,
                             pub seq: u64, pub start_ms: i64 }
pub struct MembershipView {
    pub board_id: [u8; 16], pub rules: u32, pub epoch: u32, pub epoch_seq: u64, pub epoch_clock_ms: i64,
    pub clock_ms: i64, pub purge_seq: u64, pub home_gen: u32, pub home_seq: u64,
    pub home_host: String, pub home_door: [u8; 5], pub devices: Vec<DeviceView>,
    pub recovery_dev: Dev, pub recovery_fp: String, pub recovery: Vec<RecoveryView>,
    pub pending_recovery: Option<PendingRecovery>,
    pub pending_override: Option<(u32, Dev, u64)>,             // rules, voter, seq
    pub last_reset: Option<(u64, u64)>,                        // at_seq, target
    pub skips: Vec<(u64, Hash32)>, pub ctl_seqs: Vec<u64>,
}
```

Each field is the field of the same name of `seal.md` §13.1 (`dev_r` is `dev_R`; `recovery` is its `recovery` list, `pending_recovery` its `pending_recovery` with the new key's fingerprint added); `key_fp`, `recovery_fp` and `PendingRecovery.key_fp` are `seal.md` §6.8's text. `recovery_dev` and `recovery_fp` are those of the current entry of `recovery` (`until_seq = 0`). `devices` is ascending by `dev`, `recovery` ascending by `since_seq`.

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

**Control history.** The fake keeps in memory, behind shared access because `open_snapshot` takes `&self` (§6.1), one history record (the layout of `seal.md` §21.5) for every control entry it accepts: from seq 1 for a fake made by `create`, and from the history of the snapshot it bootstrapped from for one made by `join` or after a rejoin. It puts the records with seq at most `S` into every snapshot body it seals at `S`, and checks a body's history on opening as `seal.md` §20.5 step 7 states. `FakeBundle` carries the history records with the snapshot material.

The fake has a genesis helper `FakeSealer::create(seed, name) -> (FakeSealer, frame_1, board_bytes)` that draws, in this order, the salt, the recovery key, the device keys, `dev`, `R_1` and the entry id, and builds seq 1 exactly as `seal.md` §12.2 and §13.3 with the toy empty state; the plaintext of seq 1 carries `at_ms = 0`; and a pairing helper `FakeSealer::join(seed, name, bundle: FakeBundle) -> FakeSealer` that takes a new device's keys and the roots the existing device hands over, standing in for `seal.md` §17. A fake made by `create` or `join` supports the rules `[1]`. The simulator, in the same crate, gives a fake another genesis time or another list of supported rules through crate-private functions; they are not part of §37's API.

`seal.md` §14.3 defines only rules version 1. A fake whose supported rules (the paragraph on `create` below) hold a version above 1
treats that version as allowing exactly (`ver`, `suite`) = (1, 1) and state-hash version 1, the values of version 1,
in its `classify` (`seal.md` §14.2 step 8) and in the beat it composes (`seal.md` §11.4).

- **ENG-FAKE-001** (engine) `FakeSealer` MUST implement every check, transition and layout of `seal.md` §8 to §15 and §20 that §6.3 does not substitute, and MUST produce the bytes of the vectors `engine-fake.json` (§35).
- **ENG-FAKE-002** (engine) `FakeSealer` MUST draw every random byte from its seeded SplitMix64 generator in the order its calls are made, so that equal seeds and equal call sequences give equal frames.
- **ENG-FAKE-003** (engine) `FakeSealer` MUST treat every supported rules version above 1 as allowing exactly (`ver`, `suite`) = (1, 1) and state-hash version 1, in its receipt checks and in the beats it composes.

## 7. Replica state and persistent records

The engine defines what it persists as records with the fields below; `device.md` owns the files, their byte encodings and fsync (the file kinds of `seal.md` §19.2). Each record type has one owner file:

| Record | File (`seal.md` §19.2 kind) | Written |
|---|---|---|
| `LogRecord` | 1 `log-<first seq>.tdl` (log segments) | appended per page |
| `Checkpoint` | 2 `checkpoint.tdk` | rewritten whole (§18.1) |
| base `Checkpoint` | 3 `base-<seq>.tdk` | one file per base (§18.2) |
| `HashRecord` | 4 `hashes.tdk` | appended |
| `OutboxOp` | 5 `outbox.tdo` | appended; rewritten when compacted |
| identity | 7 `identity.tdk` | rewritten whole (sealer bytes) |
| `NoticeOp` | 8 `notices.tdk` | appended; rewritten when compacted |
| `Meta` | 9 `meta.tdk` | rewritten whole |
| `Marker` | 10 `marker.tdk` | rewritten whole, or removed |
| `Bootstrap` (§7.10) | 17 `bootstrap.tdk` | written by the runtime before the first `Start` of a new replica; removed by it after the first checkpoint (`device.md` §6.1) |

The engine writes and reads no other kind: the control history for backups (`history.tdk`, kind 11, `device.md` §15.7), the per-device settings (`device.tdk`, kind 12) and the unsent commands of a re-pair (`unsent.tdk`, kind 18, `device.md` §18.4) are `device.md`'s.

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

`L ≥ K ≥ B` always. `K = L` except while a page is in progress (§12.3) and under the rules and commitment halts (§16.3). `K = B` except under a halt (§16): by many entries under the crash halt (§16.2), by one under the rules halt.

### 7.2 `LogRecord`

| Field | Type | Meaning |
|---|---|---|
| `seq` | u64 | the entry's seq |
| `chain` | [32] | `C_seq` as this device computed it |
| `tau` | i64 | `τ_seq`, the log clock after this entry as the dry run computed it (§12.3) |
| `frame` | bytes, 670 to 262,144 | the frame exactly as the relay served it |

The log holds the contiguous seqs from `log_floor` to `L`. `log_floor` is 1 for the device that created the board and `S + 1` for a device that bootstrapped from a snapshot at `S`; only retention (§7.13) and purge (§28.2) raise it. `retained_from` is `log_floor − 1` when the device holds that seq's chain value (from the snapshot, from the record it just truncated, or after a `Start` as §7.10 states), else `log_floor`. Every `TruncateLogBelow` hands the runtime `C_{s−1}` when the engine holds it (§7.11), so the value survives a restart.

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
| `last_entry` | list of `(dev, at_ms, seq, based_on_seq)`, ascending by `dev` | the newest accepted entry of every author, and the head it was composed at |
| `restored` | `Option<(u64, u64)>` | the counter range of a restored-copy reconcile in progress (§11.3) |

**`SnapRecord`:** `entry_seq: u64`, `author: Dev`, `info: SnapInfo`, `entry_clock: i64` (`τ` after the `snap` entry), `verdicts: list of (dev, Verdict, beat_seq)` (from other devices' beats, at most 16, ascending by `dev`), `own: Option<Verdict>`.

### 7.4 Outbox records

Every entry this device authors goes through the outbox, in the order it was enqueued: commands, control entries, beats and `snap` entries alike, because they share one counter sequence (SYNC.md §4.3).

| Field | Type | Meaning |
|---|---|---|
| `entry_id` | [16] | from `Sealer::new_entry_id`, kept for life, re-seals included (SEAL-PLAIN-002) |
| `order` | u64 | the record's position: one more than the largest `order` among the records in the outbox when it is enqueued, or 1 when the outbox is empty; it has no counter of its own and is rebuilt at `Start` with the records (ENG-STATE-003) |
| `at_ms` | i64 | the composer's `wall_ms` at enqueue (§27.3); for `cmd` the compile clock |
| `based_on` | `Head` | `(B, C_B)` at enqueue (SEAL-PLAIN-003, `core.md` §19) |
| `kind` | `Kind` | |
| `body` | `Body` | for `cmd` the encoded command; for `ctl` the intent; for `beat`, `snap` their info |
| `origin_state` | bytes | for `cmd`: the opaque origin state (the core's `IdMinter`) handed in with it, persisted in the same write (`core.md` §19) |
| `via` | `Via` | `Append`, `Register { dev, sign_pk }`, `Fence { host, door, home_gen }` (§11.7) |
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
- **ENG-STATE-006** (engine) The engine MUST give each newly enqueued record the `order` one more than the largest `order` among the records then in its outbox (at `Start`, the outbox as ENG-STATE-003 rebuilds it), or 1 when the outbox is empty.

### 7.5 `HashRecord`

`{seq: S, hash_version: u32, hash: [32]}` for every aligned `S` (§18.3). When the file holds several records for one `S`, the last one in file order is the device's hash at `S`.

### 7.6 `Marker`

`{first: u64, last: u64, attempt: u64}`: the entries `first..=last` are being applied in attempt `attempt` (§15). At most one marker exists.

### 7.7 `NoticeOp`

`Add { record: Vec<u8> }` (a notice or refusal record of `core.md` §10.7, canonical bytes from `SM::notice_bytes` or `SM::refusal_bytes`, with its kind) or `Dismiss { seq: u64, index: u32 }` (the `index`-th record, 0-based, whose `seq` equals `seq`, is dismissed by the user).

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
| `prev_session` | `Option<[16]>` | the session id of the previous process (§11.6) |
| `conflicts` | u32 | consecutive unmatched 409 conflicts (§11.4) |
| `clock` | `Option<(i64, i64)>` | the user's last clock confirmation: `(wall_ms, skew_ms)` (§27.2) |
| `last_beat_ms` | i64 | `wall_ms` of the last beat this device composed; 0 in the first `Meta` of a new replica (genesis or join, §19.8), so its first beat is due as soon as it is eligible (§23.1) |
| `duties` | `DutyState` | §29.3 |
| `ops` | list of `OpRecord`, at most 16 | operations in progress (§28) |
| `move_tries` | `(u32, i64)` | wrong move codes in a row, and the `wall_ms` until which entry is locked (§28.3) |
| `continuity_unchecked` | `Option<u64>` | the `S` of the last rejoin, until the user acknowledges it (§19.7 step 7) |
| `restored_copy` | `Option<u32>` | the count of own entries matched by restored-copy catch-ups (§11.3 step 3), until the user acknowledges it |
| `based_on_unchecked` | u32 | the number of accepted entries whose `based_on` check was skipped (`seal.md` §14.8 step 3) since the user last acknowledged them (§13.1); 0 in the first `Meta` of a new replica |

### 7.9 Bases

A base is a `Checkpoint` written at an aligned seq and kept 400 days (§18.2). Bases are read on demand (`ReadBase`).

### 7.10 `Loaded`

What `Start` carries, read by the runtime from the files:

| Field | Meaning |
|---|---|
| `checkpoint` | the newest checkpoint, or absent before the first |
| `bases` | the seq and `board_chain` of every base, ascending |
| `chains` | `(seq, chain)` of every log record, ascending |
| `log_floor` | the runtime's durable log floor: the `seq` of the latest `TruncateLogBelow` it completed, 1 before any |
| `floor_chain` | the `floor_chain` that `TruncateLogBelow` handed it with that floor, or absent |
| `tail` | every log record above `checkpoint.board_seq` |
| `hashes` | every `HashRecord` |
| `outbox` | every `OutboxOp`, in file order |
| `notices` | every `NoticeOp`, in file order |
| `meta` | `Meta`, or absent before the first |
| `marker` | the marker, or absent |
| `build_id` | the running build's id |
| `session` | the 16-byte session id this process sends in its socket hello (`relay.md` §11.3) |
| `bootstrap` | for a new replica, one of `Genesis { board, frame_1, relay }` (the creating device, §19.8) or `Join { material, added_seq, relay }` (a paired device, §19.8); absent otherwise |

At `Start` the engine's `log_floor` is the larger of the checkpoint's `log_floor` (for a bootstrap: 1 at genesis, `h + 1` at a join) and `Loaded.log_floor`, and it uses no `chains` or `tail` record below it except to learn `C_{log_floor−1}`. `retained_from` is `log_floor − 1` when `C_{log_floor−1}` is known, from `Loaded.floor_chain` when `Loaded.log_floor` equals `log_floor`, from the checkpoint's `board_chain` when its `board_seq` is `log_floor − 1`, or from a `chains` record at that seq; otherwise it is `log_floor`.

A checkpoint, or a base read later (`ReadBase`), whose board `SM::load` refuses is the local fault `engine/local_fault` (§21.4); the engine applies nothing from it.

- **ENG-STATE-002** (engine) At `Start`, the engine MUST rebuild the confirmed state, `M_B` and `C_B` from `checkpoint` (or `bootstrap`), then classify and apply the `tail` records above `B` in seq order under the marker rules of §15 before it emits any `Append`, `Admin` or `Blob` effect.
- **ENG-STATE-003** (engine) At `Start`, the engine MUST rebuild the outbox by replaying `outbox` in file order, the newest `Put` of each `entry_id` winning and a `Drop` removing it.
- **ENG-STATE-005** (engine) At `Start`, the engine MUST take as its log floor the larger of the checkpoint's (or the bootstrap's) and `Loaded.log_floor`, MUST NOT ask the runtime for a log record below it, and MUST set `retained_from` as §7.10 states.

### 7.11 Persist operations

| `PersistOp` | Fields | Meaning |
|---|---|---|
| `AppendLog` | `records: Vec<LogRecord>` | append, contiguous with the log head |
| `SetMarker` | `Marker` | replace the marker |
| `ClearMarker` | | remove the marker |
| `WriteCheckpoint` | `Checkpoint` | replace the checkpoint |
| `WriteBase` | `Checkpoint` | create the base at its `board_seq` |
| `DeleteBases` | `seqs: Vec<u64>` | delete those bases |
| `TruncateLogBelow` | `seq: u64, floor_chain: Option<Hash32>` | drop every log record below `seq`; `floor_chain` is `C_{seq−1}` when the engine holds it (its chain index, or a rejoin snapshot's `C_S`, §19.7), else `None`; the runtime keeps both (`device.md` §7.3) |
| `AppendHashes` | `Vec<HashRecord>` | append |
| `Outbox` | `Vec<OutboxOp>` | append, in order |
| `RewriteOutbox` | `Vec<OutboxRecord>` | replace the file with one `Put` per live record, in order |
| `Notices` | `Vec<NoticeOp>` | append |
| `RewriteNotices` | `Vec<NoticeOp>` | replace the file |
| `WriteMeta` | `Meta` | replace |
| `WriteIdentity` | `Zeroizing<Vec<u8>>` | replace the identity file with the sealer's export |

A single `Persist` may carry several ops; the runtime writes them in order and reports one `Persisted`.

- **ENG-PERSIST-005** (engine) The engine MUST emit `TruncateLogBelow`, `DeleteBases` and `RewriteOutbox` only in the cases §7.13, §19.7 (after the user's confirmation), §19.8 (a join, whose log is empty) and §28.2 name, and never because of a relay answer (SYNC.md §4.3 rule 6, V2-21).

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
| D8 | `WriteCheckpoint` with `board_seq ≥ x` | `TruncateLogBelow(y, floor_chain)` for any `y > x`, whatever its `floor_chain` |
| D9 | `Outbox(Put)` clearing a record's frame for a re-seal | sealing that record again |
| D10 | `WriteMeta` with an operation's new stage | the relay call of that stage (§28) |

- **ENG-PERSIST-006** (engine) The engine MUST respect every dependency of the table of §7.12.

### 7.13 Retention

- The log keeps every record whose seq is at or above the oldest kept base, and every record of the last 16,384 seqs.
- A base is deleted when its `τ` is more than 34,560,000,000 ms (400 days) of log clock below `τ_B`, except that the newest two bases are always kept.
- Hash records are kept for the seqs the log keeps.
- Notice and refusal records: at most 1,000 kept; when there are more, the oldest dismissed ones are dropped by a `RewriteNotices`, then the oldest.
- The outbox is rewritten when it holds more than `2 × live + 64` ops.

- **ENG-STATE-004** (engine) The engine MUST delete log records, bases, hashes and notices only by the rules of §7.13, of a confirmed rejoin (§19.7) and of a purge (§28.2).

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

1. **Refusals.** Answer `Notify(EnqueueRefused { ticket, code })` and stop when: the device has not joined (`engine/not_joined`); it is revoked by an accepted entry (`engine/revoked`, §24.4; a revocation that only the relay claims does not refuse); composing is blocked by the clock (`engine/clock_unconfirmed`, §27.2); `|at_ms − wall_ms| > 60,000` (`engine/runtime_protocol`). A halt or a stop does not refuse: editing goes on locally (SYNC.md §4.8, V2-9), and a halted device holds what it queues (§16.4).
2. **Naming hold.** If the last outbox record is held, apply §9.3 first.
3. **Trial apply.** Apply the command to a copy of the pending view under `catch_unwind` with `ctx = {seq: B + |U| + 1, device_no: d, at_ms, based_on: B}`. A panic answers `EnqueueRefused { ticket, code: "engine/trial_panic" }`; a refusal answers `EnqueueRefused` with the refusal's code. Nothing is queued (SYNC.md §4.3 rule 1, V2-14).
4. **Record.** Draw `entry_id = Sealer::new_entry_id()`; build the record `{entry_id, order, at_ms, based_on: (B, C_B), kind: cmd, body: encode(command), origin_state, via: Append, hold}`; emit `Persist(Outbox[Put(record)])`; apply the command to the pending view (the trial's result) and raise `view_version`.
5. **Answer.** When that `Persisted` arrives, emit `Notify(Enqueued { ticket, entry_id, provisional_seq })`. The facade's call returns only then, so the edit is durable before the call returns (SYNC.md §4.3 rule 1, §4.1).

- **ENG-ENQ-001** (engine) The engine MUST refuse an `Enqueue` whose trial apply panics, without queuing it, with `engine/trial_panic`.
- **ENG-ENQ-002** (engine) The engine MUST refuse an `Enqueue` whose trial apply is refused, without queuing it, with the refusal's code.
- **ENG-ENQ-003** (engine) Each new record MUST receive a fresh 128-bit `entry_id` from `Sealer::new_entry_id`.
- **ENG-ENQ-004** (engine) The engine MUST emit `Notify(Enqueued)` for a ticket only after the `Persisted` of the write that holds its record and its `origin_state`.
- **ENG-ENQ-005** (engine) A record MUST carry `based_on = (B, C_B)` at enqueue and the `at_ms` of its `Enqueue` (for a resubmitted command, the `wall_ms` of the `Resubmit`, §24.5), and the engine MUST NOT change either when it seals or re-seals it.
- **ENG-ENQ-006** (engine) The engine MUST refuse an `Enqueue` with the code of the first condition of §9.1 step 1 that holds.

### 9.2 Entries the engine composes

Control entries, beats and `snap` entries are enqueued by operations (§28) and duties (§29) with a fresh `entry_id` drawn at enqueue (`Sealer::new_entry_id`, ENG-ENQ-003), `at_ms = wall_ms`, `based_on = (B, C_B)` and no trial apply (they do not change the board). A control entry's items are composed when it is sealed (§10.2), from the membership of that moment.

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

A record takes part in the send loop from the `handle` call that enqueued it on: when §10.1 holds, the loop of that call (§4.5 step 6) seals it as steps 1 to 4 state, without waiting for the `Persisted` of its enqueue write. §7.12 names no dependency between the two, and ENG-PERSIST-001 makes the enqueue write durable before the `Persist` that carries the frame.

- **ENG-SEND-001** (engine) The engine MUST transmit the frames of one batch in `order`, at most 16 frames and at most 4,194,373 bytes per batch.
- **ENG-SEND-002** (engine) The engine MUST assign a record's counter when it seals it, never at enqueue (SYNC.md §4.3 rule 2, V2-17).
- **ENG-SEND-003** (engine) The engine MUST make a newly sealed frame durable before it transmits it (D2), and MUST transmit the identical bytes on every retry.
- **ENG-SEND-004** (engine) The engine MUST seal records in `order`, so that the counters of its frames rise with `order`, and MUST NOT seal a record while an earlier record is unsealed.
- **ENG-SEND-008** (engine) The engine MUST NOT wait for the `Persisted` of the write that enqueued a record before it seals that record: when §10.1 holds, the send loop of the `handle` call that enqueued the record MUST seal it as steps 1 to 4 of §10.2 state.
- **ENG-SEND-005** (engine) The engine MUST send a record with `via ≠ Append` alone, in its admin operation, and only when every earlier record is acknowledged or seen.

### 10.3 Counters

`next_ctr = 1 + max(a, h, w)`. `a` is this device's `last_ctr` in `M_K`, and 0 while `M_K` holds no record of this device (a device that joined holds none until it has classified its `DeviceAdded`, §19.8). `h` is the larger of `a` and the largest `ctr` of a record that has a frame, and `h = a` when no record has a frame. `w` is the relay's `last_ctr` for this device once a reconcile has verified it (§11.3). The first frame of a device has counter 1.

- **ENG-SEND-006** (engine) The engine MUST seal each new frame with `next_ctr` as §10.3 defines it.

### 10.4 Append results

For each result, in frame order (`relay.md` §9.5, §9.6):

| Result | Action |
|---|---|
| `new`, `dup` `(seq, chain)` | `ack = (seq, chain)`; `Put` the record in the next `Persist` the engine emits, in one `Outbox` op holding the acknowledged records in frame order, placed after every other op of that `Persist` (no effect waits for it); if `seq ≤ L`, run the echo check now (§11.5) |
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
| `Offline` | any | a relay target is set, the device is not stopped by `identity_clone` or `Closed` (§11.9), and, while it is revoked (§24.4) or after its last `Connect` ended in a refused login (§11.9), no `Connect` was emitted in the last 3,600,000 ms of `mono_ms` | `Connect { relay }` | `Connecting` |
| `Connecting` | `Connected { push }` | | `Hello` when `push` is false (over a socket the welcome follows by itself) | `Connecting` |
| `Connecting` | `Welcome` | the checks of §11.2 pass | the reconcile of §11.3; `Read` when the head is above `L` | `Reconciling` when the head is above `L`; `Live` when `head.seq = L` (the reconcile then completes at once: `q ≤ head.seq = L`, so §11.3 steps 2 and 3 have nothing left to read) |
| `Connecting` | the evidence stop ends (§21.3 step 5) | | `Hello` | `Connecting` |
| `Reconciling` | `Page` | §12 | as §12 | `Reconciling`, or `Live` when `L` reaches the latest known relay head and the reconcile is complete |
| `Live` | `Page` (push or read) | §12 | as §12 | `Live` |
| `Live` | `Tick` | `push` is false and 30,000 ms of `mono_ms` have passed since the last `Hello` | `Hello` | `Live` |
| any | `Welcome` | a check of §11.2 fails | the stop of §21 | unchanged |
| any | `Disconnected` | | none; outstanding requests are unanswered (ENG-SEND-007) | `Offline` |

The runtime applies its own backoff before acting on a `Connect` (`device.md`, full jitter from 1 to 60 s, SYNC.md §6.2).

The runtime decides how long a background job keeps the engine running (`device.md`); while it runs, the engine polls as the table says.

- **ENG-SESS-001** (engine) The engine MUST follow the session table of §11.1, and MUST NOT seal or send a frame of its outbox in any state but `Live`; the restore-tail, seed and fence calls of §21.3 and §28.3, which carry frames already in its log, are exempt.
- **ENG-SESS-009** (engine) On a welcome whose checks pass and whose head equals `L`, the engine MUST enter `Live` without a read.
- **ENG-SESS-010** (engine) When an evidence stop ends while its session is `Connecting`, the engine MUST emit `Hello` at once and continue from its welcome as §11.1 states.

### 11.2 Checks on a welcome

In order:

1. `relay_now` updates the clock measurement (§27.1).
2. If `head.seq < L`: evidence `relay_behind` (the relay holds less than this device's log, V2-9). If `head.seq ≤ L` and the chain this device holds at `head.seq` differs from `head.chain`: evidence `fork`.
3. If any record has `ack.seq > head.seq`: evidence `relay_behind`.
4. If `fenced`: §28.3.
5. The reconcile of §11.3 with `last_ctr` and `last_seq`; when step 2 or step 3 found evidence, only its step 1.

**One alarm per kind.** Every step runs, and a welcome raises at most one alarm of each kind. When step 2, step 3 or §11.3 step 1 finds `relay_behind`, the welcome raises one `RelayBehind` alarm with the evidence `relay_head` = the welcome's head, `relay_last_ctr` = its `last_ctr`, `local_head = (L, C_L)`, and `ack` = the acknowledgement with the largest seq above `head.seq` (absent when none is). A welcome raises no alarm of a kind for which `Meta` holds an unresolved one, so the `Hello` that §12.2 sends every 900,000 ms under a stop adds none. When any step found evidence, §11.3 runs only its step 1: the engine reads nothing, sets no `registry.restored` and takes no `w` from that welcome.

- **ENG-SESS-002** (engine) The engine MUST raise the evidence of §11.2 step 2 or step 3 when a welcome's head is below the log head or below an acknowledged seq, or names a chain value at a seq the device holds that differs from its own.
- **ENG-SESS-008** (engine) A welcome MUST raise at most one alarm of each kind, with the evidence §11.2 "One alarm per kind" gives a `RelayBehind`, and none of a kind for which an unresolved alarm is held; when it found evidence, the engine MUST run of §11.3 only its step 1.

### 11.3 Reconcile on hello

SYNC.md §4.3 rule 3, V1-16, V2-3. Let `a` be this device's `last_ctr` in `M_K` (0 while `M_K` holds no record of this device, §10.3), `h` as §10.3 defines it (the larger of `a` and the largest counter of a record with a frame, so `h = a` when no record has a frame), and `r = last_ctr` and `q = last_seq` from the welcome.

1. `r < a`: the relay lacks counters this device has classified: evidence `relay_behind`.
2. `a ≤ r ≤ h`: normal. The frames `a + 1` to `r` are stored; the engine reads up to at least `q` before it seals anything new.
3. `r > h`: frames under this device's `dev` exist that this folder never sealed: a restored copy (a backup, a VM snapshot, Time Machine) or another holder of the key. The engine sets `registry.restored = (h + 1, r)` (persisted in the next `WriteMeta`), seals nothing until the log reaches `q`, and during that catch-up:
   - supplies to `own_sealed`, for every counter in `h + 1..=r`, the hash of the frame being classified, so those frames are classified as its own;
   - for every such accepted frame, looks up its `entry_id` among the records: a match is the same entry sent by the earlier copy, and is treated as this device's own entry from then on (seen, then applied and dropped);
   - every record that holds a frame at a counter `≤ r` whose hash differs from the relay's frame at that counter has its frame cleared for re-sealing (D9);
   - when the log reaches `q`: if `a` (now updated) is below `r`, the relay claimed counters it cannot produce: evidence `withheld`; otherwise `restored` is cleared, `meta.restored_copy` becomes `Some(n + m)`, with `m` the number of records this catch-up matched by `entry_id` and `n` the count it held (0 when `None`), saturating at `u32::MAX`, persisted in the next `WriteMeta`, and `next_ctr` uses `w = r`. While `restored_copy` is set, `status.warnings` carries `RestoredCopy { entries }` with its count. `UserAction::AckRestoredCopy` sets it to `None`, persisted in the next `WriteMeta`.
4. Frames under this device's `dev` at counters above every value covered by steps 2 and 3 that the device did not seal are classified with `own_dev_foreign` (`seal.md` §14.7) and raise §21's `foreign_signer`.

The identity minter for commands (`core.md` §7.2) is raised by the runtime after this catch-up, from the marks the confirmed state now holds (`core.md` §19 requires it of `device.md`).

- **ENG-RECON-001** (engine) On a welcome whose `last_ctr` exceeds every counter it sealed, the engine MUST read the log through `last_seq` before sealing any new frame.
- **ENG-RECON-002** (engine) During a restored-copy reconcile the engine MUST drop, as already sent, every record whose `entry_id` is sequenced and accepted at a counter in the range of §11.3 step 3, and MUST NOT send it again.
- **ENG-RECON-003** (engine) The engine MUST clear for re-sealing every record whose frame sits at a counter the relay holds with a different frame.
- **ENG-RECON-004** (engine) The engine MUST accept a relay `last_ctr` above its own only after it has classified this device's own frames up to that counter, and MUST raise `withheld` when the relay cannot produce them (V1-16).
- **ENG-RECON-008** (engine) In the reconcile of §11.3, the engine MUST take `h` to be the larger of `a` and the largest counter of a record that holds a frame, so that a welcome with `r = a` is the normal case of step 2 whether or not any record holds a frame.
- **ENG-RECON-009** (engine) While `M_K` holds no record of this device, the engine MUST take `a = 0` in §10.3 and §11.3.

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
- **ENG-ECHO-002** (engine) When a `Register` or `Fence` answer arrives after its record has left the outbox, the engine MUST compare the answer's `seq` and `chain` with the frame and chain value its log holds at that seq, as §11.7 states, MUST raise `unechoed` on any difference, and MUST otherwise handle the answer as if the record were present.

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
    RecoveryRegisterSeq { seq: u64, dev_r: Dev, sign_pk: Hash32, frame_hash: Hash32 }, // relay.md §12.12, the only mode
    Revoke { dev: Dev },                                                      // relay.md §12.4; reinstate (§12.5) is the recovery tool's
    Fence { host: String, door: [u8; 5], home_gen: u32, frame: Option<Vec<u8>> }, // relay.md §12.6
    Door { host: String },                                                    // relay.md §10.9, on the new relay
    Seed { host: String, home_gen: u32, first_seq: u64, chain_before: Hash32, last_seq: u64,
           chain_last: Hash32, blob_id: [u8; 16], devices: Vec<SeedDevice> }, // relay.md §12.7, on the new relay
    SeedTail { host: String, first: u64, frames: Vec<Vec<u8>> },
    Compact { below_seq: u64, snap_entry_seq: u64, blob_id: [u8; 16], keep: Vec<u64> }, // relay.md §12.8
    RestoreTail { first_seq: u64, chain_before: Hash32, frames: Vec<Vec<u8>> }, // relay.md §12.9
    Purge { before_seq: u64, epoch: u32 },                                    // relay.md §12.10
}
pub struct SeedDevice { pub dev: Dev, pub sign_pk: Hash32, pub role: u8, pub status: u8, pub last_ctr: u64 }
pub struct AdminOk { pub seq: Option<u64>, pub chain: Option<Hash32>, pub token: bool, pub deleted: Option<u64>,
                     pub floor: Option<u64>, pub more: Option<bool>, pub purge_floor: Option<u64>,
                     pub done: Option<u64>, pub head: Option<Head>, pub door: Option<[u8; 5]> }
pub struct RelayRefusal { pub status: u16, pub code: String, pub until: Option<i64>,
                          pub head: Option<Head>, pub index: Option<u32> }
```

`Door`, `Seed` and `SeedTail` go to the new relay named by their `host` (§28.3): the runtime sends `Door` and `Seed` with that relay's create secret and `SeedTail` with the token `Seed` returned (`relay.md` §12.7). Every other request goes to the current target. A `Register` or `Fence` with a frame acknowledges that frame on success (`seq`, `chain`), and maps a `409 relay/conflict` or `409 relay/gap` to §11.4. The token a `Register` returns is the new device's and goes to the pairing machine (§28.4), never to the engine.

**An answer after the push.** The admin answer and the `push` of the seq it names travel on different connections (`relay.md` §12.3), so the push can arrive first: the engine then classifies the frame as its own, applies it and drops the record (§12.4). When a `Register` or `Fence` answer with `seq` and `chain` arrives and its record has left the outbox, the engine records no `ack`. It checks at once that its log at `seq` holds a frame under this device's `dev` at the counter it sealed for that record, and that its `C_seq` equals the answer's `chain`; a difference is evidence `unechoed` (§11.5). The rest of the answer is handled as when the record is still in the outbox: for a `Register`, `Notify(PairRegistered …)` and the token as §28.4 step 3 states.

### 11.8 No relay answer deletes anything

- **ENG-SESS-005** (engine) No relay answer, refusal, close, silence or absence MUST lead the engine to emit `TruncateLogBelow`, `DeleteBases`, an outbox `Drop` of an unapplied record, `forget_roots_below`, or any write that removes a key (SYNC.md §4.3 rule 6, V2-21).

### 11.9 Whole-request refusals and pauses

The runtime classifies every relay exchange (`relay.md` §14.5 through `device.md` §10.6, DEV-NET-011) and handles token renewal itself (`relay/token_stale`: it logs in again). What reaches the engine:

| Event | Action |
|---|---|
| `RelayRefused` `403 relay/fenced` | the relay reads only: stop sending to it; read to its head and look for the `Fence` (§28.3); a fenced relay whose log holds no accepted `Fence` raises `RelayProtocol` with `relay/fenced` and the status shows "re-home needed" |
| `RelayRefused` `403 relay/revoked` | §24.4, source `relay` |
| `RelayRefused` `429 relay/budget` (with `until`) | no `Append` until `until` (§10.4) |
| `RelayRefused` `404 relay/no_board` | the status shows the board missing at this relay; reconnect at most once per 3,600,000 ms of `mono_ms`; nothing is deleted |
| `RelayRefused` `409 relay/seeding` | retry after 60,000 ms of `mono_ms` |
| `RelayRefused` `507 relay/storage_refused` (with `until`) | the platform refused a write (`relay.md` §14.5): no `Append`, no `Admin` and no blob upload until `wall_ms ≥ until`, then each refused request is sent again; reads go on; `paused_until_ms = until`; no alarm |
| `RelayRefused` `401 relay/unauthorized` with `req: None` in `Connecting` (a refused login, `caps` or socket upgrade), while `meta.relay` is the relay of `M_K`'s latest accepted `Fence` (`home_host`, `home_door`, `home_gen`) | this device is no member of the board there (§28.7): the stop `Closed` with the action `RePair` (§24.5); no `Connect` while it is in force; it ends at the next `Start`; no alarm |
| `RelayRefused` with `req: None` in `Connecting`, any code the rows above do not name | a refused login, `caps` or socket upgrade: alarm `RelayProtocol` with the code; `Connect` again at most once per 3,600,000 ms of `mono_ms` (§11.1) |
| `RelayRefused` any other | alarm `RelayProtocol` with the code; the request is not repeated until the next `Welcome` |
| `Disconnected { paused_until_ms: Some(t) }` | a platform quota pause: no request until `wall_ms ≥ t` (`relay.md` §14.5) |

A refused login, `caps` or socket upgrade reaches the engine as `RelayRefused { req: None, refusal }`, then `Disconnected { unreachable: false, paused_until_ms: None }`, while the session is `Connecting`; the `Disconnected` takes the session to `Offline`, and the guards on `Offline` decide the next `Connect`. (`device.md` §10.6; §11.1.)

- **ENG-SESS-006** (engine) The engine MUST act on each whole-request refusal as §11.9 states, and MUST NOT delete or forget anything because of one.
- **ENG-SESS-007** (engine) On a login, `caps` or socket upgrade refused with `401 relay/unauthorized` while its relay target is the relay of its latest accepted `Fence`, the engine MUST enter the stop `Closed`, MUST emit no `Connect` while that stop is in force, and MUST offer re-pair and resubmit.

## 12. Receive

### 12.1 Pages

```rust
pub struct PageEntry { pub seq: u64, pub at: i64, pub frame: Vec<u8> }
pub struct Page { pub head: Head, pub relay_now: i64, pub floor: u64, pub purge_floor: u64,
                  pub more: bool, pub truncated: bool, pub fenced: bool, pub entries: Vec<PageEntry> }
```

The runtime fills a `Page` from a read answer or a push (`relay.md` §10.5, §11.4). `at` is carried and never used (RELAY-STORE-006).

### 12.2 When the engine reads

In `Reconciling` and `Live`, the engine emits `Read { after: L, limit: 256 }` whenever no read is outstanding and the latest relay head it knows (from a welcome or a page) is above `L`. A push whose first entry is `L + 1` is processed as a page; a push that starts above `L + 1` only raises the known relay head, which causes a read. While a stop of §21 is in force the engine reads nothing and sends `Hello` every 900,000 ms of `mono_ms`, so it can see the relay recover.

- **ENG-RECV-001** (engine) The engine MUST read pages of at most 256 entries, after its log head, one read at a time, except the single-frame fetch of §17.2 (`after = t − 1`, `limit = 1`), whose answer it processes only as §17.2 states and never by §12.3, and MUST NOT read while an evidence stop is in force, except for the check of §21.3 step 3 and for a relay it follows under §28.3.

### 12.3 Processing a page

In order:

1. **Limits.** At most 256 entries; every frame 670 to 262,144 bytes; the frames' total at most 1,048,576 bytes unless the page holds one entry. Otherwise `engine/runtime_protocol` and the page is dropped.
2. **Clock.** `relay_now` updates the measurement (§27.1).
3. **Head.** The checks of §11.2 steps 2 and 3 with the page's head.
4. **Truncation.** If `truncated` and `L + 1 < floor`: the log this device needs is gone from the relay; go to §19.7 and stop processing the page.
5. **Contiguity.** Ignore entries with seq at or below `L` whose frame equals the log's (a repeated push); an entry at or below `L` with another frame is evidence `fork`. The first remaining entry must have seq `L + 1` and each next one the seq after; otherwise evidence `withheld` (entries above the floor are contiguous on an honest relay, `relay.md` §10.5).
6. **Dry run.** For each entry in seq order: `C_s = chain_step(C_{s−1}, s, frame)`; `classify` with `M_{s−1}` from the previous step (or `M_L` for the first), and with `chain_at(b)` answering `C_b` for every `b` from `retained_from` to `s − 1`: from the chain index for `b ≤ L`, and from this dry run's earlier steps for `L < b < s`; it answers `None` for every other `b`. A `NeedFrames` result pauses the page until §17.2 supplies the frames, and then the same entry is classified again. A `Tamper` class, or a frame at or below `L` that differs (step 5), is evidence: the **cut** is that entry, and no entry from the cut on is persisted (§21). A receipt with a rules or commitment halt ends the dry run after that entry: the cut is the next one, and nothing after it is evidence (§16.3).
7. **Head chain.** If the last entry before the cut has the page's `head.seq`, its `C` must equal `head.chain`; otherwise evidence `fork` and the cut is the first entry of the page.
8. **Persist.** One `Persist` holding, in order: `AppendLog` of the entries before the cut (seq, `C_s`, `τ_s`, frame); `SetMarker { first, last, attempt }` over them; `WriteIdentity` when a classification reported `keys_changed`; and, last, the `Outbox` op of the acknowledgement `Put`s that §10.4 left for the next `Persist`, when there are any.
9. **Commit.** On its `Persisted`: `L` and `K` become the last persisted seq, `M_K` the dry run's last membership; the membership effects of §14 run for each entry in order; then the board applies entries `B + 1` to `K` (§12.4), unless a halt holds the board (§16).
10. **Continue.** If `more`, or the known relay head is above `L`, read again (§12.2); otherwise the run has ended: rebase (§8.4) and, if the reconcile is complete, the session becomes `Live`.

- **ENG-RECV-002** (engine) The engine MUST make a page's frames durable, with their chain values and a marker, before it commits their membership states or applies any of them (SYNC.md §4.7: "one fsync each").
- **ENG-RECV-003** (engine) The engine MUST compute `C_s` for every entry of a page and MUST refuse to persist any entry from the first one that fails §12.3 steps 5 to 7.
- **ENG-RECV-004** (engine) The engine MUST classify every entry with the membership state that its own classification of the previous seq produced, whatever the entry's class.

### 12.4 Applying to the board

For each seq `s` from `B + 1` to `K`, in order, with the receipt the dry run produced (or that a re-classification after `Start` produces):

1. If `s` is a target in `M_K.skips`: the entry is treated as refused (`engine/skipped`); nothing is applied.
2. Otherwise act on the receipt as §13.1 states.
3. If the author is this device and the entry's `entry_id` is a record's: set the record's `seen = s`; drop it (`Outbox[Drop]`) unless §24.3 makes it re-sealable.
4. `B = s`. If `s ≥ 64` and `s ≡ 0 mod 64`: the state hash at `s` (§18.3), a base when due (§18.2), the snapshot duty check (§19.2).

The board's apply of a page runs under the page's marker and `catch_unwind` (§15).

**After the apply.** When the board has applied, without a panic, every entry that the current marker covers (a page's entries in page mode, one entry in single mode, §15), the engine emits one `Persist` holding, in order: a `Notices` op with an `Add` for each notice and refusal record those entries produced (§13.3, §13.4), in seq order, and within one seq in the order `apply` returned them; an `Outbox` op with the `Drop` of each record that step 3 dropped, in seq order; and `ClearMarker`. When a run produces no record and no `Drop`, the `Persist` holds only `ClearMarker`. Any `Notify(NoticesAdded)` for those records waits for that `Persisted` (ENG-REF-002). A panic, or a halt that holds the board, leaves the marker in place for §15.

- **ENG-RECV-005** (engine) The engine MUST apply the entries to the confirmed state in seq order, each exactly once per state it rebuilds, with the action of §13.1 for its receipt.
- **ENG-RECV-006** (engine) After the board has applied, without a panic, every entry that a marker covers, the engine MUST write in one `Persist`, in this order, the notice and refusal records those entries produced, the outbox `Drop`s they caused, and `ClearMarker`.

## 13. Classes, refusals and notices

### 13.1 What each receipt does

| Receipt (`seal.md` §14.4) | Membership | Board | Alarm or stop | Record |
|---|---|---|---|---|
| `Skip` | unchanged (`M_s = M_{s−1}`) | nothing | alarm `skipped_frame` with seq, code, author `dev` as the frame says, frame hash; no stop | none |
| `Tamper` | — | — | evidence: `counter` for `seal/counter_replay` and `seal/counter_gap`, `fork` for `seal/based_on_fork` and `seal/based_on_future`; the stop of §21; the entry is the cut | none |
| `Refuse` | as `seal.md` §13.5 | nothing | alarm `refused_frame` naming the author when the code is in §13.2's list; no stop | an own entry: a refusal record (§13.3), unless re-sealable (§24.3) |
| `Accept`, `cmd` | as `seal.md` | `parse`; a parse failure is a refusal everywhere with alarm `refused_frame`; else `apply` under `Ctx{s, d_author, at_ms, based_on_seq}` | an `apply` panic: §15 | notices that concern this device (§13.4); an own entry's `apply` refusal or parse failure: a refusal record |
| `Accept`, `ctl` | the items (`seal.md` §13.6) | §14.1 | banners (§14.2); the halts of §16 | none |
| `Accept`, `beat` | `beat_ms` and lists (`seal.md` §13.10) | nothing | §22 when its state report disagrees | registry: report, verdicts (§23.3) |
| `Accept`, `snap` | as `seal.md` | nothing | §19.5 | registry: `SnapRecord`, own verdict |
| `NeedFrames` | — | — | §17.2 | — |
| `own_dev_foreign` flag | — | — | stop sending with `foreign_signer` (§21.1); applying goes on | — |
| `based_on_unchecked` flag | — | — | none: when an accepted entry with the flag is applied to the board for the first time, `meta.based_on_unchecked` rises by 1 (saturating at `u32::MAX`; a replay of the log from a base at `Start` does not count an entry again), persisted in the next `WriteMeta`; while it is above 0, `status.warnings` carries `BasedOnUnchecked { count }` with its value (V2-15); `UserAction::AckBasedOnUnchecked` sets it to 0, persisted in the next `WriteMeta` | — |
| `halt` set | — | — | §16 | — |
| `SealerError seal/missing_root` | — | — | §24.4 when this device is revoked in `M_{s−1}`, else the local fault `engine/missing_root` | — |

`d_author` is the author's `device_no` in `M_{s−1}`.

- **ENG-CLASS-001** (engine) The engine MUST act on each receipt class exactly as the table of §13.1 states.
- **ENG-CLASS-002** (engine) A `Skip` class MUST leave the confirmed state and membership unchanged and MUST raise `skipped_frame` without stopping the engine.
- **ENG-CLASS-003** (engine) A `Tamper` class MUST stop the engine with evidence and MUST leave the frame and every later one of the page unpersisted.
- **ENG-CLASS-004** (engine) A `Refuse` class or a parse failure MUST leave the confirmed state unchanged on every device, the author's own included (V1-1, V2-8).
- **ENG-CLASS-005** (engine) An `apply` refusal of an accepted `cmd` MUST leave the confirmed state unchanged and MUST NOT raise an alarm.

### 13.2 Refusals that name their author in an alarm

`seal/revoked_author`, `seal/bad_version`, `seal/bad_epoch`, `seal/aead`, `seal/bad_plaintext`, `seal/not_authorized`, `seal/bad_payload`, `seal/bad_ctl_sig`, `seal/ctl_shape`, `seal/ctl_author`, and a `cmd` parse failure: a validly signed frame that no honest build writes (`seal.md` §22). Every other refusal code is an ordinary refusal without an alarm.

### 13.3 Refusal records

When an entry authored by this device reaches the board head refused, by the sealer (a code that is not re-sealable), by a parse failure, by `apply`, or by a Skip, the engine appends a refusal record (`core.md` §10.7), with the bytes of `SM::refusal_bytes` (§5.1): `seq`, `author` (its own `device_no`), `code`, `op` and `target` (`SM::op_name` and `SM::target` of the command: the accepted entry's parsed payload, or, for an entry the sealer refused, the command its outbox record holds; both absent when that payload does not parse), and `base` and `current` (a `set_notes` conflict). Its `code` is a code of `core.md` §16 or `seal.md` §22, or one of the two engine codes `engine/skipped` (§17.3) and `engine/reseal_limit` (§24.3), the only engine codes a refusal record carries (`core.md` §10.7). A revoked device keeps its refusal records with its other records (§24.4, ENG-EPOCH-002); an own entry refused because its author was revoked carries the sealer's code (`seal/revoked_author`). Benign repeats (`core.md` §10.6) are successes and leave no record (V2-16).

- **ENG-REF-001** (engine) The engine MUST append one refusal record for each of its own entries that reaches the board head refused for any reason other than a re-sealable code, and MUST NOT append one for an entry of another device.
- **ENG-REF-002** (engine) The engine MUST make each refusal and notice record durable before it emits the `Notify` that announces it.

### 13.4 Notices

Every notice `apply` returns for the confirmed state and that `notice_concerns` this device's `device_no` is appended as a notice record (`core.md` §10.7); notices from the pending view are never recorded (ENG-SM-004). The user dismisses records (`UserAction::DismissNotice`), which appends `Dismiss`. Records are kept per §7.13.

**Each seq once.** The engine applies some entries more than once: at `Start` above an older checkpoint (ENG-STATE-002), after a caught panic (§15.3), and in the replays of §17.3, §19.9, §22.3 and §28.1. It appends notice and refusal records for a seq `s` only when `s` is above `notices_through`, the highest `seq` among the records it holds (computed from `Loaded.notices` at `Start`, raised by every record it appends); entries at or below it are applied without recording. A replay that only computes a state (§28.1 stage 1 and step 1, §22.3 step 4, §28.3 stage 3) records nothing. Two replays change what earlier applies produced and replace those records: a Skip replay (§17.3 step 3) from its target `t`, and an adoption (§22.3 step 5) from `S' + 1`. Each ends with one `RewriteNotices` that keeps every record with a seq below that first seq, then holds, in seq order, the records the replay produced for the seqs from it to the replay's end, with a `Dismiss` for each new record whose bytes equal those of a dismissed old record of the same seq (each old record matched at most once, in file order).

- **ENG-REF-003** (engine) The engine MUST record exactly the notices from confirmed applies that concern its own `device_no`, in the order `apply` returned them, and MUST record each seq's notice and refusal records once: none for a seq at or below the highest seq of the records it holds, except by the replacement of ENG-REF-004.
- **ENG-REF-004** (engine) After a Skip replay or an adoption, the engine MUST replace, in one `RewriteNotices`, every notice and refusal record whose seq is at or above the replay's first seq (the Skip's target, or `S' + 1`) by the records the replay produces, and MUST keep a new record dismissed exactly when a dismissed old record of the same seq has equal bytes.

## 14. Control entries and banners

### 14.1 What each accepted item does

Membership effects run when the entry's membership is committed (§12.3 step 9), so they land on a crash-halted device too (§16.2). Board effects run when the board applies the entry.

| Item | Membership effects on this device | Board effects |
|---|---|---|
| `Genesis` | none (bootstrap, §19.8) | none |
| `DeviceAdded` | banner `DeviceAdded` | none |
| `DeviceRevoked` | banner; if the target is this device: §24.4 | none |
| `DeviceReinstated` | banner; if the target is this device: §24.4 | none |
| `EpochRotated` | `WriteIdentity` when this device learned the root (D4); the epoch gate of §24.2; banner `KeysRotated` when the entry has no revoke or reinstate item; with `purge_before`: banner `HistoryPurged` and §28.2 | with `purge_before`: §28.2's local purge |
| `RecoveryKeyChange` | banners per mode (§26.1); the pinned recovery fingerprint prompt (`device.md`, SEAL-BKP-006) on a `direct` or `confirm` | none |
| `Fence` | banner `Moved`; §28.3 | none |
| `ResetTo` | banner `Reset` | §28.1 |
| `Skip` | banner `Skipped`; if a target is this device's crash-halt entry: §16.2 | §17.3 |
| `RulesUpgrade` | banner `RulesUpgraded` when it activates, `RulesVote` for a first override vote | when it activates `v`: `SM::upgrade(v)` before the next entry (ENG-SM-002) |

An own control entry that is refused by the sealer finishes its operation's stage as failed (§28.8); a refused own rotation also calls `Sealer::discard_pending` (SEAL-MEM-016).

- **ENG-CTL-001** (engine) The engine MUST run the membership effects of §14.1 for every accepted control entry when its membership is committed, including while the board is crash-halted.
- **ENG-CTL-002** (engine) When an own control entry carrying `EpochRotated` is refused, the engine MUST call `Sealer::discard_pending` with its `entry_id` and persist the identity before it seals any further frame.

### 14.2 Banners

```rust
pub struct Banner {
    pub id: (u64, u8),            // (seq, item index)
    pub kind: BannerKind, pub seq: u64, pub at_ms: i64,
    pub author: Dev, pub author_name: String, pub author_fp: String,   // "recovery key" and its fingerprint for dev_R
    pub subject: Option<Dev>, pub subject_name: Option<String>, pub subject_fp: Option<String>,
    pub detail: BannerDetail, pub acked: bool,
}
pub enum BannerKind { DeviceAdded, DeviceRevoked, DeviceReinstated, KeysRotated, HistoryPurged,
                      RecoveryProposed, RecoveryConfirmed, RecoveryChanged, RecoveryCancelled,
                      Moved, Reset, Skipped, RulesUpgraded, RulesVote }
pub enum BannerDetail { None, Purge { before_seq: u64 }, Recovery { key_fp: String, confirm_after_ms: Option<i64> },
                        Moved { host: String, home_gen: u32 }, Reset { target: u64 },
                        Skipped { target: u64, target_author: Dev }, Rules { version: u32 } }
```

A banner is created on every device except the entry's author device. This includes the device that a `DeviceAdded` names: it applies that entry after its join (§19.8), so it shows a `DeviceAdded` banner about its own addition. There is one exception: `RecoveryProposed` is created on every device, the proposer's included. A banner stays until the user acknowledges it on that device (`UserAction::AckBanner`), except `RecoveryProposed`, which cannot be acknowledged while its proposal is pending and is marked acknowledged when the proposal is confirmed or cancelled (SYNC.md §5.3 step 6, §5.5; V1-2). Banners are persisted in `Meta` (at most 256; when full, the oldest acknowledged ones go first, then the oldest).

- **ENG-BANNER-001** (engine) The engine MUST create a banner for every accepted control item of §14.1 that names one, on every device but the author's (the device a `DeviceAdded` names included), with the author's name and key fingerprint.
- **ENG-BANNER-002** (engine) The engine MUST keep each banner until the user acknowledges it on that device, and MUST NOT let a `RecoveryProposed` banner be acknowledged while its proposal is pending.

## 15. Poison pills

A page is applied to the board under one marker and `catch_unwind` (SYNC.md §4.5, V2-14). A marker that survives a hard abort makes the next attempt apply that page entry by entry, each under its own marker. An entry halts the device only when it fails in two consecutive attempts.

### 15.1 Attempts

`meta.attempt` is raised by one, and persisted (D5), at every `Start` and at every rebuild after a caught panic (§15.3). Every marker carries the attempt that wrote it.

### 15.2 At `Start`

1. Raise `attempt`.
2. If a marker `m` exists with `m.last > B` (the checkpoint's seq):
   - if `m.first = m.last = x`, the previous attempt died while applying `x` alone: if `meta.last_failed = (x, m.attempt − 1)`, halt on `x` (§16.2, `engine/crash_halt`); otherwise set `last_failed = (x, m.attempt)` and persist it (D6);
   - apply the entries `B + 1` to `m.first − 1` page-wise; then the entries `m.first` to `m.last` one at a time, each after a `SetMarker { s, s, attempt }` is durable;
   - continue page-wise after `m.last`.
3. Without such a marker, apply page-wise.

### 15.3 A caught panic

When `apply` panics on the entry `x` in attempt `a`:

1. If `meta.last_failed = (x, a − 1)`: halt on `x`.
2. Otherwise set `last_failed = (x, a)` and persist it (D6); start attempt `a + 1` (D5); rebuild the confirmed state at `x − 1` from the checkpoint and the log (`ReadLog`), page-wise; then apply `x` alone under `SetMarker { x, x, a + 1 }`.

A deterministic panic therefore halts within one run, after its second attempt; an OS kill in the middle of a page never halts by itself (V2-14). An entry that applies cleanly in single mode clears `last_failed` when it names that entry.

- **ENG-POISON-001** (engine) The engine MUST make a marker over the entries it is about to apply durable before it applies them to the board, one marker per page in page mode and one per entry in single mode.
- **ENG-POISON-002** (engine) After a `Start` that finds a marker above the checkpoint, the engine MUST apply the entries of that marker one at a time, each under its own durable marker.
- **ENG-POISON-003** (engine) The engine MUST halt on an entry only when that entry failed (a caught panic, or a single-entry marker left by a hard abort) in two consecutive attempts.
- **ENG-POISON-004** (engine) After a caught panic, the engine MUST rebuild the confirmed state from its checkpoint and log before applying any further entry, and MUST NOT use the state the panic left.
- **ENG-POISON-005** (engine) The engine MUST emit `ClearMarker` only after the board has applied, without a panic, every entry that the marker covers, and only in the `Persist` of §12.4 "After the apply".

## 16. Halts

### 16.1 The three halts

A device halts in exactly three cases (SYNC.md §4.2):

| Halt | Raised when | Board | Classification | Cleared |
|---|---|---|---|---|
| `Crash { seq, author, frame_hash }` | §15: an entry failed in two consecutive attempts | stays at `seq − 1` | continues (read-ahead, §16.2) | a Skip of `seq` is accepted; or a `Start` with another `build_id` |
| `Rules { seq, version }` | the sealer returns `UnknownRules(v)` with an accepted entry, or `SM::upgrade` fails with `core/unknown_rules` (`seal.md` §14.5) | stays at `seq − 1` | stops after `seq` | a `Start` whose build supports `version` |
| `Commitment { seq, author }` | the sealer returns `Commitment` with an accepted rotation (`seal.md` §14.5), also for a missed wrap of that rotation's epoch (§24.4); or `take_missed_wraps` returns `seal/commitment` (§24.4), with `seq = K` and `author` the current recovery identity of `M_K` | applies up to `seq` | stops after `seq` | never: the device re-pairs and resubmits (§24.5) |

`HaltRecord { kind, seq, author, since_ms, code }` is persisted in `Meta` (D6) with `since_ms = wall_ms` when it was raised.

- **ENG-HALT-001** (engine) The engine MUST halt only in the three cases of §16.1, and MUST NOT halt on a refusal, an alarm or any relay answer.
- **ENG-HALT-002** (engine) A halt MUST be persisted before any later effect, and MUST be restored at `Start` unless its clearing condition holds.

### 16.2 The crash halt and read-ahead

Under a crash halt on `x`, the board stays at `x − 1`, but the engine keeps reading, persisting and classifying, and commits every membership state, so revocations, rotations and Skips still land (V1-11, V2-6). When the membership commit of a Skip names `x`, the halt clears: the board applies `x` as skipped (§12.4 step 1) and then every classified entry after it, up to `K`.

- **ENG-HALT-003** (engine) Under a crash halt, the engine MUST keep reading, classifying and committing membership states, and MUST NOT apply any entry at or above the halting seq to the board until the halt clears.
- **ENG-HALT-004** (engine) When a Skip of the halting entry is accepted, the engine MUST clear the crash halt and apply the following entries in order.

### 16.3 The rules and commitment halts

These halts stop classification after the halting entry, because the device can no longer classify what follows (it lacks the rules version, or the epoch root). The engine keeps its session to show the relay's state and its devices' last seen, reads nothing further, and holds all records.

- **ENG-HALT-005** (engine) Under a rules or commitment halt, the engine MUST NOT classify any entry above the halting seq.

### 16.4 What a halted device does

A halted device (SYNC.md §4.5, V2-6):

- shows its confirmed state plus its own unapplied records as "frozen since" (§8.5);
- holds new edits: it queues them (§9.1) and sends none;
- sends only the records of a Skip it composes (§17.1), under the crash halt;
- takes no duty: no beat, snapshot, verdict, compaction, restore tail, upgrade proposal or purge continuation (§29.1), and never pairs a new device (`seal.md` §17).

- **ENG-HALT-006** (engine) A halted device MUST NOT seal any record other than one of a Skip operation it started under a crash halt.
- **ENG-HALT-007** (engine) A halted device MUST NOT take any duty of §29 and MUST refuse `PairRegister` with `engine/halted`.

## 17. Skip

### 17.1 Composing a Skip

`Operation::Skip { seq: t }` (SYNC.md §4.6, V1-11). The engine checks, from its log and `M_K`, in order (`engine/skip_target` on failure):

1. it holds the frame at `t` in its log;
2. `t` is not in `M_K.ctl_seqs` and not already skipped;
3. the frame's author is not this device (`seal.md` §13.6: a device may not skip its own entry; the recovery tool can);
4. `t ≥ max(M_K.purge_seq, L + 1 − 16,384)`, with `L + 1 − 16,384` taken as 0 when `L + 1 ≤ 16,384`: the Skip floor of §17.4.

It then enqueues a `ctl` record with `CtlIntent::Skip { targets: [(t, H(frame))] }`. A crash-halted device may compose it (§16.4); the halt screen offers it, naming the entry's author (§30.2).

### 17.2 Frames a Skip needs

When `classify` returns `NeedFrames`, for each `(t, hash)`: the engine reads the frame at `t` from its log when `t ≥ log_floor` (`ReadLog`), otherwise from the relay (`Read { after: t − 1, limit: 1 }`, whose entry must have seq `t`); the page answering this read is used only here, for that one entry, and is not processed by §12.3 (its head, floors and flags change nothing); it checks `H(frame) = hash` and classifies the entry again with the frame supplied. A frame that is held nowhere (below this device's log and the relay's floor) stops the engine with the local fault `engine/skip_unreachable` and shows it as an integrity problem without evidence (`seal.md` §27 item 12).

- **ENG-SKIP-001** (engine) The engine MUST obtain every frame a `NeedFrames` names, from its own log or from the relay, check its hash, and classify the entry again before it processes any later entry.

### 17.3 Applying a Skip

At the board apply of the accepted Skip at seq `s`, for each target `t` (`t < s ≤ B + 1`, so `t` was already applied, refused or skipped):

1. If `t` is the crash-halt entry, or the engine treated `t` as skipped when it applied it (the Skip's membership was committed before `t` reached the board, as when both arrive in one page), nothing more is needed. The engine keeps, for its current confirmed state, the set of seqs it treated as skipped.
2. If `t` is a `beat` or `snap` entry, its reports and verdicts are removed from the registry; the board is unchanged.
3. Otherwise (`t` is a `cmd` entry it applied) the engine **replays**: it reads the newest base with `board_seq < t` (`ReadBase`); loads its board and membership; re-classifies and re-applies the log from that base's seq + 1 to `s`, treating every target in `M_s.skips` as skipped; rewrites the hash records of every aligned seq from `t` to `s` (`AppendHashes`, the newer record winning, §7.5); replaces its notice and refusal records from `t` on by those of the replay (§13.4, ENG-REF-004); writes a checkpoint; and rebases.
4. When no base below `t` exists, the device cannot replay: §19.9.

An own entry that is skipped gets a refusal record with `engine/skipped`. Reports in the registry for seqs `S ≥ t` that arrived in entries below `s` are dropped, since they describe the state before the Skip.

- **ENG-SKIP-002** (engine) After accepting a Skip of an entry it had applied, the engine MUST rebuild its confirmed state by replay from a base below the target, with every skipped target excluded, so that its state equals that of a device that never applied the target.
- **ENG-SKIP-003** (engine) After a Skip replay, the engine MUST replace its state hashes for every aligned seq from the target to the Skip's seq and MUST stop comparing earlier reports for those seqs.

### 17.4 The Skip floor (ruling S15)

A Skip at seq `s` may target only `t ≥ F(s) = max(M_{s−1}.purge_seq, s − 16,384)`, with `s − 16,384` taken as 0 when `s ≤ 16,384` (`seal.md` §13.6, ruling S15). Every device computes `F(s)` from the log alone, and the sealer refuses a target below it before it asks for any frame (`seal.md` SEAL-RCPT-009), so the verdict does not depend on whether a device holds the frame. The engine keeps every frame a Skip may still need fetchable: its compaction never deletes an entry at or above `L − 16,384` (§20.2, `relay.md` RELAY-COMPACT-007), so every target a later Skip may name is still on the relay; and it keeps the target of every Skip already accepted (§20.2 step 3), which a device that bootstrapped after the target but before the Skip must still fetch (§17.2).

- **ENG-SKIP-004** (engine) The engine MUST NOT compose a Skip whose target is below `max(M_K.purge_seq, L + 1 − 16,384)`.

## 18. Checkpoints, bases and state hashes

### 18.1 Checkpoints

The engine writes a checkpoint (§7.3) of the confirmed state at `B` (SYNC.md §4.1):

1. when 200 entries have been applied since the last checkpoint;
2. when 600,000 ms of `mono_ms` have passed since the last checkpoint and `B` has changed;
3. on `Shutdown`, after which, once the `Persisted` arrives, it emits `Notify(ReadyToExit)`;
4. after a bootstrap, an adoption, a reset, a Skip replay, a purge and a rules upgrade, before the next entry is applied.

- **ENG-CKPT-001** (engine) The engine MUST write a checkpoint in each of the four cases of §18.1.
- **ENG-CKPT-002** (engine) On `Shutdown`, the engine MUST emit `Notify(ReadyToExit)` only after its final checkpoint, outbox and meta are durable.

### 18.2 Bases

A base is a checkpoint taken when `B` equals an aligned seq `S` (`S ≥ 64`, `S ≡ 0 mod 64`), written when any of these holds:

1. no base exists;
2. `τ_S − τ` of the newest base is at least 604,800,000 ms (seven days of log clock);
3. a snapshot is due at `S` (§19.2), so that the device can write it later from the base.

Two more bases are written whether or not their seq is aligned: the base of the state before a rejoin (§19.7 step 2) and the base at the snapshot's seq of a bootstrap (§19.7 step 4, §19.8). Retention treats them like any base (§7.13).

Bases are kept 400 days (§7.13; SYNC.md §4.1, V2-9, V2-12). A restore to a point, a Skip replay and an adoption replay from them.

- **ENG-CKPT-003** (engine) The engine MUST write a base at every aligned seq at which one of the three conditions of §18.2 holds, with the confirmed state exactly at that seq.

### 18.3 The state hash (ruling S7)

```text
state_hash = SHA-256( ctx("state") ‖ u32 hash_version ‖ SM::board_hash(state) ‖ SHA-256(membership bytes) )
ctx("state") = "taskdeck/v1/state" followed by one 0x00 byte (18 bytes; seal.md §4.1)
```

| Field | Width | Value |
|---|---|---|
| context | 18 | `ctx("state")` |
| `hash_version` | 4 | u32, 1 |
| board hash | 32 | `SHA-256(canonical board bytes)` (`core.md` §6.13) |
| membership hash | 32 | `SHA-256(canonical membership bytes)` (`seal.md` §13.2) |
| **total** | **86** | |

The state and membership are taken at the same seq: the confirmed state after the entry `S` and `M_S`. The engine records its state hash at every aligned `S` after applying `S` (`AppendHashes`, SYNC.md §4.7: "where every device records its state hash"), and computes it for every snapshot, `ResetTo` target, pairing bundle and backup. `taskdeck_sync::state_hash` (§37) is the one implementation the other crates call or reproduce.

A bootstrap from a snapshot at `S` (§19.7 step 4, §19.8) records no hash at `S`, aligned or not: the device did not apply the entry at `S`, so it has no state hash of its own there and gives no verdict on a `snap` entry for `S` (§19.5). Its first hash record is at the first aligned seq it applies.

- **ENG-HASH-001** (engine, seal, recovery) A state hash MUST be SHA-256 of exactly the 86 bytes of §18.3, with the board hash and the membership hash taken at the same seq.
- **ENG-HASH-002** (engine) The engine MUST record its state hash at every seq `S ≥ 64` with `S ≡ 0 mod 64` that it applies, after applying the entry at `S`.
- **ENG-HASH-003** (engine) The engine MUST NOT write a hash record at the seq of a snapshot it bootstrapped from.

## 19. Snapshots and bootstrap

### 19.1 The registry of snapshots

The engine keeps a `SnapRecord` (§7.3) for every accepted `snap` entry, with the verdicts it learns: its own (§19.5) and those of other devices' beats (§23.3). It keeps the newest 64.

### 19.2 When a snapshot is due, and who writes it

At the board apply of each aligned `S`, the snapshot is **due at `S`** for this device when no own `snap` record is in its outbox, its newest own `SnapRecord` (author this device), if any, has `τ_S − entry_clock ≥ 86,400,000` (at most one snapshot per 24 hours of log clock, ruling S25), and either:

1. it knows no `SnapRecord` that is usable (§19.3) with both `τ_S − entry_clock < 604,800,000` (seven days) and `S − info.seq < 1,000` (1,000 entries, SYNC.md §4.7), and none that is still waiting for verdicts (no verdict at all) with `τ_S − entry_clock < 172,800,000` (two days); or
2. it is in a mismatch it has not yet answered with a snapshot: a device of another hash group reported at an aligned seq below `S`, and this device's group is the majority or ties (§22.2). Each such mismatch makes one snapshot due.

When a snapshot is due at `S`, the device writes a base at `S` (§18.2) and records `duties.snapshot = (S, τ_S)` in `Meta`. It then writes the snapshot when all hold:

- it is eligible (§29.1);
- `wall_ms ≥ τ_S + r × 3,600,000`, where `r` is its rank in the duty order of `M_S` (§29.2);
- it has accepted no `snap` entry for a seq at or above `S` since.

So the device that beat most recently writes first, at once, and each other device waits one more hour; two devices write the same snapshot only when the first was not awake, and a duplicate costs only storage (ruling S8). The duty is cleared when a `snap` entry for a seq at or above `S` is accepted, when a later aligned seq becomes due (it replaces this one), or after three failed uploads.

- **ENG-SNAP-001** (engine) The engine MUST decide that a snapshot is due at an aligned seq exactly as §19.2 states: the 24-hour limit and one of its two conditions.
- **ENG-SNAP-002** (engine) The engine MUST write a due snapshot only when it is eligible, when `wall_ms` has reached `τ_S + r × 3,600,000` for its rank `r`, and when it has accepted no `snap` entry for that seq or a later one.
- **ENG-SNAP-006** (engine) The engine MUST NOT decide a snapshot due, or write one, at an aligned `S` while an own `snap` record is in its outbox or its newest own accepted `snap` entry has an `entry_clock` above `τ_S − 86,400,000`.

### 19.3 Usable snapshots

A snapshot is **usable** when the device knows at least one verdict `Ack` from a device other than its writer and no verdict `Mismatch` from any device (SYNC.md §4.7, V2-7). Compaction (§20), a rejoin (§19.7) and an adoption (§22.3) use only usable snapshots, or, for an adoption, the chosen device's own. A pairing never uses a stored snapshot: the existing device takes a fresh one of its own state (§28.4, V1-10).

- **ENG-SNAP-003** (engine) The engine MUST use for compaction and rejoin only snapshots that are usable as §19.3 defines it.

### 19.4 Taking and announcing a snapshot

1. Read the base at `S` (`ReadBase`); build `SnapshotMaterial { seq: S, chain: C_S, hash_version: 1, state_hash, board, membership }`, where `state_hash` must equal this device's hash record at `S` (else the local fault `engine/local_fault`).
2. `Sealer::seal_snapshot(M_K, material)`, which seals under `M_K`'s current epoch. A `SealerError` (`seal/snapshot_large`: the body would exceed 67,108,864 bytes, `seal.md` SEAL-BLOB-009) ends the attempt: nothing is uploaded or announced, and the duty stays as it was.
3. `Blob { Upload { purpose: 1, seq: S, blob } }` (`relay.md` §10.7, purpose 1).
4. On success, enqueue a `snap` record whose `SnapInfo` carries `S`, the hash, its version, the blob's id, `sha256`, size, parts, `inflated` and epoch.
5. When the send loop reaches that record and `M_K.epoch` differs from the blob's epoch, the record is dropped unsent and the duty stays (the `snap` entry's epoch must equal its frame's, `seal.md` §11.5).

- **ENG-SNAP-004** (engine) The engine MUST announce a snapshot only after its blob upload completed, and MUST drop an unsent `snap` record whose blob epoch is no longer the current epoch.

### 19.5 Verdicts

At the board apply of a `snap` entry by writer `A` for seq `S`: when `A` is not this device and this device holds a hash record at `S` with the entry's `hash_version`, its verdict is `Ack` if the hashes are equal and `Mismatch` otherwise. The verdict is stored in the `SnapRecord` and in `pending_verdicts` (at most 64; the oldest is dropped), and sent in its next beat (SYNC.md §4.7: "acks the `snap` entry, or raises a mismatch alarm, in its next beat"). A `Mismatch` raises the alarm `snapshot_mismatch` (writer, `S`, both hashes) and enters the writer's hash as a report at `S` (§22.1). The writer never judges its own snapshot. A device that did not apply `S` itself (it bootstrapped later) gives no verdict.

- **ENG-SNAP-005** (engine) For every `snap` entry by another device for a seq at which it recorded a state hash of the same version, the engine MUST produce exactly one verdict, `Ack` for equal hashes and `Mismatch` otherwise, and MUST send it in its next beat.

### 19.6 Blob requests

```rust
pub enum BlobRequest { Upload { purpose: u8, seq: u64, blob: SealedBlob }, Fetch { id: [u8; 16], parts: u32 },
                       List, Meta { id: [u8; 16] } }
pub struct BlobMeta { pub id: [u8; 16], pub purpose: u8, pub seq: u64, pub author: Dev, pub size: u64,
                      pub parts: u32, pub sha256: Hash32, pub complete: bool }
pub enum BlobOk { Uploaded, Parts(Vec<Vec<u8>>), List(Vec<BlobMeta>), Meta(BlobMeta) }
```

The runtime performs `blob-begin` and every `blob-put`, resuming from `blob-meta` after an interruption (`relay.md` §10.7 "Repeat").

### 19.7 Rejoin after a long absence

A page with `truncated` set and `L + 1 < floor` means the relay no longer holds the log this device needs: it was away for more than 365 days, or behind a purge (SYNC.md §4.7, V2-15).

1. The engine enters `Rejoining` and shows it: while `session` is `Rejoining`, `status.stops` carries one `StopView { kind: Rejoin, since_ms, alarm: None, action: ConfirmRejoin }`, with `since_ms` the `wall_ms` at which it entered. That view is derived from the session; it is no `StopRecord` in `Meta`, and it holds back nothing that `Rejoining` does not already hold back (§10.1, §11.1). The engine waits for `UserAction::ConfirmRejoin`: it will replace its log, and the user decides that, not the relay (ENG-SESS-005).
2. It writes a base of its current confirmed state (kept by retention like any base), then lists blobs (`Blob { List }`) and takes as candidates the complete purpose-1 snapshots with `seq ≥ floor − 1`, newest first.
3. For a candidate: fetch the parts; `Sealer::open_snapshot(parts, expected, author, exact: false)`, with `expected.seq`, `blob`, `sha256`, `size` and `parts` from the blob's metadata (its other fields are not read) and `author` the metadata's author (`seal.md` §20.5, declared mode); then: the membership bytes parse, name this board and list this device as current; the board loads (`SM::load`) and `SM::rules` of it equals the membership's `rules`; the state hash recomputed (§18.3) equals the body's. A failure (including the lack of the root of the blob's epoch) moves to the next candidate; a failed `SM::load` also raises the alarm `BlobFailure` with the writer, the blob id and the load's code.
4. On the first candidate that passes: the confirmed state and `M_S` from the snapshot, `B = K = L = S`, `C_S` from the body, `log_floor = S + 1`, `retained_from = S`. Then one `Persist` holding `WriteCheckpoint` and `WriteBase`, both at `S` (the store writes the snapshot's history records ahead of the checkpoint, `device.md` §15.7, §8.3); and, after its `Persisted` (D8), a `Persist` holding `TruncateLogBelow(S + 1, Some(C_S))` with `C_S` from the body (allowed by the user's confirmation, ENG-PERSIST-005). No hash record is written at `S` (§18.3). Then read from `S + 1`; the read need not wait for either `Persisted`, because the page's own `Persist` follows both in the serial queue (ENG-PERSIST-001).
5. While classifying from `S + 1`, the engine looks for the `snap` entry naming the candidate's blob: its author must be the `author` the snapshot was opened with, and its `seq`, `hash`, `sha256`, `size`, `parts`, `inflated` and `epoch` must equal what was opened (the epoch and inflated length that `open_snapshot` returned) (else alarm `snapshot_mismatch` naming the writer, and step 3 with the next candidate, from a fresh base). When the log reaches the relay head, the snapshot must be usable (§19.3); otherwise the next candidate is tried.
6. If no candidate passes: the stop `stranded` (§21.4); the device offers "re-pair and resubmit" (§24.5).
7. On success the engine sets `meta.continuity_unchecked = Some(S)`, persisted in the next `WriteMeta`. While it is set, `status.warnings` carries `ContinuityUnchecked { from: S }` (SYNC.md §4.7). `UserAction::AckContinuity` sets it to `None`, persisted in the next `WriteMeta`.

Records in the outbox survive a rejoin; frames sealed under an epoch that no longer applies are refused and re-sealed (§24.3). The rejoin asks the user nothing about these records and marks none of them. Once it has ended, the pending view rebases them on the snapshot's state and the log after it (§8.4). The send loop then seals and sends them in order, with the `based_on` and `at_ms` they were enqueued with, however old (ENG-ENQ-005). A change of theirs that overwrites a later change yields an overwrite notice (`core.md` §10.2); one that no longer applies is refused, as for any record. The rejoin prompt tells the user that these records stay queued (`device.md` §17.7, `bridge.md` §13.1).

- **ENG-BOOT-001** (engine) The engine MUST NOT replace its log for a rejoin without the user's confirmation, and MUST keep its state before the rejoin as a base.
- **ENG-BOOT-002** (engine) A rejoin MUST end only on a snapshot that opened under a root this device holds, whose recomputed state hash matched, whose announcing `snap` entry matched it in author, seq, hash, `sha256`, `size`, `parts`, `inflated` and epoch, and that is usable when the log reaches the relay head.
- **ENG-BOOT-007** (engine) While its session is `Rejoining`, the engine MUST show in `status.stops` exactly one stop of kind `Rejoin` with the action `ConfirmRejoin`, and MUST NOT persist it as a `StopRecord`.
- **ENG-BOOT-008** (engine) The engine MUST keep every outbox record through a rejoin and send it once the rejoin has ended, with the `based_on` and `at_ms` it was enqueued with, and MUST NOT drop a record, hold it back or ask about it because of the rejoin or of its age.

### 19.8 Bootstrap at creation and at pairing

- **Genesis** (ruling S2, SYNC.md §9 step 6). `Loaded.bootstrap = Genesis { board, frame_1, relay }`: the creating device's runtime has built the genesis frame with the sealer and created the board at the relay (`relay.md` §10.2); `relay` is that relay's host and the board's door, with `home_gen = 1` (the genesis membership's, `seal.md` §13.3), and the engine sets `meta.relay` from it. The engine loads `board` (`SM::load`), classifies seq 1 (the sealer uses its genesis anchor), which must be `Accept` with a `Genesis` item, checks that `SM::rules(board) = M_1.rules` and that the item's `state_hash` equals the state hash of `board` and `M_1`, and persists `AppendLog[1]`, a checkpoint and a base at 1. Otherwise (a failed load included) it stops with `engine/bad_genesis`.
- **Join** (`seal.md` §17.9). `Loaded.bootstrap = Join { material, added_seq, relay }`: the pairing machine has checked the bundle, the snapshot and the log to the relay's head. The engine loads the snapshot at `h` as §19.7 step 4 states, with `S = h`: the same state, the same `Persist` of `WriteCheckpoint` and `WriteBase` at `h` whether or not `h` is aligned (§18.2), and after its `Persisted` the same `TruncateLogBelow(h + 1, Some(C_h))`, which deletes nothing (the log is empty) and makes the runtime keep `C_h` as `floor_chain` (`device.md` §7.3). It needs no user confirmation. A material whose board `SM::load` refuses, or whose `SM::rules` differs from `M_h.rules`, is the local fault `engine/local_fault`, and the user pairs again. The engine sets the relay target from the bundle's host and door, and reads from `h + 1`; it has joined once `B ≥ added_seq` and `M_B` gives it a `device_no` (`seal.md` SEAL-PAIR-012). Before that, `Enqueue` is refused with `engine/not_joined`. Its `Meta` starts with `last_beat_ms = 0` (ENG-BEAT-003). So the first time duties are evaluated (§4.5 step 5) after it is eligible (§29.1), §23.1 condition 1 holds and it enqueues a beat. Its first entry is therefore that beat, sealed with counter 1, ahead of any command the user enqueues later.

- **ENG-BOOT-003** (engine) A device bootstrapped by pairing MUST NOT enqueue, seal or send any entry before it has applied the `DeviceAdded` at `added_seq` that names it.
- **ENG-BOOT-004** (engine) At genesis, the engine MUST check the `Genesis` item's state hash against the state hash it computes from the genesis state and `M_1`, and the genesis state's rules version (`SM::rules`) against `M_1.rules`, and MUST refuse to start the board when either differs.

### 19.9 A Skip or `ResetTo` the device cannot replay

When a Skip (§17.3) or a `ResetTo` (§28.1) needs a state from before this device's oldest base or log (it bootstrapped after the target), the engine stops its board at `s − 1` with the stop `rebootstrap { after: s }` (§21.4), keeps classifying and committing membership as under a crash halt, and waits for a usable snapshot at an aligned `S > s`. It then opens it with `exact = true` and the `snap` entry's `SnapInfo`, checks that the membership bytes equal its own `M_S`, loads the board at `S` (`SM::load`) and checks `SM::rules(board) = M_S.rules`, sets `B = S` and applies the classified entries after it. A snapshot that fails to open or to load, or fails either check, is passed over (alarm `BlobFailure` with the writer, the blob id and the code of a failed open or load), and the device waits for the next usable snapshot. This keeps its log; it needs no confirmation.

- **ENG-BOOT-005** (engine) A device that cannot replay to a Skip or `ResetTo` target MUST NOT apply the entry that needs it, and MUST resume only from a usable snapshot above that entry whose membership bytes equal its own and whose board loads with that membership's rules version.
- **ENG-BOOT-006** (engine) On a bootstrap from a snapshot at `S` (a rejoin, §19.7 step 4, or a join, §19.8), the engine MUST write the checkpoint and the base at `S` in one `Persist`, and MUST emit `TruncateLogBelow(S + 1, Some(C_S))` only after that `Persist`'s `Persisted`.

## 20. Compaction

### 20.1 Who compacts, and when

Compaction is a duty (SYNC.md §4.7, `relay.md` §12.8): an eligible device (§29.1) runs it at most once per UTC day of `wall_ms`, starting when `wall_ms ≥ the day's 00:00 UTC + r × 3,600,000` for its rank `r` (§29.2), and again on later days while the relay answers `more: true`.

### 20.2 What it sends

1. **Snapshot.** The newest `SnapRecord` that is usable, whose writer is not this device, and whose own verdict is `Ack` (this device checked it, `relay.md` §12.8 step 5). Let `S` be its seq and `x` its `entry_seq`. None: no compaction today.
2. **Bound.** `b` = the smallest of: `S`; `L − 16,384` (§17.4); and `y + 1` for the largest seq `y` in the log with `τ_y ≤ wall_ms − 31,622,400,000` (366 days of log clock, one day more than the relay's own 365-day check on its `at` column). When `b` is at or below the relay's `floor` (from the latest welcome), there is nothing to do.
3. **Keep.** Every seq of `M_K.ctl_seqs` and every target `t` of `M_K.skips` from the relay's `floor` to `b − 1`, ascending, without duplicates (V2-15; a device that bootstrapped after a target but before its Skip still fetches the target, §17.2). When that list would exceed 131,072 seqs (`relay.md` §20), `b` is lowered to the largest value for which it does not.
4. `Admin { Compact { below_seq: b, snap_entry_seq: x, blob_id: info.blob, keep } }`, with `info` the chosen `SnapRecord`'s, so that the relay relies on, and keeps (`relay.md` RELAY-COMPACT-004), exactly the blob the `snap` entry at `x` announces.

| Answer | Action |
|---|---|
| success, `more` | record the day; run again tomorrow |
| success, not `more` | record the day |
| `409 relay/too_recent` | record the day; the next run computes a lower `b` |
| `409 relay/no_snapshot`, `409 relay/target` | record the day |
| `429 relay/delete_budget` | record the day |
| any other | alarm `relay_protocol` with the code; record the day |

- **ENG-COMPACT-001** (engine) The engine MUST compact only relying on a usable snapshot by another writer that it acknowledged itself.
- **ENG-COMPACT-002** (engine) The engine MUST NOT send a `below_seq` above the snapshot's seq, above `L − 16,384`, or above the first seq whose log clock is within 366 days of its local clock.
- **ENG-COMPACT-003** (engine) The engine MUST list in `keep` every control entry of its membership and every Skip target its membership records, between the relay's floor and `below_seq`.
- **ENG-COMPACT-004** (engine) The engine MUST send at most one `Compact` per UTC day of its local clock.

## 21. Integrity evidence and the stop

### 21.1 Alarms and evidence

An alarm is raised only with evidence (SYNC.md §4.8, V2-10, V3-14). Silence is never evidence: every device's last beat and last entry are shown as "last seen" (§30.2) and never raise an alarm.

```rust
pub struct AlarmRecord { pub id: u64, pub kind: AlarmKind, pub raised_ms: i64, pub seq: Option<u64>,
                         pub author: Option<Dev>, pub code: Option<String>, pub evidence: Evidence,
                         pub resolved: bool }
```

```rust
pub enum ForkSource { Welcome, PageHead, RepeatedEntry, BasedOn }
pub enum Evidence {
    RelayBehind { relay_head: Head, relay_last_ctr: Option<u64>, local_head: Head, ack: Option<(u64, Hash32)> },
    Fork { local_chain: Hash32, relay_chain: Hash32, source: ForkSource },
    Counter { ctr: u64, expected: u64 },
    Unechoed { ack_frame_hash: Hash32, ack_chain: Hash32, log_frame_hash: Hash32, log_chain: Hash32 },
    Withheld { relay_head: u64, claimed_ctr: Option<u64>, cursor: u64, missing_from: u64, missing_to: u64,
               counters: bool },
    ForeignSigner { ctr: u64 },
    IdentityClone { session: Option<[u8; 16]>, ctrs: Vec<u64> },
    Mismatch { groups: Vec<(Hash32, Vec<Dev>)> },
    Frame { frame_hash: Option<Hash32> },
    Request { request: String },
    Blob { blob: [u8; 16] },
    RestoredCopy { entries: u32 },
}
```

Each `AlarmKind` carries the `Evidence` variant and fills the `AlarmRecord` fields of its row; a field the row does not name is absent. In `Mismatch`, `groups` are ascending by hash and each group's devices ascending by `dev`. In `Withheld`, `missing_from..=missing_to` is a range of seqs, or of counters when `counters` is true. `Request.request` names the exchange: `hello`, `read`, `append`, `login`, `blob`, or `admin/<op>` with `op` as `relay.md` §12.1 names it.

| `AlarmKind` | `Evidence` | `seq` | `author` | `code` |
|---|---|---|---|---|
| `RelayBehind` | `RelayBehind` | | | |
| `Fork` | `Fork` | the seq | | |
| `Counter` | `Counter` | the seq | the frame's author | `seal/counter_replay` or `seal/counter_gap` |
| `Unechoed` | `Unechoed` | the acknowledged seq | | |
| `Withheld` | `Withheld` | | | |
| `ForeignSigner` | `ForeignSigner` | the seq | this device | |
| `IdentityClone` | `IdentityClone` | | | |
| `StateMismatch`, `SnapshotMismatch`, `ResetMismatch` | `Mismatch` | the aligned seq, or the reset's target | the snapshot's writer or the reset's author | |
| `SkippedFrame` | `Frame` with the frame hash | the seq | | the sealer's code |
| `RefusedFrame` | `Frame` with `None` | the seq | the author | the code |
| `RelayProtocol` | `Request` | the seq the request named, if any | | the code |
| `BlobFailure` | `Blob` | | the writer | the code (`seal.md` §20.5) |
| `RestoredCopy` | `RestoredCopy` | | | |

| `AlarmKind` | Evidence | Response |
|---|---|---|
| `RelayBehind` | the relay head (or `last_ctr`) and this device's log head or acknowledgement it contradicts | evidence stop (§21.2); repair by restore tail (§21.3) |
| `Fork` | the seq, this device's chain value there and the relay's (a welcome, a page head, a repeated entry, or a `based_on` chain, `seal.md` §14.8) | evidence stop; repair by re-home |
| `Counter` | author, counter, expected counter, seq (`seal/counter_replay`, `seal/counter_gap`) | evidence stop; repair by re-home |
| `Unechoed` | seq, the acknowledged frame hash and chain, what the log holds | evidence stop; repair by re-home |
| `Withheld` | the relay head or claimed `last_ctr`, this device's cursor, the missing seqs or counters | evidence stop; repair by re-home if it lasts |
| `ForeignSigner` | seq and counter of a frame under this device's `dev` it did not seal | stop sending; "replace this device" (V1-1) |
| `IdentityClone` | the other session id, or the three conflicting frames' counters | stop sending; §11.6 |
| `StateMismatch`, `SnapshotMismatch`, `ResetMismatch` | the seq, the hashes and the devices of each group | §22 |
| `SkippedFrame` | seq, code, frame hash | none (§13.1) |
| `RefusedFrame` | seq, author, code | none |
| `RelayProtocol` | the request and the code | none |
| `BlobFailure` | writer, blob id, code (`seal.md` §20.5) | none; the next candidate or retry |
| `RestoredCopy` | the count of own entries matched | none (a warning) |

Evidence is persisted in `Meta` (at most 256 alarms; resolved ones are dropped first). An alarm's frames are not kept; its hashes and seqs are.

- **ENG-EVID-001** (engine) The engine MUST raise an alarm only with the evidence §21.1 lists for its kind, and MUST NOT raise one because a device has been silent for any length of time.
- **ENG-EVID-002** (engine) Each alarm MUST be persisted with its evidence before the `Notify` that shows it.

### 21.2 The evidence stop

On `RelayBehind`, `Fork`, `Counter`, `Unechoed` or `Withheld`, the device (SYNC.md §4.8, V2-9):

1. stops sending: no `Append`, and no `Admin` except the repair calls of §21.3 and an operation the user starts to re-home (§28.3);
2. stops applying: it reads nothing and persists nothing new from the relay (ENG-RECV-001);
3. keeps editing locally: `Enqueue` works and the pending view grows (§9.1);
4. shows one state with the evidence (§30.2);
5. takes no duty.

- **ENG-EVID-003** (engine) Under an evidence stop, the engine MUST NOT emit `Append`, MUST NOT persist any frame from the relay, and MUST accept `Enqueue`.

### 21.3 Repairs

**Restore tail** (`relay.md` §12.9). When the evidence is `RelayBehind` and the relay's chain at its head equals this device's `C` at that seq (the relay is a prefix of this device's log: a plain rollback), the device repairs it, as any device does that holds more than the relay. No device is designated: whoever is awake restores what it holds; another device's later attempt answers `relay/not_next` with the relay head and resumes from there.

1. Requests of at most 64 frames and 1,048,576 bytes, from the relay head + 1, in seq order, with `chain_before` equal to this device's chain at the relay head. The engine takes each request's frames from its durable log with `ReadLog { from, to }` over at most 64 seqs (§4.3; the in-memory state holds no frames, §7.1), ends the request before the frame that would take it above 1,048,576 bytes, and sends the frames byte for byte as the log holds them. A `LocalRead` answered `Missing` is the local fault `engine/local_fault` (§21.4), and nothing is sent. A request ends with the first control entry it contains; the next request is sent only after that entry's repairs of step 2 have succeeded, so that a frame of an author the entry registers never reaches the relay before that author's row (`relay.md` §12.9, `relay/unknown_author`).
2. After the request that restores a control entry this device classified `Accept`, before the next request, the device repairs the relay for it, in item order (`relay.md` §12.9): `RegisterSeq { seq, dev, sign_pk, frame_hash }` for a `DeviceAdded`, with `sign_pk` the added device's (`DeviceView.sign_pk`, §6.2); `RecoveryRegisterSeq { seq, dev_r, sign_pk, frame_hash }` for a `K`-shape entry, with `dev_r` and `sign_pk` those of the `recovery` entry it installed (§6.2); `Revoke` for each revoked device other than itself; `Fence` without a frame for a `Fence`; and, for a rotation that carries `purge_before = p` with new epoch `n`, `Purge { before_seq: p, epoch: n }`, whose continuation of §28.2 step 3 (`duties.purge`) runs once the stop has ended. A refused control entry needs no repair. A `DeviceReinstated` needs the recovery identity's signature: the device records the alarm `RelayProtocol` with `engine/restore_incomplete` naming the seq, and the user repairs it with the recovery tool.
3. `409 relay/not_next` with `head`: when `head.seq ≤ L` and this device's chain at `head.seq` equals `head.chain`, resume from `head.seq + 1`; when `head.seq > L`, read (the stop is lifted for this check), and clear the evidence when the relay's chain at `L` equals this device's; else evidence `Fork`.
4. `409 relay/chain_mismatch`: evidence `Fork`.
5. When the relay head reaches `L` with this device's chain, and every acknowledgement echoes, the evidence is resolved and the stop ends.

**Blobs.** A restore tail restores log rows and the author rows they need, not blobs: a blob that the rollback lost stays lost, and no device uploads it again, because its sealed bytes, which the announcing `snap` entry pins by `sha256`, cannot be reproduced. The `SnapRecord` of that `snap` entry stays in the registry. A rejoin does not meet the blob (it takes its candidates from the relay's blob list, §19.7 step 2); a compaction that names it is answered `409 relay/no_snapshot` and records the day (§20.2); the next snapshot that §19.2 makes due gives compaction a usable blob again.

**Re-home.** `Fork`, `Counter`, `Unechoed`, and `Withheld` that lasts more than 86,400,000 ms of `wall_ms` are answered by moving the board to another relay, seeded from this device's log (§28.3, SYNC.md §6.7). The stop ends on this device when it follows a `Fence` to a relay whose log agrees with its own.

- **ENG-EVID-004** (engine) A device with `RelayBehind` evidence whose log extends the relay's chain MUST restore the relay's tail from its own log at the original seqs, ending each request at its first control entry and sending, before the next request, the repairs §21.3 step 2 lists for that entry.
- **ENG-EVID-005** (engine) The engine MUST end an evidence stop only when the condition of §21.3 step 5 holds, or when it follows a `Fence` to a relay whose chain agrees with its log.

### 21.4 Local faults

`engine/persist_failed`, `engine/local_fault` (a panic in a trait method other than `apply`, an inconsistency between two of the device's own records, a checkpoint, base or pairing material of its own whose board does not load, an `SM::reset` error, or a `LocalRead` answered `Missing`), `engine/missing_root`, `engine/own_frame_refused`, `engine/skip_unreachable`, `engine/runtime_protocol` (when it is persistent: the same violation three times in one `Start`), `engine/bad_genesis`, `stranded` and `rebootstrap` stop sending and applying like evidence (except `rebootstrap`, §19.9), show their code, and end at the next `Start` (`stranded` and `rebootstrap` end as §19.7 and §19.9 say). They are not integrity alarms: nothing proves the relay did wrong.

- **ENG-EVID-006** (engine) A local fault MUST stop sending and applying, MUST NOT be shown as an integrity alarm, and MUST be re-evaluated at the next `Start`.
- **ENG-EVID-007** (engine) The engine MUST take the frames of each restore-tail request from its durable log through `ReadLog` and send them unchanged, and MUST send no request whose `ReadLog` was answered `Missing`.

## 22. Determinism mismatch and adoption

### 22.1 Reports

For each aligned seq `S`, the registry holds the latest report of each device: from its beats' `state`, and, for a writer, from its `snap` entries. This device's own report is its hash record at `S`. Only reports with this device's hash version count. Reports that a Skip made stale are dropped (§17.3).

### 22.2 Who is in the minority

For an aligned `S` at which this device holds a hash and some current device reported a different one: group the current devices' reports at `S`, this device's included, by hash. Let `g` be the size of this device's group and `m` the size of the largest other group.

- `g > m`: this device is in the majority. It raises `StateMismatch` naming the other devices, without a stop, and owes one snapshot (§19.2 condition 2).
- `g ≤ m`: this device is in the minority, or the vote is tied (two devices that disagree): the **divergence stop**. It stops sending and taking duties, keeps reading and applying (so it can compare later hashes and adopt quickly), and shows the evidence. A tie also makes one snapshot due.

The divergence stop ends when, at a later aligned seq, this device's group is strictly larger than every other group and has another member, or after an adoption (§22.3). So only the minority halts its participation (SYNC.md §4.8, V1-10), and the majority keeps working.

- **ENG-DIVERGE-001** (engine) The engine MUST enter the divergence stop exactly when, at some aligned seq, its own group of reports is not larger than another group, and MUST NOT stop when its group is strictly larger.
- **ENG-DIVERGE-002** (engine) Under the divergence stop, the engine MUST NOT seal or send any entry and MUST NOT take any duty.

### 22.3 Adoption by fingerprint

The user adopts another device's state only by choosing that device by the key fingerprint shown on that device's own screen (V1-10): `Operation::Adopt { dev, typed_fp }` (§28), where `typed_fp` is what the user read on the other device and typed or compared on this one. It reports `OpProgress` and ends with `OpDone`.

1. `typed_fp`, normalised as `seal.md` §5.2 normalises Crockford text, must equal `dev`'s key fingerprint in `M_K`; else the operation ends at once, in the same `handle` call, with `OpDone { ticket, result: Err("engine/fingerprint") }`, and nothing changes.
2. The snapshot: the newest `SnapRecord` written by `dev` whose seq `S'` is at or below `B` and at or above the lowest seq of the disagreement. If none exists, the engine shows "waiting for `dev` to write a snapshot" (that device owes one, §19.2 condition 2) and waits. The operation waits at this stage; §28.8's timeout (`engine/op_timeout`) applies.
3. Fetch and open it with `exact = true` and its `SnapInfo`, load its board (`SM::load`) and recompute its state hash (§18.3); a failure to open or to load raises `BlobFailure` with the writer, the blob id and the code, the operation ends with `Err` of that code, and nothing changes.
4. Rebuild this device's own `M_{S'}` by replay from a base at or below `S'`; its membership bytes must equal the snapshot's, and `SM::rules` of the snapshot's board must equal `M_{S'}.rules`; else the operation ends with `Err("engine/adopt_membership")` and nothing changes.
5. The confirmed state becomes the snapshot's board at `S'`; the board is replayed from `S' + 1` to `K` from the log; hash records from `S'` are rewritten; notice and refusal records from `S' + 1` are replaced by those of the replay (§13.4, ENG-REF-004); a checkpoint is written; the divergence stop ends; the operation ends with `OpDone { ticket, result: Ok(()) }`.

- **ENG-DIVERGE-003** (engine) The engine MUST adopt another state only after the user's typed fingerprint matches the chosen device's key fingerprint in its membership, and only from a snapshot written by that device.
- **ENG-DIVERGE-004** (engine) The engine MUST refuse an adoption whose snapshot's membership bytes differ from its own membership at the same seq, or whose board's rules version differs from that membership's `rules`.

## 23. Beats

### 23.1 When

A beat is a duty "daily while awake" (SYNC.md §4.2). An eligible device (§29.1) enqueues one beat record when it has no unsent beat record and:

1. `wall_ms − last_beat_ms ≥ 86,400,000`; or
2. its build's supported `rules`, `envelopes` or `hash_versions` differ from its own record in `M_K` (an update was installed); or
3. it holds a pending verdict and `wall_ms − last_beat_ms ≥ 3,600,000`.

`last_beat_ms` is set to `wall_ms` when the beat record is enqueued (persisted in the next `WriteMeta`).

### 23.2 Contents

`BeatInfo` (`seal.md` §11.4): `rules = SM::supported_rules()`, `envelopes = Sealer::supported_envelopes()`, `hash_versions = SM::hash_versions()`, `state` = this device's hash record at the largest aligned `S ≤ B`, absent when it holds no hash record at that `S` (`B < 64`, or the device bootstrapped from a snapshot at a seq at or above `S`, §18.3), `acks` = `pending_verdicts` (at most 64, ascending, distinct). The beat's head is its `based_on = (B, C_B)`. `pending_verdicts` are removed when the beat is seen accepted.

- **ENG-BEAT-001** (engine) An eligible engine MUST enqueue a beat when one of the three conditions of §23.1 holds and no beat record of it is unsent.
- **ENG-BEAT-002** (engine) A beat MUST carry exactly the contents of §23.2.
- **ENG-BEAT-003** (engine) The engine MUST start a new replica's `Meta` with `last_beat_ms = 0`.

### 23.3 What receivers derive

From an accepted beat of device `d`: `d`'s last seen (the membership's `beat_ms`, and `last_entry` for any entry), `d`'s supported versions (the membership's lists, used by §25), a report of `d`'s state hash at `state.seq` (§22.1), and `d`'s verdicts on `snap` entries (§19.5). None of these, nor their absence, raises an alarm except through §22.

## 24. Epochs and re-sealing

### 24.1 Applying a rotation

When the sealer accepts an `EpochRotated{n}` that wraps to this device, it learns `R_n` (`keys_changed`), and the engine persists the identity (D4). From the next seq on, the sealer classifies under `n` and seals under `n` (`seal.md` §13.8). Entries of other devices under `n − 1` are accepted or refused by the sealer's stale-epoch rule from the log alone (`seal.md` §13.9, ruling S6); the engine adds nothing.

### 24.2 The epoch gate

When `M_K.epoch` is above the epoch of any own record that has a frame and is not yet seen, the engine seals no new record until every such record has been seen in the log. Its old-epoch frames are resent first (they are sealed: their bytes never change), and their fate is known before anything else is sealed, so a refused one can be re-sealed ahead of everything after it.

- **ENG-EPOCH-001** (engine) The engine MUST NOT seal a new record while an own frame sealed under an older epoch than `M_K.epoch` has not yet been seen in the log.

### 24.3 Re-sealing

An own `cmd` entry refused with `seal/stale_epoch` or `seal/old_epoch` (sealed under an epoch the position no longer accepts) is **re-sealable** (SYNC.md §5.4, V2-8): the engine clears that record's `ctr`, `frame`, `frame_hash`, `epoch`, `ack` and `seen`, raises `reseals` by one, and persists it (D9). It does the same for every later own record that has been sealed and is not yet accepted. The records keep their `entry_id`, `at_ms`, `based_on` and `order`, so they are sealed again in their original order under new counters. Entries already accepted are applied and never sealed again. After a record's third re-seal is refused, it is finally refused with `engine/reseal_limit` (a refusal record, §13.3).

An own `beat` or `snap` entry that is refused is dropped (the duty re-evaluates). An own control entry that is refused fails its operation's stage (§28.8).

- **ENG-RESEAL-001** (engine) When an own `cmd` entry is refused with a re-sealable code, the engine MUST re-seal it and every later own record that is sealed and not yet accepted, in their original order, keeping their `entry_id`s.
- **ENG-RESEAL-002** (engine) The engine MUST NOT re-seal an own entry that was accepted, and MUST NOT re-seal an own entry refused with a code other than `seal/stale_epoch` or `seal/old_epoch`.

### 24.4 A revoked device

The device is **revoked** when any of these happens:

1. it accepts a `DeviceRevoked` naming itself (`source: log`, with the seq and the revoking author);
2. the relay refuses it with `403 relay/revoked` (`source: relay`; a relay answer, so it deletes nothing and proves nothing);
3. `seal/missing_root` for an entry after the seq at which `M` records it as revoked.

A revoked device sends nothing, takes no duty, keeps every record, and shows "this device was removed by <author> at <seq>" (or "the relay says this device was removed") with the action "re-pair and resubmit my unsent edits" (SYNC.md §5.4, V2-21). It reconnects at most once per 3,600,000 ms of `mono_ms` (§11.1) and then reads only: it emits no `Append`, `Admin` or blob upload (§10.1), and the runtime logs in again first, since a revocation at the relay raised its token generation (`relay.md` §5.2, §12.5). While the relay holds the device's row revoked, that login and every read are refused with `403 relay/revoked` (`relay.md` §8.2), and each such refusal leaves the revoked state as it is: a device revoked with `source: relay` reads nothing, does not classify the revoking entry, and keeps `source: relay` and the message "the relay says this device was removed" until one of the cases below ends it. So a mistaken `relay` source clears itself when the relay answers again; a `DeviceReinstated` naming it clears it as well, classified directly or after the missed roots of ruling S24 arrive (below).

- **ENG-EPOCH-002** (engine) A revoked device MUST keep its log, outbox, keys and notices, MUST NOT send, and MUST offer re-pair and resubmit.
- **ENG-EPOCH-004** (engine) A revoked device MUST emit `Connect` at most once per 3,600,000 ms of `mono_ms`, and while revoked MUST emit no `Append`, no `Admin` and no blob upload.

**Reinstatement (ruling S24).** The recovery tool reinstates a revoked device with the `reinstate` admin operation, whose `args` carry the wraps, to the device's `hpke_pk`, of the roots of epochs `first_epoch` to `E` (`seal.md` §16.2: `first_epoch` is the epoch the revoking entry's rotation set up; `E` is the epoch the reinstating entry is sealed under), and then with the `R`-shape entry whose `DeviceReinstated` names it and whose rotation to `E + 1` wraps to it like to every other recipient (`device.md` §15.8). The relay stores the wraps on the device's row and returns them in every `login` answer of that device until it is revoked again (`relay.md` §10.3 `wraps`, §12.5). The runtime logs in at every `Connect` while this device is revoked and sends `MissedRoots { first_epoch, wraps }` after each login whose answer carries them (`device.md` §10.3). While revoked, the device keeps its log and pauses classification at the first entry for which `classify` returns `seal/missing_root` (the paused seq, `K + 1`), without the local fault `engine/missing_root`. On `MissedRoots` while revoked, the engine calls `Sealer::take_missed_wraps(M_K, first_epoch, wraps)`; a `MissedRoots` with no wrap, more than 256 wraps or `first_epoch = 0` is `engine/runtime_protocol` and is ignored, and one that arrives while the device is not revoked is ignored (the relay returns the wraps until the next `revoke`). When the sealer reports a learned root, the engine persists the identity (D4), clears the revoked state (the stop `Revoked` and its banner action), and classifies and applies in seq order from the paused seq, from its log and then from the relay, before it seals or appends anything: the root of each later epoch is checked against the commitment of the rotation that set it as that rotation is classified (each rotation is sealed under the epoch before it; §6.1 `classify`), and the reinstating entry's own rotation gives `R_{E+1}`. A wrap that does not open, or a root that does not match its commitment, is the commitment halt (§16.1). A reinstated device does not pair again. If `M_K` still records this device as revoked when classification reaches the relay's head (a `reinstate` whose entry never landed), the device is revoked again with `source: log` and the revoking entry's seq and author, and pauses at the next `seal/missing_root`. The recovery tool does not offer reinstatement to a device whose revoking entry lies below `purge_seq` (lead ruling L20): such a device stays revoked and is offered re-pair and resubmit (§24.5), or the owner restores to a new board.

- **ENG-EPOCH-005** (engine) A device revoked in its membership MUST keep its log and MUST pause classification at the first entry for which the sealer returns `seal/missing_root`; on `MissedRoots` while revoked it MUST pass the wraps to `Sealer::take_missed_wraps` with `M_K`, and once the sealer reports a learned root it MUST persist the identity, clear the revoked state, and classify and apply in seq order from the paused seq up to the relay's head before it seals or appends anything.

### 24.5 Re-pair and resubmit

For a revoked device, a commitment halt, a stranded rejoin, or a closed board (§28.7, the stop `Closed` of §11.9):

1. `UserAction::ExportUnsent` makes the engine emit `Notify(Unsent { commands, from_device_no, origin_state })` with every `cmd` record that is not yet accepted, in `order`.
2. The runtime pairs this machine as a new device (new keys, new `dev`, new `device_no`; `seal.md` §17) and starts a new engine for it. The old identity's files stay until the user removes them (`device.md`).
3. Once the new engine has joined (ENG-BOOT-003), the runtime sends `Resubmit { ticket, commands, from_device_no, origin_state }`, where `origin_state` is the new identity's current origin state (the `origin_state` of its newest `cmd` outbox record, or the initial one for its own `device_no` when it has none, `device.md` §18.4), never the old identity's. The engine calls `SM::rebind(commands, from_device_no, d, view, origin_state)` with `d` its own `device_no` and `view` its pending view; a `rebind` error refuses every command with that code. Then it runs §9.1 steps 3 to 5 for each rebound command in order, with `at_ms` the `wall_ms` of the `Resubmit` input and `based_on = (B, C_B)` in the trial apply's `Ctx` and in the record, and with the origin state `rebind` returned as each record's `origin_state`. The commands' own instants (`complete.at` and the like) keep their compiled values; their original `at_ms` and `based_on` are not known (`Unsent` carries payloads only, §30.3). It answers `Notify(ResubmitReport { ticket, queued, refused: Vec<(index, code)> })`.

- **ENG-EPOCH-003** (engine) The engine MUST resubmit imported commands in their original order, rebound by `SM::rebind` to its own `device_no` from the new identity's current origin state, each through the trial apply of §9.1 with `at_ms` the `wall_ms` of the `Resubmit` and `based_on = (B, C_B)`, and MUST report every command it refused.

## 25. Rules upgrades

### 25.1 Writers never emit what the active rules forbid

The compile step emits only ops of `view.rules` (`core.md` CORE-RULES-004), and the trial apply refuses any other (`core/rules`), so no such record is queued (V2-1, V2-6, V3-2). The sealer seals only envelope versions `M_K.rules` allows (`seal.md` §14.3).

### 25.2 What the engine tells the interfaces

For every version `v` in `SM::supported_rules()` above `M_K.rules`, the status carries `VersionStatus { version: v, blockers, override_vote }`: `blockers` are the active members (computed as `seal.md` §13.10 with this device as the author) whose latest beat does not list `v`, each with name, key fingerprint, last beat and its rules list ("available once Galaxy is updated", SYNC.md §4.5); `override_vote` is `M_K.pending_override` when it names `v`.

- **ENG-RULES-001** (engine) The status MUST name, for every newer rules version this build supports, exactly the active members that block it, as `seal.md` §13.10 computes them with this device as the author.

### 25.3 The upgrade proposal (a duty)

When the largest such `v` has no blocker, the engine records `duties.upgrade = (v, wall_ms)` the first time it sees that, and an eligible device of rank `r` enqueues `RulesUpgrade { rules: v, mode: Normal }` once `wall_ms ≥` that time `+ r × 3,600,000`. A refusal (`seal/ctl_support` after a race, or `seal/ctl_target` because another device's upgrade won) clears the duty, which re-evaluates.

### 25.4 The override

`Operation::RulesOverride { v }` enqueues `RulesUpgrade { rules: v, mode: Override }`: one vote. A second device's user casts the second (`seal.md` §13.10). The engine never casts a vote on its own.

### 25.5 Activation and the unknown version

On the accepted entry that activates `v`: `SM::upgrade(v)` (ENG-SM-002), a checkpoint, a rebase. A build that lacks `v` halts (§16.1, `Rules`).

- **ENG-RULES-002** (engine) The engine MUST NOT enqueue a `RulesUpgrade` with mode `Override` except on the user's `Operation::RulesOverride`.

## 26. Timed windows

| Window | Length | Decided from | What a local clock only prompts |
|---|---|---|---|
| recovery-key change (§26.1) | 259,200,000 ms | `start_ms` (log clock after the proposal) and the confirming entry's `at_ms` (`seal.md` §13.11) | when the confirm action is offered |
| stale-epoch acceptance | 2,592,000,000 ms | the log clock (`seal.md` §13.9) | nothing |
| active member for `RulesUpgrade` | 2,592,000,000 ms | beats' `at_ms` (`seal.md` §13.10) | nothing |
| old roots kept after a purge | 2,592,000,000 ms | the log clock (§28.2) | nothing |
| pairing ticket | 600,000 ms | each side's monotonic clock and the relay's clock (`seal.md` §17.11) | the pairing screens |
| naming hold | 30,000 ms | `mono_ms` (§9.3) | |
| snapshot rank wait | 3,600,000 ms per rank | `τ_S` and `wall_ms` (§19.2) | |
| clock confirmation | 604,800,000 ms | `wall_ms` (§27.2) | |
| move-code lockout | 3,600,000 ms | `wall_ms` (§28.3) | |

Every window that decides whether an entry is valid is decided by the sealer from `at_ms` fields in the log (ruling S6). The engine's own windows only decide when this device offers or takes an action.

- **ENG-WINDOW-001** (engine) The engine MUST NOT accept, refuse, apply or skip any entry because of `wall_ms`, `mono_ms` or a relay time.

### 26.1 The 72-hour recovery-key change

SYNC.md §5.5, V1-2, `seal.md` §13.11.

| Step | Who | Engine |
|---|---|---|
| propose | a current device, on `Operation::ProposeRecoveryKey { sign_pk, hpke_pk }` (the new sheet is made by `device.md`/`recovery`) | enqueues a `P`-shape entry (`RecoveryChange { Propose }`) |
| the window | every device | on acceptance, a `RecoveryProposed` banner on every device, the proposer's too, with the new key's fingerprint and `confirm_after_ms = start_ms + 259,200,000`; it stays while the proposal is pending (ENG-BANNER-002) |
| confirm | a current device other than the proposer, on `Operation::ConfirmRecoveryKey`, offered only when `wall_ms ≥ confirm_after_ms` | enqueues a `K`-shape entry (`RecoveryChange { Confirm }` with `pending_recovery.sign_pk` and `hpke_pk`, §6.2; the sealer adds the rotation that wraps to the new key) appended as an ordinary entry (`via: Append`); once this device classifies it `Accept`, it sends `RecoveryRegisterSeq { seq, dev_r, sign_pk, frame_hash }` for it (`relay.md` §12.12), with `dev_r` and `sign_pk` those of the `recovery` entry the confirmation installed (§6.2), again after a lost answer until it is answered, and sends none for a refused entry; a refusal with `seal/ctl_window` (its clock was early) fails the stage, and the action is offered again |
| cancel | any current device on `Operation::CancelRecoveryKey`, or the recovery tool | enqueues a `P`-shape entry (`RecoveryChange { Cancel }`) |
| `direct` | the recovery tool only | the engine only sees it: banner `RecoveryChanged` |

If every device sleeps through the 72 hours, nothing happens on its own: the proposal stays pending, the banner stays on every device, and the change happens only when a user confirms on a device other than the proposer, or cancels. Revoking the proposer clears the proposal (SEAL-MEM-025). On a `confirm` or `direct`, a device that writes backups is told to confirm the new recovery fingerprint on its own screen before its next backup (`device.md`, SEAL-BKP-006).

- **ENG-WINDOW-002** (engine) The engine MUST offer `ConfirmRecoveryKey` only on a current device other than the proposer and only when `wall_ms ≥ start_ms + 259,200,000`.
- **ENG-WINDOW-003** (engine) The engine MUST NOT compose a recovery-key confirmation or cancellation without the user's operation.

## 27. The clock

### 27.1 Learning `relay_now`

Every welcome, `hello` answer and page carries `relay_now` (`relay.md` §10.4, §10.5, §11.3). On each, the engine records `skew = wall_ms − relay_now` and the `mono_ms` of the measurement. A measurement expires after 86,400,000 ms of `mono_ms` without a new one; with no valid measurement there is neither warning nor block.

### 27.2 The guard

SYNC.md §4.5, V2-20 as modified:

- `|skew| > 300,000` (five minutes): the status carries `clock_warning { skew_ms }`. It cannot be dismissed.
- `|skew| > 3,600,000` (an hour) without a valid confirmation: composing is blocked. `Enqueue` is refused with `engine/clock_unconfirmed`, and the engine composes no entry of its own (beats, duties, operations).
- A confirmation `(c_wall, c_skew)` is valid while `wall_ms − c_wall ≤ 604,800,000` and `|skew − c_skew| ≤ 300,000`. `UserAction::ConfirmClock` sets it to the current `(wall_ms, skew)`: it lifts the block, for seven days and while the skew stays within five minutes of what the user confirmed; the warning stays.

### 27.3 Relay time never stamps

`at_ms` is always the composer's own `wall_ms` (the facade's compile clock for commands), never corrected by `skew`. `relay_now` serves only the guard (RELAY-STORE-006). A hostile relay can at most make the device ask the user about the clock.

- **ENG-CLOCK-001** (engine) The engine MUST set every `at_ms` from the local clock alone and MUST NOT add a relay-derived offset to it.
- **ENG-CLOCK-002** (engine) The engine MUST refuse `Enqueue` with `engine/clock_unconfirmed`, and compose no entry itself, while the latest valid measurement's skew exceeds 3,600,000 ms in either direction and no valid confirmation exists.
- **ENG-CLOCK-003** (engine) The engine MUST show a clock warning while the latest valid measurement's skew exceeds 300,000 ms in either direction.

## 28. Operations

```rust
pub enum Operation {
    RestoreTo { seq: u64 },
    Purge,
    Move { host: String },                           // the runtime holds the new relay's create secret
    FollowAddress { host: String, door: [u8; 5], code: String }, // follow a move when the old relay is gone; code: the typed move code
    Revoke { dev: Dev, replace: bool },
    Skip { seq: u64 },
    ProposeRecoveryKey { sign_pk: Hash32, hpke_pk: Hash32 },
    ConfirmRecoveryKey,
    CancelRecoveryKey,
    RulesOverride { version: u32 },
    CloseBoard { host: String, door: [u8; 5] },      // reset trust: point the old board at the new one
    Adopt { dev: Dev, typed_fp: String },            // adopt another device's state by its fingerprint (§22.3)
}
pub struct OpRecord { pub ticket: u64, pub op: Operation, pub stage: u8, pub attempts: u8, pub started_ms: i64 }
```

Which platform may start which operation is `device.md`'s (SYNC_PLAN.md §2: restore to a point, purge, reset trust and recovery-key changes are desktop-only; adding, revoking and replacing need API 30 on Android). Every operation needs the device to be joined, `current` in `M_K`, not halted and not stopped, with three exceptions: `Skip` is also allowed under a crash halt, `Move` is also allowed under an evidence stop (it is the re-home of §21.3), and `Adopt` is also allowed under the divergence stop (it is that stop's repair, §22.3). At most one operation other than `Skip` and `RulesOverride` runs at a time (`engine/op_busy`). Each stage is persisted before its relay call (D10), so an operation resumes from its stage after a `Start`. The engine reports `Notify(OpProgress { ticket, stage })` and finally `Notify(OpDone { ticket, result })`. `Move`, `FollowAddress` and `CloseBoard` are refused with `engine/bad_host`, before any effect, when their `host` fails `seal.md` §5.4 (SEAL-ENC-006: at most 255 bytes, `:port` included).

- **ENG-OPS-007** (engine) The engine MUST refuse `Operation::Move`, `Operation::FollowAddress` and `Operation::CloseBoard` with `engine/bad_host`, emitting no effect other than the refusal, when their `host` does not match `seal.md` §5.4.

### 28.1 Restore to a point (`ResetTo`)

SYNC.md §6.6, V1-10.

**Offered targets.** `t` such that: `t < B`; `t ≥ M_K.purge_seq`; `t` greater than the `bootstrap_seq` of every current device (`seal.md` §13.6); this device holds a base at or below `t` and the log from it to `t`; `τ_t ≥ τ_B − 31,536,000,000` (365 days, so every active device still holds it, ruling of SYNC.md §6.6).

**Stages.** 1: replay from the newest base at or below `t` to `t`, with the current skip set, and compute the state hash `h_t` (§18.3) with `M_t`. 2: enqueue `ResetTo { seq: t, state_hash: h_t, hash_version: 1 }`. 3: done when it is accepted and applied.

**Every device**, at the board apply of an accepted `ResetTo` at seq `r` for `t`:

1. If this is not a replay (§17.3) and the device can rebuild `t` (a base at or below `t` and its log): replay to `t` and compare its state hash with the entry's. A difference raises `ResetMismatch` (author, `t`, both hashes), the entry is not applied, and the device enters the divergence stop (§22.2): its replay disagrees with the author's, and adoption resolves it.
2. If it cannot rebuild `t`: §19.9.
3. Otherwise: confirmed state `= SM::reset(state_t, state_{r−1})` (`core.md` §11.3; an error is the local fault `engine/local_fault`, §21.4, and the entry is not applied); a checkpoint; a rebase; banner `Reset`.

During a later replay through `r` (a Skip), the reset is applied without the comparison.

- **ENG-RESET-001** (engine) The engine MUST offer as restore targets only seqs that satisfy every condition of §28.1 "Offered targets".
- **ENG-RESET-002** (engine) Every device MUST apply a `ResetTo` by its own local replay to the target and MUST apply it only when its own state hash at the target equals the entry's, except during a replay.
- **ENG-RESET-003** (engine) A `ResetTo` MUST change only the board state, through `SM::reset`, and MUST NOT change the membership (`seal.md` §13.13).

### 28.2 Purge history

SYNC.md §4.7, V2-13, rulings S13. On the purging device:

1. `p = min(B + 1, min over every other current device d of (d's newest `based_on_seq` in `last_entry`) + 1)`: no current device is left behind the purge (a device whose newest report is older limits how much is purged; the interfaces show the date of `τ_{p−1}`). If `p ≤ M_K.purge_seq`: `engine/nothing_to_purge` (`seal.md` §13.6 requires `purge_seq < purge_before`).
2. Enqueue `CtlIntent::Rotate { revoke: [], reinstate: [], purge_before: Some(p) }`.
3. When it is accepted at seq `r` with new epoch `n`: `Admin { Purge { before_seq: p, epoch: n } }`; while the answer's `done` is below `purge_floor`, repeat once per UTC day (`duties.purge`). An answer `429 relay/delete_budget` (an `AdminResult` with that refusal) counts as that day's call and is repeated on the next UTC day, with no alarm.
4. `Notify(PurgeBackups { before_ms: τ_{p−1} })`: the runtime offers to delete older backups after the user confirms (`device.md`); and `Notify(ReuploadPicture { epoch: n })`: the runtime re-uploads the shared picture under the new epoch (`relay.md` §13.3).

`Engine::purge_preview()` (§37) returns `(p, τ_{p−1})` exactly as step 1 computes them from the current state, or `Err("engine/nothing_to_purge")` when step 1 would refuse, so the interfaces show the reach of a purge before the user confirms it.

On **every device**, at the board apply of an accepted rotation with `purge_before = p` at `r`:

1. write a base at `r` (D7);
2. when `log_floor < p`: `TruncateLogBelow(p, floor_chain)`, with `floor_chain` = `C_{p−1}` from the chain index when `p − 1 ≥ retained_from`, else `None` (§7.11), and `log_floor = p`; when `log_floor ≥ p` (the device bootstrapped between `p` and the purge rotation, lead ruling L27): no `TruncateLogBelow`, and `log_floor` and `floor_chain` stay as they are, so `log_floor` becomes the larger of the two; in both cases, drop hash records and `SnapRecord`s below `p`;
3. `DeleteBases` of every base below `r`;
4. when `τ_K ≥ τ_r + 2,592,000,000` (thirty days, after which no entry under the old epoch can be accepted, `seal.md` §13.9): `Sealer::forget_roots_below(n)` and `WriteIdentity`, which crypto-shreds the relay's copies (SYNC.md §4.7).

- **ENG-PURGE-001** (engine) The purging device MUST choose `purge_before` no higher than one more than the newest head reported by every other current device.
- **ENG-PURGE-002** (engine) On accepting a rotation with `purge_before`, every device MUST write a base at its seq before it truncates its log below `purge_before` and deletes its older bases.
- **ENG-PURGE-003** (engine) Every device MUST delete its epoch roots below the purge's new epoch once its log clock is 2,592,000,000 ms past the purge's rotation, and not before.
- **ENG-PURGE-004** (engine) `Engine::purge_preview` MUST return the `p` and `τ_{p−1}` that §28.2 step 1 computes from the engine's current state, and `Err("engine/nothing_to_purge")` exactly when that step would refuse.

### 28.3 Move board, the move code, and following a move

SYNC.md §6.7, `relay.md` §12.6, §12.7, `seal.md` §12.7, §15.4.

**On the device that moves the board** (`Operation::Move { host }`):

1. `Admin { Door { host } }` on the new relay (the runtime uses its create secret) gives the new door `d'`.
2. **Fence.** If the current relay answers and this device is under no evidence stop (§21.2): enqueue `CtlIntent::Fence { host, door: d', home_gen: M_K.home_gen + 1 }` with `via: Fence`; it is sent with `Admin { Fence { frame } }`; on success it is sequenced at `f`. The device then reads the old relay (§12.2; it is fenced, so `f` is its final head) until `L ≥ f`, classifying and applying as usual, and only then goes on to stage 3, so that the seeded tail ends at or after the `Fence`. If the current relay answers but this device is under an evidence stop, it cannot carry the old relay's entries from the stop's cut on: it fences the old relay without a frame, `Admin { Fence { host, door: d', home_gen: M_K.home_gen + 1, frame: None } }`, and goes on to stage 3 as for a relay that is gone. If the user says the old relay is gone, it goes on to stage 3 at once. In these last two cases no `Fence` record exists yet; it is enqueued in stage 5. The move's home generation `h` is `M_K.home_gen` once the old relay's `Fence` of this move is accepted (the first case), and `M_K.home_gen + 1` otherwise.
3. **Seed.** `S` = the newest usable snapshot this device acknowledged, or wrote and another device acknowledged; `first_seq = min(S + 1, max(1, M_L.purge_seq, L + 1 − 16,384))`, so that every Skip target §17.4 permits after the move is on the new relay (`seal.md` §16.2 `seed`); if this device does not hold every frame from that `first_seq` to `L` (it joined after `L + 1 − 16,384`), `first_seq` is instead the lowest `x + 1 ≤ S + 1` such that it holds every frame from `x + 1` to `L` and a base at or below `x`, and targets below it are reached only as §17.2 states. First the device seals a fresh snapshot of its own state at `S`, with itself as writer: the material of §19.4 step 1 from its base at `S`, or from a replay to `S` from a base below it, whose state hash must equal its hash record at `S` (else the local fault `engine/local_fault`), then `Sealer::seal_snapshot(M_K, material)`. It never uploads another writer's blob, nor a blob fetched from another relay: the new relay records the uploader as a blob's `author` (`relay.md` §10.7), and the writer is bound into the sealed bytes (`seal.md` §20.2). Then `Admin { Seed { … } }` on the new relay: `home_gen = h`; `last_seq = L`; `chain_before = C_{first_seq − 1}`; `chain_last = C_L`; `blob_id` = the fresh blob's id; the entries (`SeedDevice`, ascending by `dev`) are every device of `M_L` (`role = 1`; `status` 1 when current, 2 when revoked) and every recovery identity of `M_L.recovery` (`role = 2`; `status` 1 for the current one, whose `until_seq` is 0, and 2 for a retired one), each with its `sign_pk` (§6.2) and with `last_ctr` its counter in `M_{first_seq − 1}` (0 for one added at or after `first_seq`), `M_{first_seq − 1}` being rebuilt by replay from a base at or below `first_seq − 1`; a revoked device or a retired recovery identity is listed only when it authors a frame at or after `first_seq` (`relay.md` §12.7). After a `Start` in this stage, the device seals and seeds again (a new seed replaces an unfinished one, `relay.md` §12.7).
4. `Admin { SeedTail }` with the frames `first_seq` to `L` from its log, at most 256 frames and 1,048,576 bytes per request, resuming from the head of a `relay/not_next`.
5. `meta.relay = (host, d', h)`; `Connect`. If the old relay did not sequence this move's `Fence` (the second and third cases of stage 2), the device now enqueues `CtlIntent::Fence { host, door: d', home_gen: h }` with `via: Append`, which the send loop sends as an ordinary `append` on the new relay. It is never sent as a `fence` admin operation there: the new relay was seeded with `home_gen = h` and refuses it with `409 relay/home_gen` (`relay.md` §12.6).
6. Once the session on the new relay is `Live`: `Blob { Upload { purpose: 1, seq: S, blob } }` with the fresh blob of stage 3 (sealed again, with a new id, after a `Start`), and on success a `snap` record for it, as §19.4 step 4 states (§19.4 step 5 applies). Three failed uploads give the snapshot up, as for the snapshot duty (§19.2); the move does not fail for it.
7. When its `Fence` is seen in its log at `f` (the old relay's seq in the first case of stage 2, the new relay's otherwise): `Notify(MoveCode { text: Sealer::move_code(f, H(Fence frame)) })`, shown until acknowledged; and `Notify(MoveReport { carried_through: L, old_relay_answered })`, with `L` the seed's `last_seq`: when the old relay did not sequence the `Fence`, entries sequenced there after `L` were not carried (V2-9).

**On every other device:**

1. An accepted `Fence` (from the old relay's log): banner `Moved`, status `move_pending { host, home_gen, fence_seq }`; sending to the old relay stops (it is fenced).
2. `UserAction::MoveCode { text }`: refused with `engine/move_locked` while `wall_ms` is below the lockout, and with `engine/move_no_fence` while no accepted `Fence` awaits a code (no `move_pending`); otherwise `Sealer::check_move_code` against the latest accepted `Fence`: a failure (`seal/move_code`) raises the count and, at 5 failures in a row, locks entry for 3,600,000 ms; a success sets `meta.relay` from the `Fence` item, resets the count, and connects. In the same `handle` call the engine emits `Notify(MoveCodeResult { result })`: `Ok(())` on a success, else `Err` with the code.
3. On the new relay, the usual checks apply: the device reads after its log head; a relay head below it is `RelayBehind`, which this device repairs by restore tail (§21.3), carrying what it holds beyond the seeded tail; a relay whose base is above its log head answers a `truncated` page and leads to a rejoin (§19.7). Its outbox is reconciled and resubmitted by §11.3.
4. When the old relay is gone and the device holds no `Fence`: `Operation::FollowAddress { host, door, code }`, `code` being the move code the user typed with the address. While `wall_ms` is below the lockout of ENG-MOVE-002 it ends with `engine/move_locked`. Otherwise the engine reads from the new relay as a candidate, without sending; it follows only if the log there holds an accepted `Fence` at `f` naming exactly that host and door and `Sealer::check_move_code(f, H(Fence frame), code)` accepts `code`. No such `Fence`: the operation ends with `engine/move_no_fence`; a wrong code: it ends with `seal/move_code`, which counts toward the lockout of ENG-MOVE-002 like a wrong `UserAction::MoveCode`. On either failure its target stays the old relay.

A move is never followed automatically (SYNC.md §6.7).

- **ENG-MOVE-001** (engine) A device MUST change its relay target to a `Fence`'s relay only after the user typed a move code that the sealer checked against the latest accepted `Fence`.
- **ENG-MOVE-002** (engine) After 5 wrong move codes in a row, the engine MUST refuse further codes for 3,600,000 ms of its local clock.
- **ENG-MOVE-003** (engine) The moving device MUST seed the new relay with every device and recovery identity of `M_L` that §28.3 stage 3 lists, each with its `sign_pk` and its counter at `first_seq − 1`, and the frames from `first_seq` to its log head, each frame unchanged, with `first_seq` computed as §28.3 stage 3 states.
- **ENG-MOVE-004** (engine) While following an address without a `Fence` in its own log, the engine MUST NOT send any entry to the candidate relay.
- **ENG-MOVE-005** (engine) When the old relay did not sequence the move's `Fence`, the moving device MUST send that `Fence` as an `append` to the new relay after the seed, and MUST NOT send it to the new relay as a `fence` admin operation.
- **ENG-MOVE-006** (engine) When the old relay sequenced the move's `Fence` at `f`, the moving device MUST NOT send `seed` before its log head has reached `f`.
- **ENG-MOVE-007** (engine) The moving device MUST upload to the new relay only a snapshot blob it sealed itself in §28.3 stage 3, and MUST NOT upload a blob fetched from another relay.
- **ENG-MOVE-008** (engine) The engine MUST answer every `UserAction::MoveCode` with exactly one `Notify(MoveCodeResult)` in the same `handle` call, carrying `Ok(())` when the code was accepted and otherwise the code of §28.3 step 2 that refused it.

### 28.4 Adding a device (the existing device's part of pairing)

SYNC.md §5.3, `seal.md` §17.1 step 9 and SEAL-PAIR-010.

`PairRegister { ticket, device }` from the existing-device machine. Refused with `engine/halted`, `engine/not_eligible` (not current, stopped, not `Live`) or `engine/pair_busy` (one pairing at a time). Otherwise:

1. `h = B`. The material at `h`: the confirmed state's bytes, `M_h`, `C_h` and the state hash. `Sealer::seal_snapshot(M_K, material)`; `Blob { Upload { purpose: 2, seq: h } }`. A `SealerError` from `seal_snapshot` (`seal/snapshot_large`) ends the pairing before any upload or entry: `Notify(PairFailed { ticket, code })` with that code.
2. When the `BlobResult` of that upload arrives with `Uploaded`, enqueue `CtlIntent::AddDevice { dev, sign_pk, hpke_pk, name, transcript, bootstrap_seq: h }` with `based_on = (h, C_h)` and `via: Register { dev, sign_pk }`. A `BlobResult` with a `RelayRefusal` ends the pairing before any entry: nothing is enqueued, and the engine answers `Notify(PairFailed { ticket, code })` with the refusal's code.
3. When the send loop reaches it: if `M_K.epoch` differs from the epoch of the snapshot uploaded in step 1 (a rotation was classified after `h`), the engine drops the record unsent (`Outbox[Drop]`), sends nothing to the relay, and answers `Notify(PairFailed { ticket, code: "seal/pair_raced" })`; the user pairs again. Otherwise it seals the record and sends `Admin { Register { frame } }`. On success at seq `a`: `Notify(PairRegistered { ticket, head: (h, C_h), snapshot: SnapInfo, added_seq: a })`; the runtime hands the pairing machine the token from the admin answer and, from the sealer, the roots of the snapshot's epoch and of the epoch before it when that epoch is 2 or more, never a later root (`seal.md` §17.8); the machine posts slot D.
4. A refusal of the entry (a rotation raced it, `seal/stale_epoch`; a device cap) first sends `Admin { Revoke { dev } }` for the new device, so that the relay drops the row and token `register` created for it (`relay.md` §12.4); a `409 relay/target` answer (already revoked) counts as done. Then it answers `Notify(PairFailed { ticket, code })`, and the user pairs again (`seal.md` `seal/pair_raced`).

- **ENG-OPS-001** (engine) The engine MUST compose the `DeviceAdded` of a pairing with `based_on` and `bootstrap_seq` equal to the seq of the snapshot it uploaded for the bundle, and MUST send it with the `register` admin operation.
- **ENG-OPS-004** (engine) When the `DeviceAdded` of a pairing that it sent with `register` is refused, the engine MUST send the `revoke` admin operation for that device before it reports the pairing failed.
- **ENG-OPS-006** (engine) The engine MUST NOT seal a pairing's `DeviceAdded` under an epoch other than that of the snapshot it uploaded for the bundle: when `M_K.epoch` differs from it, the engine MUST drop the record unsent and report the pairing failed with `seal/pair_raced`.
- **ENG-OPS-008** (engine) The engine MUST enqueue a pairing's `DeviceAdded` only after the `BlobResult` `Uploaded` of that pairing's snapshot, and MUST enqueue nothing and report the pairing failed with the refusal's code when the upload is refused.

### 28.5 Revoking and replacing a device

`Operation::Revoke { dev, replace }` (SYNC.md §5.4, V1-3). Refused with `engine/revoke_target` when `dev` is not a current device of `M_K`, is this device, or is the last current device (ruling S14).

1. After the `WriteMeta` of stage 1 (D10): `Admin { Revoke { dev } }` (the relay drops the target's tokens at once, `relay.md` §12.4).
2. When its `AdminResult` is `Ok`, or is the refusal `409 relay/target` (the relay already holds no current row for the target, for example after a `Start` repeated stage 1 whose call had committed): enqueue `CtlIntent::Rotate { revoke: [dev], reinstate: [], purge_before: None }` as §9.2 states, in one `Persist` holding, in order, the `WriteMeta` of stage 2 and the `Outbox` op with the `Put` of that record. Any other refusal ends the operation before any entry: nothing is enqueued, and `Notify(OpDone { ticket, result: Err(code) })` carries the refusal's code.
3. Accepted: done. With `replace`, `Notify(ReplaceReady { dev })`: the user pairs that machine again as a new device.
4. Refused with `seal/ctl_epoch` or `seal/ctl_wraps` (another rotation or a membership change raced it): if `dev` is now revoked in `M_K`, done; otherwise compose again from stage 2, after every such race, until the entry is accepted or `dev` is revoked in `M_K`, bounded only by the operation timeout of §28.8 (the relay has already locked the target out, `relay.md` §12.4, so the log must follow). Any other refusal: failed.

- **ENG-OPS-002** (engine) The engine MUST send the `revoke` admin operation before it enqueues the rotation that revokes the device, and MUST compose the rotation again after every race until the target is revoked in its membership or the operation times out (§28.8).
- **ENG-OPS-009** (engine) The engine MUST enqueue the rotation of a `Revoke` operation only after the `revoke` admin answer is `Ok` or `409 relay/target`, in one `Persist` with the operation's stage 2, and on any other refusal MUST enqueue nothing and end the operation with the refusal's code.

### 28.6 Recovery-key changes

§26.1. `ConfirmRecoveryKey` is offered only as ENG-WINDOW-002 states; the `K`-shape entry is appended as an ordinary entry (stage 2), and `RecoveryRegisterSeq` for it, once this device has classified it `Accept`, is stage 3.

### 28.7 Reset trust

SYNC.md §5.4: a new board from a trusted device's state. The runtime creates the new board from this device's confirmed state (a new recovery seed and sheet, and a genesis carrying the current board bytes as an imported board; `device.md`). Then `Operation::CloseBoard { host, door }` on the old board's engine enqueues `CtlIntent::Fence { host, door, home_gen: M_K.home_gen + 1 }` with `via: Fence`. A device that follows it (§28.3) finds it is no member of the board there (`relay/unauthorized` at login), enters the stop `Closed` (§11.9, ENG-SESS-007), and the engine offers re-pair and resubmit into the new board (§24.5).

### 28.8 Operation records and failures

An operation's control entry that is refused fails its stage with the refusal's code, unless the operation's section says it composes again. `Notify(OpDone { ticket, result: Err(code) })` reports a failure. An operation whose stage cannot proceed for 30 days of `wall_ms` is ended with `engine/op_timeout`.

- **ENG-OPS-003** (engine) The engine MUST persist each operation's stage before the relay call of that stage, and MUST resume every operation from its persisted stage after a `Start`.
- **ENG-OPS-005** (engine) The engine MUST send `recovery_register` for a recovery-key confirmation only as `RecoveryRegisterSeq`, only after it has classified the `K`-shape entry `Accept`, and MUST NOT send it for a refused entry.

## 29. Duties without an always-on device

### 29.1 Eligible

A device is **eligible** for a duty (ruling S8: "any current, unhalted device") when all hold:

1. **current**: it has joined, its own record in `M_K` is `current`, and it is caught up: the session is `Live` and `L` equals the relay head of a welcome or page received within the last 600,000 ms of `mono_ms`;
2. **unhalted**: no halt (§16) and no stop (§21, §22, §24.4);
3. composing is not blocked by the clock (§27.2).

### 29.2 The duty order

The current devices of the membership the duty names (`M_S` for a snapshot, `M_K` otherwise), sorted by `beat_ms` descending, then by `dev` ascending as unsigned bytes. A device's rank is its 0-based position. The order rests on the log, so every device computes the same ranks; the device that beat most recently, and so is most likely awake, goes first.

- **ENG-DUTY-001** (engine) The engine MUST compute ranks exactly by the order of §29.2.
- **ENG-DUTY-002** (engine) The engine MUST take a duty only while it is eligible as §29.1 defines it, except restore tail, which needs only §29.1 item 2 apart from the evidence stop being repaired, and a joined, current record.

### 29.3 The duties

| Duty | Who | When | Section |
|---|---|---|---|
| beat | every eligible device | daily, on an update, or with verdicts to send | §23 |
| snapshot | an eligible device, by rank | when due at an aligned seq | §19.2 |
| verdicts | every device that applied the snapshot's seq | in its next beat | §19.5 |
| compaction | an eligible device, by rank, not the snapshot's writer | once per UTC day | §20 |
| restore tail | any device holding more than a rolled-back relay | on `RelayBehind` evidence | §21.3 |
| rules-upgrade proposal | an eligible device, by rank | when a newer version has no blocker | §25.3 |
| purge continuation | the purging device | daily until the relay reports done | §28.2 |
| backups | a desktop that is awake (`device.md`) | nightly and monthly | — |
| the shared picture after a purge | the purging device's runtime | after the purge | §28.2 |

`DutyState` in `Meta`: `snapshot: Option<(u64, i64, u8)>` (seq, `τ_S`, failures), `last_compact_day: i64` (UTC day number of `wall_ms`), `purge: Option<(u64, u32)>` (`before_seq`, epoch), `upgrade: Option<(u32, i64)>`.

### 29.4 A week with only the phone awake

| Duty | What happens |
|---|---|
| beats | the phone beats daily; the desktops' last seen ages, which is shown and raises nothing |
| snapshots | the phone writes them (its rank wait passes while the others sleep); they stay unacknowledged until a desktop wakes and acknowledges them |
| compaction | possible only on a snapshot by another writer that the phone acknowledged; otherwise it waits, which costs nothing (it concerns entries a year old) |
| pairing | works: the bundle carries a fresh snapshot of the phone's state (§28.4) |
| restore tail | the phone repairs a rollback with what it holds |
| upgrade proposals | the phone proposes when no active member blocks |
| backups | none: they wait for a desktop (ruling S8) |
| purge, restore to a point, reset trust, recovery-key change | not started: desktop-only (SYNC_PLAN.md §2); a pending recovery-key proposal stays pending, with its banner on the phone, until a desktop confirms or cancels it |

Nothing is lost and nothing blocks except backups.

## 30. What the user interfaces are told

The engine gives the data; `desktop` and `android` choose the words (SYNC_PLAN.md §6.9, §6.10). Everything here comes from `Engine::status()`, `Engine::pending()`, `Engine::notices()` and the `Notify` effects (§37).

### 30.1 `Status`

| Field | Type | Meaning |
|---|---|---|
| `session` | `Offline`, `Connecting`, `Reconciling`, `Live`, `Rejoining` | §11.1, §19.7 |
| `relay_host`, `push` | string, bool | the target and its capability |
| `last_synced_ms` | `Option<i64>` | `wall_ms` when `L` last equalled a relay head: "synced 14:05" (V3-6) |
| `unreachable_since_ms` | `Option<i64>` | the first `Disconnected { unreachable: true }` since the last `Live`: "relay unreachable since 14:05" |
| `paused_until_ms` | `Option<i64>` | an append pause (§10.4) or a quota pause from `Disconnected` |
| `queued`, `oldest_queued_ms` | u32, `Option<i64>` | records neither acknowledged nor seen, and the oldest one's `at_ms`: "· 3 waiting" |
| `doomed` | u32 | records marked "will be refused" |
| `queue_large` | bool | §9.4 |
| `head`, `board_seq` | `Head`, u64 | `L` with `C_L`, and `B` |
| `fingerprint` | string | `Sealer::sync_fingerprint` of `(B, C_B)` (`seal.md` §15.3) |
| `clock` | `{ skew_ms: Option<i64>, warning: bool, blocked: bool, confirmed_until_ms: Option<i64> }` | §27 |
| `halt` | `Option<{ kind, seq, author, author_name, author_fp, since_ms, code, skip_offer: Option<{ seq, here: bool }> }>` | §16; `here` is true when this device may compose the Skip (§17.1) |
| `stops` | list of `{ kind, since_ms, alarm: Option<u64>, action }` | §11.9, §19.7 step 1, §21, §22, §24.4; `action` is one of `RestoreTail`, `ReHome`, `Adopt`, `RePair`, `ReplaceDevice`, `UseThisCopy`, `Restart`, `ConfirmRejoin`, `Wait`; `ConfirmRejoin` only with the kind `Rejoin`, while `session` is `Rejoining` |
| `alarms` | list of `AlarmRecord` | unresolved alarms with their evidence (§21.1) |
| `banners` | list of `Banner` | unacknowledged banners (§14.2) |
| `devices` | list of `{ dev, name, key_fp, device_no, status, is_self, last_beat_ms, last_entry_ms, rules, bootstrap_seq }` | last seen, never an alarm (V2-10) |
| `rules` | `{ active: u32, versions: Vec<VersionStatus> }` | §25.2 |
| `recovery_change` | `Option<{ proposer, proposer_name, key_fp, start_ms, confirm_after_ms, can_confirm: bool }>` | §26.1 |
| `moving` | `Option<{ host, home_gen, fence_seq, tries_left: u32, locked_until_ms: Option<i64> }>` | §28.3 |
| `ops` | list of `{ ticket, kind, stage, started_ms }` | §28 |
| `notices_unread`, `refusals_unread` | u32 | the badge (V2-16) |
| `warnings` | list of `RestoredCopy { entries }`, `ContinuityUnchecked { from }`, `BasedOnUnchecked { count }` | informational; each is shown while its `Meta` field is set (`restored_copy`, `continuity_unchecked`, `based_on_unchecked` above 0), until its acknowledgement (§30.4) |

The table's fields are those of `Status`:

```rust
pub enum SessionState { Offline, Connecting, Reconciling, Live, Rejoining }
pub struct ClockStatus { pub skew_ms: Option<i64>, pub warning: bool, pub blocked: bool, pub confirmed_until_ms: Option<i64> }
pub struct SkipOffer { pub seq: u64, pub here: bool }
pub struct HaltView { pub kind: HaltKind, pub seq: u64, pub author: Dev, pub author_name: String, pub author_fp: String,
                      pub since_ms: i64, pub code: String, pub skip_offer: Option<SkipOffer> }
pub enum StopAction { RestoreTail, ReHome, Adopt, RePair, ReplaceDevice, UseThisCopy, Restart, ConfirmRejoin, Wait }
pub struct StopView { pub kind: StopKind, pub since_ms: i64, pub alarm: Option<u64>, pub action: StopAction }
pub struct DeviceRow { pub dev: Dev, pub name: String, pub key_fp: String, pub device_no: u16, pub status: DeviceStatus,
                       pub is_self: bool, pub last_beat_ms: i64, pub last_entry_ms: Option<i64>, pub rules: Vec<u32>,
                       pub bootstrap_seq: u64 }
pub struct Blocker { pub dev: Dev, pub name: String, pub key_fp: String, pub last_beat_ms: i64, pub rules: Vec<u32> }
pub struct VersionStatus { pub version: u32, pub blockers: Vec<Blocker>, pub override_vote: Option<(u32, Dev, u64)> }
pub struct RulesStatus { pub active: u32, pub versions: Vec<VersionStatus> }
pub struct RecoveryChangeView { pub proposer: Dev, pub proposer_name: String, pub key_fp: String, pub start_ms: i64,
                                pub confirm_after_ms: i64, pub can_confirm: bool }
pub struct MovingView { pub host: String, pub home_gen: u32, pub fence_seq: u64, pub tries_left: u32,
                        pub locked_until_ms: Option<i64> }
pub struct OpStatus { pub ticket: u64, pub kind: String, pub stage: u8, pub started_ms: i64 }  // kind: the Operation variant in snake_case
pub enum StatusWarning { RestoredCopy { entries: u32 }, ContinuityUnchecked { from: u64 }, BasedOnUnchecked { count: u32 } }
pub struct Status {
    pub session: SessionState, pub relay_host: String, pub push: bool, pub last_synced_ms: Option<i64>,
    pub unreachable_since_ms: Option<i64>, pub paused_until_ms: Option<i64>, pub queued: u32,
    pub oldest_queued_ms: Option<i64>, pub doomed: u32, pub queue_large: bool, pub head: Head, pub board_seq: u64,
    pub fingerprint: String, pub clock: ClockStatus, pub halt: Option<HaltView>, pub stops: Vec<StopView>,
    pub alarms: Vec<AlarmRecord>, pub banners: Vec<Banner>, pub devices: Vec<DeviceRow>, pub rules: RulesStatus,
    pub recovery_change: Option<RecoveryChangeView>, pub moving: Option<MovingView>, pub ops: Vec<OpStatus>,
    pub notices_unread: u32, pub refusals_unread: u32, pub warnings: Vec<StatusWarning>,
}
```

`VersionStatus` is §25.2's (`override_vote` is `M_K.pending_override` when it names `version`); `relay_host` is empty before a relay target is set.

`Engine::pending()` lists the unapplied records for the marks on pending rows: `{ entry_id, provisional_seq, op, target, doomed: Option<code>, acknowledged: bool }` (V3-6). `Engine::notices()` lists the notice and refusal records with their dismissed flag; the interfaces look up names by id in the current state (CORE-GEN-010).

- **ENG-UI-001** (engine) `Engine::status()` MUST return every field of §30.1 computed from the engine's current state.

### 30.2 What each screen needs

| Screen | Data |
|---|---|
| the sync line (desktop status, the phone page's masthead) | `session`, `last_synced_ms`, `unreachable_since_ms`, `paused_until_ms`, `queued`, `halt`, `stops`; it cannot be silenced (V3-6) |
| the device list | `devices`, `fingerprint`, the device's own key fingerprint |
| banners | `banners`, acknowledged one by one |
| alarm evidence | `alarms` with `Evidence`, and the `stops` action |
| the halted view | `halt` (frozen since, the entry's author, the Skip offer) and the pending view |
| the notices list, the notes merge view | `notices()`, a refusal with `base` and `current` (`core.md` §10.7) |
| "will be refused" marks | `pending()` |
| the clock prompt | `clock` |
| what blocks a rules upgrade | `rules.versions[].blockers` |
| the recovery-key change | `recovery_change` |
| following a move | `moving`, and the move code from `Notify(MoveCode)` on the moving device |

### 30.3 Notifications

```rust
pub enum Notification {
    Enqueued { ticket: u64, entry_id: Option<[u8; 16]>, provisional_seq: Option<u64> },
    EnqueueRefused { ticket: u64, code: &'static str },
    ViewChanged { version: u64 },
    StatusChanged,
    NoticesAdded { notices: u32, refusals: u32 },
    BannersChanged,
    AlarmRaised { id: u64 },
    OpProgress { ticket: u64, stage: u8 },
    OpDone { ticket: u64, result: Result<(), &'static str> },
    PairRegistered { ticket: u64, head: Head, snapshot: SnapInfo, added_seq: u64 },
    PairFailed { ticket: u64, code: &'static str },
    MoveCode { text: String },
    MoveReport { carried_through: u64, old_relay_answered: bool },
    MoveCodeResult { result: Result<(), &'static str> },   // the answer to UserAction::MoveCode (§28.3)
    Unsent { commands: Vec<Vec<u8>>, from_device_no: u16, origin_state: Vec<u8> },
    ResubmitReport { ticket: u64, queued: u32, refused: Vec<(u32, &'static str)> },
    ReplaceReady { dev: Dev },
    PurgeBackups { before_ms: i64 },
    ReuploadPicture { epoch: u32 },
    RecoveryKeyChanged { key_fp: String },
    ReadyToExit,
}
```

`Unsent.commands` are the commands' encoded payloads (`SM::encode`). They carry no `at_ms` or `based_on`: a resubmitted command is stamped with the resubmitting device's clock and head (§24.5). `Unsent.origin_state` is the old identity's and is not used for the resubmit.

### 30.4 User actions

```rust
pub enum UserAction {
    AckBanner { id: (u64, u8) }, DismissNotice { seq: u64, index: u32 }, DismissAlarm { id: u64 },
    ConfirmClock, MoveCode { text: String },
    ConfirmRejoin, UseThisCopy, ExportUnsent, AckContinuity, AckRestoredCopy, AckBasedOnUnchecked,
}
```

`DismissAlarm` hides an alarm that does not stop the engine; an alarm behind a stop is resolved only by its repair.

- **ENG-UI-002** (engine) The engine MUST ignore `DismissAlarm` for an alarm whose kind stops it while that stop is in force.
- **ENG-UI-003** (engine) The engine MUST show `ContinuityUnchecked { from: S }` in `status.warnings` from the success of a rejoin at `S` until `UserAction::AckContinuity`, across restarts, and at no other time.
- **ENG-UI-004** (engine) The engine MUST show `RestoredCopy { entries }` in `status.warnings`, with `entries` the count `meta.restored_copy` holds, from the end of a restored-copy catch-up (§11.3 step 3) until `UserAction::AckRestoredCopy`, across restarts, and at no other time.
- **ENG-UI-005** (engine) The engine MUST show `BasedOnUnchecked { count }` in `status.warnings`, with `count` the value of `meta.based_on_unchecked`, while that value is above 0, across restarts, and MUST set it to 0 on `UserAction::AckBasedOnUnchecked`.

## 31. The simulator

### 31.1 Shape

`taskdeck_sync::sim` (feature `sim`) runs `n` devices (2 to 4), each an `Engine<ToyMachine, FakeSealer>` with a model runtime, against one or two model relays, on one virtual clock, from one `u64` seed. Two runs with the same seed and configuration produce the same trace byte for byte.

```rust
pub struct SimConfig { pub devices: u8, pub days: u32, pub faults: FaultSet, pub ops: OpSet, pub rules2: Vec<bool> }
pub struct SimReport { pub seed: u64, pub trace_sha256: [u8; 32], pub violations: Vec<Violation>, pub stats: SimStats }
pub fn run(seed: u64, config: &SimConfig) -> SimReport;
```

### 31.2 Randomness

SplitMix64, on wrapping 64-bit arithmetic:

```text
next():  state = state + 0x9E3779B97F4A7C15
         z = state
         z = (z XOR (z >> 30)) × 0xBF58476D1CE4E5B9
         z = (z XOR (z >> 27)) × 0x94D049BB133111EB
         return z XOR (z >> 31)
below(n), n ≥ 1:   next() mod n
chance(p), p in parts per million:   below(1,000,000) < p
```

The master generator starts with `state = seed`. It yields, in this order: one seed per device in index order (its `FakeSealer`'s), then the seeds of the network, the fault and the workload generators. Every random choice of the simulator draws from one of these, in event order.

### 31.3 The model relay

The model relay implements, over in-memory tables, `relay.md` §7 (the tables it needs: board, devices, log, blobs, parts), §8.1 (board states), §9 (the batch, P1 to P7 with the fake signature, T1 to T5 with the budget), §10.4 `hello`, §10.5 `read` (selection, the page fields, `truncated`), §10.7 and §13 (blobs and their retention), §11.3 and §11.4 (hello, welcome, `second`, `displaced`, push to every ready socket), §12.3 to §12.10 and §12.12 (`register`, `revoke`, `reinstate`, `fence`, `seed` and `seed-tail`, `compact`, `restore_tail`, `purge`, `recovery_register`, with their own checks) and §14.1 and §14.2 (the append and delete budgets). It does not model the gate, the door, token MACs, nonces or admin signatures: an admin request passes `relay.md` §12.1 steps A4 and A6 by construction, and A5 is checked against its rows. Its clock is the virtual clock plus a per-relay offset.

- **ENG-SIM-001** (engine) The model relay MUST produce, for every request it models, the results, pages, pushes and errors that `relay.md` §9 to §14 give for its state, and the conformance cases of `relay.md` §21 that cover those sections MUST pass against it.

### 31.4 The model runtime

Per device: a persist queue with a latency drawn per write; durable storage holding exactly the writes whose `Persisted` was delivered, plus, after a crash, the write in progress if the fault generator so chooses (ENG-PERSIST-004); ticks at `ScheduleTick` times; `Connect` after a full-jitter backoff from 1 to 60 s (`device.md`); a session id per process; `Start` with `Loaded` read from durable storage after a restart delay. It compiles nothing: the workload sends `Enqueue` with toy commands and `at_ms` equal to the device's clock.

### 31.5 Workload

Each device draws its next action time (exponential, mean 10 minutes of virtual time while "awake"; devices sleep and wake on drawn schedules, one of them the phone pattern of §29.4) and its action: a toy command (`Set`, `Add`, `Del`, `Create` with a hold followed by `Name` or `Forget`, `Fail`; `Panic` only in poison configurations), or an operation from `OpSet` (revoke and re-pair, Skip when a device is halted, a rules upgrade with `rules2`, purge, move to the second model relay, restore to a point, a recovery-key proposal and confirmation).

### 31.6 Faults

| Fault | What it does | Parameter range |
|---|---|---|
| loss | a message in either direction is dropped | 0 to 200,000 ppm |
| duplication | a message is delivered twice | 0 to 50,000 ppm |
| reordering and delay | each message is delayed by a drawn amount, so later messages can overtake it | 0 to 5,000 ms |
| disconnection | a session is closed | 0 to 4 per virtual hour |
| crash | the device process stops at a persist boundary (before the write, during it, after it before `Persisted`), and restarts | 0 to 1 per 50 persists |
| clock skew | a device's `wall_ms` is offset, and may jump | −7,200,000 to +7,200,000 ms |
| relay rollback | the relay's state returns to one it had earlier | 0 to 2 per run |
| relay fork | some devices are served from a copy of the relay that diverges from a seq on | 0 or 1 per run |
| relay withholding | entries above a seq are not served to one device while others progress | 0 or 1 per run |
| injected frame | the relay stores a frame with a bad signature or another board's id | 0 to 3 per run |

Faults stop at `T_f`, the end of the configured `days`. The run then continues for 7,200,000 ms of virtual time with duties on and no workload.

### 31.7 The six invariants

Let the **reference log** be the final model relay's log after the settle, or, in a run with a fork, the log of the branch the oracle names. The **oracle** is a fresh `Engine<ToyMachine, FakeSealer>` with the genesis anchor that replays the reference log from seq 1 with no faults.

1. **Convergence.** At the end, every device that is `Live`, joined, current and unhalted, with no stop, has the same `B` and the same state hash as the oracle at that seq.
2. **Acknowledged entries.** For every `entry_id` that a relay acknowledged (`new` or `dup`) to a device: at the end, the reference log holds it exactly once with class `Accept` (applied, or refused by `apply`), after any number of re-sealable refusals; and every device whose `B` is past that seq has applied it exactly once.
3. **Queued edits.** For every `Notify(Enqueued)` with an `entry_id`: at the end, either the reference log holds that `entry_id` exactly once with class `Accept`, or the record is still in the outbox of a device that is halted, stopped or revoked, or it was finally refused with a refusal record on its device.
4. **Tampering.** No device's confirmed state at any moment differs from the oracle's at the same `B` (on the branch it followed), and every device that received a welcome or page contradicting its own log, or a page from a rolled-back or forked relay state that it would otherwise have applied, raised an alarm of §21.1 with evidence before applying any entry of it; every injected frame raised `SkippedFrame`.
5. **No deletion by the relay.** Every `TruncateLogBelow`, `DeleteBases`, outbox `Drop`, `forget_roots_below` and identity change a device emits is explained by one of: the retention rules of §7.13; an accepted purge rotation (§28.2); a `ConfirmRejoin` (§19.7); a join (§19.8); an entry that reached the board head; a cancelled hold; an accepted epoch rotation or refused own rotation.
6. **Settling.** At `T_f + 7,200,000`, every device is either `Live`, caught up, with no record older than `T_f + 600,000` in its outbox, or halted or stopped with a persisted reason; no request has been outstanding longer than 120,000 ms; and in the last 1,800,000 ms no device emitted an `Append` other than of beats and duty entries.

- **ENG-SIM-002** (engine) The simulator MUST check the six invariants of §31.7 at the end of every run and MUST report each violation with the seed, the invariant and the first trace line that shows it.
- **ENG-SIM-003** (engine) Two runs with the same seed and configuration MUST produce traces with the same SHA-256.
- **ENG-SIM-004** (engine) CI MUST run 1,000 seeds per run, `base` to `base + 999`, where `base` is `TD_SIM_BASE` when set, else the first 16 hex digits of the commit id read as a `u64`, and MUST print `base` and every failing seed.

### 31.8 A failing seed becomes a regression test

A failing seed is saved as `crates/taskdeck-sync/tests/regress/<seed>.json`: `{"spec": "engine.md §31", "cases": [{"name", "seed", "config", "invariant", "trace_sha256", "first_bad_line"}]}`. The regression test runs every file's seed and configuration and requires every invariant to hold; `trace_sha256` records the failing run for reference and is not compared after the fix.

- **ENG-SIM-005** (engine) Every seed that failed an invariant in CI MUST be committed as a regression file of §31.8 together with the fix, and the regression test MUST run every such file.

### 31.9 Trace format

UTF-8 JSON Lines, one object per line, keys in the order listed, integers only, byte strings as lowercase hex:

| `k` | Keys after `t` and `k` | Meaning |
|---|---|---|
| `start` | `dev`, `attempt` | a device started |
| `crash` | `dev`, `at` (`before`, `during`, `after`) | a crash at a persist boundary |
| `in` | `dev`, `ev` (the event's name), `seq` (when it has one), `n` (entries or results) | an event delivered |
| `out` | `dev`, `fx` (the effect's name), `id`, `n`, `frames` (the frame hashes it carries) | an effect emitted |
| `relay` | `r` (0 or 1), `op`, `head`, `chain` | a relay state change |
| `fault` | `kind`, `dev` or `r`, `p` (parameters) | a fault injected |
| `alarm` | `dev`, `kind`, `seq` | an alarm raised |
| `check` | `inv`, `ok`, `detail` | an invariant checked |

`t` is the virtual time in milliseconds. `trace_sha256` is SHA-256 of the whole file.

## 32. The fifteen V2 scenarios

Each becomes an end-to-end test in `device` (SYNC_PLAN.md §6.7 item 10). The outcome under this spec, the core rules (`core.md` §10) and the seal rules:

| # | Scenario (V2) | Outcome |
|---|---|---|
| S1 | phone offline three days | The phone shows its confirmed state plus its pending records (§8.2); its new ids use its `device_no`. On reconnect: welcome, reconcile (§11.3), catch-up pages each made durable before applying (§12.3), one rebase at the end of the run (§8.4), then its frames are sealed and sent in order (§10.2). Overwrites of newer laptop edits apply with "changed on Laptop since you last synced" notices on both devices (`core.md` §10.2); a `complete` of an item the laptop already retired is a benign repeat; edits to it are refused and recorded in the phone's refusal list (§13.3), marked "will be refused" from the rebase on. All devices converge. |
| S2 | concurrent block edits | Ordered by seq; `move_blocks` skips blocks moved since (skipped notices), `remove_blocks` removes only seen blocks, a single `move_block` overwrites with a notice (`core.md` §10.3, §10.4). Every notice is persisted on the devices it concerns (§13.4). |
| S3 | revocation while a device is offline | The desktop sends `revoke`, then the `R`-shape entry (§28.5). The laptop returns, opens its wrap of `R_{e+1}`, persists it (D4); its frames sealed under `e` before it learned the rotation are sent first and accepted within 30 days of log clock, else refused `seal/stale_epoch` and re-sealed in order with every later one (§24.2, §24.3). Two concurrent revocations: the first rotation wins, the loser discards its root, sees the target revoked and is done (§28.5). A concurrent `DeviceAdded`: refused `seal/ctl_wraps` or raced, recomposed. A device paired after the rotation holds the previous root from its bundle. Control entries are never compacted (§20.2), so a device that missed a rotation reads its wrap. |
| S4 | relay rollback, drop, reorder, fork | Rollback: the welcome head below the log head, or a `gap` with acknowledged counters above it: `RelayBehind`, stop (§21.2), restore tail by whoever holds more (§21.3), then resume. A dropped entry: a seq gap (`Withheld`) or a counter gap (`Counter`). A reorder: a `based_on` or chain mismatch (`Fork`, `Counter`). A fork: `Fork` at a welcome, a page head, an echo or a `based_on`; re-home (§28.3). In every case nothing beyond the evidence is persisted or applied, and editing continues. |
| S5 | bad snapshot | Every device that applied `S` sends a verdict; a mismatch raises `SnapshotMismatch` and makes the snapshot unusable (§19.3, §19.5). Compaction and rejoin use only usable snapshots; a pairing uses a fresh snapshot of the pairing device (§28.4). Hash comparisons use only matching hash versions. |
| S6 | version skew | A writer cannot emit ops of an inactive rules version (§25.1); the UI names the blocking device (§25.2); the upgrade is proposed only when no active member blocks it (§25.3). Only an override can leave a device behind: it halts with `Rules`, shows its frozen state plus every own record not yet applied (§8.5), holds new edits, takes no duty, and resumes from the halting entry after its update (§16.1). |
| S7 | clock skew, travel, DST | `at_ms` is the composer's clock (§27.3); instants are compiled once at the origin (`core.md` §13). Skew over five minutes warns; over an hour blocks composing until the user confirms (§27.2). Relay time never stamps. |
| S8 | quota, 48-hour outage, suspension | Local editing continues; `paused_until_ms` or `unreachable_since_ms` with the queue count is shown (§30.1); no alarm from silence (§21.1). Suspension: move the board (§28.3). No relay answer deletes anything (§11.8). |
| S9 | wipe, reinstall, disk death | A wiped phone re-pairs as a new device; its unsent outbox is lost; the old identity is revoked by the user. A Mac restored from Time Machine: the welcome's `last_ctr` is above its frames, so it catches up fully before sending, drops records already sequenced by `entry_id`, re-seals the rest above the relay's counter (§11.3), and the runtime raises its minter from the marks; no command is applied twice and no id is reissued. All devices lost: the recovery tool restores and fences the old board (`recovery`). |
| S10 | migration | The board starts from the genesis of §19.8; the other devices pair (bootstrap from a fresh snapshot). The legacy checks are `core` and `desktop`'s. |
| S11 | subscriptions | Overlays are per device and never enter the engine; a feed that panics is quarantined by `device`; subscription commands are ordinary commands. |
| S12 | idempotency | A retry sends identical bytes and gets `dup` (§10.5); a frame is durable before it is sent, so a crash between sending and persisting cannot happen; a restored copy is handled as S9; a `conflict` is matched by `entry_id` before anything is re-sealed (§11.4). |
| S13 | Android | Process death is safe: the outbox is durable before the call returns (§9.1), pages are durable before they are applied (§12.3), one marker per page; an OS kill never halts by itself (§15). Doze delays sync and shows only as "last seen". |
| S14 | compaction | A device less than 365 days behind catches up from the log. One below the relay's floor rejoins, after the user confirms, from a usable snapshot it can open, and is told continuity could not be checked (§19.7); if it lacks that epoch's root it re-pairs and resubmits (§24.5). Control entries are kept (§20.2). |
| S15 | deletions | Deleted text stays in the log, bases and backups for their retention (§7.13); "Purge history" rotates the epoch with `purge_before`, truncates every device's log and bases, shreds old roots after 30 days of log clock, purges the relay and offers to delete older backups (§28.2). A `ResetTo` cannot reach below the purge. |

## 33. Error catalogue

Codes the engine returns in `EnqueueRefused`, `OpDone`, `PairFailed`, stops, refusal records and alarms. Codes of `seal.md` §22, `core.md` §16 and `relay.md` §19 pass through unchanged where the tables above say so.

| Code | Meaning | Raised when | What the caller does |
|---|---|---|---|
| `engine/not_joined` | the device has no `device_no` yet | §9.1 step 1, §19.8 | wait for the join to finish |
| `engine/revoked` | the device is revoked | §9.1 step 1, §24.4 | offer re-pair and resubmit |
| `engine/clock_unconfirmed` | the skew exceeds an hour and the user has not confirmed the clock | §9.1 step 1, §27.2 | show the clock prompt |
| `engine/trial_panic` | the command panicked in the trial apply, or panicked in the pending view | §8.2, §9.1 step 3 | report the bug; nothing was queued |
| `engine/runtime_protocol` | an event broke this spec's contract (a limit, an id, a clock going back, `at_ms` far from `wall_ms`) | ENG-GEN-009, ENG-MODEL-004, §9.1, §12.3 | a runtime bug; the event is ignored |
| `engine/local_fault` | a trait method other than `apply` panicked, two own records disagree, an own checkpoint, base or pairing material does not load, `SM::reset` failed, or the runtime could not read a log record or base it holds | ENG-GEN-012, §4.3, §7.10, §19.4, §19.8, §21.4, §28.1 | stop; restart; report |
| `engine/persist_failed` | a write could not be made durable | ENG-PERSIST-003 | stop until restart; check the disk |
| `engine/missing_root` | the sealer lacks a root for an accepted epoch, and this device is not revoked | §13.1 | stop; re-pair and resubmit |
| `engine/own_frame_refused` | the relay refused this device's frame for its format, board, author or signature | §10.4 | stop; replace the device if it repeats |
| `engine/skip_target` | the Skip's target is not allowed | §17.1 | the UI explains which rule |
| `engine/skip_unreachable` | a Skip's target frame is held nowhere | §17.2 | stop; an integrity problem without evidence: ask the user |
| `engine/skipped` | an own entry was skipped by a Skip | §17.3 | a refusal record |
| `engine/crash_halt` | an entry failed to apply in two consecutive attempts | §15 | show the halted view and the Skip offer |
| `engine/halted` | the device is halted | §16.4 | the operation is not available |
| `engine/bad_genesis` | the genesis entry or state does not match (state hash or rules version), or the genesis board does not load | §19.8 | the board cannot be used; report |
| `engine/reseal_limit` | an own entry was refused after its third re-seal | §24.3 | a refusal record |
| `engine/fingerprint` | the typed fingerprint does not match the chosen device | §22.3 | ask again |
| `engine/adopt_membership` | the snapshot's membership, or its board's rules version, differs from this device's | §22.3 | update, or re-pair |
| `engine/nothing_to_purge` | the purge bound is not above the current purge | §28.2 | nothing to do |
| `engine/move_locked` | too many wrong move codes | §28.3 | wait the hour shown |
| `engine/move_no_fence` | the candidate relay's log holds no matching `Fence`, or no accepted `Fence` awaits a move code | §28.3 | check the address and the code |
| `engine/bad_host` | the host the user typed is not a valid host (`seal.md` §5.4) | §28, ENG-OPS-007 | ask again |
| `engine/not_eligible` | the device is not current, is stopped, or is not `Live` | §28.4 | retry when synced |
| `engine/pair_busy` | a pairing is already registering | §28.4 | finish or cancel it |
| `engine/revoke_target` | the target is not a current device, is this device, or is the last current device | §28.5 | the UI explains |
| `engine/op_busy` | another operation is running | §28 | wait for it |
| `engine/op_timeout` | an operation could not proceed for 30 days | §28.8 | start it again |
| `engine/restore_incomplete` | a restored `DeviceReinstated` needs the recovery identity at the relay | §21.3 | run the recovery tool's reinstate |
| `engine/stranded` | a rejoin found no snapshot this device can open and trust | §19.7 | re-pair and resubmit |
| `engine/rebootstrap` | a Skip or `ResetTo` needs a state this device never held | §19.9 | wait for the next usable snapshot |
| `engine/toy_format` | toy state bytes invalid, or a toy origin state that is not 8 bytes | §5.3 `load`, `rebind` | simulator only |
| `engine/toy_parse` | toy command bytes invalid | §5.3 | simulator only |
| `engine/toy_bad_ctx` | `Ctx` out of range | §5.3 | simulator only |
| `engine/toy_rules` | op not in the active toy rules | §5.3 | simulator only |
| `engine/toy_gone` | key or item absent | §5.3 | simulator only |
| `engine/toy_overflow` | arithmetic overflow | §5.3 | simulator only |
| `engine/toy_bad_id` | minted id out of range or not above the mark, or a rebind mint above 2^40 − 1 | §5.3 `apply`, `rebind` | simulator only |
| `engine/toy_cap` | 10,000 items | §5.3 | simulator only |
| `engine/toy_fail` | the `Fail` command | §5.3 | simulator only |
| `engine/toy_unknown_rules` | upgrade to an unknown or older toy version | §5.3 | simulator only |

## 34. Limits

| Name | Value | Unit | Enforced by |
|---|---|---|---|
| frames per batch | 16 | frames | ENG-SEND-001 |
| batch size | 4,194,373 | bytes | ENG-SEND-001 |
| entries per page | 256 | entries | ENG-RECV-001 |
| frame bytes per page (more than one entry) | 1,048,576 | bytes | ENG-GEN-009 |
| frame size | 670 to 262,144 | bytes | ENG-GEN-009 |
| outstanding reads; appends or frame-carrying admin calls | 1; 1 | requests | ENG-EFFECT-003 |
| checkpoint cadence | 200 entries, or 600,000 ms with a change | | ENG-CKPT-001 |
| base cadence | 604,800,000 | ms of log clock | ENG-CKPT-003 |
| base and log retention | 34,560,000,000 (400 days); the newest 2 bases always | ms of log clock | ENG-STATE-004 |
| log kept regardless of age | 16,384 | newest seqs | ENG-STATE-004 |
| state-hash stride | 64 | seqs | ENG-HASH-002 |
| state-hash input | 86 | bytes | ENG-HASH-001 |
| naming hold | 30,000 | ms of `mono_ms` | ENG-HOLD-001 |
| large-queue warning | 10,000 | records | ENG-ENQ-007 |
| `at_ms` against `wall_ms` at enqueue | 60,000 | ms | ENG-ENQ-006 |
| consecutive unmatched conflicts | 3 | results | ENG-RECON-006 |
| snapshot interval | 604,800,000 | ms of log clock | ENG-SNAP-001 |
| verdict grace | 172,800,000 | ms of log clock | ENG-SNAP-001 |
| snapshot entry trigger | 1,000 | entries behind the aligned seq | ENG-SNAP-001 |
| snapshots per device | 1 | per 86,400,000 ms of log clock | ENG-SNAP-006 |
| rank wait | 3,600,000 | ms per rank | ENG-SNAP-002 |
| snapshot upload failures before giving up | 3 | uploads | ENG-SNAP-002 |
| snapshot records kept | 64 | records | ENG-SNAP-005 |
| pending verdicts | 64 | verdicts | ENG-BEAT-002 |
| beat interval | 86,400,000 | ms of `wall_ms` | ENG-BEAT-001 |
| beat with pending verdicts | 3,600,000 | ms of `wall_ms` | ENG-BEAT-001 |
| reports kept | 1,024 | reports | ENG-DIVERGE-001 |
| compaction | 1 | per UTC day | ENG-COMPACT-004 |
| compaction age | 31,622,400,000 (366 days) | ms of log clock | ENG-COMPACT-002 |
| Skip window | 16,384 | seqs | ENG-SKIP-004 |
| re-seals per record | 3 | re-seals | ENG-RESEAL-001 |
| caught up | 600,000 | ms since the last head | ENG-DUTY-002 |
| poll interval without push while visible | 30,000 | ms of `mono_ms` | ENG-SESS-001 |
| hello interval under an evidence stop | 900,000 | ms of `mono_ms` | ENG-RECV-001 |
| reconnect while revoked | 3,600,000 | ms of `mono_ms` | ENG-EPOCH-004 |
| clock warning | 300,000 | ms of skew | ENG-CLOCK-003 |
| clock block | 3,600,000 | ms of skew | ENG-CLOCK-002 |
| clock confirmation | 604,800,000, and 300,000 of drift | ms | ENG-CLOCK-002 |
| skew measurement lifetime | 86,400,000 | ms of `mono_ms` | ENG-CLOCK-002 |
| recovery-key confirm offer | 259,200,000 | ms after `start_ms` | ENG-WINDOW-002 |
| old roots after a purge | 2,592,000,000 | ms of log clock | ENG-PURGE-003 |
| move codes before a lockout; lockout | 5; 3,600,000 | tries; ms of `wall_ms` | ENG-MOVE-002 |
| restore-tail request | 64 frames, 1,048,576 bytes | | ENG-EVID-004 |
| seed-tail request | 256 frames, 1,048,576 bytes | | ENG-MOVE-003 |
| host in `Move`, `FollowAddress`, `CloseBoard` | 255 | bytes, `:port` included (`seal.md` §5.4) | ENG-OPS-007 |
| withholding before a re-home is offered | 86,400,000 | ms of `wall_ms` | ENG-EVID-005 |
| operation timeout | 2,592,000,000 | ms of `wall_ms` | ENG-OPS-003 |
| operations at a time | 1, plus Skip and rules override | operations | ENG-OPS-003 |
| restore-to-point reach | 31,536,000,000 (365 days) | ms of log clock | ENG-RESET-001 |
| alarms, banners, stops, operations kept in `Meta` | 256, 256, 16, 16 | records | ENG-EVID-002, ENG-BANNER-002 |
| notice and refusal records kept | 1,000 | records | ENG-STATE-004 |
| simulator seeds per CI run | 1,000 | seeds | ENG-SIM-004 |
| simulator settle window | 7,200,000 | ms of virtual time | ENG-SIM-002 |

## 35. Test vectors

Stage `engine` publishes these families under `crates/taskdeck-sync/tests/vectors/`, one JSON file each, in the format of README §7. Cryptographic values in `engine-hash.json` are real SHA-256 outputs; the fake sealer's values are deterministic from its seeds.

| File | Spec | Inputs | Expected outputs |
|---|---|---|---|
| `engine-splitmix.json` | §31.2 | seeds | the first 16 outputs of `next()`, `below(n)` for several `n` |
| `engine-toy.json` | §5.3 | toy states and commands as values, `Ctx`s | state bytes, command bytes, `apply` results (state bytes after, notices, refusal codes), `reset`, `coalesce`, `rebind` outputs; one case per op and per refusal code |
| `engine-fake.json` | §6.3 | seeds, device names, entries | fake keys, frames, wraps, commitments, payload encodings, receipts for a valid frame and for one failing case per step of `seal.md` §14.2 |
| `engine-hash.json` | §18.3 | board hash, membership bytes, hash version | the 86-byte input and the state hash |
| `engine-pending.json` | §8 | a confirmed toy state, outbox records | the pending view's bytes, provisional seqs, doomed flags and codes |
| `engine-receipts.json` | §13 | a receipt (class, code, flags, kind, own or not) | the engine's action: board change, alarm kind, stop, refusal record |
| `engine-poison.json` | §15 | a marker, `last_failed`, `attempt`, and a sequence of apply outcomes and crashes | the markers written, the attempts, whether and where it halts |
| `engine-duties.json` | §19.2, §23.1, §29.2 | membership views, registries, clocks | ranks, whether a snapshot or beat is due, the rank wait |
| `engine-clock.json` | §27 | measurements, confirmations, clocks | warning, block |
| `engine-reconcile.json` | §11.3, §11.4 | outbox counters, welcome fields, conflict and gap results | the records cleared, acknowledged or re-sealed, the evidence raised |
| `engine-traces.json` | §31 | seeds and configurations | `trace_sha256` and the invariants' results, for the simulator's determinism check |
| `engine-scenarios.json` | §32 | one scripted simulator run per scenario S1 to S15 that the simulator can express (S10 and S11 excluded) | the final statuses, alarms, refusal and notice records, and state hashes |

## 36. Interfaces

**`seal.md`.**
- Provides to seal: the state-hash composition of §18.3 (ENG-HASH-001; `seal.md` §25 requires it for §17.9, §20.5 and §21.6), and `taskdeck_sync::state_hash` (§37) as its one definition, which `seal.md` §26 `state_hash` and stage `recovery` reproduce byte for byte, tested against `engine-hash.json` (§35).
- Relies on (ruling S15): `seal.md` §13.6's `Skip` precondition `t ≥ max(W.purge_seq, s − 16,384)`, refused with `seal/ctl_target` before any frame is asked for (SEAL-RCPT-009), which is the floor `F(s)` of §17.4; the engine's compaction keeps the frames above it and the target of every accepted Skip (§20.2).
- Relies on: the declared mode of `seal.md` §20.5 (SEAL-BLOB-008) for `open_snapshot(…, exact: false)` (§19.7): the parts are checked against the relay's declared `sha256` and `size`, the epoch is read from the sealed header, the AEAD authenticates before anything else is read, and inflation is capped at 67,108,864 bytes; the declared values and the writer are checked afterwards against the `snap` entry (§19.7 step 5).
- Requires: `seal.md` §17.10's `NeedSnapshotAndRegister` maps to the engine's `PairRegister`, and its `Registered` event is built by the runtime from `Notify(PairRegistered)`, the token in the `register` answer and the roots of the bundle snapshot's epoch and the epoch before it, never a later root (`seal.md` §17.8, ENG-OPS-006).
- Requires: the inputs of `seal.md` §14.1 are supplied as ENG-SEALER-001 to ENG-SEALER-003 state; `own_sealed` follows §11.3 for a restored folder (`seal.md` §14.7 leaves that choice to the engine).
- Assumes: `seal.md` SEAL-ENV-003's "confirmed membership state" is the engine's `M_K`, the membership at the class head.
- Assumes: the sealer returns `Accept` with `Halt::Commitment` for a rotation whose own wrap fails, and its classification of later seqs then needs a root it lacks (`seal/missing_root`).
- Assumes: the move-code retry limit and the 72-hour prompt timing are this spec's (§28.3, §26.1), as `seal.md` §25 says.
- Requires (ruling S24, lead ruling L1): the `reinstate` operation's `args = dev[16] ‖ u32 first_epoch ‖ u16 n ‖ n × wrap[96]` (`seal.md` §16.2), signed by the recovery identity and never sent by the engine, and the opening of a wrap delivered outside an entry (`seal.md` §7, SEAL-WRAP-002, SEAL-WRAP-003), which the sealer implements in `take_missed_wraps` and `classify` (§6.1, §24.4).

**`core.md`.**
- Relies on: `core.md` §11.3 `reset` (CORE-RULES-005) behind `SM::reset` (§5.1): the target's board with `marks` the elementwise maximum of target and current, `notes_rev` one above both, and the rules version raised to the current one (a `ResetTo` never changes the membership, ENG-RESET-003), so no id is reissued, no notes base matches by coincidence, and `BoardData.rules` still mirrors `M.rules` after a reset (§28.1).
- Relies on: `core.md` §7.5 `ids::rebind` (CORE-ID-006) behind `SM::rebind` (§24.5): every id and sid the commands mint with the old `device_no` is minted anew from the new identity's current minter against its pending view, and every later reference is rewritten.
- Relies on: `core.md` §10.7 admits `engine/skipped` and `engine/reseal_limit` as refusal codes, besides the codes of `core.md` §16 and `seal.md` §22; the adapter's `SM::refusal_bytes` is `RefusalRecord::to_canonical` (`core.md` §20).
- Relies on: `Ctx.at` is `Instant::from_at_ms(at_ms)` (`core.md` §5.1), which succeeds for every opened entry because `seal.md` §10.1 bounds `at_ms` by 253,402,300,799,999, the last millisecond of 9999; the pending view and the trial apply use `at_ms` values the facade took from a `CompileCtx.now` within that range (§9.1).
- Assumes: the snapshot JSON's `version` (`core.md` §14.3) is `view_version`, which restarts at 1 at every start (§8.2).

**`relay.md`.**
- Requires: the fields the engine reads from the welcome, `hello`, pages, results, admin answers and error bodies (§11.1, §12.1, §11.7), as `relay.md` §4.2, §9.6, §10.4, §10.5, §11.3 and §12 define them.
- Provides: what `relay.md` §22 requires of the engine: the Skip floor and the compaction bound (§17.4, §20.2); the keep list (§20.2); a compaction signed by another device than the snapshot's writer and repeated daily while `more` (§20); purge: the rotation first, then `purge`, repeated daily (§28.2); the picture re-upload after a purge (§28.2); restore tail with requests that end at each control entry, each followed by that entry's seq-mode registration, `revoke`, `fence` or `purge` (§21.3); `revoke` before its entry, composed again after every race until the target is revoked (§28.5); the move with the new relay's door and the `fence` operation on the old relay, read through its `Fence` before the seed, or the `Fence` appended on the new relay when the old one did not sequence it (§28.3); seeding with every device and recovery identity of the membership at the end of the tail and the counters before it, and uploading only a snapshot of its own sealing (§28.3); resuming a seed tail or restore tail from `relay/not_next` (§21.3, §28.3); `second`, `displaced` and 4001 tolerating one displacement after a restart (§11.6); `truncated` (§19.7); `last_ctr`, `last_seq`, `gap` and `conflict` (§11.3, §11.4); never trusting `at` or `relay_now` (§27.3, ENG-WINDOW-001); blob purposes 1, 2 and 4 (§19.4, §28.4, §28.2).
- Provides: the model relay (§31.3) proves the clauses of `relay.md` whose owners include `engine` (RELAY-SEQ-001 to RELAY-SEQ-004, RELAY-SEQ-010, RELAY-READ-001, RELAY-READ-002, RELAY-SOCK-008) through ENG-SIM-001; RELAY-STORE-006 through ENG-WINDOW-001 and ENG-CLOCK-001.
- Requires (ruling S24): `reinstate` stores its wraps on the target's devices row and `revoke` clears them (RELAY-REVOKE-005), and every `login` answer of that device carries them in `wraps` while they are stored (RELAY-LOGIN-006).

**`device.md`.**
- Requires: the runtime of §4: serial persists with `Persisted` in order (ENG-PERSIST-001), the other effects in order without waiting (ENG-EFFECT-001), `Start` with every record of §7.10, a session id per process, the backoff, login and tokens, the poll rate while visible, ticks.
- Requires: the sealer adapter composes rotations, draws its random values and retires its pending roots as §6.1 `seal` and `classify` state (ENG-SEALER-007 to ENG-SEALER-009).
- Requires: the facade compiles against the engine's pending view and sends `Enqueue` with no other input in between, with `at_ms` the compile clock (§9.1), and persists nothing of its own about the command (the origin state travels in the record).
- Requires: after a catch-up that showed higher marks, the runtime raises the `IdMinter` (`core.md` §19); after `Notify(RecoveryKeyChanged)`, backup writers ask for the new fingerprint (SEAL-BKP-006); after `Notify(PurgeBackups)` and `Notify(ReuploadPicture)`, the runtime acts on the user's confirmation; it gates operations by platform (SYNC_PLAN.md §2).
- Requires: backups read the confirmed state, `M_B` and the history from the engine (`Engine::confirmed()`, `Engine::membership()`), and the history records of every accepted control entry are kept for the life of the board (`seal.md` §25), which the engine's log retention does not provide below `log_floor`: `device.md` keeps them in `history.tdk` (kind 11, `device.md` §15.7).
- Requires (ruling S24): the runtime logs in at every `Connect` while this device is revoked, and sends `MissedRoots { first_epoch, wraps }` right after each login whose answer carries `wraps` (`device.md` §10.3).

**`bridge.md`.**
- Requires: the phone page's `version` is a change token only (§8.2); the sync line, "will be refused" marks, refusals as notices and the notes conflict sheet are fed from `status()`, `pending()` and `notices()` (§30).

**Stages `desktop` and `android`.** Build every screen of §30.2 from §30's data; acknowledge banners one by one; never offer a dismissal for a stopping alarm.

## 37. Public API

The `taskdeck-sync` crate. `#![forbid(unsafe_code)]`. Types already given in §4 to §31 are listed by name; the skeleton must compile with exactly these names.

```rust
// ---------- traits (§5, §6)
pub use model::{Ctx, Applied, SmRefusal, SmError, Coalesce, StateMachine};
pub use model::{Dev, Hash32, Head, Kind, Class, Halt, Verdict, BeatInfo, SnapInfo, RecoveryMode, UpgradeMode,
                CtlItem, CtlIntent, Body, PlainEntry, Opened, Receipt, ClassifyInput, Sealed,
                SnapshotMaterial, SealedBlob, SealerError, Sealer, DeviceStatus, DeviceView, RecoveryView,
                PendingRecovery, MembershipView};

// ---------- the state hash (§18.3)
pub fn state_hash(hash_version: u32, board_hash: &Hash32, membership_bytes: &[u8]) -> Hash32;

// ---------- events and effects (§4)
pub struct Input<C> { pub wall_ms: i64, pub mono_ms: u64, pub event: Event<C> }
pub enum Event<C> {
    Start(Loaded), Shutdown, Tick,
    Persisted { id: u64 }, PersistFailed { id: u64 },
    LocalRead { req: u64, result: LocalReadResult },
    Enqueue { ticket: u64, command: C, at_ms: i64, origin_state: Vec<u8>, hold: bool },
    ReleaseHold,
    Op { ticket: u64, op: Operation },
    PairRegister { ticket: u64, device: NewDevice },
    Resubmit { ticket: u64, commands: Vec<C>, from_device_no: u16, origin_state: Vec<u8> },
    User(UserAction),
    Connected { push: bool }, MissedRoots { first_epoch: u32, wraps: Vec<[u8; 96]> },
    Welcome(Welcome), Page { req: Option<u64>, page: Page },
    AppendResults { req: u64, results: Vec<AppendResult> },
    AdminResult { req: u64, result: Result<AdminOk, RelayRefusal> },
    BlobResult { req: u64, result: Result<BlobOk, RelayRefusal> },
    RelayRefused { req: Option<u64>, refusal: RelayRefusal },
    Second { session: [u8; 16] }, Displaced { session: [u8; 16] },
    Disconnected { unreachable: bool, paused_until_ms: Option<i64> },
}
pub enum LocalReadResult { Records(Vec<LogRecord>), Base(Checkpoint), Missing { code: &'static str } }
pub struct NewDevice { pub dev: Dev, pub sign_pk: Hash32, pub hpke_pk: Hash32, pub name: String, pub transcript: Hash32 }
pub enum AppendResult { New { seq: u64, chain: Hash32 }, Dup { seq: u64, chain: Hash32 },
                        Conflict { seq: u64, frame: Vec<u8> }, Gap { last_ctr: u64, last_seq: u64 },
                        Refused { status: u16, code: String }, Skipped }
pub enum Effect {
    Persist { id: u64, ops: Vec<PersistOp> },
    ReadLog { req: u64, from: u64, to: u64 }, ReadBase { req: u64, seq: u64 },
    Connect { relay: RelayTarget }, Disconnect, Hello,
    Read { req: u64, after: u64, limit: u16 },
    Append { req: u64, frames: Vec<Vec<u8>> },
    Admin { req: u64, op: AdminRequest },
    Blob { req: u64, op: BlobRequest },
    ScheduleTick { at_mono_ms: u64 },
    Notify(Notification),
}
pub struct RelayTarget { pub host: String, pub door: [u8; 5], pub home_gen: u32 }
pub use session::{Welcome, AdminRequest, SeedDevice, AdminOk, RelayRefusal};       // §11
pub use receive::{PageEntry, Page};                                                // §12.1
pub use snapshots::{BlobRequest, BlobMeta, BlobOk};                                // §19.6
pub use ops::{Operation, OpRecord};                                                // §28
pub use ui::{Status, SessionState, ClockStatus, SkipOffer, HaltView, StopAction, StopView, DeviceRow, Blocker,
             VersionStatus, RulesStatus, RecoveryChangeView, MovingView, OpStatus, StatusWarning,
             PendingMark, NoticeEntry, Notification, UserAction, Banner, BannerKind, BannerDetail,
             AlarmRecord, AlarmKind, Evidence, ForkSource};                        // §14.2, §21.1, §30
// PendingMark and NoticeEntry live in ui and are re-exported above; their fields:
// ui::PendingMark { pub entry_id: [u8; 16], pub provisional_seq: u64, pub op: &'static str,
//                   pub target: Option<u64>, pub doomed: Option<&'static str>, pub acknowledged: bool }
// ui::NoticeEntry { pub kind: NoticeKind, pub seq: u64, pub index: u32, pub record: Vec<u8>, pub dismissed: bool }

// ---------- records (§7)
pub struct LogRecord { pub seq: u64, pub chain: Hash32, pub tau: i64, pub frame: Vec<u8> }
pub struct Checkpoint { pub board_seq: u64, pub board_chain: Hash32, pub board: Vec<u8>,
                        pub membership: Vec<u8>, pub log_floor: u64, pub registry: Registry }
pub struct Registry { pub snapshots: Vec<SnapRecord>, pub reports: Vec<Report>,
                      pub pending_verdicts: Vec<(u64, Verdict)>, pub last_entry: Vec<LastEntry>,
                      pub restored: Option<(u64, u64)> }
pub struct SnapRecord { pub entry_seq: u64, pub author: Dev, pub info: SnapInfo, pub entry_clock: i64,
                        pub verdicts: Vec<(Dev, Verdict, u64)>, pub own: Option<Verdict> }
pub struct Report { pub seq: u64, pub dev: Dev, pub hash: Hash32, pub hash_version: u32, pub report_seq: u64 }
pub struct LastEntry { pub dev: Dev, pub at_ms: i64, pub seq: u64, pub based_on_seq: u64 }
pub enum Via { Append, Register { dev: Dev, sign_pk: Hash32 },
               Fence { host: String, door: [u8; 5], home_gen: u32 } }
pub struct OutboxRecord { pub entry_id: [u8; 16], pub order: u64, pub at_ms: i64, pub based_on: Head,
                          pub kind: Kind, pub body: Body, pub origin_state: Vec<u8>, pub via: Via, pub hold: bool,
                          pub ctr: Option<u64>, pub frame: Option<Vec<u8>>, pub frame_hash: Option<Hash32>,
                          pub epoch: Option<u32>, pub ack: Option<(u64, Hash32)>, pub seen: Option<u64>,
                          pub op: Option<u64>, pub reseals: u32 }
pub enum OutboxOp { Put(OutboxRecord), Drop([u8; 16]) }
pub struct HashRecord { pub seq: u64, pub hash_version: u32, pub hash: Hash32 }
pub struct Marker { pub first: u64, pub last: u64, pub attempt: u64 }
pub enum NoticeKind { Notice, Refusal }
pub enum NoticeOp { Add { kind: NoticeKind, seq: u64, record: Vec<u8> }, Dismiss { seq: u64, index: u32 } }
pub struct HaltRecord { pub kind: HaltKind, pub seq: u64, pub author: Dev, pub since_ms: i64, pub code: String }
pub enum HaltKind { Crash { frame_hash: Hash32 }, Rules { version: u32 }, Commitment }
pub struct StopRecord { pub kind: StopKind, pub since_ms: i64, pub alarm: Option<u64> }
pub enum StopKind { Evidence, Divergence, ForeignSigner, IdentityClone, Revoked { by_log: bool },
                    LocalFault { code: String }, Stranded, Rebootstrap { after: u64 }, Closed, Rejoin }
pub struct DutyState { pub snapshot: Option<(u64, i64, u8)>, pub last_compact_day: i64,
                       pub purge: Option<(u64, u32)>, pub upgrade: Option<(u32, i64)> }
pub struct Meta { pub attempt: u64, pub last_failed: Option<(u64, u64)>, pub build_id: String,
                  pub halt: Option<HaltRecord>, pub stops: Vec<StopRecord>, pub alarms: Vec<AlarmRecord>,
                  pub banners: Vec<Banner>, pub relay: RelayTarget, pub prev_session: Option<[u8; 16]>,
                  pub conflicts: u32, pub clock: Option<(i64, i64)>, pub last_beat_ms: i64,
                  pub duties: DutyState, pub ops: Vec<OpRecord>, pub move_tries: (u32, i64),
                  pub continuity_unchecked: Option<u64>, pub restored_copy: Option<u32>,
                  pub based_on_unchecked: u32 }
pub enum PersistOp { AppendLog(Vec<LogRecord>), SetMarker(Marker), ClearMarker, WriteCheckpoint(Checkpoint),
                     WriteBase(Checkpoint), DeleteBases(Vec<u64>), TruncateLogBelow(u64, Option<Hash32>),
                     AppendHashes(Vec<HashRecord>), Outbox(Vec<OutboxOp>), RewriteOutbox(Vec<OutboxRecord>),
                     Notices(Vec<NoticeOp>), RewriteNotices(Vec<NoticeOp>), WriteMeta(Meta),
                     WriteIdentity(zeroize::Zeroizing<Vec<u8>>) }
pub enum Bootstrap { Genesis { board: Vec<u8>, frame_1: Vec<u8>, relay: RelayTarget },
                     Join { material: SnapshotMaterial, added_seq: u64, relay: RelayTarget } }
pub struct Loaded { pub checkpoint: Option<Checkpoint>, pub bases: Vec<(u64, Hash32)>, pub chains: Vec<(u64, Hash32)>,
                    pub log_floor: u64, pub floor_chain: Option<Hash32>,
                    pub tail: Vec<LogRecord>, pub hashes: Vec<HashRecord>, pub outbox: Vec<OutboxOp>,
                    pub notices: Vec<NoticeOp>, pub meta: Option<Meta>, pub marker: Option<Marker>,
                    pub build_id: String, pub session: [u8; 16], pub bootstrap: Option<Bootstrap> }

// ---------- the engine (§4)
pub struct Config {}                                               // reserved: the relay target comes only from Loaded.meta or Loaded.bootstrap (§7.10)
pub struct Engine<SM: StateMachine, S: Sealer> { /* private */ }
impl<SM: StateMachine, S: Sealer> Engine<SM, S> {
    pub fn new(sm: SM, sealer: S, config: Config) -> Self;            // inert until Start
    pub fn handle(&mut self, input: Input<SM::Command>) -> Vec<Effect>;
    pub fn view(&self) -> &SM::State;                                   // the pending view (§8.2)
    pub fn view_version(&self) -> u64;
    pub fn confirmed(&self) -> (&SM::State, Head);                      // the confirmed state at B
    pub fn membership(&self) -> MembershipView;                         // M_K
    pub fn membership_bytes(&self) -> Vec<u8>;                          // M_B, for backups
    pub fn device_no(&self) -> Option<u16>;
    pub fn status(&self) -> Status;                                     // §30.1
    pub fn pending(&self) -> Vec<PendingMark>;
    pub fn notices(&self) -> Vec<NoticeEntry>;
    pub fn restore_targets(&self) -> Vec<(u64, i64)>;                   // §28.1: (seq, τ)
    pub fn purge_preview(&self) -> Result<(u64, i64), &'static str>;    // §28.2 step 1: (p, τ_{p−1})
    pub fn sealer(&self) -> &S;
}

// ---------- the toy, the fake and the simulator (§5.3, §6.3, §31), feature "sim"
#[cfg(feature = "sim")] pub mod toy { pub struct ToyMachine { /* rules2: bool */ }
                                     impl ToyMachine { pub fn new(rules2: bool) -> Self; }
                                     pub enum ToyCommand { Set { key: u8, value: i64 }, Add { key: u8, delta: i64 },
                                         Del { key: u8 }, Create { id: u64, name: u32 }, Name { id: u64, name: u32 },
                                         Forget { id: u64 }, Fail { code: u8 }, Panic, Mul { key: u8, factor: i64 } }
                                     pub struct ToyState { /* §5.3 */ }  pub struct ToyNotice { /* §5.3 */ } }
#[cfg(feature = "sim")] pub mod fake { pub struct FakeSealer { /* §6.3 */ }  pub struct FakeMembership { /* seal.md §13.1 */ }
                                      pub struct FakeBundle { /* roots, snapshot material and its history records, added seq */ }
                                      impl FakeSealer { pub fn create(seed: u64, name: &str) -> (Self, Vec<u8>, Vec<u8>);   // seq 1 at at_ms = 0; supported rules [1]
                                                        pub fn join(seed: u64, name: &str, bundle: FakeBundle) -> Self; }    // supported rules [1]
                                      // the genesis time and the supported rules have crate-private setters only (§6.3)
                                      }
#[cfg(feature = "sim")] pub mod sim { pub struct SplitMix64 { pub state: u64 }
                                     impl SplitMix64 { pub fn next(&mut self) -> u64; pub fn below(&mut self, n: u64) -> u64;
                                                       pub fn chance(&mut self, ppm: u32) -> bool; }
                                     pub struct SimConfig { /* §31.1 */ }  pub struct SimReport { /* §31.1 */ }
                                     pub struct FaultSet {}   // empty in the skeleton: stage engine names its fields (the faults of §31.6) through a spec change
                                     pub struct OpSet {}      // empty in the skeleton: stage engine names its fields (the operations of §31.5) through a spec change
                                     pub struct Violation { pub invariant: u8, pub line: u64, pub detail: String }
                                     pub struct SimStats {}   // empty in the skeleton: stage engine names its counts (per event and effect kind) through a spec change
                                     pub fn run(seed: u64, config: &SimConfig) -> SimReport; }
```

## 38. Open questions

Non-normative. Each states the default this spec takes.

1. **Randomness enters only through the sealer** (§6.1). The brief lists randomness among the inputs; the engine itself needs none, and `seal`'s `Entropy` trait is sealed, so the sealer draws entry ids, nonces and roots. Decided here.
2. **Every own entry goes through the outbox**, beats, control and `snap` entries included, because they share one counter (§7.4). Decided here (SYNC.md §4.1 lists only commands).
3. **Records stay until applied, not until acknowledged** (ENG-STATE-001), so a halted device still shows its own sent edits (V2-6). Decided here.
4. **Control items are composed at seal time** from the membership of that moment (§9.2), which narrows the window in which a concurrent membership change refuses a rotation (V2-8 (b)). Decided here (SYNC.md is silent).
5. **The epoch gate** (§24.2): no new seal while an older-epoch frame is unclassified, so re-sealed entries keep their order. Accepted residual: two sealed frames of the old epoch on either side of the 30-day boundary can still reorder. Decided here.
6. **Rebase per read run, and every 16 pages** (§8.4), not per page, to keep a 10,000-record outbox affordable during a 4,200-entry catch-up. Decided here (SYNC.md says "per batch").
7. **The naming hold has a 30-second ceiling** (§9.3), from V2-17's original hold; SYNC.md states only "while it is the outbox's last entry". Decided here.
8. **One append in flight** (ENG-EFFECT-003). Simpler ordering and recovery; the latency target holds because a batch carries 16 frames. Decided here.
9. **Halted devices author only Skips** (§16.4); revocations come from other devices or the recovery key. Decided here (SYNC.md: "holds new edits").
10. **The rules and commitment halts stop classification** (§16.3): the device cannot classify what follows. Only the crash halt reads ahead. Decided here (SYNC.md: "reads ahead for Skip and control entries" assumed a classifiable log).
11. **Determinism mismatch is a stop, not a fourth halt** (§22): the minority (or both sides of a tie) stops sending and keeps applying, so it can resolve by later hashes or adoption. Decided here (SYNC.md §4.8 says "halts").
12. **The Skip floor** `F(s) = max(purge_seq, s − 16,384)`, compaction that keeps the newest 16,384 seqs, and a keep list that holds every accepted Skip's target settle ruling S15 (§17.4, §20.2, `seal.md` §13.6). Decided here.
13. **A device that cannot replay a Skip or `ResetTo` target** waits for a usable snapshot after it (§19.9) rather than halting. Decided here (SYNC.md is silent).
14. **The snapshot duty is ranked by latest beat** with an hour per rank (§19.2, §29.2). A snapshot is due when the newest usable one is a week old by the log clock or 1,000 entries behind, and a device writes at most one per 24 hours of log clock, so an import produces no burst (ruling S25).
15. **A rejoin needs the user's confirmation** (§19.7), because it replaces the local log; the old state is kept as a base. Decided here.
16. **Purge stops at the most lagging current device's last reported head** (§28.2), so no current device is stranded by it; the interfaces show how far the purge reaches. Decided here (SYNC.md: "before now").
17. **Old roots are deleted 30 days of log clock after a purge** (§28.2), when no old-epoch entry can be accepted any more. Decided here.
18. **`ResetTo` mismatch puts the device in the divergence stop** and the reset is not applied there (§28.1). Decided here (SYNC.md: "checked against honest replay").
19. **Restore tail has no designated device** (§21.3): every device that holds more restores what it holds, and `relay/not_next` coordinates them. Decided here (SYNC.md: "the most advanced device").
20. **Reset trust fences the old board towards the new board's relay address** (§28.7), so old devices learn to re-pair. Decided here (SYNC.md is silent on how).
21. **The move-code limit** is 5 tries, then an hour (§28.3). Decided here (`seal.md` leaves it to the engine).
22. **The clock confirmation lasts seven days and five minutes of drift** (§27.2). Decided here (SYNC.md: "until the user confirms the clock").
23. **`view_version` restarts at every start** (§8.2). Decided here; `bridge.md` must treat it as a change token.
24. **A reinstated device reads what was sealed while it was revoked** (§24.4; ruling S24, lead rulings L1 and L20): the wraps of the roots it missed reach it in its `login` answer, outside the reinstating entry it cannot open, and it resumes classification at the paused seq; it does not pair again. A device whose revoking entry lies below `purge_seq` is not offered reinstatement and pairs again. Decided here.
25. **The model relay does not model nonces or admin signatures** (§31.3); the conformance suite covers them. Decided here.
26. **The seeded tail can reach back further than 30 days** (§28.3 stage 3): `first_seq = min(S + 1, max(1, M_L.purge_seq, L + 1 − 16,384))` keeps on the new relay every Skip target ruling S15 permits after the move, at a cost of at most 16,384 frames written once; on a quiet board that reaches back further than SYNC.md §6.7's "up to 30 days of tail". Default: the longer tail (lead ruling L2), a deliberate difference from SYNC.md §6.7.
