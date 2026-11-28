# H: the end-to-end harness and a standby smoke test

Workflow W1a, beside the chain (F1, F2), so in a worktree of its own. Common rules: `README.md`
beside this file. Task id `h`.

## Goal

The Rust harness of `tests/e2e/` (device.md §20.1) that later tasks write their scenarios with: it
starts the standby relay on loopback and drives `taskdeck-devicectl` processes by the protocol of
device.md §19.2. `taskdeck-devicectl` does not work yet (task F3 writes it), so this task proves the
harness only against the standby, and gives the later tasks typed helpers for the commands they will
need.

## Where

Create a worktree first: `git -C /home/user/taskdeck worktree add /home/user/wt/h -b wt/h HEAD`,
work only in `/home/user/wt/h`, commit there on `wt/h`, never push. `CARGO_TARGET_DIR=
/home/user/td-target/h`.

## Paths

`tests/e2e/` (replace its README's "Nothing lives here yet" with what is there and how to run it),
and in `crates/taskdeck-device/Cargo.toml` only the test target:

```toml
[[test]]
name = "e2e"
path = "../../tests/e2e/main.rs"
required-features = ["test-trust"]
```

(the status file's decision L2: no new workspace member, no change to `Cargo.lock`; the test finds
the binary with `env!("CARGO_BIN_EXE_taskdeck-devicectl")`).

## Read

- `device.md` §19 (devicectl: options and protocol) and §20.1 (the shape of the suite).
- `docs/sync/status/relay.md`, "What the next stages need to know" (how to start the standby, the
  token key and create secret, `origin`, plain HTTP, no socket), `relay/standby.js` (its `main` and
  `startStandby`; started as `node standby.js` it reads `TD_*` variables and prints nothing, so learn
  the port by starting it through `node --input-type=module -e` with an import of `startStandby`
  that prints `url`, or another way that does not race), `relay.md` §15.2 and §17.2 (the standby),
  §5.4 (the relay address), §10.4 (`hello`), and `relay/ci.sh` (how it finds Node 22).

## Build

1. `tests/e2e/main.rs` and modules under `tests/e2e/` (for example `harness/standby.rs`,
   `harness/ctl.rs`): a `Standby` that finds Node 22 or later (`TD_NODE`, then `PATH`, then the
   runner's tool cache as `relay/ci.sh` does), makes a temporary data folder, generates a token key
   and create secret for the run (made up, from the OS's randomness through a dev-dependency the
   crate already declares), starts the standby on `127.0.0.1` with port 0, learns its URL, and stops
   it on drop; its stderr goes to a log file that is printed when a test fails.
2. A `Ctl` that starts `taskdeck-devicectl run --data <dir> --keys <dir> --name <name>` with the
   options of §19.1 the milestone needs (`--clock manual`, `--zone utc`, `--platform linux`), sends
   one request per line with increasing ids, waits for the answer with that id under a timeout,
   collects `event` lines on the side, and offers `kill`, `shutdown` with its exit code, and starting
   again on the same folders. Every wait has a timeout, and a timeout fails the test with the last
   lines of the device's stderr and output. Never print a line that carries a seed, code, token or
   secret (only the answers of `sheet` and `pair_start` carry one; DEV-CTL-002).
3. Typed helpers for the commands the milestone uses, each a thin wrapper of §19.2's table: `status`,
   `board`, `submit`, `create`, `sheet`, `confirm_sheet`, `finish_create`, `pair_start`,
   `pair_status`, `pair_confirm`, `pair_cancel`, `join`, `join_sas`, `wait`, `clock`, `shutdown`,
   `kill`; and a `wait_until(predicate, timeout)` that polls with `wait` (the change counters).
4. One test that needs only the standby, `standby_answers_a_hello_without_credentials`: the harness
   starts the standby and an HTTP request to its `hello` route without credentials is refused with
   the status and `X-TD-Relay` header `relay.md` states. Write the request by hand over a
   `std::net::TcpStream` or use `ureq` from the crate's dependencies.
5. `tests/e2e/ci.sh` (run by `scripts/ci/stage.sh device`, contract at its top): on Linux
   (`CI_OS=linux`) find Node 22 as `relay/ci.sh` does and run `cargo test --locked -p
   taskdeck-device --features test-trust --test e2e`; on Windows and macOS print that the suite runs
   on Linux only for now (decision L3) and exit 0.

The scenario tests themselves (one device round trip, then the first sync) are written by F3 and
F4 with these helpers; describe in `tests/e2e/README.md` how one is written.

## Done

`bash tests/e2e/ci.sh` passes in the worktree (it runs your smoke test; `taskdeck-devicectl` still
compiles to a `main` that panics, which no test starts), and the README's green commands pass in the
worktree. Commit on `wt/h`. In `next`, give the commit id and anything the scenario writers must know
(timeouts, how failures print, what the origin of the standby is).
