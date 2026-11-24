# Stage `relay`: status

*In progress. Lead: one Fable session that writes the whole stage itself (the owner's instruction
for this stage; agents on model `opus` only check the finished work). Branch
`claude/amazing-hawking-4p2tf6`.*

## Resuming: read this first

1. The contract is `docs/sync/spec/relay.md`. The code is `relay/relay-core.js` (every rule),
   `relay/worker.js`, `relay/standby.js`; the suite is `relay/test/`.
2. Run what exists: `bash relay/test/run.sh core` (then `standby`, `worker`), or directly
   `cd relay && TD_TARGET=core node --test 'test/*.test.mjs'`.
3. The vector families are written in `relay/test/vectors/cases-*.mjs` with the short forms of
   `dsl.mjs`; `node relay/test/vectors/build.mjs` rewrites the JSON files, and the suite fails
   when a file is stale.
4. The task list below says what is done and what is next.

## Tasks

| # | Task | State |
|---|---|---|
| 1 | `relay-core.js`: encodings, credentials, strict Ed25519, the gate, every route and admin operation, sockets | written; tested on the core target for the families below |
| 2 | harness (`relay/test/lib/harness.mjs`), runner, vector generator | written |
| 3 | families gate, token, address, create, login, append, read | done: green on all three targets |
| 4 | `standby.js` and the standby target | done |
| 5 | `worker.js`, `wrangler.toml`, the harness Worker (`relay/test/worker/`) and the worker target | done; `bash relay/test/run.sh worker` starts `wrangler dev` itself |
| 6a | families blobs, pairing, admin | done: 107 cases on `core`, 68 on `standby`, 62 on `worker` |
| 6b | families seed, compact, restore, purge, budget, socket, standby, rows, day | next |
| 7 | Wycheproof and seal's vectors; source scans; `ci.sh`, `deploy.sh`, README | |
| 8 | gate green, the checks by agents (spec, works, security x2, hygiene), fixes, exit | |

## Decisions (with reasons)

- **D1. Order of checks at the object: the token's row first (§8.2, RELAY-OBJ-003), then the board
  state (§8.1).** Both are MUSTs and they overlap on an empty or seeding board; RELAY-OBJ-003 says
  "before any other check of the route". So a MAC-valid token on a board that does not exist is
  answered `401 relay/unauthorized`, and `404 relay/no_board` is what the door gets.
- **D2. The nonce of a login or admin request is found by reading the nonces of its purpose and
  device and comparing each in constant time** (RELAY-GEN-002 names nonces), not by a SQL lookup
  on the nonce value. At most 64 rows are read.
- **D3. `node --test test/` does not run a directory on Node 22** (it takes the argument as a
  file). `package.json` and `run.sh` use the glob `test/*.test.mjs`; helpers live in `test/lib/`
  and the generator in `test/vectors/`, which the glob does not match. §23's wording is for the
  spec-change pull request.
- **D4. The vector files are generated** from `relay/test/vectors/cases-*.mjs`, so that one case
  is a few lines; the JSON is what is published (relay.md §21). The builders beyond those §21
  names are listed in `relay/test/vectors/README.md`.

- **D5. A token that was never issued can name the stored generation.** `revoke` raises the
  generation without issuing a token and `reinstate` leaves it there (§5.2), so after a
  reinstatement the stored generation is one no token carries; only the relay can mint a token, so
  nothing follows from it, and the device logs in for the next generation.
- **D6. `mint_read_token` writes the writer's own row once.** When it raises `read_epoch` it marks
  the rows of lower epochs revoked except the writer's, which the same request then rewrites; the
  rows written are then the `3 + r` of §16.1.
- **D7. A `register` whose carried frame is already stored, for a device that is not registered**
  (the frame went in by a plain append first) is refused with `409 relay/conflict` and the author's
  `last_ctr` and `last_seq`. §12.3 names no code for a `dup` outcome there; the client uses the seq
  mode. Low; for the spec-change pull request.
- **D8. Rows read on the platform count the rows a write touches too** (measured under
  `wrangler dev`: an `UPDATE` of one row reads one row). Rows written agree exactly with §16.1.

## For the spec-change pull request (collected, not yet opened)

- §16.1 rows read: several bounds cannot be met as written (a devices scan for key uniqueness,
  the nonce scan of D2). Measured values to follow.
- §23: the `test` script (D3).
- §12.5: `first_epoch ≥ 1` against seal's registry (`first_epoch` below 2 is never signed; seal's
  status file asks the relay to refuse it).

## Token ledger

Agents run only for the final checks; their tokens are recorded here when they run.
