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
| 4 | the nineteen vector families of spec §21 | done: 143 cases |
| 5 | Wycheproof and seal's vectors (`strict.test.mjs`); the day and the import (`day.test.mjs`); standby and scan tests | done |
| 6 | `ci.sh`, `deploy.sh`, `README.md`, `test/vectors/README.md` | done |
| 7 | the spec change (TT-SAL/taskdeck#25) | its two commits are merged into the stage branch; the pull request itself is merged before the stage's |
| 8 | gate in the lead's run | passes: core 166 tests, standby 106, worker 118; coverage 183 of 183 clauses |
| 9 | the checks by agents (spec, works, security x2, hygiene), fixes of high and medium findings | done, one round and one repeat (below) |
| 10 | exit: status `final`, the exit pull request's body, merge | see the header |

## What was built

- **`relay/relay-core.js`** (1,944 lines, one ES module, imports `node:crypto` only): tokens,
  the door, the relay address, `verifyStrict`, the gate (`matchRoute`, `checkHeaders`,
  `checkCredential`, `gateAnswer`, and `gate` for steps 0 to 8 in one call), the storage schema and
  every SQL statement, `BoardCore` with all 21 routes, the 11 admin operations and the socket
  messages, `sqliteStorage` and `noSockets`.
- **`relay/worker.js`**: the Worker (gate, body under its deadline, one call to the object) and
  the Durable Object `Board` (SQLite, hibernating sockets, the `ping` auto-response).
- **`relay/standby.js`**: the same gate and core on Node 22 with `node:sqlite`; one file and one
  queue per board; `startStandby` for other stages' tests.
- **The suite**: 143 vector cases in 19 families, Wycheproof (210 cases) and seal's vectors, the
  typical day and the import, standby, source-scan and in-process Worker tests; three targets.

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
exports as built (D3, D4, D9, D12, D13). §4.1, §15.2: the platform rewrites a request target before
the Worker runs (from the checks, below). Affected: `engine` (model relay: the order of §8.2, the
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

## The checks (one round, by agents that did not write the work)

Run as one workflow of five agents on model `opus`, two at a time, after the gate passed in the
lead's run; then one repeat for the code a high finding changed. The lead reproduced every finding
it acted on before changing anything.

| Check | Findings | Tokens (input, cache writes and output; cache reads apart) |
|---|---|---|
| spec: the work against `relay.md` clause by clause and plan §6.4; the status file | 1 high, 1 low | 334,013 (output 52,890; cache reads 11.96 M) |
| works: the suite twice on each target, the gate, 30 cases of its own, standby against Worker | 2 low | 340,170 (output 88,934; cache reads 19.58 M) |
| security, authentication: tokens, signatures, nonces, replays, privilege | 1 high, 3 low | 217,336 (output 50,351; cache reads 7.78 M) |
| security, input: parsers, limits, memory, budgets, the gate | 4 low | 262,294 (output 66,586; cache reads 10.57 M) |
| hygiene: what is committed | 1 medium, 1 low | 47,020 (output 10,855; cache reads 1.89 M) |
| **the round** | 2 high, 1 medium, 11 low (four of them the same path finding) | 1,200,833; the harness reports 1,161,552 for the workflow |
| repeat: the change to `worker.js`, and the claim about the request target | running when this was written | |

No agent failed, and every agent's own run of the suite and of the gate passed.

### What was found and what was done

- **High, RELAY-GEN-006 (security, authentication): every board object decoded `TOKEN_KEY` and
  `CREATE_SECRET` again.** `Board`'s constructor called `makeConfig`. Fixed: the object takes the
  Config the Worker decoded (`configOf(env)` in `worker.js`). Test: `relay/test/worker.test.mjs`
  loads the shipped `worker.js` in process under a stand-in for `cloudflare:workers`; "the Worker
  and every board object of an isolate share one decoded configuration" failed on the old file.
  The same file now also shows what the Worker hands to the object (RELAY-GATE-010).
- **High as reported, RELAY-ENC-001 (spec; the other three checks called it low): the Worker
  answers `/v1/b/{board}/x/../caps` as `/v1/b/{board}/caps`, the standby with 404.** Reproduced, and
  traced to the platform: under `wrangler dev`, `request.url` already arrives with `.` and `..`
  segments resolved, `\` turned into `/` and the fragment dropped, and a Worker has no other view
  of the request target. No code can refuse what it cannot see, so nothing was changed in
  `worker.js`. Done instead: the spec says so (§4.1, and a row of §15.2, in TT-SAL/taskdeck#25); two
  gate cases pin it ("a path that is not written as the route table writes it" on the core and the
  standby, "what the platform makes of a path before the Worker sees it" on the Worker). The gate
  checks the credential on the path it received, so a rewritten target reaches nothing its
  canonical form would not.
- **Medium (hygiene): `relay/.wrangler/cache/cf.json` was committed** in the stage's second commit:
  what Cloudflare reported about the machine that ran `wrangler dev` (its network operator, city,
  coordinates, a TLS fingerprint of the session). It describes the cloud container, not the owner,
  and holds no credential. Fixed: the file is removed and `.wrangler/` is ignored
  (`relay/.gitignore`); `run.sh` runs wrangler on a copy of the sources in a temporary directory and
  sets `CLOUDFLARE_CF_FETCH_ENABLED=false`, so that fetch no longer happens; a test in
  `scan.test.mjs` fails if such a file is tracked. **The file is still in the branch's earlier
  commits** (see the open questions).
- The brief the lead gave the agents named the clause-index tool wrongly (it writes to its second
  argument): three agents so wrote `relay.md`, a generated index, into the repository root. It was
  never tracked and is deleted.

## Low findings left as they are

Listed, not fixed (the owner's rule for this stage).

1. `relay/deploy.sh` accepts an `--origin` that the relay then refuses as misconfigured
   (`https://x.example:443`, `https://a..b`, a label that starts with `-`): such a deploy would
   answer 503 to every request. For stage `release`, which owns the deploy path: check the origin
   with `configError` of `relay-core.js`.
2. `deploy.sh`'s guard against CI and cloud sessions matches `CI=true` and
   `CLAUDE_CODE_REMOTE=true` exactly; another spelling (`CI=1`) passes it. It still needs a
   logged-in wrangler to do anything.
3. The standby's start-up error for an unreadable or invalid TLS file does not name the variable
   and prints the file's path (Node's own message).
4. A request header named `__proto__` or `constructor` makes the front's header collection throw:
   the Worker answers the platform's bare 500, the standby `503 relay/object_unavailable`. No
   object call, no state change; an honest client sends no such header.
5. A socket opened with a token that a repeated `create` or `register` then made stale stays
   connected and receives pushes until its next message, which is answered `401 relay/token_stale`
   and close 4003 (§5.2 closes sockets on login and revoke only). Pushes carry sealed frames the
   same device can read with its new token.
6. The standby holds a request body in memory until it is complete: someone with the door or a
   token can make it hold about 4 MiB per connection for up to 74 s, and the number of connections
   is not limited. The standby is meant to sit behind a proxy that buffers requests (SYNC.md §6.7).
7. The standby's 10 s header deadline is enforced by Node only at its 30 s check interval
   (`connectionsCheckingInterval`), so a client without a credential can hold a connection for up
   to about 40 s.
8. A repeated or malformed `Content-Length` never reaches the gate: Node answers its own 400
   without `X-TD-Relay` on the standby, and the Worker's platform merges two identical ones. The
   gate's own handling of them is tested on the core target.
9. `docs/sync/spec/relay.md` (stage `foundation`'s text, not this stage's diff) uses
   `relay-x.workers.dev` and `relay-x7k2.example.workers.dev` as symbolic examples; they lie under
   a real Cloudflare domain. This stage's own files use reserved names only.

Noted by the spec check as left open inside the spec, not as findings; the code follows the
section named last in each: §5.2's table does not list the generation raised by a repeated
`create` (§10.2) or `register` (§12.3); RELAY-SEED-005 says `409 relay/seeding` where §12.7 says
`403 relay/forbidden` for a `seed-tail` from another device (no other device can hold a valid
token while a board is seeding); §5.2 says `purge` revokes every read token, §12.10 only when the
floor rises; an optional `CREATE_SECRET` that is present but malformed makes the configuration
invalid as a whole (§17.1 names only `TOKEN_KEY` and `RELAY_ORIGIN`).

## Token ledger

The stage was written by the lead; no agent wrote code. Agents ran only for the checks: 1,161,552
tokens for the round as the harness counts them (per agent in the table above), and the repeat's
tokens are added when it ends.

## Open questions for the owner

None blocks the stage. Two things to know:

- **The committed cache file is gone from the tree but not from the branch's history.** The stage's
  pull request is squash-merged, so `day-planner` never holds it, and it leaves GitHub's reachable
  history when the branch `claude/amazing-hawking-4p2tf6` is deleted after the merge. Purging it
  sooner means rewriting the branch and force-pushing; say so if you want that.
- **A second branch was pushed**, `claude/relay-spec-change`, because a spec change goes in a pull
  request of its own (TT-SAL/taskdeck#25). It can be deleted once that pull request is merged.

## What the next stages need to know

**For every stage that talks to the relay.**

- **A relay for your tests** is the standby, in process:
  `import { startStandby } from '<repo>/relay/standby.js'`, then
  `await startStandby({ tokenKey, createSecret, origin, dataDir, listen: '127.0.0.1:0', now })`. It
  returns `{ url, server, close }`. `tokenKey` and `createSecret` are 64 lowercase hex characters;
  without `createSecret` no board can be created or seeded; `dataDir` must exist (one file
  `<board hex>.sqlite` per board appears in it); `origin` is the relay origin devices sign in their
  login (`https://host` or `https://host:port`, lowercase, never `:443`), not where the standby
  listens; `now` is an injected clock for tests. It serves plain HTTP, reports `push: false`, and
  answers the socket route with `501 relay/no_push`: clients poll `hello` and `read`.
- **The Worker for your tests** is `wrangler dev` (the pinned one of `bash scripts/dev-setup.sh
  relay`) with the shipped `relay/wrangler.toml` and three values of your own:
  `--var TOKEN_KEY:<hex> --var CREATE_SECRET:<hex> --var RELAY_ORIGIN:https://<host>`. Copy the
  rest of the command from `relay/test/run.sh`: it runs wrangler on a copy of the sources outside
  the repository, with `WRANGLER_SEND_METRICS=false` and `CLOUDFLARE_CF_FETCH_ENABLED=false`, with
  a `HOME` and a state directory of its own. Without that copy wrangler writes `.wrangler/` next to
  the configuration; it is ignored under `relay/`, nowhere else.
- **Every request body is binary and every body route needs one decimal `Content-Length`**: a
  chunked upload is `411 relay/length_required`. Answers are JSON or binary, told apart by
  `Content-Type`; every answer of the relay's own code carries `X-TD-Relay: gate` or `object`. On
  the standby a malformed or repeated `Content-Length` is answered by Node itself (a 400 without
  `X-TD-Relay`): classify it as a platform failure (spec §14.5).
- **The published vectors** (`relay/test/vectors/*.json`, format in the `README.md` beside them)
  state request by request what the relay answers and stores; `relay/test/lib/harness.mjs` runs
  them against any target that offers `request`, `stats`, `sql` and `openSocket`.

**For `engine`** (the model relay follows `relay.md`; these are the points decided here, all in the
spec after TT-SAL/taskdeck#25 unless marked):

- The token's row is checked before the board state: a MAC-valid token on a board that does not
  exist is `401 relay/unauthorized`, and `404 relay/no_board` is what the door gets (§8.2).
- `reinstate` needs `first_epoch ≥ 2`; it leaves the generation where `revoke` put it, one that no
  token carries, and the device logs in for the next one (decision D5, §5.2).
- A `register` whose `DeviceAdded` is already stored for a device that is not registered is
  `409 relay/conflict` with the author's `last_ctr` and `last_seq`; register it in seq mode (§12.3).
- A repeated `create` or `register` gives a new token and raises the generation without closing
  sockets: a socket of the old generation is answered `401 relay/token_stale` and closed with 4003
  at its next message, and receives pushes until then.
- At most 64 nonces are kept for a board, login and admin together; issuing one more evicts the
  oldest. A device whose nonce was evicted gets `401 relay/no_nonce` and asks again.
- A u64 argument above 2^53 − 1 is refused by the check that names its field, after the nonce is
  used: `409 relay/target` for `compact.below_seq` and `purge.before_seq`, `409 relay/no_snapshot`
  for `compact.snap_entry_seq`, `400 relay/bad_args` for a keep seq and for a seed's fields
  (decision D10).
- Rows: an append of `f` new frames by `a` authors writes `2f + a` rows; a deleted `log` row is one
  row written. The read bounds of §16.1 are formulas (`relay/test/vectors/reads.mjs`).

**For `device`.**

- Plan §6.7 item 2 asks you to add seal's vectors to the relay's suite: they are in it already
  (`relay/test/strict.test.mjs`; decision D12 says through which routes).
- The client must send exactly one `Authorization` header in one of the three forms of §5.6, a
  session id of 16 bytes in the socket hello (the first message, 21 bytes), and nothing in
  `X-TD-` headers: the Worker drops them, and the relay takes the credential only from
  `Authorization`.
- The text `ping` is answered `pong` by the platform on the Worker only; the standby has no socket.
- The body deadline is 10,000 ms plus 1,000 ms per 65,536 bytes of the declared length, on both
  fronts; `408 relay/slow` is transient (§14.5).
- On the Worker the platform rewrites a request target with `.` or `..` segments, a backslash or a
  fragment before the relay sees it (§4.1): send canonical paths, as the route table writes them.

**For `recovery`.**

- With the door: `admin-nonce` and `admin` accept only the recovery identity as signer; `append`
  accepts only frames authored by a recovery identity; neither counts against any device's budget.
- `mint_read_token` as `dev_R`: ask with epoch 1, then with the `read_epoch` that
  `409 relay/stale_epoch` carries.
- `reinstate`: `first_epoch ≥ 2` and 1 to 256 wraps of 96 bytes; the target logs in with the door
  and its login answer carries the wraps until the next `revoke`.
- `fence` with its frame over the door: a `409 relay/gap` or `409 relay/conflict` carries
  `last_ctr` and `last_seq` to re-seal with.
- `recovery_register` only in seq mode, after the entry is stored; a repeat by the new identity or
  by a device is answered 200 and writes only the nonce.

**For `release`.**

- `relay/deploy.sh` is the simple form for the owner's machine (decision D11): `wrangler deploy`
  with `--name` and `--var RELAY_ORIGIN:…`, `wrangler secret put` for `TOKEN_KEY` (drawn once per
  Worker name, never shown, never replaced) and `CREATE_SECRET` (shown once; `--close` deletes it,
  `--open` sets a new one). It refuses to run when `CI=true` or `CLAUDE_CODE_REMOTE=true`; its
  `--dry-run` only bundles and runs in CI. The spec names the Script Upload API (§17.1): replace
  the script if you build that. Low findings 1 and 2 are about it.
- The relay bundle is `relay-core.js`, `worker.js` and `wrangler.toml` (86 KiB bundled).
  `wrangler.toml` names no real Worker: the name is given at deploy.
- For the rehearsal's measurements (SYNC.md §11): everything here was measured under
  `wrangler dev`, not on Cloudflare. Still to take on the real platform: the row counters (a
  deleted `log` row counted once; rows read), the duration, what the edge does to a request target
  (§4.1), to repeated headers and to a slow or chunked upload, the size cap of a socket message
  (the suite sends 4,194,379 bytes), and what a client sees when the object is over its quota.

**For `rc`.**

- The baseline: the typical day is 202 Worker requests, 218 object requests, 1,226 rows written
  (1.23%) and 4,376 rows read; the import 317, 317, 10,319 (10.32%) and 6,900. The suite fails
  above 2% and 20%.
- The low findings above are yours to weigh; 6 and 7 matter only if the standby ever faces the
  internet without a proxy in front of it.
- `bash scripts/gate.sh relay` needs the pinned wrangler (`bash scripts/dev-setup.sh relay`) and
  takes about two and a half minutes in a container.

**Working in `relay/` later.** One writer at a time. Every rule lives in `relay-core.js`; the two
fronts only read the body and call it, and `scan.test.mjs` fails if they start to decide. A change
of behaviour is a change of a vector case in `relay/test/vectors/cases-*.mjs`, then
`node relay/test/vectors/build.mjs`. A check whose refusal must keep an earlier write (a used
nonce, a counted try, a lazy cleanup) returns its error inside the transaction instead of throwing
it; every other check stands before the operation's first write.
