# tests/e2e/

The end-to-end suite: devices driven through `taskdeck-devicectl` (feature `test-trust`) against the standby relay on loopback.

- Owner: stage `device` (SYNC_PLAN.md §4).
- Spec: `docs/sync/spec/device.md` §19 (the protocol), §20 (the suite).
- CI: `bash scripts/ci/stage.sh device` runs `bash tests/e2e/ci.sh` from the
  repository root with `CI_STAGE`, `CI_OS` and `CI_WIDE` set (the contract is
  at the top of `scripts/ci/stage.sh`). `ci.sh` runs the suite on Linux and
  prints that it runs on Linux only for now on Windows and macOS (decision L3).

## What is here

The suite is one test target of the device crate (`[[test]] name = "e2e"` in
`crates/taskdeck-device/Cargo.toml`, `required-features = ["test-trust"]`): no
workspace member of its own, no dependency of its own.

| Path | What |
|---|---|
| `main.rs` | the target's root: `mod harness;` and one module per group of tests |
| `harness/standby.rs` | `Standby`: the standby relay (`relay/standby.js`) on `127.0.0.1`, stopped on drop |
| `harness/ctl.rs` | `Ctl`: a `taskdeck-devicectl` process driven by the protocol of §19.2, with typed helpers |
| `harness/http.rs` | `http_get`, `http_request`: a minimal client over `TcpStream` (the base of the hostile proxy) |
| `harness/node.rs` | `find_node`: Node 22 or later (`TD_NODE`, then `PATH`, then the runner's tool cache) |
| `smoke.rs` | tests of the harness itself: the standby alone, and `Ctl` against a stand-in |
| `stand_in_device.mjs` | the stand-in device of `smoke.rs` (a few commands of the protocol; not a model of the device) |
| `ci.sh` | the CI entry |

## Running it

From the repository root, with Node 22 or later on the `PATH` (or `TD_NODE=/path/to/node`):

```sh
cargo test --locked -p taskdeck-device --features test-trust --test e2e
# or what CI runs:
bash tests/e2e/ci.sh
```

Set `CARGO_TARGET_DIR` as the task rules say when two builds run at once.

## How a scenario is written

A scenario is a test in a module of its own (`mod s1_offline_phone;` in `main.rs`, file
`tests/e2e/s1_offline_phone.rs`), written with `Standby` and `Ctl` only:

```rust
use crate::harness::{Ctl, Source, Standby};
use std::time::Duration;

#[test]
fn one_device_round_trip() {
    let standby = Standby::start();
    let dir = tempfile::tempdir().unwrap();
    let mut desk = Ctl::start_in(dir.path(), "desk");
    desk.create(standby.url(), standby.create_secret(), Source::Empty, "desk");
    let sheet = desk.sheet();
    desk.confirm_sheet(&sheet.sheet);
    let address = desk.finish_create();
    desk.submit(serde_json::json!({ /* an intent of core.md §12.2 */ }));
    let board = desk.board();
    desk.wait_until(|c| c["any"].as_u64() > Some(0), Duration::from_secs(10));
    assert_eq!(desk.shutdown(), 0);
    // A second device: Ctl::start_in(dir.path(), "laptop"), then pair_start / join / pair_confirm / join_sas.
}
```

- **Every device a `Ctl`.** `Ctl::start_in(dir, name)` makes `<dir>/<name>/data` and `<dir>/<name>/keys` and
  starts `run --data --keys --name <name> --clock manual --zone utc --platform linux`. For another
  platform, zone or `--crash-at`, build a `CtlOptions` (`CtlOptions::in_dir`, then set its fields; `extra_args`
  takes more options) and call `Ctl::start`. `kill()` and `shutdown()` end the process; `restart()` (or
  `restart_with(|o| ...)`, for example to drop `--crash-at`) starts it again on the same folders.
- **The relay.** `Standby::start()` makes a data folder, a token key and a create secret of its own and
  returns when the standby answers. `url()` is `http://127.0.0.1:<port>`, the relay target of a device;
  `origin()` is `https://127.0.0.1:<port>`, what the standby signs into logins and what a device derives from
  the host of a relay address (relay.md §5.4); `create_secret()` is what `create` takes. The standby is a
  plain-HTTP server with no push: devices poll `hello` and `read`. It is stopped when the `Standby` drops, so a
  scenario that stops it for an outage test drops it (or calls `stop()`) while the devices live on.
- **Requests.** The typed helpers are thin wrappers of §19.2's table: `status`, `board`, `submit`, `create`,
  `sheet`, `confirm_sheet`, `finish_create`, `pair_start`, `pair_status`, `pair_confirm`, `pair_cancel`, `join`,
  `join_sas`, `wait`, `clock_advance`, `clock_set`, `shutdown`, `kill`. Any other command is
  `call_ok(cmd, json!({...}))` (must be answered `ok`; returns the whole answer), `call_err(cmd, args)` (must be
  refused; returns the error code) or `call` / `try_call` (the answer as it is; `try_call` returns the error
  instead of failing the test and takes its own timeout). Answers are the whole line, `id` and `ok` included.
  Add a typed helper to `ctl.rs` when a second scenario needs the same command.
- **Waiting.** `wait(since, timeout_ms)` is the device's `wait_for_change`; `wait_until(pred, timeout)` polls it
  and hands `pred` the counters of §17.3 (`any`, `view`, `status`, `notices`, `banners`, `overlay`, `weather`,
  `picture`, `backup`, `pairing`, `ops`). A counter says that something changed, not what: read `status()` or
  `board()` after it. `take_events()` returns the `changed` and `exit` event lines received so far.
- **Timeouts.** Every request waits at most `CtlOptions::timeout` (30 s by default; `wait` adds its own
  `timeout_ms`), every process end at most 10 s, the standby's start 20 s, an HTTP exchange 10 s. A timeout is a
  failed test, never a hang.
- **How a failure prints.** A `Ctl` error (`CtlError`, shown by the panic of `call`) carries the last 40 lines
  of the device's protocol (`->` requests, `<-` answers and events) and of its stderr; the whole stderr is in
  `<dir>/<name>/stderr.log`. When a test panics, dropping a `Ctl` prints that context again and dropping a
  `Standby` prints the standby's stderr. Secrets never print: the fields `create_secret`, `text`, `code`,
  `digits`, `sheet`, `seed`, `token`, `secret` and `address` are replaced by `<redacted>` in everything the
  harness shows, and `Sheet` and `PairStart` have no `Debug` of their values (DEV-CTL-002). Do not `println!`
  them either.
- **Deterministic.** The clock of a device is manual: it moves only with `clock_advance` / `clock_set`.
  Anything that has to wait for real time (a poll interval of the standby's polling) waits through `wait_until`
  with a timeout, never with `sleep`.

The first scenarios (one device round trip, then the first sync) are written by tasks F3 and F4 with these
helpers; the fifteen of §20.2 follow.
