# Brief: task 2, the workspace (stage foundation)

SYNC_PLAN.md §6.1 item 2: "Workspace. Keep the root package; add the crates in §4 as members with every
planned dependency declared at its pinned version, so later stages rarely touch `Cargo.lock`. Add `relay/`
and `android/` with a README each." §3.2: `Cargo.lock`, the root `Cargo.toml` and `ci.yml` are shared
files; later stages may add only one dependency per pull request, so what you declare now is what they get.
§3.4 pre-approves: the crates named in SYNC.md §2.3 and §5.1 at those versions; anything already in
`Cargo.lock`; `security-framework`; `webpki-roots` if `android-shell` chooses bundled roots; for tests and
tooling only `proptest`, `libfuzzer-sys`, `cargo-fuzz`. Nothing else may be added. If you believe a crate
outside that list is needed, do not add it: report it.

The skeleton types come in a later task, from the specs. This task creates the crates as shells that
compile, with their dependencies declared and resolved.

Paths you may create or change: the root `Cargo.toml` (the `[workspace]` table and a new
`[workspace.dependencies]` table only; do not touch `[package]`, `[features]`, `[dependencies]` or the
profiles), `Cargo.lock`, `crates/taskdeck-*/**`, `relay/README.md`, `android/README.md`, `tests/e2e/README.md`.

## Crates (SYNC_PLAN.md §4; all `publish = false`, edition 2024, version 0.1.0)
| Directory | Package | Kind | Depends on (workspace crates) | Third-party dependencies to declare |
|---|---|---|---|---|
| `crates/taskdeck-core` | `taskdeck-core` | lib | — | serde (derive), serde_json, chrono 0.4.45 (with `serde`, without `clock` and without default features if that compiles: the core must not be able to read the clock or the local zone); dev: proptest |
| `crates/taskdeck-seal` | `taskdeck-seal` | lib | — | chacha20poly1305 0.11.0, ed25519-dalek 3.0.0, x25519-dalek 3.0.0 (static secrets), hpke 0.15.1, hkdf 0.13.0, sha2 0.11.1, getrandom 0.4.3, zeroize, subtle 2.6, serde, serde_json, flate2 (already in the lockfile; snapshots and backups are deflated); dev: proptest |
| `crates/taskdeck-sync` | `taskdeck-sync` | lib | — (decision D6: generic over traits it defines itself) | sha2 0.11.1 and zeroize (the state hash and the identity export, `engine.md`), serde, serde_json; dev: proptest |
| `crates/taskdeck-device` | `taskdeck-device` | lib and the bin `taskdeck-devicectl` | core, seal, sync, backup | ureq 3.5.2, tungstenite 0.30.0, rustls 0.23.46 (ring provider only), rustls-platform-verifier 0.7.1, getrandom, zeroize, serde, serde_json, chrono, flate2, image (JPEG only), tempfile; Windows only: `windows` 0.62 (the version already in the lockfile) with the features DPAPI needs; macOS only: security-framework (the version the platform verifier already pulls, no second one); a cargo feature for test-only trust that is off by default and enables nothing yet (name it `test-trust`) |
| `crates/taskdeck-backup` | `taskdeck-backup` | lib | core, seal | serde, serde_json, flate2 |
| `crates/taskdeck-recover` | `taskdeck-recover` | bin (and lib if useful) | backup, seal, core | — |
| `crates/taskdeck-android` | `taskdeck-android` | cdylib named `taskdeck` (`libtaskdeck.so`) | device | jni 0.22.4 |

Rules:
- Put every third-party version in the root `[workspace.dependencies]` and refer to it with
  `workspace = true` in the crates. The versions named in SYNC.md §2.3 and §5.1 are exact pins (`=x.y.z`).
  For crates already in `Cargo.lock`, use the version the lockfile already has, so nothing existing moves.
- `zeroize`: SYNC.md names 1.9.2, which was published on 2026-11-17 and is inside the seven-day cooldown
  until 2026-11-24. Declare `zeroize = "1.9"` and keep the lockfile at the version it already holds
  (1.9.0); say so in a comment. The lead moves it once the cooldown has passed.
- TLS: rustls must use the `ring` provider only. No `aws-lc-rs`, `aws-lc-sys`, `openssl`, `native-tls` or
  `cmake`-built crate may enter the lockfile (the phone build cross-compiles; SYNC.md §5.1 "ring remains
  only under rustls"). Choose each crate's features to make that true (rustls, ureq, tungstenite and the
  platform verifier each have defaults to switch off) and prove it with `cargo tree -i`.
- One RustCrypto generation: after resolution `cargo tree -d` must show no duplicate of any cryptographic
  crate (chacha20, poly1305, aead, cipher, curve25519-dalek, ed25519, digest, crypto-common, sha2, hkdf,
  hmac, zeroize, subtle). Duplicates that exist today in unrelated crates (getrandom 0.2/0.3/0.4, syn,
  windows-sys) may remain; nothing new may be duplicated without you reporting it.
- Nothing already in `Cargo.lock` may change version. Check with a diff: only additions.
- Every NEW lockfile entry must have been published at least 7 days before 2026-11-21. Check with
  `bash scripts/cooldown.sh --base origin/day-planner` (read `scripts/cooldown.sh --help`). If the newest
  compatible version of a transitive crate is too young, pin an older one with
  `cargo update -p <crate> --precise <version>`. List every new entry with its publish date.
- `bash scripts/supply-chain.sh` must pass with the new lockfile (cargo-deny advisories, bans and sources;
  cargo-audit; the cooldown). Install the tools with `bash scripts/dev-setup.sh foundation` if missing.
- Each crate: a `Cargo.toml`, `src/lib.rs` (or `main.rs`) with a crate-level doc comment that names its
  spec and stage, `#![forbid(unsafe_code)]` in core, seal, sync, backup and recover (the device and android
  crates need FFI later; give them `#![deny(unsafe_op_in_unsafe_fn)]` and no `unsafe` yet), one trivial
  test so `cargo test -p` has something to run, and a `README.md` of a few lines: owner stage, spec, the
  CI hook convention (`ci.sh` in the crate directory is run by `scripts/ci/stage.sh`, if that script
  exists read its header and follow it). So that an unused dependency is still resolved and compiled,
  reference each declared dependency once in a private `const _: () = { … };` block or a `use … as _;`
  line marked "declared for the stage that fills this crate" (cargo warns about nothing either way; the
  point is that a wrong feature set fails to compile now, not in wave 1).
- `taskdeck-core` gets a `clippy.toml` in its directory that disallows what SYNC.md §4.5 and
  SYNC_PLAN.md §6.2 forbid there: `chrono::Local` and `chrono::offset::Local` as types; the methods
  `chrono::Utc::now`, `chrono::Local::now`, `std::time::SystemTime::now`, `std::time::Instant::now`; and
  the `std::fs`, `std::net`, `std::thread` and `std::env` entry points that clippy's
  `disallowed-methods` / `disallowed-types` can name. Prove the configuration bites with a scratch file
  you delete afterwards. `taskdeck-sync` gets the same list.
- Build proofs, all with `--locked`, in the `CARGO_TARGET_DIR` you are given:
  `cargo test --workspace --exclude task_deck`; `cargo clippy --workspace --exclude task_deck --exclude taskdeck-fixtures
  --all-targets -- -D warnings`; `cargo check -p taskdeck-core -p taskdeck-sync --target
  wasm32-unknown-unknown`; and, if an Android NDK is installed (it is not by default: report that, do not
  install it), `cargo check` of `taskdeck-android` for `aarch64-linux-android`. Check that crates without
  C dependencies (core, seal, sync, backup) do `cargo check --target aarch64-linux-android` without an
  NDK. Finally the root package still behaves as before: `cargo metadata` shows the root as the only
  default member; `cargo build --locked --no-default-features --bin taskdeck-server` builds.
- `relay/README.md`, `android/README.md`, `tests/e2e/README.md`: what will live there, the owning stage,
  the spec, the hook convention, in a few lines each. No code.

Report: the final dependency table with versions and features; every new lockfile entry with its publish
date; the duplicate check; the TLS-provider check; what you could not build here.


Before you fix the dependency lists, read the "Interfaces" and "Public API" sections of every spec present in
`docs/sync/spec/` (seal.md, core.md, relay.md, engine.md and any other): where a spec names a crate, a feature or
a cargo feature of its own crate (for example seal's `test-entropy`, device's `test-trust`), declare it, provided
it is on SYNC_PLAN.md §3.4's pre-approved list; report anything a spec asks for that is not.

## State after the implementation (2026-11-22)

Seven shell crates are members; the root package is still the only default member; 56 third-party entries
were added to `Cargo.lock` and nothing that existed moved. The lead ran every proof of this brief in the main
checkout.

## Lead rulings for task workspace

1. **A spec's clause wins over this brief's table.** Where a clause fixes a crate's dependencies or features
   (ENG-GEN-005, DEV-GEN-011, `bridge.md` §12.4, `core.md` §18) and the table above says otherwise, the
   clause holds. So `serde` and `serde_json` are development dependencies of `taskdeck-sync`, `getrandom`
   and `tempfile` of `taskdeck-device`, and the device's `image` has the features `jpeg` and `png`.
2. **`taskdeck-android` has a default backend** (ruling S32 in the status file): `default =
   ["backend-device"]`; both backends or neither is a `compile_error!` once the skeleton adds it; the echo
   flavour builds with `--no-default-features --features backend-echo`. Workspace-wide cargo commands and
   `scripts/ci/stage.sh android` then work without feature arguments.
3. **`taskdeck-recover` depends on `taskdeck-device`.** `device.md` §15.8: the tool's online steps use
   `taskdeck_device::recovery_online::RecoveryLink`. The dependency is declared now, so the stage that writes
   those steps touches no shared file. A change under `crates/taskdeck-device/` must then mark the area
   `recovery` as well: for that one rule, and only for it, this task may change `scripts/ci/changes.sh`, its
   row in `scripts/ci/README.md` and `scripts/tests/test-ci-changes.sh`. `bash scripts/ci/reads.sh` must pass.
4. **`taskdeck-seal` does not depend on `taskdeck-core`.** SEAL-ENC-003 (payloads in the canonical encoding
   of `core.md`) is met inside seal for seal's own payloads; the two crates are built side by side in wave 1.
5. **`webpki-roots` is not declared.** SYNC_PLAN.md §3.4 approves it only if `android-shell` chooses bundled
   roots; `Cargo.lock` already holds 1.0.12, so declaring it later moves nothing. (That DEV-GEN-011 leaves it
   out while DEV-NET-002 names it is a matter for the specs, follow-up 12.)
6. **`rand_core` for seal, if hpke needs it.** If the sealing functions of hpke 0.15.1 take a `rand_core`
   generator and hpke does not re-export the traits seal must implement for its `Entropy`, seal declares
   `rand_core` at the version `Cargo.lock` already holds for hpke. Check hpke's API in the registry source
   and decide; say which.
7. **No fuzz set-up here.** A stage that adds `crates/<crate>/fuzz` gives it its own `[workspace]` table,
   which is what cargo-fuzz writes; the lead records for those stages that such a directory has a lockfile
   of its own.
8. **Known and accepted:** `base64` 0.22 and 0.23 side by side (ureq 3.5.2 needs the newer one; not a
   cryptographic crate); the development-only duplicates under `proptest`; `openssl-probe`, pure Rust, under
   `rustls-native-certs`; the lockfile-only entries `borsh`, `borsh-derive` and `syn` 3 that are never
   compiled; `cargo build --workspace` on Windows would write `taskdeck.pdb` and `TaskDeck.pdb` into one
   directory (the default build is the root package alone).

## Already raised and handled (do not re-report)

- [lead finding] `taskdeck-android` had no default backend (fixed: `default = ["backend-device"]`, ruling 2).
- [lead finding] `taskdeck-recover` did not depend on `taskdeck-device` (fixed; a change under
  `crates/taskdeck-device/` or `crates/taskdeck-sync/` now marks the area `recovery`, because the device
  links the engine and the reads check follows the chain; accepted by the lead).
- [lead finding] `rand_core` for seal: hpke 0.15.1 re-exports it (`hpke::rand_core`), so nothing is declared;
  a type-check in seal's `const _` block proves a wrapper for `single_shot_seal_with_rng` compiles (ruling 6).
