# Stage `device`: where the parts did not fit

Every case met while the real core, seal and relay are wired into the engine, or where a spec is wrong, unclear or silent, in the order met. Format: `briefs/README.md`. The lead's judgement of each is in `../device.md` ("Where the parts did not fit").

## h-1. The standby's origin must name its port, which port 0 only learns after the start
- Kind: misfit
- Where: relay.md §17.2 (`TD_RELAY_ORIGIN`, `TD_LISTEN`), §5.4 (the origin of an address is `https://` and its host, with the port); `relay/standby.js` `startStandby` (the origin is read once, by `makeConfig`); device.md §20.1, §19.1 (a relay on `127.0.0.1`, port 0).
- Says / does: a device derives the relay origin it signs in its login from the host of the relay address, which for the standby on loopback is `127.0.0.1:<port>` (§5.4, port written when not 443). The standby checks the login against the origin it was configured with. With `listen: '127.0.0.1:0'` the port is known only after `startStandby` returned, but the origin is needed to call it.
- Done here: `tests/e2e/harness/standby.rs` reserves a free loopback port (bind to port 0, read it, release it), starts the standby on that port with origin `https://127.0.0.1:<port>`, and retries up to five times if the port was taken in between. A small race remains, retried.
- Proposed fix: relay change: when `startStandby` is given `origin` with port `0` (or a `{port}` placeholder, or no origin and an `originFromListen` option), it takes the bound port into the origin after `listen`; or `startStandby` accepts `origin` as a function called with the port. Either removes the race for every test that wants port 0.
- Evidence: `smoke::standby_origin_names_its_own_host`.

## h-2. Decision: the harness stops the standby by closing its standard input
- Kind: decision
- Where: relay.md §17.2 (the standby serves until `SIGTERM`); `tests/e2e/harness/standby.rs`.
- Says / does: `node standby.js` stops on `SIGTERM` and prints nothing, so the port cannot be learned from it. The harness launches `startStandby` through `node --input-type=module -e`, which prints its `url` as one line and stays up until its standard input ends.
- Done here: `Standby::stop` (and drop) closes the child's standard input and waits up to 10 s, then kills it. Std's `Child` has no portable way to send `SIGTERM` without `unsafe` or a dependency, and the end of standard input also stops the standby if the test process itself dies.
- Proposed fix: none.
- Evidence: `smoke::standby_serves_several_requests_and_refuses_an_unknown_route`.

## h-3. §19.2 does not say how a result sits in the answer, nor what `wait` and an unknown `cmd` do
- Kind: spec-unclear
- Where: device.md §19.2 (the Answer line and the table's Result and Arguments columns); §17.3 (`wait_for_change(since_any, timeout_ms)`).
- Says / does: an answer is `{"id", "ok": true, ...result}`, which the harness reads as the result's fields at the top level (`{"id": 3, "ok": true, "wall_ms": …}`); for results the table calls "the status as record JSON", `Submitted`, "the counters", "a ticket" the spec does not say whether they are flattened into the answer or under one key. The table gives `wait` the argument `since` where §17.3 has `since_any`, and the counters as `Changes` (§17.3) whose field names the harness assumes to be the answer's top-level fields (`any`, `view`, …). `{"id": 0, "ok": false, "error": "device/ctl_request"}` is the answer to an invalid line; the answer to a valid line with an unknown `cmd` is not stated (the stand-in used by the harness's tests answers it with its own id and the same code).
- Done here: the typed helpers return the whole answer line and read `canonical`, `state_hash`, `sheet`, `address`, `recovery_fp`, `code`, `wall_ms`, `any` at the top level; `wait` sends `since` and `timeout_ms`. F3 writes the device; if it differs, the helpers of `tests/e2e/harness/ctl.rs` change, not the tests.
- Proposed fix: spec change in §19.2: "a result's fields are written in the answer beside `id` and `ok`; a record is written as its JSON object, `Submitted` and tickets as the fields of their records; `wait`'s answer is the fields of `Changes`; an unknown `cmd` is answered `{"id": <id>, "ok": false, "error": "device/ctl_request"}`".
- Evidence: `smoke::ctl_drives_a_device_by_the_protocol` (against the stand-in; no device exists yet).
