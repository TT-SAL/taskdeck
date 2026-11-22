# Two-readers round 3: the follow-ups 6 to 13 of r1-followups.md

## seal.md

### TR3-FOLLOW-1 (follow-up 7)

**§7, paragraph "The ephemeral key"**, last sentence, new text:

> A test vector (§24, `seal-rotation.json`) gives those 32 bytes for each wrap, in that order.

**§24, table of families**: a new row after `seal-wrap.json`:

| File | Spec | Inputs | Expected outputs |
|---|---|---|---|
| `seal-rotation.json` | §7, §12.5 | one rotation's sealing call: board, author keys, `ctr`, the new epoch, its recipients' keys in ascending recipient-id order, and entropy in draw order (the bytes drawn for the root, then the 32 bytes `ikm` of each wrap in that order, then the frame's nonce) | the root; for each wrap, its `ikm`, the ephemeral public key `DeriveKeyPair(ikm)` yields and the 96-byte wrap, in item order; the `EpochRotated` item; the frame bytes; a case whose entropy is one byte short, with the error |

## engine.md

### TR3-FOLLOW-2 (follow-up 6)

**§28.2 "Purge history", "On every device", step 2**, new text:

> 2. when `log_floor < p`: `TruncateLogBelow(p, floor_chain)`, with `floor_chain` = `C_{p−1}` from the chain index when `p − 1 ≥ retained_from`, else `None` (§7.11), and `log_floor = p`; when `log_floor ≥ p` (the device bootstrapped between `p` and the purge rotation, lead ruling L27): no `TruncateLogBelow`, and `log_floor` and `floor_chain` stay as they are, so `log_floor` becomes the larger of the two; in both cases, drop hash records and `SnapRecord`s below `p`;

### TR3-FOLLOW-3 (follow-up 8)

`RestoredCopy` and `BasedOnUnchecked` each get what lead ruling T4 gave the continuity warning: an action, a stored flag in `Meta`, a facade call and a bridge call. The names: `UserAction::AckRestoredCopy` and `UserAction::AckBasedOnUnchecked`; `Meta.restored_copy` and `Meta.based_on_unchecked`; the facade calls `ack_restored_copy()` and `ack_based_on_unchecked()`; the bridge calls `ack_restored_copy` and `ack_based_on_unchecked`.

**§7.8 `Meta`**: two new rows after `continuity_unchecked`:

| Field | Type | Meaning |
|---|---|---|
| `restored_copy` | `Option<u32>` | the count of own entries matched by restored-copy catch-ups (§11.3 step 3), until the user acknowledges it |
| `based_on_unchecked` | u32 | the number of accepted entries whose `based_on` check was skipped (`seal.md` §14.8 step 3) since the user last acknowledged them (§13.1); 0 in the first `Meta` of a new replica |

**§11.3 "Reconcile on hello", step 3, last bullet**, new text:

>    - when the log reaches `q`: if `a` (now updated) is below `r`, the relay claimed counters it cannot produce: evidence `withheld`; otherwise `restored` is cleared, `meta.restored_copy` becomes `Some(n + m)`, with `m` the number of records this catch-up matched by `entry_id` and `n` the count it held (0 when `None`), saturating at `u32::MAX`, persisted in the next `WriteMeta`, and `next_ctr` uses `w = r`. While `restored_copy` is set, `status.warnings` carries `RestoredCopy { entries }` with its count. `UserAction::AckRestoredCopy` sets it to `None`, persisted in the next `WriteMeta`.

**§13.1 "What each receipt does"**, the row of the `based_on_unchecked` flag, new text (fourth cell changed, the others as they are):

| `based_on_unchecked` flag | — | — | none: at the board apply of an accepted entry with the flag, `meta.based_on_unchecked` rises by 1 (saturating at `u32::MAX`), persisted in the next `WriteMeta`; while it is above 0, `status.warnings` carries `BasedOnUnchecked { count }` with its value (V2-15); `UserAction::AckBasedOnUnchecked` sets it to 0, persisted in the next `WriteMeta` | — |

**§30.1 `Status`**, the row `warnings`, new text:

| Field | Type | Meaning |
|---|---|---|
| `warnings` | list of `RestoredCopy { entries }`, `ContinuityUnchecked { from }`, `BasedOnUnchecked { count }` | informational; each is shown while its `Meta` field is set (`restored_copy`, `continuity_unchecked`, `based_on_unchecked` above 0), until its acknowledgement (§30.4) |

**§30.4 "User actions"**, the enum, new text:

```rust
pub enum UserAction {
    AckBanner { id: (u64, u8) }, DismissNotice { seq: u64, index: u32 }, DismissAlarm { id: u64 },
    ConfirmClock, MoveCode { text: String },
    ConfirmRejoin, UseThisCopy, ExportUnsent, AckContinuity, AckRestoredCopy, AckBasedOnUnchecked,
}
```

**§30.4**, two new clauses after ENG-UI-003:

- **ENG-UI-004** (engine) The engine MUST show `RestoredCopy { entries }` in `status.warnings`, with `entries` the count `meta.restored_copy` holds, from the end of a restored-copy catch-up (§11.3 step 3) until `UserAction::AckRestoredCopy`, across restarts, and at no other time.
- **ENG-UI-005** (engine) The engine MUST show `BasedOnUnchecked { count }` in `status.warnings`, with `count` the value of `meta.based_on_unchecked`, while that value is above 0, across restarts, and MUST set it to 0 on `UserAction::AckBasedOnUnchecked`.

**§37 Public API**, the signatures that change:

```rust
pub struct Meta { pub attempt: u64, pub last_failed: Option<(u64, u64)>, pub build_id: String,
                  pub halt: Option<HaltRecord>, pub stops: Vec<StopRecord>, pub alarms: Vec<AlarmRecord>,
                  pub banners: Vec<Banner>, pub relay: RelayTarget, pub prev_session: Option<[u8; 16]>,
                  pub conflicts: u32, pub clock: Option<(i64, i64)>, pub last_beat_ms: i64,
                  pub duties: DutyState, pub ops: Vec<OpRecord>, pub move_tries: (u32, i64),
                  pub continuity_unchecked: Option<u64>, pub restored_copy: Option<u32>,
                  pub based_on_unchecked: u32 }
```

and `UserAction` as in §30.4 above. `StatusWarning` and `Status` do not change.

## relay.md

### TR3-FOLLOW-4 (follow-up 10)

**§8.2**, the row "token, role 1, on `login-nonce` and `login`", new text:

| Credential | Valid at the object when | Else |
|---|---|---|
| token, role 1, on `login-nonce` and `login` | a `devices` row with `dev` and `role = 1` exists; `state = 1` | as above, but the generation is not checked |

## device.md

### TR3-FOLLOW-5 (follow-up 8)

The names and the engine side are those of `engine.md` TR3-FOLLOW-3: `UserAction::AckRestoredCopy` and `UserAction::AckBasedOnUnchecked`; `Meta.restored_copy` and `Meta.based_on_unchecked`; the facade calls `ack_restored_copy()` and `ack_based_on_unchecked()`; the bridge calls `ack_restored_copy` and `ack_based_on_unchecked`.

**§17.7 "Notices, banners, alarms, the halted view"**, two new rows after `ack_continuity()`:

| Call | Screens | Engine input | Errors | Auth | Blocks | Platforms |
|---|---|---|---|---|---|---|
| `ack_restored_copy()` | D, A | `User(AckRestoredCopy)`: the user has read `RestoredCopy` (`engine.md` §11.3 step 3) | — | — | short | all |
| `ack_based_on_unchecked()` | D, A | `User(AckBasedOnUnchecked)`: the user has read `BasedOnUnchecked` (`engine.md` §13.1) | — | — | short | all |

**§19.2 "Protocol"**, the row of the commands without arguments, new text:

| `cmd` | Arguments | Facade call | Result |
|---|---|---|---|
| `confirm_clock`, `confirm_rejoin`, `use_this_copy`, `ack_continuity`, `ack_restored_copy`, `ack_based_on_unchecked` | | | |

**§25 Public API**, two new signatures after `ack_continuity`:

```rust
    pub fn ack_restored_copy(&self) -> Result<(), DeviceError>;
    pub fn ack_based_on_unchecked(&self) -> Result<(), DeviceError>;
```

### TR3-FOLLOW-6 (follow-up 12)

**§3, DEV-GEN-011**, new text:

- **DEV-GEN-011** (device) The crate's non-development dependencies MUST be exactly `taskdeck-core`, `taskdeck-seal`, `taskdeck-sync`, `taskdeck-backup`, `serde`, `serde_json`, `chrono`, `zeroize`, `subtle`, `sha2`, `base64`, `flate2`, `image` (features `jpeg` and `png` only), `ureq`, `tungstenite`, `rustls`, `rustls-platform-verifier`, `url`, `webpki-roots` 1.0.12 if and only if stage `android-shell` chooses bundled roots for an API level (SYNC_PLAN.md §3.4, §6.6 item 7; §10.1), and, per target, `windows` (Windows) and `security-framework` (macOS), at the workspace's pinned versions.

### TR3-FOLLOW-7 (follow-up 13)

**§4.1 "Where the code lives"**, a new row after `taskdeck-devicectl`:

| Crate | Owner | What it holds | Links |
|---|---|---|---|
| `taskdeck-recover` | `recovery` | the recovery tool (SYNC_PLAN.md §6.8): the backup reader, the replay and the online steps of §15.8 | backup, seal, core, and `taskdeck-device` with default features for `recovery_online::RecoveryLink` (§15.8, §25; lead ruling 3 of the workspace brief) |

**§15.8 "The recovery tool's online steps"**, first sentence, new text:

> `taskdeck-recover` (stage `recovery`) runs offline until these steps, which use `taskdeck_device::recovery_online::RecoveryLink` (§25), so the tool's crate depends on `taskdeck-device` (§4.1), over the transport of §10, with the relay address from the sheet (never from the backup, SEAL-BKP-004), the door as credential where no token exists, and the recovery identity (`dev_R`, the recovery signing key) as signer:

## bridge.md

### TR3-FOLLOW-8 (follow-up 8)

The names and the engine side are those of `engine.md` TR3-FOLLOW-3: `UserAction::AckRestoredCopy` and `UserAction::AckBasedOnUnchecked`; `Meta.restored_copy` and `Meta.based_on_unchecked`; the facade calls `ack_restored_copy()` and `ack_based_on_unchecked()`; the bridge calls `ack_restored_copy` and `ack_based_on_unchecked`.

**§11.4 "The `call` table"**, two new rows after `ack_continuity`:

| Name | Arguments | `value` | Facade (`device.md`) | Auth | Stage |
|---|---|---|---|---|---|
| `ack_restored_copy` | `{}` | `null` | `ack_restored_copy` | — | android |
| `ack_based_on_unchecked` | `{}` | `null` | `ack_based_on_unchecked` | — | android |

**§13.1 "What the phone does"**, two new rows after "Devices: continuity":

| Screen | Shows | Offers (the `call` used) | Needs |
|---|---|---|---|
| Devices: restored copy | `RestoredCopy { entries }` of `status().warnings`, with its count | Acknowledge (`ack_restored_copy`) | all |
| Devices: unchecked history | `BasedOnUnchecked { count }` of `status().warnings`, with its count | Acknowledge (`ack_based_on_unchecked`) | all |

**§21 Public API**: no signature changes (the `call` names are table rows; `PageFacade` stays as it is, BRIDGE-ROUTE-003).

### TR3-FOLLOW-9 (follow-up 9)

**§6.7**, first sentence, new text:

> The `echo` flavour builds `taskdeck-android` with `--no-default-features --features backend-echo` (so without the default `backend-device`, §11.1), and links no `taskdeck-device`.

(The rest of the sentence, "It exists so that …", stays.)

**§11.1 "Library and loading"**, first paragraph, new text:

> `crates/taskdeck-android` builds a `cdylib` named `taskdeck` (`libtaskdeck.so`) with `jni` 0.22.4. Features: `backend-echo` (`android-shell`; no `taskdeck-device`) and `backend-device` (`android`; links `taskdeck-device` with its default features, and so `taskdeck-core`, `taskdeck-seal`, `taskdeck-sync`, `taskdeck-backup`; no code of the root package, V3-17), with `default = ["backend-device"]` (ruling S32). A build has exactly one of them: building with both or with neither is a `compile_error!`. The Gradle `echo` flavour builds with `--no-default-features --features backend-echo`; the `full` flavour builds with the default features, and so with `backend-device`. A build that passes no feature arguments (a workspace-wide cargo command, `cargo test -p taskdeck-android`) gets `backend-device`, never `backend-echo`.

**§11.1**, a new clause after that paragraph:

- **BRIDGE-GEN-011** (android-shell) `crates/taskdeck-android` MUST declare `default = ["backend-device"]` and MUST fail to compile with both `backend-echo` and `backend-device` or with neither; the `echo` flavour MUST build it with `--no-default-features --features backend-echo`, and the `full` flavour MUST build it with the default features and MUST NOT pass `--no-default-features`.

**§12.1**, the paragraph "Build variants", new text:

> Build variants: `echoDebug` (`android-shell`'s tests, `--no-default-features --features backend-echo`), `echoTiming` and `fullTiming` (the timing harness, §16: built as release, with R8 and the optimised Rust profile, signed with the debug key, never published; each with its flavour's features), `fullDebug` (the default features, so `backend-device`, and `taskdeck-device` with `test-trust` for the end-to-end tests against a relay on loopback, reached through `adb reverse`), `fullRelease` (the default features, so `backend-device`, without `test-trust`; the only variant `release` builds and the only one a phone runs). `echoRelease` does not exist.

**§12.4 "Dependencies and their verification"**, the row `taskdeck-android`, new text:

| Where | Dependencies |
|---|---|
| `taskdeck-android` | `jni` 0.22.4, `serde`, `serde_json`, `base64`, `sha2`, `zeroize`; with `backend-device` (the default feature, §11.1) also `taskdeck-core`, `taskdeck-device`, `rustls-platform-verifier` (for its Android initialiser); all at the workspace's pins |

**§21 Public API**, the first lines of the block, new text (the two `compile_error!` lines unchanged, a comment above them):

```rust
// Cargo features (§11.1, ruling S32): default = ["backend-device"]; the echo flavour builds with
// --no-default-features --features backend-echo.
#[cfg(all(feature = "backend-echo", feature = "backend-device"))] compile_error!("choose one backend");
#[cfg(not(any(feature = "backend-echo", feature = "backend-device")))] compile_error!("choose a backend");
```

## README.md

### TR3-FOLLOW-10 (follow-up 11)

**§9 "Glossary"**, the row `notice`, new text:

| Term | Meaning | Defined in |
|---|---|---|
| notice | A persistent per-device message that is not a refusal, such as an overwrite notice. | `core.md`, `engine.md` |

## For the lead

1. **Counting `BasedOnUnchecked` once per entry** (TR3-FOLLOW-3). The change as written raises `meta.based_on_unchecked` at the board apply of each accepted entry that carries the flag. If the engine applies a seq to the board a second time after a restart (replaying from a base), the count could go up twice for that seq. My recommendation is to count an entry only when its board apply advances `B` past a seq the persisted checkpoint had not covered. I did not write that in, because §18 decides whether a replay re-runs the board apply, and I did not read §18.
2. **Several restored-copy catch-ups before an acknowledgement** (TR3-FOLLOW-3). As written, the counts add up, saturating at `u32::MAX`. The other choice is that the newest catch-up's count replaces the stored one. I recommend adding them, which is what is written, so that no matched entries drop out of the count before the user has seen it.
3. **`TrustRoots::Bundled` while `webpki-roots` is not declared** (TR3-FOLLOW-6; lead ruling 5 of the workspace brief). With the conditional dependency, DEV-NET-002's bundled path has nothing to link until `android-shell` makes its choice. I recommend that the stage which makes that choice adds `webpki-roots` and implements `Bundled` in the same change. I wrote no refusal for `Bundled` in the meantime.
