# Brief: `docs/sync/spec/relay.md` (owning stage: `relay`)

Scope (SYNC_PLAN.md §6.1 item 3): "every route and socket message, authentication, sequencing and
idempotency, caps, budgets, errors, the storage schema, compaction and purge, push and plain HTTPS polling
(SYNC.md §4.1, §4.3, §6)". The `relay` stage (§6.4) builds `relay-core.js` behind a storage interface, the
Worker and Durable Object, the Node standby and a conformance suite run against all three. Its gate asserts
rows written per operation, a typical day under 2% of every free-plan limit, refusal of unauthenticated,
oversized and slow requests before the Durable Object runs, replays refused by the object without a write,
and no runtime dependencies. `engine` builds its model relay from your spec; `device` builds the client.

## Sources
SYNC.md §2, §4.1, §4.3, §4.7, §4.8, §5.2, §5.3, §5.4, §5.5, §6 (all), §7 (the relay, DoS and theft rows),
§8 (the relay paragraph). docs/sync/research/R3 §2, §5, §6, §7, §8. V1 findings 1, 5, 10, 16. V2 findings
3, 9, 15, 21. SYNC_PLAN.md §2, §3.4, §5 (what is known about wrangler and node:sqlite), §6.4, §6.5, §6.7.
`docs/sync/spec/seal.md` (frame, login and admin signatures, pairing slots, the context-string registry,
which you extend only by asking: list what you need in "Interfaces").

## Must contain
1. **Shape and invariants.** One relay core behind a storage interface; the Worker (the gate, caps, routing;
   no HTML, no CORS headers, no logs, nothing outbound); the Durable Object (SQLite, hibernating sockets
   only, no timers, alarms or long-polls); the standby (Node 22, `node:sqlite`, plain HTTPS, no sockets).
   No runtime dependencies. The path prefix `/v1`; deploys are additive.
2. **The Worker gate** (ruling S4): the token's exact layout and text form (`"TD1."` …, role, board, dev,
   gen, MAC), the MAC input, the roles (`device`, `backup-read`), the door and the relay address (text form,
   grouping, case and look-alike handling), the create secret; constant-time comparison; which credential
   each route accepts; everything refused before any Durable Object call: no or bad credential, a body over
   the route's cap, a body that arrives too slowly. Make "too slowly" a number a test can drive.
3. **Every route**, one subsection each, in the eight families (create, login, socket, read, append,
   snapshots and blobs, pairing, admin): method, path, credential, request layout (binary layouts as byte
   tables, JSON as schema tables), response layout, every error, idempotency, the cap, rows written and
   rows read. Say which routes the standby lacks and what it answers.
4. **The socket**: upgrade, hello, every message in both directions with its layout (append batches,
   acknowledgements, pushed entries, the second-socket notice, errors), ping handling that does not wake the
   object, one live socket per device (V2-21), behaviour across hibernation and deploys.
5. **Sequencing and idempotency** (SYNC.md §4.3): batches of at most 16 frames; the signature checked
   first, strictly (the relay is never laxer than the clients, V1-16); one `transactionSync` per batch;
   `ctr = last_ctr + 1`; a lower `ctr` returns the stored seq if the hash matches, else `409 conflict` with
   the stored frame; a higher one gets `409 gap` with what the client needs; `dev` must be the authenticated
   device, except frames authored by the recovery identity (ruling S3); what an acknowledgement means; that
   a replay causes no write.
6. **Login**: the two steps, the 32-byte relay nonce and how it is kept without timers, the signed message
   (from `seal.md`), the token issued, generations, what revocation and rotation do to tokens at once.
7. **Admin operations**: register (with `DeviceAdded`), revoke, reinstate, fence, seed, compaction, restore
   tail, purge, minting and revoking backup-read tokens: arguments, the admin signature and nonce, who may
   sign (a member or the recovery identity), exactly what the relay can and cannot verify without keys.
8. **Storage schema**: every table, column, type, index and key (start from SYNC.md §4.1 and add what the
   routes need: nonces, budgets, the chain head, the door is not stored). A table of rows written and rows
   read per operation; the conformance suite asserts it.
9. **Read**: pages of at most 256 entries or 1 MiB, the cursor, what each entry carries, `relay_now`, the
   head, what a client is told when the log before its cursor was compacted.
10. **Snapshots and blobs**: ids, parts of at most 1 MiB, upload and download, the hash commitment, retention
    (the newest three snapshots plus each author's newest), what compaction may rely on.
11. **Pairing tickets and slots**: registering a ticket (by the existing device, with its token), posting
    and reading slots A to D (the new device comes in through the door), single use, 10 minutes checked
    lazily, 5 tries and what counts as a try, what the relay stores and never sees.
12. **Compaction and purge**: the relay cannot see kinds, acknowledgements or ages inside ciphertext, so say
    exactly what a signed request names (which seqs go, which stay because they are control entries) and
    what the relay checks itself; daily slices of at most 5,000 deletes; purge.
13. **Fence, seed and restore tail**: the fenced state (reads only); seeding a new relay with the device
    set, a snapshot and up to 30 days of tail including other devices' frames, each signature checked;
    restore tail accepting frames at their original seqs only with registered signatures and only if they
    extend the relay's chain; what the relay needs to store to check a chain without keys.
14. **Budgets and quotas**: per-client and per-device daily budgets (5,000 requests, 3,000 appends), how
    they are counted without timers, what a client sees at a budget or at the platform's quota, the
    typical-day table of SYNC.md §6.2 recomputed from your rows-per-operation table.
15. **Capabilities** (ruling S9): how a client learns whether this relay pushes.
16. Error catalogue (HTTP status plus a machine code for each), Limits, Test vectors (the conformance
    suite's fixture format: requests, expected responses and expected storage effects; it signs its own
    frames from `seal.md`'s layout), Interfaces, Public API (the exports of `relay-core.js`, the storage
    interface's methods, the Worker's and standby's entry points and configuration names), Open questions.

## Gaps to close (decide, and say so)
- Text encodings in headers and URLs (the token, ids in paths); binary or JSON for each body.
- What `gen` counts (per device, per board) and exactly when it moves.
- How the first device's keys and the recovery identity get registered at `create`, and what `create`
  stores as seq 1 (ruling S2).
- What the `at` column holds and that clients never trust it.
- How a client tells "the relay's budget" from "Cloudflare's quota page" from "unreachable".
- What the standby does differently, item by item, and that the conformance suite covers both.
- Whether the relay computes chain values for every append or only for restore tail, and the cost in rows.
