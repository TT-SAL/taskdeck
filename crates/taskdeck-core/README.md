# taskdeck-core

The pure board of the TaskDeck sync design: board data and its canonical bytes, log commands, the deterministic versioned `apply`, intents and the compile step, the read side (planner, archive, scoring, the ICS parser, the snapshot builders, reminders) and the legacy import. Pure functions and plain data: no clock, zone, file, network, thread, environment or random source. `clippy.toml` here disallows them in every target, and the crate builds for `wasm32-unknown-unknown`.

- Spec: `docs/sync/spec/core.md` (the public API is its §20). Owner: stage `core` (SYNC_PLAN.md §4).
- Dependencies: `serde`, `serde_json`, `chrono` without `clock`; for the tests only `proptest`, `sha2` and chrono's `clock`.

## Modules

| Module | What |
|---|---|
| `canon` | canonical encoding v1 and its parser (§4) |
| `time` | instants, the `Zone` trait and its three zones, local days and minutes (§5) |
| `board` | `BoardData`, its invariants, canonical bytes and loading (§6) |
| `ids` | ids, the minter, rebinding (§7) |
| `command`, `apply` | log commands v1, `apply`, conflict notices, rules versions, `reset` (§8 to §11) |
| `intent`, `compile` | intent JSON and the compile step (§12, §13) |
| `planner`, `tasks`, `archive`, `read`, `snapshot`, `reminders` | the read queries and the snapshot JSON (§14) |
| `ics`, `overlay`, `weather` | the calendar parser with its bounds, the overlay and weather types (§14.6) |
| `legacy` | the import of today's files (§15) |

## Tests

Tests read fixtures and vectors with `include_str!` / `include_bytes!` of one plain string literal relative to the test file, never from the file system (the tests are under the same `clippy.toml`). `tests/common/` holds what several targets share: the fixture folders (`boards.rs`), the fixtures' zones as `TableZone`s (`zones.rs`), the recorded snapshots and transcripts, the devices of the property tests (`devices.rs`) and the budget workloads (`budget.rs`). Vector families are under `tests/vectors/`. Each test that proves a clause says so with `// spec: CORE-XXX-NNN`.

The property tests of §18.6 are `tests/determinism.rs` (run under two `TZ` values by the gate) and `tests/rebase.rs`. The fuzz targets are in `fuzz/`, a package of its own with its own `Cargo.lock`: `command_apply`, `board_bytes`, `intent_compile`, `ics_parse`, `snapshot`; stage `release`'s nightly job runs them (`cargo fuzz run <target>` in `fuzz/`), `ci.sh` checks that they compile.

## The gate

From the repository root, always with `-p taskdeck-core`:

```sh
cargo test --locked -p taskdeck-core
cargo clippy --locked -p taskdeck-core --all-targets -- -D warnings
bash crates/taskdeck-core/ci.sh          # the library alone, host and wasm32
bash scripts/gate.sh core                # every item of scripts/gates/core.gate
```

## Budgets

CORE-PERF-001 to -003 (§17): a 31-day snapshot of the 10,000-row fixture within 40 ms, the 5,000 commands of the budget workload applied within 1,000 ms, the fixture's canonical bytes loaded within 150 ms, each the median of five runs. The crate reads no clock, so the shell times them: the examples `budget_snapshot`, `budget_apply` and `budget_checkpoint` read `n` from standard input, do their setup once and their workload `n` times; `ci.sh`, on Linux (`CI_OS=linux`, as CI's core job sets it), builds them with `--release`, computes `(T(n) − T(0)) / n` five times, and prints the medians on lines starting `budget:`. It fails on a median over its limit, or more than 25% over the baseline recorded in `ci.sh`. `tests/budgets.rs` runs each workload once for its result. To run the budgets locally: `CI_OS=linux bash crates/taskdeck-core/ci.sh`.

## CI

`bash scripts/ci/stage.sh core` runs `cargo test` and `cargo clippy --all-targets` for this package, then `bash crates/taskdeck-core/ci.sh` with `CI_STAGE`, `CI_OS` and `CI_WIDE` set (the contract is at the top of `scripts/ci/stage.sh`). The stage's own checks go in that `ci.sh`, not in `ci.yml`.
