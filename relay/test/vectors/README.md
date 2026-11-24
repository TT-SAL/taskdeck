# The relay's test vectors

One JSON file per family (`docs/sync/spec/relay.md` §21), in the format of the specs' README §7:
`{"spec": "...", "cases": [...]}`. The files are generated from `cases-*.mjs`; change a case
there and run `node relay/test/vectors/build.mjs`.

A case has a `name`, the `clauses` it cites, the `targets` it runs on (`core`, `standby`,
`worker`), a `setup` and its `steps`. A case with `call` instead of `steps` calls one pure
function of the core (`args` in, `expect` or `error` out). A case with `script` is replayed by
`day.test.mjs` from its `counts`.

## Setup

| Field | Meaning |
|---|---|
| `keys` | name → `{"seed": hex32}`: an Ed25519 seed. A name that starts with `rec` is a recovery key: its `dev` on a board is `dev_R`. Any other key's `dev` is fixed by the harness from its public key. |
| `boards` | name → `{"salt", "recovery", "first"}`; the board id follows from them (`"id"` overrides it). The harness creates each board with one `create` unless `"create": false`. |
| `token_key`, `create_secret`, `origin`, `clock_ms` | the relay's configuration and clock; the harness's standard values when absent. `"create_secret": null` is a relay without one. |
| `state` | steps run before the case's own. |

On the Worker target every run draws its own board ids (the salt is mixed with a random value),
and the object's clock follows `clock_ms` through the harness Worker.

## Steps

- `{"clock_ms"?, "request": {...}, "expect": {...}, "save"?: {name: field}}`: one HTTPS request.
- `{"socket": name, "open": {"board", "cred"}, "expect": {"status"}}`, then
  `{"socket": name, "send"?: builder, "close"?: true, "expect": {"messages": [...], "text", "closed": code, "silent": true}}`.
- `{"fill_log": {"board", "cred", "author", "from_ctr", "count"}}`: that many new frames, sixteen to a batch.
- `{"storage_fault": "<statement prefix>" | null}`, `{"entropy_fault": true | false}`: core target only.

`request`: `route` (a name of §10.1) or `raw` (`{"method", "path"}`), `board`, `params`, `query`,
`cred` (`{"token": key, "gen"}`, `{"read_token": key, "gen"}`, `{"door": true | board}`,
`{"create": true | hex32}`, `{"raw": header value}`, `{}`), `proof`, `headers` (`null` removes
one, a list sends it more than once), `body` (a builder), `slow` (`{"declare", "send"}`).

## Builders

Expanded by the harness into bytes, never sent as JSON. `"$last.<field>"` and
`"$saved.<name>"` are fields of earlier JSON responses.

| Builder | Bytes |
|---|---|
| `{"$hex": "…"}`, `{"$text": "…"}`, `{"$cat": [...]}`, `{"$u8": n}`, `{"$u16": n}`, `{"$u32": n}`, `{"$u64": n \| hex8}` | literal bytes, concatenation, big-endian integers |
| `{"$fill": {"len", "seed"}}`, `{"$slice": {"of", "from", "to"}}`, `{"$hash": builder}` | filler `H(seed ‖ u32 0) ‖ H(seed ‖ u32 1) ‖ …`, a part of a value, SHA-256 |
| `{"$key": name}`, `{"$dev": name}`, `{"$dev_r": name}`, `{"$board": name}`, `{"$tid": ticket key}` | a public key, a dev, the `dev_R` of any key, a board id, a ticket id |
| `{"$frame": {"author", "ctr", "body_len", "fill", "sig"}}` | a frame as `seal.md` §8: body `ver 1 ‖ suite 1 ‖ epoch u32 1 ‖ 24 zero bytes ‖` filler, signed by `author`. `sig`: `good`, `flip`, `zero-s`, `high-s`. `magic`, `len_delta`, `ctr_hex`, `board_id`, `signer`, `tail` damage it. |
| `{"$batch": [frames]}`, `{"$frames": [frames]}` | `"TDA1" ‖ n ‖ (u32 len ‖ frame)…`; the same without the header |
| `{"$create": {"board", …}}` | the `create` body with the board's Genesis frame |
| `{"$login": {"dev", "gen", "nonce", …}}` | the `login` body, signed as `seal.md` §16.1 |
| `{"$admin": {"op", "signer", "nonce", "args", "extra", …}}` | the admin envelope, signed as `seal.md` §16.2; `args` are the registry's named fields |
| `{"$slot": {"label", "body_len", …}}`, `{"$blob_begin": {…}}`, `{"$part": {"size", "index"}}` | a pairing slot, the `blob-begin` body, a blob part |
| `{"$hello": {"session"}}`, `{"$sock_append": {"req", "batch"}}`, `{"$sock_read": {"req", "after", "limit"}}` | socket messages |
| `{"$chain": seq}`, `{"$chain_from": {"before", "first", "frames"}}` | the chain value the harness computes itself with SEAL-CHAIN-001 |
| `{"$token": {"key", "gen", "role"}}`, `{"$door": true}` | the token and door the harness computes from the token key |

## Expectations

`status`, `x_td_relay`, `object_called`, `json` (exact fields in order; `"$any"`, `"$push"`,
`{"$time": offset}` and builders match values), `error`, `body`, `results` (the append results;
the chain of every `new` and `dup` is checked), `page` (`head_seq`, `floor`, `purge_floor`, `more`,
`truncated`, `fenced`, `seqs`, `frames`), `rows_written` (exact), `rows_read_max`, `elapsed_ms`,
`storage` (`board`, `devices` by key name, `log` seqs, `count` per table, and partial rows of
`blobs`, `tickets`, `read_tokens`, `nonces`). Every response is also checked for
`Cache-Control: no-store`, `X-TD-Relay`, its `Content-Type` and the absence of CORS headers.
