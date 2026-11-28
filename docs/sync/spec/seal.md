# TaskDeck sync spec: seal

*Frames, the entry envelope, signatures and their contexts, the key hierarchy, membership and every control entry, the hash chain, pairing, the recovery sheet, sealed local files, snapshots and blobs, and the backup envelope. Owning stage: `seal`. Conventions, notation and the glossary are in [`README.md`](README.md); this spec does not restate them.*

## 1. Scope

This spec fixes every byte that the `seal` crate (`crates/taskdeck-seal/`) produces or checks, and every rule by which a device decides who may change the membership of a board. It realises:

- SYNC.md §4.2 (frame, envelope, signatures, kinds, receipt checks and their outcomes), §4.8 (hash chain, `based_on`, fingerprint), §5.1 (algorithms), §5.2 (key hierarchy, HPKE info, admin operations), §5.3 (pairing), §5.4 (revocation, rotation, races), §5.5 (recovery sheet, the recovery key's powers, the recovery-key change), §5.6 (sealed local files: the envelope only), §4.7 (the snapshot envelope), §6.6 (the backup envelope, ResetTo), §6.7 (Fence and the move code), §7 (the integrity and pairing controls), §9 step 6 (the genesis entry);
- the verifier findings V1-1, V1-2, V1-3, V1-5, V1-6, V1-10, V1-11, V1-12, V1-13, V1-14, V1-16, V1-17 and V2-3, V2-7, V2-8, V2-9, V2-15, V2-21;
- SYNC_PLAN.md §2 (no hub, no always-on device), §3.4 (security rules), §6.1 item 3, §6.3 (the `seal` stage), §6.4 (the relay verifies frame, login and admin signatures from these layouts), §6.8 (the backup reader and writer, the sheet);
- the rulings S1–S10 of `docs/sync/status/foundation.md`.

The `seal` stage builds all of it as pure functions and sans-IO state machines: no file, network, thread or clock access, and randomness only through the entropy interface of §3.

Non-goals, each owned elsewhere:

- what a replica does with a classified entry (apply, refuse, stop, halt, banners, re-sealing, duties): `engine.md`;
- the content of a `cmd` payload, the canonical encoding itself, the canonical board bytes and `apply`: `core.md`;
- routes, the token layout, the door's use, sequencing and storage at the relay: `relay.md`;
- the content of each sealed local file, the key stores that wrap the storage key, fsync order, backup scheduling: `device.md`;
- the printed layout of the recovery sheet and the recovery tool's workflow: stage `recovery` (SYNC_PLAN.md §6.8), from §18 and §21 here.

The M1 hub is dropped (SYNC_PLAN.md §2): there are no hub frames, no hub contexts (`hub-req`, `hub-resp`) and no pairwise hub keys.

## 2. Terms

Terms used here that README §9 does not define.

| Term | Meaning |
|---|---|
| author | The `dev` in a frame's header: a device, or the recovery identity. |
| authority | A key that may sign frames at a given position: a current device or the current recovery identity (§13.4). |
| current device | A device record whose status is `current` in the membership state before the entry is applied. |
| recovery identity | The current recovery signing key acting as an author, with the `dev` of §6.3. "Current recovery identity" is the one named by the newest recovery record whose `until_seq` is 0. |
| `M_s` | The membership state after the entry at seq `s` has been classified (§13, §14). `M_0` is empty. |
| `E` | In §8–§14: the current epoch number `M_{s−1}.epoch`. In §17: the existing device in pairing. Context makes it clear. |
| log clock `τ` | `M.clock_ms`: the largest `at_ms` of all entries accepted so far (§13.5). |
| opened | A frame whose ciphertext was decrypted and whose plaintext structure was valid (§14 steps 1 to 12). |
| receipt class | One of `Skip`, `Tamper`, `Refuse`, `Accept`, `NeedFrames`: the result of the receipt checks (§14). |
| padded length | `P(U)` of §10.2. |
| sealed bytes | The complete output of sealing a blob or snapshot (§20), before it is split into parts. |
| `vb8(x)`, `vb16(x)`, `vb32(x)` | `u8`, `u16` or `u32` length in bytes of `x`, then `x` (§5.1). |
| `ctx(name)` | The context string `taskdeck/v1/<name>` followed by one `0x00` byte (README §5.2, registry §4). |
| `H(x)` | SHA-256 of `x`. |
| `HKDF(salt, ikm, info, L)` | HKDF-SHA256 extract-then-expand (RFC 5869) with output length `L` bytes. A salt written "empty" is the zero-length string. |
| `HMAC(k, x)` | HMAC-SHA256 (RFC 2104) with key `k`. |
| `x[a..b]` | Bytes `a` (inclusive) to `b` (exclusive) of `x`, counting from 0. |
| projection | The reduced membership a backup reader derives from a backup's history records (§21.6). |

## 3. Rules for every implementation

These hold for all code that implements this spec: the `seal` crate, the relay's verification code (`relay`), and the backup reader and writer (`recovery`).

- **SEAL-GEN-001** (seal) The `seal` crate MUST declare `#![forbid(unsafe_code)]` at its root.
- **SEAL-GEN-002** (seal) The `seal` crate MUST NOT read a clock, the file system, the network, environment variables or thread-local state; every time value it uses MUST be an argument of the call.
- **SEAL-GEN-003** (seal) The `seal` crate MUST implement SHA-256, HKDF, HMAC, AEAD, Ed25519, X25519 and HPKE only by calling the pinned crates of SYNC.md §5.1 (`sha2`, `hkdf` with its `hmac` dependency, `chacha20poly1305`, `ed25519-dalek`, `x25519-dalek`, `hpke`); it MUST NOT contain its own implementation of any of these primitives.
- **SEAL-GEN-004** (seal) The `seal` crate MUST draw random bytes only through the `Entropy` trait of §26, and every function that needs random bytes MUST take an `Entropy` argument and fail with `seal/entropy` when it reports an error.
- **SEAL-GEN-005** (seal, device) Key material that is long-lived (the recovery seed, device signing and X25519 keys, epoch roots, the storage key `L_d`, the board salt) MUST be drawn from `OsEntropy` (the operating system's generator through `getrandom`): the `Entropy` trait MUST be sealed so that no other crate can implement it, and a build without the cargo feature `test-entropy` MUST NOT contain any implementation of it other than `OsEntropy`.
- **SEAL-GEN-006** (seal) Every type that holds secret bytes (seed, private keys, epoch roots, derived symmetric keys, the pairing secret and every key derived from it, the storage key and file keys, decrypted HPKE plaintexts of roots) MUST erase them on drop through `zeroize` (`ZeroizeOnDrop` or `Zeroizing`).
- **SEAL-GEN-007** (seal, relay) Every comparison of a MAC, a commitment, a SAS, a ticket proof, a move code or any other value derived from a secret MUST be constant-time (`subtle::ConstantTimeEq` in Rust; in the relay, a comparison whose running time does not depend on the position of the first difference).
- **SEAL-GEN-008** (seal, relay, recovery) Every parser MUST check a length field against both its stated maximum in §23 and the number of bytes actually remaining in the input before it allocates or reads that many bytes.
- **SEAL-GEN-009** (seal, recovery) A parser MUST NOT interpret bytes protected by a signature, MAC or AEAD before that protection has been verified, except the fixed-width header fields that the verification itself needs (the steps of §14, §17, §19, §20 and §21 name them).
- **SEAL-GEN-010** (seal, relay, recovery) Every Ed25519 verification MUST use the strict rules of README §5.5: a non-canonical `S`, a small-order public key or a small-order `R` MUST fail verification.
- **SEAL-GEN-011** (seal) Every public parsing function of the `seal` crate MUST return an error, never panic, for any input bytes; a property test over random and mutated inputs MUST prove it for every parser of this spec.
- **SEAL-GEN-012** (seal) A sealing function MUST draw a fresh 24-byte nonce from its `Entropy` argument for every AEAD message, and MUST NOT accept a nonce from its caller.
- **SEAL-GEN-013** (seal) Every X25519 Diffie-Hellman output that is all zero bytes MUST be treated as a failure of the operation that computed it.
- **SEAL-GEN-014** (seal) A build of the `seal` crate with the cargo feature `test-entropy` MUST contain the byte string `taskdeck-test-entropy-enabled-3b9e5d` (`TEST_ENTROPY_MARKER`, which `FixedEntropy::new` passes to `std::hint::black_box`), and a build without the feature MUST NOT.

## 4. Context strings

### 4.1 Registry

Every context string in the system is listed here. Each is the ASCII text in the first column followed by one `0x00` byte (README §5.2). Other specs use only strings from this table; a new one is added only by a change to this spec.

| Name `<use>` | Primitive | Where used | Defined in |
|---|---|---|---|
| `frame` | signature | frame signature | §8.3 |
| `login` | signature | relay login | §16.1 |
| `admin` | signature | relay admin operations | §16.2 |
| `backup` | signature | backup and segment files | §16.3 |
| `ctl` | signature | inner signature of a control payload | §11.3 |
| `entry` | AEAD AD | entry envelope | §9.2 |
| `blob` | AEAD AD | sealed snapshots and blobs | §20.2 |
| `file` | AEAD AD | sealed local file records | §19.3 |
| `pair-bundle` | AEAD AD | the pairing bundle (slot D) | §17.8 |
| `wrap` | HPKE info | epoch-root wraps | §7 |
| `wrap-aad` | HPKE aad | epoch-root wraps | §7 |
| `backup-hpke` | HPKE info | backup and segment payloads | §21.3 |
| `backup-aad` | HPKE aad | backup and segment payloads | §21.3 |
| `key-entry` | HKDF info | `K_entry,e` | §6.5 |
| `key-snap` | HKDF info | `K_snap,e` | §6.5 |
| `key-blob` | HKDF info | `K_blob,e` | §6.5 |
| `file-key` | HKDF info | per-file-kind subkeys of `L_d` | §6.6 |
| `recovery-hpke` | HKDF info | recovery HPKE key from the seed | §6.1 |
| `recovery-sign` | HKDF info | recovery signing key from the seed | §6.1 |
| `pair-ticket` | HKDF info | pairing ticket key | §17.3 |
| `pair-mac` | HKDF info | pairing slot MAC key | §17.3 |
| `pair-bundle-key` | HKDF info | pairing bundle key | §17.8 |
| `sas` | HKDF info | short authentication string | §17.7 |
| `pair` | HMAC input | pairing slot MAC | §17.4 |
| `pair-tid` | hash | ticket id from the ticket key | §17.3 |
| `pair-commit` | hash | slot A commitment | §17.5 |
| `pair-transcript` | hash | pairing transcript `T` | §17.7 |
| `pair-code` | hash | check characters of the pairing code | §17.2 |
| `board` | hash | `board_id` | §6.2 |
| `recovery-dev` | hash | the recovery identity's `dev` | §6.3 |
| `epoch` | hash | epoch commitment `c_e` | §6.5 |
| `chain` | hash | hash chain `C_0` and `C_s` | §15.1 |
| `fingerprint` | hash | the sync fingerprint's four words | §15.3 |
| `key-fp` | hash | key fingerprints in banners | §6.8 |
| `move` | hash | the move code | §15.4 |
| `sheet-check` | hash | check characters of the recovery sheet | §18.1 |
| `state` | hash | state hash (ruling S7); composition owned by `engine.md` | §25 |
| `door` | HMAC input | the door (ruling S4); layout owned by `relay.md` | §25 |
| `token` | HMAC input | relay token MAC; layout owned by `relay.md` | §6.7 |
| `local-key` | AD or entropy | wrapping `L_d` in the OS key store; use owned by `device.md` | §6.6 |

Dropped and never registered: `hub-req`, `hub-resp` (SYNC_PLAN.md §2). SYNC.md's `HKDF(s, "ticket")`, `HKDF(s, "mac")`, `HKDF(shared secret, s, "bundle")` and the `info = "taskdeck/v1/" ‖ kind ‖ e` of SYNC.md §5.2 are replaced by `pair-ticket`, `pair-mac`, `pair-bundle-key` and `key-entry`, `key-snap`, `key-blob`. SYNC.md's use of `taskdeck/v1/backup\0` as an HPKE info is replaced by `backup-hpke`, so each string names exactly one use.

### 4.2 Rules

- **SEAL-CTX-001** (seal, relay, recovery) Every signature, MAC, hash-chain step, HKDF info, hash with a context and AEAD or HPKE associated data defined in this spec MUST begin with exactly the context string the registry of §4.1 assigns to that use.
- **SEAL-CTX-002** (seal) A context string MUST NOT be used for a use other than the one §4.1 assigns to it; a test MUST prove, for each pair of signature uses (`frame`, `login`, `admin`, `backup`, `ctl`), that a signature made for one fails verification as the other.
- **SEAL-CTX-003** (seal) An AEAD ciphertext made for one AD use (`entry`, `blob`, `file`, `pair-bundle`) MUST NOT open under the AD of another use, and an HPKE ciphertext made with the `wrap` info MUST NOT open with the `backup-hpke` info or the reverse; a test MUST prove both.

## 5. Shared encodings

### 5.1 Length prefixes

`vb8`, `vb16` and `vb32` prefix a byte string with its length in bytes as a big-endian `u8`, `u16` or `u32` (README §5.1). Every variable-length field in this spec uses one of them, and its clause or table names which.

### 5.2 Crockford base32

Used for the pairing code, the recovery sheet, the move code, key fingerprints and the relay address (`relay.md`).

- Alphabet, value 0 to 31: `0123456789ABCDEFGHJKMNPQRSTVWXYZ`.
- **Encoding** a byte string: read it as a bit string, most significant bit of byte 0 first; take 5 bits at a time; if the last group has fewer than 5 bits, append zero bits on the right; write each group as its alphabet character. `n` bytes give `ceil(8n / 5)` characters.
- **Normalising** typed input, in this order: (1) remove every space (`U+0020`), hyphen (`U+002D`) and tab; (2) map `a`–`z` to `A`–`Z`; (3) map `O` to `0`, and `I` and `L` to `1`. After normalisation every character must be in the alphabet; `U` and anything else is an error.
- **Decoding** reverses encoding; the padding bits appended on the right must be zero.

- **SEAL-ENC-001** (seal) Crockford base32 encoding MUST produce only uppercase characters of the alphabet of §5.2 and MUST NOT insert separators; display grouping is applied after encoding, as each use states.
- **SEAL-ENC-002** (seal) Decoding typed Crockford base32 MUST normalise exactly as §5.2 lists, MUST reject any character outside the alphabet after normalisation, and MUST reject input whose padding bits are not zero.

### 5.3 JSON payloads

The payloads of kinds `ctl`, `beat` and `snap` (§11), the pairing bundle (§17.8) and the sync metadata of a backup (§21) are JSON in the canonical encoding v1 that `core.md` defines. This spec adds only the following profile.

- Byte strings are JSON strings of lowercase hexadecimal of exactly twice the stated byte width (README §5.4).
- Integers are JSON integers from 0 to 2^53 − 1 unless a table says otherwise. A `u64` such as a seq is carried in JSON only when it is within that range; seqs and counters never exceed it in practice, and a value above it is an error.
- Strings are UTF-8; each table states a length limit in bytes.
- A field marked "required" is always present. A field marked "optional" is either present with a valid value or absent; no field is ever `null`. A field with a stated default is omitted by the canonical encoding when it holds the default, and an absent field means the default. The tables in this spec state every default; a field without one has none.
- Arrays have the order the table states.

- **SEAL-ENC-003** (seal) A parser of a JSON payload defined in this spec MUST reject the payload unless its bytes are exactly the canonical encoding (`core.md`) of the value parsed from them.
- **SEAL-ENC-004** (seal) A parser of a JSON payload defined in this spec MUST reject unknown fields, duplicate keys, missing required fields, floating-point numbers, `null`, integers outside the stated range, byte strings that are not lowercase hexadecimal of the stated width, strings over their stated byte limit, arrays over their stated count limit, and arrays not in their stated order.

### 5.4 Relay origins and hosts

- A **relay origin** is the ASCII serialisation of the relay's HTTPS origin (RFC 6454 §6.2): `https://`, the host in lowercase, and `:` with the port only when the port is not 443. It has no trailing slash and no path. It is at most 263 bytes.
- A **host** in a Fence or a bundle is the host part alone, with an optional `:port`, at most 255 bytes in all: one to 253 bytes of lowercase ASCII letters, digits, `-` and `.`, forming dot-separated labels of 1 to 63 bytes that neither start nor end with `-`, then optionally `:` and a decimal port from 1 to 65535 without leading zeros, other than 443: the default port is never written, so that each relay origin has exactly one host. The full regular expression is `^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?(\.[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?)*(:[1-9][0-9]{0,4})?$`, with the port at most 65535 and not 443, the part before `:` at most 253 bytes, and the whole value at most 255 bytes, so that it fits the one-byte length of `vb8` (§13.2, §16.2).
- The relay origin of a host `h` is `https://` ‖ `h` (`relay_origin`, §26).

- **SEAL-ENC-005** (seal, relay) A relay origin used in a signed or hashed message (§16.1, §17.7) MUST be the serialisation of §5.4 and MUST be carried as `vb16`.
- **SEAL-ENC-006** (seal) A host field (§12.7, §17.8) MUST match the regular expression of §5.4, with at most 253 bytes before any `:`, a port of at most 65535 that is not 443, and at most 255 bytes in all; an encoder MUST refuse to produce, and a parser MUST reject, any other value.

### 5.5 Device names

A device name is a UTF-8 string of 1 to 64 bytes that contains no character in `U+0000`–`U+001F` or `U+007F`–`U+009F`. Names are display data and never unique keys.

- **SEAL-ENC-007** (seal) A device name in any layout of this spec MUST be 1 to 64 bytes of UTF-8 without the control characters of §5.5; a parser MUST reject any other value.

## 6. Keys

### 6.1 Recovery seed and recovery key

```text
seed                    32 bytes from OsEntropy; exists only on paper (§18) and briefly in memory
recovery HPKE key pair  DeriveKeyPair(ikm_h)                       (RFC 9180 §7.1.3, DHKEM(X25519, HKDF-SHA256))
   ikm_h              = HKDF(salt = empty, ikm = seed, info = ctx("recovery-hpke"), L = 32)
recovery signing key    Ed25519 secret key (RFC 8032 §5.1.5) whose 32-byte private seed is
   sk_r               = HKDF(salt = empty, ikm = seed, info = ctx("recovery-sign"), L = 32)
```

The recovery public key is the pair `(rsign_pk, rhpke_pk)`: the Ed25519 public key and the X25519 public key (as HPKE serialises it, 32 bytes).

- **SEAL-KEY-001** (seal) A recovery seed MUST be exactly 32 bytes drawn from `OsEntropy` when it is created.
- **SEAL-KEY-002** (seal, recovery) The recovery HPKE key pair MUST be `DeriveKeyPair(ikm_h)` of RFC 9180 §7.1.3 for DHKEM(X25519, HKDF-SHA256), with `ikm_h` as §6.1 defines it.
- **SEAL-KEY-003** (seal, recovery) The recovery signing key MUST be the Ed25519 key whose 32-byte private key is `sk_r` as §6.1 defines it.
- **SEAL-KEY-004** (seal) The `seal` crate MUST NOT provide any function that writes the seed in any form other than the sheet encoding of §18, and the sheet encoding MUST be returned only in a zeroizing string type.

### 6.2 Board id

```text
salt      = 16 bytes from OsEntropy, drawn once when the board is created
board_id  = H(ctx("board") ‖ rsign_pk[32] ‖ salt[16])[0..16]
```

`rsign_pk` is the signing key of the genesis recovery key. A later recovery-key change (§12.6) never changes `board_id`: the board id certifies the genesis recovery key, and the signed control entries certify every later one.

- **SEAL-KEY-005** (seal, relay, recovery) `board_id` MUST equal `H(ctx("board") ‖ rsign_pk ‖ salt)[0..16]` with the genesis recovery signing key and the genesis salt.
- **SEAL-KEY-017** (seal, recovery) A receiver MUST refuse the entry at seq 1 with `seal/ctl_genesis`, and a backup reader MUST reject the history with `seal/backup_history`, when the Genesis item's `salt` and `recovery.sign_pk` do not give the frame's `board_id` by §6.2.

### 6.3 The recovery identity's dev

```text
dev_R = H(ctx("recovery-dev") ‖ board_id[16] ‖ rsign_pk[32])[0..16]
```

The recovery identity authors frames under `dev_R` with its own counter, starting at 1 with the Genesis frame (ruling S3). After a recovery-key change the new key has a new `dev_R`, computed with the same `board_id` and the new signing key, and its counter starts at 1.

- **SEAL-KEY-006** (seal, relay) The `dev` of a recovery identity MUST be `H(ctx("recovery-dev") ‖ board_id ‖ rsign_pk)[0..16]` for its signing key `rsign_pk`.
- **SEAL-KEY-007** (seal) A frame authored by a recovery identity MUST carry that identity's `dev_R` in its header and that identity's next counter value, the first frame of each recovery identity carrying `ctr = 1`.

### 6.4 Device keys

Each device has, generated on the device and never leaving it:

| Key | Generation | Public part |
|---|---|---|
| `dev` | 16 bytes from `OsEntropy` | itself |
| signing key | Ed25519 private key: 32 bytes from `OsEntropy` | `sign_pk[32]` |
| X25519 key | HPKE `DeriveKeyPair(ikm)` with `ikm` = 32 bytes from `OsEntropy` | `hpke_pk[32]` |

A device that is replaced (SYNC.md §5.4, V1-3) generates all three again and is a new device.

- **SEAL-KEY-008** (seal) A device's `dev`, signing key and X25519 key MUST each be generated from fresh `OsEntropy` output as §6.4 lists, and the `seal` crate MUST NOT offer a function that derives one of them from another value.
- **SEAL-KEY-009** (seal) The `seal` crate MUST NOT offer any function that returns a device's or the recovery key's private key bytes, or an epoch root's bytes, except the serialisations used for the sealed identity file (§19.2, kind 7), which MUST be returned only in a zeroizing buffer.

### 6.5 Epoch roots, commitments and per-kind keys

```text
R_e        = 32 bytes from OsEntropy, one per epoch e (u32, starting at 1)
c_e        = H(ctx("epoch") ‖ board_id[16] ‖ u32 e ‖ R_e[32])
K_entry,e  = HKDF(salt = board_id, ikm = R_e, info = ctx("key-entry") ‖ u32 e, L = 32)
K_snap,e   = HKDF(salt = board_id, ikm = R_e, info = ctx("key-snap")  ‖ u32 e, L = 32)
K_blob,e   = HKDF(salt = board_id, ikm = R_e, info = ctx("key-blob")  ‖ u32 e, L = 32)
```

`K_entry,e` seals entries (§9), `K_snap,e` snapshots (§20, blob kind 1), and `K_blob,e` every other blob (§20, blob kind 2).

- **SEAL-KEY-010** (seal) An epoch root MUST be 32 bytes drawn from `OsEntropy` by the device that composes the entry carrying it (Genesis or EpochRotated).
- **SEAL-KEY-011** (seal, recovery) The commitment of epoch `e` MUST be `H(ctx("epoch") ‖ board_id ‖ u32 e ‖ R_e)`.
- **SEAL-KEY-012** (seal, device, recovery) `K_entry,e`, `K_snap,e` and `K_blob,e` MUST be derived exactly as §6.5 states, and a key derived for one kind MUST NOT be used for another.
- **SEAL-KEY-013** (seal, device) A device that learns an epoch root (from a wrap, a bundle or a backup) MUST check it against the commitment `c_e` recorded in its membership state before it derives a key from it, except that a joining device derives `K_snap` from the bundle's root of the snapshot's epoch to open the snapshot that carries its first membership, and checks every bundle root against that membership before it derives any other key (§17.9 steps 4 and 5).

### 6.6 Storage key and file keys

`L_d` is 32 bytes from `OsEntropy`, drawn when a device first stores sealed files for a board, and kept only wrapped by the operating system's key store (`device.md`, SYNC.md §5.6). When the key store accepts associated data or optional entropy (Android Keystore GCM AAD, DPAPI's optional entropy), `device.md` passes `ctx("local-key") ‖ board_id ‖ dev`.

```text
K_file(kind) = HKDF(salt = board_id, ikm = L_d, info = ctx("file-key") ‖ u8 kind ‖ dev[16], L = 32)
```

`kind` is the file kind of §19.2.

- **SEAL-KEY-014** (seal, device) Each sealed local file of kind `k` MUST be sealed under `K_file(k)` as §6.6 defines it, never under `L_d` directly and never under the key of another kind.

### 6.7 Relay tokens

A relay token is an opaque bearer string to every client: it is stored, sealed, carried in the pairing bundle and in backups, and sent in the `Authorization` header, but never parsed. Its layout, its MAC and its text form belong to `relay.md`; its MAC input begins with `ctx("token")`. In layouts here it is ASCII of 1 to 512 bytes.

- **SEAL-KEY-015** (seal) The `seal` crate MUST treat a relay token as an opaque ASCII string of 1 to 512 bytes, the empty `read_token` of §21.5 standing for no token, and MUST NOT derive any key or identifier from it.

### 6.8 Key fingerprints

Banners show who authored a membership change by key, not only by name (V1-2, V1-6). The fingerprint of a device or of a recovery key is:

```text
kfp = H(ctx("key-fp") ‖ sign_pk[32] ‖ hpke_pk[32])[0..10]
text: Crockford base32 of kfp (16 characters), shown as four groups of four joined by "-"
short form: the first group, followed by "…"
```

For example a banner reads "key 7F3A…" and the device details show `7F3A-K2QZ-0M9C-RT4E` (symbolic). `key_fingerprint`, `DevicePublic::fingerprint` and `RecoveryPublic::fingerprint` (§26) return the grouped text.

- **SEAL-KEY-016** (seal) The key fingerprint of a device or recovery key MUST be the Crockford base32 encoding of `H(ctx("key-fp") ‖ sign_pk ‖ hpke_pk)[0..10]`, grouped as §6.8 states.

## 7. Epoch-root wraps

An epoch root is wrapped to each recipient with HPKE base mode, single-shot (RFC 9180 §6.1), suite DHKEM(X25519, HKDF-SHA256) `0x0020`, HKDF-SHA256 `0x0001`, ChaCha20-Poly1305 `0x0003`.

```text
recipient   a current device (id = its dev, pk = its hpke_pk) or the current recovery key (id = dev_R, pk = rhpke_pk)
info        = ctx("wrap") ‖ board_id[16] ‖ u32 e ‖ recipient_id[16] ‖ recipient_pk[32]
aad         = ctx("wrap-aad") ‖ u8 ver=1 ‖ u8 suite=1 ‖ u16 0x0020 ‖ u16 0x0001 ‖ u16 0x0003 ‖ c_e[32]
plaintext   = R_e (32 bytes)
(enc, ct)   = SealBase(pkR = recipient_pk, info, aad, plaintext)
```

The info binds the use, the board, the epoch and the recipient; the aad binds the envelope version, the suite and the commitment (V1-12).

**The ephemeral key.** `SealBase` makes its ephemeral key with RFC 9180's `GenerateKeyPair`: `wrap_root` draws exactly 32 bytes `ikm` from its `Entropy` argument and takes the key pair `DeriveKeyPair(ikm)` (RFC 9180 §7.1.3). It draws nothing else. The wraps of one rotation are made in ascending recipient-id order, the order in which they stand in the item (§12.5); so within the one sealing call of a rotation the root is drawn first, then the 32 bytes of each wrap in that order, then the frame's nonce (`engine.md` §6.1 `seal`, ENG-SEALER-008). A test vector (§24, `seal-rotation.json`) gives those 32 bytes for each wrap, in that order.

**Wrap layout** (96 bytes):

| Offset | Field | Width | Value |
|---|---|---|---|
| 0 | `recipient_id` | 16 | the recipient's `dev` or `dev_R` |
| 16 | `enc` | 32 | HPKE encapsulated key |
| 48 | `ct` | 48 | HPKE ciphertext: 32 bytes of root and the 16-byte tag |
| | **total** | **96** | |

In JSON a wrap is a 192-character hex string of these 96 bytes. A receiver opens a wrap only when its `recipient_id` is the receiver's own id, so that every byte of a wrap is bound; every failure to open is `seal/commitment`, and wrapping to a public key whose X25519 output is all zero is `seal/dh_zero`.

**Wraps delivered outside an entry.** A device that the recovery identity reinstates (ruling S24) receives, outside the log, the wraps of the roots of the epochs it missed: the recovery identity signs them in the `reinstate` operation (§16.2), and the relay returns them at the device's login (`relay.md`; `device.md` §10.3). The device opens the wrap of epoch `e` by SEAL-WRAP-002, with `c_e` as its own membership records it, and only once its membership records `c_e`, oldest epoch first: the commitment of the first wrapped epoch is already in its membership (it classified the revoking rotation, which is sealed under an epoch it held; the recovery tool offers no reinstatement when the revoking entry lies below `purge_seq`, lead ruling L20, `device.md` §15.8); each later one enters it when the device classifies, in seq order, the rotation that set that epoch, which is sealed under the epoch before it. It uses a root only after it matched `c_e` (SEAL-KEY-013).

- **SEAL-WRAP-001** (seal, recovery) A wrap of `R_e` MUST be produced by HPKE `SealBase` with the suite, `info`, `aad` and plaintext of §7, and laid out as the 96-byte table of §7.
- **SEAL-WRAP-002** (seal, recovery) Unwrapping MUST call HPKE `OpenBase` with the `info` and `aad` rebuilt from the receiver's own `board_id`, the epoch named by the item, the receiver's own id and public key, and the commitment `c_e` from the same item (for a wrap delivered outside an entry, §7: the epoch it is delivered for and `c_e` as the receiver's membership records it), never from values taken from the wrap.
- **SEAL-WRAP-003** (seal, engine, device) A device MUST treat a failure to open its own wrap, or an opened root whose commitment differs from `c_e`, as the commitment halt of §14.5, for a wrap delivered outside an entry (§7) as for one inside a rotation.
- **SEAL-WRAP-004** (seal) `wrap_root` MUST draw exactly 32 bytes from its `Entropy` argument, MUST derive HPKE's ephemeral key pair from them with `DeriveKeyPair`, and MUST draw nothing else.

## 8. Frame

### 8.1 Layout

A frame is what a device appends and the relay sequences.

| Offset | Field | Width | Type | Value or rule |
|---|---|---|---|---|
| 0 | `magic` | 4 | ASCII | `"TDF1"` |
| 4 | `board_id` | 16 | bytes | the board |
| 20 | `dev` | 16 | bytes | the author: a device's `dev` or a recovery identity's `dev_R` |
| 36 | `ctr` | 8 | u64 | the author's counter, from 1, rising by exactly 1 per frame of this author |
| 44 | `len` | 4 | u32 | the number of bytes of `body`, and nothing else |
| 48 | `body` | `len` | bytes | the envelope of §9 |
| 48 + `len` | `sig` | 64 | bytes | Ed25519 signature (§8.3) |
| | **total** | **112 + `len`** | | at most 262,144 bytes (256 KiB) |

The first 48 bytes are the **frame header**. With the bounds of §9 and §10, `len` is between 558 and 258,094, so a valid frame is between 670 and 258,206 bytes; the 256 KiB cap is the outer bound that a relay or receiver checks before anything else.

### 8.2 Frame hash

`frame_hash = H(frame)`, over all `112 + len` bytes including `sig`. It is used by the chain (§15.1), Skip (§12.9), the admin arguments of §16.2 and the move code (§15.4).

### 8.3 Signed bytes

```text
signed = ctx("frame") ‖ frame[0 .. 48 + len]
sig    = Ed25519-Sign(author's signing key, signed)
```

The signature covers the header and the body, so the author commits to board, author, counter, length, envelope version, suite, epoch, nonce and ciphertext. The seq is assigned later by the relay and is bound by the chain instead (§15).

- **SEAL-FRAME-001** (seal, relay) A frame MUST be laid out exactly as the table of §8.1, with `len` equal to the byte length of `body`.
- **SEAL-FRAME-002** (seal, relay) A frame's signature MUST be the Ed25519 signature by the author's signing key over `ctx("frame") ‖ frame[0 .. 48 + len]`.
- **SEAL-FRAME-003** (seal, relay) A receiver MUST reject a frame whose total length exceeds 262,144 bytes or is below 670 bytes, whose first four bytes are not `"TDF1"`, or whose `len` field is not its total length minus 112, before reading any other field.
- **SEAL-FRAME-004** (seal) A sealer MUST refuse to produce a frame with `ctr = 0`, returning `seal/frame_format` and no frame.
- **SEAL-FRAME-005** (device) A sealer MUST produce the frame for a given (author, `ctr`) at most once and MUST return the stored bytes, never a new sealing, when it is asked again for an entry it already sealed (SYNC.md §4.3 rule 2).

## 9. Envelope

### 9.1 Body v1

| Offset in `body` | Field | Width | Type | Value or rule |
|---|---|---|---|---|
| 0 | `ver` | 1 | u8 | 1 |
| 1 | `suite` | 1 | u8 | 1 (XChaCha20-Poly1305 entries, Ed25519 frames, HPKE wraps as §7) |
| 2 | `epoch` | 4 | u32 | the epoch whose `K_entry,e` sealed the ciphertext |
| 6 | `nonce` | 24 | bytes | fresh from `Entropy` |
| 30 | `ciphertext` | `len − 30` | bytes | AEAD output: the padded plaintext of §10, then the 16-byte tag |
| | **total** | **`len`** | | `len − 30 − 16` is the padded plaintext length, a value of `P` (§10.2) from 512 to 258,048 |

### 9.2 Associated data and key

```text
AD  = ctx("entry") ‖ frame header[48] ‖ u8 ver ‖ u8 suite ‖ u32 epoch
key = K_entry,epoch
ciphertext = XChaCha20-Poly1305-Seal(key, nonce, AD, padded plaintext)
```

`frame header` is bytes 0 to 47 of the frame: magic, `board_id`, `dev`, `ctr` and `len`. The AD therefore binds the board, the author, the counter, the length, the version, the suite and the epoch.

### 9.3 Which epochs a position accepts

The receiver decides the key from the log position, never from the frame alone. For the entry at seq `s`, with `E = M_{s−1}.epoch`:

1. `epoch = E` is accepted for every kind.
2. `epoch = E − 1` (only when `E ≥ 2`) is accepted only under the stale-epoch rule of §13.9: its 30-day condition is checked before decryption (step 9 of §14.2), the others after.
3. Any other epoch is refused: an epoch above `E` with `seal/bad_epoch`, an older one with `seal/old_epoch` (§14.2 step 9).

An entry carrying `EpochRotated{E+1}` is sealed under epoch `E`, because no receiver can know `R_{E+1}` before applying it. The new epoch applies to every entry at a seq greater than the rotation's seq (§13.8).

- **SEAL-ENV-001** (seal) A body MUST be laid out exactly as the table of §9.1, with `ver = 1` and `suite = 1` under rules version 1.
- **SEAL-ENV-002** (seal) The AEAD associated data of an entry MUST be `ctx("entry") ‖ frame[0..48] ‖ ver ‖ suite ‖ epoch` and the key `K_entry,epoch`.
- **SEAL-ENV-003** (device) A sealer MUST seal an entry under the epoch that is current in its confirmed membership state when it seals, and MUST NOT seal under an epoch whose rotation entry it has not applied.
- **SEAL-ENV-004** (device) A sealer MUST seal an entry that carries `EpochRotated{e+1}` under epoch `e`.
- **SEAL-ENV-005** (seal) A receiver MUST accept for the entry at seq `s` only the epochs `M_{s−1}.epoch` and, under §13.9, `M_{s−1}.epoch − 1`, and MUST refuse a frame under any other epoch without attempting decryption.

## 10. Plaintext and padding

### 10.1 Layout

| Offset | Field | Width | Type | Value or rule |
|---|---|---|---|---|
| 0 | `entry_id` | 16 | bytes | random, from `Entropy`, one per entry; kept when an entry is re-sealed (`engine.md`) |
| 16 | `kind` | 1 | u8 | 1 `cmd`, 2 `ctl`, 3 `beat`, 4 `snap` (§11) |
| 17 | `at_ms` | 8 | i64 | the author's clock when it composed the entry; 0 to 253,402,300,799,999 (9999-12-31T23:59:59.999Z, the end of `core.md` §5.1's instant range) |
| 25 | `based_on_seq` | 8 | u64 | the seq of the composer's board head when it composed the entry (`engine.md` ENG-ENQ-005); 0 if it held no entry |
| 33 | `based_on_chain` | 32 | bytes | the chain value `C_{based_on_seq}` (§15.1); `C_0` when `based_on_seq = 0` |
| 65 | `payload_len` | 4 | u32 | the byte length of `payload` |
| 69 | `payload` | `payload_len` | bytes | the kind's payload (§11) |
| 69 + `payload_len` | `padding` | `P(U) − U` | bytes | all `0x00`; `U = 69 + payload_len` |
| | **total** | **`P(U)`** | | |

The fixed prefix is 69 bytes. The plaintext is never longer than 258,048 bytes.

### 10.2 Padding function

Let `U = 69 + payload_len`, the unpadded length. The padded length `P(U)` is:

1. If `U ≤ 8192`: `P(U) = 512 × ceil(U / 512)` (512-byte buckets).
2. If `U > 8192` (Padmé, Nikitin et al., PETS 2019):
   1. `E = floor(log2 U)`;
   2. `S = floor(log2 E) + 1`;
   3. `z = E − S`;
   4. `m = 2^z − 1`;
   5. `P(U) = (U + m) AND NOT m` (round `U` up to a multiple of `2^z`).

All arithmetic is on unsigned 64-bit integers; `floor(log2 x)` is the index of the highest set bit. The two rules meet at 8192: for `U` from 8193 to 16383, Padmé rounds to multiples of 512 as well. Examples: `P(69) = 512`, `P(600) = 1024`, `P(8192) = 8192`, `P(8193) = 8704`, `P(20000) = 20480`, `P(258048) = 258048`, `P(258049) = 262144`.

**Cap.** The largest padded plaintext that fits the 256 KiB frame is 258,048 bytes (`262,144 − 112 − 30 − 16 = 261,986`, and the largest value of `P` not above it is `258,048 = 63 × 4096`). A payload with `P(69 + payload_len) > 258,048`, that is `payload_len > 257,979`, cannot be sealed.

### 10.3 Unpadding

In this order, after the AEAD has opened:

1. The plaintext length `L` must be at least 69.
2. Read `payload_len`; `U = 69 + payload_len` must be at most `L` (computed without overflow).
3. `P(U)` must equal `L` exactly.
4. Every byte from `U` to `L` must be `0x00`.
5. `kind` must be 1, 2, 3 or 4; `at_ms` must be from 0 to 253,402,300,799,999.

- **SEAL-PLAIN-001** (seal) A plaintext MUST be laid out exactly as the table of §10.1.
- **SEAL-PLAIN-002** (seal) A sealer MUST draw a fresh `entry_id` from `Entropy` for every new entry and MUST keep the same `entry_id` when the engine asks it to re-seal an entry.
- **SEAL-PLAIN-003** (seal) A sealer MUST set `based_on_seq` and `based_on_chain` to the caller's `based_on` unchanged, and MUST refuse with `seal/bad_plaintext` to seal an entry whose `based_on_seq` is 0 and whose `based_on_chain` is not `C_0` (§15.1).
- **SEAL-PAD-001** (seal) A sealer MUST pad the plaintext with `0x00` bytes to exactly `P(69 + payload_len)` bytes as §10.2 computes it.
- **SEAL-PAD-002** (seal) A sealer MUST refuse, with `seal/payload_too_large`, a payload for which `P(69 + payload_len)` exceeds 258,048.
- **SEAL-PAD-003** (seal) A receiver MUST perform the five checks of §10.3 in order and MUST classify a frame that fails any of them as refused (§14).

## 11. Kinds and payloads

### 11.1 Kind numbers and authors

| `kind` | Name | Payload | May be authored by | Epochs accepted |
|---|---|---|---|---|
| 1 | `cmd` | one log command, canonical encoding, opaque to seal | a current device | current; previous under §13.9 |
| 2 | `ctl` | the control payload of §11.3 | per item, §13.6 | current only |
| 3 | `beat` | the beat payload of §11.4 | a current device | current; previous under §13.9 |
| 4 | `snap` | the snapshot announcement of §11.5 | a current device | current only |

- **SEAL-KIND-001** (seal) A receiver MUST refuse an opened entry whose `kind` is not 1, 2, 3 or 4, whose author is not allowed by the table of §11.1 for its kind, or whose epoch is not accepted for its kind by that table.

### 11.2 `cmd`

The payload is the bytes of exactly one log command in the canonical encoding, as `core.md` defines it, from 1 to 257,979 bytes. Seal does not parse it: it returns the bytes to the engine, which hands them to `core`'s strict parser. A parse failure there is a refusal of the same class as a failed check here (§14.4; `engine.md` maps it).

- **SEAL-KIND-002** (seal) The `seal` crate MUST NOT interpret a `cmd` payload and MUST return its bytes unchanged with the opened entry.

### 11.3 `ctl`

The payload is a JSON object:

| Field | Type | Required | Constraints | Meaning |
|---|---|---|---|---|
| `items` | array of control items | required | 1 to 16 items, in the order the author applied them; each item an object of §12 | the items, applied all or nothing |
| `sig` | hex, 64 bytes | required | | the inner signature below |

**Inner signature.** A control payload carries a second signature, by the same key as the frame, over its items in plaintext, so that a backup reader can verify membership history without any epoch root (§21.6):

```text
items_bytes = the canonical encoding of the value of "items"
inner       = ctx("ctl") ‖ board_id[16] ‖ dev[16] ‖ u64 ctr ‖ entry_id[16] ‖ i64 at_ms ‖ H(items_bytes)
sig         = Ed25519-Sign(author's signing key, inner)
```

`board_id`, `dev` and `ctr` are the frame header's; `entry_id` and `at_ms` the plaintext's.

- **SEAL-KIND-003** (seal) A control payload MUST be the object of §11.3 with 1 to 16 items and an inner signature over `ctx("ctl") ‖ board_id ‖ dev ‖ ctr ‖ entry_id ‖ at_ms ‖ H(items_bytes)` by the frame's author.
- **SEAL-KIND-004** (seal, recovery) A receiver and a backup reader MUST verify the inner signature of a control payload with the author's signing key and MUST refuse the entry, or reject the history record, when it fails.

### 11.4 `beat`

A beat is the daily signed statement of a device (`engine.md` decides when). Its head is the plaintext's `based_on_seq` and `based_on_chain`; the payload does not repeat it.

| Field | Type | Required | Constraints | Default | Meaning |
|---|---|---|---|---|---|
| `rules` | array of integer | required | 1 to 16 distinct values from 1 to 2^31 − 1, ascending | | rules versions this build supports |
| `envelopes` | array of array of 2 integers | required | 1 to 16 distinct pairs `[ver, suite]`, each 1 to 255, ascending by `ver` then `suite` | | envelope versions and suites this build can open |
| `hash_versions` | array of integer | required | 1 to 16 distinct values from 1 to 2^31 − 1, ascending | | state-hash versions this build can compute |
| `state` | object | optional | see below | absent | the author's state hash at the latest seq `S ≡ 0 mod 64` with `S ≤ based_on_seq` |
| `acks` | array of object | optional | 0 to 64 objects, ascending by `entry`, distinct `entry` | `[]` | verdicts on `snap` entries |

`state`: `{"seq": S, "hash": hex32, "hash_version": integer}` with `S ≥ 64`, `S ≡ 0 mod 64`, `S ≤ based_on_seq`, `hash_version` from 1 to 2^31 − 1.

`acks` item: `{"entry": integer, "verdict": "ack" | "mismatch"}`, `entry` the seq of a `snap` entry, below the beat's own seq.

- **SEAL-KIND-005** (seal) A receiver MUST refuse a `beat` whose payload violates the table of §11.4, including a `state.seq` that is not a positive multiple of 64 or exceeds `based_on_seq`, or an `acks.entry` at or after the beat's own seq.

### 11.5 `snap`

A `snap` entry announces a snapshot blob (§20) of the author's state at `S`.

| Field | Type | Required | Constraints | Meaning |
|---|---|---|---|---|
| `seq` | integer | required | `S ≥ 64`, `S ≡ 0 mod 64`, `S ≤ based_on_seq` | the seq the snapshot captures |
| `hash` | hex, 32 bytes | required | | the state hash at `S` (ruling S7) |
| `hash_version` | integer | required | 1 to 2^31 − 1 | its version |
| `blob` | hex, 16 bytes | required | | the blob id (§20.1) |
| `sha256` | hex, 32 bytes | required | | `H(sealed bytes)` |
| `size` | integer | required | 67 to 67,108,864 | length of the sealed bytes |
| `parts` | integer | required | `ceil(size / 1,048,576)` | number of parts |
| `inflated` | integer | required | 1 to 67,108,864 | length of the snapshot body (§20.4) before compression |
| `epoch` | integer | required | equal to the frame's `epoch` | the epoch whose `K_snap,e` sealed it |

- **SEAL-KIND-006** (seal) A receiver MUST refuse a `snap` entry whose payload violates the table of §11.5, including `parts` not equal to `ceil(size / 1,048,576)` or `epoch` not equal to the frame's epoch.

## 12. Control items

### 12.1 Encoding and entry shapes

Each control item is a JSON object with a field `"type"` naming it, and the fields its table lists. Within one control entry the items form one of the **shapes** below; any other sequence is refused with `seal/ctl_shape`. "Revoke" and "Reinstate" stand for `DeviceRevoked` and `DeviceReinstated`.

| Shape | Items, in order | Notes |
|---|---|---|
| G | `Genesis` | only at seq 1, and seq 1 holds only this shape |
| A | `DeviceAdded` | one device per entry |
| R | 0 to 15 items each `Revoke` or `Reinstate`, then `EpochRotated` | targets pairwise distinct; revocation and reinstatement always rotate (SYNC.md §5.4) |
| K | `RecoveryKeyChange`, then `EpochRotated` | authored by the current recovery identity (§13.11); a new recovery key always rotates, and the rotation wraps to the new key |
| F | `Fence` | |
| T | `ResetTo` | |
| S | 1 to 16 `Skip` items | targets pairwise distinct |
| U | `RulesUpgrade` | |

Hex fields below are lowercase hexadecimal of the stated byte width (§5.3). Integers are within 0 to 2^53 − 1 unless stated. A bound a table states against the entry's own seq (`bootstrap_seq`, `purge_before`, the targets of `ResetTo` and `Skip`) is checked with the item's row of §13.6, at step 16 of §14.2, and refused with `seal/ctl_target`; step 15 checks what an item alone decides, the lower bound 1 included (`seal/bad_payload`).

- **SEAL-CTL-001** (seal) A receiver MUST refuse a control entry whose items do not form one of the shapes of §12.1.
- **SEAL-CTL-002** (seal) A receiver MUST refuse a control entry at a seq other than 1 that contains a `Genesis` item.

### 12.2 `Genesis`

| Field | Type | Required | Constraints | Meaning |
|---|---|---|---|---|
| `type` | string | required | `"Genesis"` | |
| `salt` | hex, 16 | required | with `recovery.sign_pk` gives `board_id` (§6.2) | the board salt |
| `recovery` | object | required | `{"sign_pk": hex32, "hpke_pk": hex32}` | the genesis recovery public key |
| `device` | object | required | `{"dev": hex16, "sign_pk": hex32, "hpke_pk": hex32, "name": string}`; `dev` differs from `dev_R`; name per §5.5 | the first device |
| `epoch` | integer | required | 1 | |
| `commit` | hex, 32 | required | `c_1` | commitment to `R_1` |
| `wraps` | array of hex, 96 | required | exactly two: to `device.dev` and to `dev_R`, ascending by recipient id | wraps of `R_1` |
| `rules` | integer | required | 1 | the first rules version |
| `hash_version` | integer | required | 1 | the state-hash version of `state_hash` |
| `state_hash` | hex, 32 | required | | the state hash after seq 1 (ruling S7): the genesis `BoardData` and `M_1` |
| `snapshot` | object | optional | `{"blob": hex16, "sha256": hex32, "size": integer, "parts": integer, "inflated": integer}` with the constraints of §11.5 | for an imported board, the genesis snapshot blob (§20, blob kind 1, `S = 1`); absent for an empty board |

The Genesis frame is authored by the recovery identity: `dev = dev_R`, `ctr = 1`, `epoch = 1`, `based_on_seq = 0`, `based_on_chain = C_0`, and it is signed with the recovery signing key (ruling S2).

### 12.3 `DeviceAdded`

| Field | Type | Required | Constraints | Meaning |
|---|---|---|---|---|
| `type` | string | required | `"DeviceAdded"` | |
| `dev` | hex, 16 | required | | the new device's id |
| `sign_pk` | hex, 32 | required | | its Ed25519 public key |
| `hpke_pk` | hex, 32 | required | | its X25519 public key |
| `name` | string | required | §5.5 | its name |
| `transcript` | hex, 32 | required | | the pairing transcript `T` (§17.7) that named these keys |
| `bootstrap_seq` | integer | required | 1 ≤ value < the entry's seq | the seq of the snapshot in the pairing bundle (§17.8): the new device holds the log only after it |

The `dev`, `sign_pk`, `hpke_pk` and `name` are exactly the ones in slot B of the pairing whose transcript is `transcript` (V1-6).

### 12.4 `DeviceRevoked` and `DeviceReinstated`

| Field | Type | Required | Constraints | Meaning |
|---|---|---|---|---|
| `type` | string | required | `"DeviceRevoked"` or `"DeviceReinstated"` | |
| `dev` | hex, 16 | required | | the target device |

### 12.5 `EpochRotated`

| Field | Type | Required | Constraints | Default | Meaning |
|---|---|---|---|---|---|
| `type` | string | required | `"EpochRotated"` | | |
| `epoch` | integer | required | 2 to 2^32 − 1 | | the new epoch `n` |
| `commit` | hex, 32 | required | | | `c_n` |
| `wraps` | array of hex, 96 | required | 2 to 17 wraps, ascending by `recipient_id`, distinct recipients | | wraps of `R_n` |
| `purge_before` | integer | optional | 1 ≤ value ≤ the entry's seq | absent | set by "Purge history before now": every device and the relay drop the log before this seq (`engine.md`, `relay.md`) |

### 12.6 `RecoveryKeyChange`

| Field | Type | Required | Constraints | Meaning |
|---|---|---|---|---|
| `type` | string | required | `"RecoveryKeyChange"` | |
| `sign_pk` | hex, 32 | required | | the new recovery signing key |
| `hpke_pk` | hex, 32 | required | | the new recovery HPKE key |

### 12.7 `Fence`

| Field | Type | Required | Constraints | Meaning |
|---|---|---|---|---|
| `type` | string | required | `"Fence"` | |
| `host` | string | required | §5.4 | the new relay's host |
| `door` | hex, 5 | required | | the new relay's door for this board (ruling S4); with `host` and `board_id` it gives the new relay address (`relay.md`) |
| `home_gen` | integer | required | 2 to 2^32 − 1 | the board's new home generation |

The **home generation** counts the relays a board has lived on: 1 at genesis, plus 1 per accepted Fence. It is seal's own counter and is unrelated to the token generation `gen` of `relay.md`.

### 12.8 `ResetTo`

| Field | Type | Required | Constraints | Meaning |
|---|---|---|---|---|
| `type` | string | required | `"ResetTo"` | |
| `seq` | integer | required | 1 ≤ value < the entry's seq | the target seq `t` |
| `state_hash` | hex, 32 | required | | the author's state hash at `t` |
| `hash_version` | integer | required | 1 to 2^31 − 1 | its version |

The target is identified by its seq; `state_hash` is what every device compares with its own replay to `t` (`engine.md`, V1-10).

### 12.9 `Skip`

| Field | Type | Required | Constraints | Meaning |
|---|---|---|---|---|
| `type` | string | required | `"Skip"` | |
| `seq` | integer | required | 1 ≤ value < the entry's seq | the target seq `t` |
| `frame_hash` | hex, 32 | required | | `H(frame at t)` |

The target is identified by both its seq and its frame hash, so a Skip names exactly one frame of this log.

### 12.10 `RulesUpgrade`

| Field | Type | Required | Constraints | Meaning |
|---|---|---|---|---|
| `type` | string | required | `"RulesUpgrade"` | |
| `rules` | integer | required | 2 to 2^31 − 1 | the version `v` to activate |
| `mode` | string | required | `"normal"` or `"override"` | the fleet-gated form, or one vote of the two-member override (§13.10) |

- **SEAL-CTL-003** (seal) A receiver MUST refuse a control entry any of whose items violates its schema table in §12.2 to §12.10.

## 13. Membership

### 13.1 State

The membership state `M` is derived from the log in seq order. It holds no secrets: epoch roots are kept beside it, by the device, and never enter canonical bytes.

| Field | Type | Meaning |
|---|---|---|
| `board_id`, `salt` | bytes | from Genesis |
| `rules` | u32 | the active rules version |
| `epoch` | u32 | the current epoch `E` |
| `epoch_seq` | u64 | the seq of the entry that set `E` (1 for epoch 1) |
| `epoch_clock_ms` | i64 | the log clock after that entry |
| `clock_ms` | i64 | the log clock `τ` (§13.5) |
| `home_gen`, `home_seq`, `home_host`, `home_door` | | the latest Fence (from Genesis until the first Fence: generation 1, seq 0, empty host and zero door; in `M_0` every field is zero, §13.3) |
| `purge_seq` | u64 | the latest `purge_before`, or 0 |
| `next_device_no` | u16 | the number the next DeviceAdded receives |
| `commitments` | list | `(e, seq, c_e)` for every epoch, ascending by `e` |
| `recovery` | list | every recovery key: `dev_R`, `sign_pk`, `hpke_pk`, `since_seq`, `until_seq` (0 while current), `last_ctr`; ascending by `since_seq`; exactly one has `until_seq = 0` |
| `devices` | list | every device ever added, ascending by `dev`: `dev`, `sign_pk`, `hpke_pk`, `device_no`, `status` (`current` or `revoked`), `added_seq`, `added_by`, `bootstrap_seq`, `status_seq` and `status_by` (seq and author of the last status change; `added_seq` and `added_by` at first), `last_ctr`, `beat_ms`, the supported `rules`, `envelopes` and `hash_versions` from its latest beat, `name` |
| `pending_override` | optional | one RulesUpgrade override vote: `rules`, voter, `seq` |
| `last_reset` | optional | the latest accepted ResetTo: its own seq, `seq` target, `hash_version`, `state_hash` |
| `skips` | list | `(t, frame_hash)` of every accepted Skip target, ascending by `t` |
| `ctl_seqs` | list | the seqs of every accepted control entry, ascending |

### 13.2 Canonical membership bytes

These bytes feed the state hash (ruling S7), snapshot bodies (§20.4), among them the one the pairing bundle names (§17.8), and backups (§21.5). All integers are big-endian.

| Field | Width | Type | Rule |
|---|---|---|---|
| `magic` | 4 | ASCII | `"TDM1"` |
| `version` | 4 | u32 | 1 |
| `board_id` | 16 | bytes | |
| `salt` | 16 | bytes | |
| `rules` | 4 | u32 | |
| `epoch` | 4 | u32 | |
| `epoch_seq` | 8 | u64 | |
| `epoch_clock_ms` | 8 | i64 | |
| `clock_ms` | 8 | i64 | |
| `home_gen` | 4 | u32 | |
| `home_seq` | 8 | u64 | 0 before any Fence |
| `home_host` | 1 + n | vb8 | empty before any Fence |
| `home_door` | 5 | bytes | zero before any Fence |
| `purge_seq` | 8 | u64 | |
| `next_device_no` | 2 | u16 | |
| `n_commit` | 4 | u32 | then `n_commit` × 44 bytes: `u32 e ‖ u64 seq ‖ c_e[32]`, ascending by `e` |
| `n_recovery` | 2 | u16 | then `n_recovery` × 104 bytes: `dev_R[16] ‖ sign_pk[32] ‖ hpke_pk[32] ‖ u64 since_seq ‖ u64 until_seq ‖ u64 last_ctr`, ascending by `since_seq` |
| `n_dev` | 2 | u16 | then `n_dev` device records (below), ascending by `dev` as unsigned bytes |
| `pending_override` | 1 + 0 or 28 | u8 flag | 0, or 1 followed by `u32 rules ‖ voter[16] ‖ u64 seq` |
| `last_reset` | 1 + 0 or 52 | u8 flag | 0, or 1 followed by `u64 at_seq ‖ u64 target ‖ u32 hash_version ‖ state_hash[32]` |
| `n_skip` | 4 | u32 | then `n_skip` × 40 bytes: `u64 t ‖ frame_hash[32]`, ascending by `t` |
| `n_ctl` | 4 | u32 | then `n_ctl` × 8 bytes: `u64 seq`, ascending |

Fixed part before the lists: 4 + 4 + 16 + 16 + 4 + 4 + 8 + 8 + 8 + 4 + 8 = 84 bytes, then `home_host`, then 5 + 8 + 2 = 15 bytes, then the counted lists and flags.

**Device record:**

| Field | Width | Type | Rule |
|---|---|---|---|
| `dev` | 16 | bytes | |
| `sign_pk` | 32 | bytes | |
| `hpke_pk` | 32 | bytes | |
| `device_no` | 2 | u16 | 1 to 8191 |
| `status` | 1 | u8 | 1 current, 2 revoked |
| `added_seq` | 8 | u64 | |
| `added_by` | 16 | bytes | author of the adding entry (`dev_R` for the genesis device) |
| `bootstrap_seq` | 8 | u64 | 0 for the genesis device |
| `status_seq` | 8 | u64 | |
| `status_by` | 16 | bytes | |
| `last_ctr` | 8 | u64 | |
| `beat_ms` | 8 | i64 | §13.10 |
| `rules` | 1 + 4n | u8 count, then `u32` each | ascending |
| `envelopes` | 1 + 2n | u8 count, then `u8 ver ‖ u8 suite` each | ascending |
| `hash_versions` | 1 + 4n | u8 count, then `u32` each | ascending |
| `name` | 1 + n | vb8 | §5.5 |

Fixed part of a record: 16 + 32 + 32 + 2 + 1 + 8 + 16 + 8 + 8 + 16 + 8 + 8 = 155 bytes, then the three lists and the name. A parser also rejects, with `seal/membership_format`, bytes whose state no log can produce: another number of current recovery records than one, commitments other than exactly those of the epochs 1 to `epoch`, a recovery record whose `dev` is not the `dev_R` of its `sign_pk` (§6.3) or whose `until_seq` is not the `since_seq` of the record after it, an id or a `device_no` that appears twice, a `device_no` at or above `next_device_no`, a `board_id` that the first recovery key and the salt do not give, a `home_host` that is neither empty nor a host of §5.4, or a device name that fails §5.5 (`M_0` of §13.3 apart).

- **SEAL-MEM-001** (seal) The canonical membership bytes MUST be produced exactly as §13.2 lays them out, with every list in its stated order, so that equal states give equal bytes.
- **SEAL-MEM-002** (seal) A parser of canonical membership bytes MUST reject, with `seal/membership_format`, input that is not in the canonical form of §13.2 (wrong magic or version, a list out of order or with duplicates, a flag other than 0 or 1, a count over its limit in §23, or trailing bytes).
- **SEAL-MEM-003** (seal) Canonical membership bytes MUST NOT contain any epoch root, private key, storage key or pairing secret.

### 13.3 Genesis

The state before seq 1, `M_0`, is empty: its canonical bytes are the layout of §13.2 with `magic` and `version` as the table states, every other field zero, every flag 0 and every list empty; it is the only state whose `epoch` is 0, and a parser accepts exactly those bytes for it. The entry at seq 1 is classified from the anchor alone, whatever membership the caller passes, with a **genesis anchor** supplied by the caller: `(rsign_pk, salt)` and `R_1`. A creating device knows them, and so does any device that replays the log from seq 1 with its own copy of `R_1`; a pairing device never classifies seq 1, because it starts from the bundle's snapshot (§17.9); a backup reader takes `rsign_pk` from the history and checks it against the sheet's `board_id` (§21.6). Applying Genesis sets:

- `board_id`, `salt`; `rules = 1`; `epoch = 1`; `epoch_seq = 1`; `epoch_clock_ms = clock_ms = at_ms`; `home_gen = 1`, `home_seq = 0`; `purge_seq = 0`; `next_device_no = 2`;
- `commitments = [(1, 1, commit)]`;
- `recovery = [(dev_R, sign_pk, hpke_pk, since_seq 1, until_seq 0, last_ctr 1)]`;
- one device record: the item's `device`, `device_no = 1`, `status = current`, `added_seq = 1`, `added_by = dev_R`, `bootstrap_seq = 0`, `status_seq = 1`, `status_by = dev_R`, `last_ctr = 0`, `beat_ms = at_ms`, `rules = [1]`, `envelopes = [[1,1]]`, `hash_versions = [1]`;
- `ctl_seqs = [1]`; no pending items, no reset, no skips.

- **SEAL-MEM-004** (seal) The entry at seq 1 MUST be accepted only if its frame's `dev` is `dev_R` computed from the anchor's `rsign_pk`, its `ctr` is 1, its signature verifies under `rsign_pk`, `board_id` equals `H(ctx("board") ‖ rsign_pk ‖ salt)[0..16]`, its epoch is 1, it opens under `K_entry,1` from the anchor's `R_1`, `based_on_seq` is 0 with `based_on_chain = C_0`, and its only item is a Genesis item whose `salt` and `recovery.sign_pk` equal the anchor's.
- **SEAL-MEM-005** (seal) A receiver MUST check that `H(ctx("epoch") ‖ board_id ‖ u32 1 ‖ R_1)` equals the Genesis item's `commit`, and that the Genesis `wraps` name exactly the first device and `dev_R`, before it accepts seq 1.
- **SEAL-MEM-006** (seal) Applying an accepted Genesis MUST produce exactly the state §13.3 lists.

### 13.4 Authorities at a position

For the entry at seq `s ≥ 2`, the possible authors are taken from `M_{s−1}`:

- a **current device**: a device record with `status = current`;
- the **current recovery identity**: the recovery record with `until_seq = 0`, provided its `since_seq < s`;
- a **known non-authority**: a revoked device, or a retired recovery record (`until_seq ≠ 0`), or a recovery record whose `since_seq` is `s` or later. Its signature can be checked, so its frames are refused, not skipped;
- anyone else is **unknown**.

A device added by the entry at seq `a` is current from `a + 1`, so it can author only frames sequenced after the entry that added it (SYNC.md §4.4). Within one control entry, the author's class is decided once, from `M_{s−1}`, even if an item of the same entry changes it.

- **SEAL-MEM-007** (seal) A receiver MUST decide the author class of the entry at seq `s` from `M_{s−1}` alone, as §13.4 lists, and MUST NOT treat a device as current at or before the seq of the entry that added it.

### 13.5 Bookkeeping per class

Every frame changes `M` only as this table says (the receipt classes are those of §14).

| Class | Author counter | Log clock `τ` | `ctl_seqs` | Items, beat fields |
|---|---|---|---|---|
| `Skip`, `Tamper`, `NeedFrames` | unchanged | unchanged | unchanged | none |
| `Refuse` (signature verified, counter right) | set to `ctr` | unchanged | unchanged | none |
| `Accept` | set to `ctr` | `τ' = max(τ, at_ms)` | `s` appended if `kind = ctl` | applied (§13.6), or the beat's (§13.10) |

The log clock is the largest `at_ms` of all accepted entries. It never moves backwards. `τ'` is the log clock after the entry itself; the item rules of §13.6 use it where they say so. A refused entry at seq 1 leaves `M_0`: no record exists whose counter it could consume.

- **SEAL-MEM-008** (seal) Classifying the entry at seq `s` MUST change the membership state exactly as the table of §13.5 states for its class, and in no other way.
- **SEAL-MEM-009** (seal) A refused entry MUST consume its author's counter value, and a skipped frame MUST NOT (V1-16).

### 13.6 Transitions

The items of an accepted-so-far control entry are applied in order to a working copy `W`, initialised to `M_{s−1}`. Each item's preconditions are evaluated against `W` as earlier items of the same entry left it; author classes come from `M_{s−1}` (§13.4). If every item passes, `M_s` is `W` with the bookkeeping of §13.5. If any item fails, the whole entry is refused with that item's code, and `M_s` is `M_{s−1}` with the `Refuse` bookkeeping. "D" is a current device, "R" the current recovery identity, `A` the entry's author.

| Item | Author | Preconditions (refusal code) | Effects on `W` |
|---|---|---|---|
| `Genesis` | the anchor's `dev_R` | §13.3 (`seal/ctl_genesis`) | §13.3 |
| `DeviceAdded` | D | new `dev` differs from every device `dev` and every `dev_R` ever recorded; `sign_pk` differs from every recorded `sign_pk`; `hpke_pk` differs from every recorded `hpke_pk` (`seal/ctl_duplicate_key`); fewer than 16 current devices and fewer than 256 device records, and `next_device_no ≤ 8191` (`seal/ctl_device_cap`); `bootstrap_seq ≥ purge_seq` and `bootstrap_seq < s` (`seal/ctl_target`) | adds a record: `device_no = next_device_no`, `status = current`, `added_seq = status_seq = s`, `added_by = status_by = A`, `bootstrap_seq` from the item, `last_ctr = 0`, `beat_ms = at_ms`, `rules = [W.rules]`, `envelopes` = those rules version `W.rules` allows (§14.3), `hash_versions = [1]`, `name`; then `next_device_no += 1` |
| `DeviceRevoked` | D other than the target, or R | target is a current device (`seal/ctl_target`); after the whole entry at least one current device remains (`seal/ctl_last_device`) | target `status = revoked`, `status_seq = s`, `status_by = A`; clears `pending_override` if the target cast it |
| `DeviceReinstated` | R | target is a revoked device whose `status_by` is a device, not a recovery identity (`seal/ctl_target`); fewer than 16 current devices (`seal/ctl_device_cap`) | target `status = current`, `status_seq = s`, `status_by = A`, `beat_ms = at_ms`; `last_ctr` unchanged |
| `EpochRotated` | D or R | `epoch = W.epoch + 1` (`seal/ctl_epoch`); `wraps` name exactly the current devices of `W` and `W`'s current recovery identity, once each, ascending (`seal/ctl_wraps`); `purge_before` is present only when `A` is a device (`seal/ctl_author`); if `purge_before` is present, `W.purge_seq < purge_before ≤ s` (`seal/ctl_target`) | `epoch = n`, `epoch_seq = s`, `epoch_clock_ms = τ'`; appends `(n, s, commit)`; `purge_seq = purge_before` if present |
| `RecoveryKeyChange` | R | the new keys are fresh as for `DeviceAdded`, and the new `dev_R` (§6.3) differs from every recorded `dev` and `dev_R` (`seal/ctl_duplicate_key`) | the current recovery record gets `until_seq = s`; a record `(new dev_R, sign_pk, hpke_pk, since_seq = s, until_seq = 0, last_ctr = 0)` is appended |
| `Fence` | D or R | `home_gen = W.home_gen + 1` (`seal/ctl_home_gen`) | `home_gen`, `home_seq = s`, `home_host`, `home_door` from the item |
| `ResetTo` | D | `t < s`, `t ≥ W.purge_seq`, `t` greater than the `bootstrap_seq` of every current device, `hash_version` allowed by `W.rules` (§14.3) (`seal/ctl_target`) | `last_reset = (s, t, hash_version, state_hash)` |
| `Skip` | D, or R | `t < s` and `t ≥ max(W.purge_seq, s − 16,384)`, with `s − 16,384` taken as 0 when `s ≤ 16,384` (the Skip window, ruling S15); `t` not in `ctl_seqs` and not in `skips` (`seal/ctl_target`); the frame at `t` is supplied and `H(frame) = frame_hash` (`seal/ctl_target`; `NeedFrames` when not supplied, §14.6); when `A` is a device, the target frame's `dev` differs from `A` (`seal/ctl_author`) | appends `(t, frame_hash)` to `skips` |
| `RulesUpgrade` `normal` | D | `v > W.rules` (`seal/ctl_target`); every active member of §13.10 lists `v` in its `rules` (`seal/ctl_support`) | `rules = v`; `pending_override` cleared |
| `RulesUpgrade` `override` | D | `v > W.rules` (`seal/ctl_target`); not a second vote by the same voter for the same `v` (`seal/ctl_duplicate`) | if `pending_override = (v, B, ·)` with `B ≠ A` and `B` current: `rules = v` and `pending_override` cleared; otherwise `pending_override = (v, A, s)` |

An item whose author does not match its Author column is refused with `seal/ctl_author`. An item whose effect would make a list of the membership state exceed its limit in §23 is refused with `seal/ctl_cap`; for `DeviceAdded` and `DeviceReinstated` the device limits use `seal/ctl_device_cap` as the table says. A control entry that would take `ctl_seqs` over its limit is refused with `seal/ctl_cap` after its items passed. "Every recorded" key or id means every device record and every recovery record, current or retired. Whether a revocation leaves a current device is judged over the whole entry, with its later revocations and reinstatements taken as applied: when none would be left, the refusal is the first revocation's (`seal/ctl_last_device`); a later item that then fails its own row refuses the entry with its own code.

- **SEAL-MEM-010** (seal) A receiver MUST apply the items of a control entry in order to a working copy and MUST either accept every item or refuse the entry as a whole, leaving the membership state as §13.6 states.
- **SEAL-MEM-011** (seal) Each control item MUST be accepted only when its author matches the Author column and every precondition of its row in §13.6 holds, and MUST then change the working copy exactly as the Effects column states.
- **SEAL-MEM-012** (seal) A refused control entry MUST be refused with the code of the first item, in order, that failed.

### 13.7 `device_no`

`device_no` is assigned when `DeviceAdded` is applied: the next number in log order, starting at 1 for the genesis device and never reused, even after revocation (SYNC.md §4.4, V2-8). The new device reads it from the sequenced entry before it mints any id; the pairing bundle carries the seq of that entry (§17.8). `device_no × 2^40 + n < 2^53` bounds it at 8191 (README §5.6).

- **SEAL-MEM-013** (seal) `device_no` MUST be assigned from `next_device_no` at the moment `DeviceAdded` is applied and MUST never be assigned twice in one board.
- **SEAL-MEM-014** (seal) A `DeviceAdded` that would assign a `device_no` above 8191 MUST be refused with `seal/ctl_device_cap`.

### 13.8 Epochs and the commitment

- The first `EpochRotated{E+1}` in log order wins. A later one is refused everywhere, its author's device included (V2-8), and its author discards the root it drew. A rotation that lost a race to one other rotation was sealed under the epoch that one replaced, so at its own position it is a control entry under the previous epoch, and step 13 of §14.2 refuses it with `seal/stale_epoch`; when two or more rotations were sequenced before it, its epoch is older than the previous one and step 9 refuses it with `seal/old_epoch`; a rotation sealed under the current epoch that names another epoch than the next is refused at step 16 with `seal/ctl_epoch`.
- A rotation applies from the next seq: the entry at seq `s` that carries `EpochRotated{n}` is itself sealed under `n − 1` (§9.3), and every entry at a seq above `s` is under `n`, or under `n − 1` through the stale-epoch rule (§13.9).
- Each device that is a recipient of an accepted rotation opens its own wrap and checks the root against `commit`. A failure to open, or a mismatch, means some member gave devices different roots; that device halts (§14.5). Other devices, whose wraps are good, continue (V1-12).
- The recovery key's wrap cannot be checked by devices. The recovery tool checks it against `commit` when it unwraps it (§21.6).

- **SEAL-MEM-015** (seal) A receiver MUST refuse an `EpochRotated` whose `epoch` is not exactly one more than the current epoch of the working copy.
- **SEAL-MEM-016** (device) A device whose own rotation entry was refused MUST discard the epoch root it drew for it and MUST NOT seal any entry under that root.
- **SEAL-MEM-017** (seal) After accepting an `EpochRotated{n}` at seq `s`, a receiver MUST classify entries at seqs above `s` with `n` as the current epoch.

### 13.9 Stale-epoch entries

An entry at seq `s` sealed under `E − 1`, where `E = M_{s−1}.epoch ≥ 2`, is accepted only if all of these hold; condition 4 is checked before the entry is opened (step 9 of §14.2), so that no receiver needs a root of `E − 1` once the 30 days have passed, and the others after it has opened (SYNC.md §5.4, V2-8, ruling S6):

1. its kind is `cmd` or `beat`;
2. its author is a current device (never the recovery identity);
3. `based_on_seq < M_{s−1}.epoch_seq` (it was composed before the rotation);
4. `M_{s−1}.clock_ms − M_{s−1}.epoch_clock_ms ≤ 2,592,000,000` (30 days of log clock since the rotation).

Otherwise it is refused with `seal/stale_epoch`. A revoked device's entries are refused before this rule is reached (§14 step 7).

- **SEAL-MEM-018** (seal) A receiver MUST accept an entry under the previous epoch only when all four conditions of §13.9 hold, and MUST refuse it with `seal/stale_epoch` otherwise.

### 13.10 Beats, active members and RulesUpgrade

An accepted `beat` by device `d` sets `d.beat_ms = max(d.beat_ms, at_ms)` and replaces `d.rules`, `d.envelopes` and `d.hash_versions` with the beat's lists. A device's `beat_ms` starts at the `at_ms` of the entry that added (or reinstated) it.

**Active members** for a `RulesUpgrade` by author `A` at seq `s`:

1. Let `ref` be the largest `beat_ms` among the current devices of `W` other than `A`. If there is no other current device, `A` alone is active.
2. A current device `d` is active if `d = A` or `d.beat_ms ≥ ref − 2,592,000,000` (30 days).

The reference excludes the author (decided here; SYNC.md requires "a beat within 30 days" and ruling S6 requires the decision to rest on `at_ms` in the log). That does not stop a device that controls two current devices: a current device can add a second device by itself (`DeviceAdded`), and with the two it can make the others look inactive and activate, with `normal` or with the two votes of `override`, a rules version that no build knows. The board is then repaired only by "Reset trust" (`device.md` §18.6). The owner accepted this limit (decision O1, 2026-11-23).

`normal` activates `v` only if every active member, the author included, lists `v` in its latest beat. `override` is the two-member path of SYNC.md §4.5: two votes for the same `v` by two different current devices, in two entries; the second vote activates `v`. A vote is cleared when its voter is revoked or when any upgrade activates. The new rules version applies to entries at seqs above the activating entry.

A device whose build does not support the activated version halts (§14.5). Under `normal` that can only be a device that was not active; under `override` any device can be one.

- **SEAL-MEM-019** (seal) A receiver MUST compute the active members of a `RulesUpgrade` exactly as §13.10 states.
- **SEAL-MEM-020** (seal) A `RulesUpgrade` with mode `normal` MUST be refused with `seal/ctl_support` when any active member's latest `rules` list lacks `v`.
- **SEAL-MEM-021** (seal) A `RulesUpgrade` with mode `override` MUST activate `v` only on a second vote for the same `v` by a different current device than the pending voter.
- **SEAL-MEM-022** (seal) After a `RulesUpgrade` activates `v` at seq `s`, a receiver MUST check entries at seqs above `s` under rules version `v`.

### 13.11 The recovery-key change

The recovery key changes only by an entry that the current recovery identity signs, which needs the current sheet (SYNC.md §5.5, V1-2; the owner's decision O1, 2026-11-23): a `K`-shape entry, the `RecoveryKeyChange` and a rotation whose wraps go to the new key. It takes effect at once. A lost sheet is answered by "Reset trust" (`device.md` §18.6), not by a change of the recovery key. There is no path without the sheet, because a current device can add another device by itself and a waiting time could only be measured on the `at_ms` that authors write. One compromised device could then propose and confirm alone, at once, and hold the recovery key.

A recovery identity is new from the entry that installs it: its frames are accepted from the next seq, with counter 1. The relay learns its key from the admin operation `recovery_register` (§16.2), which the entry's sender sends only after it has classified that entry `Accept`, because a `K`-shape entry can be refused everywhere (a lost rotation race) and the relay cannot see it. Until then the relay still accepts the old recovery identity, whose frames clients refuse from the installing entry on (`seal/revoked_author`).

- **SEAL-MEM-023** (withdrawn) The 72-hour window of a `confirm`; the change without the sheet is removed (decision O1, `CHANGES.md` 2026-11-23).
- **SEAL-MEM-024** (withdrawn) A `confirm` by the proposer; the change without the sheet is removed (decision O1, `CHANGES.md` 2026-11-23).
- **SEAL-MEM-025** (withdrawn) The proposal of a revoked proposer; the change without the sheet is removed (decision O1, `CHANGES.md` 2026-11-23).

### 13.12 The recovery identity's powers and limits

The recovery identity may author only control entries, and among them only `Genesis` (at seq 1), `DeviceRevoked`, `DeviceReinstated`, `EpochRotated`, `RecoveryKeyChange`, `Fence` and `Skip` (SYNC.md §5.5, V1-5). It never enrols a device, never resets or upgrades rules, never sets `purge_before` (a purge is a device's action, `relay.md` §12.10), and never authors `cmd`, `beat` or `snap`. It reinstates only a device that a device revoked, which lets it overrule a hostile revocation; it cannot revoke the last current device.

- **SEAL-MEM-026** (seal) A receiver MUST refuse every entry authored by a recovery identity that is not a control entry, and every control item by a recovery identity outside the list of §13.12.

### 13.13 What ResetTo and Skip change

`ResetTo` and `Skip` change the board state through the engine's replay (`engine.md`); in membership they only record the target. No control entry is ever undone: `Skip` cannot target one (`ctl_seqs`), and a `ResetTo` resets the board state only, so every revocation, rotation and recovery-key change between `t` and the ResetTo stays in force. A Skip of a `beat` or `snap` entry changes nothing in membership.

- **SEAL-MEM-027** (seal) Accepting a `ResetTo` or a `Skip` MUST NOT change any membership field except `last_reset` or `skips`.

## 14. Receipt checks

### 14.1 Inputs

To classify the frame at seq `s`, a receiver needs, besides the frame:

| Input | Why the frame does not carry it |
|---|---|
| `s` | the relay assigns it after signing; the chain binds it |
| `C_{s−1}` | to compute `C_s` (§15.1) |
| `M_{s−1}` | authors and their keys, counters, the epoch, rules, the log clock, `ctl_seqs`, `skips` |
| the epoch roots it holds for `E` and `E − 1` | the frame names only an epoch number |
| a chain lookup `C_b` for `b` from `retained_from` to `s − 1`, and `retained_from` | the `based_on` check (§14.8) |
| its own `dev`, and for each `ctr` it sealed the hash of that frame | the own-dev check (§14.7); a read-only reader such as the recovery tool has no own `dev` |
| its own X25519 secret key | to open its wrap in a rotation |
| the rules versions its build supports | the halt of §14.5 |
| for `s = 1`, the genesis anchor of §13.3 | the first entry's author key and root are inside it |
| for each `Skip` item, the frame at the target seq | its author and hash (§14.6) |

### 14.2 The checks, in order

Each step runs only if every earlier one passed. The class and code are the outcome when the step fails.

| Step | Check | On failure |
|---|---|---|
| 1 | total length 670 to 262,144 bytes; `magic = "TDF1"`; `len` = total − 112 | `Skip`, `seal/frame_format` |
| 2 | `board_id` equals the receiver's | `Skip`, `seal/wrong_board` |
| 3 | `dev` is a current device, the current recovery identity, or a known non-authority of `M_{s−1}` (§13.4); at `s = 1`, `dev` is the anchor's `dev_R` | `Skip`, `seal/unknown_author` |
| 4 | `sig` verifies (strict) under that author's `sign_pk` over the bytes of §8.3 | `Skip`, `seal/bad_signature` |
| 5 | `ctr = last_ctr + 1` for that author (at `s = 1`, `ctr = 1`) | `Tamper`, `seal/counter_replay` if `ctr ≤ last_ctr`, `seal/counter_gap` otherwise |
| 6 | own-dev check (§14.7) | not a failure: sets the flag `own_dev_foreign` |
| 7 | the author is a current device or the current recovery identity (at `s = 1`, the anchor's recovery identity) | `Refuse`, `seal/revoked_author` |
| 8 | (`ver`, `suite`) is allowed by `M_{s−1}.rules` (§14.3) | `Refuse`, `seal/bad_version` |
| 9 | `epoch` is `E`, or `E − 1` with `E ≥ 2` and condition 4 of §13.9 holding; at `s = 1`, `epoch = 1` | `Refuse`, `seal/bad_epoch` when `epoch > E` (at `s = 1`, any epoch other than 1); `seal/old_epoch` when `epoch < E − 1`; `seal/stale_epoch` when `epoch = E − 1` and condition 4 of §13.9 fails |
| 10 | AEAD opens under `K_entry,epoch` with the AD of §9.2 | `Refuse`, `seal/aead` |
| 11 | the unpadding checks of §10.3 | `Refuse`, `seal/bad_plaintext` |
| 12 | the `based_on` checks of §14.8 | `Tamper`, `seal/based_on_future` or `seal/based_on_fork`; or the flag `based_on_unchecked` |
| 13 | the author and epoch are allowed for the kind (§11.1) | `Refuse`, `seal/not_authorized`; `seal/stale_epoch` for a `ctl` or `snap` under `E − 1` |
| 14 | under `E − 1`: conditions 1 to 3 of the stale-epoch rule of §13.9 | `Refuse`, `seal/stale_epoch` |
| 15 | for `ctl`, `beat`, `snap`: the payload parses under §5.3 and its table; for `ctl`, the inner signature verifies (§11.3) | `Refuse`, `seal/bad_payload`, or `seal/bad_ctl_sig` |
| 16 | for `ctl`: the shape (§12.1) and every item (§13.6) | `Refuse` with the item's code; or `NeedFrames` (§14.6) |
| 17 | none | `Accept`, with the halts of §14.5 where they apply |

If the receiver holds no root for an epoch that step 9 accepted, or holds one that does not match the commitment its membership records for that epoch (SEAL-KEY-013), classification stops with the local error `seal/missing_root`; that is a fault of the receiver's own store, not a property of the frame, and `engine.md` decides what follows.

- **SEAL-RCPT-001** (seal) A receiver MUST run the checks of §14.2 in the order of the table and MUST return the class and code of the first step that fails.
- **SEAL-RCPT-002** (seal) A frame that fails step 1, 2, 3 or 4 MUST be classified `Skip` and MUST leave the membership state unchanged.
- **SEAL-RCPT-003** (seal) A frame that passes step 4 and fails step 5 MUST be classified `Tamper` and MUST leave the membership state unchanged.
- **SEAL-RCPT-004** (seal) A frame that passes steps 1 to 5 and fails a later step other than step 12 (and other than the `NeedFrames` outcome of step 16) MUST be classified `Refuse` with the code of that step, on every device, the author's own device included.
- **SEAL-RCPT-005** (seal) A validly signed frame MUST NOT lead to a halt except through the commitment and rules halts of §14.5; in particular an unknown `ver` or `suite`, an AEAD failure, bad padding and an unparseable payload MUST each lead to `Refuse` (V1-1).
- **SEAL-RCPT-006** (seal) The classification of a frame MUST be a deterministic function of the inputs of §14.1, so that two receivers with equal inputs return equal classes, codes, flags and membership states.

### 14.3 What each rules version allows

| Rules version | (`ver`, `suite`) allowed | State-hash versions |
|---|---|---|
| 1 | (1, 1) | 1 |

A later rules version extends this table through a change to this spec and `core.md`; until then a version above 1 allows no envelope, so every frame under it is refused with `seal/bad_version`, and a `DeviceAdded` under it records no envelope. A device's beat lists what its build can open (§11.4).

### 14.4 Classes and what the receiver learns

| Class | Meaning | Counter consumed | Returned to the engine |
|---|---|---|---|
| `Skip` | not authenticated as any key of this board: an unknown author or a bad signature, or not a frame of this board | no | the code; the engine raises a tampering alarm (`engine.md`) |
| `Tamper` | authenticated, but impossible on an honest relay: a replayed or skipped counter, or a `based_on` head that contradicts the log | no | the code and the evidence (author, `ctr`, expected value, or `based_on` seq and both chain values) |
| `Refuse` | validly signed and in sequence, but fails a later check before apply | yes | the code, the author, `entry_id` if the entry opened |
| `Accept` | passes every check | yes | the opened entry (`entry_id`, kind, `at_ms`, `based_on`, payload), the new membership state, any root learned, the halts and flags |
| `NeedFrames` | a Skip item's target frame was not supplied | no | the list of `(t, frame_hash)` needed |

Flags travel with any class from step 6 on: `own_dev_foreign` (§14.7) and `based_on_unchecked` (§14.8).

A `cmd` entry is `Accept` at this level; `core`'s strict parse and `apply` follow, and a parse failure is mapped by the engine to the `Refuse` outcome, an `apply` failure to an ordinary refusal (`engine.md`).

### 14.5 Halts

Of the three halts of SYNC.md §4.2, two arise here; both come with `Accept`, because the entry is valid and every other device applies it the same way:

1. **Commitment halt** (`seal/commitment`): the entry carries an `EpochRotated` that lists this device as a recipient, and its wrap does not open, or the opened root does not match `commit` (V1-12).
2. **Rules halt** (`seal/unknown_rules`): the entry activates a rules version that this device's build does not support (V2-6).

The third, an entry that crashes `apply` on two consecutive starts, is the engine's (`engine.md`).

- **SEAL-RCPT-007** (seal) A receiver that is a recipient of an accepted `EpochRotated` MUST open its own wrap and compare `H(ctx("epoch") ‖ board_id ‖ u32 n ‖ R_n)` with `commit`, and MUST return the commitment halt when the wrap fails to open or the values differ.
- **SEAL-RCPT-008** (seal) A receiver MUST return the rules halt with an accepted entry that activates a rules version absent from its build's supported list.

### 14.6 Skip targets

To check a `Skip` item the receiver needs the frame at the target seq `t`: from its own log, or fetched from the relay and checked against `frame_hash` (`engine.md`). When a frame is not supplied, classification returns `NeedFrames` with every missing `(t, frame_hash)`, changes nothing, and the caller classifies the same entry again with the frames. A supplied frame whose hash differs from `frame_hash` refuses the entry (`seal/ctl_target`); because the hash is in the signed item, the relay cannot substitute another frame. A target below `max(purge_seq, s − 16,384)`, in `ctl_seqs` or already in `skips` refuses the entry (`seal/ctl_target`) before any frame is asked for, so whether a device holds a frame never changes the verdict (ruling S15); every frame a Skip may still name is kept on the relay (`relay.md` RELAY-COMPACT-007).

For an `S`-shape entry the preconditions of §13.6 are checked in two passes, on every device alike: first, for each item in order, every precondition that does not need the target frame (the floor, `ctl_seqs`, `skips`); then, for each item in order, the frame's presence (`NeedFrames` when any is missing), its hash and its author. The entry is refused with the first failure in that order, which is the order SEAL-MEM-012 means for this shape; `NeedFrames` therefore comes before the refusal for a supplied frame's wrong hash when a missing target stands earlier. The author of a supplied target that is no well-formed frame is the 16 bytes at its offset 20, which the item's `frame_hash` authenticates; a frame shorter than that has no author, so the author check passes.

- **SEAL-RCPT-009** (seal) A receiver MUST check, for every `Skip` item of an entry and before it asks for any frame, every precondition of §13.6 that does not need the target frame, and MUST refuse the entry when one fails, whether or not frames were supplied; only when all of them pass MUST it return `NeedFrames`, without changing any state, for the target frames not supplied, and MUST it refuse the entry when a supplied target frame's hash differs from the item's `frame_hash`.

### 14.7 Frames under this device's own dev

A frame that passes step 4 with `dev` equal to the receiver's own `dev`, at a `ctr` for which the receiver has no record of sealing a frame with this `frame_hash`, means that another holder of this device's signing key exists (V1-1). The frame is still classified by the remaining steps like any other, so that every device stays in step; the flag `own_dev_foreign` tells the engine to raise "someone is signing as this device: replace it" (`engine.md`). The engine decides which records it supplies, including after a restored folder (SYNC.md §4.3 rule 3).

- **SEAL-RCPT-010** (seal) A receiver MUST set `own_dev_foreign` on a frame whose `dev` is its own and whose signature verifies, when its supplied record has no frame with that `ctr` and `frame_hash`, and MUST classify the frame by the remaining steps unchanged.

### 14.8 `based_on`

With `b = based_on_seq` and `h = based_on_chain` of the entry at seq `s`:

1. `b ≥ s`: the author claims a head at or after its own position, which an honest relay cannot produce. `Tamper`, `seal/based_on_future` (V1-16).
2. `b = 0`: `h` must equal `C_0`; otherwise `Tamper`, `seal/based_on_fork`.
3. `1 ≤ b < retained_from`: the receiver no longer holds `C_b`; the check is skipped and the flag `based_on_unchecked` is set, a warning, never an alarm (V2-15). The same holds when the caller's chain lookup has no value for a `b` at or above `retained_from`.
4. otherwise `h` must equal the receiver's `C_b`; otherwise `Tamper`, `seal/based_on_fork`: the author and the receiver saw different logs at `b` (SUNDR fork consistency).

- **SEAL-RCPT-011** (seal) A receiver MUST classify an entry with `based_on_seq ≥ s` as `Tamper` with `seal/based_on_future`.
- **SEAL-RCPT-012** (seal) A receiver that holds `C_b` for `b = based_on_seq` MUST classify the entry as `Tamper` with `seal/based_on_fork` when `based_on_chain` differs from it.
- **SEAL-RCPT-013** (seal) A receiver that does not hold `C_b` because `b` is below its retained log MUST skip the chain comparison and set `based_on_unchecked`, and MUST NOT classify the entry as `Tamper` for that reason.

## 15. Hash chain, head and fingerprints

### 15.1 Chain

```text
C_0 = H(ctx("chain") ‖ board_id[16])                                   (34-byte input)
C_s = H(ctx("chain") ‖ C_{s−1}[32] ‖ u64 s ‖ frame_hash_s[32])         (90-byte input), s ≥ 1
```

`frame_hash_s` is `H` of the frame at `s` (§8.2), whatever its receipt class: the chain covers every sequenced frame, skipped and refused ones included, so the relay can compute it without keys (restore tail, `relay.md`). The two forms have inputs of different lengths (34 and 90 bytes), so an input of one is never an input of the other.

The **head** is `(s, C_s)` for the latest seq a device holds.

- **SEAL-CHAIN-001** (seal, relay) `C_0` and `C_s` MUST be computed exactly as §15.1 states, over every sequenced frame regardless of its receipt class.
- **SEAL-CHAIN-002** (seal) The `seal` crate MUST provide the chain step as a function of `(C_{s−1}, s, frame)` alone, so that a receiver computes `C_s` before and independently of classification.

### 15.2 Head comparison

A head `(s, C)` received from anywhere (a beat, a bundle, a relay's welcome) is consistent with a local log only if the local log holds `C_s = C`, or does not reach `s` yet. `engine.md` owns what a mismatch triggers.

### 15.3 Sync fingerprint

Shown in Settings on every device, and used to adopt a state by fingerprint (V1-10):

```text
x       = H(ctx("fingerprint") ‖ board_id[16] ‖ u64 s ‖ C_s[32])
i_k     = the 11-bit big-endian integer at bits 11k to 11k + 10 of x, k = 0, 1, 2, 3
          (bit 0 is the most significant bit of x[0])
word_k  = line i_k + 1 of wordlist.txt (the line with 0-based index i_k)
text    = decimal s, one space, word_0, word_1, word_2, word_3 joined by single spaces
```

For example `4096 abandon ability able about` (symbolic). The words come from the BIP-39 English word list, 2048 lowercase ASCII words of 3 to 8 letters whose first four letters are unique, MIT licence, from `https://raw.githubusercontent.com/bitcoin/bips/master/bip-0039/english.txt` (fetched 2026-11-21). It is committed as [`wordlist.txt`](wordlist.txt): 2048 lines, each word followed by one `0x0A`, no other bytes, SHA-256 `2f5eed53a4727b4bf8880d8f3f199efc90e58503646d9ff8eff3a2ed3b24dbda`. 44 bits make an accidental match of two different heads at the same seq a one in 1.7 × 10^13 event.

- **SEAL-FP-001** (seal) The sync fingerprint MUST be computed exactly as §15.3 states from the committed `wordlist.txt`.
- **SEAL-FP-002** (seal) The `seal` crate MUST embed the word list at build time and a test MUST check that its SHA-256 is `2f5eed53a4727b4bf8880d8f3f199efc90e58503646d9ff8eff3a2ed3b24dbda` and that it has 2048 lines.

### 15.4 Move code

The device that moves the board shows a move code once its `Fence` entry is sequenced, and each other device follows only after the user types it there (SYNC.md §6.7):

```text
mc   = H(ctx("move") ‖ board_id[16] ‖ u64 fence_seq ‖ fence_frame_hash[32])[0..5]
text = Crockford base32 of mc (8 characters), shown as XXXX-XXXX
```

`fence_seq` and `fence_frame_hash` are the seq and frame hash of the accepted `Fence` entry, wherever it was sequenced (the old relay, or the new one after the seeded tail). A following device computes the expected code from the Fence it holds, normalises the typed text (§5.2), and compares the decoded 5 bytes in constant time; a mismatch is `seal/move_code`. A code that matches a Fence other than the latest accepted one is not accepted. Retry limits are the engine's. `move_code` (§26) returns the shown form, with its hyphen.

- **SEAL-FP-003** (seal) The move code MUST be the Crockford base32 encoding of `H(ctx("move") ‖ board_id ‖ u64 fence_seq ‖ fence_frame_hash)[0..5]`.
- **SEAL-FP-004** (seal) A device MUST accept a typed move code only if it decodes (§5.2) to exactly the 5 bytes computed from the latest accepted `Fence`, compared in constant time.

## 16. Login, admin and backup signatures

### 16.1 Login

The relay sends a 32-byte `relay_nonce`; the device draws a 32-byte `client_nonce` from `Entropy` and signs, with its device signing key:

| Field | Width | Type | Value |
|---|---|---|---|
| context | 18 | ASCII | `ctx("login")` |
| `origin` | 2 + n | vb16 | the relay origin (§5.4) the device is connected to, from its own configuration, never from the relay |
| `board_id` | 16 | bytes | |
| `dev` | 16 | bytes | the device's own |
| `gen` | 4 | u32 | the token generation being requested (`relay.md`) |
| `relay_nonce` | 32 | bytes | from the relay |
| `client_nonce` | 32 | bytes | from the device |
| | **120 + n** | | |

The relay supplies only the nonce; every other field is fixed by the device, so the relay cannot make a login signature that is also a frame or any other signed message (V1-1). The recovery identity has no token and never logs in.

- **SEAL-SIG-001** (seal, relay) A login signature MUST be the device's Ed25519 signature over exactly the bytes of the table of §16.1.
- **SEAL-SIG-002** (seal, device) A device MUST refuse, with `seal/challenge_length`, to sign a login or admin message when the relay's nonce is not exactly 32 bytes, and MUST take `origin` from its own configuration, never from the relay.

### 16.2 Admin operations

An admin operation needs a fresh signature from a current device or the current recovery identity (SYNC.md §5.2, V1-5):

| Field | Width | Type | Value |
|---|---|---|---|
| context | 18 | ASCII | `ctx("admin")` |
| `op` | 1 + n | vb8 | the operation name from the registry below, ASCII |
| `board_id` | 16 | bytes | |
| `signer` | 16 | bytes | the signer's `dev` or `dev_R` |
| `args_hash` | 32 | bytes | `H(args)`, `args` laid out as the registry says |
| `relay_nonce` | 32 | bytes | the relay's fresh admin nonce (`relay.md`) |
| | **115 + n** | | |

**Registry of operations and their `args`.** `relay.md` owns when each is sent and what the relay does; the bytes signed are fixed here. `role` is 1 for a device and 2 for a recovery identity; `status` is 1 current and 2 revoked.

| `op` | `args` layout | Width | Sent with |
|---|---|---|---|
| `register` | `dev[16] ‖ sign_pk[32] ‖ frame_hash[32]` | 80 | frame mode: the `DeviceAdded` frame appended in the same call, `frame_hash` its hash; seq mode (repairing the relay's rows after a restore tail, `relay.md` §12.9): the entry already stored at the `u64 seq` carried in the unsigned `extra`, `frame_hash` its hash |
| `revoke` | `dev[16]` | 16 | before the `R`-shape entry that revokes it; or alone, for a device that `register` added at the relay but whose `DeviceAdded` clients refused (`engine.md` §28.4) |
| `reinstate` | `dev[16] ‖ u32 first_epoch ‖ u16 n ‖ n × wrap[96]` | 22 + 96n | signed by the current recovery identity, before the `R`-shape entry that reinstates `dev`: the wraps (§7) of the roots of epochs `first_epoch` to `first_epoch + n − 1`, in order, each to the target's `hpke_pk`, with `1 ≤ n ≤ 256`; `first_epoch` is the epoch the rotation in the revoking entry set up, and no reinstatement is offered for a revoking entry below `purge_seq` (lead ruling L20, `device.md` §15.8); the last wrapped epoch is the one the reinstating entry is sealed under, whose own rotation wraps to the target as to every recipient (ruling S24, lead ruling L1); the relay stores the wraps and returns them at the target's login (`relay.md`) |
| `recovery_register` | `dev_R[16] ‖ sign_pk[32] ‖ frame_hash[32]` | 80 | after the `K`-shape entry that installs the new recovery key has been appended as an ordinary entry, sequenced, and classified `Accept` by the sender: the unsigned `extra` carries that entry's `u64 seq`, `frame_hash` is its hash; never with the frame in the same call (`relay.md` §12.12) |
| `fence` | `vb8(host) ‖ door[5] ‖ u32 home_gen` | 10 + n | to the old relay, with the `Fence` frame in the same call (in the unsigned `extra`) or after the `Fence` entry is sequenced |
| `seed` | `u32 home_gen ‖ u64 first_seq ‖ chain_before[32] ‖ u64 last_seq ‖ chain_last[32] ‖ blob_id[16] ‖ u16 n ‖ n × (dev[16] ‖ sign_pk[32] ‖ u8 role ‖ u8 status ‖ u64 last_ctr)` | 102 + 58n | to the new relay, with its create secret. `first_seq` is the first tail seq: `min(S + 1, max(1, purge_seq, last_seq + 1 − 16,384))`, with `S` the seeded snapshot's seq and `purge_seq` that of `M_{last_seq}`, whenever the seeding device holds those frames (`engine.md` §28.3), so that the new relay holds every Skip target §13.6 permits after the move; `chain_before = C_{first_seq − 1}`; `last_seq` is the last tail seq (`first_seq − 1` for no tail) and `chain_last = C_{last_seq}`; the entries are the devices and recovery identities of `M_{last_seq}` with their role and status there, and each `last_ctr` is that author's counter at `first_seq − 1`; `blob_id` is the seeded snapshot's, or 16 zero bytes, and is informational |
| `compact` | `u64 below_seq ‖ u64 snap_entry_seq ‖ blob_id[16] ‖ u32 n ‖ n × u64 keep_seq` | 36 + 8n | `blob_id` is the blob the `snap` entry at `snap_entry_seq` announces (its `SnapAnnounce.blob`, §11.5, §26), which the relay records as `compact_snap`; `keep_seq` ascending: the control entries and the recorded Skip targets (§13.1 `skips`) below `below_seq` that stay (`engine.md` §20.2); `below_seq` at most `head_seq − 16,384` (`relay.md` §12.8) |
| `restore_tail` | `u64 first_seq ‖ u32 n ‖ n × frame_hash[32]` | 12 + 32n | the frames re-uploaded at their original seqs; `chain_before` and the frames travel unsigned in `extra` (`relay.md` §12.9) |
| `purge` | `u64 before_seq ‖ u32 epoch` | 12 | by a device, after the rotation that carries `purge_before` has been sequenced: `before_seq` is that `purge_before`, `epoch` the rotation's new epoch; `extra` is empty |
| `mint_read_token` | `dev[16] ‖ u32 epoch` | 20 | `dev` is the signer: a device that writes backups, or `dev_R` for the recovery tool (ruling S21). `epoch` is the writer's current membership epoch; the recovery tool sends 1 and, when refused with `relay/stale_epoch`, the `read_epoch` that refusal reports (`relay.md` §4.2, §12.11) |
| `revoke_read_token` | `dev[16]` | 16 | |

What the relay can check without keys is the signature, the signer's registration and role, the nonce, the operation's own arguments, and for `register` and `recovery_register` that `frame_hash` is the hash of the frame carried in the same call or stored at the seq named in `extra`. It cannot see inside entries; clients enforce membership from the log (§13). `admin_message` and `sign_admin` (§26) refuse arguments the registry excludes: a host outside §5.4 with `seal/host_format`, anything else with `seal/bad_payload`; a relay nonce of another length than 32 bytes is refused first.

**What a device signs to register a new device "in the same call"** (SYNC.md §4.4, gap closed here): the existing device seals the `DeviceAdded` entry, computes its frame hash, and signs `register` with `args = dev_N ‖ sign_pk_N ‖ frame_hash`. The relay appends the frame and registers `dev_N` with `sign_pk_N` in one transaction, effective for frames after that seq, and returns the new device's first token (`relay.md`). Because the hash names the exact frame, the admin signature cannot be reused to register a key with any other entry.

- **SEAL-SIG-003** (seal, relay) An admin signature MUST be the signer's Ed25519 signature over exactly the bytes of the first table of §16.2, with `args_hash = H(args)`.
- **SEAL-SIG-004** (seal, relay) The `args` of each admin operation MUST be laid out exactly as the registry of §16.2 states, and an operation name outside the registry MUST NOT be signed or accepted.
- **SEAL-SIG-005** (engine, device) The `register` arguments MUST name the hash of the frame that carries the matching `DeviceAdded`, and the keys in them MUST be the ones that `DeviceAdded` names.

### 16.3 Backup signature

```text
signed = ctx("backup") ‖ H(file body)
```

`file body` is every byte of the backup or segment file before its `sig` field (§21.2). The writer signs with its device signing key.

- **SEAL-SIG-006** (seal, recovery) A backup or segment signature MUST be the writer's Ed25519 signature over `ctx("backup") ‖ H(file body)`, with `file body` as §21.2 delimits it.

## 17. Pairing

`E` is the existing device, a current, unhalted member that holds a token; `N` is the new device. Pairing is SYNC.md §5.3 made exact: a typed code that is a ticket and a MAC key, never a key to the board; slots A to D; an explicit transcript; a SAS copied from E into N.

### 17.1 Sequence

1. E authenticates the user (`device.md`; on Android BIOMETRIC_STRONG only, SYNC_PLAN.md §2).
2. E draws the secret `s` and the ephemeral key pair `(esk_E, epk_E)` and `r_E` from `Entropy`; derives `ticket_key`, `tid` and `mac_key` (§17.3); builds slot A (§17.5); registers the ticket `tid` with slot A at the relay with its token (`relay.md`); and shows the relay address and the code (§17.2) on its pairing sheet, with capture excluded (SYNC_PLAN.md §2).
3. On N, the user types the code and types or pastes the relay address. N checks the code's check characters, derives the same keys, and reads slot A with `tid` and `ticket_key` as proof. N verifies A's MAC.
4. N generates its long-term keys (§6.4) and `(esk_N, epk_N)`, and posts slot B.
5. E reads slot B, verifies its MAC and contents (§17.6), and posts slot C: the opening of its commitment.
6. N reads slot C, verifies its MAC, and checks the commitment of A against it.
7. Both compute `dh`, the transcript `T` and the SAS (§17.7). E shows its six digits. The user types them into N; N compares them with its own and shows "accepted" or "does not match".
8. The user confirms on E that N accepted (copy-and-enter, V1-6).
9. E takes a fresh snapshot of its state at its head `h`, uploads it (§20), seals `DeviceAdded` with `based_on_seq = h`, and appends it with the `register` admin operation (§16.2) in one call; the relay returns the seq `a` and N's first token.
10. E seals the bundle (§17.8) and posts it as slot D.
11. N reads slot D, opens the bundle, downloads and checks the snapshot, reads the log from `h + 1` and verifies it to the relay's head, finds its `DeviceAdded` at `a`, and reads its `device_no` (§17.9).

### 17.2 The code

```text
s        = 10 bytes (80 bits) from Entropy
check    = the first 10 bits of H(ctx("pair-code") ‖ s)
code     = Crockford base32 of s (16 characters) ‖ Crockford base32 of check (2 characters, 5 bits each)
display  = XXXX-XXXX-XXXX-XXXX-CC
```

On input, N normalises (§5.2), requires exactly 18 characters, decodes the first 16 into 10 bytes (80 bits, no padding) and the last two into 10 bits, and compares the bits with the check computed from the decoded `s`. A failed check means a typing error: N asks again and contacts no relay. The check catches a random error with probability `1 − 2^−10`; any error that passes is caught by the relay finding no ticket, or by a MAC. `new_code` (§26) returns the display form; `parse_code` reads the display form and the bare 18 characters.

- **SEAL-CODE-001** (seal) A pairing code MUST be 16 Crockford base32 characters of an 80-bit secret from `Entropy` followed by 2 characters of the first 10 bits of `H(ctx("pair-code") ‖ s)`.
- **SEAL-CODE-002** (seal) A device MUST reject a typed pairing code that does not normalise to exactly 18 characters of the alphabet or whose check characters do not match, before deriving any key from it or sending any request.

### 17.3 Keys derived from the code

```text
ticket_key = HKDF(salt = board_id, ikm = s, info = ctx("pair-ticket"), L = 32)
tid        = H(ctx("pair-tid") ‖ ticket_key)[0..16]
mac_key    = HKDF(salt = board_id, ikm = s, info = ctx("pair-mac"), L = 32)
```

E registers `tid`. N addresses the ticket by `tid` and proves knowledge of the code by sending `ticket_key`; the relay checks `H(ctx("pair-tid") ‖ ticket_key)[0..16] = tid` in constant time (`relay.md`). The relay learns `ticket_key` but never `s` or `mac_key`.

- **SEAL-PAIR-001** (seal) `ticket_key`, `tid` and `mac_key` MUST be derived from the decoded code and `board_id` exactly as §17.3 states.

### 17.4 Slots

| Field | Width | Type | Value |
|---|---|---|---|
| `label` | 1 | u8 | `0x41` A, `0x42` B, `0x43` C, `0x44` D |
| `role` | 1 | u8 | the sender: `0x45` (E) for A, C, D; `0x4E` (N) for B |
| `body_len` | 2 | u16 | byte length of `body` |
| `body` | `body_len` | bytes | §17.5, §17.6, §17.8 |
| `mac` | 32 | bytes | `HMAC(mac_key, ctx("pair") ‖ label ‖ role ‖ tid[16] ‖ u16 body_len ‖ body)` |
| | **36 + `body_len`** | | |

Every MAC carries the slot's label and its sender's role, so no slot can be replayed as another or reflected to its sender (V1-6). A reader checks a slot's framing first: `body_len` is at most 16,040 and the slot is exactly `36 + body_len` bytes; a slot that fails this is refused as one that is not authenticated, with `seal/slot_mac`. Then come the MAC, the label and the role (`seal/slot_mac`), and only then the widths and the layout of the body (`seal/slot_format`).

| Slot | Body | Body width |
|---|---|---|
| A | `dev_E[16] ‖ commit[32]` | 48 |
| B | `dev_N[16] ‖ sign_pk_N[32] ‖ hpke_pk_N[32] ‖ epk_N[32] ‖ u8 n_rules ‖ n_rules × u32 ‖ vb8(name_N)` | 114 + 4 × `n_rules` + name bytes; 119 to 242 |
| C | `epk_E[32] ‖ r_E[32]` | 64 |
| D | `nonce[24] ‖ ciphertext` | 40 to 16,040 |

In B, `n_rules` is 1 to 16 and the list strictly ascending, each value from 1 to 2^31 − 1: the rules versions N's build supports. `name_N` follows §5.5.

- **SEAL-PAIR-002** (seal) Each slot MUST be laid out as §17.4 states, with the label and role of its row and the MAC over `ctx("pair") ‖ label ‖ role ‖ tid ‖ body_len ‖ body`.
- **SEAL-PAIR-003** (seal) A device MUST verify a slot's MAC in constant time, and its label and role, before it reads any field of the slot's body, and MUST abort the pairing on any failure.

### 17.5 Slot A: the commitment

```text
r_E    = 32 bytes from Entropy
commit = H(ctx("pair-commit") ‖ epk_E[32] ‖ r_E[32])
```

E commits to its ephemeral key before it sees N's, and N reveals its keys before it sees E's, so a relay in the middle that knows the code must fix its keys on both sides before it learns either SAS input; it wins only by guessing the six digits (10^−6 per attempt, R4 §5.4).

- **SEAL-PAIR-004** (seal) N MUST check `H(ctx("pair-commit") ‖ epk_E ‖ r_E)` from slot C against `commit` from slot A in constant time, and MUST abort on a mismatch.
- **SEAL-PAIR-005** (seal) E MUST NOT post slot C before it has read and verified slot B.

### 17.6 E's checks on slot B

In order, aborting on the first failure:

1. the MAC, label `B`, role `N` (§17.4);
2. the layout and limits of §17.4;
3. `dev_N`, `sign_pk_N` and `hpke_pk_N` differ from every `dev`, `dev_R`, `sign_pk` and `hpke_pk` recorded in E's membership state (`seal/pair_keys_conflict`);
4. N's rules list contains E's active rules version (`seal/pair_rules`);
5. E's membership has fewer than 16 current devices, fewer than 256 device records and `next_device_no ≤ 8191` (`seal/ctl_device_cap`).

### 17.7 Transcript and SAS

```text
esk_X = 32 bytes from Entropy (X = E or N), used as an X25519 scalar as RFC 7748 §5 decodes it
epk_X = X25519(esk_X, 9)                                             (the base point u = 9)
dh  = X25519(esk_E, epk_N) on E, X25519(esk_N, epk_E) on N         (all-zero output aborts, SEAL-GEN-013)
T   = H(ctx("pair-transcript") ‖ vb16(relay origin) ‖ board_id[16] ‖ tid[16] ‖ dev_E[16] ‖ slot A ‖ slot B ‖ slot C)
sas = HKDF(salt = s, ikm = dh, info = ctx("sas") ‖ T, L = 8)
digits = (sas read as a big-endian u64) mod 1,000,000, written as 6 decimal digits with leading zeros
```

Slots A, B and C enter `T` verbatim as posted, MACs included, so `T` covers N's long-term keys and name, both ephemeral keys, the commitment and its opening (V1-6). The relay origin is the one each side is connected to (§5.4). N compares the digits as they were typed, without normalising them.

The ephemeral keys are not HPKE keys: `epk_X` is the RFC 7748 X25519 function of the raw 32 bytes and the base
point, with no `DeriveKeyPair` (§6.4 uses `DeriveKeyPair` only for a device's long-term X25519 key).

- **SEAL-PAIR-006** (seal) `T` and the six SAS digits MUST be computed exactly as §17.7 states.
- **SEAL-PAIR-007** (seal) N MUST compare the typed digits with its own SAS in constant time, MUST allow at most 3 entries, and MUST abort the pairing after the third mismatch.
- **SEAL-PAIR-008** (seal, device) E MUST NOT seal `DeviceAdded` or post slot D before the user has confirmed on E that N accepted the digits.

### 17.8 The bundle (slot D)

```text
bundle_key = HKDF(salt = s, ikm = dh, info = ctx("pair-bundle-key") ‖ T, L = 32)
AD         = ctx("pair-bundle") ‖ board_id[16] ‖ tid[16] ‖ T[32]
slot D body = nonce[24] ‖ XChaCha20-Poly1305-Seal(bundle_key, nonce, AD, bundle JSON)
```

The bundle is JSON (§5.3), at most 16,000 bytes:

| Field | Type | Required | Constraints | Meaning |
|---|---|---|---|---|
| `board_id` | hex, 16 | required | equal to the board N asked for | |
| `host` | string | required | §5.4 | the relay's host (ruling S4) |
| `door` | hex, 5 | required | | the relay's door for this board |
| `roots` | array of object | required | 1 or 2 objects `{"epoch": integer, "root": hex32}`, ascending by `epoch`: the current epoch of `M_h`, which is `snapshot.epoch`, and the epoch before it when the current epoch is 2 or more; never a later epoch (`engine.md` §28.4, ENG-OPS-006) | the current and previous epoch roots |
| `head` | object | required | `{"seq": h, "chain": hex32}` | E's head when it took the snapshot |
| `snapshot` | object | required | `{"blob", "sha256", "size", "parts", "inflated", "epoch", "hash", "hash_version"}` with the constraints of §11.5 (`hash` is the state hash at `h`; `epoch` the current epoch). The object carries no seq: the snapshot's seq is `head.seq`, and §11.5's rule that a snapshot's seq is a multiple of 64 does not apply to it | the fresh snapshot of E's state at `h` (V1-10) |
| `added_seq` | integer | required | `a > h` | the seq of the `DeviceAdded` |
| `token` | string | required | ASCII, 1 to 512 bytes | N's first relay token (§6.7) |

The device set, the recovery public keys, the epochs and their commitments travel inside the snapshot, whose body holds the canonical membership bytes `M_h` and the control history back to seq 1 (§20.4, ruling S27), and whose hash the bundle authenticates. The bundle carries no recovery seed, no private key other than the roots, and no token of E.

- **SEAL-PAIR-009** (seal) The bundle MUST be sealed with `bundle_key` and the AD of §17.8 and MUST hold exactly the fields of its table.
- **SEAL-PAIR-010** (seal, engine) E MUST seal `DeviceAdded` with `based_on_seq = h`, the head at which it took the bundle's snapshot, naming exactly the `dev_N`, `sign_pk_N`, `hpke_pk_N` and `name_N` of slot B, `transcript = T` and `bootstrap_seq = h`.

### 17.9 N's checks on the bundle and the join

In order, aborting on the first failure:

1. slot D's framing, MAC, label `D` and role `E` (`seal/slot_mac`), then its width (`seal/slot_format`);
2. AEAD open under `bundle_key` with the AD of §17.8 (`seal/bundle_open`);
3. the JSON parses strictly (§5.3), and `board_id`, `host` and `door` are the ones N was given with the code (`seal/bundle_invalid`);
4. with the bundle's token, the snapshot downloads and passes §20.5 with `S = h`, writer `dev_E`, the bundle's `sha256`, `size`, `parts`, `inflated`, `epoch`, `hash` and `hash_version`, and `K_snap` from the bundle's root for that epoch (`seal/bundle_invalid`); this use of a bundle root before step 5 has checked it against its commitment is the exception SEAL-KEY-013 states;
5. the snapshot body's `C_S` equals `head.chain`; its membership bytes parse (§13.2), name the same `board_id`, list `dev_E` as a current device, and have as current epoch the highest epoch in `roots`; every root matches its commitment there (§6.5); and `snapshot.hash_version` is one the membership's rules allow (§14.3), and the state hash recomputed from the body under it (`engine.md`) equals `snapshot.hash` (`seal/bundle_invalid`);
6. every entry from `h + 1` to the relay's head is classified in seq order from `M_h` (the snapshot's membership) and `C_h = head.chain`: the entries are consecutive from `h + 1`, and a gap or a seq out of place aborts (`seal/bundle_invalid`); any `Tamper`, an entry signed with N's own new identity (§14.7) and an accepted entry with the commitment halt abort (`seal/bundle_invalid`), and one with the unknown-rules halt aborts (`seal/pair_rules`); `Skip` and `Refuse` continue (the entry at `added_seq` is step 7's); `NeedFrames` is resolved by taking each named frame from the entries already read when its seq is above `h`, and otherwise by fetching it from the relay with the bundle's token (`relay.md` `read` with `after = t − 1`, `limit = 1`), checking that it has seq `t` and the named `frame_hash`, and classifying the same entry again with the frames; a named frame the relay does not serve, or serves with another seq or hash, aborts (`seal/bundle_invalid`); `seal/missing_root` aborts (`seal/pair_raced`: a rotation after `h` of which N is not a recipient);
7. the entry at `added_seq` is accepted, authored by `dev_E`, with `based_on_seq = h`, and holds one `DeviceAdded` naming exactly N's `dev`, `sign_pk`, `hpke_pk`, name and `T`, with `bootstrap_seq = h`; otherwise `seal/device_added_mismatch` (also when the log ends before `added_seq`), or `seal/pair_raced` when that entry was refused;
8. N's `device_no` is the one `M_{added_seq}` assigns it.

A rotation sequenced between `h` and `added_seq` races the pairing in either order. If E sealed the `DeviceAdded` before it classified the rotation, the entry is a stale-epoch control entry and is refused everywhere (§11.1): step 7 aborts with `seal/pair_raced`, and E's engine revokes the registered device (`engine.md` §28.4). If E classified the rotation first, its engine never sends the `DeviceAdded` (`engine.md` ENG-OPS-006) and the pairing fails on E with `seal/pair_raced`. Either way N, which is no recipient of that rotation, meets `seal/missing_root` at the first entry after it (step 6) unless it aborted earlier, and the user pairs again (V2-8). That holds with a relay that serves the log as it is. The checks above bind the log up to `h` (the bundle's head and the snapshot) and the `DeviceAdded` entry itself; nothing E signed binds the entries between `h` and `added_seq`. A relay that replaces one of them, a racing rotation for example, with a frame that N skips or refuses can make N accept a `DeviceAdded` that the board refused, and N then reports `Joined`. E's own result is what tells whether the device was added: its engine reports the pairing failed, also after E's machine has reached `Done`, and has the relay drop the device (`engine.md` §28.4 step 4; showing that report is `device.md`'s, an open point in `CHANGES.md`), and N meets the fork at the first honest entry the relay serves it that is based on the replaced seq or a later one, whose chain it cannot match (§14.8). A bundle that carried `C_{added_seq}` would let N refuse at once; E would have to classify its own entry before it seals the bundle, which is `engine.md`'s to decide (an open point in `CHANGES.md`).

- **SEAL-PAIR-011** (seal) N MUST perform the checks of §17.9 in order and MUST NOT persist the board, append any frame or mint any id before all of them have passed.
- **SEAL-PAIR-012** (seal) N MUST take its `device_no` only from the accepted `DeviceAdded` at `added_seq`, never from the bundle.

### 17.10 State machines

Both roles are sans-IO machines. Inputs arrive as events; the machine answers with effects. Time is a monotonic
millisecond count (`u64`) passed with the inputs that need it: E's `Start` (the call `Existing::start`), N's
`CodeEntered` and `Tick` on both; no other event carries a time. N's machine is made by `New::new()`, in `Idle`, and
draws nothing until `Slot{A}`. §26 gives the events and effects with their fields.

**E (existing device):**

| State | Event | Guard | Effects | Next |
|---|---|---|---|---|
| `Idle` | `Start{membership, head, relay origin, own dev, now}` (the call `Existing::start`) | E is current and unhalted | draw `s`, `esk_E`, `r_E`; `RegisterTicket{tid, slot A}`; `ShowCode{code}` | `Posted` |
| `Posted` | `SlotB{bytes}` | §17.6 passes | `PostSlot{slot C}`; compute `T`, SAS; `ShowSas{digits}` | `Showing` |
| `Showing` | `UserConfirmed` | | `NeedSnapshotAndRegister{device item}` (the engine takes the snapshot at `h`, seals `DeviceAdded`, appends with `register`); the item's `bootstrap_seq` is the head seq of `Start`, a placeholder that the engine replaces with the snapshot's seq (`engine.md` ENG-OPS-001) | `Registering` |
| `Registering` | `Registered{h, chain, snapshot ref, added_seq, token, roots, host, door}` | the values are ones the bundle's table allows (§17.8); otherwise the last row, with `seal/bundle_invalid` | `PostSlot{slot D}`; `Done{dev_N}` | `Done` |
| `Registering` | `RegisterFailed{code}` | | `Abort{code}` | `Aborted` |
| any but `Done`, `Aborted` | `SlotB` failing §17.6, `Cancel`, or `Tick{now}` with `now − start ≥ 600,000` | | `DeleteTicket{tid}`; `Abort{code}` | `Aborted` |

**N (new device):**

| State | Event | Guard | Effects | Next |
|---|---|---|---|---|
| `Idle` | `CodeEntered{code, host, board_id, door, name, now}` (the relay address as `relay.md` parses it; `name` the new device's name, §5.5) | §17.2 check passes | derive keys; `FetchSlot{tid, ticket_key, A}` | `AwaitA` |
| `Idle` | `CodeEntered` | check fails, or after it the host is outside §5.4 (`seal/host_format`) or the name outside §5.5 (`seal/slot_format`) | `AskAgain{code}`; nothing is derived or fetched | `Idle` |
| `AwaitA` | `Slot{A}` | MAC passes | generate long-term keys and `esk_N`; `PostSlot{slot B}`; `FetchSlot{C}` | `AwaitC` |
| `AwaitC` | `Slot{C}` | MAC and commitment pass | compute `T`, SAS; `AskSas` | `AwaitSas` |
| `AwaitSas` | `SasEntered{digits}` | equal | `ShowAccepted`; `FetchSlot{D}` | `AwaitD` |
| `AwaitSas` | `SasEntered{digits}` | unequal, fewer than 3 tries so far | `AskSas` | `AwaitSas` |
| `AwaitD` | `Slot{D}` | §17.9 steps 1 to 3 pass | `FetchSnapshot{token, blob, parts}` | `AwaitSnapshot` |
| `AwaitSnapshot` | `Snapshot{parts}` | §17.9 steps 4 and 5 pass | `ReadLog{token, from h + 1}` | `Joining` |
| `Joining` | `Entries{frames}` | a classification of §17.9 step 6 returns `NeedFrames` for seqs at or below `h` | `FetchFrames{token, seqs}` | `Joining` |
| `Joining` | `Frames{frames}` | each frame has the seq asked for and the `frame_hash` its `Skip` item names | classify the waiting entry again with them and continue §17.9 step 6: `FetchFrames` again as in the row above, or, when steps 6 to 8 pass, `Joined{device_no, keys, bundle, snapshot}` | `Joining`; `Done` after `Joined` |
| `Joining` | `Entries{frames}` | §17.9 steps 6 to 8 pass with no frame left to fetch | `Joined{device_no, keys, bundle, snapshot}` | `Done` |
| any but `Done`, `Aborted` | a failed check, the third SAS mismatch, `Cancel`, or `Tick{now}` with `now − start ≥ 600,000` | | `Abort{code}`; erase `s` and every derived key and the long-term keys generated for this attempt | `Aborted` |

`start` is the `now` of `Start` on E and of the `CodeEntered` that passes the check on N. A slot that is byte for byte one the machine has already accepted is ignored: the relay answers every read of a stored slot with the same bytes (`relay.md` §10.8), and a runtime that both listens and polls can deliver one twice. Any other event for which a machine's state has no row aborts it: a slot with `seal/slot_mac` (its label is not the one awaited); `Snapshot`, `Entries` or `Frames` with `seal/bundle_invalid`; `RegisterFailed{code}` with its `code`; any other with `seal/pair_cancelled`. On E such an abort is the last row of its table (`DeleteTicket{tid}`, `Abort{code}`). A `Tick` before the limit changes nothing, and `Done` and `Aborted` ignore every event. `Existing::start` on a start it must refuse returns an aborted machine and `Abort` alone, with no `DeleteTicket`: `seal/ctl_author` when the own device is not a current device of the membership, `seal/host_format` for a relay origin outside §5.4, `seal/entropy`; whether E is halted is its caller's to know. `Entries` holds the whole log from `h + 1` to the relay's head in one event (paging is the caller's), a second `Entries` aborts, and `Frames` holds exactly the frames asked for, each once.

**Draws.** Each machine draws from the `Entropy` it is given, in this order. E, on `Start`: `s` (10 bytes),
`esk_E` (32), `r_E` (32). N, on `Slot{A}` after the MAC passes: `dev_N` (16), the signing key (32), the X25519
`ikm` (32) of §6.4, then `esk_N` (32). No other value is drawn before slot B is posted.

- **SEAL-PAIR-016** (seal) Each pairing machine MUST draw its random values in the order of §17.10 "Draws": E `s`, `esk_E`, `r_E`; N `dev_N`, the signing key, the X25519 `ikm`, `esk_N`.
- **SEAL-PAIR-017** (seal) A pairing's ephemeral secret key MUST be 32 bytes from `Entropy` used as an RFC 7748 X25519 scalar, and its public key MUST be `X25519(esk, 9)`.

### 17.11 Limits and aborts

| What | Limit |
|---|---|
| ticket lifetime | 10 minutes from registration (relay), and 600,000 ms of local monotonic time on each side |
| tries at the relay | the fifth slot-B post burns the ticket, so at most 4 are examined; what counts as a try is `relay.md` §10.8's |
| SAS entries on N | 3 |
| slots per ticket | one each of A, B, C, D; an identical repeat of a stored slot is answered again and stores nothing; a post of a stored label with other bytes is refused (`relay.md` §10.8) |

Abort codes: a typed code that fails §17.2 is `seal/code_format` or `seal/code_check` (no abort: N asks again); a slot whose framing, MAC, label or role fails, or that its state does not await (other than an identical repeat of a slot it accepted, which is ignored, §17.10), is `seal/slot_mac`; a slot whose layout or limits fail is `seal/slot_format`; an event out of place and the refusals of `Existing::start` are as §17.10 states; a commitment mismatch is `seal/commit_mismatch`; an all-zero `dh` is `seal/dh_zero`; the third SAS mismatch is `seal/sas_mismatch`; the time limit is `seal/pair_timeout`; a user's cancel is `seal/pair_cancelled`; E's checks of §17.6 and N's of §17.9 use the codes listed there.

Every abort erases `s`, `ticket_key`, `mac_key`, `bundle_key`, `dh`, the ephemeral secret keys and the SAS. A pairing is never resumed after an abort; the user starts again with a new code. The code and the digits are text in the API (§26), because they go to a screen and come from a keyboard: the crate erases its own copies, and the caller clears the strings it was handed.

- **SEAL-PAIR-013** (seal, device) Each pairing machine MUST abort when 600,000 ms of the supplied monotonic time have passed since its start without reaching `Done`.
- **SEAL-PAIR-014** (seal) An aborted pairing machine MUST erase every secret it derived or generated, and MUST NOT accept any further event except as `Aborted`.
- **SEAL-PAIR-015** (seal, relay) A ticket id MUST be `H(ctx("pair-tid") ‖ ticket_key)[0..16]` for the `ticket_key` of §17.3, and a check of a ticket proof MUST compute exactly that value from the presented `ticket_key`.

## 18. The recovery sheet

### 18.1 Encoding of the seed

```text
seed bits  = the 256 bits of the seed, then 4 zero bits (260 bits)
chars      = Crockford base32 of the seed (52 characters)
check      = the first 20 bits of H(ctx("sheet-check") ‖ seed)
check chars= Crockford base32 of check (4 characters)
sheet text = chars ‖ check chars (56 characters) in 14 groups of 4; group 14 is the check
```

Printed as seven lines of two groups each, groups separated by a space and numbered `1`–`14` beside them; the exact page is stage `recovery`'s (SYNC_PLAN.md §6.8). `RecoverySeed::to_sheet` (§26) returns the fourteen groups joined by single spaces, 69 characters.

### 18.2 Reading a typed sheet

In order:

1. normalise as §5.2 (spaces, hyphens and tabs removed, case folded, `O`→`0`, `I` and `L`→`1`);
2. require exactly 56 characters (`seal/sheet_length`), then that each is of the alphabet (`seal/sheet_char`);
3. decode the first 52 into 260 bits; the last 4 must be zero (`seal/sheet_padding`);
4. decode the last 4 into 20 bits and compare them with the check of the decoded seed (`seal/sheet_check`).

The check catches a random error with probability `1 − 2^−20`. At setup, the user retypes all 56 characters from one printed copy and the device compares the decoded seed with the one it holds, then opens a test backup with it (V1-17; `engine.md`, `recovery`).

### 18.3 What else the sheet prints

The sheet holds, besides the seed (ruling S4, SYNC.md §5.5):

- the **relay address** (`relay.md`): host and the Crockford base32 of `board_id ‖ door`; it is the "relay host and board id" the recovery tool trusts, never a backup's own fields (V1-13);
- the **recovery key fingerprint** (§6.8) of the key derived from this seed, which backup writers pin;
- the **release-signing key fingerprint** (stage `release`);
- the date it was printed.

- **SEAL-SHEET-001** (seal, recovery) The sheet text MUST be the 52 Crockford base32 characters of the seed followed by 4 characters of the first 20 bits of `H(ctx("sheet-check") ‖ seed)`, in 14 groups of 4.
- **SEAL-SHEET-002** (seal, recovery) Reading a typed sheet MUST follow the four steps of §18.2 in order and MUST reject the input at the first failing step.
- **SEAL-SHEET-003** (seal) The decoded seed and every intermediate buffer of §18 MUST be held only in zeroizing types.

## 19. Sealed local files

### 19.1 Envelope

Every file a device keeps for a board (SYNC.md §4.1, §5.6) is a header followed by one or more sealed records. A file that is rewritten as a whole (a checkpoint) has one record; a file that grows by appends (the log) has one record per append. The contents of each record are `device.md`'s.

**Header** (55 bytes):

| Offset | Field | Width | Type | Value |
|---|---|---|---|---|
| 0 | `magic` | 4 | ASCII | `"TDL1"` |
| 4 | `ver` | 1 | u8 | 1 |
| 5 | `suite` | 1 | u8 | 1 (XChaCha20-Poly1305) |
| 6 | `kind` | 1 | u8 | the file kind (§19.2) |
| 7 | `dev` | 16 | bytes | the device that owns the file |
| 23 | `board_id` | 16 | bytes | |
| 39 | `file_id` | 16 | bytes | random from `Entropy`, drawn when the file is created |
| | **total** | **55** | | |

**Record** (`44 + plaintext length` bytes):

| Field | Width | Type | Value |
|---|---|---|---|
| `ct_len` | 4 | u32 | byte length of `ciphertext`, 16 to 67,108,880; `FileSealer::seal_record` (§26) refuses a plaintext over 67,108,864 bytes with `seal/payload_too_large`, before it draws |
| `nonce` | 24 | bytes | fresh from `Entropy` |
| `ciphertext` | `ct_len` | bytes | XChaCha20-Poly1305 of the record plaintext, with its 16-byte tag |

### 19.2 File kinds

| `kind` | File (SYNC.md §4.1, `device.md` §6.1) | Records |
|---|---|---|
| 1 | `log-<first seq>.tdl`: log segments, frames with seq and chain value | appended |
| 2 | `checkpoint.tdk` | one |
| 3 | `base-<seq>.tdk`: weekly base checkpoints | one |
| 4 | `hashes.tdk`: the state hash at every seq ≡ 0 mod 64 | appended |
| 5 | `outbox.tdo` | appended |
| 6 | `overlay.tdk`: the overlay | one |
| 7 | `identity.tdk`: the device's private keys and the epoch roots it holds | one |
| 8 | `notices.tdk`: notices and refusals | appended |
| 9 | `meta.tdk`: sync metadata | one |
| 10 | `marker.tdk`: a marker, present only while set | one |
| 11 | `history.tdk`: one history record (§21.5) per accepted control entry | appended |
| 12 | `device.tdk`: per-device settings | one |
| 13 | `picture.tdk`: the shared picture as this device shows it | one |
| 14 | `weather.tdk` | one |
| 15 | `draft.tdk`: an unsaved notes draft | one |
| 16 | `feed-<subscription id>.tdk`: one feed's cached body | one |
| 17 | `bootstrap.tdk`: the bootstrap material until the first checkpoint | one |
| 18 | `unsent.tdk`: entries kept during a re-pair | one |
| 19 | `own-picture.tdk`: the phone's own picture, never uploaded (ruling S29) | one |

Kinds 20 to 255 are reserved; a kind is added only through a change to this table. The contents of every kind are `device.md`'s. A file of kind `k` is sealed under `K_file(k)` (§6.6).

### 19.3 Associated data

```text
AD(record i) = ctx("file") ‖ header[55] ‖ u64 i
```

`i` is the record's 0-based position in the file. The AD binds every record to its file kind, its device, its board (V1-12) and its file instance and position, so records cannot be moved between files, reordered or spliced from another copy of the same kind. Replacing a whole file with an older copy of itself is not detected here; `device.md` and `engine.md` detect it through the head and counters.

### 19.4 Reading

In order:

1. the file is at least 55 bytes; the header has `magic = "TDL1"`, `ver = 1`, `suite = 1`, the `kind` the caller expects, and the caller's own `dev` and `board_id` (`seal/file_header`);
2. for each record from `i = 0`: if fewer than 28 bytes remain, or `ct_len` is outside 16 to 67,108,880, or fewer than `28 + ct_len` bytes remain, the rest of the file is a **torn tail** (`seal/file_torn_tail`); otherwise open it with `AD(i)`;
3. a record that fails to open is `seal/file_corrupt` if any byte follows it, and `seal/file_bad_tail` if it is the last bytes of the file.

The reader returns the records that opened, in order, with the outcome for the rest: a clean end, or a torn or bad tail with the count of valid bytes (`FileTail`, §26). A corrupt record is an error and `open_file` then returns no record (`seal/file_corrupt` alone). `device.md` decides what a torn or bad tail means (a write interrupted by a crash, or damage) and whether to truncate.

- **SEAL-FILE-001** (seal, device) A sealed local file MUST consist of the 55-byte header of §19.1 followed by records laid out as §19.1, each sealed under `K_file(kind)` with the AD of §19.3.
- **SEAL-FILE-002** (seal) A sealer MUST draw a new `file_id` whenever a file is created or rewritten as a whole, and MUST number appended records consecutively from 0.
- **SEAL-FILE-003** (seal) A reader MUST follow the steps of §19.4 in order, MUST check each `ct_len` against both its limit and the bytes remaining before allocating, and MUST return `seal/file_corrupt` for a record that fails to open and is followed by more bytes.
- **SEAL-FILE-004** (seal, device) Every record plaintext that holds a secret (kind 7) MUST be returned only in a zeroizing buffer.

## 20. Snapshots and blobs

### 20.1 Blob ids

A blob id is 16 bytes from `Entropy`, drawn by the writer before sealing. Within one sealing call (`seal_snapshot`, and every function of §26 that seals a blob), the `blob_id` is the first value drawn from the call's `Entropy` and the nonce of §20.2 the second; nothing else is drawn in between. It is not a hash of the content: the id is inside the AEAD's AD, so it must exist before the ciphertext does. What makes a blob self-certifying is the SHA-256 of the sealed bytes, published in a signed entry (the `snap` entry, the bundle, or the `core` command that references a picture), and checked before decryption (V1-14).

### 20.2 Sealed layout

| Offset | Field | Width | Type | Value |
|---|---|---|---|---|
| 0 | `magic` | 4 | ASCII | `"TDB1"` |
| 4 | `ver` | 1 | u8 | 1 |
| 5 | `suite` | 1 | u8 | 1 |
| 6 | `blob_kind` | 1 | u8 | 1 snapshot (key `K_snap,e`), 2 picture or other blob (key `K_blob,e`) |
| 7 | `epoch` | 4 | u32 | `e` |
| 11 | `blob_id` | 16 | bytes | §20.1 |
| 27 | `nonce` | 24 | bytes | fresh from `Entropy` |
| 51 | `ciphertext` | rest | bytes | AEAD of the padded body (§20.3), with its 16-byte tag |
| | **total** | **67 + padded length** | | at most 67,108,864 bytes (64 MiB) |

```text
AD  = ctx("blob") ‖ board_id[16] ‖ sealed[0..27] ‖ u64 S ‖ writer_dev[16]
```

`S` is the seq the snapshot captures (§20.4), 1 for a genesis snapshot, and 0 for a blob of kind 2. `writer_dev` is the device that sealed it: the author of the entry that publishes its hash (the `snap` entry, or the `core` command that references a picture); E for a pairing snapshot (§17.9 step 4); and for the genesis snapshot (`S = 1`), whose hash the recovery-authored Genesis entry publishes, the Genesis item's `device.dev`, the first device, which seals and uploads it. A reader takes `writer_dev` from the signed entry or the bundle, never from the relay's blob `author` (`relay.md` §10.7), except in the declared mode of §20.5, where the caller checks it afterwards against the author of the signed `snap` entry. A blob of kind 2 is the other exception: the board's reference (`core.md` §6.9) names no writer, so the reader takes the relay's `author` (`device.md` §14.2 step 3); the `sha256` that the signed command carries fixes the sealed bytes, so a wrong `author` only makes the open fail. The AD binds board, kind, epoch, id, position and writer, so a relay working with a revoked device's old key cannot pass one blob off as another (V1-12).

### 20.3 Padded body

```text
padded = u32 body_len ‖ body ‖ 0x00 × (P(4 + body_len) − 4 − body_len)
```

`P` is the function of §10.2. For a snapshot, `body` is the compressed snapshot body of §20.4; for a picture it is the JPEG bytes (`device.md`). `seal_blob` refuses a body whose sealed bytes would exceed 67,108,864 bytes with `seal/payload_too_large`, before it draws.

### 20.4 Snapshot body

Before compression:

| Field | Width | Type | Value |
|---|---|---|---|
| `magic` | 4 | ASCII | `"TDSN"` |
| `S` | 8 | u64 | the seq captured |
| `chain` | 32 | bytes | `C_S` |
| `hash_version` | 4 | u32 | |
| `state_hash` | 32 | bytes | the state hash at `S` (ruling S7, composed as `engine.md` says) |
| `board` | 4 + n | vb32 | the canonical board bytes at `S` (`core.md`) |
| `membership` | 4 + n | vb32 | the canonical membership bytes `M_S` (§13.2) |
| `history` | 4 + Σ | u32 count, then records | `n_hist`, then `n_hist` history records (§21.5), one for each seq in `M_S.ctl_seqs`, ascending, the first at seq 1 (ruling S27) |

The history lets a device that bootstraps from this snapshot (a pairing's, or a periodic one after compaction or purge) write backups whose history starts at seq 1 (SEAL-BKP-002), although it never received the control entries below `S`. It counts toward the inflated limit of 67,108,864 bytes.

The body is compressed with raw DEFLATE (RFC 1951, `flate2`), and the result is `body` in §20.3. Its uncompressed length is at most 67,108,864 bytes: `seal_snapshot` refuses a longer body, or one whose sealed bytes would exceed the sealed-blob maximum of 67,108,864 bytes (§23), with `seal/snapshot_large`, and produces no sealed bytes (SEAL-BLOB-009). Near that maximum `P` rounds up to a multiple of 1,048,576, so the largest compressed body (and the largest body of a kind-2 blob) that fits is 66,060,284 bytes; its sealed blob is 66,060,355 bytes. `seal_snapshot` also refuses, before it draws, membership bytes that do not parse (`seal/membership_format`) and, with `seal/blob_open`, a membership of another board, a history whose seqs are not exactly the membership's `ctl_seqs` (SEAL-BLOB-006), and a history record with a seq above `S`. It does not run the projection and does not judge `hash_version` or `state_hash`: a body with such a fault is sealed; its reader refuses a `state_hash` or `hash_version` that differs from the signed one or that the body's board and membership do not give (§20.5 step 7), and whether the rules allow a version is judged by §17.9 step 5 and by the engine.

### 20.5 Opening, in order

Given the parts and the expected values from the signed entry or the bundle (`sha256`, `size`, `parts`, `inflated`, `epoch`, `S`, `state_hash`, `hash_version`, writer):

1. there are exactly `parts` parts, each 1,048,576 bytes except the last, which is 1 to 1,048,576 bytes; together `size` bytes, and `size` is 67 to 67,108,864 (`seal/blob_size`);
2. `H(concatenation) = sha256`, in constant time (`seal/blob_hash`): nothing is decrypted before this;
3. the header has `magic = "TDB1"`, `ver = 1`, `suite = 1`, the expected `blob_kind` and `epoch` (`seal/blob_header`); the header's `blob_id` is not compared with the reference's, because the signed `sha256` fixes every sealed byte and the id is what the caller fetches by;
4. the AEAD opens with the AD of §20.2 under the kind's key for that epoch (`seal/blob_open`);
5. the padding is valid as §10.3 checks it, with `U = 4 + body_len` (`seal/blob_open`);
6. for a snapshot: inflate with an output cap of `inflated` bytes; more output, or input left after the end of the DEFLATE stream, or output shorter than `inflated`, fails (`seal/inflate`);
7. for a snapshot: the body parses as §20.4 with no trailing bytes; `S`, `hash_version` and `state_hash` equal the expected values, which `open_snapshot` is given from the signed entry or the bundle (§26), and the state hash composed as `engine.md` §18.3 states, with the expected `hash_version` in its version field, SHA-256 of the body's `board` bytes as they stand as the board hash and SHA-256 of its `membership` bytes as the membership hash, equals the expected one too (the crate does not judge here whether the rules allow that version); the membership bytes parse; no history record has a seq above `S`; the history verifies by the projection steps of §21.6 (with the receiver's own `board_id` in place of the sheet's), and the history agrees with `membership` as §21.6 step 11 states (the same board id, the same devices with the same keys and statuses, the same current recovery key, the same epochs and commitments, the same `ctl_seqs`) (`seal/blob_open`). A body whose board bytes do not give the signed state hash is therefore refused here, whoever the caller is; an engine that recomputes the hash from the board it loads (`engine.md`) repeats the check. The declared mode makes neither comparison (below): there the engine's check is the only one.

For a blob of kind 2, steps 6 and 7 do not apply and `expected.inflated` is not read.

Splitting: part `i` (0-based) is bytes `i × 1,048,576` to `min((i + 1) × 1,048,576, size)` of the sealed bytes; `parts = ceil(size / 1,048,576)`. The relay stores parts by `(blob_id, i)` (`relay.md`).

**Opening a declared snapshot** (a rejoin, `engine.md` §19.7). A device that has not yet classified the announcing `snap` entry opens a snapshot from what the relay declares about the blob (`relay.md` §10.7): its `sha256`, its `size`, the seq `S`, and its `author`, used as `writer_dev`. The steps are those above, with these differences: step 1 uses the declared `size` and `parts = ceil(size / 1,048,576)`; step 2 the declared `sha256`; step 3 checks `magic`, `ver`, `suite` and `blob_kind = 1`, and reads `epoch` from bytes 7 to 11; step 4 takes the root of that epoch from the caller's roots (none: `seal/missing_root`) and derives `K_snap` from it; step 6 inflates under a cap of 67,108,864 bytes, with no expected length; step 7 checks `S` against the declared seq and compares neither `hash_version` nor `state_hash`. The reader returns the body together with the header's epoch and the inflated length. No declared value is trusted: the caller checks the seq, the state hash and its `hash_version`, `sha256`, `size`, `parts`, `inflated`, `epoch` and the writer against the signed `snap` entry once it has classified it (`engine.md` §19.7 step 5).

- **SEAL-BLOB-001** (seal) A blob MUST be sealed as one AEAD message laid out as §20.2, with the AD of §20.2, before it is split into parts of at most 1,048,576 bytes as §20.5 states.
- **SEAL-BLOB-002** (seal) A reader MUST check the SHA-256 of the concatenated parts against the value from the signed entry before any decryption, and MUST reject a mismatch.
- **SEAL-BLOB-003** (seal) A reader MUST inflate a snapshot under an output cap equal to the signed `inflated` value, at most 67,108,864 bytes, and MUST fail when the stream would exceed it.
- **SEAL-BLOB-004** (seal) A snapshot body MUST be laid out as §20.4 and compressed with raw DEFLATE before padding and sealing.
- **SEAL-BLOB-005** (seal) A writer MUST draw a fresh `blob_id` from `Entropy` for every blob it seals.
- **SEAL-BLOB-010** (seal) A function that seals a blob MUST draw the `blob_id` (16 bytes) and then the nonce (24 bytes) from its `Entropy` argument, in that order, and MUST draw nothing else between or before them.
- **SEAL-BLOB-006** (seal) A writer MUST put in a snapshot body's `history` exactly one history record (§21.5) for each seq in its membership's `ctl_seqs`, in seq order, starting at seq 1.
- **SEAL-BLOB-007** (seal) A reader MUST reject with `seal/blob_open` a snapshot whose history fails a projection step of §21.6 or disagrees with the snapshot's membership as §20.5 step 7 states.
- **SEAL-BLOB-008** (seal) A reader opening a declared snapshot MUST check the parts against the declared `size` and SHA-256 before any decryption, MUST derive the key only from a root it already holds for the epoch in the sealed header, MUST open the AEAD before it parses any field of the body, and MUST inflate under a cap of 67,108,864 bytes.
- **SEAL-BLOB-009** (seal) A writer MUST refuse with `seal/snapshot_large`, and MUST produce no sealed bytes, when a snapshot body laid out as §20.4 exceeds 67,108,864 bytes before compression or its sealed blob would exceed 67,108,864 bytes.

## 21. Backups and segments

### 21.1 Purpose

A nightly backup (`TaskDeck-<date>-<seq>-<tag>.tdbk`, ruling S28; names and the writer tag are `device.md` §15.3's) holds the state at a head; a monthly segment holds a month's frames (SYNC.md §6.6). Both are HPKE-sealed to the recovery key, so only the sheet opens them and their writer cannot read them back, and both are signed by their writer's device key and carry the signed membership history back to the self-certifying `board_id`, so a forged file is rejected (V1-13). The file names, the schedule and the destinations are `device.md`'s.

### 21.2 File layout

| Offset | Field | Width | Type | Value |
|---|---|---|---|---|
| 0 | `magic` | 4 | ASCII | `"TDBK"` |
| 4 | `ver` | 1 | u8 | 1 |
| 5 | `suite` | 1 | u8 | 1 |
| 6 | `file_type` | 1 | u8 | 1 nightly backup, 2 monthly segment |
| 7 | `board_id` | 16 | bytes | |
| 23 | `writer_dev` | 16 | bytes | the writing device |
| 39 | `writer_sign_pk` | 32 | bytes | its signing key |
| 71 | `recipient` | 16 | bytes | `dev_R` of the recovery key it is sealed to |
| 87 | `seq` | 8 | u64 | nightly: the head seq; segment: the last seq it holds |
| 95 | `written_ms` | 8 | i64 | the writer's clock; display only |
| 103 | `enc` | 32 | bytes | HPKE encapsulated key |
| 135 | `ct_len` | 8 | u64 | byte length of `ct` |
| 143 | `ct` | `ct_len` | bytes | HPKE ciphertext of the compressed payload, with its 16-byte tag |
| 143 + `ct_len` | `sig` | 64 | bytes | §16.3 over bytes 0 to `143 + ct_len` |
| | **total** | **207 + `ct_len`** | | at most 268,435,456 bytes (256 MiB) |

The **file body** of §16.3 is bytes 0 to `143 + ct_len`, the header and `ct`.

### 21.3 Sealing to the recovery key

```text
info = ctx("backup-hpke") ‖ board_id[16] ‖ u64 seq ‖ writer_dev[16] ‖ recipient[16] ‖ rhpke_pk[32]
aad  = ctx("backup-aad") ‖ file[0..143]
(enc, ct) = HPKE base mode to rhpke_pk with info and aad; plaintext = raw DEFLATE (RFC 1951) of the payload (§21.5)
```

HPKE base mode gives no sender authentication, which is why the file is also signed (§16.3). The writer uses the recovery public key whose fingerprint (§6.8) the user confirmed on this device's own screen, and refuses to write when the membership's current recovery key differs from that pinned one, until the user confirms the new fingerprint there (V1-2). `write_backup` (§26) also refuses the inconsistencies it can see without the recovery key and without replaying the history: membership bytes that do not parse (`seal/membership_format`); a writer that is not a current device of the given membership with its own signing key (`seal/backup_writer`); a history whose seqs are not exactly the membership's `ctl_seqs` (`seal/backup_history`, SEAL-BKP-002); a history of more than 1,048,576 records (`seal/backup_history`); and, with `seal/backup_payload`, a membership of another board, a nightly head other than `seq`, a segment whose `first_seq` is 0, whose frames are not the seqs `first_seq` to `seq`, or whose history reaches `first_seq`, a frame, a frame count, a token or `meta` outside its limit (§21.5, §23), two roots for one epoch, a root that does not match its commitment, and a payload or a file over its limit (§23). Beyond the `ctl_seqs` and a segment's `first_seq`, it does not verify the history's signatures or rules, the membership's agreement with the history, `hash_version` or `state_hash`: a file built from such inputs is written, and its reader rejects it at step 9 or 11 of §21.6. `write_backup` keeps the epoch roots and the read token out of the DEFLATE compressor, whose buffers no caller can erase (SEAL-GEN-006): it compresses the bytes before `n_roots` and ends them with a sync or full flush (byte-aligned, no final bit), writes the bytes from `n_roots` to the end of `read_token` itself as stored blocks without the final bit (RFC 1951 §3.2.4, at most 65,535 bytes each), and compresses `meta` with a fresh compressor, whose last block carries the final bit. This block layout is not part of the format: a reader inflates the stream as any other and does not depend on it, and another writer meets SEAL-GEN-006 by any means that leaves no copy of these bytes behind. The crate's reader resets its decompressor before dropping it, which in the pinned `miniz_oxide` zeroes its 32 KiB window. What a dependency keeps in its own state stays out of the crate's reach; of the decompressor that is its bit buffer, a few bytes of input.

### 21.4 What is signed and what is sealed

| Part | Signed by the writer | Sealed to the recovery key |
|---|---|---|
| header (bytes 0 to 142) | yes | in the HPKE aad |
| payload | yes, as ciphertext | yes |

### 21.5 Payload

Inflated, the payload is (all integers big-endian):

| Field | Width | Type | Present in | Rule |
|---|---|---|---|---|
| `magic` | 4 | ASCII | both | `"TDBP"` |
| `version` | 4 | u32 | both | 1 |
| `file_type` | 1 | u8 | both | equal to the header's |
| `n_hist` | 4 | u32 | both | 1 to 1,048,576 history records follow, ascending by seq, the first at seq 1 |
| history records | | | both | below |
| `membership` | 4 + n | vb32 | both | nightly: `M_seq`; segment: `M_{first_seq − 1}` |
| `board` | 4 + n | vb32 | nightly | the canonical board bytes at `seq` (`core.md`) |
| `head_chain` | 32 | bytes | nightly | `C_seq` |
| `hash_version` | 4 | u32 | nightly | |
| `state_hash` | 32 | bytes | nightly | the state hash at `seq` (ruling S7) |
| `first_seq` | 8 | u64 | segment | the first seq the segment holds |
| `chain_before` | 32 | bytes | segment | `C_{first_seq − 1}` |
| `n_frames` | 4 | u32 | segment | `seq − first_seq + 1`, at most 1,048,576 |
| frames | | | segment | `n_frames` × `vb32(frame)`, for the consecutive seqs `first_seq` to `seq` |
| `n_roots` | 2 | u16 | both | then `n_roots` × `(u32 e ‖ R_e[32])`, ascending by `e`: every root the writer holds whose commitment the file's `membership` records. `write_backup` is given every root the device holds (`device.md` §15.3) and leaves out those of epochs for which that membership has no commitment: a segment's membership is `M_{first_seq − 1}`, so the roots of epochs rotated inside the segment are not here (they are in those rotations' wraps to the recovery key), nor is a root drawn for a rotation that is not yet accepted. It refuses a root that does not match its commitment (`seal/backup_payload`), and a reader rejects a root without a matching commitment in that membership |
| `read_token` | 2 + n | vb16 | both | the writer's backup-read token (§6.7), per writer and generation (V1-2); `n = 0` means none, written only by the test backup of a board being created, which holds no token yet (`device.md` §18.1 step 6); a reader treats it as absent |
| `meta` | 4 + n | vb32 | both | sync metadata, canonical JSON owned by `device.md`, at most 1,048,576 bytes |

The payload has no trailing bytes.

**History record** (156 bytes plus the items):

| Field | Width | Type | Value |
|---|---|---|---|
| `seq` | 8 | u64 | the entry's seq |
| `dev` | 16 | bytes | its author |
| `ctr` | 8 | u64 | its counter |
| `entry_id` | 16 | bytes | |
| `at_ms` | 8 | i64 | |
| `frame_hash` | 32 | bytes | `H(frame)`, for checking against a relay copy |
| `items` | 4 + n | vb32 | the canonical encoding of the entry's `items` array (§11.3), at most 262,144 bytes |
| `sig` | 64 | bytes | the entry's inner signature (§11.3) |

A writer builds each record from an accepted `ctl` entry with `HistoryRecord::from_accepted` (§26), keeps its bytes as `to_bytes` gives them, and reads them back with `from_bytes`, which rejects bytes that are not exactly one record of this table with `seal/backup_history`. `from_accepted` refuses a frame whose header does not parse with `seal/frame_format`, an opened entry that is not of kind `ctl` with `seal/not_control` (SEAL-BKP-010), and a control payload that is not §11.3's with `seal/bad_payload`.

The history holds one record for every accepted control entry from seq 1 to the file's `seq` (or `first_seq − 1` for a segment), in seq order: exactly the seqs in the membership's `ctl_seqs`. It needs no epoch root to verify, so a purge that deletes old roots leaves it verifiable. A record after the file's `seq` fails step 9 of §21.6, and one at or after a segment's `first_seq` fails step 11; so a segment has `first_seq ≥ 2`.

### 21.6 Reading, in order

The reader has, from the sheet and never from the file, the seed (hence the recovery keys and `dev_R`) and the `board_id` in the relay address (V1-13).

1. `223 ≤ size ≤ 268,435,456` (`seal/backup_size`).
2. The fixed header: `magic = "TDBK"`, `ver = 1`, `suite = 1`, `file_type` 1 or 2, `ct_len = size − 207` (`seal/backup_header`).
3. `board_id` equals the sheet's (`seal/backup_board`).
4. `recipient` equals `dev_R` of the typed seed (`seal/backup_recipient`).
5. `sig` verifies (strict) under `writer_sign_pk` over `ctx("backup") ‖ H(file[0 .. 143 + ct_len])` (`seal/backup_signature`).
6. HPKE `OpenBase` with the seed's recovery key and the `info` and `aad` of §21.3, rebuilt from the header and the seed's public key (`seal/backup_open`).
7. Inflate under a cap of 536,870,912 bytes (512 MiB); more output or input after the end of the stream fails (`seal/inflate`).
8. Parse `magic`, `version`, `file_type` and the history records alone, checking each length before reading (`seal/backup_payload`).
9. Verify the history and build the projection (below), and check that no record's `seq` is above the header's `seq` (`seal/backup_history`).
10. `writer_dev` is a current device of the projection with `writer_sign_pk`, and the projection's current recovery identity is the sheet's: its `dev_R` is `recipient`, and its two public keys are those of the typed seed (`seal/backup_writer`). `dev_R` binds the signing key alone, so the HPKE key is compared as well: a history whose last recovery-key change pairs the sheet's signing key with another HPKE key is rejected, so the state a restore starts from never holds a recovery key the sheet does not hold (SEAL-BKP-004). For a segment the projection ends at `first_seq − 1`, so its writer must be a current device at that seq; a writer chooses the start of its segments so that it is (`device.md` §15.2, DEV-BKP-005).
11. Parse the rest strictly. The `membership` bytes parse (§13.2) and agree with the projection: the same board id, the same devices with the same keys and statuses, the same current recovery key, the same epochs and commitments, the same `ctl_seqs`. Every root has a commitment in `membership` and matches it. For a nightly file, `hash_version` is one the membership's rules allow (§14.3), and the state hash recomputed from `board` and `membership` (`engine.md`) equals `state_hash`. For a segment, the frames' chain from `chain_before` is recomputed (§15.1) and each frame is at most 262,144 bytes; the file carries no value to compare the chain with, so that comparison is the caller's, who chains segments and nightly files (`seal/backup_payload`).

**Projection.** Starting empty, for each history record in order:

1. its `seq` is above the previous record's, and the first record has seq 1;
2. its `dev` is an authority in the projection so far (a current device, or the current recovery identity), and for the first record `dev` is `dev_R` of the Genesis item's recovery key (the first record's items are parsed at this step to find that key; step 6 binds it to the board id);
3. its `ctr` is above the previous record's `ctr` for the same `dev` (gaps are normal: other entries are not in the history), and the first record's `ctr` is 1 (SEAL-MEM-004);
4. its `sig` verifies (strict) under that author's `sign_pk` over the inner message of §11.3, with the record's `dev`, `ctr`, `entry_id`, `at_ms` and `H(items)`;
5. its items parse (§12) and form a shape (§12.1);
6. the items change the projection by the reduced rules: Genesis (first record only) sets the board, checks `board_id` against the sheet as §6.2 and that its wraps name exactly the first device and `dev_R`, and adds the first device and the recovery key; `DeviceAdded`, `DeviceRevoked`, `DeviceReinstated`, `EpochRotated` and `RecoveryKeyChange` run their rows of §13.6 whole, because control entries alone decide every precondition of those rows (fresh keys and ids, the caps, the authors, the targets' statuses, the last current device, the next epoch number, the wraps' recipients, `purge_before`); `Fence`, `ResetTo`, `Skip` and `RulesUpgrade` only check their Author column.

The checks that need non-control entries (counters of other kinds, beats, the log clock) cannot be repeated from the history; a backup is trusted for them because its writer was a member, which steps 9 and 10 prove. They prove that the writer was a current device at the last control entry of the file's history, and nothing later. No field says at which seq a membership was taken, so a device revoked at seq `k` can still write a file that claims any `seq`, before or after `k`, with its history cut before `k` and any board content. No check inside a file can tell such a file from an authentic one written before the revocation, and its `seq` and `written_ms` are the writer's own claims. The recovery tool therefore shows the writer, `written_ms` and the seq of the last control entry (`VerifiedBackup`'s `writer`, `written_ms` and the last record of its `history`, §26), and trusts no file over another for its `seq` or its date alone. A relay copy can show more: a control entry at or below the file's `seq` that the history lacks, or a frame whose hash differs from a record's `frame_hash`. Whether the recovery tool makes that comparison is `device.md`'s to state (§15.8; an open point in `CHANGES.md`).

**Checking a file one wrote.** `verify_backup_file` (§26) runs steps 1, 2 and 5 only, under the writer's own signing key, so that a writer checks the file it wrote without the recovery key (`device.md` §15.3.2). It also requires the header's `writer_sign_pk` to equal that key (`seal/backup_signature`; compared after steps 1 and 2 and before step 5), returns the header fields, and opens nothing.

- **SEAL-BKP-001** (seal, recovery) A backup or segment file MUST be laid out as §21.2, sealed as §21.3, and signed as §16.3.
- **SEAL-BKP-002** (seal, recovery) The payload MUST be laid out as §21.5, and its history MUST hold exactly one record for each seq in the membership's `ctl_seqs`, in seq order.
- **SEAL-BKP-003** (seal, recovery) A reader MUST perform the steps of §21.6 in order and MUST reject the file at the first failing step.
- **SEAL-BKP-004** (seal, recovery) A reader MUST take `board_id` and the recovery key from the sheet and MUST NOT use any relay host, board id or recovery key from the file in their place (V1-13). At step 10 of §21.6 it MUST reject with `seal/backup_writer` a file whose projection's current recovery record has a signing key or an HPKE public key other than the typed seed's.
- **SEAL-BKP-005** (seal, recovery) A reader MUST NOT parse the payload's membership, board, frames, roots, token or metadata before the history has been verified and the writer has passed step 10.
- **SEAL-BKP-006** (seal, recovery) A writer MUST seal a backup only to the recovery public key pinned on the writing device, and MUST refuse with `seal/backup_unpinned` when the membership's current recovery key differs from it.
- **SEAL-BKP-007** (seal, recovery) The writer's interface MUST NOT retain or return the HPKE context, the plaintext payload after sealing, or any key that opens the file, and the `seal` crate MUST NOT offer any function that opens a backup with a device key.
- **SEAL-BKP-008** (seal, recovery) The projection MUST be built by the six steps of §21.6, and a history record that fails any of them MUST reject the file.
- **SEAL-BKP-009** (seal) `verify_backup_file` MUST run exactly steps 1, 2 and 5 of §21.6, in that order, MUST reject with `seal/backup_signature` a file whose header's `writer_sign_pk` differs from the given key, and MUST NOT open, inflate or parse any byte of `ct`.
- **SEAL-BKP-010** (seal) `HistoryRecord::from_accepted` MUST return the record of §21.5 with `seq` the given seq, `dev` and `ctr` from the frame header, `entry_id` and `at_ms` from the opened plaintext, `frame_hash = H(frame)`, `items` the canonical encoding of the payload's `items` and `sig` its inner signature (§11.3), and MUST refuse with `seal/not_control` an opened entry whose kind is not `ctl`.

## 22. Error catalogue

"Receipt" codes are returned with a receipt class (§14.4); the engine acts on the class, and shows the code in evidence. The caller of every other code is the function's caller.

| Code | Meaning | Raised when | What the caller does |
|---|---|---|---|
| `seal/frame_format` | not a well-formed frame, or a frame asked for with `ctr = 0` | §14.2 step 1; `seal_frame` with `ctr = 0` (SEAL-FRAME-004); `HistoryRecord::from_accepted` (§21.5) | class `Skip`; tampering alarm (`engine.md`); from `seal_frame`: a fault of the caller, nothing is sealed; from `HistoryRecord::from_accepted`: a bug of the caller |
| `seal/wrong_board` | frame of another board | step 2 | `Skip`; tampering alarm |
| `seal/unknown_author` | no key of this board signed it; when signing, a recovery signer of another board than the call's | step 3; `seal_frame`, `ctl_payload`, `sign_admin` (§26) | `Skip`; tampering alarm; when signing, nothing is signed (a bug of the caller) |
| `seal/bad_signature` | signature fails under the author's key | step 4 | `Skip`; tampering alarm |
| `seal/counter_replay` | `ctr` at or below the author's last | step 5 | `Tamper`; stop and show evidence (`engine.md`) |
| `seal/counter_gap` | `ctr` skips values | step 5 | `Tamper`; stop and show evidence |
| `seal/revoked_author` | signed by a revoked device or a retired recovery key | step 7 | `Refuse`; alarm naming the author |
| `seal/bad_version` | (`ver`, `suite`) not allowed by the active rules | step 8 | `Refuse`; alarm naming the author |
| `seal/bad_epoch` | epoch above the current one (at seq 1, not 1) | step 9, §9.3 | `Refuse`; alarm naming the author |
| `seal/old_epoch` | epoch older than the previous one, among them a control entry that lost a race to two or more rotations (§13.8) | step 9, §9.3 | `Refuse`; the author re-seals a `cmd` (`engine.md` §24.3); an own control entry fails or repeats its operation (`engine.md` §28); no alarm |
| `seal/missing_root` | the receiver lacks a root for an accepted epoch, or holds one that does not match the commitment its membership records (SEAL-KEY-013), or lacks one for a declared snapshot's epoch | step 9; §20.5 (declared mode); §17.9 step 6 | local fault; `engine.md` decides; a rejoin tries its next candidate; in pairing N aborts with `seal/pair_raced` |
| `seal/aead` | ciphertext does not open | step 10 | `Refuse`; alarm naming the author |
| `seal/bad_plaintext` | plaintext layout, padding, kind byte or `at_ms` range wrong; when sealing, a `based_on_chain` other than `C_0` for `based_on_seq` 0, an `at_ms` out of range or an empty `cmd` payload | step 11, §10.3; `seal_frame` (§26), SEAL-PLAIN-003 | `Refuse`; alarm naming the author; when sealing, nothing is sealed (a bug of the caller) |
| `seal/based_on_future` | `based_on_seq ≥ s` | step 12, §14.8 | `Tamper`; stop and show evidence |
| `seal/based_on_fork` | `based_on_chain` differs from the local chain | step 12, §14.8 | `Tamper`; stop and show evidence |
| `seal/not_authorized` | author not allowed for the kind | step 13, §11.1 | `Refuse`; alarm naming the author |
| `seal/stale_epoch` | previous-epoch entry outside §13.9, among them a control entry that lost a race to one other rotation (§13.8) | steps 9, 13 and 14 | `Refuse`; the author re-seals a `cmd` (`engine.md` §24.3); an own control entry fails or repeats its operation (`engine.md` §28) |
| `seal/bad_payload` | `ctl`, `beat` or `snap` payload invalid; when building, an item or payload its table excludes, or an admin argument the registry excludes | step 15; `ctl_payload`, `beat_payload`, `snap_payload`, `admin_message`, `sign_admin` (§26); `HistoryRecord::from_accepted` (§21.5) | `Refuse`; alarm naming the author; when building, nothing is produced (a bug of the caller) |
| `seal/bad_ctl_sig` | inner control signature fails | step 15, §11.3 | `Refuse`; alarm naming the author |
| `seal/ctl_shape` | items form no shape of §12.1 | step 16; `ctl_payload` (§26) | `Refuse`; alarm naming the author; from `ctl_payload`, nothing is produced (a bug of the caller) |
| `seal/ctl_genesis` | a Genesis item or anchor is wrong | step 16, §13.3 | `Refuse`, or at seq 1 the board is unusable: the caller stops bootstrapping |
| `seal/ctl_author` | the item's author is not allowed; `Existing::start` for a device that is not current | §13.6, §17.10 | `Refuse`; alarm naming the author; from `Existing::start`, an aborted machine |
| `seal/ctl_target` | the item's target or seq is not allowed | §13.6 | `Refuse` |
| `seal/ctl_duplicate_key` | a new key or id is already recorded | §13.6 | `Refuse` |
| `seal/ctl_duplicate` | a second override vote by the same voter | §13.6 | `Refuse` |
| `seal/ctl_device_cap` | device limits of §23 reached | §13.6, §17.6 | `Refuse`; pairing aborts on E |
| `seal/ctl_cap` | a membership list would exceed its limit | §13.6 | `Refuse` |
| `seal/ctl_epoch` | a rotation sealed under the current epoch names an epoch other than the next | §13.6, §13.8 | `Refuse`; its author discards its root (a rotation that lost a race is refused with `seal/stale_epoch` or `seal/old_epoch`, §13.8) |
| `seal/ctl_wraps` | rotation wraps do not match the recipients | §13.6 | `Refuse` |
| `seal/ctl_last_device` | a revocation would leave no current device | §13.6 | `Refuse` |
| `seal/ctl_home_gen` | Fence's home generation is not the next | §13.6 | `Refuse` |
| `seal/ctl_support` | an active member does not support the version | §13.6, §13.10 | `Refuse`; the UI names the blocker (`engine.md`) |
| `seal/commitment` | own wrap fails or mismatches its commitment | §14.5; every failure of `unwrap_root` (§7) | `Accept` with the commitment halt; from `unwrap_root`, the caller treats it as that halt (SEAL-WRAP-003) |
| `seal/unknown_rules` | an activated rules version this build lacks | §14.5 | `Accept` with the rules halt |
| `seal/payload_too_large` | payload over the padding cap; a file record's plaintext over 67,108,864 bytes; a kind-2 blob whose sealed bytes would exceed 67,108,864 | sealing, §10.2; `FileSealer::seal_record`, §19.1; `seal_blob`, §20.3 | the composer splits or refuses the edit; a file record over the limit, or a body handed to `seal_blob` over 66,060,284 bytes, is a bug of the caller and nothing is sealed |
| `seal/entropy` | the entropy source failed | any sealing or generation | retry later; never fall back to another source |
| `seal/membership_format` | canonical membership bytes invalid | §13.2; `write_backup` (§21.3); `seal_snapshot` (§20.4) | reject the input that carried them |
| `seal/challenge_length` | relay nonce not 32 bytes | §16.1, §16.2 | do not sign; treat the relay answer as invalid |
| `seal/host_format` | a host is not a host of §5.4 (a port of 443 included) | `relay_origin`, `login_message`, `admin_message`, `sign_admin`, `ctl_payload`, `Existing::start`, `CodeEntered` (§17.10), §5.4 | refuse the host; ask the user again |
| `seal/move_code` | typed move code does not match | §15.4 | ask again (`engine.md` limits tries) |
| `seal/code_format` | pairing code not 18 alphabet characters; any failure of `base32::decode_typed` | §17.2, §26 | ask again; no request sent; from `decode_typed`, the caller maps it to its own code |
| `seal/code_check` | pairing code check characters wrong | §17.2 | ask again; no request sent |
| `seal/slot_mac` | slot framing, MAC, label or role fails; a slot the machine's state does not await, other than an identical repeat of one it accepted (ignored, §17.10) | §17.4, §17.9, §17.10 | abort the pairing |
| `seal/slot_format` | slot layout or limits wrong; in `CodeEntered`, a device name outside §5.5 | §17.4, §17.6, §17.10 | abort the pairing; for the name, N asks again |
| `seal/commit_mismatch` | slot C does not open slot A's commitment | §17.5 | abort the pairing |
| `seal/dh_zero` | X25519 output all zero | `wrap_root` (§7), §17.7, SEAL-GEN-013 | abort the pairing; `wrap_root` produces no wrap |
| `seal/sas_mismatch` | third wrong SAS entry on N | §17.7 | abort the pairing |
| `seal/pair_keys_conflict` | N's keys or id already recorded | §17.6 | abort; N generates new keys on the next attempt |
| `seal/pair_rules` | N does not support the active rules | §17.6 | abort; the user updates N |
| `seal/pair_timeout` | 10 minutes passed | §17.10 | abort |
| `seal/pair_cancelled` | the user cancelled; an event out of place that is no slot, snapshot, entries, frames or `RegisterFailed` (§17.10) | §17.10 | abort |
| `seal/bundle_open` | bundle does not open | §17.9 | abort |
| `seal/bundle_invalid` | bundle, snapshot or log check fails; `Registered` with values the bundle's table excludes; `Snapshot`, `Entries` or `Frames` out of place | §17.9, §17.10 | abort |
| `seal/device_added_mismatch` | the sequenced DeviceAdded differs from N's keys | §17.9 | abort; the user revokes the added device on E |
| `seal/pair_raced` | a rotation raced the pairing: E's DeviceAdded was refused or never sent, or N meets an entry under an epoch whose root the bundle does not hold | §17.9 steps 6 and 7, `engine.md` §28.4 | abort; pair again (when E's DeviceAdded was sent and refused, E's engine first sends `revoke` for the new device, `engine.md` §28.4) |
| `seal/sheet_length` | not 56 characters after normalising | §18.2 | ask again |
| `seal/sheet_char` | a character outside the alphabet | §18.2 | ask again, pointing at the group |
| `seal/sheet_padding` | the 4 padding bits are not zero | §18.2 | ask again |
| `seal/sheet_check` | check characters wrong | §18.2 | ask again |
| `seal/file_header` | sealed file header wrong | §19.4; `FileSealer::resume` (§26) | treat the file as not this device's (`device.md`) |
| `seal/file_torn_tail` | incomplete last record | §19.4 | `device.md` truncates after the good records |
| `seal/file_bad_tail` | last record does not open | §19.4 | `device.md` decides: truncate or alarm |
| `seal/file_corrupt` | a record followed by others does not open; identity bytes of another length than 80, or with an X25519 key that is not clamped | §19.4; `DeviceKeys::from_identity_bytes` (§26) | tampering or damage alarm (`device.md`) |
| `seal/blob_size` | parts or size wrong, a size under 67 bytes among them | §20.5 | refetch once, then alarm naming the writer |
| `seal/blob_hash` | sealed bytes do not match the signed hash | §20.5 | alarm naming the writer; never decrypt |
| `seal/blob_header` | blob header wrong | §20.5 | alarm naming the writer |
| `seal/blob_open` | blob or snapshot body fails to open or parse; a body `seal_snapshot` refuses (§20.4) | §20.5; `seal_snapshot` (§20.4) | alarm naming the writer; from `seal_snapshot`, nothing is sealed (a bug of the caller) |
| `seal/snapshot_large` | a snapshot body over 67,108,864 bytes before compression, or a sealed snapshot over 67,108,864 bytes | §20.4 | nothing is sealed; `engine.md` decides (§19.4, §28.4) |
| `seal/inflate` | inflate exceeded the cap or the stream was wrong | §20.5, §21.6 | alarm naming the writer, or reject the backup |
| `seal/backup_size` | backup file too small or too large | §21.6 | reject the file |
| `seal/backup_header` | backup header wrong | §21.6 | reject the file |
| `seal/backup_board` | not this sheet's board | §21.6 | reject the file |
| `seal/backup_recipient` | sealed to another recovery key | §21.6 | try another typed sheet, else reject |
| `seal/backup_signature` | writer signature fails; for `verify_backup_file`, also a header whose `writer_sign_pk` is not the given key | §21.6 | reject the file |
| `seal/backup_open` | HPKE does not open | §21.6 | reject the file |
| `seal/backup_payload` | payload layout or consistency wrong | §21.6; `write_backup` (§21.3) | reject the file; from `write_backup`, nothing is written (a bug of the caller) |
| `seal/backup_history` | membership history fails; bytes that are not one history record | §21.6; `HistoryRecord::from_bytes` (§21.5); `write_backup` (§21.3) | reject the file as forged; from `write_backup`, nothing is written |
| `seal/backup_writer` | the writer was not a current device at the end of the file's history, or that history's current recovery key is not the sheet's pair of public keys | §21.6 step 10; `write_backup` (§21.3) | reject the file as forged; from `write_backup`, nothing is written |
| `seal/backup_unpinned` | membership's recovery key differs from the pinned one | §21.3; `write_backup` (§26) | do not write; ask the user to confirm the new fingerprint (`device.md`) |
| `seal/not_control` | `HistoryRecord::from_accepted` was given an entry that is not of kind `ctl` | §21.5, SEAL-BKP-010 | a bug of the caller |

## 23. Limits

| Name | Value | Unit | Enforced by |
|---|---|---|---|
| frame size, maximum | 262,144 | bytes | SEAL-FRAME-003 |
| frame size, minimum | 670 | bytes | SEAL-FRAME-003 |
| padded plaintext, maximum | 258,048 | bytes | SEAL-PAD-002 |
| payload, maximum | 257,979 | bytes | SEAL-PAD-002 |
| `at_ms` | 0 to 253,402,300,799,999 | ms since 1970-01-01T00:00:00Z (to 9999-12-31T23:59:59.999Z) | SEAL-PAD-003 |
| padding bucket up to the threshold | 512 | bytes | SEAL-PAD-001 |
| padding threshold for Padmé | 8,192 | bytes | SEAL-PAD-001 |
| items per control entry | 16 | items | SEAL-KIND-003 |
| revocations and reinstatements per entry | 15 | items | SEAL-CTL-001 |
| Skip items per entry | 16 | items | SEAL-CTL-001 |
| wraps per rotation | 2 to 17 | wraps | SEAL-CTL-003 |
| wraps per reinstatement | 1 to 256 | wraps | SEAL-SIG-004 |
| current devices | 16 | devices | SEAL-MEM-011 |
| device records per board | 256 | records | SEAL-MEM-011 |
| `device_no`, maximum | 8,191 | | SEAL-MEM-014 |
| recovery records | 64 | records | SEAL-MEM-002 |
| commitments (epochs) | 65,535 | entries | SEAL-MEM-002 |
| Skip targets recorded | 65,535 | entries | SEAL-MEM-002 |
| Skip window | 16,384 | seqs below the Skip entry's own seq | SEAL-MEM-011 |
| control entries recorded (`ctl_seqs`) | 1,048,576 | entries | SEAL-MEM-002 |
| device name | 64 | bytes | SEAL-ENC-007 |
| host | 255 | bytes in all, `:port` included (at most 253 before it) | SEAL-ENC-006 |
| relay origin | 263 | bytes | SEAL-ENC-005 |
| relay token | 512 | bytes | SEAL-KEY-015 |
| beat lists (`rules`, `envelopes`, `hash_versions`) | 16 | values each | SEAL-KIND-005 |
| beat `acks` | 64 | objects | SEAL-KIND-005 |
| stale-epoch window | 2,592,000,000 | ms of log clock (30 days) | SEAL-MEM-018 |
| active-member window | 2,592,000,000 | ms (30 days) | SEAL-MEM-019 |
| pairing secret | 80 | bits | SEAL-CODE-001 |
| pairing code check | 10 | bits | SEAL-CODE-001 |
| pairing lifetime | 600,000 | ms | SEAL-PAIR-013 |
| SAS entries on N | 3 | tries | SEAL-PAIR-007 |
| slot body (A, B, C) | 48, 119 to 242, 64 | bytes | SEAL-PAIR-002 |
| slot D body | 16,040 | bytes | SEAL-PAIR-002 |
| bundle JSON | 16,000 | bytes | SEAL-PAIR-009 |
| relay and admin nonce | exactly 32 | bytes | SEAL-SIG-002 |
| recovery sheet | 56 | characters | SEAL-SHEET-002 |
| sheet check | 20 | bits | SEAL-SHEET-001 |
| sealed file record ciphertext | 16 to 67,108,880 | bytes | SEAL-FILE-003 |
| sealed blob, maximum | 67,108,864 | bytes | SEAL-BLOB-001, SEAL-BLOB-009 |
| blob part | 1,048,576 | bytes | SEAL-BLOB-001 |
| inflated snapshot | 67,108,864 | bytes | SEAL-BLOB-003, SEAL-BLOB-009 |
| history records per snapshot | 1,048,576 | records | SEAL-BLOB-006 |
| backup file | 268,435,456 | bytes | SEAL-BKP-003 |
| inflated backup payload | 536,870,912 | bytes | SEAL-BKP-003 |
| history records per backup | 1,048,576 | records | SEAL-BKP-002 |
| `items` per history record | 262,144 | bytes | SEAL-BKP-002 |
| frames per segment | 1,048,576 | frames | SEAL-BKP-002 |
| backup `meta` | 1,048,576 | bytes | SEAL-BKP-002 |

## 24. Test vectors

Stage `seal` publishes these families under `docs/sync/vectors/`, one JSON file each, in the format of README §7 (`"spec"` naming the section, `"cases"` an array; byte strings in lowercase hex). Every value drawn from `Entropy` in a case is given as an input (`"entropy"`: the bytes in the order they are drawn), so the outputs are deterministic. Each family has valid cases and, for every check it covers, at least one invalid case with the expected error code or receipt class.

| File | Spec | Inputs | Expected outputs |
|---|---|---|---|
| `seal-frame.json` | §8, §9 | board, author keys, `ctr`, epoch root, plaintext fields, entropy | frame bytes, `frame_hash`, signed bytes, AD |
| `seal-padding.json` | §10.2, §10.3 | `payload_len` values (including 0, 443, 444, 8123, 8124, 257,979, 257,980) and malformed plaintexts, including `at_ms` 253,402,300,799,999 (accepted) and 253,402,300,800,000 (refused) | `P(U)`, or the unpadding error |
| `seal-kdf.json` | §6 | seed, salt, roots, `L_d`, kinds, epochs | recovery keys, `board_id`, `dev_R`, `c_e`, `K_entry,e`, `K_snap,e`, `K_blob,e`, `K_file(k)`, key fingerprints |
| `seal-wrap.json` | §7 | root, recipient keys, epoch, entropy | 96-byte wrap; open results; tampered-wrap failures |
| `seal-rotation.json` | §7, §12.5 | one rotation's sealing call: board, author keys, `ctr`, the new epoch, its recipients' keys in ascending recipient-id order, and entropy in draw order (the bytes drawn for the root, then the 32 bytes `ikm` of each wrap in that order, then the frame's nonce) | the root; for each wrap, its `ikm`, the ephemeral public key `DeriveKeyPair(ikm)` yields and the 96-byte wrap, in item order; the `EpochRotated` item; the frame bytes; a case whose entropy is one byte short, with the error |
| `seal-commit.json` | §6.5, §13.8 | rotations with good and bad wraps | accept, or the commitment halt |
| `seal-chain.json` | §15 | board, frames | `C_0`, `C_s`, sync fingerprints, move codes |
| `seal-membership.json` | §11 to §14 | `M_{s−1}` (canonical bytes), a frame, the other inputs of §14.1 | class, code, flags, `M_s` canonical bytes; one case per row of §13.6 and per refusal code, plus the races of V2-8 |
| `seal-receipt.json` | §14 | the same | one case per step of §14.2, valid and failing |
| `seal-signatures.json` | §16 | login and admin fields, every registry operation | signed bytes and signatures; a 31- and a 33-byte nonce refused |
| `seal-pairing.json` | §17 | `s`, ephemeral and long-term keys, relay origin, board, membership, bundle fields, entropy | code, `ticket_key`, `tid`, `mac_key`, slots A to D, `T`, SAS digits, bundle key and ciphertext |
| `seal-code.json` | §17.2, §5.2 | secrets and typed strings with errors, case and look-alikes | code text, or the error |
| `seal-sheet.json` | §18 | seeds and typed sheets | sheet text, or the error |
| `seal-file.json` | §19 | `L_d`, kind, board, dev, `file_id`, record plaintexts, entropy | file bytes; reads with torn, bad and corrupt tails |
| `seal-blob.json` | §20 | blob kind, epoch root, `S`, writer, body (a snapshot body with its history), entropy | sealed bytes, `sha256`, parts; opening failures in the order of §20.5, including a history that fails the projection or disagrees with the membership; declared-mode openings (§20.5) with the returned epoch and inflated length, and their failures |
| `seal-backup.json` | §21 | seed, writer keys, a small history, payload fields, entropy | file bytes; reader results for each step of §21.6, including forged, truncated and re-signed files; `verify_backup_file` results for a good file, a re-signed file, a file whose header names another `writer_sign_pk`, and a truncated one; history records built by `from_accepted` from accepted `ctl` entries, their bytes, and `from_bytes` failures |

**Known-answer suites** (SYNC_PLAN.md §6.3), kept under `crates/taskdeck-seal/tests/kat/` with their source and licence noted: RFC 8032 §7.1 (Ed25519); RFC 9180 Appendix A for the suite DHKEM(X25519, HKDF-SHA256), HKDF-SHA256, ChaCha20-Poly1305 in base mode; RFC 5869 Appendix A test cases 1 to 3 (HKDF-SHA256); draft-irtf-cfrg-xchacha-03 Appendix A (XChaCha20-Poly1305); Wycheproof `ed25519_test.json` and `xchacha20_poly1305_test.json`. The `relay` stage runs the Wycheproof Ed25519 vectors and `seal-frame.json`, `seal-signatures.json` and `seal-chain.json` in its conformance suite (V1-16).

## 25. Interfaces

**Provided to every spec.** The context-string registry (§4.1). Other specs use only those strings.

**`core.md`.**
- Requires: the canonical encoding v1 and a function that encodes a JSON value canonically, used for the payloads of §11, the items of §11.3 and the bundle (§5.3); that its encoding omits exactly the fields whose value equals a default the schema states, so the defaults in this spec's tables are honoured. The `seal` crate does not link `core`: it carries its own encoder and strict parser for exactly this encoding, and stage `device`, which links both crates, compares the two in a test.
- Requires: the canonical board bytes, used in snapshot bodies (§20.4) and backups (§21.5).
- Requires: any log command that references a blob (the shared picture) carries the blob id, `sha256` of the sealed bytes, `size`, `parts` and `epoch`, so a reader can check §20.5 before decrypting.
- Requires: `BoardData.rules` mirrors `M.rules` (§13.10); the engine sets it when a RulesUpgrade activates, and `apply` reads it from there.
- Provides: nothing else; seal never parses a `cmd` payload (SEAL-KIND-002).

**`engine.md`.**
- Requires: the sealer trait wraps §14's classification and §13's membership machine; the engine supplies the inputs of §14.1, including each own frame's hash for the own-dev check, the chain lookup and `retained_from`, and the Skip target frames after `NeedFrames`.
- Requires: the engine maps the classes of §14.4 to its actions (skip with alarm, stop on `Tamper`, refuse everywhere, apply, halt), and maps a `core` strict-parse failure of a `cmd` to the `Refuse` outcome.
- Requires: the state hash of ENG-HASH-001 (`engine.md` §18.3), over §13.2's membership bytes. Seal computes it with its own `state_hash` (§26), which reproduces `taskdeck_sync::state_hash` byte for byte and is tested against `engine-hash.json` (`engine.md` §35), for the checks of §17.9, §20.5 and §21.6.
- Requires: when to beat and snapshot, how to re-seal refused entries (keeping `entry_id`, SEAL-PLAIN-002), the banners for every accepted control item, the move-code prompt and its retry limit, and what a device does on `seal/missing_root`, on `ResetTo` and `Skip` (replay or re-bootstrap) and on `purge_before`.
- Requires: that a device composing `DeviceAdded` does so at the head of the bundle's snapshot (SEAL-PAIR-010), and that a halted device never pairs.
- Provides: §11 to §15 as pure functions, the pairing machines of §17.10.

**`relay.md`.**
- Requires: the relay verifies frames (SEAL-FRAME-002), login (SEAL-SIG-001) and admin (SEAL-SIG-003, SEAL-SIG-004) signatures from these layouts, with strict Ed25519 (SEAL-GEN-010), and computes the chain (SEAL-CHAIN-001) for restore tail.
- Requires: at `create`, the relay checks the Genesis frame's signature under the supplied recovery signing key, that `dev = dev_R` (SEAL-KEY-006) and that `board_id` derives from that key and the salt (SEAL-KEY-005), and registers the recovery identity with role `recovery`, and the first device from the request. These are the relay's whole share of the checks of seq 1 (`relay.md` §10.2): the epoch, the opening under `K_entry,1`, `based_on` and the Genesis item lie inside `body`, so SEAL-MEM-004 and SEAL-KEY-017 are the clients' alone.
- Requires: ticket proofs are checked as `H(ctx("pair-tid") ‖ ticket_key)[0..16] = tid` (SEAL-PAIR-015); slots are stored as opaque bytes of at most 36 + 16,040 bytes, one per label.
- Requires: `register` in frame mode appends the `DeviceAdded` frame whose hash its arguments name and returns the new device's first token, and in seq mode names such an entry already stored; `recovery_register` names, by seq, a stored `K`-shape entry its sender has classified `Accept`, and never appends a frame (`relay.md` §12.12); the relay registers neither key without such a frame.
- Requires: the token MAC input begins with `ctx("token")`, the door with `ctx("door")` (ruling S4), and the relay address text form encodes `board_id ‖ door` as Crockford base32 (§5.2).
- Requires: a relay nonce is exactly 32 bytes (SEAL-SIG-002).
- Provides: the operation registry of §16.2; `relay.md` owns when each operation is sent and what it does.

**`device.md`.**
- Requires: the files of §19.2, their contents, write and fsync order, and the handling of torn and bad tails; the key stores that wrap `L_d` with `ctx("local-key") ‖ board_id ‖ dev` as associated data or entropy where the store accepts it (§6.6).
- Requires: every accepted control entry's history record (§21.5) is kept for the life of the board, through purges, so backups can carry the full history (SEAL-BKP-002) and every snapshot it seals carries it (SEAL-BLOB-006).
- Requires: the sealer adapter opens, with `unwrap_root`, the wraps a `login` answer carries for a reinstated device (`device.md` §10.3), as §7 states (SEAL-WRAP-002, SEAL-WRAP-003, SEAL-KEY-013): epoch by epoch, oldest first, each once its membership records that epoch's commitment. `Receipt.learned_root` reports only a root that an entry's own wraps teach. The sealer adapter also carries the caller's duties SEAL-FRAME-005, SEAL-ENV-003, SEAL-ENV-004 and SEAL-MEM-016, which the crate's pure functions cannot violate, and the caller's half of SEAL-WRAP-003 (a wrap delivered outside an entry: the adapter returns `seal/commitment`, the engine halts, `engine.md` §24.4); the transport (`device.md` §10) carries SEAL-SIG-005 and the caller's half of SEAL-SIG-002 (the origin comes from the device's own configuration). The adapter also carries the caller's halves of SEAL-KEY-012 (`SymKey` carries no kind, so the caller passes each function the key of its own kind) and of SEAL-KEY-013 (`derive_epoch_key` and `open_snapshot_declared` take a root without a membership to check it against, so the adapter gives them only roots it has already checked against the commitment its membership records). In pairing, the runtime of `device` feeds both machines monotonic milliseconds and ticks (SEAL-PAIR-013), sends `UserConfirmed` only when the user has confirmed on E (SEAL-PAIR-008), gives N `OsEntropy` (SEAL-GEN-005), clears the code and digit strings it was handed and gathers the log into one `Entries` event; the engine composes and seals the `DeviceAdded` (SEAL-PAIR-010, `engine.md` ENG-OPS-001) and, after `Joined`, which hands out no root beyond the bundle's, classifies the log from `h + 1` again and learns later roots from N's own wraps. Stage `device` owns these duties of the adapter, the transport and the runtime (SEAL-PAIR-008 and SEAL-PAIR-013 together with `seal`, which tests the machines' half), and SEAL-SIG-005 and SEAL-WRAP-003 together with `engine`, which supplies the `register` arguments and halts on such a wrap; SEAL-PAIR-010 is `engine`'s with `seal`.
- Requires: backup scheduling, destinations and file names; the pinned recovery fingerprint (SEAL-BKP-006); the `meta` JSON of §21.5.
- Requires: release builds never enable the `test-entropy` feature (SEAL-GEN-005), which stage `release` checks by the feature resolution and by searching every artifact for `TEST_ENTROPY_MARKER` (SEAL-GEN-014; `device.md` §11.2, DEV-TRUST-003).

**Stage `recovery`** (SYNC_PLAN.md §6.8): builds the backup writer and reader of §21, the sheet's printed page and retype check from §18, and the recovery signer's operations from §12 and §13.12.

## 26. Public API

The `taskdeck-seal` crate exposes pure functions and sans-IO machines. No function performs I/O or reads a clock; randomness enters only through `Entropy`. Secret types implement `ZeroizeOnDrop` and do not implement `Debug`, `Display`, `Clone` or `serde::Serialize`. Signatures are given in Rust; the skeleton must compile with exactly these names.

```rust
#![forbid(unsafe_code)]

// ---------- ids and small values (seal.md §5, §8, §15)
pub struct BoardId(pub [u8; 16]);
pub struct DevId(pub [u8; 16]);
pub struct EntryId(pub [u8; 16]);
pub struct BlobId(pub [u8; 16]);
pub struct FrameHash(pub [u8; 32]);
pub struct Chain(pub [u8; 32]);
pub struct Head { pub seq: u64, pub chain: Chain }
pub struct Epoch(pub u32);

pub enum SealError { /* one unit variant per code of §22, named the CamelCase of the code's name:
                        seal/frame_format is FrameFormat, seal/bad_version is BadVersion, seal/ctl_genesis is CtlGenesis */ }
impl SealError { pub fn code(&self) -> &'static str; }   // "seal/…"

// ---------- randomness (§3)
pub trait Entropy: private::Sealed { fn fill(&mut self, out: &mut [u8]) -> Result<(), SealError>; }  // sealed trait
pub struct OsEntropy;                                     // getrandom
impl Entropy for OsEntropy { /* … */ }
#[cfg(feature = "test-entropy")] pub struct FixedEntropy { /* replays given bytes */ }
#[cfg(feature = "test-entropy")] impl FixedEntropy { pub fn new(bytes: Vec<u8>) -> Self; }   // passes TEST_ENTROPY_MARKER to std::hint::black_box
#[cfg(feature = "test-entropy")] pub const TEST_ENTROPY_MARKER: &str = "taskdeck-test-entropy-enabled-3b9e5d";  // SEAL-GEN-014

// ---------- context strings (§4)
pub mod ctx { pub const FRAME: &[u8] = b"taskdeck/v1/frame\0"; /* one const per row of §4.1 */ }

// ---------- encodings (§5)
pub mod base32 {
    pub fn encode(bytes: &[u8]) -> String;
    pub fn decode_typed(text: &str, bytes: usize) -> Result<Vec<u8>, super::SealError>;   // seal/code_format on any failure; callers map it to their own codes
}
pub fn relay_origin(host: &str) -> Result<String, SealError>;   // §5.4: "https://" ‖ host; seal/host_format when host fails SEAL-ENC-006

// ---------- keys (§6, §7)
pub struct RecoverySeed { /* secret */ }
impl RecoverySeed {
    pub fn generate(rng: &mut dyn Entropy) -> Result<Self, SealError>;
    pub fn to_sheet(&self) -> zeroize::Zeroizing<String>;                    // §18.1
    pub fn from_sheet(text: &str) -> Result<Self, SealError>;                // §18.2
    pub fn keys(&self) -> RecoveryKeys;
}
pub struct RecoveryKeys { /* secret: signing key, HPKE secret */ }
impl RecoveryKeys { pub fn public(&self) -> RecoveryPublic; }
pub struct RecoveryPublic { pub sign_pk: [u8; 32], pub hpke_pk: [u8; 32] }
impl RecoveryPublic { pub fn dev(&self, board: &BoardId) -> DevId; pub fn fingerprint(&self) -> String; }
pub fn board_id(rsign_pk: &[u8; 32], salt: &[u8; 16]) -> BoardId;

pub struct DeviceKeys { /* secret: dev, signing key, X25519 key */ }
impl DeviceKeys {
    pub fn generate(rng: &mut dyn Entropy) -> Result<Self, SealError>;
    pub fn public(&self) -> DevicePublic;
    pub fn to_identity_bytes(&self) -> zeroize::Zeroizing<Vec<u8>>;           // §19.2 kind 7: 80 bytes, dev[16] ‖ Ed25519 private seed[32] ‖ X25519 private key[32] (clamped, as RFC 9180 serialises it)
    pub fn from_identity_bytes(bytes: &[u8]) -> Result<Self, SealError>;      // another length, or an X25519 key that is not clamped: seal/file_corrupt
}
pub struct DevicePublic { pub dev: DevId, pub sign_pk: [u8; 32], pub hpke_pk: [u8; 32] }
impl DevicePublic { pub fn fingerprint(&self) -> String; }

pub enum Signer<'a> { Device(&'a DeviceKeys), Recovery(&'a RecoveryKeys, BoardId) }   // a Recovery signer of another board than the call's: seal/unknown_author

pub struct EpochRoot { /* secret */ }
impl EpochRoot {
    pub fn generate(rng: &mut dyn Entropy) -> Result<Self, SealError>;
    pub fn from_bytes(bytes: [u8; 32]) -> Self;
    pub fn to_bytes(&self) -> zeroize::Zeroizing<[u8; 32]>;                   // §19.2 kind 7 only (SEAL-KEY-009)
    pub fn commitment(&self, board: &BoardId, epoch: u32) -> [u8; 32];       // §6.5
}
pub enum KeyKind { Entry, Snap, Blob }
pub struct SymKey { /* secret */ }
pub fn derive_epoch_key(root: &EpochRoot, board: &BoardId, epoch: u32, kind: KeyKind) -> SymKey;
pub fn wrap_root(root: &EpochRoot, board: &BoardId, epoch: u32, commit: &[u8; 32],
                 recipient: DevId, recipient_pk: &[u8; 32], rng: &mut dyn Entropy) -> Result<[u8; 96], SealError>;
pub fn unwrap_root(wrap: &[u8; 96], board: &BoardId, epoch: u32, commit: &[u8; 32],
                   own: &Recipient) -> Result<EpochRoot, SealError>;
pub enum Recipient<'a> { Device(&'a DeviceKeys), Recovery(&'a RecoveryKeys, BoardId) }   // a Recovery recipient of another board than the call's: seal/commitment

// ---------- entries (§8–§11)
pub enum Kind { Cmd = 1, Ctl = 2, Beat = 3, Snap = 4 }
pub struct Plain { pub entry_id: EntryId, pub kind: Kind, pub at_ms: i64, pub based_on: Head, pub payload: Vec<u8> }
pub fn padded_len(unpadded: u64) -> u64;                                       // §10.2; u64::MAX when the sum would overflow
pub fn new_entry_id(rng: &mut dyn Entropy) -> Result<EntryId, SealError>;
pub fn seal_frame(signer: &Signer, board: &BoardId, ctr: u64, epoch: u32, key: &SymKey,
                  plain: &Plain, rng: &mut dyn Entropy) -> Result<Vec<u8>, SealError>;
pub fn frame_hash(frame: &[u8]) -> FrameHash;
pub struct FrameHeader { pub board: BoardId, pub dev: DevId, pub ctr: u64, pub len: u32 }
pub fn frame_header(frame: &[u8]) -> Result<FrameHeader, SealError>;           // §14.2 step 1 only

pub enum CtlItem { Genesis(Genesis), DeviceAdded(DeviceAdded), DeviceRevoked(DevId),
                   DeviceReinstated(DevId), EpochRotated(EpochRotated), RecoveryKeyChange(RecoveryKeyChange),
                   Fence(Fence), ResetTo(ResetTo), Skip(SkipItem), RulesUpgrade(RulesUpgrade) }
// one struct per item of §12, fields as its table; plus:
pub struct Genesis { pub salt: [u8; 16], pub recovery: RecoveryPublic, pub device: GenesisDevice, pub epoch: u32,
                     pub commit: [u8; 32], pub wraps: Vec<[u8; 96]>, pub rules: u32, pub hash_version: u32,
                     pub state_hash: [u8; 32], pub snapshot: Option<GenesisSnapshot> }
                     // wraps: a Vec, as EpochRotated.wraps; it holds exactly two (§12.2), and one with any other count fails §12.2
pub struct GenesisDevice { pub dev: DevId, pub sign_pk: [u8; 32], pub hpke_pk: [u8; 32], pub name: String }   // §12.2 `device`
pub struct GenesisSnapshot { pub blob: BlobId, pub sha256: [u8; 32], pub size: u64, pub parts: u32, pub inflated: u64 }  // §12.2 `snapshot`
pub struct Beat { /* §11.4 */ }
pub struct SnapAnnounce { /* §11.5 */ }
pub fn ctl_payload(items: &[CtlItem], signer: &Signer, board: &BoardId, ctr: u64,
                   entry_id: &EntryId, at_ms: i64) -> Result<Vec<u8>, SealError>;   // §11.3; an item its table excludes: seal/bad_payload; a Fence host outside §5.4: seal/host_format; items that form no shape: seal/ctl_shape
pub fn beat_payload(beat: &Beat) -> Result<Vec<u8>, SealError>;
pub fn snap_payload(snap: &SnapAnnounce) -> Result<Vec<u8>, SealError>;

// ---------- membership (§13)
pub struct Membership { /* §13.1 */ }
impl Membership {
    pub fn to_canonical_bytes(&self) -> Vec<u8>;                               // §13.2
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, SealError>;
    pub fn board(&self) -> BoardId;
    pub fn epoch(&self) -> u32;
    pub fn rules(&self) -> u32;
    pub fn devices(&self) -> &[DeviceRecord];
    pub fn current_recovery(&self) -> &RecoveryRecord;
    pub fn device_no(&self, dev: &DevId) -> Option<u16>;
}
pub struct DeviceRecord { /* §13.2 device record, public fields */ }
pub struct RecoveryRecord { /* §13.2 */ }
pub struct GenesisAnchor { pub rsign_pk: [u8; 32], pub salt: [u8; 16], pub root1: EpochRoot }

// ---------- receipt (§14)
pub struct ReceiptInput<'a> {
    pub seq: u64,
    pub frame: &'a [u8],
    pub prev_chain: Chain,
    pub membership: &'a Membership,                    // M_{s−1}
    pub roots: &'a dyn Fn(u32) -> Option<&'a EpochRoot>,
    pub chain_at: &'a dyn Fn(u64) -> Option<Chain>,    // C_b for b ≥ retained_from
    pub retained_from: u64,
    pub own: Option<OwnIdentity<'a>>,                  // None for read-only readers
    pub supported_rules: &'a [u32],
    pub genesis: Option<&'a GenesisAnchor>,            // Some only for seq 1
    pub skip_targets: &'a dyn Fn(u64) -> Option<&'a [u8]>,
}
pub struct OwnIdentity<'a> { pub keys: &'a DeviceKeys, pub sealed: &'a dyn Fn(u64) -> Option<FrameHash> }
pub enum Class { Skip, Tamper, Refuse, Accept, NeedFrames(Vec<(u64, FrameHash)>) }
pub enum Halt { Commitment, UnknownRules(u32) }
pub struct Receipt {
    pub class: Class,
    pub code: Option<&'static str>,                    // the failing step's code; with a halt, the halt's; None for an Accept without a halt and for NeedFrames
    pub chain: Chain,                                  // C_s
    pub own_dev_foreign: bool,
    pub based_on_unchecked: bool,
    pub opened: Option<Opened>,                        // Some exactly when step 11 passed (the plaintext opened and unpadded), whatever the class
    pub membership: Membership,                        // M_s (equal to M_{s−1} when unchanged)
    pub learned_root: Option<(u32, EpochRoot)>,
    pub halt: Option<Halt>,
}
pub struct Opened { pub entry_id: EntryId, pub kind: Kind, pub at_ms: i64, pub based_on: Head,
                    pub payload: Vec<u8>, pub items: Option<Vec<CtlItem>>, pub beat: Option<Beat>,
                    pub snap: Option<SnapAnnounce> }
pub fn classify(input: &ReceiptInput) -> Result<Receipt, SealError>;

// ---------- chain and fingerprints (§15)
pub fn chain_zero(board: &BoardId) -> Chain;
pub fn chain_step(prev: &Chain, seq: u64, frame: &[u8]) -> Chain;
pub struct SyncFingerprint { pub seq: u64, pub words: [&'static str; 4] }
pub fn sync_fingerprint(board: &BoardId, head: &Head) -> SyncFingerprint;
pub fn move_code(board: &BoardId, fence_seq: u64, fence_hash: &FrameHash) -> String;
pub fn check_move_code(board: &BoardId, fence_seq: u64, fence_hash: &FrameHash, typed: &str) -> Result<(), SealError>;
pub fn key_fingerprint(sign_pk: &[u8; 32], hpke_pk: &[u8; 32]) -> String;
pub fn state_hash(hash_version: u32, board_hash: &[u8; 32], membership_bytes: &[u8]) -> [u8; 32];  // engine.md §18.3, ENG-HASH-001

// ---------- signatures (§16)
pub fn login_message(origin: &str, board: &BoardId, dev: &DevId, generation: u32,
                     relay_nonce: &[u8], client_nonce: &[u8; 32]) -> Result<Vec<u8>, SealError>;
pub fn sign_login(keys: &DeviceKeys, origin: &str, board: &BoardId, generation: u32, relay_nonce: &[u8],
                  rng: &mut dyn Entropy) -> Result<([u8; 32], [u8; 64]), SealError>;   // (client_nonce, sig)
pub enum AdminOp { Register { dev: DevId, sign_pk: [u8; 32], frame_hash: FrameHash }, Revoke(DevId),
                   Reinstate { dev: DevId, first_epoch: u32, wraps: Vec<[u8; 96]> }, RecoveryRegister { dev: DevId, sign_pk: [u8; 32], frame_hash: FrameHash },
                   Fence { host: String, door: [u8; 5], home_gen: u32 }, Seed(SeedArgs), Compact(CompactArgs),
                   RestoreTail { first_seq: u64, frames: Vec<FrameHash> }, Purge { before_seq: u64, epoch: u32 },
                   MintReadToken { dev: DevId, epoch: u32 }, RevokeReadToken(DevId) }
impl AdminOp { pub fn name(&self) -> &'static str; pub fn args(&self) -> Vec<u8>; }
pub fn admin_message(op: &AdminOp, board: &BoardId, signer: &DevId, relay_nonce: &[u8]) -> Result<Vec<u8>, SealError>;
pub fn sign_admin(signer: &Signer, op: &AdminOp, board: &BoardId, relay_nonce: &[u8]) -> Result<[u8; 64], SealError>;

// ---------- pairing (§17)
pub mod pair {
    pub fn new_code(rng: &mut dyn super::Entropy) -> Result<(PairSecret, String), super::SealError>;
    pub fn parse_code(text: &str) -> Result<PairSecret, super::SealError>;
    pub struct PairSecret { /* secret */ }
    pub struct Existing { /* secret: E's machine, §17.10 */ }
    pub struct New { /* secret: N's machine, §17.10 */ }
    pub enum ExistingEvent {
        SlotB { bytes: Vec<u8> }, UserConfirmed,
        Registered { h: u64, chain: super::Chain, snapshot: super::SnapAnnounce, added_seq: u64, token: String,
                     roots: Vec<(u32, super::EpochRoot)>, host: String, door: [u8; 5] },
        RegisterFailed { code: super::SealError }, Cancel, Tick { now: u64 } }
    pub enum ExistingEffect {
        RegisterTicket { tid: [u8; 16], slot_a: Vec<u8> }, ShowCode { code: String }, PostSlot { slot: Vec<u8> },
        ShowSas { digits: String }, NeedSnapshotAndRegister { device: super::DeviceAdded }, Done { dev: super::DevId },
        DeleteTicket { tid: [u8; 16] }, Abort { code: super::SealError } }
    pub enum NewEvent {
        CodeEntered { code: String, host: String, board_id: super::BoardId, door: [u8; 5], name: String, now: u64 },
        Slot { bytes: Vec<u8> }, SasEntered { digits: String }, Snapshot { parts: Vec<Vec<u8>> },
        Entries { frames: Vec<(u64, Vec<u8>)> }, Frames { frames: Vec<(u64, Vec<u8>)> }, Cancel, Tick { now: u64 } }
    pub enum NewEffect {
        FetchSlot { tid: [u8; 16], ticket_key: zeroize::Zeroizing<[u8; 32]>, label: u8 },
        AskAgain { code: super::SealError }, PostSlot { slot: Vec<u8> }, AskSas, ShowAccepted,
        FetchSnapshot { token: String, blob: super::BlobId, parts: u32 }, ReadLog { token: String, from: u64 },
        FetchFrames { token: String, seqs: Vec<u64> },
        Joined { device_no: u16, keys: super::DeviceKeys, bundle: Bundle, snapshot: super::SnapshotBody },
        Abort { code: super::SealError } }
    pub struct Bundle { pub board_id: super::BoardId, pub host: String, pub door: [u8; 5],
                        pub roots: Vec<(u32, super::EpochRoot)>, pub head: super::Head, pub snapshot: super::SnapAnnounce,
                        pub added_seq: u64, pub token: String }               // slot D, §17.8; ZeroizeOnDrop
    impl Existing { pub fn start(membership: &super::Membership, head: super::Head, relay_origin: &str,
                                 own: &super::DevId, now: u64, rng: &mut dyn super::Entropy) -> (Self, Vec<ExistingEffect>);
                    pub fn on(&mut self, ev: ExistingEvent, rng: &mut dyn super::Entropy) -> Vec<ExistingEffect>; }
    impl New { pub fn new() -> Self;                                          // Idle; draws nothing (§17.10)
               pub fn on(&mut self, ev: NewEvent, rng: &mut dyn super::Entropy) -> Vec<NewEffect>; }
}

// ---------- sealed files (§19)
#[repr(u8)]
pub enum FileKind { Log = 1, Checkpoint = 2, Base = 3, Hashes = 4, Outbox = 5, Overlay = 6, Identity = 7, Notices = 8,
                    Meta = 9, Marker = 10, History = 11, Device = 12, Picture = 13, Weather = 14, Draft = 15,
                    Feed = 16, Bootstrap = 17, Unsent = 18, OwnPicture = 19 }
pub struct StorageKey { /* secret L_d */ }
impl StorageKey { pub fn generate(rng: &mut dyn Entropy) -> Result<Self, SealError>; pub fn from_bytes(b: [u8; 32]) -> Self; }
pub struct FileSealer { /* header and next record index */ }
impl FileSealer {
    pub fn create(key: &StorageKey, kind: FileKind, board: &BoardId, dev: &DevId, rng: &mut dyn Entropy) -> Result<(Self, Vec<u8>), SealError>; // header bytes
    pub fn resume(key: &StorageKey, header: &[u8], records: u64) -> Result<Self, SealError>;
    pub fn seal_record(&mut self, plaintext: &[u8], rng: &mut dyn Entropy) -> Result<Vec<u8>, SealError>;
}
pub struct OpenedFile { pub records: Vec<zeroize::Zeroizing<Vec<u8>>>, pub tail: FileTail }
pub enum FileTail { Clean, Torn { valid_bytes: u64 }, Bad { valid_bytes: u64 } }
pub fn open_file(key: &StorageKey, kind: FileKind, board: &BoardId, dev: &DevId, bytes: &[u8]) -> Result<OpenedFile, SealError>;

// ---------- blobs and snapshots (§20)
pub enum BlobKind { Snapshot = 1, Picture = 2 }
pub struct SealedBlob { pub id: BlobId, pub bytes: Vec<u8>, pub sha256: [u8; 32], pub parts: u32 }
pub struct BlobRef { pub id: BlobId, pub sha256: [u8; 32], pub size: u64, pub parts: u32, pub inflated: u64, pub epoch: u32 }
pub struct SnapshotBody { pub seq: u64, pub chain: Chain, pub hash_version: u32, pub state_hash: [u8; 32],
                          pub board: Vec<u8>, pub membership: Vec<u8>, pub history: Vec<HistoryRecord> }
pub fn seal_snapshot(body: &SnapshotBody, key: &SymKey, board: &BoardId, epoch: u32, writer: &DevId,
                     rng: &mut dyn Entropy) -> Result<SealedBlob, SealError>;
pub fn open_snapshot(parts: &[&[u8]], expected: &BlobRef, s: u64, state_hash: &[u8; 32], hash_version: u32, writer: &DevId, key: &SymKey,
                     board: &BoardId) -> Result<SnapshotBody, SealError>;  // state_hash, hash_version: the signed entry's or the bundle's (§20.5 step 7)
pub fn open_snapshot_declared<'a>(parts: &[&[u8]], sha256: &[u8; 32], size: u64, s: u64, writer: &DevId,
                                  roots: &dyn Fn(u32) -> Option<&'a EpochRoot>, board: &BoardId)
                                  -> Result<(SnapshotBody, u32, u64), SealError>;  // §20.5 declared mode: (body, epoch, inflated)
pub fn seal_blob(body: &[u8], key: &SymKey, board: &BoardId, epoch: u32, writer: &DevId,
                 rng: &mut dyn Entropy) -> Result<SealedBlob, SealError>;
pub fn open_blob(parts: &[&[u8]], expected: &BlobRef, writer: &DevId, key: &SymKey, board: &BoardId) -> Result<Vec<u8>, SealError>;
                                                                 // kind 2; expected.inflated is not read (callers pass 0), §20.5
pub fn split_parts(sealed: &[u8]) -> Vec<&[u8]>;

// ---------- backups (§21)
pub struct HistoryRecord { /* §21.5 */ }
impl HistoryRecord {
    pub fn from_accepted(seq: u64, frame: &[u8], opened: &Opened) -> Result<Self, SealError>;  // SEAL-BKP-010
    pub fn to_bytes(&self) -> Vec<u8>;                                                         // §21.5 layout
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SealError>;                                // seal/backup_history
    pub fn seq(&self) -> u64;  pub fn dev(&self) -> DevId;  pub fn ctr(&self) -> u64;  pub fn frame_hash(&self) -> FrameHash;  pub fn items(&self) -> Result<Vec<CtlItem>, SealError>;  // items: seal/backup_history when they do not parse (a record not yet verified)
}
pub enum BackupContent { Nightly { membership: Vec<u8>, board: Vec<u8>, head: Head, hash_version: u32, state_hash: [u8; 32] },
                         Segment { membership: Vec<u8>, first_seq: u64, chain_before: Chain, frames: Vec<Vec<u8>> } }
pub struct BackupInput<'a> { pub board: BoardId, pub seq: u64, pub written_ms: i64, pub history: &'a [HistoryRecord],
                             pub content: BackupContent, pub roots: Vec<(u32, EpochRoot)>, pub read_token: Option<String>,
                             pub meta: Vec<u8>, pub pinned_recovery: RecoveryPublic }
pub fn write_backup(writer: &DeviceKeys, input: BackupInput, rng: &mut dyn Entropy) -> Result<Vec<u8>, SealError>;
pub struct VerifiedBackup { pub writer: DevicePublic, pub seq: u64, pub written_ms: i64, pub file_type: u8, pub content: BackupContent,  // written_ms: the header's, the writer's clock, display only
                            pub roots: Vec<(u32, EpochRoot)>, pub read_token: Option<String>, pub meta: Vec<u8>, pub history: Vec<HistoryRecord>, pub projection: Membership }  // history: the verified records, in order
pub fn read_backup(bytes: &[u8], sheet_board: &BoardId, recovery: &RecoveryKeys) -> Result<VerifiedBackup, SealError>;
pub struct BackupHeader { pub file_type: u8, pub board: BoardId, pub writer_dev: DevId, pub writer_sign_pk: [u8; 32],
                          pub recipient: DevId, pub seq: u64, pub written_ms: i64, pub ct_len: u64 }   // §21.2
pub fn verify_backup_file(bytes: &[u8], writer_sign_pk: &[u8; 32]) -> Result<BackupHeader, SealError>;  // §21.6 steps 1, 2, 5; SEAL-BKP-009
```

The engine's sealer trait (`engine.md`, decision D6) is implemented in `taskdeck-device` over these functions; `seal` does not depend on the engine crate.

## 27. Open questions

Non-normative. Each states the default this spec takes.

1. **Inner signature on control payloads** (§11.3). Decided here: every control payload carries a second signature over its items in plaintext, so backups can prove membership history without epoch roots and a purge can delete old roots (SYNC.md is silent on how a reader verifies history after a purge).
2. **Fence names the relay by `host` and `door`** (§12.7), not by the relay address text, so seal needs no parser of `relay.md`'s text form. Decided here (SYNC.md is silent).
3. **Home generation** (§12.7). SYNC.md's `Fence{new relay, g+1}` is read as a board-level counter of relays, seal's own, unrelated to the token `gen`. Decided here.
4. **Move code from the Fence's seq and frame hash** (§15.4). Decided here (SYNC.md gives only the length).
5. **Pairing code and sheet checks are hash-based** (`pair-code`, `sheet-check`), not an error-correcting code: random errors are caught with probability `1 − 2^−10` and `1 − 2^−20`, single-character errors are not guaranteed to be located. Decided here (SYNC.md fixes only the character counts).
6. **Active members exclude the upgrade's author from the reference time** (§13.10). Decided here (SYNC.md is silent). A device that controls two current devices can still make the others look inactive (§13.10); the owner accepted that limit (decision O1).
7. **Revocations, reinstatements and recovery-key changes always rotate in the same entry** (§12.1 shapes R and K), and only the recovery identity reinstates. Decided here; SYNC.md lets the recovery key "reinstate one another device revoked" and says rotation follows revocation.
8. **A revocation that would leave no current device is refused** (`seal/ctl_last_device`), even from the recovery identity; total compromise is handled by "Reset trust" or a restore. Decided here.
9. **Control and `snap` entries are never accepted under the previous epoch** (§11.1), which settles V2-8(b): a DeviceAdded raced by a rotation is refused and the pairing is repeated. Decided here.
10. **Skip of a `beat` or `snap` entry** is valid and changes nothing (§13.13). Decided here.
11. **Skip needs the target frame** (§14.6). The target is always at or above `F(s) = max(purge_seq, s − 16,384)` (ruling S15), and the relay never compacts the newest 16,384 seqs (`relay.md` RELAY-COMPACT-007), so a device that lacks the frame fetches it from the relay. Only a frame lost with a relay that no longer answers can be missing; the engine then reports an integrity problem without evidence (`engine.md` §17.2). Decided here.
12. **ResetTo's target must be after every current device's `bootstrap_seq`** (§13.6), so every current device holds the log it must replay; the 365-day reach of bases is enforced only by the offering device (`engine.md`). Decided here.
13. **The recovery key's wrap in a rotation cannot be checked by devices** (§13.8): a malicious rotator could give the recovery key a bad root; the recovery tool detects it on use. Accepted residual.
14. **Purge keeps every root inside backups?** Default: no. Backups carry the roots the writer holds (§21.5); after a purge those are only the new ones, and the history stays verifiable through inner signatures.
15. **Backup `meta`, the sync metadata,** is opaque here and defined by `device.md`. Decided here.
16. **Rules version 1 allows only envelope (1, 1) and state-hash version 1** (§14.3). A later version extends the table through this spec and `core.md`.
17. **A device the recovery key reinstates reads what was sealed while it was revoked** (ruling S24, lead rulings L1 and L20). The wraps of the roots it missed travel in the recovery-signed `reinstate` operation (§16.2), outside the reinstating entry, which the device cannot open; the relay returns them at the device's login, and the device opens each by §7 once its membership records that epoch's commitment. More than 256 missed epochs: the recovery tool does not offer reinstatement, and the owner restores to a new board. A device whose revoking entry lies below `purge_seq` is not offered reinstatement either: it pairs again, or the owner restores to a new board. Decided there.
