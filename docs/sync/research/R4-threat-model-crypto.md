# R4: Threat model, cryptography and supply chain

> Working paper from the design run of 2026-11-20 that produced [`SYNC.md`](../../../SYNC.md). It is an agent's report, kept for the detail the design compresses: file and line references, sources, and the reasoning behind each requirement. Where it disagrees with `SYNC.md` or [`SYNC_PLAN.md`](../../../SYNC_PLAN.md), those win. Line numbers refer to `day-planner` at commit `1cfff2f`.

# R4 — Threat model, cryptography and supply chain

**Scope.** This covers an end-to-end-encrypted (E2EE) log of sequenced commands, stored on an untrusted free relay (a Cloudflare Durable Object). Around it sit:
- native clients: egui on Windows and macOS, and a native Android app;
- third-party ICS calendar feeds;
- Open-Meteo for weather;
- optional push through FCM.

Tags: [V] = verified, with evidence; [A] = assumed; [U] = uncertain.

## 1. Bottom line

**E2EE plus native clients turns a malicious relay into a nuisance.** Three conditions make this hold:
- Every client that holds a key gets its code only from builds the user controls.
- Every client authenticates each byte from the relay before parsing it.
- Every client checks the order of the log itself.

Under those conditions the relay can only:
- deny service;
- see metadata (IP addresses, timing, sizes, device count);
- attempt forks or rollbacks, which clients detect.

It cannot read data, forge it, or turn it into code. A web app served by the relay loses this property, because whoever controls the host controls the code that holds the key. That alone justifies a native Android app.

**The risk moves to the supply chain, the accounts and the devices.** Two concrete examples from this repository:
- **TLS trust bug in today's lockfile.** The committed `Cargo.lock` matches several 2026 RustSec advisories [V: lockfile checked against advisory-db commit f291af9, 2026-11-20]. The worst is TLS trust: webpki-roots 1.0.9 with rustls-webpki 0.103.14 may accept certificate chains through distrusted emSign intermediate certificates whose private keys are "now well known" (RUSTSEC-2026-0381 and -0382). TaskDeck's ICS and weather fetchers use exactly these bundled roots, because reqwest 0.12's `rustls-tls` feature means `rustls-tls-webpki-roots` [V].
- **A backdoor the lockfile kept out.** On 2026-08-20, `arrayref` 0.3.10 shipped a backdoor that ran at build time and made itself persistent through an HKCU Run key or a macOS LaunchAgent [V]. TaskDeck pulls arrayref in through tiny-skia and is pinned at 0.3.9 (Cargo.lock:160-161) [V].

**Crypto design (§5):**
- one epoch key per board (XChaCha20-Poly1305, with subkeys from HKDF-SHA256);
- an Ed25519 signing key and an X25519 key-wrapping key on each device;
- a hash chain computed by the clients;
- commit-then-reveal pairing confirmed with a short authentication string (SAS);
- key rotation when a device is lost;
- a paper recovery key and backups encrypted with age.

MLS is overkill here.

## 2. Assets and trust boundaries

**Assets to protect:**
- the board's plaintext, including location;
- subscription URLs (ICS links are bearer credentials);
- keys: epoch keys, device keys, relay tokens, the recovery key and the APK signing key;
- the integrity and availability of the log;
- the integrity of each device.

**Trust boundaries:**
- **Trusted:** each device's OS, the TaskDeck binary, and everything compiled into it.
- **Untrusted for confidentiality and integrity:** the relay, Cloudflare, networks, ICS publishers, Open-Meteo and FCM.
- **The relay** is trusted for availability only. Clients work offline first, so losing the relay delays sync rather than losing data.

## 3. Adversaries and controls

| Adversary | Against a naive design | Controls |
|---|---|---|
| Honest-but-curious relay or Cloudflare | Reads everything | E2EE; padding; random device ids; device names only inside ciphertext; constant User-Agent; no request logging. Residual: IPs, timing, sizes. |
| Malicious relay, or a hijacked Cloudflare account deploying new Worker code | Serves page code that steals keys; injects, replays, drops, reorders, rolls back, forks or withholds entries | Native clients only. Signature and AEAD checked before parsing. Per-device counters catch replays and drops. Hash chain plus a stored head catches rollback. Seen-heads plus a fingerprint catch forks. The offline outbox absorbs DoS. Ordering entries from different devices is the sequencer's legitimate power. |
| Network attacker | Steals tokens sent in clear: today's sync client accepts `http://` (sync.rs:135) because Tailscale encrypted the link. Exploits bad TLS trust. | https/wss only; tokens in the `Authorization` header, never in URLs; fix the TLS advisories. With E2EE, a TLS failure only exposes metadata or allows DoS. |
| Thief with a locked device | Reads the disk offline | BitLocker, FileVault, Android file-based encryption (FBE); secrets wrapped by the OS keystore; remote revoke and rotate. Android Advanced Protection reboots a phone left locked for 72 h [V]. |
| Thief with an unlocked device | Reads everything; opens the pairing or recovery screens; enrols a device; revokes yours | Device-credential gate (BiometricPrompt) on pairing, recovery and revocation; FLAG_SECURE; revoke and rotate from another device; worst case, rebuild from the paper key. |
| Malware on one device | Reads data, signs as that device, destroys data, pushes hostile data or subscriptions | Not preventable on that device (§10). Contain it: signatures stop impersonation of other devices; synced data never carries code; parsers are memory-safe and size-capped; images are decoded in pure Rust; the full log, backups and restore-to-sequence-number undo damage; alerts fire on a new device, a new subscription or a mass delete. Rotate keys after cleanup. |
| GitHub account or CI compromise | Malicious commits; tampered releases; stolen CI secrets | Phishing-resistant MFA. `main` protected and changed only by reviewed PRs, AI-agent branches included. No signing keys or Cloudflare tokens in CI; actions pinned to commit SHAs; read-only token; immutable releases with attestations. There is no `.github/` directory today [V]. |
| Compromised dependency | `build.rs` and proc-macros run on the build machine; runtime code runs inside the trusted base | §4.2 |
| Malicious ICS publisher | Parser exploits, resource exhaustion, SSRF through redirects, tracking | Already in place: `https_only`, at most 3 redirects, 20 s timeout, 8 MiB cap, parser limits (subscriptions.rs:93-95,732-742; ics.rs:126-135) [V]. Add: refuse IP-literal and private-range hosts, which `checked_url` does not do today (subscriptions.rs:156-180) [V]; never fetch ATTACH or URL values. |
| Someone who photographs the QR code or steals a token | Today the QR code or link *is* the master read/write token (phone.rs:1903-1923) | The QR code holds only a single-use ticket that expires in 10 minutes; SAS confirmation; per-device tokens stored hashed, which cannot decrypt anything. |
| Google, Microsoft, Apple or Samsung account takeover | Resets other accounts by email; remote installs through Play [A]; remote wipe; reads the BitLocker recovery key held in the Microsoft account [V]; uses Samsung "Remote unlock" [V; availability varies] | Passkeys or security keys; Google Advanced Protection; Remote unlock off; TaskDeck excluded from cloud backup and device-to-device (D2D) transfer. |
| Malicious update | A trojaned build reaches every device | No auto-updater; Android signature continuity with an offline signing key; install only reviewed or attested builds. |
| Push provider (FCM) | Reads payloads: FCM uses TLS only, with no built-in E2EE [V] | Skip push at first. If added, send empty data messages that only mean "go fetch". |

## 4. Malware delivery

### 4.1 Channels into a device

| Channel | Control |
|---|---|
| Android install or update | Build from a reviewed commit. Sign with an offline key, never in CI. Install via ADB, or register on a free limited-distribution account (no ID needed, up to 50 devices). Google's developer verification goes global from January 2027; unregistered apps will then need ADB or its "advanced flow" [V]. Android rejects updates signed with a different key [A]. No in-app updater. Samsung Auto Blocker blocks non-store installs and USB commands, and is on by default on devices launched with One UI 6.1.1 or later [V]; switch it off only while installing. |
| Windows or macOS install or update | Build locally from a reviewed tag, or use a CI artifact checked with `gh attestation verify` [V]. No updater. |
| Build time (crates, Gradle, npm) | §4.2 |
| Relay data | Process in this order: cap the frame size; check that board, epoch and device are known; verify the Ed25519 signature; open the AEAD; remove padding; parse with serde under size caps; validate in `Board::apply`. An unknown version stops sync and asks for an update. It is never skipped, because skipping makes replicas diverge. |
| ICS feeds | The hardened parser, plus the additions in §3. |
| Open-Meteo | serde under a size cap, numbers only. The phone page already coerces values to numbers before its one `innerHTML` sink (phone.html:1073, 3596-3604) [V]. Round coordinates to 0.01°: the desktop sends full f32 precision (weather.rs:290-297) [V]. |
| QR codes | Read only on an explicit pairing screen; strict versioned schema; no exported deep-link intent filters. |
| Push | Wake-up hint only. |
| Images | Decode only authenticated bytes, in pure Rust under `image::Limits`, as phone.rs:1517-1535 already does [V]. Re-encode on the sending device to strip EXIF. Never pass synced bytes to the platform's C/C++ image decoders (the class of bug behind libwebp CVE-2023-4863). |
| Commits from AI agents and other contributors | Branch, then PR, then human review. Changes to dependencies get the most scrutiny. |

### 4.2 Supply chain

**Exposure today** [V: script output]:
- rustls 0.23.43: RUSTSEC-2026-0285, -0346, -0348 and -0349, all fixed in 0.23.46.
- rustls-webpki 0.103.14 (-0382) and webpki-roots 1.0.9 (-0381).
- tokio 1.52.3: unsound (-0386).
- crossbeam-epoch, quick-xml and quinn-proto/-udp are also flagged. quinn comes in through reqwest's optional http3 support and is probably not compiled.
- chrono is unmaintained (-0543); the advisory suggests jiff or time.

**Controls:**
- **Now:** run `cargo update` on those crates and commit the lockfile. Move to reqwest 0.13, whose default TLS is rustls with rustls-platform-verifier [V]. Certificate distrust then arrives with OS updates.
- **Lockfile discipline:**
  - Build with `--locked`.
  - Update deliberately, and only to releases at least 7 days old. Use `cargo-cooldown` until Cargo's min-publish-age (RFC 3923) ships [U].
  - Why it matters: the malicious arrayref versions were live for under two hours, and the attacker yanked the good versions to steer resolution toward the bad one [V].
- **Gate every lockfile change** with:
  - `cargo deny check`: advisories, sources limited to crates.io, no git dependencies, banned crates;
  - `cargo audit`;
  - `cargo vet` with imported audits, run on a small security-core crate (crypto, envelope, chain, apply).
- **Isolate builds** so no `build.rs` can reach personal secrets. Two options:
  - Ephemeral GitHub runners with no secrets, SHA-pinned actions (GitHub can enforce pinning since August 2025 [V]), immutable releases and attestations [V].
  - A dedicated build VM or user account.

  CI must stay free of secrets: 2026 brought GhostAction, wfx-loot and several compromised third-party actions [V].
- **Android:**
  - Gradle dependency verification: `--write-verification-metadata pgp,sha256`, with the metadata and keyring committed [V].
  - Pin the Gradle wrapper with `distributionSha256Sum`.
  - Use only `google()` and `mavenCentral()`, with no dynamic versions.
- **No npm on personal machines.** Worms in the Shai-Hulud family stole developer and CI credentials throughout 2026 [V]. Write the relay as dependency-free JavaScript, and deploy it from a throwaway container or through the API with a scoped token. The relay's supply chain only affects availability.
- **Could:** reproducible builds, checking that CI and local builds produce the same binary hash.

## 5. Cryptographic protocol

### 5.1 Keys
- **Randomness and derivation:** the OS random-number generator via getrandom, and HKDF-SHA256 with `salt = board_id`.
- **Board id:** 16 random bytes.
- **Epoch root key:** `R_e`, 32 random bytes per epoch. Per-kind subkeys are `K_kind,e = HKDF(R_e, "taskdeck/v1/"‖kind‖u32 e)`, for the kinds entry, snapshot and blob.
- **Each device** creates an Ed25519 signing key and an X25519 key-wrapping key, which never leave it, plus a 256-bit relay token.
- **Recovery:** an age identity printed on paper. Devices keep only its public recipient.

**Why both kinds of key.** One shared epoch key keeps encryption simple: every device reads the same ciphertext. Per-device keys add attribution and make revocation meaningful. A lost device that still holds `R_e` cannot:
- impersonate the other devices;
- author a valid key rotation;
- read the next epoch.

### 5.2 Envelope
A fixed binary header, parsed by offset:

`"TDE1"‖ver‖kind‖board_id‖epoch‖device_id(16)‖counter(u64)‖seen_seq(u64)‖seen_chain(32)‖nonce(24)‖len‖ct‖sig(64)`

- **Ciphertext:** `ct` is the padded plaintext encrypted with XChaCha20-Poly1305 under `K_kind,e`. The associated data (AD) is the whole header.
- **Signature:** `sig` is Ed25519 over `"taskdeck/v1/sig"‖header‖ct`.
- **Idempotency:** `(device_id, counter)` replaces today's X-TaskDeck-Request key.
- **Sequence numbers:** the relay assigns `seq` after encryption, so it cannot be part of the AD. The hash chain binds it instead.

**Why XChaCha20-Poly1305 rather than AES-256-GCM:**
- **Nonces:** several devices encrypt under one key without coordinating, and 192-bit random nonces make collisions negligible. GCM with random 96-bit IVs is capped at 2^32 messages per key (NIST SP 800-38D) [A]. That is enough here, but leaves less margin.
- **Timing:** ChaCha is constant-time in software.
- **Standing:** XChaCha is formally an expired IRTF draft (revision 03) [V]. In practice it is HChaCha20 plus RFC 8439, exactly as in libsodium.
- **Key commitment:** partitioning-oracle attacks [V] do not apply, because the header leaves exactly one candidate key.

### 5.3 Order, replay, rollback and forks
- **Hash chain:** on receipt, each client computes `C_s = SHA-256("taskdeck/v1/chain"‖C_{s-1}‖u64 s‖SHA-256(envelope))`.
- **Gaps and replays:** `seq` must be contiguous, and each device's counter must rise by exactly 1 in chain order. A gap means withholding; a repeat means replay. Both are rejected and raise an alarm.
- **Rollback:** each device stores its head `(s, C_s)` and refuses any log that does not extend it.
- **Forks:** every entry carries the head its author had seen. A receiver whose chain differs at `seen_seq` has found a fork. This is SUNDR's fork consistency [V]: a relay can keep devices apart only by partitioning them permanently, and the first entry that crosses the partition exposes it.
- **Visibility:** Settings shows a sync fingerprint (the sequence number plus four words from `C_s`), and each device posts a signed heartbeat head daily.
- **Snapshots:** a snapshot commits to `(S, C_S)`. A new device accepts a snapshot only if it matches the head received during pairing.

### 5.4 Pairing
The QR code is only a meeting point.
1. The existing device E authenticates its user, then gets a relay ticket: a 128-bit id plus a 256-bit secret, single-use, valid 10 minutes. E shows the relay URL, `board_id` and ticket as a QR code.
2. The new device N sends `commit = SHA-256(xpk_N‖edpk_N‖r_N)` through the relay mailbox.
3. E replies with its ephemeral key `epk_E` and a random value `r_E`. N then opens its commitment.
4. Both screens show `SAS = 6 digits of HKDF(r_E‖r_N‖epk_E‖xpk_N‖edpk_N)`, and the user confirms on both.
5. Encrypted under `X25519(e_E, xpk_N)`, E sends the epoch keys, the head, the device set and the recovery recipient. E then appends a signed add-device entry, and N redeems the ticket for its own token.

Because N commits before it sees `r_E`, a relay in the middle cannot grind keys to match the SAS. It succeeds with probability 10^-6 per attempt, and each attempt is visible (Vaudenay-style SAS) [A]. Photographing the QR code gives a race, not a key.

### 5.5 Revocation, epochs and recovery
- **Revoking a device** deletes its relay token, then appends a signed rotation entry. That entry carries a new `R_{e+1}`, age-encrypted to each remaining device and to the recovery recipient, and commits to the current head.
- **After rotation,** entries from revoked devices are rejected, and so are epoch-e entries placed after the rotation.
- **Rotate** on device loss or suspected compromise. A revoked device keeps whatever it already had.
- **Recovery** needs the paper key plus the log or a backup. The relay keeps the full log: a personal board is megabytes, against 5 GB of free storage.

### 5.6 Metadata
- Pad commands to 512-byte buckets.
- Pad notes, snapshots and blobs with Padmé: at most 12% overhead, and only O(log log M) bits of size information leak [V].
- Keep device names inside ciphertext only.
- Use a constant User-Agent and log no requests [A].

### 5.7 Libraries
Versions are from crates.io on 2026-11-20 [V]. All crypto stays in the Rust core; Kotlin only asks the Android Keystore to wrap one 32-byte secret.

| Use | Crate | Review status |
|---|---|---|
| AEAD | chacha20poly1305 0.11.0 | NCC Group, 2020: no vulnerabilities, but the review covered an old commit [V] |
| Signatures and key exchange | ed25519-dalek 3.0.0, x25519-dalek 3.0.0 | Quarkslab, 2019: curve25519-dalek in depth, the other two lightly. RUSTSEC-2022-0093 and -2024-0344 are fixed [V] |
| Key derivation and hashing | hkdf 0.13.0, sha2 0.11.1 | No dedicated audit found [A] |
| Key wrapping and backups | age 0.12.1, without the `plugin` feature (RUSTSEC-2024-0433) | No formal audit; openSUSE code review [V] |
| Alternative key wrapping | hpke 0.15.1 | No paid audit; Cloudflare reviewed version 0.8 [V] |
| Clearing secrets from memory | zeroize 1.9.2 | — |

**MLS (OpenMLS): no.** MLS provides forward secrecy and post-compromise security for large, changing groups. That buys nothing when every device stores the full plaintext and three devices need only pairwise key wrapping. OpenMLS also had an SRLabs audit in 2026 that found 8 issues (1 High), plus a tag-length bypass (GHSA-8x3w-qj7j-gqhf) [V].

**Post-quantum.** The symmetric parts are fine. X25519 key wraps stored on the relay could be recorded now and decrypted later by a quantum computer. Could: use age's hybrid ML-KEM-768 + X25519 recipients, which Go age supports from version 1.3 [V]; support in the Rust crate is uncertain [U].

## 6. Relay authentication and abuse

- **Tokens:** 256 bits per device, sent in `Authorization` and stored as SHA-256 hashes. Revocation deletes the row. Allow about 8 devices and one live ticket.
- **API:** append-only; no client can delete history.
- **Caps:**
  - 64 KiB per entry;
  - blobs under the 8 MB row limit;
  - per-device rate and byte budgets enforced in the Durable Object;
  - unauthenticated calls refused in the Worker before they reach the Durable Object.
- **Residual quota DoS:** anyone who learns the hostname can exhaust the 100,000-requests-per-day quota, blocking the relay until 00:00 UTC. Whether requests blocked by Cloudflare's WAF count toward that quota is unknown [U]. Clients ride out an outage offline.
- **What the relay stores:** ciphertext, token and ticket hashes, and FCM tokens if push is used.
- **What it can infer:** IP addresses (hence location and travel), activity times (hence routine), device count, and padded sizes.
- **New or second relays:** integrity lives in the clients, so a new relay needs no trust. A seeded log is accepted only if it extends every device's stored head.

## 7. Local protection

**Windows:**
- DPAPI or Credential Manager (keyring-core's Windows store) keeps secrets out of plain file copies. Any process running as the same user can still decrypt them [V].
- Data lives in `%APPDATA%\TaskDeck` (paths.rs:273-281).
- Turn on Device Encryption or BitLocker. Its recovery key sits in the Microsoft account [V].

**macOS:**
- Store secrets in the login Keychain: other apps must prompt before reading them [A].
- Sign every build with one stable self-signed identity; otherwise each rebuild prompts again.
- Turn on FileVault.

**Android:**
- Keep data in app-private storage, protected by FBE.
- Wrap secrets with a non-exportable Keystore AES-GCM key, in StrongBox when the phone has one [V].
- Set `allowBackup=false` **and** `dataExtractionRules` excluding everything under both `<cloud-backup>` and `<device-transfer>`. Some manufacturers ignore `allowBackup=false` for D2D transfer [V].
- Use FLAG_SECURE on screens that show secrets, export only the launcher activity, and turn cleartext traffic off.
- Use `setUnlockedDeviceRequired` [V] only if nothing has to decrypt while the phone is locked.

**App-level encryption of local copies** is of low value: it adds little on top of full-disk encryption and does not stop malware running as the user. Could: cache the relay's ciphertext instead of plaintext.

## 8. Backups

- **Nightly:** the desktop exports the board, archive, notes and log, age-encrypted to the paper key's public recipient. It needs no secret to do this.
- **Where:**
  - a second local disk;
  - any cloud drive, which sees only ciphertext;
  - an occasional offline USB copy.

  Keep 30 daily and 12 monthly copies.
- **The paper key:** keep a sealed copy off-site as well, never next to the backups.
- **Restore drill, quarterly:** on another machine, decrypt with the stock `age` CLI, load into a temporary data directory, and check item counts and the latest edits.

## 9. Accounts

The email account is the root, because it can reset GitHub, Cloudflare, Google, Microsoft, Apple and Samsung.
- **Must:** passkeys everywhere, SMS removed as a recovery method, recovery codes on paper.
- **Cloudflare:** a deploy token scoped to Workers Scripts:Edit, IP-filtered and with a TTL, because tokens otherwise never expire [V]. Never use the Global API key.
- **GitHub:** fine-grained personal access tokens with expiry dates.
- **Should:** two hardware keys for email, GitHub and Cloudflare; Google Advanced Protection; Samsung Remote unlock off.

## 10. What cannot be protected

- **Malware controlling an unlocked device** sees what you see and holds that device's keys. Rotation limits how long it keeps access, not what it has already seen.
- **A thief holding your unlocked phone** has full access until you revoke it.
- **OS vendors** have the last word on every device.
- **A malicious change in a trusted dependency** that passes review and the cooldown, as with the xz backdoor.
- **Availability** of a free service, and the **metadata** the relay sees.
- **Forks** are detected only once devices exchange information.
- **Losing every device and the paper key** loses the data, by design.

## 11. Recommendation

**Must:**
1. Native clients only; the relay never serves code. If a WebView is used, it loads bundled assets only, with the network blocked, and the key never enters JavaScript.
2. The §5.2 envelope: authenticate before parsing, and refuse unknown versions.
3. Hash chain, per-device counters, stored heads and seen-heads.
4. Per-device keys and signed control entries.
5. SAS pairing, with no key ever placed in a QR code.
6. Revocation followed by key rotation.
7. Hashed per-device tokens, https only, an append-only API, size and rate caps.
8. A paper recovery key, age-encrypted backups in two places, and a quarterly restore drill.
9. Supply chain:
   - fix today's advisories;
   - build with `--locked`;
   - gate lockfile changes with cargo-deny and cargo-audit;
   - wait at least 7 days before adopting a new release;
   - review every dependency change, including AI-authored ones.
10. Android: keep the APK signing key offline; ship no auto-updater; exclude TaskDeck from backup and D2D; wrap secrets with the Keystore; gate sensitive actions behind device authentication.
11. Passkeys on every account, and a scoped, expiring Cloudflare token.
12. FCM only with empty payloads.

**Should:**
- cargo-vet on a small core crate.
- Isolated builds: secret-free CI with attestations, or a build VM.
- Gradle dependency verification.
- Padding.
- OS keystores on the desktops.
- Refusing ICS hosts in private address ranges.
- Pure-Rust image decoding.
- Alerts, plus restore to a chosen sequence number.
- The sync fingerprint.
- reqwest 0.13 with the platform certificate verifier.
- Hardware keys and Google Advanced Protection.

**Could:**
- Encrypted local caches.
- Post-quantum key wrapping.
- Reproducible builds.
- `setUnlockedDeviceRequired`.
- An on-phone calendar mirror (Android CalendarContract) instead of a plaintext ICS feed. Today's feed URL carries the read/write token (phone.rs:1917-1923), and the docs suggest giving that URL to Google Calendar (DOCUMENTATION.md:2893) [V].

**Residual risk after Must and Should:**
- **Remote malware or data theft through TaskDeck's own channels** (relay, ICS, weather, push, QR): very low. None of them carries code, parsing is memory-safe and size-capped, and synced data is authenticated.
- **Losing a whole board** still has three routes:
  - **A compromised device:** moderate likelihood; the main residual risk.
  - **A poisoned dependency** that slips past the lockfile, cooldown, audit and vet: low likelihood, high impact.
  - **An account takeover combined with your own build habits:** low with passkeys.
- **Accepted:** metadata exposure and quota denial of service.

---

## Key facts

- **[VERIFIED]** TaskDeck's committed Cargo.lock currently matches 2026 RustSec advisories, including RUSTSEC-2026-0381 (webpki-roots 1.0.9 ships emSign ECC Root CA - G3) and RUSTSEC-2026-0382 (rustls-webpki 0.103.14). Together they may accept chains through distrusted emSign intermediates whose private keys are 'now well known'. _(evidence: python3 -I <scratch>/scripts/lockaudit.py run against rustsec/advisory-db clone at commit f291af9 (2026-11-20) in <scratch>/advdb/db; advisory files crates/webpki-roots/RUSTSEC-2026-0381.md and crates/rustls-webpki/RUSTSEC-2026-0382.md)_
- **[VERIFIED]** The lockfile's rustls 0.23.43 is affected by RUSTSEC-2026-0285, -0346, -0348 and -0349 (patched in 0.23.45 or 0.23.46). Also flagged: tokio 1.52.3 (unsound, -0386), crossbeam-epoch, quick-xml and quinn-proto/quinn-udp. quinn is probably not compiled, because it is reqwest's optional http3 dependency. _(evidence: lockaudit.py output (same run); https://rustsec.org/advisories/RUSTSEC-2026-0349)_
- **[VERIFIED]** chrono, a core TaskDeck dependency, is flagged unmaintained (RUSTSEC-2026-0543, 2026-09-14). The advisory recommends jiff or time. _(evidence: advdb/db/crates/chrono/RUSTSEC-2026-0543.md; Cargo.lock chrono 0.4.45)_
- **[VERIFIED]** TaskDeck enables reqwest's rustls-tls feature. In reqwest 0.12.28 that feature equals rustls-tls-webpki-roots, a bundled root store that only changes when the app is rebuilt. The weather and ICS fetchers both use it. _(evidence: Cargo.toml (reqwest = { version = "0.12", default-features = false, features = ["blocking", "rustls-tls"] }); ~/.cargo/registry/src/*/reqwest-0.12.28/Cargo.toml:148)_
- **[VERIFIED]** reqwest 0.13.5, the latest stable, defaults to rustls with rustls-platform-verifier (the OS trust store) and aws-lc-rs. _(evidence: crates.io API https://crates.io/api/v1/crates/reqwest/0.13.5 features: default-tls = ['rustls']; rustls = ['__rustls-aws-lc-rs','dep:rustls-platform-verifier','__rustls'])_
- **[VERIFIED]** arrayref 0.3.10, published 2026-08-20 from a compromised maintainer account, ran malware from the build.rs of a typosquatted proc-macro1 dependency and persisted itself (HKCU Run key, LaunchAgent or systemd user service). The attacker also yanked legitimate versions. TaskDeck depends on arrayref through tiny-skia and is pinned at 0.3.9 (since 2026-06-26). _(evidence: Cargo.lock:160-161; git log -S'name = "arrayref"' -- Cargo.lock (f653361, 2026-06-26); https://socket.dev/blog/popular-rust-crates-compromised ; https://www.theregister.com/security/2026/08/21/hackers-poison-popular-rust-crates-to-steal-developers-credentials/5291075)_
- **[VERIFIED]** The repository has no CI configuration: there is no .github directory. _(evidence: ls -a of the day-planner worktree)_
- **[VERIFIED]** The desktop sync client accepts http:// server addresses; it relied on Tailscale for transport security. _(evidence: src/sync.rs:132-136)_
- **[VERIFIED]** Today one read/write token appears as ?token= in the phone link, the QR code and the /calendar.ics feed URL, and the docs suggest subscribing to the feed from Google Calendar. _(evidence: src/phone.rs:1005-1041, 1903-1923; Cargo.toml qrcode comment; DOCUMENTATION.md:2893)_
- **[VERIFIED]** ICS fetching already has https_only, at most 3 redirects, a 20 s timeout, an 8 MiB cap and parser limits, but checked_url does no filtering of IP-literal or private-range hosts. _(evidence: src/subscriptions.rs:93-95, 156-180, 732-742; src/ics.rs:107-135)_
- **[VERIFIED]** Images uploaded from the phone are decoded with image::Limits: at most 20,000 px per side, an allocation cap equal to 40 megapixels at 4 bytes per pixel, and a 6 MiB body cap. _(evidence: src/phone.rs:1517-1535)_
- **[VERIFIED]** The desktop sends full-precision configured coordinates to Open-Meteo; the phone page rounds GPS coordinates to 3 decimals. _(evidence: src/weather.rs:290-297; src/phone.html:3862-3863)_
- **[VERIFIED]** The phone page has one innerHTML sink (the el() helper). Weather values reach it only as Math.round-coerced numbers and xml()-escaped words. The CSP allows 'unsafe-inline' scripts. _(evidence: src/phone.html:1073, 3055, 3596-3604; src/phone.rs:1209-1219)_
- **[VERIFIED]** Per-user data lives in %APPDATA%\TaskDeck on Windows (Roaming) and ~/Library/Application Support/TaskDeck on macOS. _(evidence: src/paths.rs:267-281)_
- **[VERIFIED]** NCC Group reviewed RustCrypto's aes-gcm and chacha20poly1305 crates (public report Feb 2020, commissioned by MobileCoin) and found no vulnerabilities. The review was pinned to an old commit, not the current 0.11. _(evidence: https://www.nccgroup.com/research/public-report-rustcrypto-aesgcm-and-chacha20pluspoly1305-implementation-review/)_
- **[VERIFIED]** Quarkslab's 2019 audit for Tari covered curve25519-dalek in depth and ed25519-dalek and x25519-dalek only marginally. RUSTSEC-2022-0093 (ed25519-dalek, fixed in 2.0) and RUSTSEC-2024-0344 (curve25519-dalek, fixed in 4.1.3) are fixed. _(evidence: https://blog.quarkslab.com/security-audit-of-dalek-libraries.html ; advdb/db/crates/ed25519-dalek/RUSTSEC-2022-0093.md ; https://osv.dev/vulnerability/RUSTSEC-2024-0344)_
- **[VERIFIED]** rust-hpke (crate hpke) has had no paid audit; Cloudflare did an internal review of version 0.8. _(evidence: https://docs.rs/crate/hpke/0.14.0 ; https://github.com/rozbb/rust-hpke)_
- **[UNCERTAIN]** No published formal audit of age or rage was found, only a limited openSUSE code review. RUSTSEC-2024-0433 shows that malicious age plugin names, recipients or identities could cause arbitrary binary execution. _(evidence: https://lists.opensuse.org/archives/list/bugs@lists.opensuse.org/thread/I6R5LFAVQPZ4GFMBRA3KFVTU64GXYV4S ; advdb/db/crates/age/RUSTSEC-2024-0433.md)_
- **[VERIFIED]** OpenMLS had an SRLabs audit in 2026 that found 8 issues, 1 rated High. Separately, GHSA-8x3w-qj7j-gqhf (Feb 2026, fixed in openmls 0.7.2) let truncated or empty tags pass validation. _(evidence: https://advisories.gitlab.com/pkg/cargo/openmls/GHSA-8x3w-qj7j-gqhf/ ; https://hackernews-dynamic.seasondane.workers.dev/news/48307020 (secondary))_
- **[VERIFIED]** The XChaCha20 Internet-Draft (draft-irtf-cfrg-xchacha) has expired; its last revision is 03, dated 2020-01-10. _(evidence: https://datatracker.ietf.org/doc/draft-irtf-cfrg-xchacha/)_
- **[VERIFIED]** Android developer verification has been enforced since 2026-09-30 in Brazil, Indonesia, Singapore and Thailand. The global rollout begins January 2027 and expands to all install sources over several months. Unregistered apps then need ADB or the 'advanced flow'. Free limited-distribution accounts need no government ID and allow up to 50 devices. _(evidence: https://developer.android.com/blog/posts/android-developer-verification-2027.md.txt (19 Nov 2026); https://developer.android.com/blog/posts/android-developer-verification-rolling-out-to-all-developers-on-play-console-and-android-developer-console)_
- **[VERIFIED]** On Android 12 and later, allowBackup="false" may not disable device-to-device transfer on some manufacturers' devices; the dataExtractionRules <device-transfer> section controls it. _(evidence: https://developer.android.com/about/versions/12/behavior-changes-12 (Backup and restore section))_
- **[UNCERTAIN]** Samsung Auto Blocker blocks app installs from outside official stores and blocks commands over USB. It is on by default for devices launched with One UI 6.1.1 or later. Whether it blocks ADB specifically is not documented. _(evidence: https://docs.samsungknox.com/admin/fundamentals/whitepaper/samsung-knox-mobile-security/system-security/samsung-auto-blocker/ ; https://www.androidauthority.com/enable-sideloading-one-ui-6-1-1-3463446)_
- **[VERIFIED]** FCM is protected by TLS only and has no built-in end-to-end encryption. Google suggests encrypting payloads yourself or sending empty data messages as fetch signals. The payload limit is 4096 bytes. _(evidence: https://firebase.google.com/docs/cloud-messaging/encryption)_
- **[VERIFIED]** GitHub immutable releases became generally available on 2025-10-28. Since 2025-08-15, Actions policies can block actions and require them to be pinned to full commit SHAs. _(evidence: https://github.blog/changelog/2025-10-28-immutable-releases-are-now-generally-available/ ; https://github.blog/changelog/2025-08-15-github-actions-policy-now-supports-blocking-and-sha-pinning-actions/)_
- **[VERIFIED]** 2026 saw several CI secret-theft campaigns (GhostAction, wfx-loot in Nov 2026, compromised third-party actions) and npm worms of the Shai-Hulud family that stole developer and CI credentials. Exact figures differ between sources. _(evidence: https://www.endorlabs.com/learn/github-actions-supply-chain-attack-targeting-ci-secrets-at-scale ; https://thehackernews.com/2026/05/github-actions-supply-chain-attack.html ; https://unit42.paloaltonetworks.com/monitoring-npm-supply-chain-attacks/)_
- **[UNCERTAIN]** Cargo's minimum-publish-age (RFC 3923) exists as an RFC, but its stabilization is unconfirmed. The third-party tool cargo-cooldown 0.3.1 is available now. _(evidence: https://rust-lang.github.io/rfcs/3923-cargo-min-publish-age.html ; https://docs.rs/crate/cargo-cooldown/0.3.1)_
- **[VERIFIED]** DPAPI (CryptProtectData) data can normally be decrypted only with the same user's logon credentials on the same computer; CRYPTPROTECT_LOCAL_MACHINE widens this to any local user. That any process running as the same user can decrypt is implied by the docs rather than stated. _(evidence: https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptprotectdata)_
- **[VERIFIED]** Windows 11 24H2 turns on device encryption automatically when the user signs in with a Microsoft account, and backs the recovery key up to that account. _(evidence: https://www.tomshardware.com/software/windows/windows-11-24h2-will-enable-bitlocker-encryption-for-everyone-happens-on-both-clean-installs-and-reinstalls (secondary))_
- **[VERIFIED]** Android Advanced Protection includes Inactivity Reboot, which restarts a phone left locked for 72 hours and returns user data to its encrypted state. _(evidence: https://support.google.com/android/answer/16339980)_
- **[VERIFIED]** Cloudflare API tokens can be restricted by client IP and given a TTL; by default they never expire. _(evidence: https://developers.cloudflare.com/fundamentals/api/how-to/restrict-tokens/)_
- **[UNCERTAIN]** It is unknown whether requests blocked by a Cloudflare WAF rule count against the Workers free-plan request quota. _(evidence: No official statement found; https://community.cloudflare.com/t/workers-behavior-with-firewall-rules/109182)_
- **[UNCERTAIN]** Go age 1.3 and later supports post-quantum hybrid recipients (mlkem768x25519, age1pq1...). Support in the Rust age crate is unconfirmed. _(evidence: https://pkg.go.dev/filippo.io/age@v1.3.1)_
- **[VERIFIED]** Latest stable crate versions on 2026-11-20: chacha20poly1305 0.11.0, ed25519-dalek 3.0.0, x25519-dalek 3.0.0, curve25519-dalek 5.0.0, hkdf 0.13.0, sha2 0.11.1, zeroize 1.9.2, age 0.12.1, hpke 0.15.1, openmls 0.9.2, keyring 4.2.0, rustls 0.23.46, rustls-webpki 0.103.17, webpki-roots 1.0.12, rustls-platform-verifier 0.7.1, cargo-deny 0.22.0, cargo-audit 0.23.0, cargo-vet 0.10.3. _(evidence: crates.io API queries https://crates.io/api/v1/crates/<name> (max_stable_version))_
- **[VERIFIED]** SUNDR (OSDI 2004) defines fork consistency: an untrusted server that hides one client's updates from another must keep their views permanently forked, and the fork is detected once they see each other's operations. _(evidence: https://www.usenix.org/legacy/event/osdi04/tech/li_j.html)_
- **[VERIFIED]** Padmé padding limits length leakage to O(log log M) bits with at most 12% overhead. _(evidence: https://petsymposium.org/popets/2019/popets-2019-0056.php)_
- **[VERIFIED]** Partitioning-oracle attacks exploit AEADs that do not commit to their key, including AES-GCM and ChaCha20-Poly1305. _(evidence: https://usenix.org/conference/usenixsecurity21/presentation/len)_
- **[ASSUMED]** NIST SP 800-38D limits AES-GCM with random 96-bit IVs to 2^32 invocations per key. _(evidence: Standard reference (NIST SP 800-38D section 8.3); not re-fetched in this session)_
- **[ASSUMED]** Android refuses to install an update signed with a different key unless an APK Signature Scheme v3 rotation proof is present. _(evidence: Platform behaviour (source.android.com APK signature scheme v3); not re-fetched)_
- **[UNCERTAIN]** Samsung Find's 'Remote unlock' can unlock a phone remotely once enabled, but its availability has varied over time. _(evidence: https://www.samsung.com/latin_en/support/mobile-devices/how-can-i-unlock-my-galaxy-device-if-i-forgot-the-security-pin-pattern-or-password/ ; https://r2.community.samsung.com/t5/Galaxy-S/Remote-unlock-removed/td-p/15585760)_

## Recommendations

- MUST (now): refresh the lockfile so that rustls is >= 0.23.46, rustls-webpki >= 0.103.16 and webpki-roots >= 1.0.10. This closes the emSign-intermediate exposure in the ICS and weather fetchers, which an attacker could use to intercept TLS. Then plan the move to reqwest 0.13 (rustls-platform-verifier) and assess replacing chrono, which is now unmaintained.
- MUST: only native clients may hold keys, and the relay never serves code. If the Android app uses a WebView, it loads bundled assets only, network access from the WebView is blocked, and keys never enter JavaScript.
- MUST: use one envelope everywhere. Encrypt with XChaCha20-Poly1305 (chacha20poly1305 0.11), using the whole header as associated data, per-kind HKDF-SHA256 subkeys and random 24-byte nonces, and sign header||ciphertext with Ed25519. Process strictly in the order size cap, signature, AEAD, parse, Board::apply validation. Refuse unknown versions instead of skipping them.
- MUST: clients enforce integrity themselves. Each client computes a hash chain over the relay's order, requires contiguous sequence numbers and per-device counters that rise by exactly one, persists its head, and puts its last-seen head in every entry. Gaps, replays, rollbacks and forks raise alarms. Pairing hands the current head to the new device.
- MUST: give each device its own Ed25519 and X25519 keys, and sign every device-set and rotation entry. Revoking a device means deleting its relay token and rotating to a new epoch whose key is age-wrapped to the remaining devices and to the recovery recipient.
- MUST: pair through a QR code that holds only a rendezvous ticket (single use, valid 10 minutes), followed by a commit-then-reveal 6-digit SAS confirmed on both screens. Never put a key in a QR code. Require device authentication before pairing, before showing the recovery key, and before revoking a device.
- MUST: on the relay, give each device a 256-bit token sent in the Authorization header (never in a URL) and store only its SHA-256 hash. Use an append-only API, cap entry and blob sizes, enforce per-device budgets, accept https/wss only, and keep no request logs.
- MUST: print a paper recovery key (an age identity). Write nightly age-encrypted backups from the desktop to two locations plus an occasional offline copy, and run a quarterly restore drill with the stock age CLI.
- MUST: supply-chain gates. Build with --locked. Run cargo deny (advisories, sources limited to crates.io, no git dependencies) and cargo audit on every lockfile change. Adopt new releases only after a cooldown of at least 7 days (cargo-cooldown). Review every dependency diff, including AI-agent-authored ones. Keep npm/wrangler off personal machines and deploy the dependency-free relay from a throwaway container or through the API.
- MUST (Android): keep the APK signing key offline and never in CI; have no in-app or automatic updater on any platform. Set allowBackup=false plus dataExtractionRules that exclude both cloud-backup and device-transfer. Wrap secrets with a Keystore key (StrongBox when present), set FLAG_SECURE on screens that show secrets, and export only the launcher. Install via ADB or a free limited-distribution registration.
- MUST (accounts): use passkeys or security keys on the email, GitHub, Cloudflare, Google, Microsoft, Apple and Samsung accounts; remove SMS recovery; keep recovery codes on paper. Deploy with a Cloudflare token scoped to Workers Scripts:Edit, filtered by IP and given a TTL; never use the Global API key.
- SHOULD: isolate builds from personal secrets. Either build on secret-free GitHub Actions (SHA-pinned actions, read-only token) with immutable releases and artifact attestations checked with gh attestation verify before installing, or build in a dedicated VM or user account. Run cargo-vet with imported audits on a small security-core crate.
- SHOULD: on Android, enable Gradle dependency verification (pgp+sha256, committed), pin the wrapper's distributionSha256Sum, restrict repositories to google() and mavenCentral(), and keep Android dependencies minimal.
- SHOULD: reduce metadata. Pad commands to 512-byte buckets and notes, snapshots and blobs with Padmé; send a constant User-Agent; keep device names only inside ciphertext.
- SHOULD: build containment and detection: alerts on a new device, a new subscription or a mass deletion; restore to a chosen sequence number; a sync fingerprint in Settings; a daily signed heartbeat head from each device.
- SHOULD: harden other inputs. Refuse IP-literal and private-range ICS hosts and never fetch ATTACH or URL values. Decode synced images only in pure Rust under limits, and re-encode them on the sending device to strip EXIF. Round Open-Meteo coordinates to 0.01°.
- SHOULD: store desktop secrets in the OS keystore (keyring-core's Windows and Apple stores) and confirm BitLocker/FileVault are on. Get two hardware security keys, enable Google Advanced Protection, and turn Samsung Remote unlock off.
- COULD: use post-quantum hybrid key wrapping (age mlkem768x25519 if the Rust crate supports it, otherwise X-Wing or ML-KEM); reproducible builds cross-checked between CI and local; local caches kept encrypted; setUnlockedDeviceRequired if nothing must decrypt while the phone is locked; an Android CalendarContract mirror (local account) instead of the plaintext ICS feed; FCM only as empty wake-up messages.

## Open questions

- Does the Rust age crate (0.12.x) support age v1.3's mlkem768x25519 post-quantum recipients? If not, is the harvest-now-decrypt-later risk on relay-stored key wraps acceptable?
- On the user's One UI version, does Samsung Auto Blocker's USB-command blocking also block `adb install`? This decides the exact install and update procedure.
- Is the user in a region where developer verification is already enforced, or when will the 2027 global rollout reach them? This decides between relying on ADB and registering the app on a free limited-distribution account.
- Do requests blocked by a Cloudflare WAF rule count against the Workers free-plan request quota? If they do not, would a custom domain (not free) be worth it as quota-DoS protection?
- Which Android UI approach will be chosen: Compose with UniFFI (which adds JNA), a WebView reusing phone.html, or egui? This sets the Gradle/Maven dependency surface and whether WebView XSS controls are needed.
- Must the phone decrypt while locked (widgets, reminders, refreshes triggered by push)? This decides setUnlockedDeviceRequired and whether FCM is needed at all.
- Does the user want an external ICS feed (for example Google Calendar 'From URL')? If so, that read-only subset leaves end-to-end encryption and needs a separate read-only capability.
- Are quinn/quinn-proto and the other flagged crates actually compiled into either binary? `cargo tree -e features` would tell; the answer only affects triage, since cargo-audit flags them from the lockfile.
- Has Cargo's min-publish-age (RFC 3923) been stabilized, or should cargo-cooldown be used in the meantime?
- Is the repository public or private? On a private repo, GitHub Actions minutes are limited and macOS runners count at 10x, which affects whether isolated CI builds stay free.
- Is Samsung 'Remote unlock' enabled on the user's phone?
