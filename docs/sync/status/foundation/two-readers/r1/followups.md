# Two-readers round 1: changes for the round-1 follow-ups

Source: `docs/sync/status/foundation/resolutions/r1-followups.md` items 1 to 4 (item 5 needs nothing), with
lead rulings L1, L16 and L20 to L23 of `r1-lead.md`.

- Item 1 (the recovery tool's reinstatement steps, L22) is TR1-FOLLOW-1. It writes the steps and
  `RecoveryLink::reinstate` in device.md. It also makes the matching edits in relay.md (the recovery tool's
  `revoke` alone, its interface entry) and the L20 edits in seal.md and engine.md that the steps depend on:
  L20 dropped "or the epoch in force at `purge_seq`", and four places still say it (see "For the lead" 1).
- Item 2 (`TruncateLogBelow (seq, floor_chain)`) is TR1-FOLLOW-2, in engine.md only. Every use in device.md
  already has two arguments (§7.3 table, DEV-FILE-006) or names the op without arguments (§7.4, §7.6, §8.3).
- Item 3 (the order of `history.tdk` and `bootstrap.tdk`) is TR1-FOLLOW-3, in device.md.
- Item 4 (relay.md citations of SEAL-MEM-004 and SEAL-PAIR-001) is TR1-FOLLOW-4: nothing to change (shown
  under relay.md).

## seal.md

### TR1-FOLLOW-1 (item 1: the recovery tool's reinstatement steps; lead rulings L20, L22)

These edits do what L20 rules: they remove "or the epoch in force at `purge_seq`" and state that no
reinstatement is offered across a purge. The steps themselves are in device.md §15.8.

**§7, the paragraph "Wraps delivered outside an entry."** Replace the whole paragraph with:

**Wraps delivered outside an entry.** A device that the recovery identity reinstates (ruling S24) receives, outside the log, the wraps of the roots of the epochs it missed: the recovery identity signs them in the `reinstate` operation (§16.2), and the relay returns them at the device's login (`relay.md`; `device.md` §10.3). The device opens the wrap of epoch `e` by SEAL-WRAP-002, with `c_e` as its own membership records it, and only once its membership records `c_e`, oldest epoch first: the commitment of the first wrapped epoch is already in its membership (it classified the revoking rotation, which is sealed under an epoch it held; the recovery tool offers no reinstatement when the revoking entry lies below `purge_seq`, lead ruling L20, `device.md` §15.8); each later one enters it when the device classifies, in seq order, the rotation that set that epoch, which is sealed under the epoch before it. It uses a root only after it matched `c_e` (SEAL-KEY-013).

**§16.2, the registry row `reinstate`.** Replace the row with:

| `reinstate` | `dev[16] ‖ u32 first_epoch ‖ u16 n ‖ n × wrap[96]` | 22 + 96n | signed by the current recovery identity, before the `R`-shape entry that reinstates `dev`: the wraps (§7) of the roots of epochs `first_epoch` to `first_epoch + n − 1`, in order, each to the target's `hpke_pk`, with `1 ≤ n ≤ 256`; `first_epoch` is the epoch the rotation in the revoking entry set up, and no reinstatement is offered for a revoking entry below `purge_seq` (lead ruling L20, `device.md` §15.8); the last wrapped epoch is the one the reinstating entry is sealed under, whose own rotation wraps to the target as to every recipient (ruling S24, lead ruling L1); the relay stores the wraps and returns them at the target's login (`relay.md`) |

No clause, limit or interface entry changes: SEAL-SIG-004 and the limit "wraps per reinstatement, 1 to 256"
stand.

**§27, item 18.** Replace the item with:

18. **A device the recovery key reinstates reads what was sealed while it was revoked** (ruling S24, lead rulings L1 and L20). The wraps of the roots it missed travel in the recovery-signed `reinstate` operation (§16.2), outside the reinstating entry, which the device cannot open; the relay returns them at the device's login, and the device opens each by §7 once its membership records that epoch's commitment. More than 256 missed epochs: the recovery tool does not offer reinstatement, and the owner restores to a new board. A device whose revoking entry lies below `purge_seq` is not offered reinstatement either: it pairs again, or the owner restores to a new board. Decided there.

## core.md

No change.

## relay.md

### TR1-FOLLOW-1 (item 1: the recovery tool's reinstatement steps; lead ruling L22)

The recovery tool's undo (device.md §15.8 step 11) uses `revoke` alone, signed by the recovery identity,
which §12.4 already accepts. These edits name that case and add the reinstatement to the recovery stage's
interface entry. The shared part is the phrase "a device that `reinstate` returned to current but whose
reinstating entry was not sequenced or was refused by clients", which device.md §15.8 step 11 also uses.

**§12.4 `revoke`, the first paragraph** (it begins "`args = dev[16]`, the target."). Replace it with:

`args = dev[16]`, the target. Sent before the `R`-shape entry that revokes it (`seal.md` §16.2), so the target's next request fails at once (SYNC.md §5.4); or alone, for a device that `register` added but whose `DeviceAdded` clients refused (a rotation raced the pairing, `engine.md` §28.4), so that its row stops counting as current and its token is refused; or alone, by the recovery identity, for a device that `reinstate` returned to current but whose reinstating entry was not sequenced or was refused by clients (`device.md` §15.8 step 11), so that its row agrees with the membership again and its stored wraps are cleared. The relay cannot tell the cases apart and treats them alike.

The checks, effects, RELAY-REVOKE-001 to RELAY-REVOKE-005, the registry row of §12.1 (signer "a current
device other than the target, or the current recovery identity") and the error catalogue do not change.

**§22, the paragraph "Stage `recovery`."** Replace it with:

**Stage `recovery`.** Requires: the recovery tool takes host, board id and door from the sheet's relay address (§5.4), obtains nonces and sends recovery-signed operations with the door (§12.1), mints its own backup-read token as `dev_R` to top up a backup (§12.11), asking with epoch 1 and then with the `read_epoch` a `relay/stale_epoch` refusal carries (§4.2), so that it never raises the relay's read epoch; sends `recovery_register` for a `direct` recovery-key change only after the `K`-shape entry it appended is sequenced and classified `Accept` (§12.12); reinstates a device that a device revoked with `reinstate` (§12.5) before it appends, with the door (§9.3), the `R`-shape entry that reinstates it, and sends `revoke` alone (§12.4) for a device that `reinstate` returned to current but whose reinstating entry was not sequenced or was refused by clients (`device.md` §15.8); and fences the old board with a `Fence` frame and the `fence` operation over the door (§12.6), re-sealing that frame once with the `last_ctr` that a `409 relay/gap` or `409 relay/conflict` answer carries (§4.2, RELAY-ADMIN-005).

### TR1-FOLLOW-4 (item 4: relay.md citations of seal clauses whose relay duty moved)

No change. relay.md cites SEAL-PAIR-001 nowhere. It cites SEAL-MEM-004 once, in the §22 `seal.md` block,
third bullet ("SEAL-MEM-004 and SEAL-KEY-017 concern the encrypted body and are the clients' alone"), which
already says the relay has no duty there. The relay's part of the ticket id is cited as SEAL-PAIR-015 in the
first bullet ("the ticket id (SEAL-PAIR-015)") and in the second ("SEAL-PAIR-015 by §6.2 step 7"). §6.2
step 7 states the check itself and cites no id. No relay clause (RELAY-…) cites either old id.

## engine.md

### TR1-FOLLOW-1 (item 1: the recovery tool's reinstatement steps; lead ruling L20)

**§24.4, the paragraph "Reinstatement (ruling S24)."** Replace the whole paragraph with:

**Reinstatement (ruling S24).** The recovery tool reinstates a revoked device with the `reinstate` admin operation, whose `args` carry the wraps, to the device's `hpke_pk`, of the roots of epochs `first_epoch` to `E` (`seal.md` §16.2: `first_epoch` is the epoch the revoking entry's rotation set up; `E` is the epoch the reinstating entry is sealed under), and then with the `R`-shape entry whose `DeviceReinstated` names it and whose rotation to `E + 1` wraps to it like to every other recipient (`device.md` §15.8). The relay stores the wraps on the device's row and returns them in every `login` answer of that device until it is revoked again (`relay.md` §10.3 `wraps`, §12.5). The runtime logs in at every `Connect` while this device is revoked and sends `MissedRoots { first_epoch, wraps }` after each login whose answer carries them (`device.md` §10.3). While revoked, the device keeps its log and pauses classification at the first entry for which `classify` returns `seal/missing_root` (the paused seq, `K + 1`), without the local fault `engine/missing_root`. On `MissedRoots` while revoked, the engine calls `Sealer::take_missed_wraps(M_K, first_epoch, wraps)`; a `MissedRoots` with no wrap, more than 256 wraps or `first_epoch = 0` is `engine/runtime_protocol` and is ignored, and one that arrives while the device is not revoked is ignored (the relay returns the wraps until the next `revoke`). When the sealer reports a learned root, the engine persists the identity (D4), clears the revoked state (the stop `Revoked` and its banner action), and classifies and applies in seq order from the paused seq, from its log and then from the relay, before it seals or appends anything: the root of each later epoch is checked against the commitment of the rotation that set it as that rotation is classified (each rotation is sealed under the epoch before it; §6.1 `classify`), and the reinstating entry's own rotation gives `R_{E+1}`. A wrap that does not open, or a root that does not match its commitment, is the commitment halt (§16.1). A reinstated device does not pair again. If `M_K` still records this device as revoked when classification reaches the relay's head (a `reinstate` whose entry never landed), the device is revoked again with `source: log` and the revoking entry's seq and author, and pauses at the next `seal/missing_root`. The recovery tool does not offer reinstatement to a device whose revoking entry lies below `purge_seq` (lead ruling L20): such a device stays revoked and is offered re-pair and resubmit (§24.5), or the owner restores to a new board.

ENG-EPOCH-005 does not change.

**§38 (open questions), item 24.** Replace the item with:

24. **A reinstated device reads what was sealed while it was revoked** (§24.4; ruling S24, lead rulings L1 and L20): the wraps of the roots it missed reach it in its `login` answer, outside the reinstating entry it cannot open, and it resumes classification at the paused seq; it does not pair again. A device whose revoking entry lies below `purge_seq` is not offered reinstatement and pairs again. Decided here.

(§38 is engine.md's "Open questions"; the item begins "24. **A reinstated device reads what was sealed
while it was revoked**".)

### TR1-FOLLOW-2 (item 2: `TruncateLogBelow` takes `(seq, floor_chain)`)

Every use gives both arguments, as §7.11 defines them (`floor_chain` is `C_{seq−1}` when the engine holds
it, else `None`). The other places that name the op (§7.10, §7.11, ENG-PERSIST-005, ENG-SESS-005, D7, the
check "No deletion by the relay", the public API's `PersistOp::TruncateLogBelow(u64, Option<Hash32>)`) name
it without arguments or already have two; they do not change.

**§7.12, row D8.** Replace the row with:

| D8 | `WriteCheckpoint` with `board_seq ≥ x` | `TruncateLogBelow(y, floor_chain)` for any `y > x`, whatever its `floor_chain` |

**§19.7, step 4.** Replace the step with:

4. On the first candidate that passes: `TruncateLogBelow(S + 1, Some(C_S))` with `C_S` from the body (allowed by the user's confirmation, ENG-PERSIST-005), the confirmed state and `M_S` from the snapshot, `B = K = L = S`, `C_S` from the body, `log_floor = S + 1`, `retained_from = S`, a checkpoint and a base; then read from `S + 1`.

**§28.2, "On every device", step 2.** Replace the step with:

2. `TruncateLogBelow(p, floor_chain)`, with `floor_chain` = `C_{p−1}` from the chain index when `p − 1 ≥ retained_from`, else `None` (§7.11), and `log_floor = p`; drop hash records and `SnapRecord`s below `p`;

## device.md

### TR1-FOLLOW-1 (item 1: the recovery tool's reinstatement steps; lead rulings L1, L16, L20, L22, L23)

**§15.8.** After the paragraph that begins "Devices of the old board that follow the Fence" and before
DEV-BKP-011, insert:

**Reinstating a revoked device** (ruling S24; lead rulings L1, L20, L23), on the board the sheet names, once a top-up of a verified backup to the relay head `h` (steps 1 to 3, except that step 3 writes no folder) has given the membership `M_h`, its epoch `E = M_h.epoch`, and the roots of steps 2 and 3:

6. **Offer.** The tool offers to reinstate a device `dev` only when `M_h` records `dev` as revoked with `status_by` a device, not a recovery identity (`seal.md` §13.6, `DeviceReinstated`); its revoking entry, the entry at `dev`'s `status_seq`, is at or above `M_h.purge_seq` (lead ruling L20); `M_h` has fewer than 16 current devices; and `n = E − first_epoch + 1` is at most 256, where `first_epoch` is the `epoch` of the `EpochRotated` item of the revoking entry (`seal.md` §12.1 shape R). Otherwise it says why with `device/recovery_reinstate` and sends nothing: the device pairs again, or the owner restores to a new board (ruling S14).
7. **Missed roots.** For each epoch `e` from `first_epoch` to `E`, in order, the tool takes `R_e` from the backup's roots, or else unwraps it (`unwrap_root` with `Recipient::Recovery`) from the wrap to `dev_R` in the `EpochRotated{e}` item, read from the backup's verified history (`seal.md` §21.6) for a rotation at or below `b` and from the entries read after `b` otherwise. It checks `R_e` against `c_e` as `M_h` records it (`EpochRoot::commitment`, `seal.md` §6.5, §13.8), then wraps it to `dev` with `wrap_root(R_e, board, e, c_e, dev, hpke_pk)`, `hpke_pk` being `dev`'s key in `M_h` (SEAL-WRAP-001). A root it cannot get this way (a rotation wrapped only to an earlier recovery key) ends the reinstatement with `device/recovery_reinstate`, and the user may pick a newer backup. A wrap that does not open, or a root that differs from `c_e`, ends it with `seal/commitment`.
8. **`reinstate`.** `admin-nonce` with the door and `signer = dev_R`, then `reinstate` (`relay.md` §12.5) with `AdminOp::Reinstate { dev, first_epoch, wraps }` (`seal.md` §16.2), the `n` wraps of step 7 in epoch order, signed by the recovery key, with the door (`RecoveryLink::reinstate`). A refusal ends the reinstatement with the relay's code, shown to the user; the relay has changed nothing.
9. **The reinstating entry.** The tool draws `R_{E+1}` (SEAL-GEN-005) and computes `c_{E+1}` (`seal.md` §6.5). It then seals one `ctl` entry authored by the recovery identity under epoch `E` with `K_entry,E`: `based_on` the head `h`; `ctr = 1 +` the highest counter of an entry authored by `dev_R` in the backup's history and in the entries it read (1 when none); and the items `DeviceReinstated { dev }`, then `EpochRotated { epoch: E + 1, commit: c_{E+1}, wraps }`, with a wrap of `R_{E+1}` to every device current after the `DeviceReinstated` (`dev` included) and to the current recovery key (`seal.md` §12.1 shape R, §12.5). It sends the frame with `append` and the door (`relay.md` §9.3, RELAY-SEQ-009; `RecoveryLink::append`). On `gap`, it re-seals the entry once with `ctr = last_ctr + 1` and sends it again. `new` or `dup` gives the entry's seq `s` (step 10). `conflict`, a second `gap` or a refusal leads to step 11.
10. **Check.** The tool mints a read token again as step 1 states, because `reinstate` revoked every backup-read token (RELAY-REVOKE-004). It reads the entries after `h` up to `s` and classifies them in seq order from `M_h` with the replay of step 3. If it classifies the entry at `s` `Accept`, the reinstatement is done: the device receives the wraps at its next login and reads what it missed (`engine.md` §24.4). If it refuses that entry (an entry sequenced between `h` and `s`, such as another rotation, made it invalid, `seal.md` §13.8), step 11 follows.
11. **Undo.** The tool sends `revoke { dev }` (`relay.md` §12.4), signed by the recovery key, with the door (`RecoveryLink::revoke`), for a device that `reinstate` returned to current but whose reinstating entry was not sequenced or was refused by clients. This returns the device's row to revoked and clears its stored wraps (RELAY-REVOKE-005), so the relay agrees with the membership again. After the first undo, the tool reads to the relay head as step 10 does and starts again at step 6, with that head as `h`. After a second undo, or when `revoke` is refused, it ends with `device/recovery_reinstate`, giving the relay's code when `revoke` was refused. A device that already received the wraps is revoked again by its own classification (`engine.md` §24.4, lead ruling L23).

**§15.8, clauses.** After DEV-BKP-013, append:

- **DEV-BKP-014** (recovery) The recovery tool MUST offer to reinstate a device only when every condition of §15.8 step 6 holds in the membership at the relay head it verified, and MUST send no request for that device otherwise.
- **DEV-BKP-015** (recovery) The recovery tool MUST send `reinstate` with the wraps of the roots of epochs `first_epoch` to `E`, in epoch order, each root checked against its commitment before it is wrapped to the target's `hpke_pk`. It MUST send it before it appends the reinstating entry, which is sealed under `E` and whose rotation to `E + 1` wraps to the target.
- **DEV-BKP-016** (recovery) When the reinstating entry is not sequenced, or the tool's classification refuses it, the recovery tool MUST send `revoke` for the target before it ends or starts again, and MUST NOT start again more than once.

**§21, error catalogue.** After the row `device/recovery_truncated`, insert:

| `device/recovery_reinstate` | the recovery tool cannot reinstate this device: it was not revoked by a device, its revoking entry lies below the purge, 16 devices are current, more than 256 epochs were missed, a missed root cannot be obtained, or a second attempt failed | §15.8 | pair the device again, or restore to a new board; for a missing root, pick a newer backup |

**§22, limits.** After the row "recovery read-token mints", insert:

| wraps per reinstatement | 1 to 256 | wraps (epochs `first_epoch` to `E`) | DEV-BKP-014 |
| reinstatement attempts | 2 | tries (one more after an undo) | DEV-BKP-016 |

**§24, the paragraph "Stage `recovery`."** Replace it with:

**Stage `recovery`.** Requires: `RecoveryLink` (§15.8, §25) for the online steps; the local-only writer (§9.3) to write a restored folder; `history.tdk`, the log segments and checkpoints in the layouts of §6.4 when the tool reads a device's own folder (DEV-STORE-005 names `recovery` as an owner). Requires (ruling S24, lead ruling L22): `reinstate` (`relay.md` §12.5, RELAY-REVOKE-004, RELAY-REVOKE-005), `revoke` signed by the recovery identity (`relay.md` §12.4), and `append` with the door for a frame of `dev_R` (`relay.md` §9.3, RELAY-SEQ-009); the wraps of `seal.md` §7 (SEAL-WRAP-001) and the `reinstate` arguments of `seal.md` §16.2 (SEAL-SIG-004). Provides: the backup writer and reader through `seal`; the offline replay of a top-up; the reinstatement of a device that a device revoked (§15.8 steps 6 to 11); the sheet's printed page.

**§25, the module `recovery_online`.** Replace the whole module with:

```rust
// ---------- recovery tool online steps (§15.8)
pub mod recovery_online {
    pub struct RecoveryLink { /* transport to one relay, door credential */ }
    pub enum FenceAnswer { Fenced { seq: u64, chain: [u8; 32] },             // the Fence is sequenced
                           Counter { last_ctr: u64, last_seq: u64 } }        // 409 relay/gap or relay/conflict (§15.8 step 5)
    pub enum AppendAnswer { Stored { seq: u64, chain: [u8; 32] },            // new or dup (§15.8 step 9)
                            Gap { last_ctr: u64, last_seq: u64 },            // re-seal once (§15.8 step 9)
                            Conflict { seq: u64 } }                          // §15.8 step 11
    impl RecoveryLink {
        pub fn new(address: &str, recovery: &taskdeck_seal::RecoveryKeys, trust: super::TrustRoots)
            -> Result<Self, super::DeviceError>;
        pub fn read_token(&mut self, backup_token: Option<&str>) -> Result<(), super::DeviceError>;   // §15.8 step 1: the backup's token, else epoch 1, then the refusal's read_epoch
        pub fn read_after(&mut self, after: u64) -> Result<taskdeck_sync::Page, super::DeviceError>;
        pub fn fence(&mut self, frame: Vec<u8>, host: &str, door: [u8; 5], home_gen: u32)
            -> Result<FenceAnswer, super::DeviceError>;
        pub fn reinstate(&mut self, dev: [u8; 16], first_epoch: u32, wraps: Vec<[u8; 96]>)
            -> Result<(), super::DeviceError>;                               // §15.8 step 8: admin-nonce, then reinstate, with the door
        pub fn append(&mut self, frame: Vec<u8>) -> Result<AppendAnswer, super::DeviceError>;   // §15.8 step 9: one frame of dev_R with the door; a refused result is the error
        pub fn revoke(&mut self, dev: [u8; 16]) -> Result<(), super::DeviceError>;              // §15.8 step 11: admin-nonce, then revoke, with the door
    }
}
```

**§26, open questions.** After item 24, append:

25. **The reinstating entry's counter can be too low** (§15.8 step 9). The tool takes the recovery identity's counter from the backup's history and the entries it read. A frame of `dev_R` that clients refused is in neither, and after a purge it is not on the relay either. An `append` answers a counter that is too low with `conflict`, which carries no `last_ctr` (`relay.md` §9.6). Default: the tool undoes (step 11) and ends with the relay's code, and the owner pairs the device again.

### TR1-FOLLOW-3 (item 3: the order of `history.tdk` and `bootstrap.tdk` when a device joins)

The order is the one §15.7 states: `history.tdk` first, then the bootstrap. §18.3 changes to match it, and
§15.7's last sentences now say what "before it persists the bootstrap" means for a pairing and for a rejoin,
in the words of DEV-RUN-006.

**§15.7.** Replace the paragraph with:

Every device holds every control entry from seq 1 in `history.tdk` (`history_complete = true`). A device that created the board, or restored it, has them from the start. A device that bootstraps from a snapshot (a pairing's, or a rejoin's) takes the records up to the snapshot's seq from the snapshot's `history` (`seal.md` §20.4, ruling S27), which `seal.md` §20.5 has verified against the snapshot's membership, and records every later accepted control entry itself (§8.3). The sealer adapter fills a snapshot's `history` from `history.tdk` (the records with seq at most the snapshot's) whenever it seals one. When it opens one for a bootstrap, it hands the records to the store, which writes those it does not already hold to `history.tdk` before it persists the bootstrap: for a pairing, before `bootstrap.tdk` (§18.3, DEV-PAIR-006); for a rejoin (`engine.md` §19.7), in the first `Persist` that carries a `WriteCheckpoint`, before that op (DEV-RUN-006).

**§18.3, the paragraph "Persisting the join".** Replace it with:

**Persisting the join**, in order: `KeyStore::create(L_d)`; the identity directory (created when the key store has not, §12.5) with `store.tds`, `identity.tdk` (keys and the bundle's roots), `device.tdk` (`device_name`, the bundle's token for the host (without `push`, §10.3), `minter` for its `device_no`, `history_complete: true`), `history.tdk` from the bootstrap snapshot's history (`seal.md` §20.4, §15.7), and `bootstrap.tdk` (`Join` with the snapshot material, `added_seq` and the relay target); then the plain files are set aside (§18.4); then `active.tds` is replaced; then the facade reopens in synced mode with `Bootstrap::Join` (`engine.md` §19.8). A crash before `active.tds` leaves an inactive directory, ignored by `open`, and a `DeviceAdded` that the user pairs again over.

**§18.3, clauses.** After DEV-PAIR-003, insert:

- **DEV-PAIR-006** (device) A joining device MUST make `history.tdk`, holding the bootstrap snapshot's history, durable before it writes `bootstrap.tdk`.

## bridge.md

No change.

## For the lead

1. **L20 was not yet in the specs.** seal.md §7 and §16.2 and engine.md §24.4 still said "or the epoch in
   force at `purge_seq`", or built on it (seal.md §7: "unless `first_epoch` was raised …"; engine.md §24.4:
   "A device revoked before a purge cannot replay across it …"). The open questions (seal.md §27 item 18,
   engine.md item 24) still described that case. The reinstatement steps need L20 (step 6 refuses a revoking
   entry below `purge_seq`), so TR1-FOLLOW-1 applies L20 in those places. Recommendation: accept. If another
   change file already carries L20, keep only one of the two.
2. **Undoing a reinstatement whose entry fails (device.md §15.8 step 11, relay.md §12.4).** No ruling covers
   the relay row after a `reinstate` whose entry is not sequenced or is refused. L23 covers only the device.
   Without an undo, the relay keeps a current row (a valid token, a place among the 16 current devices) for a
   device that the membership calls revoked, and a second `reinstate` is refused with `409 relay/target`.
   That would make a retry impossible: a rotation that wins the race leaves the device without that epoch's
   root. The change uses `revoke` alone, signed by the recovery identity, which the relay already accepts
   and which clears the wraps (RELAY-REVOKE-005), and allows one retry. Recommendation: accept. The
   alternative is to drop the undo and the retry, and the device pairs again after any lost race.
3. **The reinstating entry's counter** (device.md §26 item 25, written as the default). Unlike `fence`,
   `append` gives no `last_ctr` on `conflict`. The guess is low only when a frame of `dev_R` that clients
   refused was later purged or never read. Recommendation: accept the residual. A later pass could have
   `append`'s `conflict` with `seq = 0` carry the author's `last_ctr`, which changes relay.md §9.6.
4. **Outside these follow-ups, noticed while writing TR1-FOLLOW-2.** engine.md §28.2 step 2 sets
   `log_floor = p`, but a device that bootstrapped from a snapshot between `p` and the purge rotation has
   `log_floor > p`. Taken literally, the step lowers its floor, and with `floor_chain = None` it would clear
   the stored `floor_chain`, although §7.10 says purge only raises the floor. The change keeps `p` as
   written. Recommendation: in a later pass, write the step as `TruncateLogBelow(max(p, log_floor), …)`, or
   skip it when `p ≤ log_floor`.
