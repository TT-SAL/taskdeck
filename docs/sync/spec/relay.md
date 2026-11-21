# TaskDeck sync spec: relay

*Every route and socket message of the blind relay, its credentials and gate, sequencing and idempotency, caps, budgets and errors, the storage schema, snapshots and blobs, pairing tickets, the signed admin operations (compaction, purge, fence, seed, restore tail and the rest), push over hibernating sockets and plain HTTPS polling on the standby. Owning stage: `relay`. Conventions, notation and the glossary are in [`README.md`](README.md); this spec does not restate them.*

## 1. Scope

This spec fixes every byte the relay accepts or sends, every row it writes, and every decision it takes. The `relay` stage (SYNC_PLAN.md §6.4) builds it under `relay/`: `relay-core.js` behind a storage interface, `worker.js` (the Worker and the Durable Object), `standby.js` (Node 22 with `node:sqlite`) and the conformance suite. Stage `engine` builds its model relay from it (SYNC_PLAN.md §6.5), stage `device` the client (SYNC_PLAN.md §6.7), stage `recovery` its online steps.

It realises:

- SYNC.md §2.1 (the relay row), §2.2, §4.1 (relay storage), §4.3 (append, sequencing, idempotency, one live socket per device), §4.7 (catch-up pages, snapshot retention, compaction, purge at the relay), §4.8 (restore tail, the `/v1` path, additive deploys), §4.9 (push, free pings), §5.2 (tokens, admin operations, backup-read tokens), §5.3 (tickets and slots at the relay), §5.4 (what revocation does at the relay), §5.5 (the recovery identity at the relay), §6 (hosting, quota math, EUR 0, failure modes, exit plan), §7 (the relay, DoS and theft rows), §8 (the relay paragraph);
- R3 §2, §5, §6, §7, §8; the verifier findings V1-1, V1-5, V1-10, V1-16, V2-3, V2-9, V2-15, V2-21;
- SYNC_PLAN.md §2 (no hub, no forwarding, no always-on device), §3.4 (security rules), §5 (wrangler, `node:sqlite`), §6.4 (the `relay` stage and its gate), §6.5 (the model relay), §6.7;
- the rulings S1–S4, S9, S13–S15 of `docs/sync/status/foundation.md`.

Non-goals, each owned elsewhere:

- the frame, login and admin signature layouts, the admin argument registry, the chain, the pairing slots and the context strings: `seal.md` (§8, §15, §16, §17, §4.1). This spec uses them and adds no context string;
- what a client does with any answer (alarms, halts, re-sealing, duties, which device compacts and when): `engine.md`;
- the client's transport, backoff, User-Agent, key stores and its own request budget: `device.md`;
- `relay/deploy.sh`, the runbooks and the release bundle: stage `release`.

The relay is blind: it never holds an epoch root, never decrypts, and never parses anything inside a frame's `body`. There is no hub, no forwarding and no `hub-req`/`hub-resp` (SYNC_PLAN.md §2).

## 2. Terms

Terms used here that README §9 does not define.

| Term | Meaning |
|---|---|
| Worker | The stateless Cloudflare Worker in `worker.js`: the gate, then a call to the object. |
| object | The board's Durable Object (class `Board`), or on the standby the in-process instance of the core for that board. |
| core | `relay-core.js`: every rule of this spec that decides a response or a storage effect. |
| front | The code that receives HTTP: the Worker's `fetch`, or the standby's HTTP server. |
| gate | The checks the front runs before any object call (§6). |
| credential | What a request presents: a token, the door, or the create secret (§5). |
| board row | The single row of table `board` (§7). A board with no board row is **empty**. |
| board state | `open`, `fenced` or `seeding`, from the board row; or `empty`. |
| author row | The row of table `devices` for a frame's `dev`. |
| relay head | `(head_seq, head_chain)`: the highest stored seq and its chain value, or `(base_seq, base_chain)` when no log row exists (§7.3). |
| compaction floor, purge floor | `compact_floor` and `purge_floor` of the board row (§12.8, §12.10). |
| floor | `max(compact_floor, purge_floor)`. |
| relay clock | `now()`: the platform's `Date.now()` in milliseconds. Used only for the expiries, budgets, ages and the `at` column this spec names. |
| UTC day | `floor(now / 86,400,000)`. |
| batch | 1 to 16 frames sent in one append (§9.2). |
| result | The relay's answer for one frame of a batch (§9.5). |
| page | A read answer: the relay head, floors and up to 256 entries (§10.5). |
| push | A page the object sends unasked over a socket after new entries are stored (§11.4). |
| session id | 16 random bytes a client process draws at start and sends in its socket hello (§11.3). |
| try | A slot-B post that reaches the object for an unexpired ticket (§10.8). |
| purpose | What an uploaded blob is for: periodic snapshot, pairing snapshot, genesis snapshot or other blob (§10.7). |
| delete budget | The at most 5,000 log rows that compaction and purge together may delete per UTC day (§12.8). |

## 3. Shape and invariants

### 3.1 Components

| Component | File | Runs | Does | Never does |
|---|---|---|---|---|
| core | `relay/relay-core.js` | inside the object (Worker) or the standby process | every check of §8 to §15, all SQL, building every response and socket message | HTTP parsing, reading bodies, timers, logs, outbound I/O |
| Worker | `relay/worker.js` (default export) | Cloudflare Workers Free | route lookup, the gate (§6), reading the body under its deadline, answering `caps` and `door`, one call to the object | storage, HTML, CORS headers, logs, any fetch other than the object stub |
| object | `relay/worker.js` (class `Board`) | one SQLite-backed Durable Object per board | hands each request and socket message to the core with a `Storage` over `ctx.storage.sql` and a `Sockets` over the hibernation API | timers, alarms, long-polls, non-hibernating sockets |
| standby | `relay/standby.js` | Node 22, `node:sqlite`, `node:http` or `node:https` | the same gate and core over plain HTTPS; one SQLite file per board | sockets, push |
| suite | `relay/test/` | `node --test` | the conformance suite of §21 against the core in process, the standby over HTTP, and the Worker under `wrangler dev` | ship in the relay bundle |

One object serves one board, named `env.BOARD.idFromName(<board_id as 32 lowercase hex>)`.

### 3.2 Invariants

- **RELAY-SHAPE-001** (relay) Every rule of this spec that decides a status, a response body, a socket message or a storage effect MUST be implemented once, in `relay-core.js`; `worker.js` and `standby.js` MUST call it and MUST NOT implement any such rule themselves, except reading the body under its deadline (§6.2 step 9) and the socket plumbing of §11.
- **RELAY-SHAPE-002** (relay) `relay/package.json` MUST declare no `dependencies`, `optionalDependencies`, `peerDependencies` or `bundledDependencies`; its only `devDependencies` entry MUST be `wrangler` at an exact pinned version.
- **RELAY-SHAPE-003** (relay) The relay code MUST import only `node:crypto` (for `createHash`, `createHmac`, `timingSafeEqual`), the platform globals `crypto.subtle` and `crypto.getRandomValues`, `cloudflare:workers` (in `worker.js` only), and `node:http`, `node:https`, `node:sqlite`, `node:fs` and `node:path` (in `standby.js` only).
- **RELAY-SHAPE-004** (relay) A response of the relay MUST NOT carry a `Content-Type` other than `application/json` or `application/octet-stream`, and MUST NOT carry any `Access-Control-` header.
- **RELAY-SHAPE-005** (relay) The relay code MUST NOT call any `console` method, write to any log, or emit analytics, and `relay/wrangler.toml` MUST set `[observability] enabled = false`.
- **RELAY-SHAPE-006** (relay) `worker.js` MUST NOT call `fetch` or open any connection other than the call to the board's object stub.
- **RELAY-SHAPE-007** (relay) The object MUST NOT call `setTimeout`, `setInterval` or `storage.setAlarm`, MUST NOT define an `alarm` handler, and MUST answer every request from the state at the time it handles it, never by waiting for a later event.
- **RELAY-SHAPE-008** (relay) The object MUST accept sockets only with `ctx.acceptWebSocket` (the hibernation API) and MUST NOT let any answer depend on in-memory state that outlives the handling of one request or one socket message, other than the socket attachments of §11.2.
- **RELAY-SHAPE-009** (relay) Every route MUST have a path that starts with `/v1/`.
- **RELAY-SHAPE-010** (relay) Within `/v1`, a deploy MUST NOT remove a route, change a request or response layout, change the meaning of an error code, or send a socket message type to a client whose hello did not announce it (§11.3); it may add routes, tables and columns.
- **RELAY-SHAPE-011** (relay) The standby MUST run on Node 22 with `node:sqlite` and no command-line flags, and MUST serve every route of §10 with the same status, headers and body as the Worker for the same state and request, except the differences listed in §15.2.

### 3.3 Rules for every implementation

These follow from SYNC_PLAN.md §3.4 and bind all relay code.

- **RELAY-GEN-001** (relay) The relay MUST compute SHA-256 only with `node:crypto` `createHash('sha256')`, HMAC-SHA256 only with `node:crypto` `createHmac('sha256', key)`, Ed25519 verification only with `crypto.subtle.verify({name: 'Ed25519'}, …)` behind the checks of §3.4, and random bytes only with `crypto.getRandomValues`; it MUST NOT contain its own implementation of any of these.
- **RELAY-GEN-002** (relay) Every comparison of a MAC, a door, the create secret, a ticket proof or a nonce MUST first compare lengths and then use `timingSafeEqual`, never `===`, `Buffer.equals`, string comparison or an early-exit loop.
- **RELAY-GEN-003** (relay) Every length read from a request (a `Content-Length`, a count, a `u32` length prefix, a query value) MUST be checked against its limit in §20 and against the bytes actually remaining before the relay allocates, slices or reads that many bytes.
- **RELAY-GEN-004** (relay) The front MUST NOT read a request body before the request's credential has been verified (§6.2 steps 1 to 7), and the core MUST NOT read any byte of a frame's `body` other than as an opaque byte string.
- **RELAY-GEN-005** (relay) The relay MUST NOT put `TOKEN_KEY`, `CREATE_SECRET`, a ticket proof, a nonce it has not yet issued, or any byte of a request body into an error response, an exception message or a socket close reason.
- **RELAY-GEN-006** (relay) The relay MUST decode `TOKEN_KEY` and `CREATE_SECRET` once per isolate or process into byte arrays, MUST NOT copy them into any other value, and MUST overwrite with zero bytes (`fill(0)`) every temporary buffer that held the create secret from a request or a ticket proof once the comparison is done.
- **RELAY-GEN-007** (relay) The relay code MUST NOT use `eval`, `new Function`, `import()` with a computed specifier, or `WebAssembly`.
- **RELAY-GEN-008** (relay) Every integer the relay reads from a request or a frame header (seq, `ctr`, generation, epoch, lengths, counts) MUST be within 0 to 2^53 − 1, and a `u64` field above 2^53 − 1 MUST be refused with the error that field's check names.

### 3.4 Strict Ed25519 at the relay

The relay must never accept a signature that `ed25519-dalek` `verify_strict` rejects (V1-16, SEAL-GEN-010). WebCrypto alone does not promise that, so `verifyStrict(pk, msg, sig)` in the core is, in order:

1. `pk` is 32 bytes and `sig` is 64 bytes; otherwise false.
2. `S` = `sig[32..64]` read as a little-endian integer must be below `L = 2^252 + 27742317777372353535851937790883648493`; otherwise false.
3. For each of `A = pk` and `R = sig[0..32]`: let `y` be the 32 bytes read little-endian with bit 255 cleared. `y` must be below `p = 2^255 − 19`, and `y` must not be one of `0`, `1`, `p − 1`, `Y1` or `Y2`, where `Y1` is the little-endian value of `26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc05` and `Y2` that of `c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac037a` (the y-coordinates of every point of order 1, 2, 4 or 8). Otherwise false.
4. `crypto.subtle.importKey('raw', pk, {name: 'Ed25519'}, false, ['verify'])`, then `crypto.subtle.verify({name: 'Ed25519'}, key, sig, msg)`; an exception is false.

Step 3 also refuses every non-canonical encoding of `A` and `R`, which honest signers never produce, so the relay is never laxer than the clients and stricter only on forged input.

- **RELAY-GEN-009** (relay) Every Ed25519 verification by the relay (frames, login, admin signatures) MUST be `verifyStrict` with the four steps of §3.4 in order.
- **RELAY-GEN-010** (relay) `verifyStrict` MUST return false for every case of Wycheproof `ed25519_test.json` and of `seal-signatures.json` that `ed25519-dalek` `verify_strict` rejects, and true for every case those files mark valid.

## 4. Encodings

### 4.1 Text in paths, queries and headers

| Value | Where | Text form |
|---|---|---|
| `board_id`, `tid`, blob id | path segments | exactly 32 lowercase hex characters |
| part index | path segment | decimal, `0` or `[1-9][0-9]*`, at most 2 digits |
| `after`, `limit` | query of `GET …/log` | decimal without leading zeros (`0` allowed), at most 16 digits |
| token | `Authorization: Bearer <token>` | §5.1 |
| door | `Authorization: Door <door>` | 10 lowercase hex characters |
| create secret | `Authorization: Create <secret>` | 64 lowercase hex characters |
| ticket proof | `X-TD-Proof: <ticket_key>` | 64 lowercase hex characters |

A path is matched exactly: no trailing slash, no empty segment, no percent-encoding, no `.` or `..` segment. Uppercase hex is not accepted. A query is allowed only on `GET /v1/b/{board}/log`; there each of `after` and `limit` appears at most once, in any order, and no other parameter, no empty value and no fragment is allowed.

- **RELAY-ENC-001** (relay) The front MUST accept a path only if it matches a route of §10.1 exactly as §4.1 states, and MUST answer any other path or method with `404 relay/not_found`.
- **RELAY-ENC-002** (relay) The front MUST refuse a query on any route other than `GET …/log`, and on that route any parameter other than `after` and `limit`, a repeated parameter, or a value not in the text form of §4.1, with `400 relay/bad_query`.

### 4.2 Bodies

Every request body is binary, with a layout table in this spec; the relay contains no JSON parser for untrusted input. Responses are binary where they carry frames, blob parts, slots or append results, and JSON otherwise.

| Route family | Request body | Success response body |
|---|---|---|
| caps, door | none | JSON |
| create | binary (§10.2) | JSON |
| login | binary (§10.3) | JSON |
| hello | none | JSON |
| read | none | binary page (§10.5) |
| append | binary batch (§9.2) | binary results (§9.6) |
| blobs | none, or binary begin (§10.7), or raw part bytes | JSON, or raw part bytes |
| pairing | binary (§10.8) | JSON, or raw slot bytes |
| admin | binary envelope (§12.1) | JSON |
| seed tail | binary (§12.7) | JSON |
| socket | binary messages (§11) | binary messages |

**JSON responses.** UTF-8, RFC 8259, produced by `JSON.stringify` of objects whose keys are in the order the tables give; integers only, each within 0 to 2^53 − 1; byte strings as lowercase hex of exactly twice their width; booleans as `true`/`false`; no `null`; an optional field is absent when it has no value. Clients parse them strictly (README §5.3): every table here is complete.

**Error responses.** Every refusal outside an append result or a socket message is HTTP JSON:

| Field | Type | Required | Constraints | Meaning |
|---|---|---|---|---|
| `error` | string | required | a code of §19 | what went wrong |
| `until` | integer | optional; present exactly for `relay/budget` and `relay/delete_budget` | ms since the epoch | when the budget resets: `(UTC day + 1) × 86,400,000` |
| `index` | integer | optional; present exactly when one frame of a `seed-tail` or `restore_tail` request refused the whole request (§12.7, §12.9) | 0 to 255 | the 0-based position of that frame in the request |
| `head` | object | optional; present exactly for `relay/not_next` | `{"seq": integer, "chain": hex32}` | the relay head (§7.3), so that a device whose earlier answer was lost learns where to resume |

A client parses an error body strictly too: a field outside this table, or a field present where its row says it is absent, is a protocol error it reports as a relay bug.

**Headers on every response.** `Cache-Control: no-store`, `X-TD-Relay: gate` when the front produced the response without calling the object, `X-TD-Relay: object` when the core produced it, and `Content-Type` as above. No other header is set by relay code (the platform may add its own).

- **RELAY-ENC-003** (relay) Every request body MUST be parsed by the binary layout this spec gives for its route, and a body that does not match it exactly (length, counts, prefixes, trailing bytes) MUST be refused with `400 relay/bad_body` unless the route's section names another code.
- **RELAY-ENC-004** (relay) Every JSON response MUST contain exactly the fields of its table, in the table's order, with the stated types, and no other field.
- **RELAY-ENC-005** (relay) Every response other than the `101` of an accepted socket upgrade MUST carry `Cache-Control: no-store` and `X-TD-Relay` set to `gate` or `object` as §4.2 states.
- **RELAY-ENC-006** (relay) Every refusal answered over HTTP MUST have the status the error catalogue (§19) gives for its code and the JSON body of the error table of §4.2.

### 4.3 Binary helpers

`u8`, `u16`, `u32`, `u64`, `i64`, `vb8`, `vb16`, `vb32` are as in README §5.1 and `seal.md` §5.1. All integers are big-endian. Hex in JSON and in headers is lowercase. "base64url" is RFC 4648 §5 without padding.

## 5. Credentials

### 5.1 Tokens

A token is a bearer credential the relay mints and alone can check. Clients store it, send it and never parse it (SEAL-KEY-015).

**Token bytes** (69 bytes):

| Offset | Field | Width | Type | Value |
|---|---|---|---|---|
| 0 | `role` | 1 | u8 | 1 `device`, 2 `backup-read` |
| 1 | `board_id` | 16 | bytes | the board |
| 17 | `dev` | 16 | bytes | role 1: the device; role 2: the backup writer (a device's `dev`, or `dev_R` for the recovery identity, §12.11) |
| 33 | `gen` | 4 | u32 | the generation (§5.2) |
| 37 | `mac` | 32 | bytes | `HMAC(TOKEN_KEY, ctx("token") ‖ token[0..37])` |
| | **total** | **69** | | |

The MAC input is `ctx("token")` (18 bytes) followed by the first 37 token bytes: 55 bytes.

**Text form:** `"TD1."` followed by the base64url (no padding) of the 69 bytes: 4 + 92 = 96 ASCII characters. In a header: `Authorization: Bearer TD1.<92 characters>`.

**Checking** (the gate, §6.2 step 6): the text is exactly 96 characters, starts with `TD1.`, the rest is 92 characters of the base64url alphabet that decode to 69 bytes, `role` is 1 or 2, `board_id` equals the path's board, and the MAC verifies. The object then checks state and generation (§8.2).

- **RELAY-TOKEN-001** (relay) A token MUST be laid out as the table of §5.1, with `mac = HMAC(TOKEN_KEY, ctx("token") ‖ token[0..37])`, and written in the text form of §5.1.
- **RELAY-TOKEN-002** (relay) The gate MUST accept a token only if every check of §5.1 passes, verifying the MAC in constant time, and MUST refuse any other with `401 relay/unauthorized` before any object call.
- **RELAY-TOKEN-003** (device, recovery) A client MUST treat a token as an opaque ASCII string, MUST send it only in the `Authorization` header of requests to the relay origin that issued it, and MUST NOT put it in a URL, a log or an error message.

### 5.2 Generations

`gen` counts the tokens a credential holder has been issued; a token is valid only while its `gen` equals the stored one. There is one counter per device (`devices.gen`) and one per backup writer (`read_tokens.gen`).

| Event | `devices.gen` of the device | Effect on its tokens |
|---|---|---|
| registered by `create` (first device) or `register` (§12.3) | set to 1; a token with `gen = 1` is returned | the new token is valid |
| registered by `seed` (§12.7) | set to 0; the seed's signer: set to 1 | none exists, and the device logs in; the signer's token with `gen = 1` is returned by `seed` |
| `login` succeeds (§10.3) | set to the requested value, which must be stored + 1 | every older token is invalid at once; sockets with an older `gen` are closed with 4003 |
| `revoke` (§12.4) | stored + 1 | every token is invalid at once; the device's sockets are closed with 4003 |
| `reinstate` (§12.5) | unchanged | tokens stay invalid (revoke moved `gen`); the device logs in |
| a rotation, a fence, compaction, purge | unchanged | none |

| Event | `read_tokens` row of writer `w` | Effect |
|---|---|---|
| `mint_read_token` by `w` (§12.11) | `gen` = stored + 1 (1 when no row), `state = 1`, `epoch` from the arguments | the new token is valid, every older one of `w` invalid |
| `revoke_read_token` for `w` | `state = 2` | `w`'s token is invalid |
| `revoke`, `reinstate`, `recovery_register` or `purge` (each precedes or carries a rotation, SYNC.md §5.2) | every row `state = 2` | every backup-read token is invalid |
| `mint_read_token` with an epoch above `board.read_epoch` | every row with a lower `epoch`: `state = 2` | tokens of older epochs are invalid |

A generation never decreases, and a row of `devices` or `read_tokens` is never deleted while the board exists, so no token can become valid again. A `gen` of 2^32 − 1 cannot be raised; a login or mint that would need it is refused with `409 relay/gen_exhausted`.

- **RELAY-TOKEN-004** (relay) The object MUST change `devices.gen` and `read_tokens.gen` and `state` exactly as the tables of §5.2 state, and in no other case.
- **RELAY-TOKEN-005** (relay) The object MUST never decrease a stored generation and MUST never delete a row of `devices` or `read_tokens` of a board that is not being re-seeded (§12.7).

### 5.3 The door

```text
door = HMAC(TOKEN_KEY, ctx("door") ‖ board_id[16])[0..5]          (33-byte input, 5-byte output)
```

The door is stateless for the Worker, fixed for the life of a deployment (`TOKEN_KEY` never changes), never stored by the object, and not a secret that protects data (ruling S4). A request that presents it reaches only the routes §6.3 lists for it.

- **RELAY-DOOR-001** (relay) The door of a board MUST be `HMAC(TOKEN_KEY, ctx("door") ‖ board_id)[0..5]`.
- **RELAY-DOOR-002** (relay) The gate MUST compare a presented door with the computed one in constant time and MUST refuse a mismatch with `401 relay/unauthorized` before any object call.
- **RELAY-DOOR-003** (relay) The object MUST NOT store the door in any table; every response that carries it (create, door, hello, welcome, seed) MUST compute it from `TOKEN_KEY`.
- **RELAY-DOOR-004** (relay) A deployment MUST keep one `TOKEN_KEY` for its whole life; a new key is a new relay, reached through the fenced re-home (SYNC.md §6.7).

### 5.4 The relay address

What a person types or pastes to name a board on a relay (ruling S4): the pairing sheet shows it beside the code, a Fence names its parts (`seal.md` §12.7), and the recovery sheet prints it.

```text
B        = Crockford base32 (seal.md §5.2) of board_id[16] ‖ door[5]      21 bytes → 34 characters, 2 zero padding bits
text     = host "/" B                                                     canonical form, no separators in B
display  = host "/" B in groups of 4 joined by "-", the last group of 2   e.g. relay-x.workers.dev/0123-4567-…-89 (symbolic)
```

`host` is the host form of `seal.md` §5.4 (lowercase, optional `:port`, 443 never written). The relay origin of the address is `https://` ‖ `host`.

**Parsing** typed or pasted text, in order:

1. Remove leading and trailing spaces, tabs and line breaks.
2. If the text starts with `https://` in any letter case, remove those 8 characters. Any other `scheme://` prefix is an error (`relay/address`).
3. Split at the first `/`. There must be exactly one `/`; the part before it is the host, the part after it is `B`.
4. Map `A`–`Z` in the host to `a`–`z`; the host must then match `seal.md` §5.4's host rule, and a port of 443 is removed.
5. Normalise `B` as `seal.md` §5.2 (remove spaces, hyphens and tabs; fold case; `O` to `0`; `I` and `L` to `1`). It must then be exactly 34 alphabet characters whose 2 padding bits are zero.
6. Decode `B` into 21 bytes: `board_id = [0..16]`, `door = [16..21]`.

Any failure is `relay/address`. The door has no check characters: a typing error in `B` is caught by the relay, which answers `401 relay/unauthorized` to the wrong door (with odds of a mistyped address passing of 2^−40).

- **RELAY-ADDR-001** (relay, device) A relay address MUST be written in the canonical text form of §5.4 and displayed only in its display form.
- **RELAY-ADDR-002** (device, recovery) A client MUST parse a relay address by the six steps of §5.4 in order and MUST reject the input at the first failing step with `relay/address`, before sending any request.

### 5.5 The create secret

`CREATE_SECRET` is 32 bytes, configured as 64 lowercase hex characters, set by the owner for creating a board or seeding one after a re-home, and deleted afterwards (SYNC.md §2.1). While it is absent, the create family of routes (§6.3) is refused with `403 relay/create_disabled` by the gate.

- **RELAY-GATE-001** (relay) The gate MUST refuse every route of the create family with `403 relay/create_disabled` when `CREATE_SECRET` is not configured, and MUST compare a presented create secret with the configured one in constant time, refusing a mismatch with `401 relay/unauthorized`.

### 5.6 Credential header forms

The `Authorization` header must appear exactly once and match one of these exactly (one space after the scheme, no other whitespace):

| Form | Regular expression of the header value |
|---|---|
| token | `^Bearer TD1\.[A-Za-z0-9_-]{92}$` |
| door | `^Door [0-9a-f]{10}$` |
| create | `^Create [0-9a-f]{64}$` |

`X-TD-Proof` must appear exactly once on the routes that require it and match `^[0-9a-f]{64}$`.

- **RELAY-GATE-002** (relay) The gate MUST refuse a request whose `Authorization` header is absent, repeated, or matches none of the forms of §5.6, with `401 relay/unauthorized`.

## 6. The gate

### 6.1 Purpose

The gate refuses, before any object call, every request that has no valid credential, that is too large, or whose body arrives too slowly (SYNC_PLAN.md §6.4, R3 §7). It is stateless: it needs only `TOKEN_KEY`, `CREATE_SECRET` and the route table of §10.1. The Worker runs it in `fetch`; the standby runs the same core function in its HTTP handler.

### 6.2 Order of checks

The front runs these steps in order; the first that fails answers with its code and `X-TD-Relay: gate`, and no later step runs.

| Step | Check | On failure |
|---|---|---|
| 0 | the configuration is valid (§17.1; on the standby it was checked at start, §17.2) | `503 relay/misconfigured` |
| 1 | method and path match a route of §10.1 (§4.1) | `404 relay/not_found` |
| 2 | the query is allowed (§4.1) | `400 relay/bad_query` |
| 3 | a route without a body: no `Transfer-Encoding` header and `Content-Length` absent or `0`. A route with a body: no `Transfer-Encoding` header and exactly one `Content-Length` header in decimal | `400 relay/unexpected_body`; `411 relay/length_required` |
| 4 | `Content-Length` at most the route's cap (§10.1), and equal to the fixed length where the route has one | `413 relay/too_large`; `400 relay/bad_body` |
| 5 | the socket route has `Upgrade: websocket` (case-insensitive value) | `400 relay/bad_upgrade` |
| 6 | the `Authorization` header has one of the forms of §5.6, and that form is accepted by the route (§10.1) | `401 relay/unauthorized` |
| 7 | the credential verifies: a token as §5.1 (MAC and board), with a role the route accepts; a door as §5.3; the create secret as §5.5; on routes that require it, `X-TD-Proof` has the form of §5.6 and `H(ctx("pair-tid") ‖ ticket_key)[0..16]` equals the path's `tid` in constant time | `401 relay/unauthorized`; a token of the wrong role `403 relay/forbidden`; `403 relay/create_disabled`; `403 relay/bad_proof` |
| 8 | gate-only routes (`caps`, `door`; on the standby `socket`) are answered here (§10.9, §15.2) | |
| 9 | the whole body arrives within `deadline_ms = 10,000 + ceil(Content-Length × 1,000 / 65,536)` milliseconds of the front receiving the request, and its length equals `Content-Length` | `408 relay/slow`; `400 relay/bad_body` |
| 10 | the object is called once (§6.4); if the call throws | `503 relay/object_unavailable` |

The deadline is 10 seconds of grace plus 1 second per 64 KiB: a 1 MiB blob part has 26 seconds, a full 16-frame batch 74 seconds. A test drives "too slowly" by declaring `Content-Length: 1000`, sending 10 bytes and then nothing: the front must answer `408 relay/slow` between 10,016 and 11,016 ms after the request, and the object must not have been called.

- **RELAY-GATE-003** (relay) The front MUST run the steps of §6.2 in order and MUST answer with the code of the first failing step.
- **RELAY-GATE-004** (relay) The front MUST NOT call the object for a request that fails any of steps 0 to 9 of §6.2, and every response to such a request MUST carry `X-TD-Relay: gate`.
- **RELAY-GATE-005** (relay) The front MUST refuse with `413 relay/too_large`, without reading the body, a request whose `Content-Length` exceeds the route's cap in §10.1.
- **RELAY-GATE-006** (relay) The front MUST read the body to completion before calling the object, and MUST answer `408 relay/slow`, without calling the object, when the body is not complete within `deadline_ms` of §6.2.
- **RELAY-GATE-007** (relay) The front MUST refuse with `400 relay/bad_body` a body whose received length differs from its `Content-Length`.
- **RELAY-GATE-008** (relay) The gate MUST check the ticket proof of the routes that require it (§10.1) before any object call, and MUST refuse a failed proof with `403 relay/bad_proof`.

### 6.3 Credentials per route

Which credential each route accepts is the "Credentials" column of §10.1. In short:

- **token, role 1 (`device`)**: every route except `door`, `create` and the create-family use of `admin/nonce` and `admin/seed`.
- **token, role 2 (`backup-read`)**: only `caps`, `hello`, `read`, `blob-list`, `blob-meta`, `blob-get`.
- **door**: the pre-token routes only: `caps`, `login-nonce`, `login`, `append` (recovery-authored batches, §9.3), `slot-post` (label B, with a proof), `slot-get` (labels A, C, D, with a proof), `admin-nonce` and `admin` (signer a recovery identity, §12.1).
- **create secret** (the **create family**): `caps`, `door`, `create`, `admin-nonce` and `admin/seed` on an empty or seeding board.

A token whose MAC verifies passes the gate whatever its generation; the object checks state and generation (§8.2). The login routes accept such a token even when its generation is stale: that is how a device with an old token reaches login without the door.

- **RELAY-GATE-009** (relay) The gate MUST refuse with `401 relay/unauthorized` a credential form the route does not accept, and with `403 relay/forbidden` a token whose role the route does not accept.

### 6.4 Calling the object

The front builds a new request for the object; nothing the client sent is passed on except the method, the route, the path parameters, the query values and the body bytes. It adds the verified credential:

| Internal field | Value |
|---|---|
| `cred` | `token`, `door` or `create` |
| `role` | for a token: 1 or 2 |
| `dev` | for a token: its `dev` |
| `gen` | for a token: its `gen` |

In the Worker these travel as the headers `X-TD-Cred`, `X-TD-Role`, `X-TD-Dev` (32 hex) and `X-TD-Gen` (decimal) on a request to `https://object/<route name>` passed to `stub.fetch`; the Worker removes every header whose name starts with `X-TD-` from the client's request before it adds them. On the standby they are arguments of the in-process call. The object is reachable only through its binding, so these fields cannot come from a client.

- **RELAY-GATE-010** (relay) The front MUST pass to the object only the fields of §6.4 and the method, route, path parameters, query values and body, and MUST remove every client-supplied header whose name starts with `X-TD-` before it adds its own.

## 7. Storage schema

### 7.1 Tables

The object's SQLite database holds these tables, created by `SQL.schema` of the core (§23). Columns are `NOT NULL` unless marked "or NULL". Byte columns hold exactly the stated width. Integers are within 0 to 2^53 − 1.

```sql
CREATE TABLE IF NOT EXISTS board (
  one           INTEGER PRIMARY KEY CHECK (one = 1),
  schema        INTEGER NOT NULL,  -- 1
  board_id      BLOB    NOT NULL,  -- 16
  state         INTEGER NOT NULL,  -- 1 open, 2 fenced, 3 seeding
  created_at    INTEGER NOT NULL,  -- relay clock at create or seed
  base_seq      INTEGER NOT NULL,  -- seq below the first stored entry: 0 after create, first_seq - 1 after seed
  base_chain    BLOB    NOT NULL,  -- 32: C_base_seq
  compact_floor INTEGER NOT NULL,  -- below it only kept entries remain
  purge_floor   INTEGER NOT NULL,  -- below it nothing is served
  purge_done    INTEGER NOT NULL,  -- every log row below it has been deleted by purge
  compact_snap  BLOB,              -- 16 or NULL: the snapshot the latest compaction relied on
  del_day       INTEGER NOT NULL,  -- UTC day of del_count
  del_count     INTEGER NOT NULL,  -- log rows deleted by compaction and purge on del_day
  read_epoch    INTEGER NOT NULL,  -- highest epoch named by mint_read_token or purge
  home_gen      INTEGER NOT NULL,  -- 1 after create; the seed's home_gen after seed; the fence's after fence
  fence_host    TEXT,              -- or NULL; seal.md §5.4 host
  fence_door    BLOB,              -- 5 or NULL
  seed_signer   BLOB,              -- 16 or NULL: while seeding, the device that seeded
  seed_last_seq INTEGER,           -- or NULL: while seeding, the last seq of the tail
  seed_chain    BLOB               -- 32 or NULL: while seeding, C_seed_last_seq
);

CREATE TABLE IF NOT EXISTS devices (
  dev      BLOB    PRIMARY KEY NOT NULL,  -- 16
  sign_pk  BLOB    NOT NULL,              -- 32
  role     INTEGER NOT NULL,              -- 1 device, 2 recovery
  state    INTEGER NOT NULL,              -- 1 current, 2 revoked (role 1) or retired (role 2)
  gen      INTEGER NOT NULL,              -- token generation; always 0 for role 2
  last_ctr INTEGER NOT NULL,              -- the highest ctr stored for this author
  last_seq INTEGER NOT NULL,              -- the seq of (dev, last_ctr), or 0
  app_day  INTEGER NOT NULL,              -- UTC day of app_count
  app_count INTEGER NOT NULL,             -- new frames stored for this author on app_day
  added_at INTEGER NOT NULL               -- relay clock
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS log (
  seq   INTEGER PRIMARY KEY NOT NULL,
  dev   BLOB    NOT NULL,   -- 16
  ctr   INTEGER NOT NULL,
  hash  BLOB    NOT NULL,   -- 32: H(frame)
  chain BLOB    NOT NULL,   -- 32: C_seq (seal.md §15.1)
  at    INTEGER NOT NULL,   -- relay clock at sequencing, non-decreasing in seq
  frame BLOB    NOT NULL    -- 670 to 262,144 bytes
);
CREATE UNIQUE INDEX IF NOT EXISTS log_dev_ctr ON log (dev, ctr);

CREATE TABLE IF NOT EXISTS blobs (
  id       BLOB    PRIMARY KEY NOT NULL,  -- 16
  slot     INTEGER NOT NULL,              -- 1 to 2^31: parts are stored under slot * 64 + part
  purpose  INTEGER NOT NULL,              -- 1 snapshot, 2 pairing snapshot, 3 genesis snapshot, 4 other
  seq      INTEGER NOT NULL,              -- S for purposes 1 to 3, 0 for purpose 4
  author   BLOB    NOT NULL,              -- 16: the uploading device
  size     INTEGER NOT NULL,              -- 67 to 67,108,864
  parts    INTEGER NOT NULL,              -- ceil(size / 1,048,576)
  sha256   BLOB    NOT NULL,              -- 32: declared H(sealed bytes)
  received INTEGER NOT NULL,              -- parts stored so far
  complete INTEGER NOT NULL,              -- 0 or 1
  at       INTEGER NOT NULL               -- relay clock at begin
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS parts (
  pk   INTEGER PRIMARY KEY NOT NULL,      -- slot * 64 + part
  data BLOB    NOT NULL                   -- 1 to 1,048,576 bytes
);

CREATE TABLE IF NOT EXISTS tickets (
  tid     BLOB    PRIMARY KEY NOT NULL,   -- 16
  creator BLOB    NOT NULL,               -- 16: E's dev
  expires INTEGER NOT NULL,               -- relay clock + 600,000 at registration
  tries   INTEGER NOT NULL,               -- 0 to 5
  a BLOB NOT NULL, b BLOB, c BLOB, d BLOB -- slot bytes (seal.md §17.4), b to d or NULL
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS nonces (
  nonce   BLOB    PRIMARY KEY NOT NULL,   -- 32
  purpose INTEGER NOT NULL,               -- 1 login, 2 admin
  dev     BLOB    NOT NULL,               -- 16: the device that will log in, or the admin signer
  at      INTEGER NOT NULL                -- relay clock at issue
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS read_tokens (
  dev   BLOB    PRIMARY KEY NOT NULL,     -- 16: the backup writer
  gen   INTEGER NOT NULL,
  epoch INTEGER NOT NULL,
  state INTEGER NOT NULL                  -- 1 valid, 2 revoked
) WITHOUT ROWID;
```

SYNC.md §4.1 named the tables `log`, `devices`, `snapshots`, `blobs`, `tickets` and `meta`. Here `meta(k, v)` becomes the single `board` row (one row read instead of one per key), `snapshots` and the blob metadata become `blobs`, and the part bytes are `parts`, keyed by an integer so that a part write is one row (a composite primary key on a rowid table would add an index row). `nonces` and `read_tokens` are new. The door is not stored (ruling S4).

Every table except `log` and `parts` is `WITHOUT ROWID` with no secondary index, so writing one of its rows writes one row; a `log` row writes two (the row and `log_dev_ctr`); a `parts` row writes one (R3 §2.1: every index entry counts as a row written).

- **RELAY-STORE-001** (relay) The object's database MUST have exactly the tables, columns, keys and index of §7.1, and no other.
- **RELAY-STORE-002** (relay) The core MUST run `SQL.schema` before it handles the first event of an object instance (in the Worker inside `ctx.blockConcurrencyWhile` in the constructor), and running it on a database that already has the schema MUST write no row.
- **RELAY-STORE-003** (relay) Every `log` row MUST satisfy `hash = H(frame)` and `chain = H(ctx("chain") ‖ C_{seq−1} ‖ u64 seq ‖ hash)` where `C_{seq−1}` is the `chain` of row `seq − 1`, or `base_chain` when `seq − 1 = base_seq` (SEAL-CHAIN-001).
- **RELAY-STORE-004** (relay) The object MUST store `at` as `max(now, at of the row at seq − 1)` (`max(now, created_at)` for the first stored row), so that `at` never decreases with seq.
- **RELAY-STORE-005** (relay) The seqs of the `log` rows at or above `floor` MUST be contiguous up to the relay head; below `floor` only rows kept by compaction (§12.8) may remain, and below `purge_floor` none MUST be served.

### 7.2 What the `at` column is

`at` is the relay's clock when it sequenced the entry, made non-decreasing. It is shown nowhere as a fact about the entry. The relay uses it only for the 365-day age check of compaction (§12.8). Clients receive it in pages and use it for nothing that changes state or decides validity: every time rule is decided from `at_ms` inside entries (ruling S6), and relay time never stamps anything (SYNC.md §4.5).

- **RELAY-STORE-006** (engine, device) A client MUST NOT use the `at` of a page entry, `relay_now` or any other relay-supplied time in any validity decision, stamp, `Ctx` or timed window; it may use `relay_now` only for the clock-skew warning of SYNC.md §4.5.

### 7.3 The relay head

`head_seq` is `SELECT seq, chain FROM log ORDER BY seq DESC LIMIT 1`, or `(base_seq, base_chain)` when the log has no row. The head row is never deleted: compaction deletes only below a snapshot seq at most the head (§12.8), and purge only below `purge_before`, which is at most the seq of the rotation that carries it (`seal.md` §12.5). The relay keeps no separate head row, so an append writes three rows, not four.

- **RELAY-STORE-007** (relay) The object MUST NOT delete the `log` row at the relay head.

## 8. Object rules common to all routes

### 8.1 Board states

| State | Set by | Allowed |
|---|---|---|
| `empty` (no board row) | — | `create`, `admin-nonce` and `admin/seed` with the create secret |
| `seeding` | `admin/seed` (§12.7) | `seed-tail` by the seeding device; `admin-nonce` and `admin/seed` with the create secret (a new seed replaces the old) |
| `open` | `create`; the last `seed-tail`; a `seed` with an empty tail | every route of §10.1, subject to each route's checks |
| `fenced` | `admin/fence` (§12.6) | `caps`, `login-nonce`, `login`, `hello`, `socket` (read-only), `read`, `blob-list`, `blob-meta`, `blob-get` |

| Current state | Event | Guard | Next state |
|---|---|---|---|
| `empty` | `create` succeeds | §10.2 | `open` |
| `empty` or `seeding` | `admin/seed` succeeds | §12.7; tail empty | `open` |
| `empty` or `seeding` | `admin/seed` succeeds | §12.7; tail not empty | `seeding` |
| `seeding` | `seed-tail` stores seq `seed_last_seq` | its chain equals `seed_chain` | `open` |
| `open` | `admin/fence` succeeds | §12.6 | `fenced` |
| any | any other request | | unchanged |

A request that its board's state does not allow is refused: on an empty board with `404 relay/no_board`; on a seeding board with `409 relay/seeding`; on a fenced board with `403 relay/fenced`; but a request with the create secret on an `open` or `fenced` board (`create`, `admin-nonce`, `admin/seed`) with `409 relay/exists` (§10.2 says when a `create` repeat succeeds instead). On a seeding board, `create` is `409 relay/seeding`.

- **RELAY-OBJ-001** (relay) The object MUST allow in each board state only the routes the first table of §8.1 lists, and MUST refuse every other with the code §8.1 gives for that state.
- **RELAY-OBJ-002** (relay) The board state MUST change only as the second table of §8.1 states, so that a `fenced` board never becomes `open` again.

### 8.2 Credentials at the object

The gate has checked the MAC; the object checks state, in one read of the credential's row:

| Credential | Valid at the object when | Else |
|---|---|---|
| token, role 1 | a `devices` row with `dev`, `role = 1` exists; `state = 1`; `gen` equals the token's | no row or `role = 2`: `401 relay/unauthorized`; `state = 2`: `403 relay/revoked`; `gen` differs: `401 relay/token_stale` |
| token, role 1, on `login-nonce` and `login` | a `devices` row with `dev` and `role = 1` exists | as above, but the generation is not checked |
| token, role 2 | a `read_tokens` row with `dev` exists, `state = 1`, `gen` equals the token's | `401 relay/token_stale` (no row: `401 relay/unauthorized`) |
| door, create secret | always (the gate checked it); each route then limits what it does with it | |

- **RELAY-OBJ-003** (relay) For every request with a token, the object MUST check the token's row as the table of §8.2 states before any other check of the route, and MUST answer with the code the table gives when it fails.
- **RELAY-OBJ-004** (relay) The object MUST NOT write any row for a request it refuses for its credential or its board state.

### 8.3 The relay clock

The object reads `now()` once per request or socket message, at the start of its handling, and uses that value throughout. It uses the clock only for: `at` (§7.2), nonce expiry (§10.3, §12.1), ticket expiry (§10.8), blob expiry (§13), the UTC day of the append and delete budgets (§14), the compaction age (§12.8) and `relay_now` in responses. A wrong relay clock can delay or refuse service but never change what a client applies.

- **RELAY-OBJ-005** (relay) The object MUST read the relay clock once per request or socket message and MUST use it only for the purposes §8.3 lists.

### 8.4 Ordering, transactions and acknowledgements

The object handles one request or socket message at a time between awaits; its only awaits are the signature verifications of a request, which run before its transaction. Every storage effect of a request runs in one `transaction` of the `Storage` interface (in the Worker `ctx.storage.transactionSync`), which first re-reads every row its checks depend on (SYNC.md §4.3). Nothing is sent before the transaction has committed.

**How checks and verifications interleave.** A request runs in three phases: (1) it parses its body and reads, outside any transaction, the keys its signature checks need (the author rows of its frames, the signer's row, the device row for a login); (2) it awaits every `verifyStrict` the request needs and keeps the results; (3) it runs its one transaction, which re-reads every row its checks depend on and evaluates the checks in the order its section lists, using the results of phase 2 at the step where each signature check stands (a key that changed between phase 1 and phase 3 fails that step). The code returned is that of the first failing step in the section's order, exactly as if the steps had run one after another. Where a section says a row is deleted "whatever follows" or "also when a later check fails" (the nonces of §10.3 and §12.1, the tries of §10.8, expired tickets and blobs), that write is part of this one transaction and is committed alone when a later step refuses the request. In the Worker the output gate holds the response until SQLite has confirmed the commit on 3 of 5 replicas (R3 §2.3); on the standby SQLite runs with `journal_mode = WAL` and `synchronous = FULL`, so a commit has reached the disk.

- **RELAY-OBJ-006** (relay) Every request that writes MUST write all its rows in one transaction of the `Storage` interface, which MUST re-read every row its checks depend on after the last await of the request.
- **RELAY-OBJ-009** (relay) A request whose checks include a signature verification MUST return the code of the first failing step in the order its section lists, and MUST commit no write other than those its section names as surviving a refusal, whichever phase of §8.4 detected the failure.
- **RELAY-OBJ-007** (relay) The object MUST NOT send a response or socket message that reports a write before that write's transaction has committed.
- **RELAY-OBJ-008** (relay) The standby MUST open every database with `PRAGMA journal_mode = WAL` and `PRAGMA synchronous = FULL`.

## 9. Sequencing and idempotency

### 9.1 What the relay checks of a frame

The relay checks exactly what decides whether clients would skip a frame or find a counter wrong (`seal.md` §14.2 steps 1 to 5) and nothing inside `body` (SYNC.md §4.3, V1-16). A frame that passes is one every client classifies as `Refuse` or `Accept`, so it consumes its author's counter everywhere, as it does at the relay.

### 9.2 The batch

**Batch** (the append body, and the payload of the socket's append message):

| Field | Width | Type | Value |
|---|---|---|---|
| `magic` | 4 | ASCII | `"TDA1"` |
| `n` | 1 | u8 | 1 to 16 |
| frames | `n × (4 + len_i)` | | each `u32 len_i ‖ frame_i`, `len_i` from 670 to 262,144 |
| | **total** | **5 + Σ(4 + len_i)** | at most 4,194,373 bytes |

`len_i` is the frame's total length, so `frame_i`'s own `len` field must equal `len_i − 112` (`seal.md` §8.1).

### 9.3 Checks before the transaction

For frames `i = 0, 1, …` in order, the core runs these checks until one fails; frame `i` then gets the refused result of that check and no later frame is examined.

| Step | Check | Refused with |
|---|---|---|
| P1 | `len_i` from 670 to 262,144; `frame[0..4] = "TDF1"`; the frame's `len` field equals `len_i − 112` | `400 relay/bad_frame` |
| P2 | `board_id` (bytes 4 to 20) equals the board's | `400 relay/wrong_board` |
| P3 | `ctr` (bytes 36 to 44) from 1 to 2^53 − 1 | `400 relay/bad_frame` |
| P4 | an author row for `dev` (bytes 20 to 36) exists | `400 relay/unknown_author` |
| P5 | with a token: `dev` equals the token's `dev`, or the author row has `role = 2`. With the door: the author row has `role = 2` | `403 relay/not_author` |
| P6 | the author row has `state = 1` | `403 relay/revoked` |
| P7 | `verifyStrict(author.sign_pk, ctx("frame") ‖ frame[0 .. 48 + len], frame[48 + len ..])` (§3.4, SEAL-FRAME-002) | `400 relay/bad_signature` |

A recovery identity's frame (author `role = 2`) is accepted from any request that passed the gate with a token or the door, because its signature is what authenticates it (ruling S3).

### 9.4 The transaction

One transaction then runs for frames `0` up to, but not including, the frame that failed §9.3 (all frames if none did). It reads the relay head once, and for each frame in order:

| Step | Check | Outcome |
|---|---|---|
| T1 | re-read the author row; it still has `state = 1` and the same `sign_pk` as in P7 | else refused `403 relay/revoked`; stop |
| T2 | `ctr = last_ctr + 1` | go to T4 |
| T3a | `ctr ≤ last_ctr`: find the `log` row with this `(dev, ctr)` and `seq ≥ purge_floor`. Found and its `hash = H(frame)` | result `dup` with that row's `seq` and `chain`; no write; continue with the next frame |
| T3b | found and its `hash` differs | result `conflict` with that row's `seq` and `frame`; stop |
| T3c | not found (compacted, purged, or before the board's `base_seq`) | result `conflict` with `seq = 0` and no frame; stop |
| T3d | `ctr > last_ctr + 1` | result `gap` with the author row's `last_ctr` and `last_seq`; stop |
| T4 | for an author with `role = 1`: let `count = app_count` if `app_day` is today's UTC day, else 0; `count < 3,000` | else refused `429 relay/budget`; stop |
| T5 | store: `seq = head_seq + 1`; `at` as RELAY-STORE-004; `chain = H(ctx("chain") ‖ head_chain ‖ u64 seq ‖ H(frame))`; insert the `log` row; the author's `last_ctr = ctr`, `last_seq = seq`, and for `role = 1` `app_day` = today, `app_count = count + 1`; the head becomes `(seq, chain)` | result `new` with `seq` and `chain` |

"Stop" means no later frame of the batch is examined; their results are `skipped`. The frames stored before the stop stay stored: each was valid in order. Each author row is written once per batch, after its last frame, with its final values.

- **RELAY-SEQ-001** (relay, engine) The relay MUST store a frame only when its `ctr` is exactly the author's stored `last_ctr + 1`, at `seq = head_seq + 1`.
- **RELAY-SEQ-002** (relay, engine) For a frame with `ctr ≤ last_ctr`, the relay MUST answer `dup` with the stored seq and chain when the stored frame at `(dev, ctr)` has the same hash, `conflict` with the stored seq and frame when it has another, and `conflict` with seq 0 and no frame when no stored frame at or above `purge_floor` has that `(dev, ctr)`.
- **RELAY-SEQ-003** (relay, engine) For a frame with `ctr > last_ctr + 1`, the relay MUST answer `gap` with the author's stored `last_ctr` and `last_seq`.
- **RELAY-SEQ-004** (relay, engine) The relay MUST process a batch's frames in order, MUST stop at the first frame whose result is `conflict`, `gap` or a refusal, MUST keep the frames it stored before that one, and MUST give every later frame the result `skipped`.
- **RELAY-SEQ-005** (relay) A batch, or the part of it before a stop, whose frames all get `dup`, `conflict`, `gap` or a refusal MUST write no row (SYNC_PLAN.md §6.4: a replay is refused without a write).
- **RELAY-SEQ-006** (relay) The relay MUST run the checks P1 to P7 of §9.3 for every frame before its transaction, and MUST verify every frame signature before deciding any counter outcome for that frame.
- **RELAY-SEQ-007** (relay) The relay MUST run the steps T1 to T5 of §9.4 for a batch in one transaction, and MUST write each author row at most once per batch.
- **RELAY-SEQ-008** (relay) The relay MUST accept a frame whose author row has `role = 2` from any request with a token of role 1 or with the door, and MUST refuse with `403 relay/not_author` a frame of a role-1 author other than the token's device.
- **RELAY-SEQ-009** (relay) The relay MUST refuse an append whose credential is the door unless every frame of the batch is authored by a role-2 author, giving the first other frame `403 relay/not_author`.

### 9.5 Results

| Result | Meaning for the client |
|---|---|
| `new` | stored now at `seq`; the relay's chain value there is `chain` |
| `dup` | already stored at `seq` with the same hash: this is the acknowledgement of a resend (SYNC.md §4.3 rule 2) |
| `conflict` | another frame is stored under this `(dev, ctr)` (`seq`, `frame`), or none is held any more (`seq = 0`): client rule 4 of SYNC.md §4.3 |
| `gap` | the relay holds counters only up to `last_ctr`, at `last_seq`; the client fetches its own frames there before believing it (V1-16) |
| refused | a check of §9.3 or §9.4 failed, with an HTTP status and code |
| `skipped` | not examined, because an earlier frame stopped the batch |

**Acknowledgement.** A `new` or `dup` result is the relay's statement that this exact frame (by hash) is durably stored at `seq` (§8.4). It says nothing about the frame's validity, which clients decide; the client's echo check compares it later with what it reads at `seq` (SYNC.md §4.3 rule 5).

- **RELAY-SEQ-010** (relay, engine) The relay MUST send a `new` or `dup` result only for a frame whose `log` row with that hash is committed at that seq.

### 9.6 Results layout

**Results** (the append response body, and the payload of the socket's result message):

| Field | Width | Type | Value |
|---|---|---|---|
| `magic` | 4 | ASCII | `"TDR1"` |
| `n` | 1 | u8 | the batch's `n` |
| results | | | `n` results, in frame order |

Each result starts with a status byte:

| `status` | Result | Then | Width |
|---|---|---|---|
| 0 | `new` | `u64 seq ‖ chain[32]` | 41 |
| 1 | `dup` | `u64 seq ‖ chain[32]` | 41 |
| 2 | `conflict` | `u64 seq ‖ u32 flen ‖ frame[flen]` (`flen = 0` when `seq = 0`) | 13 + `flen` |
| 3 | `gap` | `u64 last_ctr ‖ u64 last_seq` | 17 |
| 4 | refused | `u16 http_status ‖ vb8(code)`; `code` the ASCII error code | 4 + code length |
| 5 | `skipped` | nothing | 1 |

**HTTP status of an append response with a results body:** 200 when every result is `new` or `dup`; otherwise the status of the stopping result: 409 for `conflict` and `gap`, the refused result's own status for a refusal. Responses for whole-request refusals (credential, board state, a malformed batch) are the JSON errors of §4.2 instead, and are told apart by `Content-Type`.

- **RELAY-SEQ-011** (relay) An append response for a well-formed batch from an allowed credential in an `open` board MUST carry the results body of §9.6, with the HTTP status §9.6 states.

## 10. Routes

### 10.1 The route table

`{board}` is the board id (32 hex), `{tid}` a ticket id (32 hex), `{id}` a blob id (32 hex), `{part}` a part index, `{label}` one of `a`, `b`, `c`, `d`, `{op}` an operation name of the `seal.md` §16.2 registry. Credentials: **T1** a token of role 1; **T2** a token of role 2; **D** the door; **C** the create secret; **P** the `X-TD-Proof` header (§5.6) as well. "Fixed" is the exact body length when the route has one.

| Name | Method | Path | Credentials | Body cap (bytes) | Fixed | Object | Section |
|---|---|---|---|---|---|---|---|
| `caps` | GET | `/v1/b/{board}/caps` | T1, T2, D, C | 0 | 0 | no | §10.9 |
| `door` | GET | `/v1/b/{board}/door` | C | 0 | 0 | no | §10.9 |
| `create` | POST | `/v1/b/{board}/create` | C | 262,244 | | yes | §10.2 |
| `login-nonce` | POST | `/v1/b/{board}/login/nonce` | T1 (any generation), D | 16 | 16 | yes | §10.3 |
| `login` | POST | `/v1/b/{board}/login` | T1 (any generation), D | 148 | 148 | yes | §10.3 |
| `hello` | GET | `/v1/b/{board}/hello` | T1, T2 | 0 | 0 | yes | §10.4 |
| `socket` | GET | `/v1/b/{board}/socket` | T1 | 0 | 0 | yes (standby: no, §15.2) | §11 |
| `read` | GET | `/v1/b/{board}/log` | T1, T2 | 0 | 0 | yes | §10.5 |
| `append` | POST | `/v1/b/{board}/append` | T1, D | 4,194,373 | | yes | §10.6 |
| `blob-list` | GET | `/v1/b/{board}/blobs` | T1, T2 | 0 | 0 | yes | §10.7 |
| `blob-begin` | POST | `/v1/b/{board}/blobs` | T1 | 65 | 65 | yes | §10.7 |
| `blob-meta` | GET | `/v1/b/{board}/blobs/{id}` | T1, T2 | 0 | 0 | yes | §10.7 |
| `blob-put` | PUT | `/v1/b/{board}/blobs/{id}/{part}` | T1 | 1,048,576 | | yes | §10.7 |
| `blob-get` | GET | `/v1/b/{board}/blobs/{id}/{part}` | T1, T2 | 0 | 0 | yes | §10.7 |
| `pair-open` | POST | `/v1/b/{board}/pair` | T1 | 100 | 100 | yes | §10.8 |
| `slot-post` | POST | `/v1/b/{board}/pair/{tid}/{label}` | label `b`: D + P; labels `c`, `d`: T1 | 16,076 | | yes | §10.8 |
| `slot-get` | GET | `/v1/b/{board}/pair/{tid}/{label}` | labels `a`, `c`, `d`: D + P; label `b`: T1 | 0 | 0 | yes | §10.8 |
| `pair-close` | DELETE | `/v1/b/{board}/pair/{tid}` | T1 | 0 | 0 | yes | §10.8 |
| `admin-nonce` | POST | `/v1/b/{board}/admin/nonce` | T1, D, C | 16 | 16 | yes | §12.1 |
| `admin` | POST | `/v1/b/{board}/admin/{op}` | per operation (§12.1 "Credentials"): `seed` C only; every other `{op}` T1, or T1 and D | per operation, §12.1 | per operation | yes | §12 |
| `seed-tail` | POST | `/v1/b/{board}/seed/tail` | T1 | 1,049,610 | | yes | §12.7 |

`slot-post` with label `a` and any route with an `{op}` outside the registry, a `{label}` outside the four, or a `{part}` above 63 do not match and are `404 relay/not_found`.

### 10.2 `create`

Creates a board on an empty object: the Genesis frame as seq 1, the recovery identity and the first device (ruling S2, SYNC.md §9 step 6). The genesis snapshot of an imported board is uploaded afterwards with the first device's token (`blob-begin`, purpose 3).

**Request** (100 + `flen` bytes):

| Offset | Field | Width | Type | Value |
|---|---|---|---|---|
| 0 | `rsign_pk` | 32 | bytes | the genesis recovery signing key |
| 32 | `salt` | 16 | bytes | the board salt |
| 48 | `dev` | 16 | bytes | the first device |
| 64 | `sign_pk` | 32 | bytes | its signing key |
| 96 | `flen` | 4 | u32 | 670 to 262,144 |
| 100 | `frame` | `flen` | bytes | the Genesis frame (`seal.md` §12.2) |

**Checks**, in order:

1. The board is empty; else see "Repeat" below.
2. `H(ctx("board") ‖ rsign_pk ‖ salt)[0..16]` equals the path's board (SEAL-KEY-005); else `400 relay/bad_genesis`.
3. The frame passes P1 to P3 of §9.3; its `dev` equals `dev_R = H(ctx("recovery-dev") ‖ board_id ‖ rsign_pk)[0..16]` (SEAL-KEY-006); its `ctr` is 1; else `400 relay/bad_genesis`.
4. `verifyStrict(rsign_pk, ctx("frame") ‖ frame[0..48 + len], sig)`; else `400 relay/bad_genesis`.
5. `dev` differs from `dev_R` and `sign_pk` from `rsign_pk`; else `400 relay/bad_genesis`.

**Effects**, in one transaction: the board row (`schema = 1`, `state = 1`, `created_at = now`, `base_seq = 0`, `base_chain = C_0 = H(ctx("chain") ‖ board_id)`, `compact_floor = 1`, `purge_floor = 1`, `purge_done = 1`, `compact_snap` NULL, `del_day = 0`, `del_count = 0`, `read_epoch = 1`, `home_gen = 1`, fence and seed columns NULL); the devices row of `dev_R` (`role = 2`, `state = 1`, `gen = 0`, `last_ctr = 1`, `last_seq = 1`); the devices row of `dev` (`role = 1`, `state = 1`, `gen = 1`, `last_ctr = 0`, `last_seq = 0`); the `log` row at seq 1 with `chain = C_1`. The counters columns `app_day`, `app_count` are 0 and `added_at = now`.

The relay cannot see that the Genesis item names the same first device; a mismatch harms only its creator, whose device then cannot append.

**Response** (200):

| Field | Type | Required | Meaning |
|---|---|---|---|
| `seq` | integer | required | 1 |
| `chain` | hex, 32 | required | `C_1` |
| `token` | string | required | the first device's token, `gen = 1` |
| `door` | hex, 5 | required | the board's door (§5.3) |

**Repeat.** A `create` on an `open` board whose `log` row at seq 1 exists with `hash` equal to `H(frame)` and whose devices row `dev` exists with `role = 1`, `state = 1` and the request's `sign_pk`, is answered 200 with the same fields and a new token: `gen` of `dev` = stored + 1 (one row written). Any other `create` on a non-empty board is `409 relay/exists`, with no write. A lost response therefore costs nothing but a retry.

- **RELAY-CREATE-001** (relay) `create` MUST run the checks of §10.2 in order and MUST write nothing when any fails.
- **RELAY-CREATE-002** (relay) A successful `create` MUST store exactly the rows §10.2 lists, with the Genesis frame as the `log` row at seq 1.
- **RELAY-CREATE-003** (relay) A repeated `create` MUST be answered as §10.2 "Repeat" states, and a `create` on a non-empty board otherwise MUST be refused with `409 relay/exists` without a write.

### 10.3 Login

Login gives a device a token, on the board's relay or on a new relay after a re-home (SYNC.md §5.2). It has two steps. The relay keeps the nonce without a timer: a row with its issue time, checked when used (V1-1).

**Step 1, `login-nonce`.** Request: `dev[16]` (16 bytes). With a token, `dev` must equal the token's `dev` (else `403 relay/forbidden`).

Checks: a devices row for `dev` with `role = 1` (else `401 relay/unauthorized`); `state = 1` (else `403 relay/revoked`); `gen < 2^32 − 1` (else `409 relay/gen_exhausted`).

Effects, in one transaction: delete every `nonces` row with `at < now − 120,000`; if 64 or more rows remain, delete the one with the smallest `(at, nonce)` until 63 remain; insert `(nonce, purpose 1, dev, now)` with a fresh 32-byte `nonce` from `crypto.getRandomValues`.

Response (200):

| Field | Type | Required | Meaning |
|---|---|---|---|
| `nonce` | hex, 32 | required | the relay nonce for `seal.md` §16.1 |
| `gen` | integer | required | the generation to request: stored `gen` + 1 |
| `expires` | integer | required | `now + 120,000`, ms since the epoch; display only |

**Step 2, `login`.** Request (148 bytes):

| Offset | Field | Width | Type | Value |
|---|---|---|---|---|
| 0 | `dev` | 16 | bytes | the device |
| 16 | `gen` | 4 | u32 | the generation it signed |
| 20 | `nonce` | 32 | bytes | from step 1 |
| 52 | `client_nonce` | 32 | bytes | the device's own |
| 84 | `sig` | 64 | bytes | `seal.md` §16.1 |

Checks, in order: with a token, `dev` equals the token's `dev` (else `403 relay/forbidden`); a `nonces` row with this `nonce`, `purpose = 1`, the same `dev` and `at ≥ now − 120,000` (else `401 relay/no_nonce`); the devices row has `role = 1` and `state = 1` (else `403 relay/revoked`); `gen` equals stored `gen` + 1 (else `409 relay/bad_gen`); `verifyStrict(sign_pk, login message, sig)` where the login message is `seal.md` §16.1's table with `origin` = the configured `RELAY_ORIGIN` (else `401 relay/bad_login_sig`).

Effects, in one transaction: delete the nonce row (also when a later check fails, so a nonce is tried once); on success set the device's `gen` to the signed `gen`; then close every socket of `dev` whose attachment `gen` is lower, with code 4003 (§11.6).

Response (200):

| Field | Type | Required | Meaning |
|---|---|---|---|
| `token` | string | required | the new token (§5.1), role 1 |
| `gen` | integer | required | its generation |
| `push` | boolean | required | the capability of §15.1 |

- **RELAY-LOGIN-001** (relay) A login nonce MUST be 32 bytes from `crypto.getRandomValues`, stored with its purpose, device and issue time, and accepted at most once and only within 120,000 ms of its issue.
- **RELAY-LOGIN-002** (relay) The relay MUST delete a login nonce when a `login` names it, whether or not the login then succeeds.
- **RELAY-LOGIN-003** (relay) The relay MUST verify the login signature over exactly the bytes of `seal.md` §16.1 with `origin` equal to its configured `RELAY_ORIGIN`, and MUST accept only `gen` equal to the stored generation plus one.
- **RELAY-LOGIN-004** (relay) A successful login MUST set the device's stored generation to the signed one, so that every token with a lower generation is refused from then on, and MUST close the device's sockets with a lower generation.
- **RELAY-LOGIN-005** (relay) The relay MUST keep at most 64 `nonces` rows, removing expired rows and then the oldest as §10.3 states when it issues a nonce.

### 10.4 `hello`

The HTTPS counterpart of the socket's welcome (§11.3), for background jobs and the standby (SYNC.md §4.3 rule 3). It writes nothing.

Response (200):

| Field | Type | Required | Constraints | Meaning |
|---|---|---|---|---|
| `head` | object | required | `{"seq": integer, "chain": hex32}` | the relay head (§7.3) |
| `last_ctr` | integer | optional: present for a role-1 token | | the device's stored `last_ctr` |
| `last_seq` | integer | optional: present for a role-1 token | | the device's stored `last_seq` |
| `appends_left` | integer | optional: present for a role-1 token | 0 to 3,000 | `3,000 − app_count` today (3,000 when `app_day` is not today) |
| `relay_now` | integer | required | | the relay clock |
| `floor` | integer | required | | `max(compact_floor, purge_floor)` |
| `purge_floor` | integer | required | | |
| `state` | string | required | `"open"` or `"fenced"` | |
| `fence` | object | optional: present when `state` is `"fenced"` | `{"host": string, "door": hex5, "home_gen": integer}` | what the fence named; untrusted until the Fence entry is read from the log (`seal.md` §15.4) |
| `door` | hex, 5 | required | | this board's door |
| `push` | boolean | required | | §15.1 |

- **RELAY-HELLO-001** (relay) `hello` MUST answer with the fields of §10.4 computed from the committed state and MUST write no row.

### 10.5 `read`

Catch-up reads pages of at most 256 entries or 1 MiB of frames (SYNC.md §4.7). Query: `after` (required, the client's cursor: the highest seq it holds) and `limit` (optional, 1 to 256, default 256).

**Selection.** Rows with `seq > after` and `seq ≥ purge_floor`, ascending, at most `limit` rows, and at most as many as fit 1,048,576 bytes of frames: rows are taken while the sum of their frame lengths stays at or below 1,048,576, and the first row is always taken (a frame is at most 262,144 bytes, so a page always holds at least 4 frames when 4 remain).

**Page** (the read response body, and the payload of the socket's page and push messages):

| Field | Width | Type | Value |
|---|---|---|---|
| `magic` | 4 | ASCII | `"TDP1"` |
| `head_seq` | 8 | u64 | the relay head |
| `head_chain` | 32 | bytes | |
| `relay_now` | 8 | i64 | the relay clock |
| `floor` | 8 | u64 | `max(compact_floor, purge_floor)` |
| `purge_floor` | 8 | u64 | |
| `flags` | 1 | u8 | bit 0 `more`: rows above the last returned seq exist; bit 1 `truncated`: `after + 1 < floor`; bit 2 `fenced`; other bits 0 |
| `n` | 2 | u16 | 0 to 256 |
| entries | | | `n` × entry |
| | **71 + Σ entries** | | |

**Entry:** `u64 seq ‖ i64 at ‖ u32 flen ‖ frame[flen]`: 20 + `flen` bytes.

**What a client is told about compaction.** When `truncated` is set, the entries between `after + 1` and `floor − 1` that are not in the page were deleted by compaction or purge; those in the page below `floor` are the control entries compaction kept (§12.8). The client then follows `engine.md` (bootstrap from a snapshot, or continue when it needs nothing below the floor). Entries at or above `floor` are always contiguous.

- **RELAY-READ-001** (relay, engine) A read MUST return the rows with `seq > after` and `seq ≥ purge_floor` in ascending seq order, at most `limit` and at most 256 of them, stopping before the row that would take the frames above 1,048,576 bytes except that the first row is always returned.
- **RELAY-READ-002** (relay, engine) Every page MUST carry the relay head, `relay_now`, `floor`, `purge_floor` and the flags of §10.5, with `truncated` set exactly when `after + 1 < floor`.
- **RELAY-READ-003** (relay) A read MUST write no row.

### 10.6 `append`

The batch of §9.2 over HTTPS, for background jobs and the standby; processing and results are §9. Credentials: a role-1 token, or the door for a batch of recovery frames (§9.3 P5). A batch whose layout fails §9.2 (wrong magic, `n` outside 1 to 16, a length prefix past the end, trailing bytes) is `400 relay/bad_batch` with no write. After the transaction commits, every stored entry is pushed (§11.4).

- **RELAY-APPEND-001** (relay) The relay MUST refuse a batch that does not match the layout of §9.2 with `400 relay/bad_batch` before any frame check, and MUST then write no row.
- **RELAY-APPEND-002** (relay) After a batch commits with at least one `new` result, the object MUST push the new entries to every socket that has completed its hello (§11.4).

### 10.7 Snapshots and blobs

A blob is the sealed bytes of `seal.md` §20, stored in parts of at most 1 MiB. The relay never decrypts or hashes a blob: the commitment is the `sha256` the uploader declares, which clients check against the value in a signed entry before decrypting (SEAL-BLOB-002). The relay checks sizes, completeness and who uploads.

**`blob-begin`** request (65 bytes):

| Offset | Field | Width | Type | Value |
|---|---|---|---|---|
| 0 | `id` | 16 | bytes | the blob id (`seal.md` §20.1) |
| 16 | `purpose` | 1 | u8 | 1 periodic snapshot, 2 pairing snapshot, 3 genesis snapshot, 4 other blob (the shared picture) |
| 17 | `seq` | 8 | u64 | the snapshot's `S`; 0 for purpose 4 |
| 25 | `size` | 8 | u64 | the sealed length, 67 to 67,108,864 |
| 33 | `sha256` | 32 | bytes | `H(sealed bytes)` |

The lazy deletions of §13.2 run first, in the request's transaction, and are kept whatever the checks decide. Checks, in order (each failure `400 relay/bad_blob` unless stated):

1. The board is `open`.
2. `size` from 67 to 67,108,864.
3. By purpose: 1: `seq ≥ 64`, `seq ≡ 0 mod 64`, `seq ≤ head_seq`; 2: `1 ≤ seq ≤ head_seq`; 3: `seq = 1` and no blob of purpose 3 with another `id` exists (else `409 relay/blob_exists`); 4: `seq = 0`.
4. No `blobs` row with `id` exists; else `409 relay/blob_exists`.
5. Fewer than 3 incomplete blobs exist, and the sum of `size` of all `blobs` rows plus `size` is at most 805,306,368 (768 MiB); else `409 relay/blob_quota`.

Effects: insert the `blobs` row with `slot` = 1 + the largest `slot` in `blobs` (1 when none), `author` = the token's `dev`, `parts = ceil(size / 1,048,576)`, `received = 0`, `complete = 0`, `at = now`. Response (200): `{"id": hex16, "parts": integer}`.

**Repeat.** When step 4 finds a `blobs` row with this `id` whose `author` is the token's `dev` and whose `purpose`, `seq`, `size` and `sha256` equal the request's, the answer is 200 with the same response and no write other than the lazy deletions; the uploader then reads `blob-meta` and sends the parts it lacks. Any other existing `id` is `409 relay/blob_exists`.

**`blob-put`** of part `i`: the body is the part's bytes. Checks: the blob exists (else `404 relay/no_blob`); its `author` is the token's `dev` (else `403 relay/forbidden`); `i < parts` (else `400 relay/bad_blob`); the body length is exactly `1,048,576` for `i < parts − 1` and `size − (parts − 1) × 1,048,576` for the last part (else `400 relay/bad_blob`). If the part is already stored: identical bytes give 200 with no write; different bytes `409 relay/part_conflict`. Otherwise, in one transaction: insert `parts` row `slot × 64 + i`; `received += 1`; when `received = parts`, set `complete = 1` and run the retention of §13.1. Response (200): `{"received": integer, "complete": boolean}`.

**`blob-meta`** answers for any `blobs` row (complete or not):

| Field | Type | Required | Meaning |
|---|---|---|---|
| `id` | hex, 16 | required | |
| `purpose` | integer | required | 1 to 4 |
| `seq` | integer | required | |
| `author` | hex, 16 | required | the uploading device |
| `size` | integer | required | |
| `parts` | integer | required | |
| `sha256` | hex, 32 | required | as declared |
| `received` | integer | required | |
| `complete` | boolean | required | |

**`blob-list`** answers `{"blobs": [...]}`: the objects of `blob-meta` for every complete blob, ordered by `purpose` ascending, then `seq` descending, then `at` descending, then `id` ascending; only the first 64 objects in that order. Retention (§13) does not bound the count by 64 (each author's newest snapshot and two other blobs per author are kept), so a client looks up any blob a signed entry names by its id with `blob-meta`, never by its presence in the list.

**`blob-get`** of part `i` of a complete blob answers the part's bytes (`application/octet-stream`). An unknown blob is `404 relay/no_blob`, an incomplete one `409 relay/blob_incomplete`, `i ≥ parts` `400 relay/bad_blob`.

- **RELAY-BLOB-001** (relay) `blob-begin` MUST run the checks of §10.7 in order and MUST create the `blobs` row only when all pass.
- **RELAY-BLOB-008** (relay) A `blob-begin` that repeats an existing blob's `id`, author, purpose, seq, size and `sha256` MUST be answered 200 with that blob's `id` and `parts` and MUST write no row other than the lazy deletions of §13.2.
- **RELAY-BLOB-002** (relay) The relay MUST accept a part only from the blob's author, only with exactly the length §10.7 gives for its index, and MUST store each part index at most once, answering a different second upload with `409 relay/part_conflict`.
- **RELAY-BLOB-003** (relay) The relay MUST mark a blob complete exactly when all its parts are stored, and MUST serve parts and list a blob only once it is complete.
- **RELAY-BLOB-004** (relay) The relay MUST NOT decrypt, decompress or parse any blob part.

### 10.8 Pairing tickets and slots

The relay carries the four slots of `seal.md` §17 between E (with its token) and N (with the door and the ticket proof). It stores the ticket id, the slot bytes (opaque, MAC'd by the devices), the creator, the expiry and the tries. It never sees the code `s`, `mac_key`, the SAS, `bundle_key` or the bundle's plaintext; it sees `ticket_key` in N's proof header, checks it in the gate and does not store it. Slot B is MAC'd, not encrypted, so the relay sees N's public keys and name.

**Expiry, checked lazily.** A ticket with `expires ≤ now` is treated as absent by every route and is deleted in the transaction of the request that finds it.

**`pair-open`** (E). Request (100 bytes): `tid[16] ‖ slot A[84]`. Checks: the board is `open`; slot A is `label 0x41 ‖ role 0x45 ‖ u16 48 ‖ body[48] ‖ mac[32]` (else `400 relay/bad_slot`); delete expired tickets; no ticket `tid` exists (else the repeat below, or `409 relay/ticket_exists`); fewer than 4 tickets exist (else `409 relay/ticket_cap`). Effect: insert `(tid, creator = token dev, expires = now + 600,000, tries = 0, a)`. Response (200): `{"expires": integer}`. **Repeat:** when the existing unexpired ticket `tid` has `creator` = the token's `dev` and `a` equal to the request's slot A, the answer is 200 with its stored `expires` and no write other than the deletion of expired tickets.

**Slot layouts the relay checks** (`seal.md` §17.4; it checks the label, role and length only):

| Label | Byte 0 | Byte 1 | `body_len` (bytes 2 to 4) | Total |
|---|---|---|---|---|
| A | `0x41` | `0x45` | 48 | 84 |
| B | `0x42` | `0x4E` | 119 to 242 | 155 to 278 |
| C | `0x43` | `0x45` | 64 | 100 |
| D | `0x44` | `0x45` | 40 to 16,040 | 76 to 16,076 |

In every case the total must equal `36 + body_len`.

**`slot-post`**, in this order of progress: B (N, door and proof), then C (E), then D (E).

| Label | Credential | Requires | Refused when |
|---|---|---|---|
| `b` | D + P | `a` present, `b` absent | `b` present: `409 relay/slot_order` |
| `c` | T1, the ticket's `creator` | `b` present, `c` absent | else `409 relay/slot_order`; other device `403 relay/forbidden` |
| `d` | T1, the ticket's `creator` | `c` present, `d` absent | else `409 relay/slot_order`; other device `403 relay/forbidden` |

Order of checks:

1. The board is `open`.
2. The ticket exists and is unexpired (else `404 relay/no_ticket`; an expired one is deleted).
3. For labels `c` and `d`: the token's `dev` is the ticket's `creator` (else `403 relay/forbidden`).
4. For label `b`: `tries += 1`; when `tries` becomes 5 the ticket is deleted and the post refused with `409 relay/tries`. For tries 1 to 4 the new `tries` value is written even when a later step refuses the post.
5. The slot layout of the table above (else `400 relay/bad_slot`).
6. **Repeat:** the label is already stored with exactly the posted bytes: answer 200 `{}`, storing nothing more (for `b` the try of step 4 stays counted), and send no notice.
7. The progress rule of the table (else `409 relay/slot_order`).

Effect: store the slot. For label `b`, after commit the object sends the slot notice (§11.4) to the creator's sockets. Response (200): `{}`.

**What counts as a try.** Every slot-B post that reaches the object for an existing, unexpired ticket, stored or refused. Tries 1 to 4 are examined, and the first well-formed one is stored; the fifth burns the ticket. So a code holder racing N, or N repeating a post whose answer was lost, has at most four attempts in all. A wrong proof never reaches the object (§6.2 step 7). A slot read is not a try.

**`slot-get`**: N reads `a`, `c`, `d` with the door and the proof; E reads `b` with its token as the ticket's creator (else `403 relay/forbidden`). The ticket must exist and be unexpired (else `404 relay/no_ticket`); an absent slot is `404 relay/slot_empty`; a present one is answered with its bytes (`application/octet-stream`). N polls for C and D; E is told of B on its socket and otherwise polls.

**Single use.** Each label is stored at most once, so one ticket pairs at most one slot B; a repeat of the identical bytes is answered as if it had been stored now, so a lost answer can be retried, and any other second post is `409 relay/slot_order`. The ticket lives until it expires, its creator closes it, or the fifth try. Reading D does not delete it, so a lost response can be retried.

**`pair-close`** (E): the board is `open`; the ticket exists and is unexpired (else `404 relay/no_ticket`); the token's `dev` is its `creator` (else `403 relay/forbidden`). Effect: delete the ticket. Response (200): `{}`. It is the `DeleteTicket{tid}` effect of `seal.md` §17.10.

- **RELAY-PAIR-001** (relay) The relay MUST treat a ticket whose `expires` is at or before the relay clock as absent, and MUST delete it in the transaction of the request that found it.
- **RELAY-PAIR-002** (relay) The relay MUST store each slot label of a ticket at most once, only in the order A, B, C, D, and only from the credential §10.8 assigns to that label.
- **RELAY-PAIR-007** (relay) A `pair-open` or `slot-post` that repeats exactly the bytes already stored for that ticket and label, from the credential allowed for it, MUST be answered 200 without storing anything new, and any other post of a stored label MUST be refused with `409 relay/slot_order` (`pair-open`: `409 relay/ticket_exists`).
- **RELAY-PAIR-003** (relay) The relay MUST count every slot-B post that reaches the object for an unexpired ticket as a try, and MUST delete the ticket and refuse the post with `409 relay/tries` when the count reaches 5.
- **RELAY-PAIR-004** (relay) The relay MUST check each slot's label byte, role byte and length as the table of §10.8 states and MUST NOT check or strip its MAC.
- **RELAY-PAIR-005** (relay) The relay MUST NOT store a ticket proof and MUST keep at most 4 tickets per board.
- **RELAY-PAIR-006** (relay) Slot reads MUST write no row other than the deletion of an expired ticket.

### 10.9 `caps` and `door`

Answered by the front after the gate, with no object call and no row (`X-TD-Relay: gate`).

`caps` (200):

| Field | Type | Required | Value |
|---|---|---|---|
| `v` | integer | required | 1 |
| `push` | boolean | required | `true` from the Worker, `false` from the standby |
| `max_batch` | integer | required | 16 |
| `max_frame` | integer | required | 262,144 |
| `page_entries` | integer | required | 256 |
| `page_bytes` | integer | required | 1,048,576 |
| `part_bytes` | integer | required | 1,048,576 |

`door` (200, create secret only): `{"door": hex5}`, the door of the path's board (§5.3). The device that moves a board asks the new relay for it before it appends its Fence (`seal.md` §12.7), since the object of a new relay does not exist yet.

- **RELAY-CAPS-001** (relay) The front MUST answer `caps` and `door` after the gate without calling the object, with the fields of §10.9.

## 11. The socket

### 11.1 Upgrade

`GET /v1/b/{board}/socket` with `Upgrade: websocket` and a role-1 token. The Worker's gate passes it to the object; the object checks the token row (§8.2) and that the board is `open` or `fenced`, then creates a `WebSocketPair`, calls `ctx.acceptWebSocket(server, [<dev as 32 hex>])`, stores the attachment of §11.2 and answers 101 with the client end. A refused upgrade is an ordinary JSON error response. The standby has no socket (§15.2).

- **RELAY-SOCK-001** (relay) The object MUST accept a socket only for a role-1 token valid at the object (§8.2) on an `open` or `fenced` board, tagged with the device's `dev` as 32 hex characters.

### 11.2 Attachments

Each socket carries a serialised attachment (at most 2,048 bytes), the only per-socket state, which survives hibernation:

| Field | Type | Meaning |
|---|---|---|
| `dev` | 32 hex | the token's device |
| `gen` | integer | the token's generation |
| `session` | 32 hex, or absent before hello | from hello |
| `features` | integer | from hello; 0 before hello |
| `ready` | boolean | true once the welcome has been sent |

### 11.3 Hello and welcome

The client's first message must be hello; the object answers with welcome. Until then the object sends nothing on the socket and pushes nothing to it.

**Client to relay** (all binary; byte 0 is the type):

| Type | Name | Layout | Total |
|---|---|---|---|
| `0x01` | hello | `0x01 ‖ session[16] ‖ u32 features` | 21 |
| `0x02` | append | `0x02 ‖ u32 req ‖ batch (§9.2)` | 5 + batch, at most 4,194,378 |
| `0x03` | read | `0x03 ‖ u32 req ‖ u64 after ‖ u16 limit` (`limit` 1 to 256) | 15 |

`session` is 16 random bytes the client process draws once when it starts and keeps for its life (`device.md`); `features` is 0 in this version and names the optional socket message types a client understands, one bit each, none defined yet. `req` is chosen by the client and echoed in the answer.

**Relay to client:**

| Type | Name | Layout | Total |
|---|---|---|---|
| `0x81` | welcome | below | 93 + fence part |
| `0x82` | result | `0x82 ‖ u32 req ‖ results (§9.6)` | |
| `0x83` | page | `0x83 ‖ u32 req ‖ page (§10.5)` | |
| `0x84` | push | `0x84 ‖ page (§10.5)` | |
| `0x85` | second | `0x85 ‖ session[16]`: the session this socket displaced | 17 |
| `0x86` | displaced | `0x86 ‖ session[16]`: the session that displaced this socket | 17 |
| `0x87` | slot | `0x87 ‖ tid[16] ‖ u8 label` (`0x42`) | 18 |
| `0x88` | error | `0x88 ‖ u32 req ‖ u16 http_status ‖ vb8(code)` | 8 + code length |
| `0x89` | state | `0x89 ‖ u8 state ‖ u8 has_fence ‖ fence part` | 3 + fence part |

**Welcome:**

| Field | Width | Type | Value |
|---|---|---|---|
| `type` | 1 | u8 | `0x81` |
| `head_seq` | 8 | u64 | the relay head |
| `head_chain` | 32 | bytes | |
| `last_ctr` | 8 | u64 | the device's stored `last_ctr` |
| `last_seq` | 8 | u64 | the device's stored `last_seq` |
| `relay_now` | 8 | i64 | |
| `floor` | 8 | u64 | |
| `purge_floor` | 8 | u64 | |
| `state` | 1 | u8 | 1 open, 2 fenced |
| `caps` | 1 | u8 | bit 0 push (always 1 here); other bits 0 |
| `appends_left` | 4 | u32 | as in §10.4 |
| `door` | 5 | bytes | the board's door |
| `has_fence` | 1 | u8 | 0, or 1 when `state = 2` |
| fence part | 0 or `1 + h + 9` | | when `has_fence = 1`: `vb8(host) ‖ door[5] ‖ u32 home_gen` from the board row |
| | **93 + fence part** | | |

**On hello**, in order: if the attachment already has a session, or the message is not exactly 21 bytes, close with 4004. Otherwise, for every other socket tagged with the same `dev` that has a session (a socket that has not said hello yet is left alone; it is handled by these steps when it says hello):

1. if it has the same session as this hello: close it with 4002 (a reconnect of the same process);
2. otherwise: send it `displaced` with this session, close it with 4001, and send this socket `second` with that socket's session.

Then store `session`, `features` and `ready = true` in the attachment and send the welcome. So a device has at most one live socket, a reconnect of one process is silent, and two processes sharing one identity are reported to both ends whichever of them says hello first (V2-21, SYNC.md §4.3). What a client does with `second` and `displaced` is `engine.md`'s. Bits of `features` the relay does not know are stored and otherwise ignored.

- **RELAY-SOCK-002** (relay) The object MUST close with code 4004 a socket whose first message is not a well-formed hello, or that sends a second hello.
- **RELAY-SOCK-003** (relay) On a hello, the object MUST close every other socket of the same device that has completed hello, as the steps of §11.3 state, sending `displaced` and `second` exactly when the two sessions differ, so that at most one socket per device has completed hello; it MUST NOT close a socket that has not yet said hello because of another socket's hello.
- **RELAY-SOCK-004** (relay) The object MUST NOT send any message other than the answers of §11.3 to a socket whose attachment has `ready = false`.
- **RELAY-SOCK-005** (relay) The object MUST NOT send a socket message type whose feature bit the socket's hello did not set, other than the types of §11.3.

### 11.4 Append, read and push over the socket

**Append and read.** An append message is processed exactly as the append route (§9), and answered by `result` with its `req`; a read message exactly as the read route (§10.5) with `after` and `limit`, answered by `page`. Before either, the object re-reads the device row: if the device is no longer current or its `gen` differs from the attachment's, it sends `error` (`403 relay/revoked` or `401 relay/token_stale`) and closes with 4003. A whole-request refusal (board state, a malformed batch, a `limit` outside 1 to 256) is answered by `error` with the request's `req` and the code the HTTPS route would use. A message of an unknown type, of the wrong length for its type, or a text message other than `ping` is answered by `error` with `req = 0` and `400 relay/bad_message`, then the socket is closed with 4004. A message longer than 4,194,378 bytes is closed with 1009 without processing.

**Push.** After any transaction that stored new `log` rows (an append over HTTPS or a socket, `register`, `recovery_register`, `fence` with a frame, `restore_tail`), the object sends each socket with `ready = true`, of every device, the author's included, `push` messages carrying exactly the new entries in seq order, split as pages are (§10.5: at most 256 entries and 1 MiB of frames per message, `more` set on all but the last). The head fields are the head after the commit; `truncated` is 0. A send that throws is ignored: the client catches up by reading.

**Slot notice.** After a slot B is stored, the object sends `slot` with the ticket id to every ready socket of the ticket's creator.

**State notice.** After `fence` commits, the object sends `state` (2, with the fence part) to every ready socket. Sockets stay open on a fenced board for reading.

- **RELAY-SOCK-006** (relay) The object MUST process a socket append and a socket read exactly as the append and read routes, and MUST answer each with the request's `req`.
- **RELAY-SOCK-007** (relay) Before handling a socket append or read, the object MUST re-read the device row and, when the device is not current or its generation differs from the socket's, MUST send `error` and close the socket with 4003.
- **RELAY-SOCK-008** (relay, engine) After every transaction that stores new entries, the object MUST push exactly those entries, in seq order, to every socket with `ready = true`, including the author's.
- **RELAY-SOCK-009** (relay) The object MUST close with 1009, without processing it, a socket message longer than 4,194,378 bytes, and MUST answer a malformed message with `error` `relay/bad_message` and close with 4004.

### 11.5 Pings, hibernation and deploys

Clients send WebSocket protocol pings every 30 seconds (`device.md`); the platform answers them without waking the object and without charge (R3 §2.1). For client libraries that cannot send protocol pings, the object sets `ctx.setWebSocketAutoResponse(new WebSocketRequestResponsePair("ping", "pong"))` in its constructor, so the text message `ping` is answered `pong` by the runtime without waking the object.

Between events the object hibernates; the attachment (§11.2) and SQLite are all it needs when it wakes. A deploy is additive (RELAY-SHAPE-010): an object that adopts new code after hibernating keeps its sockets; a forced update closes them (R3 §2.1), and clients reconnect and say hello again.

- **RELAY-SOCK-010** (relay) The object MUST register the auto-response of §11.5 and MUST NOT require any application message from a client to keep its socket open.

### 11.6 Close codes

| Code | Sent when | The client |
|---|---|---|
| 4001 | another process of the same device said hello (§11.3) | does not reconnect on its own; `engine.md` decides |
| 4002 | the same session said hello on a newer socket | nothing: it is already on the new socket |
| 4003 | the device was revoked, or a login raised its generation | logs in again (or stops, when revoked) |
| 4004 | protocol violation | reconnects with backoff; a repeat is a bug to report |
| 1009 | message too big | as 4004 |
| 1000, 1001, 1006, 1011, 1012 | platform or network | reconnects with backoff (`device.md`) |

- **RELAY-SOCK-011** (relay) The object MUST close sockets only with the codes of §11.6, in the cases the table states.

## 12. Admin operations

An admin operation changes what the relay will accept, or deletes what it holds. Each needs a fresh admin signature (`seal.md` §16.2) from a current member device or the current recovery identity; a token alone never suffices (SYNC.md §5.2, V1-5). The relay cannot see inside entries, so it checks only what §12.2 lists; clients enforce membership from the log (`seal.md` §13).

### 12.1 The nonce and the envelope

**`admin-nonce`.** Request: `signer[16]`. With a role-1 token, `signer` must be the token's `dev` or the `dev` of a role-2 row; with the door, a role-2 row; with the create secret, the board must be empty or seeding (else `409 relay/exists`) and `signer` is not looked up. For a token or the door, the signer's row must exist (`401 relay/unauthorized`) and have `state = 1` (`403 relay/revoked`). The board must be `open` for a token or the door (`403 relay/fenced`, `409 relay/seeding`, `404 relay/no_board`). Effects as §10.3 step 1, with `purpose = 2` and `dev = signer`. Response (200): `{"nonce": hex32, "expires": integer}`.

**Admin request** (`POST …/admin/{op}`):

| Offset | Field | Width | Type | Value |
|---|---|---|---|---|
| 0 | `signer` | 16 | bytes | the signer's `dev` or `dev_R` |
| 16 | `nonce` | 32 | bytes | from `admin-nonce` |
| 48 | `sig` | 64 | bytes | `seal.md` §16.2 over `op`, `board_id`, `signer`, `H(args)`, `nonce` |
| 112 | `args_len` | 4 | u32 | |
| 116 | `args` | `args_len` | bytes | the `seal.md` §16.2 registry layout for `op` |
| 116 + `args_len` | `extra_len` | 4 | u32 | |
| 120 + `args_len` | `extra` | `extra_len` | bytes | per operation, below; not signed |
| | **total** | **120 + `args_len` + `extra_len`** | | |

**Per operation:**

| `op` | `args` width | `extra` | Body cap | Credentials | Signer must be | Section |
|---|---|---|---|---|---|---|
| `register` | 80 | the `DeviceAdded` frame (670 to 262,144), or `u64 seq` (8) | 262,344 | T1 | a current device, the token's | §12.3 |
| `revoke` | 16 | empty | 136 (fixed) | T1, D | a current device other than the target, or the current recovery identity | §12.4 |
| `reinstate` | 16 | empty | 136 (fixed) | T1, D | the current recovery identity | §12.5 |
| `fence` | 10 + host length | empty, or the `Fence` frame | 262,533 | T1, D | a current device or the current recovery identity | §12.6 |
| `seed` | 102 + 58n | empty | 18,782 | C | a current device in the seeded set | §12.7 |
| `compact` | 20 + 8n | empty | 1,048,716 | T1 | a current device other than the snapshot's author | §12.8 |
| `restore_tail` | 12 + 32n | `chain_before[32] ‖ n × (u32 len ‖ frame)` | 1,051,044 | T1, D | a current device or the current recovery identity | §12.9 |
| `purge` | 12 | empty | 132 (fixed) | T1 | a current device | §12.10 |
| `mint_read_token` | 20 | empty | 140 (fixed) | T1, D | the writer named in `args`, a current device or the current recovery identity | §12.11 |
| `revoke_read_token` | 16 | empty | 136 (fixed) | T1, D | a current device or the current recovery identity | §12.11 |
| `recovery_register` | 80 | the `K`-shape frame (670 to 262,144), or `u64 seq` (8) | 262,344 | T1, D | the current recovery identity or a current device | §12.12 |

"Current recovery identity" is the role-2 row with `state = 1`; "current device" a role-1 row with `state = 1`.

**Common checks**, in order, before the operation's own:

| Step | Check | On failure |
|---|---|---|
| A1 | the envelope's lengths add up to the body; `args` has the registry layout and width for `op` (counts within §20); `extra` has the layout of the table | `400 relay/bad_body`; `400 relay/bad_args` |
| A2 | the board state allows `op`: `seed` on an empty or seeding board, every other `op` on an `open` one | `404 relay/no_board`, `409 relay/seeding`, `403 relay/fenced`, `409 relay/exists` (a `seed` on an open or fenced board) |
| A3 | the credential is one the "Credentials" column allows for `op` (the gate has refused any other form, §6.2 step 6); with a role-1 token, `signer` is the token's `dev` or a role-2 row; with the door, `signer` is a role-2 row; `seed` only with the create secret, and the create secret only for `seed` | `403 relay/forbidden` |
| A4 | a `nonces` row with this `nonce`, `purpose = 2`, `dev = signer` and `at ≥ now − 120,000`; it is deleted now, whatever follows | `401 relay/no_nonce` |
| A5 | the signer's row (for `seed`, its entry in `args`) exists, is current, and has a role allowed by the "Signer must be" column | `403 relay/not_signer` |
| A6 | `verifyStrict(signer.sign_pk, admin message, sig)`, the admin message being `seal.md` §16.2's first table with `op` the path's operation name | `401 relay/bad_admin_sig` |
| A7 | the operation's own checks, then its effects, in one transaction | the operation's codes |

- **RELAY-ADMIN-001** (relay) An admin nonce MUST be 32 bytes from `crypto.getRandomValues`, bound to its signer, accepted at most once and only within 120,000 ms of its issue, and deleted when an admin request names it with that signer, whether or not the operation then succeeds.
- **RELAY-ADMIN-002** (relay) The object MUST run the common checks A1 to A7 of §12.1 in order for every admin request and MUST perform no effect of the operation unless all pass.
- **RELAY-ADMIN-003** (relay) The object MUST verify the admin signature over exactly the bytes of `seal.md` §16.2 (SEAL-SIG-003), with `op` taken from the path and `args_hash = H(args)` computed by the relay from the received `args`.
- **RELAY-ADMIN-004** (relay) The object MUST accept each operation only from a signer the "Signer must be" column of §12.1 allows and only with a credential its "Credentials" column allows.

### 12.2 What the relay can and cannot verify

| The relay checks | The relay cannot check (clients do, from the log) |
|---|---|
| the admin signature, strictly, under the signer's registered key | whether a matching control entry exists or will be appended (a `revoke` without its `R`-shape entry) |
| the signer's role and state, and the nonce's freshness and binding | whether the membership in the log agrees with the relay's rows |
| each argument against the relay's own rows (target state, device counts, floors, head, chain) | what any frame contains: its kind, items, acknowledgements or `at_ms` |
| for `register` and `recovery_register`, that `frame_hash` is the hash of the frame carried or named | whether a `DeviceAdded` names the registered keys |
| for frames carried by an operation, everything of §9.3 and §9.4 | whether a compaction's keep list names every control entry below its bound |
| for `restore_tail` and `seed`, every signature, counter and the chain | whether a snapshot was acknowledged, or its content is right |
| ages from its own `at` column, snapshots from its own blob rows | anything a member is allowed to do: a compromised member can do through the relay what it could do through the log |

### 12.3 `register`

Registers a new device's key in the same call that appends the `DeviceAdded` naming it, and returns the new device's first token (`seal.md` §16.2, §17.1 step 9; SYNC.md §4.4).

`args = dev_N[16] ‖ sign_pk_N[32] ‖ frame_hash[32]`. `extra` is the `DeviceAdded` frame (frame mode), or `u64 seq` of an entry already stored (seq mode, used after a restore tail, §12.9).

Checks, in order:

1. `dev_N` is not the `dev` of any row and `sign_pk_N` not the `sign_pk` of any row; else see "Repeat", or `409 relay/duplicate_key`.
2. fewer than 16 role-1 rows with `state = 1`, and fewer than 256 role-1 rows; else `409 relay/device_cap`.
3. frame mode: `H(frame) = frame_hash` (else `400 relay/frame_mismatch`); the frame's `dev` is the signer (else `403 relay/not_author`); the frame passes §9.3 and its §9.4 outcome is `new` (otherwise the operation fails with that outcome's code: `409 relay/conflict`, `409 relay/gap`, or the refusal's).
4. seq mode: a `log` row at `seq` with `seq ≥ purge_floor` and `hash = frame_hash` exists; else `400 relay/frame_mismatch`.

Effects, in one transaction: (frame mode) the append of §9.4 T5; then insert the devices row (`dev_N`, `sign_pk_N`, `role = 1`, `state = 1`, `gen = 1`, `last_ctr = 0`, `last_seq = 0`, counters 0, `added_at = now`). After commit, push (frame mode). The new device's key is accepted for frames sequenced after this one (SEAL-MEM-007 holds by construction: N cannot append before it has its token).

Response (200): `{"seq": integer, "chain": hex32, "token": string}`: the `DeviceAdded`'s seq and chain (frame mode) or the named entry's (seq mode), and N's token with `gen = 1`.

**Repeat.** When step 1 finds a role-1 row for `dev_N` with `state = 1` and the same `sign_pk_N`, and (frame mode) the frame's outcome is `dup` or (seq mode) step 4 passes: answer 200 with the stored seq and chain and a new token, `gen` = stored + 1 (one row written). A lost response is then retried with a fresh nonce.

- **RELAY-REG-001** (relay) `register` MUST append the carried `DeviceAdded` frame and insert the new device's row in one transaction, or do neither.
- **RELAY-REG-002** (relay) `register` MUST refuse with `400 relay/frame_mismatch` a carried or named frame whose hash differs from `frame_hash`, and MUST NOT register a key without such a frame.
- **RELAY-REG-003** (relay) `register` MUST refuse a `dev` or `sign_pk` already present in any row with `409 relay/duplicate_key`, except the repeat of §12.3.
- **RELAY-REG-004** (relay) `register` MUST refuse with `409 relay/device_cap` when 16 role-1 rows are current or 256 role-1 rows exist.

### 12.4 `revoke`

`args = dev[16]`, the target. Sent before the `R`-shape entry that revokes it (`seal.md` §16.2), so the target's next request fails at once (SYNC.md §5.4).

Checks: a role-1 row for `dev` with `state = 1` (else `409 relay/target`); a device signer is not the target (else `403 relay/not_signer`); after the change at least one role-1 row has `state = 1` (else `409 relay/last_device`, ruling S14).

Effects, in one transaction: the target's `state = 2` and `gen` + 1; every `read_tokens` row `state = 2`. After commit: close the target's sockets with 4003. Response (200): `{}`.

- **RELAY-REVOKE-001** (relay) A successful `revoke` MUST set the target's state to revoked and raise its generation in the same transaction, so that every token of the target is refused from the next request on.
- **RELAY-REVOKE-002** (relay) `revoke` MUST be refused with `409 relay/last_device` when it would leave no current role-1 device.
- **RELAY-REVOKE-003** (relay) A successful `revoke` MUST mark every backup-read token revoked and MUST close every socket of the target with 4003.

### 12.5 `reinstate`

`args = dev[16]`. Signer: the current recovery identity only (`seal.md` §13.6). Checks: a role-1 row for `dev` with `state = 2` (else `409 relay/target`); fewer than 16 current role-1 rows (else `409 relay/device_cap`). Effects: `state = 1`, `gen` unchanged; every `read_tokens` row `state = 2`. The device logs in for a new token. Response (200): `{}`.

- **RELAY-REVOKE-004** (relay) `reinstate` MUST be accepted only from the current recovery identity, MUST set the target's state to current without changing its generation, and MUST mark every backup-read token revoked.

### 12.6 `fence`

Ends this relay's service to the board, after which it answers reads only (SYNC.md §6.7, `seal.md` §12.7).

`args = vb8(host) ‖ door[5] ‖ u32 home_gen`. `extra` is empty or the signer's `Fence` frame, appended first so that one call carries both.

Checks: `host` matches `seal.md` §5.4's host rule (else `400 relay/bad_args`); `home_gen > board.home_gen` (else `409 relay/home_gen`); with a frame: its `dev` is the signer (else `403 relay/not_author`), and it passes §9.3 and its §9.4 outcome is `new` or `dup` (else that outcome's code).

Effects, in one transaction: the append (outcome `new`; a `dup` stores nothing); `state = 2`, `home_gen`, `fence_host`, `fence_door` from `args`. After commit: push (if a frame was stored), then the state notice (§11.4). Response (200): `{"seq": integer, "chain": hex32}` with the frame's seq and chain, or `{}` without a frame.

**After a lost response.** A fenced board refuses `admin-nonce` and every admin request with `403 relay/fenced`, so a `fence` is never repeated. A device whose `fence` answer was lost reads `hello`: when `state` is `"fenced"` and `fence` equals its arguments, its fence took effect (`engine.md`).

- **RELAY-FENCE-001** (relay) A successful `fence` MUST set the board state to fenced with the host, door and home generation of its arguments, in the same transaction as the append of its frame when one is carried.
- **RELAY-FENCE-002** (relay) On a fenced board the object MUST refuse with `403 relay/fenced` every request with a token or the door to a route other than `login-nonce`, `login`, `hello`, `socket`, `read`, `blob-list`, `blob-meta` and `blob-get` (among them every append over HTTPS or a socket, `blob-begin`, `blob-put`, every pairing route, `seed-tail`, `admin-nonce` and every admin operation), and MUST keep answering those eight with the effects of their sections; a request with the create secret is refused with `409 relay/exists` (§10.2, §12.1).

### 12.7 `seed` and `seed-tail`

Seeds an empty relay after a re-home: the device set, the newest acknowledged snapshot and up to 30 days of tail including other devices' frames, each signature checked (SYNC.md §6.7, V2-9). The board does not exist on the new relay yet, so the credential is the new relay's create secret (§5.5), and the admin signature is checked against the key the seeded set gives the signer.

`args` (`seal.md` §16.2): `u32 home_gen ‖ u64 first_seq ‖ chain_before[32] ‖ u64 last_seq ‖ chain_last[32] ‖ blob_id[16] ‖ u16 n ‖ n × (dev[16] ‖ sign_pk[32] ‖ u8 role ‖ u8 status ‖ u64 last_ctr)`.

Meanings this spec fixes: `first_seq` is the seq of the first tail frame; `chain_before = C_{first_seq − 1}`; `last_seq` the last tail seq (`first_seq − 1` for no tail) and `chain_last = C_{last_seq}`; `blob_id` names the snapshot the seeding device will upload (informational; the relay does not reserve it); each entry's `last_ctr` is that author's counter at `first_seq − 1`, before the tail, so the relay checks the tail's counters as they arrive. The seeded set is every device and recovery identity of the membership at `last_seq` (after the whole tail), each with its `role` and its `status` there: a device added inside the tail is listed with `last_ctr = 0`, a device revoked inside the tail with `status = 2`, a recovery identity retired inside the tail with `status = 2`. The new relay then accepts appends after the tail from exactly the authors that are current at its end. Members revoked before `first_seq` may be omitted unless they author a tail frame; a tail frame whose author is not listed is refused (`relay/unknown_author`), and the seeding device re-seeds with a complete set.

Checks (A7), in order, each failing with `400 relay/bad_args`:

1. `home_gen ≥ 2`; `first_seq ≥ 1`; `first_seq − 1 ≤ last_seq ≤ 2^53 − 1`; if `last_seq = first_seq − 1`, `chain_last = chain_before`.
2. `n` from 1 to 320; entries in strictly ascending `dev` order; `sign_pk` pairwise distinct; `role` 1 or 2; `status` 1 or 2; `last_ctr` within 0 to 2^53 − 1.
3. every role-2 entry has `dev = H(ctx("recovery-dev") ‖ board_id ‖ sign_pk)[0..16]`; exactly one role-2 entry has `status = 1`; at most 64 role-2 entries.
4. from 1 to 16 role-1 entries with `status = 1`, and at most 256 role-1 entries.

A5 then requires the signer to be a role-1 entry with `status = 1`, and A6 uses its `sign_pk`.

Effects, in one transaction: on a seeding board, delete every row of every table except `nonces` first (a new seed replaces an unfinished one). Insert the board row: `schema = 1`, `state` = 1 (`open`) when the tail is empty, else 3 (`seeding`); `created_at = now`; `base_seq = first_seq − 1`; `base_chain = chain_before`; `compact_floor = first_seq`; `purge_floor = first_seq`; `purge_done = first_seq`; `compact_snap` NULL; `del_day = del_count = 0`; `read_epoch = 1`; `home_gen` from `args`; fence columns NULL; `seed_signer` = the signer, `seed_last_seq = last_seq`, `seed_chain = chain_last` (NULL when `open`). Insert one devices row per entry: `role`, `state = status`, `gen = 0`, `last_ctr` from the entry, `last_seq = 0`, counters 0, `added_at = now`; the signer's row gets `gen = 1`.

Response (200):

| Field | Type | Required | Meaning |
|---|---|---|---|
| `door` | hex, 5 | required | this relay's door for the board |
| `token` | string | required | the signer's token, `gen = 1` |
| `next_seq` | integer | required | `first_seq`: the seq the first `seed-tail` frame must have |
| `state` | string | required | `"seeding"` or `"open"` |

**`seed-tail`** (role-1 token of `seed_signer`, board `seeding`; else `403 relay/forbidden` or `409 relay/target`). Request: `u64 first ‖ u16 n ‖ n × (u32 len ‖ frame)`, `n` from 1 to 256, the frame lengths summing to at most 1,048,576.

Checks, in order: `first = head_seq + 1` (else `409 relay/not_next`); `first + n − 1 ≤ seed_last_seq` (else `409 relay/target`); for each frame in order: P1 to P3 of §9.3; an author row exists, of any role and state (`400 relay/unknown_author`); `verifyStrict` under its `sign_pk` (`400 relay/bad_signature`); `ctr` = the author's `last_ctr` + 1, counting the frames of this request already checked (`409 relay/gap` when above, `409 relay/conflict` when below). A failing frame refuses the whole request, with its 0-based `index` in the error body (§4.2), and nothing is written.

Effects, in one transaction: store every frame as §9.4 T5 without the budget (`app_count` unchanged); each author row written once. If the last stored seq is `seed_last_seq`, its chain must equal `seed_chain` — else the whole request is refused with `409 relay/chain_mismatch` and the device re-seeds — and the board becomes `open` with the seed columns set to NULL. Response (200): `{"head": {"seq": integer, "chain": hex32}, "state": "seeding" | "open"}`.

The seeding device then uploads the snapshot (`blob-begin`, purpose 1) and the other devices log in with the door when they follow (`seal.md` §15.4; `engine.md`).

- **RELAY-SEED-001** (relay) `seed` MUST be accepted only on an empty or seeding board, only with the create secret, and only when the signer is a current role-1 entry of the seeded set whose key verifies the admin signature.
- **RELAY-SEED-002** (relay) `seed` MUST run the four argument checks of §12.7 and MUST store exactly the board and device rows §12.7 lists, with each author's `last_ctr` as its counter before `first_seq`.
- **RELAY-SEED-003** (relay) `seed-tail` MUST store a request's frames only if each has a registered author, a strictly valid signature and the next counter of its author, at the next seq, and MUST otherwise write nothing.
- **RELAY-SEED-004** (relay) The object MUST change a seeding board to open only when the frame at `seed_last_seq` is stored and its chain equals `seed_chain`, and MUST refuse a `seed-tail` that completes the tail with any other chain with `409 relay/chain_mismatch` and no write.
- **RELAY-SEED-005** (relay) While a board is seeding, the object MUST refuse every route other than `seed-tail` from the seeding device and `admin-nonce` and `admin/seed` with the create secret, with `409 relay/seeding`.

### 12.8 `compact`

Compaction deletes entries older than 365 days that a snapshot acknowledged by another device covers, in daily slices, keeping control entries (SYNC.md §4.7, V2-15). The relay sees neither kinds nor acknowledgements, so the signed request names what goes and what stays, and the relay checks what it can itself.

`args = u64 below_seq ‖ u64 snap_entry_seq ‖ u32 n ‖ n × u64 keep_seq` (`n` from 0 to 131,072): delete entries with seq below `below_seq` except the `keep_seq`s (the control entries below it), relying on the snapshot announced by the `snap` entry at `snap_entry_seq`.

Checks, in order:

1. `keep_seq` strictly ascending, each below `below_seq` (else `400 relay/bad_args`).
2. With `lo = max(compact_floor, purge_floor)`: `lo < below_seq ≤ head_seq` (else `409 relay/target`).
3. A `log` row at `snap_entry_seq` with `seq ≥ purge_floor` exists; let `A` be its `dev` (else `409 relay/no_snapshot`).
4. A complete blob of purpose 1 by author `A` with `below_seq ≤ seq < snap_entry_seq` exists; the chosen snapshot is the one with the largest `seq` (then the latest `at`, then the smallest `id`) (else `409 relay/no_snapshot`).
5. `A` is not the signer (else `403 relay/not_signer`): the signer states that it, another device, acknowledged that snapshot.
6. `remaining = 5,000 − (del_count if del_day is today, else 0)` is above 0 (else `429 relay/delete_budget`).
7. `hi = min(below_seq, lo + remaining)`; the `log` row at `hi − 1` has `at ≤ now − 31,536,000,000` (365 days) (else `409 relay/too_recent`).

Effects, in one transaction: delete every `log` row with `lo ≤ seq < hi` whose seq is not a `keep_seq` (`d` rows); `compact_floor = hi`; `compact_snap` = the chosen snapshot's id; `del_day` = today, `del_count` = the used count + `d`; delete every blob of purpose 3 with its parts (§13.3). Response (200): `{"deleted": d, "floor": hi, "more": hi < below_seq}`. A device repeats the request on a later day while `more` is true (`engine.md`).

**Skip targets (ruling S15).** The relay cannot see which frames a later `Skip` may name, so it guarantees the one thing it can: a `log` row at or above `purge_floor` is deleted only by a signed `compact` that puts it below `below_seq` and leaves it out of `keep_seq`, or by a re-seed of an unfinished seed (§12.7); purge deletes only below `purge_floor`, and `seal.md` §13.6 already refuses a Skip target below `purge_seq`. Every other row at or above `purge_floor` stays readable, and a device that lacks a target frame `t` fetches it with `read` (`after = t − 1`, `limit = 1`; the entry returned has seq `t` exactly when the frame is held). So the floor of ruling S15 holds at the relay exactly when every compacting device sends `below_seq` no higher than that floor or lists the frames above it as `keep_seq`; `engine.md` owns the floor and that duty (§22).

- **RELAY-COMPACT-001** (relay) `compact` MUST run the seven checks of §12.8 in order and MUST delete nothing unless all pass.
- **RELAY-COMPACT-002** (relay) `compact` MUST delete only `log` rows with `max(compact_floor, purge_floor) ≤ seq < hi` that are not named as `keep_seq`, and MUST then set `compact_floor` to `hi`.
- **RELAY-COMPACT-003** (relay) `compact` MUST refuse a request whose newest deleted candidate seq `hi − 1` has an `at` younger than 31,536,000,000 ms with `409 relay/too_recent`.
- **RELAY-COMPACT-004** (relay) `compact` MUST record the snapshot it relied on in `compact_snap`, and the retention of §13 MUST NOT delete that snapshot while it is recorded there.
- **RELAY-COMPACT-005** (relay) Compaction and purge together MUST delete at most 5,000 `log` rows per board per UTC day of the relay clock.
- **RELAY-COMPACT-006** (relay) The object MUST NOT delete a `log` row at or above `purge_floor` except by a `compact` whose `below_seq` is above that row's seq and whose `keep_seq` list does not name it, or by the replacement of an unfinished seed (§12.7).

### 12.9 `restore_tail`

After a plain relay rollback, the most advanced device re-uploads frames at their original seqs; the relay accepts them only with registered signatures and if they extend its chain (SYNC.md §4.8, V2-9).

`args = u64 first_seq ‖ u32 n ‖ n × frame_hash[32]` (`n` from 1 to 64). `extra = chain_before[32] ‖ n × (u32 len ‖ frame)`, the frames in seq order, their lengths summing to at most 1,048,576. `chain_before` is the client's `C_{first_seq − 1}`; it is not signed, because it only lets the relay refuse a client that disagrees with its head.

Checks, in order: `first_seq = head_seq + 1` (else `409 relay/not_next`); `chain_before = head_chain` (else `409 relay/chain_mismatch`); for each frame `i` in order: `H(frame_i) = frame_hash_i` (`400 relay/frame_mismatch`); P1 to P3 of §9.3; an author row exists, of any role and state (`400 relay/unknown_author`); `verifyStrict` (`400 relay/bad_signature`); `ctr` = the author's `last_ctr` + 1 counting earlier frames of this request (`409 relay/gap` above, `409 relay/conflict` below). A failing frame refuses the whole request, with its `index` in the error body (§4.2); a refused `restore_tail` writes nothing but the nonce deletion of A4.

Effects, in one transaction: store every frame as §9.4 T5, without the budget; each author row written once. After commit: push. Response (200): `{"head": {"seq": integer, "chain": hex32}}`.

Restore tail does not change any device row's role, state or key. A rollback that lost a registration, revocation, reinstatement or recovery-key change is repaired by the restoring device sending `register` or `recovery_register` in seq mode, `revoke` and `reinstate` for the restored control entries, in log order, between restore calls (`engine.md`).

- **RELAY-RESTORE-001** (relay) `restore_tail` MUST accept frames only at `head_seq + 1` onward, only when `chain_before` equals the relay's head chain, and only when each frame's hash equals the signed list, its author is registered, its signature verifies strictly and its counter is the author's next.
- **RELAY-RESTORE-002** (relay) `restore_tail` MUST store all its frames or none, and MUST NOT change the role, state or key of any device row.

### 12.10 `purge`

"Purge history before now" (SYNC.md §4.7): the rotation that carries `purge_before` (`seal.md` §12.5, ruling S13) is appended as an ordinary entry, and the `purge` operation makes the relay drop everything before it.

`args = u64 before_seq ‖ u32 epoch`: `before_seq` is the `purge_before` of the rotation, `epoch` its new epoch.

Checks: `epoch ≥ 1` (else `400 relay/bad_args`); `purge_floor ≤ before_seq ≤ head_seq` (else `409 relay/target`).

Effects, in one transaction:

1. If `before_seq > purge_floor`: `purge_floor = before_seq`; delete every blob of purposes 1, 2 and 3 with `seq < before_seq`, with its parts (and set `compact_snap` to NULL if it named one of them); `read_epoch = max(read_epoch, epoch)`; every `read_tokens` row `state = 2`.
2. With `remaining` as §12.8 step 6 (0 allowed here): delete every `log` row with `purge_done ≤ seq < min(purge_floor, purge_done + remaining)`; set `purge_done` to that bound; count the deleted rows into `del_day` and `del_count`.

From step 1 on, no row below `purge_floor` is ever served (RELAY-STORE-005), so the purge takes effect at once; step 2 deletes the rows physically within the daily delete budget. A device repeats `purge` with the same `before_seq` on later days until `done` reaches `purge_floor` (`engine.md`). Response (200): `{"purge_floor": integer, "done": integer}`, `done` being `purge_done`.

- **RELAY-PURGE-001** (relay) A successful `purge` with `before_seq` above `purge_floor` MUST raise `purge_floor` to `before_seq` in its transaction, so that no entry below it is served from then on.
- **RELAY-PURGE-002** (relay) `purge` MUST delete every snapshot blob of purposes 1 to 3 below `before_seq` and mark every backup-read token revoked when it raises the floor.
- **RELAY-PURGE-003** (relay) `purge` MUST delete `log` rows only below `purge_floor`, in ascending order from `purge_done`, within the daily delete budget, and MUST accept a repeat with `before_seq = purge_floor` to continue.

### 12.11 Backup-read tokens

Minted per writer and generation, revoked with that device and at every rotation (SYNC.md §5.2, V1-2). The recovery tool, holding no token, mints one for the recovery identity itself (`dev = dev_R`), which is how it reads with a recovery signature (ruling S4).

This keeps ruling S3: the recovery identity never holds a device token. It never logs in, its `devices` row keeps `gen = 0`, no route accepts a role-1 token for it, and its frames are accepted because of their signatures (§9.3), never because of a token. A backup-read token minted for `dev_R` is the role-2 credential of §5.1: it reaches only the read routes of §6.3, is revoked with every other backup-read token at each rotation (§5.2), and exists only because a recovery signature on `mint_read_token` asked for it, which is ruling S4's "recovery-signed read".

**`mint_read_token`**, `args = dev[16] ‖ u32 epoch`. Checks: `dev` equals the signer (else `403 relay/not_signer`); `epoch ≥ 1` (else `400 relay/bad_args`); `epoch ≥ read_epoch` (else `409 relay/stale_epoch`); the row's `gen < 2^32 − 1` (else `409 relay/gen_exhausted`). Effects: if `epoch > read_epoch`, every `read_tokens` row with a lower `epoch` gets `state = 2` and `read_epoch = epoch`; then the row of `dev` gets `gen` = stored + 1 (1 if new), `epoch`, `state = 1`. Response (200): `{"token": string, "gen": integer}` with a role-2 token.

**`revoke_read_token`**, `args = dev[16]`. Checks: a `read_tokens` row for `dev` exists (else `409 relay/target`). Effect: `state = 2`. Response (200): `{}`.

A backup-read token reaches only `caps`, `hello`, `read`, `blob-list`, `blob-meta` and `blob-get` (§6.3).

- **RELAY-RTOKEN-001** (relay) `mint_read_token` MUST be accepted only when the writer named in its arguments is the signer, and MUST invalidate the writer's earlier backup-read token by raising its generation.
- **RELAY-RTOKEN-002** (relay) A `mint_read_token` with an epoch above `read_epoch` MUST mark revoked every backup-read token of a lower epoch, and one with an epoch below `read_epoch` MUST be refused with `409 relay/stale_epoch`.
- **RELAY-RTOKEN-003** (relay) A backup-read token MUST NOT be accepted by any route other than `caps`, `hello`, `read`, `blob-list`, `blob-meta` and `blob-get`.

### 12.12 `recovery_register`

Registers a new recovery identity in the same call that appends the `K`-shape entry installing it (`seal.md` §13.11, §16.2), and retires the old one.

`args = dev_R[16] ‖ sign_pk[32] ‖ frame_hash[32]`; `extra` is the `K`-shape frame (frame mode, authored by the signer) or `u64 seq` (seq mode, after a restore tail).

Checks, in order: `dev_R = H(ctx("recovery-dev") ‖ board_id ‖ sign_pk)[0..16]` (else `400 relay/bad_args`); `dev_R` and `sign_pk` are in no row (else the repeat below, or `409 relay/duplicate_key`); fewer than 64 role-2 rows (else `409 relay/device_cap`); the frame checks of §12.3 steps 3 and 4.

Effects, in one transaction: the append (frame mode); every role-2 row with `state = 1` gets `state = 2` (retired: its later frames are refused at P6); insert the new row (`role = 2`, `state = 1`, `gen = 0`, `last_ctr = 0`, `last_seq = 0`); every `read_tokens` row `state = 2` (the entry rotates). After commit: push (frame mode). Response (200): `{"seq": integer, "chain": hex32}`. **Repeat:** a row for `dev_R` with `role = 2`, `state = 1` and the same `sign_pk`, with the frame's outcome `dup` (or step 4 of §12.3 passing in seq mode), is answered 200 with the stored seq and chain and no write other than the nonce deletion of A4.

- **RELAY-RECKEY-001** (relay) `recovery_register` MUST check that `dev_R` derives from `sign_pk` and the board id (SEAL-KEY-006), and MUST append its frame, retire the previous current recovery identity and insert the new one in one transaction.

## 13. Blob retention

The relay keeps the newest three snapshots plus each author's newest (SYNC.md §4.7, V1-10), the snapshot the latest compaction relied on, pairing snapshots for an hour, the genesis snapshot until the first compaction, and the two newest other blobs of each author. Deletions run only inside the requests named here, never on a timer.

### 13.1 When a blob completes

In the transaction of the `blob-put` that completes a blob:

- **Purpose 1.** Order the complete purpose-1 blobs by `seq` descending, then `at` descending, then `id` ascending. Keep the first three; keep, for each author, its first blob in that order; keep the blob named by `compact_snap`. Delete every other complete purpose-1 blob.
- **Purpose 4.** Order the complete purpose-4 blobs of the completing blob's author by `at` descending, then `id` ascending. Keep the first two; delete the others of that author.
- Purposes 2 and 3: nothing.

### 13.2 When a blob begins

In the transaction of every `blob-begin`, before its checks (§10.7): delete every incomplete blob with `at ≤ now − 3,600,000`, and every complete purpose-2 blob with `at ≤ now − 3,600,000`. A pairing takes at most 10 minutes (`seal.md` §17.11), so an hour covers N's download with room to retry.

### 13.3 Compaction and purge

`compact` deletes every purpose-3 blob (§12.8): once a compaction has relied on a later acknowledged snapshot, no device needs the genesis snapshot to bootstrap. `purge` deletes every blob of purposes 1 to 3 with `seq` below `before_seq` (§12.10). Purpose-4 blobs survive a purge: the epoch they were sealed under is shredded with its root, and the engine re-uploads the shared picture under the new epoch (`engine.md`), after which §13.1 removes the old one.

Deleting a blob deletes its `blobs` row and every `parts` row from `slot × 64` to `slot × 64 + 63`.

### 13.4 What compaction may rely on

A compaction relies on one complete purpose-1 snapshot at `S ≥ below_seq` (§12.8). That snapshot stays until a later compaction relies on another or a purge deletes it, so a device whose cursor falls below the floor can always bootstrap from a snapshot at or above the floor, announced by a `snap` entry the compaction left in place (`snap_entry_seq > S ≥ below_seq`).

- **RELAY-BLOB-005** (relay) On completing a blob of purpose 1 or 4, the object MUST delete exactly the blobs §13.1 says to delete, and no other.
- **RELAY-BLOB-006** (relay) Every `blob-begin` MUST delete the incomplete blobs and the complete purpose-2 blobs whose `at` is 3,600,000 ms or more before the relay clock, before it checks the blob count and bytes.
- **RELAY-BLOB-007** (relay) The object MUST delete blobs only in the cases of §13.1 to §13.3, and every deletion of a blob MUST delete its `blobs` row and all its `parts` rows in one transaction.

## 14. Budgets and quotas

### 14.1 The append budget

Each role-1 author may store at most 3,000 new frames per UTC day of the relay clock (SYNC.md §6.2). The count lives in the author's devices row (`app_day`, `app_count`), which every stored frame updates anyway, so counting costs no row; a new day is noticed when a frame arrives and resets the count in the same write (no timer). `dup`, `conflict` and `gap` results, refused frames, the frames of `restore_tail` and `seed-tail`, and frames authored by a recovery identity do not count. At the budget, a frame gets the refused result `429 relay/budget` (§9.4 T4), and `hello` and the welcome show `appends_left = 0`. Reads go on.

- **RELAY-BUDGET-001** (relay) The object MUST refuse with `429 relay/budget` a new frame of a role-1 author that has already stored 3,000 new frames in the current UTC day of the relay clock, counting as §14.1 states.
- **RELAY-BUDGET-002** (relay) The object MUST keep the append count only in the author's devices row and MUST reset it only when a frame of that author is stored on a later UTC day.

### 14.2 The delete budget

Compaction and purge together delete at most 5,000 `log` rows per board per UTC day (§12.8, RELAY-COMPACT-005): deletes count as rows written (R3 §2.1), and 5,000 deletes cost 10,000 rows written, 10% of the daily limit, on a day with a backlog. The count is `del_day` and `del_count` of the board row.

### 14.3 The client's request budget

A device sends at most 5,000 requests per UTC day of its own clock: HTTPS requests plus socket messages it sends, protocol pings and `ping` texts excluded (SYNC.md §6.2, R3 §6). The relay does not count requests: counting would write a row per request, double the rows of a read-only day and break the rule that a replay writes nothing (RELAY-SEQ-005). A stranger's requests are refused by the gate and cost only Worker requests, which no relay-side counter could stop (SYNC.md §7, the DoS row).

- **RELAY-CLIENT-001** (device) A device MUST NOT send more than 5,000 requests to the relay per UTC day of its own clock, counting HTTPS requests and socket messages it sends and not counting protocol pings or `ping` text messages; over the budget it MUST send nothing but a `hello` every 15 minutes until the next 00:00 UTC.

### 14.4 Caps on stored state

| What | Cap | Enforced by |
|---|---|---|
| nonces | 64 rows; each 120,000 ms | RELAY-LOGIN-005, RELAY-ADMIN-001 |
| tickets | 4; each 600,000 ms | RELAY-PAIR-005, RELAY-PAIR-001 |
| incomplete blobs | 3 | RELAY-BLOB-001 |
| blob bytes declared | 805,306,368 | RELAY-BLOB-001 |
| devices rows | 256 role 1, 64 role 2 | RELAY-REG-004, RELAY-RECKEY-001 |

A leaked door reaches only the pre-token routes, whose stored state these caps bound: the same exposure as a revoked device's stale token (ruling S4).

### 14.5 What a client sees, and how it tells the cases apart

Every response the relay's own code produces carries `X-TD-Relay` (§4.2), so a client can tell relay answers from the platform's.

| The client observes | Case | It shows | It does |
|---|---|---|---|
| no HTTP response: DNS, TCP or TLS failure, or no response within its timeout (`device.md`) | unreachable | "relay unreachable since 14:05 · 3 waiting" | backoff with full jitter, 1 to 60 s (SYNC.md §6.2) |
| a response without `X-TD-Relay`, status 429 (Cloudflare's Error 1027) | the platform's daily quota | "sync paused until 00:00 UTC" | no request until the next 00:00 UTC of its own clock, then one `hello` |
| a response without `X-TD-Relay`, any other status | the platform failed | as unreachable | as unreachable |
| `503 relay/object_unavailable` | the board's object failed or is over its quota | as unreachable | as unreachable |
| `507 relay/storage_refused` | the platform refused a write (rows-written quota or storage full) | "sync paused until 00:00 UTC" | no append until the next 00:00 UTC; reads go on |
| `429 relay/budget` (append result or JSON) | this device's append budget | "daily sync limit reached until 00:00 UTC" | no append until `until` (the next 00:00 UTC); reads go on |
| `429 relay/delete_budget` | compaction or purge budget | nothing | the duty retries on the next UTC day (`engine.md`) |

When a storage call throws inside a transaction (the platform's rows-written limit, or a full database), the core rolls the transaction back and answers `507 relay/storage_refused`. Any other exception escaping the core in the Worker makes the object call throw, and the Worker answers `503 relay/object_unavailable`.

- **RELAY-BUDGET-003** (relay) When a storage call throws during a request, the core MUST roll back the request's transaction and answer `507 relay/storage_refused`, and the front MUST answer `503 relay/object_unavailable` when the object call itself throws.
- **RELAY-CLIENT-002** (device) A client MUST classify every exchange with the relay that matches a row of the table of §14.5 into exactly that row, by the presence of `X-TD-Relay`, the status and the code, and MUST act as that row states; every other relay answer it MUST handle by its code (§19).
- **RELAY-CLIENT-003** (device) A client MUST NOT delete local data or keys, or forget a token, because of any relay answer or the lack of one (V2-21), except that it replaces a token after a successful login.

## 15. Capabilities and the standby

### 15.1 Push or poll (ruling S9)

A client learns whether this relay pushes from `push` in `caps` (§10.9), in the `login` response (§10.3), in `hello` (§10.4), and from bit 0 of the welcome's `caps`. When `push` is true, a visible app keeps one socket open and catches up by reading over it; background jobs use `hello`, `read` and `append` over HTTPS. When `push` is false, every client polls `hello` and `read` over HTTPS at the intervals `device.md` sets. Routes and bodies are otherwise the same.

- **RELAY-CAPS-002** (relay) The Worker MUST report `push: true` and the standby `push: false` in every place §15.1 lists.
- **RELAY-CLIENT-004** (device) A client MUST NOT open a socket to a relay that reported `push: false`.

### 15.2 What the standby does differently

| Item | Worker | Standby |
|---|---|---|
| transport | HTTPS from Cloudflare | `node:https` when `TD_TLS_CERT` and `TD_TLS_KEY` are set, else `node:http` on the configured address, for use behind a TLS proxy such as Caddy (SYNC.md §6.7) or on loopback in tests |
| `socket` route | the object | answered by the gate with `501 relay/no_push` after step 7 of §6.2 |
| push, slot and state notices | sent | none; clients poll |
| `caps.push`, login and hello `push` | `true` | `false` |
| storage | the object's SQLite | one `node:sqlite` `DatabaseSync` file per board, `<TD_DATA_DIR>/<board hex>.sqlite`, WAL, `synchronous = FULL` |
| isolation | one object per board, one event at a time between awaits | one core instance per board in one process; requests for one board are handled one at a time, in arrival order, by a per-board queue |
| durability of an acknowledgement | committed on 3 of 5 replicas (output gate) | committed to the local disk |
| header and body deadlines | the platform's for headers; §6.2 for the body | `server.headersTimeout = 10,000` ms; §6.2 for the body; `server.requestTimeout` = the body deadline + 10,000 ms |
| rows counters | the platform's `rowsRead` and `rowsWritten` | none; the suite counts with its counting storage (§18) |
| platform quotas | Cloudflare's daily limits | none; the relay's own budgets and caps are the same |
| an exception escaping the core | the object call throws: `503 relay/object_unavailable` | caught by the front: `503 relay/object_unavailable` |
| `X-TD-Relay`, statuses, codes, layouts | §4 to §14 | identical |

- **RELAY-STBY-001** (relay) The standby MUST answer the `socket` route with `501 relay/no_push` from the gate, after the credential checks, and MUST send no push.
- **RELAY-STBY-002** (relay) The standby MUST handle the requests of one board one at a time, in arrival order, and MUST keep each board in its own database file named as §15.2 states.
- **RELAY-STBY-003** (relay) The standby MUST set `headersTimeout` and `requestTimeout` as §15.2 states and MUST otherwise behave as RELAY-SHAPE-011 requires.

## 16. Rows per operation and the typical day

### 16.1 Rows written and read

Rows written are exact; rows read are upper bounds (the platform counts rows scanned, which depends on the query plan). `f` is the number of frames stored, `a` the number of distinct authors among them, `x` the number of rows a lazy cleanup deletes, `r` the `read_tokens` rows whose `state` changes, `d` the `log` rows deleted, `P` the parts of a deleted blob, `B` the `blobs` rows, `k` the entries returned.

| Operation | Rows written | Rows read, at most |
|---|---|---|
| any gate refusal; `caps`; `door` | 0 (no object call) | 0 |
| `create` | 5 (board, 2 devices, log row and its index) | 3 |
| `create` repeat | 1 | 3 |
| `login-nonce`, `admin-nonce` | 1 + `x` | 2 + 64 |
| `login` (success) | 2 (nonce, device) | 3 |
| `login` (refused after the nonce was found) | 1 | 3 |
| `hello`; socket upgrade; socket hello | 0 | 3 |
| `read`, socket read of `k` entries | 0 | 4 + `k` |
| append with `f` new frames by `a` authors (the rest `dup` or stopped) | `2f + a` | 3 + 3 × frames in the batch |
| append whose frames are all `dup`, `conflict`, `gap` or refused | 0 | 3 + 3 × frames in the batch |
| `blob-begin` | 1 + Σ(1 + `P`) over blobs cleaned up | 2 + `B` |
| `blob-begin` repeat | Σ(1 + `P`) over blobs cleaned up | 2 + `B` |
| `blob-put` (part already stored, identical) | 0 | 3 |
| `blob-put` | 2 (part, blob row) + Σ(1 + `P`) over blobs deleted on completion | 3 + `B` |
| `blob-list`, `blob-meta`, `blob-get` | 0 | 2 + `B` |
| `pair-open` | 1 + `x` | 2 + 4 |
| `pair-open` repeat | `x` | 2 + 4 |
| `slot-post`, `pair-close`, fifth try | 1 | 3 |
| `slot-post` repeat of a stored slot | 1 for label `b` (the try); 0 for `c` and `d` | 3 |
| `slot-get` | 0 (1 if it deletes an expired ticket) | 3 |
| `register` (frame mode) | 5 (nonce, log row and index, signer row, new device) | 8 |
| `register` (seq mode) | 2 | 6 |
| `register` repeat | 2 | 8 |
| `revoke`, `reinstate` | 2 + `r` | 4 + 64 |
| `fence` | 2 (nonce, board); 5 with a stored frame | 8 |
| `seed` | 2 + `n` (nonce, board, devices); plus every row deleted on a re-seed | 3 + 2`n` |
| `seed-tail` | `2f + a`; + 1 (board) when it completes | 3 + 3`f` |
| `compact` | 2 + 2`d` + Σ(1 + `P`) over genesis blobs | 8 + `B` + `d` + (`hi − lo`) |
| `restore_tail` | 1 + `2f + a` | 5 + 3`f` |
| `purge` | 2 + `r` + 2`d` + Σ(1 + `P`) over deleted blobs | 5 + `B` + 256 + `d` |
| `mint_read_token` | 2 + `r` (nonce, the writer's row); 3 + `r` when it raises `read_epoch` (the board row too) | 4 + 256 |
| `revoke_read_token` | 2 | 4 |
| `recovery_register` (frame mode) | 6 + `r` (nonce, log row and index, signer row, old and new recovery rows) when a device signs; 5 + `r` when the retiring recovery identity signs (its row is written once) | 10 + 64 |
| `recovery_register` (seq mode) | 3 + `r` (nonce, old and new recovery rows) | 8 + 64 |
| `recovery_register` repeat | 1 (the nonce) | 10 + 64 |
| any admin request refused after A4 | 1 (the nonce) | 4 |

- **RELAY-ROWS-001** (relay) Every operation MUST write exactly the rows the table of §16.1 states for it on the Worker's platform counters and on the suite's counting storage, and MUST read at most the rows it states.

### 16.2 The typical day

SYNC.md §6.2's day, recomputed from §16.1: 3 devices (two desktops, one phone), 300 edits, 50 phone opens, 48 background syncs; plus what that day implies: 3 beats; 280 edits sent over sockets and 20 queued on the phone and sent by background jobs; the desktops reconnecting 10 times each; one compaction of one day's entries (305) a year after creation; one snapshot of 3 MB (3 parts) a week, which replaces an older one.

| Item | Count | Worker requests | Object requests | Rows written | Rows read |
|---|---|---|---|---|---|
| socket upgrades (phone opens, desktop reconnects) | 70 | 70 | 70 | 0 | 210 |
| socket hellos | 70 | 0 | 3.5 (70 ÷ 20) | 0 | 210 |
| socket appends (edits and beats) | 283 | 0 | 14.2 | 849 | 1,698 |
| socket catch-up reads (6 entries each) | 70 | 0 | 3.5 | 0 | 700 |
| background `hello` and `read` | 96 | 96 | 96 | 0 | 576 |
| background appends | 20 | 20 | 20 | 60 | 120 |
| compaction (`admin-nonce`, `compact` deleting 305) | 2 | 2 | 2 | 1 + 612 | 66 + 620 |
| weekly snapshot (begin, 3 parts, one older snapshot deleted), per day | 4 ÷ 7 | 0.6 | 0.6 | 11 ÷ 7 = 1.6 | 2 |
| retries and `caps` | 10 | 10 | 5 | 0 | 15 |
| **total** | | **≈ 199** | **≈ 215** | **≈ 1,524** | **≈ 4,217** |
| free-plan daily limit | | 100,000 | 100,000 | 100,000 | 5,000,000 |
| **share** | | **0.20%** | **0.22%** | **1.52%** | **0.08%** |

Duration, on R3 §6's model (each event 50 ms at 128 MB): about 610 events × 0.05 s × 0.125 GB ≈ 3.8 GB-s of 13,000 (0.03%); the real figure is among SYNC.md §11's measurements, taken in the owner's rehearsal. Storage is not a daily limit: after a year, with compaction, about 0.15 GB of 5 GB (SYNC.md §6.2).

- **RELAY-ROWS-002** (relay) The conformance suite MUST replay the day of §16.2 against the Worker target and MUST fail unless the Worker requests, object requests, rows written and rows read it measures, plus the compaction counted on the core target, each stay below 2% of the daily limit of the table.

## 17. Deployment and configuration

### 17.1 The Worker

| Name | Kind | Value |
|---|---|---|
| `TOKEN_KEY` | secret | 32 bytes as 64 lowercase hex characters |
| `CREATE_SECRET` | secret, optional | 32 bytes as 64 lowercase hex characters; absent once the board is created |
| `RELAY_ORIGIN` | variable | the relay origin of `seal.md` §5.4, e.g. `https://relay-x7k2.example.workers.dev` (symbolic) |
| `BOARD` | Durable Object binding | class `Board`, exported by `worker.js` |
| migration | `[[migrations]]` | `tag = "v1"`, `new_sqlite_classes = ["Board"]` |
| `compatibility_flags` | | `["nodejs_compat"]` (for `node:crypto`) |
| `compatibility_date` | | a date pinned in `relay/wrangler.toml` |
| `workers_dev`, `preview_urls` | | `true` (under a random Worker name, SYNC.md §6.1), `false` |
| `[observability]` | | `enabled = false` |

When `TOKEN_KEY` is not 64 lowercase hex characters, or `RELAY_ORIGIN` is not a relay origin, every request is answered `503 relay/misconfigured` by the gate. `relay/deploy.sh` (stage `release`) uploads exactly this configuration through the Script Upload API (SYNC.md §8).

### 17.2 The standby

| Environment variable | Required | Value |
|---|---|---|
| `TD_TOKEN_KEY` | yes | as `TOKEN_KEY` |
| `TD_CREATE_SECRET` | no | as `CREATE_SECRET` |
| `TD_RELAY_ORIGIN` | yes | as `RELAY_ORIGIN` |
| `TD_DATA_DIR` | yes | an existing directory for the board files |
| `TD_LISTEN` | yes | `host:port`, e.g. `127.0.0.1:8443` |
| `TD_TLS_CERT`, `TD_TLS_KEY` | no, both or neither | PEM file paths |

`node standby.js` reads these, exits with status 2 and a one-line message naming the variable (never its value) when one is missing or invalid, and otherwise serves until it receives `SIGTERM`. `startStandby` (§23) takes the same values as options, plus an injected clock for tests.

- **RELAY-SHAPE-012** (relay) The Worker and the standby MUST read their configuration only under the names of §17, and MUST answer every request with `503 relay/misconfigured` (the Worker) or refuse to start (the standby) when a required value is invalid.

## 18. The conformance suite

`relay/test/` holds one `node --test` suite with three targets, chosen by `TD_TARGET`: `core` (the core in process over `node:sqlite`, with a counting storage and an injected clock), `standby` (the standby over HTTP on loopback, started in process with an injected clock) and `worker` (`wrangler dev` with the pinned `wrangler`, real clock). It signs its own frames, login and admin messages from `seal.md`'s layouts with WebCrypto in test-only helpers; stage `device` later adds `seal`'s published vectors (SYNC_PLAN.md §6.4).

**The counting storage** wraps `Storage` and counts rows written as the platform does: for each statement of `SQL` (§23), the number of rows it changed times one plus the number of indexes on its table (2 for `log`, 1 for every other table). The suite checks on the Worker target that the platform's counters agree with it for every operation that target can run.

**What it must cover**, each item as at least one case on every target that can run it (time-dependent cases on `core` and `standby` only):

1. every route of §10.1 and every socket message of §11, with each success and each error of their sections;
2. the gate: unauthenticated, wrong-credential, oversized and slow requests refused with `X-TD-Relay: gate` and no object call (on the Worker, the object is observed through a test-only route of the suite's own harness Worker that wraps `Board` and counts calls; the shipped `worker.js` has no such route);
3. sequencing: new, dup, conflict, gap, stop rules, recovery frames from a token and from the door, the budget; a replay writes no row;
4. every admin operation, its signer rules and its repeat;
5. strict Ed25519: every Wycheproof `ed25519_test.json` case and every case of `seal-signatures.json` and `seal-frame.json`, through append and login (RELAY-GEN-010);
6. the rows of §16.1 for every operation, and the day of §16.2;
7. `relay/package.json` has no runtime dependency; no relay source file contains `console.`, `eval(`, `new Function`, `setTimeout`, `setInterval` or `setAlarm` (a source scan).

- **RELAY-TEST-001** (relay) The suite MUST run the cases of §21 against the `core`, `standby` and `worker` targets, every case on each target its `targets` field names, and the gate MUST require all three to pass.
- **RELAY-TEST-002** (relay) The suite MUST prove for each refusal of §6.2 on the Worker target that the object was not called.

## 19. Error catalogue

HTTP refusals carry the JSON body of §4.2; per-frame refusals are results of §9.6; socket refusals are `error` messages (§11.3). "Gate" in the second column means the front raises it without calling the object.

| Code | Meaning (status) | Raised when | What the caller does |
|---|---|---|---|
| `relay/not_found` | no such route (404; gate) | §4.1, §6.2 step 1 | a client bug: report it |
| `relay/bad_query` | query not allowed (400; gate) | §4.1, §6.2 step 2 | a client bug |
| `relay/unexpected_body` | body on a route without one (400; gate) | §6.2 step 3 | a client bug |
| `relay/length_required` | no single decimal `Content-Length`, or `Transfer-Encoding` sent (411; gate) | §6.2 step 3 | a client bug |
| `relay/too_large` | body over the route's cap (413; gate) | §6.2 step 4 | a client bug: split or refuse locally |
| `relay/bad_body` | body length or layout wrong (400; gate or object) | §6.2 steps 4, 9; §4.2; §12.1 A1 | a client bug |
| `relay/bad_upgrade` | socket route without `Upgrade: websocket` (400; gate) | §6.2 step 5 | a client bug |
| `relay/unauthorized` | no valid credential for this route, or an unknown device (401; gate or object) | §5, §6.2 steps 6 and 7, §8.2 | check the relay address; a device with a token logs in |
| `relay/forbidden` | credential valid but not allowed for this use (403; gate or object) | §6.3, §8.2, §10, §12.1 A3 | a client bug |
| `relay/create_disabled` | no `CREATE_SECRET` configured (403; gate) | §5.5 | the owner sets it, or nothing to do |
| `relay/bad_proof` | ticket proof does not match `tid` (403; gate) | §6.2 step 7 | N re-enters the code |
| `relay/slow` | body not complete within its deadline (408; gate) | §6.2 step 9 | retry with backoff |
| `relay/object_unavailable` | the object call threw (503; gate) | §6.2 step 10, §14.5 | as unreachable (§14.5) |
| `relay/misconfigured` | invalid relay configuration (503; gate) | §6.2 step 0, §17.1 | as unreachable; the owner fixes the deploy |
| `relay/no_push` | the standby has no socket (501; gate) | §15.2 | poll |
| `relay/no_board` | the board does not exist here (404) | §8.1 | check the relay address |
| `relay/seeding` | the board is being seeded (409) | §8.1, §12.7 | retry later |
| `relay/fenced` | the board is fenced: reads only (403) | §8.1, §12.6 | stop writing; follow the move code (`engine.md`) |
| `relay/exists` | the board exists already (409) | §10.2, §12.1 A2 | nothing: the board is there |
| `relay/revoked` | the device or author is revoked or retired (403) | §8.2, §9.3 P6, §9.4 T1 | `engine.md`: "re-pair and resubmit" (SYNC.md §5.4) |
| `relay/token_stale` | the token's generation is not current (401) | §8.2, §11.4 | log in (§10.3) |
| `relay/gen_exhausted` | the generation cannot be raised (409) | §5.2 | replace the device |
| `relay/bad_genesis` | the create request is inconsistent (400) | §10.2 | a client bug |
| `relay/no_nonce` | the nonce is unknown, used, expired or bound to another device (401) | §10.3, §12.1 A4 | fetch a new nonce and sign again |
| `relay/bad_gen` | the login asks for another generation than stored + 1 (409) | §10.3 | fetch a new nonce (its `gen` is current) |
| `relay/bad_login_sig` | the login signature fails (401) | §10.3 | check the configured relay origin; `engine.md` alarms on a repeat |
| `relay/bad_admin_sig` | the admin signature fails (401) | §12.1 A6 | as above |
| `relay/not_signer` | this signer may not perform this operation (403) | §12.1 A5, §12.4, §12.8, §12.11 | a client bug |
| `relay/bad_args` | the operation's arguments are invalid (400) | §12 | a client bug |
| `relay/bad_batch` | the batch layout is wrong (400) | §10.6 | a client bug |
| `relay/bad_frame` | frame length, magic, `len` or `ctr` wrong (400; per frame) | §9.3 P1, P3 | a client bug: never resend this frame |
| `relay/wrong_board` | the frame names another board (400; per frame) | §9.3 P2 | a client bug |
| `relay/unknown_author` | no row for the frame's `dev` (400; per frame) | §9.3 P4, §12.7, §12.9 | a new device waits for its registration; else a bug |
| `relay/not_author` | the frame's author is not the token's device and not a recovery identity (403; per frame) | §9.3 P5, §12.3, §12.6 | a client bug |
| `relay/bad_signature` | the frame's signature fails strictly (400; per frame) | §9.3 P7, §12.7, §12.9 | `engine.md`: someone else is signing, or local corruption |
| `relay/budget` | the author's daily append budget is used (429; per frame or JSON, with `until`) | §14.1 | §14.5 |
| `relay/conflict` | another frame is stored under this `(dev, ctr)` (409; per frame or JSON) | §9.4 T3b, T3c; §12.7; §12.9 | SYNC.md §4.3 rule 4 (`engine.md`) |
| `relay/gap` | the counter is above the next one (409; per frame or JSON) | §9.4 T3d; §12.7; §12.9 | SYNC.md §4.3 rule 3: fetch own frames first |
| `relay/duplicate_key` | the id or key is already registered (409) | §12.3, §12.12 | the device generates new keys and pairs again |
| `relay/device_cap` | too many devices or recovery identities (409) | §12.3, §12.5, §12.12 | revoke a device first |
| `relay/frame_mismatch` | the carried or named frame's hash differs from the signed one (400) | §12.3, §12.9, §12.12 | a client bug |
| `relay/target` | the target or seq range is not allowed in the relay's state (409) | §12.4, §12.5, §12.7, §12.8, §12.10, §12.11 | re-read state; usually already done |
| `relay/last_device` | the revocation would leave no current device (409) | §12.4 | refuse in the UI (ruling S14) |
| `relay/home_gen` | the fence's home generation is not above the stored one (409) | §12.6 | a client bug |
| `relay/not_next` | the tail does not start at the relay head + 1 (409, with `head`) | §12.7, §12.9 | compare `head` with its own chain: equal means an earlier request landed, so resume after it; a fork means a re-home |
| `relay/chain_mismatch` | the chain does not extend the relay's, or the seed tail ends on another chain (409) | §12.7, §12.9 | restore impossible: re-home, or re-seed |
| `relay/no_snapshot` | no qualifying snapshot for the compaction (409) | §12.8 | upload or wait for an acknowledged snapshot |
| `relay/too_recent` | entries younger than 365 days by the relay clock (409) | §12.8 | retry later with a lower bound |
| `relay/delete_budget` | 5,000 deletions already today (429, with `until`) | §12.8, §14.2 | retry on the next UTC day |
| `relay/stale_epoch` | a read token for an epoch below the relay's (409) | §12.11 | mint for the current epoch |
| `relay/bad_blob` | blob arguments, part index or part length wrong (400) | §10.7 | a client bug |
| `relay/blob_exists` | a blob with this id but other fields, or a second genesis snapshot, exists (409) | §10.7 | draw a new id; or nothing |
| `relay/blob_quota` | too many incomplete blobs or too many bytes (409) | §10.7 | retry after an hour |
| `relay/no_blob` | no such blob (404) | §10.7 | the blob was deleted: use another snapshot |
| `relay/blob_incomplete` | the blob is not complete (409) | §10.7 | wait for the uploader |
| `relay/part_conflict` | a different part with this index is stored (409) | §10.7 | a client bug: start a new blob |
| `relay/bad_slot` | slot label, role or length wrong (400) | §10.8 | a client bug |
| `relay/ticket_exists` | the ticket id is registered with another creator or slot A (409) | §10.8 | draw a new code |
| `relay/ticket_cap` | 4 tickets are open (409) | §10.8 | wait 10 minutes |
| `relay/no_ticket` | no such ticket, or it expired (404) | §10.8 | pair again |
| `relay/tries` | the fifth slot-B post burned the ticket (409) | §10.8 | pair again with a new code |
| `relay/slot_order` | the slot is already posted with other bytes, or posted before its predecessor (409) | §10.8 | abort the pairing (`seal.md` §17.10) |
| `relay/slot_empty` | the slot is not posted yet (404) | §10.8 | poll again |
| `relay/bad_message` | a malformed socket message (400; then close 4004) | §11.4 | a client bug |
| `relay/storage_refused` | the platform refused a write (507) | §14.5 | §14.5 |
| `relay/address` | a typed relay address does not parse (client only) | §5.4 | ask again |

## 20. Limits

| Name | Value | Unit | Enforced by |
|---|---|---|---|
| frames per batch | 1 to 16 | frames | RELAY-APPEND-001 |
| frame size | 670 to 262,144 | bytes | RELAY-SEQ-006 |
| `ctr`, seq and every integer read | 2^53 − 1 | | RELAY-GEN-008 |
| append body | 4,194,373 | bytes | RELAY-GATE-005 |
| socket message | 4,194,378 | bytes | RELAY-SOCK-009 |
| body deadline | 10,000 + ceil(length × 1,000 / 65,536) | ms | RELAY-GATE-006 |
| page | 256 entries and 1,048,576 bytes of frames (at least one entry) | | RELAY-READ-001 |
| `limit` | 1 to 256 | entries | RELAY-ENC-002 |
| login and admin nonce lifetime | 120,000 | ms | RELAY-LOGIN-001, RELAY-ADMIN-001 |
| nonces stored | 64 | rows | RELAY-LOGIN-005 |
| ticket lifetime | 600,000 | ms | RELAY-PAIR-001 |
| tickets per board | 4 | tickets | RELAY-PAIR-005 |
| slot-B tries | 5 (the fifth burns) | posts | RELAY-PAIR-003 |
| slot sizes A, B, C, D | 84; 155 to 278; 100; 76 to 16,076 | bytes | RELAY-PAIR-004 |
| blob size | 67 to 67,108,864 | bytes | RELAY-BLOB-001 |
| blob part | 1,048,576 | bytes | RELAY-BLOB-002 |
| parts per blob | 64 | parts | RELAY-BLOB-001 |
| incomplete blobs | 3 | blobs | RELAY-BLOB-001 |
| declared blob bytes per board | 805,306,368 | bytes | RELAY-BLOB-001 |
| incomplete and pairing blob lifetime | 3,600,000 | ms | RELAY-BLOB-006 |
| snapshots kept | newest 3, each author's newest, `compact_snap` | blobs | RELAY-BLOB-005 |
| other blobs kept | 2 per author | blobs | RELAY-BLOB-005 |
| append budget | 3,000 | new frames per role-1 author per UTC day | RELAY-BUDGET-001 |
| delete budget | 5,000 | `log` rows per board per UTC day | RELAY-COMPACT-005 |
| client request budget | 5,000 | requests per device per UTC day | RELAY-CLIENT-001 |
| compaction age | 31,536,000,000 | ms (365 days) of relay clock | RELAY-COMPACT-003 |
| compaction keep list | 131,072 | seqs | RELAY-COMPACT-001 |
| restore tail per request | 64 frames, 1,048,576 bytes of frames | | RELAY-RESTORE-001 |
| seed tail per request | 256 frames, 1,048,576 bytes of frames | | RELAY-SEED-003 |
| seeded entries | 320 (at most 256 role 1, 64 role 2, 16 current role 1) | entries | RELAY-SEED-002 |
| current devices | 16 | rows | RELAY-REG-004 |
| device rows | 256 role 1, 64 role 2 | rows | RELAY-REG-004, RELAY-RECKEY-001 |
| token | 96 | ASCII characters | RELAY-TOKEN-001 |
| door | 5 | bytes | RELAY-DOOR-001 |
| relay address | host, `/`, 34 characters | | RELAY-ADDR-001 |
| generation | 2^32 − 1 | | RELAY-TOKEN-004 |
| socket attachment | 2,048 | bytes | RELAY-SHAPE-008 |
| typical day | 2% of every daily limit | | RELAY-ROWS-002 |

## 21. Test vectors

Stage `relay` publishes these families under `relay/test/vectors/`, one JSON file each, in the format of README §7. They are the conformance suite's fixtures (§18); `seal`'s published vectors are added by stage `device` (SYNC_PLAN.md §6.7).

**Case format.**

| Field | Type | Meaning |
|---|---|---|
| `name` | string | unique within the file |
| `targets` | array of `"core"`, `"standby"`, `"worker"` | where the case runs; time-dependent cases omit `"worker"` |
| `setup` | object | `token_key` (hex32), `create_secret` (hex32, optional), `origin`, `clock_ms`; `keys`: name → `{"seed": hex32}` (an Ed25519 seed, RFC 8032, so signatures are deterministic); `boards`: name → `{"salt": hex16, "recovery": key name, "first": key name}`, created by a `create` the harness performs; `state`: further steps run before the case, in the step format |
| `steps` | array | run in order |

**Step:** `{"clock_ms"?: integer, "request": …, "expect": …}` for HTTPS, or `{"socket": name, "open" | "send" | "close": …, "expect": …}` for a socket.

`request`: `{"route": name of §10.1, "board": board name, "params": {…}, "query": {…}, "cred": {"token": key name} | {"read_token": key name} | {"door": board name} | {"create": true} | {"raw": header value} | {}, "proof": hex32?, "body": builder, "slow": {"declare": integer, "send": integer}?}`.

**Builders** (expanded by the harness, never sent as JSON): `{"$hex": "…"}`; `{"$batch": [builder…]}`; `{"$frame": {"board", "author", "ctr", "body_len", "fill", "sig": "good" | "flip" | "zero-s" | "high-s"}}`, whose body is `ver 1 ‖ suite 1 ‖ epoch u32 1 ‖ 24 zero bytes ‖` filler bytes `H(fill ‖ u32 0) ‖ H(fill ‖ u32 1) ‖ …` cut to `body_len`, signed as SEAL-FRAME-002 (the relay never opens the body); `{"$login": {"dev", "gen", "nonce": "$last.nonce" | hex32, "client_nonce", "origin"}}`; `{"$admin": {"op", "signer", "nonce", "args": {…named fields of the seal.md §16.2 registry…}, "extra": builder}}`; `{"$slot": {"label", "body_len"}}`; `{"$blob_begin": {…}}`; `{"$part": {"blob", "index", "fill"}}`. `"$last.<field>"` refers to the previous step's JSON response.

`expect`: `{"status", "x_td_relay": "gate" | "object", "object_called": boolean, "json": {…} (exact, `"$any"` for a random or token field), "results": [{"status": "new", "seq", "chain": "$chain"}…], "page": {…}, "message": {…}, "rows_written": integer, "rows_read_max": integer, "storage": {"board": {…}, "devices": {…}, "log": [{"seq", "dev", "ctr"}…], "blobs": […], "tickets": […]} (each a partial match on the named rows), "elapsed_ms": [min, max]}`. `"$chain"` means the value the harness computes with SEAL-CHAIN-001.

**Families.**

| File | Spec | Covers |
|---|---|---|
| `relay-gate.json` | §5, §6 | every gate step's refusal; slow bodies; header forms; `object_called: false` |
| `relay-token.json` | §5.1, §5.2, §8.2 | token bytes and text for given keys; stale, revoked, unknown and wrong-board tokens; generations |
| `relay-address.json` | §5.4 | address text and display for given hosts, boards and doors; typed inputs with spaces, hyphens, case, look-alikes, `https://`, ports and errors |
| `relay-create.json` | §10.2 | create, its checks, repeat and `exists` |
| `relay-login.json` | §10.3 | both steps, expiry, reuse, wrong generation, wrong origin, socket closes |
| `relay-append.json` | §9 | every row of §9.3 and §9.4, stop rules, recovery frames by token and door, replays with `rows_written: 0` |
| `relay-read.json` | §10.4, §10.5 | pages, limits, the 1 MiB rule, floors and `truncated` |
| `relay-socket.json` | §11 | hello, welcome, second and displaced sessions, push, slot and state notices, errors and close codes |
| `relay-blobs.json` | §10.7, §13 | begin, parts, completion, retention, lazy expiry, quotas |
| `relay-pairing.json` | §10.8 | slot order, credentials, tries, expiry, notices |
| `relay-admin.json` | §12.1 to §12.6, §12.11, §12.12 | nonces, envelope, signer rules, every operation and its repeat |
| `relay-seed.json` | §12.7 | seed, re-seed, tail, chain completion |
| `relay-compact.json` | §12.8, §13.4 | every check, slices, the age rule, `compact_snap` |
| `relay-restore.json` | §12.9 | restore tail and the seq-mode registrations |
| `relay-purge.json` | §12.10 | floors, slices, blob and token effects |
| `relay-budget.json` | §14 | the append and delete budgets across a UTC day boundary; `storage_refused` |
| `relay-rows.json` | §16.1 | one case per row of the table |
| `relay-day.json` | §16.2 | the typical day as a script |
| `relay-standby.json` | §15.2 | the standby's differences |

Wycheproof `ed25519_test.json` and `seal`'s `seal-signatures.json`, `seal-frame.json` and `seal-chain.json` are run as they are, through append and login (RELAY-GEN-010).

## 22. Interfaces

**`seal.md`.**
- Requires: the frame layout and signature (SEAL-FRAME-001 to SEAL-FRAME-003), the login message (SEAL-SIG-001), the admin message and its registry of operation names and `args` layouts (SEAL-SIG-003, SEAL-SIG-004), the chain (SEAL-CHAIN-001), `board_id` and `dev_R` derivations (SEAL-KEY-005, SEAL-KEY-006), the ticket id (SEAL-PAIR-001), the slot layouts (`seal.md` §17.4), the host and origin forms (`seal.md` §5.4) and Crockford base32 (`seal.md` §5.2). It uses only registered context strings: `frame`, `login`, `admin`, `chain`, `board`, `recovery-dev`, `pair-tid`, `token`, `door`; it registers none.
- Requires: the clauses of `seal.md` that name `relay` as an owner, realised here as follows: SEAL-GEN-007 by RELAY-GEN-002; SEAL-GEN-008 by RELAY-GEN-003; SEAL-GEN-010 by RELAY-GEN-009 and RELAY-GEN-010; SEAL-CTX-001 by using only `ctx(...)` of the registry (§5, §9.3, §10.2); SEAL-ENC-005 by the login message of §10.3 with `RELAY_ORIGIN`; SEAL-KEY-005 and SEAL-KEY-006 by §10.2 and §12.12; SEAL-FRAME-001 to SEAL-FRAME-003 by §9.3 P1 and P7; SEAL-CHAIN-001 by RELAY-STORE-003; SEAL-SIG-001 by RELAY-LOGIN-003; SEAL-SIG-003 and SEAL-SIG-004 by RELAY-ADMIN-003 and §12.1 A1; SEAL-PAIR-001 by §6.2 step 7.
- Requires: SEAL-MEM-004 names `relay` as an owner, but a keyless relay can check only its frame-level part: `dev = dev_R` from the supplied `rsign_pk`, `ctr = 1`, the signature under `rsign_pk`, and `board_id` from `rsign_pk` and `salt` (§10.2). The epoch, the opening under `K_entry,1`, `based_on` and the Genesis item lie inside `body`, which the relay never reads (RELAY-GEN-004). I assume `seal.md` narrows the relay's share of SEAL-MEM-004 to those four checks (or drops `relay` from its owners); until then stage `relay` cites it only for them.
- I assume `seal.md` accepts, without a change to its bytes, these meanings this spec gives to its registry (and ask that its "Sent with" column say so): `register` and `recovery_register` may name an entry already stored, by `seq` in the unsigned `extra`, after a restore tail (§12.3, §12.12); `fence` carries its `Fence` frame in the same call or none (§12.6); `seed`'s per-device `last_ctr` is the author's counter at `first_seq − 1` and `blob_id` is informational (§12.7); `restore_tail`'s `chain_before` travels unsigned beside the arguments (§12.9); `mint_read_token` may be signed by the recovery identity with `dev = dev_R` (§12.11).
- I assume `seal.md` §17.10's `RegisterTicket{tid, slot A}` is `pair-open`, `FetchSlot` is `slot-get` with the door and `X-TD-Proof: ticket_key`, `PostSlot` is `slot-post`, and `DeleteTicket{tid}` is `pair-close`; that "what counts as a try" (`seal.md` §17.11) is §10.8 here; and that the bundle's `token` (`seal.md` §17.8) is the 96-character token of §5.1.
- I assume `seal.md` §17.11's "a second post of the same label is refused by the relay" reads as "a second post with other bytes": an identical repeat is answered 200 and stores nothing (RELAY-PAIR-007), so a lost answer does not abort a pairing, and the single-use property is unchanged.
- I assume the hosts carried in `vb8(host)` (`seal.md` §12.7, §16.2 `fence`, and the welcome and `state` messages here) are at most 255 bytes. `seal.md` §5.4 allows 253 bytes before the `:` plus a port of up to 6 bytes (259 in all), which a `vb8` cannot carry; the `fence` body cap of §12.1 (262,533) is computed for 259 and is therefore an upper bound. A Fence host longer than 255 bytes cannot be encoded and is refused by the encoder (`seal.md` should say so).
- I assume `seal.md` §13.6's Skip floor (`t ≥ purge_seq`) is what ruling S15 settles, or that the settled floor is computed from the log so that `engine.md` can bound compaction by it (§12.8 "Skip targets").
- Provides: the token, the door, the relay address and its parsing (§5); the token text that `seal.md` treats as opaque.

**`engine.md`.**
- Requires: the engine's model relay implements every clause owned by `engine` here (the sequencing outcomes, the read pages, the floors and push), and the faults of SYNC_PLAN.md §6.5 on top.
- Requires, of the engine's duties: the Skip floor of ruling S15 is computed from the log, and compaction sends `below_seq` no higher than that floor (or names every frame between the floor and `below_seq` as `keep_seq`) and no higher than the snapshot acknowledged by a device other than the snapshot's author, lists every control entry below `below_seq` as `keep_seq`, is signed by a device other than that author, and repeats daily while `more` is true (§12.8); purge appends the rotation with `purge_before`, then sends `purge`, and repeats daily until `done = purge_floor` (§12.10); after a purge the shared picture is re-uploaded under the new epoch (§13.3); restore tail sends the seq-mode `register` and `recovery_register` and the `revoke` and `reinstate` operations of the restored control entries in log order (§12.9); a revocation sends `revoke` before appending its `R`-shape entry, and retries that entry until it is sequenced (§12.4); the move appends the `Fence` with the `fence` operation, using the new relay's door (§10.9, §12.6); seeding sends the device set of the membership at the end of the tail with each author's counter before the tail (§12.7), and resumes a seed tail or restore tail after a lost answer from the `head` of `relay/not_next` (§4.2).
- Requires: the engine decides what `second`, `displaced` and close code 4001 mean (an alarm policy that tolerates one displacement after a crash and restart), what `truncated` triggers, and how it reconciles with `last_ctr`, `last_seq`, `gap` and `conflict` (SYNC.md §4.3 rules 3 and 4, V1-16), never trusting `at` or `relay_now` (RELAY-STORE-006).
- Requires: snapshot uploads use purpose 1 for periodic snapshots, 2 for the pairing bundle's, 3 for the genesis snapshot of an imported board, and 4 for the shared picture (§10.7).
- Provides: nothing to the relay; the relay reads no engine data.

**`device.md`.**
- Requires: the transport sends the constant User-Agent, protocol pings every 30 seconds, a session id per process (§11.3), uploads at no less than 64 KiB/s (§6.2), and polls `hello` and `read` on a relay with `push: false` and slot C and D while pairing as N; it keeps tokens opaque (RELAY-TOKEN-003), parses relay addresses (RELAY-ADDR-002), keeps the request budget (RELAY-CLIENT-001), classifies failures (RELAY-CLIENT-002), never deletes on a relay answer (RELAY-CLIENT-003) and opens no socket to the standby (RELAY-CLIENT-004).
- Requires: the device stores each board's relay address (host and door) with its token.
- Provides: nothing to the relay.

**Stage `recovery`.** Requires: the recovery tool takes host, board id and door from the sheet's relay address (§5.4), obtains nonces and sends recovery-signed operations with the door (§12.1), mints its own backup-read token as `dev_R` to top up a backup (§12.11), and fences the old board with a `Fence` frame and the `fence` operation over the door (§12.6).

**`core.md`, `bridge.md`.** None: the relay never reads core data, and the phone page never talks to the relay.

## 23. Public API

`relay-core.js` is an ES module with no imports other than `node:crypto`. Every function is synchronous unless marked `async`. Byte strings are `Uint8Array`.

```js
// ---------- constants (§10.1, §19, §20)
export const PROTOCOL = 1;
export const LIMITS;            // frozen object: one property per row of §20, e.g. LIMITS.maxBatch === 16
export const ROUTES;            // frozen array of the rows of §10.1: { name, method, pattern, creds, cap, fixed, object }
export const SQL;               // frozen object: SQL.schema (array of statements of §7.1) and every statement the core runs, by name
export class RelayError extends Error {
  constructor(status, code, extra);      // extra: { until?, index? }
  status; code; extra;
}

// ---------- encodings and credentials (§4, §5)
export function hex(bytes);                               // lowercase
export function unhex(text, width);                       // throws RelayError(400, 'relay/bad_body') on any other text
export function makeToken(tokenKey, role, board, dev, gen);        // -> 96-character string (§5.1)
export function checkToken(tokenKey, text, board);                 // -> { role, dev, gen } or throws 401 relay/unauthorized
export function door(tokenKey, board);                             // -> Uint8Array(5)
export function formatAddress(host, board, door);                  // -> canonical text (§5.4)
export function displayAddress(host, board, door);                 // -> display text
export function parseAddress(text);                                // -> { host, origin, board, door } or throws 'relay/address'
export function ctEqual(a, b);                                     // §3.3: lengths, then timingSafeEqual

// ---------- crypto (§3.4, seal.md §8, §15)
export async function verifyStrict(pk, msg, sig);                  // -> boolean
export function frameHash(frame);                                  // -> Uint8Array(32)
export function chainZero(board);                                  // -> C_0
export function chainStep(prev, seq, frameHash);                   // -> C_seq
export function loginMessage(origin, board, dev, gen, relayNonce, clientNonce);
export function adminMessage(op, board, signer, args, relayNonce);

// ---------- the gate (§6)
export function matchRoute(method, path, query);                   // -> { route, params, query } or throws
export function checkHeaders(route, headers);                      // steps 3 to 5; -> { contentLength }
export async function checkCredential(route, params, headers, config);   // steps 6 and 7; -> Cred
export function bodyDeadlineMs(contentLength);                     // 10,000 + ceil(len * 1000 / 65536)
export function gateAnswer(route, params, cred, config);           // caps, door; -> Reply or null
/** @typedef {{ cred: 'token'|'door'|'create', role?: 1|2, dev?: Uint8Array, gen?: number }} Cred */
/** @typedef {{ status: number, headers: Record<string,string>, body: Uint8Array }} Reply */
/** @typedef {{ tokenKey: Uint8Array, createSecret: Uint8Array|null, origin: string, push: boolean }} Config */

// ---------- the board (§7 to §16)
export class BoardCore {
  constructor({ storage, sockets, now, random, config, board });   // board: Uint8Array(16)
  migrate();                                                       // runs SQL.schema (RELAY-STORE-002)
  async handle(request);       // request: { route, params, query, cred, body, upgrade? } -> Reply, or { socket: true, reply }
  async onMessage(socket, data);                                   // a socket message (§11)
  onClose(socket, code);                                           // forgets nothing: state is in SQLite
}

/** The storage interface (§3.1). Rows map column names to number | string | Uint8Array | null.
 * interface Storage {
 *   all(sql: string, ...params): Array<Row>;          // a query
 *   run(sql: string, ...params): { changes: number }; // a statement
 *   transaction(fn: () => T): T;                       // synchronous; all or nothing; rethrows
 * }
 * The sockets interface (§11).
 * interface Sockets {
 *   accept(dev: Uint8Array, attachment: object): { response: any, socket: Socket };
 *   list(dev?: Uint8Array): Array<Socket>;             // every socket of the board, or of one device
 *   attachment(socket): object;                         // §11.2
 *   setAttachment(socket, attachment: object): void;
 *   send(socket, bytes: Uint8Array): void;              // throws when closed; the core ignores it
 *   close(socket, code: number, reason: string): void;
 * }
 */
export function sqliteStorage(db);        // over node:sqlite DatabaseSync (standby, tests)
export const noSockets;                   // Sockets with no socket (standby)
```

`worker.js`:

```js
import { DurableObject } from 'cloudflare:workers';
export default { async fetch(request, env, ctx) };      // the gate (§6), then env.BOARD.get(env.BOARD.idFromName(hex(board))).fetch(…)
export class Board extends DurableObject {
  constructor(ctx, env);                                 // blockConcurrencyWhile(migrate); setWebSocketAutoResponse (§11.5)
  async fetch(request);                                  // the internal request of §6.4
  async webSocketMessage(ws, message);
  async webSocketClose(ws, code, reason, wasClean);
  async webSocketError(ws, error);
}
// env: TOKEN_KEY, CREATE_SECRET, RELAY_ORIGIN, BOARD (§17.1)
```

`standby.js`:

```js
export async function startStandby({ tokenKey, createSecret, origin, dataDir, listen, tlsCert, tlsKey, now });
  // -> { url: string, close: () => Promise<void> }; `now` is for tests only and defaults to Date.now
// `node standby.js` reads TD_TOKEN_KEY, TD_CREATE_SECRET, TD_RELAY_ORIGIN, TD_DATA_DIR, TD_LISTEN, TD_TLS_CERT, TD_TLS_KEY (§17.2)
```

Other files of `relay/`: `wrangler.toml` (§17.1), `package.json` (no dependencies; `wrangler` pinned as the only devDependency; `"type": "module"`; `"test": "node --test test/"`), `test/` (§18, §21), `README.md`, and `deploy.sh` (stage `release`).

## 24. Open questions

Non-normative. Each states the default this spec takes.

1. **The door travels in `Authorization: Door …`**, and the create secret in `Authorization: Create …`, so every credential is in the one header the gate reads. Decided here (SYNC.md is silent; ruling S4 fixes only the door's value).
2. **Every request body is binary**, so the relay has no JSON parser for untrusted input and the strict-JSON rule of README §5.3 cannot be broken by `JSON.parse`'s duplicate keys. Decided here (SYNC.md is silent).
3. **Token text: `"TD1."` and base64url of 69 bytes with a full 32-byte MAC.** Decided here (SYNC.md §5.2 gives the fields, not the encoding).
4. **`gen` is per device, raised by every login and every revoke**, so one token per device is valid at a time; a second process that logs in displaces the first. Backup-read tokens have their own per-writer `gen`. Decided here (SYNC.md is silent).
5. **The request budget of 5,000 per client is kept by the client, not counted at the relay** (§14.3): counting at the relay costs a row per request and makes replays write. The relay enforces the 3,000-append budget in the row an append writes anyway. Decided here (SYNC.md §6.2 lists both budgets without saying where they are counted).
6. **The relay computes the chain for every append**: no extra row (it is a column of the `log` row), 32 bytes per row, one SHA-256 per frame. It gives the head chain to `hello`, pages, restore tail and seed checks. Decided here (SYNC.md §4.8 needs it only for restore tail).
7. **Seeding uses the create secret** and a `door` route on the new relay, because the board does not exist there and the moving device needs the new door before it appends its Fence. Decided here; ruling S4 names the create secret for `create` only, and this spec widens it to the create family (§6.3).
8. **`seed`'s `last_ctr` is the counter before the tail; `register` and `recovery_register` have a seq mode; `restore_tail` carries an unsigned `chain_before`; the recovery identity mints its own read token.** Decided here; the bytes of `seal.md` §16.2 are unchanged, and §22 asks `seal.md` to record the meanings.
9. **A try is a slot-B post** (§10.8), because the ticket proof is checked statelessly in the gate and a wrong proof never reaches the object. Decided here (SYNC.md says "5 tries" without saying of what).
10. **Blob purposes and retention** (§13): pairing snapshots kept an hour, the genesis snapshot until the first compaction, two other blobs per author. A `ResetTo` far in the past may name a picture no longer held; the device then shows no picture. Decided here (SYNC.md is silent).
11. **"Acknowledged by another device" is checked as "signed by a device other than the snapshot's author"**; the keep list cannot be checked. A compromised member could strand a device by omitting control entries, as it could harm the board in other ways until revoked. Decided here (SYNC.md §4.7 assumes the relay can tell).
12. **The 365-day age rule uses the relay's own `at`**, so a re-homed relay compacts nothing for its first 365 days. Decided here, as conservative.
13. **Purge deletes control entries below `purge_before` too**, and takes effect at once for reads while its deletions run in daily slices. Decided here (SYNC.md §4.7 says "truncates every log").
14. **`node:crypto` through `nodejs_compat`** for synchronous SHA-256 and HMAC inside `transactionSync`; "no runtime dependencies" is read as no npm package. Decided here.
15. **Counters under `wrangler dev`** may differ from production's; the gate measures them there, and the owner's rehearsal measures them on the real relay (SYNC.md §11, "Still to measure").
16. **Telling Cloudflare's quota from an outage** rests on `X-TD-Relay` and status 429 (Error 1027). A Durable Object over its daily request quota makes the object call throw and looks like an outage (`503 relay/object_unavailable`). Decided here; to be confirmed in the rehearsal.
17. **A fence is final** on that relay: v1 has no unfence. An abandoned move is finished by moving again. Decided here.
18. **`revoke` precedes its `R`-shape entry** (`seal.md` §16.2), so if the entry never lands the relay has locked the target out while the log still lists it; the engine retries the entry. Default kept.
19. **The second-socket notice can fire once after a crash and restart**, when the old connection is still open at the relay; the engine's alarm policy absorbs one (§22). Decided here.
20. **The standby can listen on plain HTTP** behind a TLS proxy (SYNC.md §6.7, Caddy) or on loopback for tests; clients still use `https://` origins. Decided here.
21. **Identical repeats succeed** for `create`, `register`, `recovery_register`, `blob-begin`, `blob-put`, `pair-open` and every slot post, so a lost answer costs only a retry; a different second request under the same id is still refused. Decided here (SYNC.md is silent on retries other than appends).
22. **A seed or restore tail resumes from `relay/not_next`**, whose error body carries the relay head, because a seeding board answers no `hello`. Decided here (SYNC.md is silent).
23. **The seeded device set is the membership at the end of the tail**, with counters from before it (§12.7). Decided here (SYNC.md §6.7 says only "the device set").
24. **A socket that has not said hello is never closed by another socket's hello** (§11.3), so a second process is always reported by `second` and `displaced` whichever process speaks first. Decided here.
25. **Signature checks run before the one transaction of a request and their results are applied in the section's order** (§8.4), so a refused login or admin request commits only its nonce deletion in the same single transaction. Decided here.
