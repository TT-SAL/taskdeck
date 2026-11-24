# The model relay: what to call (task C)

For the simulator's author; no need to read `src/sim/relay/`. Module `taskdeck_sync::sim::relay` (feature `sim`), written
from `relay.md` (engine.md §31.3). Conformance: `tests/relay_conformance.rs` (its header lists the families and steps that
apply). One `ModelRelay` is the object of **one board on one relay**: a second relay (a move) is a second `ModelRelay` of the
same board id. No clock, no randomness, `BTreeMap`s only: equal calls give equal answers.

## Types

| Item | What it is |
|---|---|
| `ModelRelay` (`Clone, Debug, PartialEq, Eq`) | tables `board`, `devices`, `log` (with its `(dev, ctr)` index), `blobs`, `parts`, and the sockets |
| `Cred::{Token { dev, generation }, Door, Create}` | the credential, as the gate checked it: a role-1 token (its row is checked, §8.2), the door, the create secret |
| `BoardState::{Open, Fenced, Seeding}` | §8.1; an empty board is `None` |
| `BoardRow`, `DeviceRow`, `LogRow`, `BlobRow` | the rows of §7.1 (pub fields; `DeviceRow.generation` is the column `gen`; `wraps: Option<(u32, Vec<[u8; 96]>)>`) |
| `Reply<T> { result: Result<T, RelayRefusal>, last: Option<(u64, u64)>, token_gen: Option<u32>, out: Vec<Out> }` | every HTTPS-like call's answer: `last` = `last_ctr`, `last_seq` of a `409 relay/gap` or `relay/conflict` refusing a register's or fence's frame (§4.2); `token_gen` = the generation of a token the call issued; `out` = socket messages, in send order |
| `Out { socket: SocketId, dev, session: Option<[u8; 16]>, msg: SockMsg }` | one message for one socket (`session` is `None` before hello) |
| `SockMsg::{Welcome(Welcome), Result { req, results }, Page { req, page }, Push(Page), Second(session), Displaced(session), Error { req, refusal }, State { host, door, home_gen }, Close(code)}` | §11.3 messages; `Close` means the relay closed it (4001, 4002, 4003, 4004) and the socket is gone |
| `SocketId = u64` | from `socket_open`, unique per `ModelRelay` |
| `AdminOp` | an admin operation with every argument the relay reads (`Register { dev, sign_pk, frame_hash, frame: Option<Vec<u8>>, seq }`, `Revoke`, `Reinstate { dev, first_epoch, wraps }`, `Fence`, `Seed`, `Compact`, `RestoreTail { first_seq, hashes, chain_before, frames }`, `Purge`, `RecoveryRegister { dev_r, sign_pk, frame_hash, seq }`) |
| `Login { generation, wraps }` | a login's answer |

Refusals are `RelayRefusal { status, code, until, head, index }` with the status of `relay.md` §19; `until` for
`relay/budget` (of a carried frame) and `relay/delete_budget`, `head` for `relay/not_next`, `index` for a tail's frame.

## Calls (every `now` is that relay's clock: the virtual clock plus the relay's offset)

| Call | Returns |
|---|---|
| `ModelRelay::new(board_id, relay_key: [u8; 32])` | an empty object; `relay_key` stands for `TOKEN_KEY` and only derives the door |
| `door() -> [u8; 5]` | `H("FAKE-DOOR" ‖ relay_key ‖ board_id)[0..5]` |
| `create(now, rsign_pk, salt, dev, sign_pk, frame_1) -> Reply<AdminOk>` | §10.2 with the fake derivations (`fake_board_id`, `fake_dev_r`) and the fake signature; `seq`, `chain`, `door`, `token: true`, `token_gen` 1 (a repeat raises it) |
| `login(now, cred, dev) -> Reply<Login>` | §10.3 without nonce and signature: token of `dev` (any generation) or door; generation + 1, the stored wraps; `out` closes `dev`'s sockets of a lower generation with 4003 |
| `hello(now, cred) -> Reply<Welcome>` | §10.4 (`last_ctr`, `last_seq`, `appends_left` set; `push` true; the door is `door()`) |
| `read(now, cred, after, limit) -> Reply<Page>` | §10.5; `limit` 1 to 256 and `after` ≤ 2^53 − 1, else `400 relay/bad_query` |
| `append(now, cred, frames: &[Vec<u8>]) -> Reply<Vec<AppendResult>>` | §9 (token, or door for recovery frames); 1 to 16 frames else `400 relay/bad_batch`; `out` pushes the stored entries |
| `admin(now, cred, signer, &AdminRequest) -> Reply<AdminOk>` | the engine's request: `Door` (create secret) answers `door`; `SeedTail` is `seed_tail`; the rest become an `AdminOp` (`Register` with `frame_hash = H(frame)`, `RestoreTail` with `hashes = H(frame_i)`) |
| `op(now, cred, signer, &AdminOp) -> Reply<AdminOk>` | §12.1: token row, A1's counts, A2, A3, A5, then the operation (§12.3 to §12.10, §12.12). Nonces (A4) and the signature (A6) pass by construction |
| `seed_tail(now, cred, first, frames) -> Reply<AdminOk>` | §12.7 with the seeding device's token; `head` set; the board is open once the tail ends at `seed_last_seq` |
| `blob_begin(now, cred, id, purpose, seq, size, sha256) -> Reply<u32>` | §10.7 begin (and the lazy deletions of §13.2); the part count |
| `blob_put(now, cred, id, index, data) -> Reply<(u32, bool)>` | `(received, complete)`; completion runs §13.1 |
| `blob_meta(cred, id) -> Reply<BlobRow>`, `blob_list(cred) -> Reply<Vec<BlobRow>>`, `blob_get(cred, id, index) -> Reply<Vec<u8>>` | §10.7 |
| `blob(now, cred, &BlobRequest) -> Reply<BlobOk>` | the engine's request: `Upload` = begin, then every 1 MiB part in order; `Fetch` = parts `0..parts`; `List`, `Meta`. The first refusal ends it. `meta(&BlobRow) -> BlobMeta` converts a row |
| `socket_open(cred) -> Result<SocketId, RelayRefusal>` | §11.1: role-1 token, open or fenced board; not ready until hello |
| `socket_hello(now, id, session, features) -> Vec<Out>` | §11.3: for each other socket of the device that said hello: same session → `Close(4002)`; else `Displaced` + `Close(4001)` to it and `Second` to this one; then `Welcome`. A second hello → `Close(4004)` |
| `socket_append(now, id, req, frames) -> Vec<Out>` | §11.4: `Result` (or `Error`) on this socket first, then the pushes; a stale or revoked credential → `Error` + `Close(4003)`; before hello → `Close(4004)` |
| `socket_read(now, id, req, after, limit) -> Vec<Out>` | `Page` (or `Error`) on this socket |
| `socket_close(id)` | the client or the network closed it |

Pushes: every call that stores entries (append over HTTPS or socket, `register` with a frame, `fence` with a frame,
`restore_tail`, `inject_frame`) returns in `out` one `Push` per page of the new entries (256 entries, 1 MiB) for every
ready socket, the author's included; `fence` adds a `State` to every ready socket; `revoke` closes the target's sockets
with 4003. Deliver `out` in order (a dropped or delayed message is the simulator's fault to inject).

## Admin answers (`AdminOk`)

`Register` → `seq`, `chain` (of the `DeviceAdded`, or of the named entry in seq mode), `token: true`, `token_gen` 1 (a
repeat: stored + 1). `Revoke`, `Reinstate` → nothing set. `Fence` → `seq`, `chain` with a frame. `Seed` → `door`,
`token: true`, `token_gen` 1 (the signer's); the next seq is `first_seq`, the state `state()`. `Compact` → `deleted`,
`floor`, `more`. `RestoreTail`, `SeedTail` → `head`. `Purge` → `purge_floor`, `done`. `RecoveryRegister` → `seq`, `chain`.

## Recipes

- **A board.** `let (fake, f1, _) = FakeSealer::create(seed, "Desk");` gives frame 1 (author `dev_R`). The create request
  needs `rsign_pk`, `salt`, the first `dev` and `sign_pk`; the fake has no accessor for the first two, so read them from
  `fake.export_identity()` (api-fake-toy.md layout: `dev` at bytes 8..24, `sign_pk` 56..88, `board_id` 120..136, anchor
  flag 136, `rsign_pk` 137..169, `salt` 169..185). Then `ModelRelay::new(board_id, key).create(now, &rsign_pk, &salt,
  &dev, &sign_pk, &f1)`.
- **Register a device.** The engine's `Admin { Register { frame, dev, sign_pk } }` → `relay.admin(now, &Cred::Token { dev:
  e, generation }, &e, &req)`; the new device's token is `Cred::Token { dev, generation: reply.token_gen.unwrap() }`.
- **A session.** `socket_open(&cred)`, `socket_hello(now, id, session, 0)` → deliver the `Welcome` as `Event::Welcome`;
  `Second(s)` / `Displaced(s)` as `Event::Second` / `Event::Displaced`; `Push(p)` as `Event::Page { req: None, page: p }`.
- **Reinstate** (recovery-signed, not an engine request): `relay.op(now, &Cred::Door, &dev_r, &AdminOp::Reinstate { .. })`.
- **Rollback.** `let snap = relay.clone();` … `relay.rollback_to(&snap)` restores the tables and keeps the sockets.
  **Fork:** serve some devices from `relay.clone()`.
- **Inject.** `inject_frame(now, frame) -> Option<(Head, Vec<Out>)>` stores any bytes at the next seq (no check, no
  counter, out of the `(dev, ctr)` index) and pushes them.
- **Invariants (§31.7).** `board_row()`, `state()`, `head()`, `devices()`, `log()` (`BTreeMap<u64, LogRow>`), `blobs()`,
  `part(id, index)`, `part_count()`, `sockets()`, `same_tables(&other)`.

Fake helpers, written from engine.md §6.3 (not from `src/fake.rs`): `fake_sign_pk(sk)`, `fake_sign(pk, m)`,
`fake_signed_frame(pk, header_and_body)` (signs `ctx("frame") ‖ bytes`, appends the 64 bytes), `fake_board_id(rsign_pk,
salt)`, `fake_dev_r(board, rsign_pk)`, `status_of(code)` (the HTTP status of §19).
