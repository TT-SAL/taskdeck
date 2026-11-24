# The fake sealer and the toy: what to call (tasks A, A2)

For the engine's author and the simulator's author; no need to read `src/fake.rs` or `src/toy.rs`. Both are compiled only
with the feature `sim` (ENG-TOY-002). Vectors: `crates/taskdeck-sync/tests/vectors/engine-{splitmix,toy,fake}.json`.

## Randomness (ENG-FAKE-002)

`sim::SplitMix64 { pub state }`: `next()`, `below(n)` (= `next() mod n`, panics for `n = 0`), `chance(ppm)` (`below(10^6) <
ppm`). A fake owns one generator. Drawing `N` bytes takes `ceil(N/8)` outputs as big-endian bytes, the last cut. Only
`create`, `join`, `new_entry_id`, `seal`, `seal_snapshot` and the crate-private sealing helpers draw; `classify`, `open_*`,
`take_missed_wraps` and the rest never do. A wrap draws 32 bytes it throws away (the real sealer's ephemeral key).

## `fake` (engine.md §6.3)

| Item | What it does; draws |
|---|---|
| `pub struct FakeSealer` | implements `Sealer` (engine.md §6.1) with `Membership = FakeMembership`. Holds keys, roots, pending roots, the anchor (creator only), missed wraps, the control history, its supported rules, and per counter its own last sealing (entry, epoch, frame) for SEAL-FRAME-005 |
| `pub struct FakeMembership` (`Clone, Debug, Default, PartialEq, Eq`) | `seal.md` §13.1; fields private: read it through `Sealer::view`. `default()` is `M_0`, whose bytes parse |
| `pub struct FakeBundle { pub roots: Vec<(u32, [u8; 32])>, pub material: SnapshotMaterial, pub history: Vec<Vec<u8>>, pub added_seq: u64 }` | what a pairing hands over: roots, the snapshot material, its history records (`seal.md` §21.5 layout) |
| `pub fn create(seed: u64, name: &str) -> (FakeSealer, Vec<u8>, Vec<u8>)` | the board's genesis at `at_ms = 0`: (the fake, frame of seq 1, the toy's empty board, 16 bytes). Draws salt 16, recovery key 32, device key 32, `dev` 16, `R_1` 32, entry id 16, 2 × 32 (wraps, ascending recipient id), nonce 24 |
| `pub(crate) fn create_at(seed, name, at_ms: i64) -> (..)` | the same with another genesis time |
| `pub fn join(seed: u64, name: &str, bundle: FakeBundle) -> FakeSealer` | a new device: draws key 32, `dev` 16 (the generator continues from there); keeps the bundle's roots whose commitment the snapshot membership records, and its history records. `name` is not used (the `DeviceAdded` names it) |
| `pub(crate) fn join_keys(seed: u64) -> (Dev, Hash32, Hash32)` | `(dev, sign_pk, hpke_pk)` that `join(seed, ..)` will have; draws from its own copy only. Also a fresh key pair for a recovery-key change |
| `pub(crate) fn bundle(&self, material: SnapshotMaterial, added_seq: u64) -> FakeBundle` | every root this fake holds, the material, its history records up to `material.seq` |
| `pub(crate) fn history(&self) -> Vec<Vec<u8>>` | its control history records (`seal.md` §21.5 layout), in seq order: what the model runtime persists beside the identity (`device.md` §15.7). Draws nothing |
| `pub(crate) fn restore(identity: &[u8], history: Vec<Vec<u8>>, reseed: u64, supported_rules: &[u32]) -> Result<FakeSealer, SealerError>` | the fake of a new process (engine.md §31.4): keys, board id, anchor, roots and pending roots from `export_identity()` bytes, the given history, a generator seeded with `reseed` (never the old one: it would draw the same entry ids again), the build's rules. Missed wraps and the SEAL-FRAME-005 frames start empty (memory only). `seal/file_corrupt` for identity bytes that do not parse exactly (magic, version 1, exact length), keys that do not belong together, an anchor of another board, roots or pending roots not strictly ascending, a history record that does not parse or repeats a seq |
| `pub(crate) fn set_supported_rules(&mut self, rules: &[u32])` | the build's rules (default `[1]`); the rules halt fires for a version outside it. Versions above 1 allow `(1, 1)` and hash version 1 (ENG-FAKE-003) |
| `pub(crate) fn seal_as_recovery(&mut self, m: &FakeMembership, ctr: u64, entry: &PlainEntry) -> Result<Sealed, SealerError>` | seals `entry` as `m`'s current recovery identity, counter `ctr` = its `last_ctr + 1` (`view(m).recovery`, last record). `Body::Ctl(Rotate{revoke, reinstate, purge_before: None})`, `Fence`, `Skip` give shapes R, F, S; anything else is sealed and refused by receivers. Draws as `seal`; keeps no frame, no pending root (`keys_changed` false). `seal/bad_payload` without a current recovery record |
| `pub(crate) fn seal_recovery_key_change(&mut self, m, ctr, entry: &PlainEntry, new_key: (Hash32, Hash32)) -> Result<Sealed, SealerError>` | a K-shape entry by the recovery identity: `RecoveryKeyChange{sign_pk, hpke_pk}`, then `EpochRotated` wrapping to the current devices and the new key. `entry.body` unread. Draws root, wraps, nonce |
| `pub(crate) fn reinstate_wraps(&mut self, m: &FakeMembership, dev: &Dev) -> Result<(u32, Vec<[u8; 96]>), SealerError>` | the `reinstate` wraps (`seal.md` §16.2): `(first_epoch, wraps)` for epochs `first_epoch` (the revoking entry's rotation) to `m.epoch`, to `dev`'s `hpke_pk`; 32 bytes per wrap. `seal/ctl_target` unless a device revoked `dev` at or above `purge_seq`; `seal/missing_root`; `seal/bad_payload` over 256 |
| `pub(crate) fn plaintext_bytes(entry_id: &[u8; 16], kind: u8, at_ms: i64, based_on: &Head, payload: &[u8]) -> Option<Vec<u8>>` | the padded plaintext of `seal.md` §10.1, any field values; `None` over the cap. Draws nothing |
| `pub(crate) fn forge_frame(&self, dev: &Dev, sign_pk: &Hash32, ctr: u64, ver: u8, epoch: u32, root: Option<[u8; 32]>, plaintext: &[u8]) -> Vec<u8>` | a signed frame with every envelope field chosen (suite 1, zero nonce), AEAD under `root` or this fake's root of `epoch` (zeros if none). Draws nothing |

`Sealer` methods, as engine.md §6.1 states them, with these fake specifics: `membership_zero()` is `M_0` (`seal.md` §13.3),
`FakeMembership::default()`, the input for seq 1 (any membership works there). `seal` returns its stored frame (no draw,
`keys_changed` false) only when asked again under a counter for the same entry (id, `at_ms`, `based_on`, body, hence kind)
with `m.epoch` equal to the stored epoch (SEAL-FRAME-005); another entry, or a newer epoch, under a counter it sealed before
(a frame cleared for re-sealing, engine.md §11.3 step 3, §11.4) is sealed anew and replaces the stored one. That store is the
only `own_sealed`-related state the fake keeps; it is memory only (not in the identity), so after `restore` every counter
seals anew, and `classify` reads own frames only through `own_sealed`. A stored rotation whose pending root was discarded
comes back without it; accepting it gives the root by the own wrap. `seal` refuses `ctr = 0` (`seal/frame_format`), `based_on.seq = 0` off `C_0` or `at_ms` out of range
(`seal/bad_plaintext`), a missing root of `m.epoch` (`seal/missing_root`), a bad name (`seal/bad_payload`) or host
(`seal/host_format`). A rotation's root is pending under the entry id until `classify` accepts the entry (removed with
`keys_changed`) or `discard_pending`. `classify` never draws and never panics. Called again for a seq it classified before with the same inputs (a restart
above the checkpoint, a replay after a Skip or a `ResetTo`), in any order of seqs, it returns the same receipt but
`keys_changed` (false once nothing is new), keeps one history record per seq (replaced by the same bytes), and does not
fail for a pending root already gone or a root already known; a root forgotten by `forget_roots_below` stays
`seal/missing_root`. `take_missed_wraps(m, first, wraps)` opens
at once those whose commitment `m` records, keeps the rest for the rotations that set them. `seal_snapshot` puts the history
of `material.seq` in the body, uncompressed, and draws blob id 16 then nonce 24, after its refusals. `open_snapshot` with
`exact` recomputes the state hash from the body (engine.md §18.3). `sync_fingerprint` is `"<seq> <hex of x[0..6]>"`,
`move_code` the 10 hex characters of `mc` (typed input compared without spaces and hyphens, lowercased).
`export_identity` is `"FKID" ‖ u32 1 ‖ dev ‖ sk[32] ‖ sign_pk ‖ hpke_pk ‖ board_id ‖ u8 anchor (1: rsign_pk ‖ salt ‖ R_1) ‖
u32 n ‖ n × (u32 e ‖ R_e) ‖ u32 n ‖ n × (entry_id ‖ u32 e ‖ root)`, roots by epoch and pending roots by entry id, ascending
(vector case `identity`); the history is not in it (`history()`).

## Recipes

- **A board.** `let (mut a, f1, board) = FakeSealer::create(seed, "Desk");` classify `f1` at seq 1 with
  `a.membership_zero()` (any membership gives the same: seq 1 is classified from `M_0` and the anchor), `prev_chain = a.chain_zero()`; keep `receipt.membership` as `M_1`.
- **A second device.** `let (dev, sign_pk, hpke_pk) = FakeSealer::join_keys(seed2);` E seals
  `Body::Ctl(CtlIntent::AddDevice { dev, sign_pk, hpke_pk, name, transcript, bootstrap_seq: h })` with `based_on` the head
  `h` of its snapshot material at `h`; once it is sequenced at `s`: `let b = FakeSealer::join(seed2, "Laptop",
  a.bundle(material_h, s));` b starts from `membership_from_bytes(&material_h.membership)` and classifies from `h + 1`
  (`retained_from = h`).
- **A recovery-signed entry.** `a.seal_as_recovery(&m, rec.last_ctr + 1, &entry)` with `rec` the last of `view(&m).recovery`;
  a key change: `a.seal_recovery_key_change(&m, ctr, &entry, (spk, hpk))` with `(_, spk, hpk) = join_keys(fresh_seed)`.
  The new identity's first counter is 1; the old one is refused from the next seq (`seal/revoked_author`).
- **A restart (§31.4).** Persist `export_identity()` at every `WriteIdentity` and `history()` with it; the new process
  runs `FakeSealer::restore(&id, history, fresh_seed, &rules)`, then the engine classifies the tail above its checkpoint
  again (same receipts; an own rotation still pending in `id` loses its pending root as it is accepted).
- **Reinstating a device revoked by a device.** `let (first, wraps) = a.reinstate_wraps(&m, &dev)?;` (the relay's
  `reinstate`), then the recovery identity seals `Rotate { reinstate: vec![dev], .. }`; the target calls
  `take_missed_wraps(&its_m, first, &wraps)` and classifies the rotations in seq order.

**A frame that fails each step of `seal.md` §14.2** (the class and code in brackets; the vectors' `receipt` cases do each):
1 cut a frame below 670 bytes, change `magic` or the `len` field at offset 44 (`Skip`, `frame_format`);
2 a frame of another board, `create(other_seed, ..)` (`Skip`, `wrong_board`);
3 a frame by a device before its `DeviceAdded` is classified (`Skip`, `unknown_author`);
4 flip the last byte (`Skip`, `bad_signature`);
5 classify an accepted frame again at the next seq, or `seal` with counter + 2 (`Tamper`, `counter_replay` / `counter_gap`);
6 a frame under this device's `dev` whose hash `own_sealed` does not return (flag `own_dev_foreign`; e.g. two fakes joined
with the same seed);
7 a frame sealed by a revoked device with its pre-revocation membership (`Refuse`, `revoked_author`);
8 `forge_frame(.., ver = 2, ..)` (`bad_version`);
9 `forge_frame` with `epoch` above `E` (`bad_epoch`); `seal` with a membership two rotations old (`old_epoch`); one
rotation old, after more than 30 days of log clock (`stale_epoch`, before any decryption);
10 `forge_frame` with a wrong `root` (`aead`);
11 `plaintext_bytes`, then a nonzero padding byte or `kind` 5 (`bad_plaintext`);
12 `based_on.seq >= s` (`Tamper`, `based_on_future`), a wrong chain (`based_on_fork`), `b < retained_from` (flag
`based_on_unchecked`);
13 a `cmd` by the recovery identity (`not_authorized`); a `ctl` sealed under `E − 1` (`stale_epoch`);
14 a `cmd` under `E − 1` with `based_on.seq >= epoch_seq` (`stale_epoch`);
15 kind 3 with payload `[0]`, or a `ctl` payload that does not parse (`bad_payload`);
16 a `Fence` with `home_gen` not `+1` (`ctl_home_gen`), a `Skip` of a control seq (`ctl_target`), a `RulesUpgrade` to
the active version (`ctl_target`); a `Skip` with `skip_targets` returning `None` (`NeedFrames`);
17 any valid frame (`Accept`); a rotation whose wrap to this device does not open (`Accept` with `Halt::Commitment`).
A root that this fake lacks, or that differs from its commitment, is `Err(seal/missing_root)` (`forget_roots_below`).

## `toy` (engine.md §5.3)

| Item | What it does |
|---|---|
| `pub struct ToyMachine`; `pub fn new(rules2: bool) -> ToyMachine` | implements `StateMachine` (§5.1) with `State = ToyState`, `Command = ToyCommand`, `Notice = ToyNotice`; supported rules `[1]`, or `[1, 2]` with `rules2`; hash versions `[1]` |
| `pub enum ToyCommand { Set{key: u8, value: i64}, Add{key, delta: i64}, Del{key}, Create{id: u64, name: u32}, Name{id, name}, Forget{id}, Fail{code: u8}, Panic, Mul{key, factor: i64} }` | ops 1 to 9; `Mul` only under rules 2 (`apply`'s check; `parse` accepts it) |
| `pub struct ToyState { pub rules: u32, pub keys: BTreeMap<u8, (i64, u64, u16)>, pub items: BTreeMap<u64, u32>, pub marks: BTreeMap<u16, u64> }` | keys: `(value, stamp_seq, stamp_dev)`; marks: highest item number minted per device. Empty state: `rules 1`, nothing else, 16 bytes `"TOY1" 00000001 0000 00000000 0000` |
| `pub struct ToyNotice { pub seq: u64, pub author: u16, pub other: u16, pub other_seq: u64, pub key: u8 }` | a `Set` over another device's newer value; 21 bytes; concerns `author` and `other` |

An item id is `device_no × 2^40 + n`. Codes: `engine/toy_format`, `toy_parse`, `toy_bad_ctx`, `toy_rules`, `toy_gone`,
`toy_overflow`, `toy_bad_id`, `toy_cap`, `toy_fail`, `toy_unknown_rules`; `Panic` panics. Nothing in the toy is random.

**The toy's origin state** (the `origin_state` of `Enqueue`, `Resubmit` and `rebind`) is the minter `u64 next_n`, 8 bytes
big-endian; `rebind` mints `n = max(next_n, mark of to_device_no in view + 1)` and returns `next_n = n + 1`. engine.md does
not state the initial value for a device with no `cmd` record; `next_n = 1` (`0000000000000001`) is the natural one, since
`rebind` raises it above the view's mark anyway.
