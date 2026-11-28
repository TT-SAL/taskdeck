# What W1a left for W1b (the "next" of each task)

## H, the harness

The commit is 59be7a0 on wt/h, to be merged into claude/optimistic-lovelace-6z2f55. It touches only tests/e2e/, the [[test]] stanza in crates/taskdeck-device/Cargo.toml and docs/sync/status/device/fit.md, which gains h-1 to h-3.

Timeouts and failure output:
- A Ctl request waits at most 30 s by default (CtlOptions.timeout). wait adds its own timeout_ms.
- A process end waits 10 s, the standby start 20 s, and an HTTP exchange 10 s.
- A failure prints the last 40 protocol lines and stderr lines with secrets redacted. These fields are redacted: create_secret, text, code, digits, sheet, seed, token, secret, address.
- Full device stderr is in <dir>/<name>/stderr.log.
- Dropping a Ctl or Standby during a panic prints their context.

The standby:
- Standby::start() gives url() = http://127.0.0.1:<port>, which is the relay target.
- origin() = https://127.0.0.1:<port> is what the standby signs into logins and what a device must derive from the host of a relay address.
- create_secret() and token_key() are never printed.
- It is plain HTTP with no push, so devices poll.
- It is stopped on drop (see h-2).

What Ctl assumes about the protocol (the real devicectl from F3 must match, or the helpers in tests/e2e/harness/ctl.rs change, not the tests):
- Result fields sit at the top level of the answer, beside id and ok.
- Field names are as follows:
  - board: canonical, state_hash
  - sheet: sheet, address, recovery_fp
  - pair_start: code, address
  - finish_create: address
  - clock: wall_ms
  - wait: the Changes fields (any, view and so on), sent as since and timeout_ms
- shutdown means an answer, then the exit event, then exit code 0.
- kill means no answer, and the process dies by abort.
- Request ids restart at 1 after restart().

Ctl::start_in(dir, name) makes <dir>/<name>/data and keys with --clock manual --zone utc --platform linux. Other options go through CtlOptions.extra_args, and restart_with changes options (for example to drop --crash-at). Any command without a typed helper is call_ok, call_err or try_call.

Scenario modules are added as mod lines in tests/e2e/main.rs; README.md has an example.

tests/e2e/ci.sh needs Node 22 or later on the runner; relay/ci.sh does the same lookup.

The test target compiles taskdeck-devicectl, whose main is still unimplemented!(). No test starts it, so F3 will start exercising it.

## F1, the adapters

For F2 (the store):

**Making a SealSealer from stored bytes**
- Call `taskdeck_device::SealSealer::open(board_id: [u8;16], identity: &[u8], history: &[Vec<u8>])`.
  - `board_id` comes from the identity directory or the file headers; kind 7 does not carry it (f1-7).
  - `identity` is the plaintext of identity.tdk (kind 7). It is exactly the bytes `export_identity()` gives and every `WriteIdentity` carries: "TDI1" ‖ vb16(80-byte keys) ‖ u16 n_roots ‖ n×(u32 epoch ‖ root[32]) ‖ u8 n_pending ‖ n×(entry_id[16] ‖ u32 epoch ‖ root[32]) ‖ u8 has_anchor ‖ (rsign_pk ‖ salt ‖ root1). Anything else is device/record_format.
  - `history` is the history.tdk records (kind 11, `HistoryRecord::to_bytes`), in any order, without a repeated seq.
- Before the first Start of a creating device, write the identity from `create()`'s `export_identity()` (as identity0 in flow_tests). `create()` takes `GenesisInput { recovery, salt, keys, root1, device_name, at_ms, board_hash }` and returns (sealer, frame_1) for `Bootstrap::Genesis { board, frame_1, relay }`.

**History records**
- After each Persist, write to history.tdk the records of `sealer.history()` (a Vec of (seq, bytes), ascending) whose seq is above history.tdk's last seq.
- The sealer keeps every record in memory (f1-15). It adds one for each accepted control entry it classifies and those of each snapshot it opens.

**Loaded fields the test store fills** (Store::loaded in crates/taskdeck-device/src/adapt/flow_tests.rs)
- `checkpoint`: the latest `WriteCheckpoint`.
- `bases`: (board_seq, board_chain) of each `WriteBase`, ascending.
- `chains`: (seq, chain, tau) of every log record.
- `log_floor`: max(1, the last `TruncateLogBelow` seq); `floor_chain` with it.
- `tail`: the log records above checkpoint.board_seq.
- `hashes`, `outbox` and `notices`: in append order, with the `Rewrite*` ops replacing them.
- `meta` and `marker`: as last written.
- `build_id` "build-1" and a fresh 16-byte session per Start.
- `bootstrap`: only while there is no checkpoint.

**Ordering the test runtime keeps, which F2 must keep**
- Apply each Persist's ops in order, then answer `Persisted` in emission order.
- Answer ReadLog/ReadBase from durable records only.

**Not yet done**
- A joining device's constructor from a pairing bundle. It must check every bundle root against the snapshot membership (SEAL-KEY-013).
- The recovery-identity sealing (K shape, recovery Fence).
- The seal-versus-core canonical JSON comparison test that seal.md §25 asks of device.

## F2, the store

For F3 (the runtime and facade). Paths are under crates/taskdeck-device/src/store/; the API is re-exported from crate::store.

**Opening an existing folder (Device::open)**
1. Take the lock first: `store::take_lock(&data_dir) -> Lock`. Keep the `Lock` value for as long as the facade lives.
2. Compute `cloud = store::is_cloud_folder(&data_dir)`. Skip it on Android.
3. Call `store::open_folder(files, &data_dir, &lock, cloud, &*key_store, build_id, now_ms)`. `files` is `OpenOptions.files`, or else `Arc::new(OsFiles)`. `now_ms` is the device or test clock.
4. `LocalOnly { pending_create }`: add `OpenWarning::PendingCreate` to the report when `pending_create` is set, and `lock.warning()` when the lock is not held.
5. `KeyLost { descriptor, reason }`: build the KeyLost view from the descriptor. It carries no device name (f2-12).
6. `Synced(opened)`:
   - Build the sealer with `opened.sealer()?`, then `Engine::new(CoreMachine::new(), sealer, Config {})` and `Start(opened.loaded)`.
   - Put `opened.warnings` into the report.
   - Keep `opened.store` (an `IdentityStore`) on the sync thread.
   - The facade's minter is `max(opened.store.minter(), mark + 1 from the view)` (§8.3).

**Running the engine on the store**
- On each `Effect::Persist { id, ops }`, call `store.persist(&ops, &engine.sealer().history())`.
  - Ok: deliver `Persisted { id }`, in the order the effects were emitted.
  - Err: deliver `PersistFailed { id }`. The store then refuses every later write until the next open.
- Answer `ReadLog` with `store.read_log(from, to)`.
- Answer `ReadBase` with `store.read_base(seq, now_ms)`. It returns `(LocalReadResult, Option<OpenWarning>)`; the warning means the base was quarantined.
- The store's own writes to `device.tdk` (tokens, the request budget) go through `store.write_settings(DeviceSettings)`. Fields other than the five the store owns live in `DeviceSettings::other` as raw JSON (f2-4); F3 should type `tokens` and `requests`, with the token in `Zeroizing`.

**Creating a board (§18.1, already in this order)**
1. Call `store::create_identity(files, &data_dir, &*key_store, &l_d, NewIdentity { descriptor: StoreDescriptor { board_id, dev, key_store: key_store.kind(), created_ms }, identity: &sealer.export_identity(), settings, bootstrap: &Bootstrap::Genesis { board, frame_1, relay }, genesis_blob })`. `settings` is `DeviceSettings::new(name)` with `history_complete = true` and `minter = Some(IdMinter::new(1))`. `pinned_recovery_*` still have to be added to it, through `other`. This call:
   - stores the key first (`KeyStore::create`);
   - creates `sync/` (with a dir-sync of the data folder) and the identity directory (with a dir-sync of `sync/`);
   - writes `store.tds`, `identity.tdk`, `device.tdk`, `genesis.tdb` (only when there is a genesis snapshot) and `bootstrap.tdk`, each by the replace procedure;
   - then writes `pending.tds`.
2. After the relay's create and the token are durable, call `store::activate(&*files, &data_dir, &board, &dev)`. It replaces `active.tds`, then removes `pending.tds`.
3. Reopen the folder as above. The first `Start` carries `Bootstrap::Genesis` read back from `bootstrap.tdk`. The flow test does exactly this.
- A join uses the same calls with `Bootstrap::Join { material, added_seq, relay }`.

**Still missing**
- §5.4 and the facade: `Device::open` itself; `TEST_TRUST_MARKER` passed to `black_box`; the KeyLost device name (f2-12); `forget_identity` and `abandon_create`, which remove an identity directory (§17.2, §18.1). `identities()` exists.
- §8.3 and the store's other files: none of `overlay.tdk`, `feed-*.tdk`, `picture.tdk`, `own-picture.tdk`, `weather.tdk`, `draft.tdk` or `unsent.tdk` is written or read. `genesis.tdb` is written and removed but never read (`resume_create` must read it). Typed `device.tdk` fields beyond the five the store owns are also missing. The history records, the floor, the minter before `RewriteOutbox` and the bootstrap removal are done.
- Not compiled here: `CrashFiles` (still `unimplemented!()`) and the crash suite; the Windows `MoveFileExW` path (no Windows target in this environment, so the first Windows CI run must compile `store::os_files::platform_windows`); and the macOS and Windows parts of the cloud check.
