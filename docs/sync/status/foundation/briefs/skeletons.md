# Brief: task 4, skeleton crates (stage foundation)

SYNC_PLAN.md §6.1 item 4: "Skeletons. Public types and traits from the specs compile in every crate. Bodies
may be `unimplemented!()` here and nowhere later. Include a toy state machine and a fake sealer so `engine`
can start without `core` and `seal`." The stage gate: "skeletons compile on Windows, macOS, Linux and for the
Android targets".

The crates exist as shells (task 2, `workspace.md`): manifests, dependencies, a crate-level doc comment, one
trivial test. This task gives them the public surface the specs fix, and gives `taskdeck-sync` a working toy
state machine and fake sealer. It adds no behaviour anywhere else.

Paths you may create or change: `crates/taskdeck-*/**`. Not the root `Cargo.toml`, not `Cargo.lock` (every
dependency is already declared; if one is missing, report it, do not add it), not the specs.

## Part A: the public surface

Each spec ends with a section "Public API" that says "the skeleton must compile with exactly these names".

| Crate | Section | What else to read |
|---|---|---|
| `crates/taskdeck-core` | `core.md` §20 | the sections §20 points to for the fields of each type |
| `crates/taskdeck-seal` | `seal.md` §26 | the same; §22 for the error codes |
| `crates/taskdeck-sync` | `engine.md` §37 | §4 to §7 for the types "already given"; `toy`, `fake` are part B; `sim` stays a declared, empty module |
| `crates/taskdeck-device`, its bin `taskdeck-devicectl` | `device.md` §25 | DEV-GEN-001 (where `unsafe` may ever stand), §11 (`test-trust`) |
| `crates/taskdeck-android` | `bridge.md` §21, the Rust part | §11 (features, JNI names), BRIDGE-GEN-001 |
| `crates/taskdeck-backup`, `crates/taskdeck-recover` | no spec names a public item of theirs | they stay shells; say so in their doc comment |

Rules:
- Find each "Public API" section with Grep and read it in full; open the rest of a spec only where the
  section points (a type "given in §7.3" takes its fields from §7.3's table). Do not read a spec in full.
- Every public item the section writes exists with exactly its path, name, generics, fields, variants and
  signature. A `pub use` the section writes resolves as written. Where the section abbreviates (`/* … */`,
  "as §x"), take the definition from the section it names; where two places disagree, do not choose:
  report it and follow the "Public API" section.
- Data is real, behaviour is not. Constants, limits, error-code strings and enum variants carry the spec's
  values. Every function and method body is exactly `unimplemented!()`, so that
  `grep -rn 'unimplemented!()' crates/<crate>` lists what a stage has left; no `todo!()`, no panics with
  other texts, no placeholder return values. Trait declarations have no bodies.
- Derives and impls: what the spec states (for example seal's secret types implement `ZeroizeOnDrop` and
  do not implement `Debug`, `Display`, `Clone` or `Serialize`); otherwise only what the signatures need.
- Each item gets a one-line doc comment naming the spec section or clause it comes from. Do not copy
  spec text.
- Crate roots: the attribute the spec states (`#![forbid(unsafe_code)]` in core, seal, sync;
  `#![deny(unsafe_code)]` in device and android, DEV-GEN-001 and BRIDGE-GEN-001; no `unsafe` yet).
- Cargo features exactly as the specs name them (seal's test-entropy feature if §26 names one; sync `sim`;
  device `test-trust`; android `backend-echo` and `backend-device`). No default feature set enables a test
  feature. **Lead ruling S32:** `taskdeck-android` declares `default = ["backend-device"]`; building with
  both backends or with neither stays a `compile_error!`; the echo flavour builds with
  `--no-default-features --features backend-echo`. So every workspace-wide cargo command works without
  feature arguments, and a build that forgets its flags gets the real backend, never the echo one.
- The one trivial test each shell has stays; add no test of behaviour in part A.
- `crates/taskdeck-android/ci.sh` (the hook `scripts/ci/stage.sh` runs; read that script's header): checks
  that the skeletons compile for the Android targets of `rust-toolchain.toml`: `cargo check --locked` of
  core, seal, sync, backup and device and of `taskdeck-android` with each backend, for each Android target.
  A crate with C code (ring, under rustls) needs the NDK's compiler: take it from `ANDROID_NDK_HOME`, else
  `ANDROID_NDK_LATEST_HOME`, else `ANDROID_NDK_ROOT`, and set `CC_<target>`, `AR_<target>` and the cargo
  linker variable from it. With no NDK: on CI (`CI=true`) that is a failure; elsewhere the hook checks the
  crates without C code, prints which checks it could not run, and exits 0. The hook runs on Linux only
  (`CI_OS`); on other systems it prints that and exits 0.

## Part B: the toy state machine and the fake sealer (`taskdeck-sync`, feature `sim`)

`engine.md` §5.3 (`ToyMachine`) and §6.3 (`FakeSealer`, `FakeMembership`), compiled only with the feature
`sim` (ENG-TOY-002). These are working code, tested, not `unimplemented!()`: stage `engine` starts from them.

- `ToyMachine` implements `StateMachine` exactly as §5.3 states (encode, parse, apply, reset, coalesce,
  rebind). Tests: every example and table row of §5.3; equal inputs give equal bytes.
- `FakeSealer` implements `Sealer`, and `FakeMembership` `MembershipView`, as §6.3 states: `seal.md` §5,
  §8 to §15 and §20 with §6.3's substitutions and nothing else changed. Read §6.3 first, then those
  sections of `seal.md` (find them with Grep; they are long: read each once). Every random byte comes from
  the seeded SplitMix64 generator in call order (ENG-FAKE-002).
- Tests for the fake: equal seeds and equal call sequences give equal bytes; the genesis helper; seal, then
  classify and open, for each kind of entry; each refusal and halt §6.3 keeps from `seal.md` reached at
  least once; a membership change of each kind (add, revoke with rotation, the recovery-key change).
  Write expected bytes as hex in the tests where the spec fixes a layout, so that stage `engine` can turn
  them into its vector files (ENG-TOY-001 and ENG-FAKE-001 are that stage's clauses).
- No iteration over a `HashMap` or `HashSet` may reach an output; no `usize` is encoded; nothing depends on
  the platform's endianness or pointer width. The code builds for `wasm32-unknown-unknown`.
- If something of §6.3 cannot be finished, it is `unimplemented!()` and it is listed at the top of the
  module under "Not yet" and in your report, with the `seal.md` section it lacks. No silent gap.

## Proofs (all with `--locked`)

`cargo test --workspace --exclude task_deck`; `cargo test -p taskdeck-sync --features sim`;
`cargo test -p taskdeck-android --no-default-features --features backend-echo`;
`cargo clippy --workspace --exclude task_deck --all-targets -- -D warnings`, and again for
`-p taskdeck-sync --features sim`, for `-p taskdeck-device --features test-trust`, for seal with its test
feature if it has one, and for the echo backend; `cargo check -p taskdeck-core -p taskdeck-sync
--target wasm32-unknown-unknown`, and `-p taskdeck-sync --features sim` for the same target;
`bash crates/taskdeck-android/ci.sh` with `CI_STAGE=android CI_OS=linux CI_WIDE=false`;
`bash scripts/tests/run.sh`; `bash scripts/ci/reads.sh`; and the root package as before:
`cargo build --locked --no-default-features --bin taskdeck-server`.

Report: per crate the number of public items written and of `unimplemented!()` bodies; every place where a
"Public API" section was ambiguous, abbreviated without a source, or contradicted another section (with
both places); what part B's "Not yet" list holds; what you could not build here.

## Lead rulings for task skeletons

(none yet)

## Already raised and handled

(none yet)
