# relay/

The blind relay of TaskDeck sync: it sequences signed, encrypted frames for one board and
never holds a key that opens them. Contract: [`docs/sync/spec/relay.md`](../docs/sync/spec/relay.md).
Owner: stage `relay` (SYNC_PLAN.md §4, §6.4).

| File | What it is |
|---|---|
| `relay-core.js` | Every rule of the spec: encodings, tokens, the door, strict Ed25519, the gate, the routes, the admin operations, the socket messages, all SQL. One ES module; it imports `node:crypto` only. |
| `worker.js` | The Cloudflare Worker (the gate, then one call to the object) and the Durable Object `Board` (SQLite storage, hibernating sockets). It decides nothing itself. |
| `standby.js` | The same gate and core on Node 22 with `node:sqlite`, over plain HTTPS polling. It is also the local relay for other stages' tests. |
| `wrangler.toml` | The Worker's configuration (spec §17.1). No name of a real Worker, no address and no secret is in it. |
| `deploy.sh` | For the owner, on the owner's machine. Sessions and CI never run it. |
| `test/` | The conformance suite (spec §18, §21). It does not ship. |
| `ci.sh` | The stage's CI checks (`bash scripts/ci/stage.sh relay` runs it). |

There are no runtime dependencies. `wrangler` is the only development dependency, pinned; the
suite takes it from `bash scripts/dev-setup.sh relay`, never from `npm install` here.

## Running the suite

```bash
bash scripts/dev-setup.sh relay        # once: the pinned wrangler (cloud sessions and CI only)
bash relay/test/run.sh core            # the core in process, injected clock, counting storage
bash relay/test/run.sh standby         # the standby over HTTP on loopback
bash relay/test/run.sh worker          # the Worker under `wrangler dev`, no account, no network
bash scripts/gate.sh relay             # the stage's gate: all three, and the scans
```

Extra arguments go to `node --test`, for example `--test-name-pattern='relay-append'`.
`TD_ROWS=1` prints the rows each measured request wrote and read.

What is where under `test/`:

- `suite.test.mjs` runs every case of every vector family on the target `TD_TARGET` names.
- `vectors/*.json` are the published families (spec §21). They are generated: the cases are
  written in `vectors/cases-*.mjs` with the short forms of `vectors/dsl.mjs`, and
  `node relay/test/vectors/build.mjs` rewrites the JSON. The suite fails when a file is stale.
  `vectors/README.md` describes the case format.
- `strict.test.mjs` runs Wycheproof's `ed25519_test.json` and seal's published vectors
  (`docs/sync/vectors/seal-frame.json`, `seal-signatures.json`, `seal-chain.json`) as they are.
- `day.test.mjs` replays the typical day and the 5,000-command import of spec §16.2 and checks
  the totals against the free plan's daily limits.
- `standby.test.mjs` and `scan.test.mjs` cover what only the standby does and what is checked by
  reading the sources.
- `lib/harness.mjs` is the harness: the builders, the three targets, the expectation checks.
- `worker/` is the suite's harness Worker: the shipped `worker.js` wrapped by test-only routes
  under `/__test/` (object calls, the platform's row counters, a clock offset, a table dump).
  `wrangler dev` runs it; it is never deployed, and the shipped `worker.js` has no such route.

## The standby

```bash
TD_TOKEN_KEY=<64 hex> TD_RELAY_ORIGIN=https://<host> TD_DATA_DIR=<dir> TD_LISTEN=127.0.0.1:8443 \
  node relay/standby.js
```

`TD_CREATE_SECRET` (64 hex) enables creating or seeding a board and is removed afterwards;
`TD_TLS_CERT` and `TD_TLS_KEY` (PEM files) make it serve HTTPS itself, otherwise it serves HTTP
for a TLS proxy in front of it or for tests on loopback. A missing or invalid value ends the
process with status 2 and one line that names the variable. It keeps one SQLite file per board
in `TD_DATA_DIR` and stops on `SIGTERM`.

In a test of another stage, start it in process:

```js
import { startStandby } from '../relay/standby.js';
const relay = await startStandby({ tokenKey, createSecret, origin, dataDir, listen: '127.0.0.1:0' });
// relay.url is http://127.0.0.1:<port>; await relay.close() when done
```

`origin` is what devices sign in their login (spec §10.3); it need not be where the standby listens.

## The Worker

`TOKEN_KEY` and `CREATE_SECRET` are secrets, `RELAY_ORIGIN` is a variable (spec §17.1).
`TOKEN_KEY` stays the same for the whole life of a deployment: a new key is a new relay, which
boards reach only through the fenced move. Deploying is the owner's step (`deploy.sh`, and the
runbook that stage `release` writes); nothing in this repository names a real relay.
