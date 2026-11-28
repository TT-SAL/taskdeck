# F2: the sealed store and the test key stores

Workflow W1a, chain step 2 (after F1). Common rules: `README.md` beside this file. Task id `f2`.

## Goal

The store of synced mode exactly as `device.md` §5 to §8 lay it out, so that the engine's records
survive a restart in the spec's file formats: every `PersistOp` executed on disk with the write and
fsync order the spec states, and `Loaded` rebuilt from the data folder at open. Plus the key-store
interface and the two test key stores of `test-trust`. The crash-kill suite (`CrashFiles` killing at
every write boundary) is **not** this task; torn and bad tails at open are, because they are part of
the format.

## Paths

`crates/taskdeck-device/src/store.rs` and new files under `crates/taskdeck-device/src/store/`,
`crates/taskdeck-device/src/keystore/` (or the key-store items where the skeleton has them in
`lib.rs`), the `mod`/`pub use` lines of `lib.rs`, F1's in-process test module (to run it over this
store), `docs/sync/status/device/fit.md`.

## Read

- F1's `next` (in your prompt) and the code of `crates/taskdeck-device/src/adapt/`.
- `device.md` §5.1 and §5.2 (the data folder and the lock; §5.3 cloud folders and §5.4 modes only as
  far as `open` must refuse or report), §6 (layout and encodings: every subsection whose kind the
  engine's records or the identity need: 1, 2, 3, 4, 5, 7, 8, 9, 10, 11, 17, and §6.2, §6.3, §6.5),
  §7.1 to §7.5 (the file layer, the write procedures, executing a `Persist`, reading at open,
  retention), §8.3 (what the store adds around the engine's records), §12.1 and §12.6 (what a key
  store holds; the test stores), §16 (per-device settings, as far as `device.tdk` is needed now),
  §21 and §22 for the codes and limits you meet, §25 for the names (`FileLayer`, `OsFiles`,
  `KeyStore`, `FileKeyStore`, `MemoryKeyStore`, the kind constants).
- `engine.md` §7 (the records, `Loaded`, the persist operations, §7.12 what must be durable first,
  §7.13 retention) and §4.4 (durability).
- `seal.md` §19 (sealed local files: envelope, kinds, associated data, reading) and §6.6 (the
  storage key and file keys), and the matching items of §26 (`FileSealer`, `open_file`, `FileTail`,
  `StorageKey`).
- `docs/sync/status/engine.md` "What I would warn the device session about" (persistence).

## Build

1. The file layer of §7.1 (`FileLayer`, `OsFiles`: the write, fsync, replace and directory-sync
   steps on Linux, Windows and macOS; the Windows replace through the declared `windows` features).
2. The data folder and its lock (§5.1, §5.2).
3. Every sealed file the engine's records need, in the layout and encodings of §6 (record JSON of
   §6.5 where the spec says so), sealed with `taskdeck-seal`'s file functions under the storage key,
   with the file kinds of §25; the identity file (kind 7) holding what `SealSealer` exports.
4. `Persist` execution (§7.3): each `PersistOp` in order, with the syncs DEV-FILE-004 asks, returning
   only when durable; the store's own records around them (§8.3).
5. Open (§7.4): read every file back, handle torn and bad tails as §7.4 says (with the
   `OpenWarning`s of §25), build the `taskdeck_sync::Loaded` the engine starts from, and the
   `SealSealer` from the identity.
6. The key store: the `KeyStore` trait as the skeleton declares it, `FileKeyStore` (a directory) and
   `MemoryKeyStore`, both only with `test-trust` (§11.1 item 4, §12.6), wrapping `L_d` as §12.1
   states. `DpapiKeyStore` and `KeychainKeyStore` are not this task: leave them as they are.
7. Tests: every `PersistOp` through disk and back gives the same `Loaded`; a reopen after each kind
   of record; a torn tail and a bad tail of a log segment at open; a second open of a locked folder;
   F1's in-process flow run over this store with the restart reading the files back
   (`FileKeyStore` in a temporary directory).

## Done

The README's green commands. Commit and push. In `next`, say how the runtime (F3) opens a data folder
for a new board (what it writes at create, in which order) and for an existing one, and what of
§5.4's modes and §8.3 is still missing.
