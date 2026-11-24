# Stage `relay`: status

*In progress: built, gate green in the lead's own run, the checks by agents are next. Lead: one
Fable session that wrote the whole stage itself (the owner's instruction for this stage: no
implementation by agents; agents on model `opus` only check the finished work, one round). Branch
`claude/amazing-hawking-4p2tf6`, draft pull request TT-SAL/taskdeck#24.*

## Resuming: read this first

1. The contract is `docs/sync/spec/relay.md`. The code is `relay/relay-core.js` (every rule),
   `relay/worker.js`, `relay/standby.js`; the suite is `relay/test/`; `relay/README.md` says what
   is where.
2. Run what exists: `bash scripts/dev-setup.sh relay` once, then `bash scripts/gate.sh relay`
   (or one target: `bash relay/test/run.sh core|standby|worker`).
3. The vector families are written in `relay/test/vectors/cases-*.mjs` with the short forms of
   `dsl.mjs`; `node relay/test/vectors/build.mjs` rewrites the JSON files, and the suite fails
   when a file is stale. The bounds on rows read are `relay/test/vectors/reads.mjs`.
4. The task table says what is done and what is next. Everything is committed and pushed after
   each step.

## Tasks

| # | Task | State |
|---|---|---|
| 1 | `relay-core.js`: encodings, credentials, strict Ed25519, the gate, every route and admin operation, sockets | done |
| 2 | harness (`relay/test/lib/harness.mjs`), runner, vector generator | done |
| 3 | `standby.js`; `worker.js`, `wrangler.toml`; the harness Worker (`relay/test/worker/`) | done |
| 4 | the nineteen vector families of spec §21 | done: 141 cases |
| 5 | Wycheproof and seal's vectors (`strict.test.mjs`); the day and the import (`day.test.mjs`); standby and scan tests | done |
| 6 | `ci.sh`, `deploy.sh`, `README.md`, `test/vectors/README.md` | done |
| 7 | the spec change (TT-SAL/taskdeck#25) | open, waiting for CI; then merge `day-planner` into the stage branch |
| 8 | gate in the lead's run | passes: core 161 tests, standby 101, worker 113; coverage 183 of 183 clauses |
| 9 | the checks by agents (spec, works, security x2, hygiene), fixes of high and medium findings | next |
| 10 | exit: status `final`, the exit pull request's body, merge | |

## What was built

- **`relay/relay-core.js`** (about 1,150 lines, one ES module, imports `node:crypto` only): tokens,
  the door, the relay address, `verifyStrict`, the gate (`matchRoute`, `checkHeaders`,
  `checkCredential`, `gateAnswer`, and `gate` for steps 0 to 8 in one call), the storage schema and
  every SQL statement, `BoardCore` with all 21 routes, the 11 admin operations and the socket
  messages, `sqliteStorage` and `noSockets`.
- **`relay/worker.js`**: the Worker (gate, body under its deadline, one call to the object) and
  the Durable Object `Board` (SQLite, hibernating sockets, the `ping` auto-response).
- **`relay/standby.js`**: the same gate and core on Node 22 with `node:sqlite`; one file and one
  queue per board; `startStandby` for other stages' tests.
- **The suite**: 141 vector cases in 19 families, Wycheproof (210 cases) and seal's vectors, the
  typical day and the import, standby and source-scan tests; three targets.

## Decisions (with reasons)

- **D1. Order of checks at the object: the token's row first (§8.2, RELAY-OBJ-003), then the board
  state (§8.1).** Both are MUSTs and they overlap on an empty or seeding board. A MAC-valid token
  on a board that does not exist is answered `401 relay/unauthorized`; `404 relay/no_board` is
  what the door gets. In the spec change (§8.2).
- **D2. The nonce of a login or admin request is found by reading the nonces of its purpose and
  device and comparing each in constant time** (RELAY-GEN-002 names nonces), not by a SQL lookup
  on the nonce value. At most 64 rows are read, twice for one request.
- **D3. `node --test test/` does not run a directory on Node 22.** `package.json` and `run.sh` use
  the glob `test/*.test.mjs`; helpers live in `test/lib/`, `test/vectors/` and `test/worker/`,
  which the glob does not match. In the spec change (§18, §23).
- **D4. The vector files are generated** from `relay/test/vectors/cases-*.mjs`, so that one case
  is a few lines; the JSON is what is published (spec §21). `relay/test/vectors/README.md` lists
  the builders, step kinds and expectations, more than §21 names.
- **D5. A token that was never issued can name the stored generation.** `revoke` raises the
  generation without issuing a token and `reinstate` leaves it there (§5.2), so after a
  reinstatement the stored generation is one no token carries; only the relay can mint a token,
  and the device logs in for the next generation.
- **D6. `mint_read_token` writes the writer's own row once**: when it raises `read_epoch` it marks
  the rows of lower epochs revoked except the writer's, which the same request rewrites. The rows
  written are then the `3 + r` of §16.1.
- **D7. A `register` whose carried frame is already stored, for a device that is not registered**
  (the frame went in by a plain append first) is refused with `409 relay/conflict` and the author's
  counters; the client then uses the seq mode. In the spec change (§12.3).
- **D8. Rows on the platform, measured under `wrangler dev` 4.166.0**: a deleted `log` row is one
  row written, not two; rows read count every row a statement touches, the rows a write changes
  included. Rows written agree exactly with §16.1 once `2d` reads `d`. In the spec change.
- **D9. The harness Worker's clock follows the case's clock** (a test-only route sets an offset
  for each board's object), so that the compaction case and the import over two UTC days run
  against the platform's counters. Cases whose expectations turn on an exact instant still leave
  the Worker target out. In the spec change (§18).
- **D10. A u64 above 2^53 − 1 in an admin argument is read as "no value" and refused by the check
  of §12 that names its field** (RELAY-GEN-008), after the nonce, with that check's code; a u64 in
  a fixed layout before any nonce (a blob's size, a frame's counter) is refused where it is read.
- **D11. `deploy.sh` is written for the owner and refuses to run in CI or a cloud session.** It
  uses `wrangler deploy` and `wrangler secret put`. The spec leaves the upload through the Script
  Upload API and the runbook to stage `release` (spec §1, §17.1), the plan lists a `deploy.sh`
  under this stage (§6.4 item 5): this one is the simple form, and `release` may replace it. Its
  `--dry-run` (bundle only, nothing uploaded) runs in CI and proves the shipped Worker bundles.
- **D12. Seal's frame vectors reach the log through `create` and `seed-tail`, not `append`.** The
  suite does not hold the private keys of seal's vectors, and `append` takes a device's frame only
  with that device's token. Seal's Genesis frame creates its own board (its payload carries the
  salt); the four device frames are a seed's tail, checked by the same code as an append; the two
  login signatures go through `login` on the core target, where the relay's nonce can be chosen.
- **D13. `startStandby` also returns `server`** (the `node:http` server), for the suite: it reads
  the two timeouts of §15.2 from it and uses it to fix the arrival order of two requests.

## The spec change (TT-SAL/taskdeck#25)

`docs/sync/spec/relay.md` and `CHANGES.md` only; no byte layout, route, status or code changes.
§7.1, §14.2, §16.1, §16.2, §18: the rows as measured (D8) and the read bounds as formulas.
§8.2: D1. §12.5: `first_epoch ≥ 2` (seal's Q80). §12.3: D7. §18, §21, §23: the suite and the core's
exports as built (D3, D4, D9, D12, D13). Affected: `engine` (model relay: the order of §8.2, the
`first_epoch` floor), `device` and `recovery` (a `reinstate` with `first_epoch` 1 is refused).

## Measurements

Taken by the suite; the Worker figures are the platform's counters under `wrangler dev` 4.166.0
(local runtime, no account), in the container and again in CI with the same numbers.

| What | Worker requests | Object requests | Rows written | Rows read |
|---|---|---|---|---|
| The typical day of spec §16.2 (Worker target, snapshot counted whole, compaction from the core) | 202 (0.20%) | 218.2 (0.22%) | 1,226 (1.23%) | 4,376 (0.09%) |
| The same on the core's counting storage | 202 | 218.1 | 1,228 | 3,710 |
| A 5,000-command import over two UTC days | 317 (0.32%) | 317 (0.32%) | 10,319 (10.32%) | 6,900 (0.14%) |
| Limits of the free plan, per day | 100,000 | 100,000 | 100,000 | 5,000,000 |

- Rows written for one operation: exactly §16.1 on both counters (family `relay-rows.json` and
  the `rows_written` of 130 steps elsewhere).
- A compaction of 4,996 entries: 5,000 rows written, 5,019 rows read.
- The slow-body refusal: 408 after 10,024 ms on the standby and 10,131 ms on the Worker (limits
  10,016 to 11,016).
- Suite duration in the container: core 23 s, standby 39 s, worker 86 s (CI: 11 s, 24 s, 52 s).
- The shipped Worker's bundle: 86 KiB (22.8 KiB gzip).

## Deviations from the plan

- **How the stage is run**: by the owner's instruction, the lead wrote everything and agents only
  check at the end (plan §3.1 and §7 say agents write). Everything else in §3 holds.
- **The compaction of the typical day is counted on the core target** and added to the Worker's
  figures, as RELAY-ROWS-002 says; its rows read are taken as the bound of §16.1, because the
  core's counting storage sees only the rows its queries return.
- No new checker or scanner script was written; the source scans are tests of the suite
  (`relay/test/scan.test.mjs`), which spec §18 item 7 asks for.

## Facts found along the way

- `wrangler dev` passes a request body to the Worker as it arrives: the slow-body refusal works
  there as on the standby.
- Node's HTTP parser answers a malformed or 20-digit `Content-Length` itself with a 400 that
  carries no `X-TD-Relay`; those cases of the gate run on the core target only.
- The hibernation API under `wrangler dev` behaves as the spec assumes: tags, attachments,
  `getWebSockets`, the `ping` auto-response, close codes 4001 to 4004 and 1009, a message of
  4,194,379 bytes.
- CI (`ubuntu-latest`) runs the whole `relay/ci.sh` in about two minutes.
- A push to the pull request starts two runs (the push and the pull request); the first relay job
  failed once on a test of the suite that depended on a 5 ms delay (the standby's arrival order).
  The test now fixes the order by an event and fails without the queue.

## Token ledger

No agent ran while the stage was written. The checks' tokens are recorded here when they run.

## Low findings left as they are

(To be filled from the checks.)

## Open questions for the owner

None so far.

## What the next stages need to know

(Written at the exit.)
