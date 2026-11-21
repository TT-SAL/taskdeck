# V1: Security attack (adversarial verification)

> Working paper from the design run of 2026-11-20 that produced [`SYNC.md`](../../../SYNC.md). It is an agent's report, kept for the detail the design compresses: file and line references, sources, and the reasoning behind each requirement. Where it disagrees with `SYNC.md` or [`SYNC_PLAN.md`](../../../SYNC_PLAN.md), those win. Line numbers refer to `day-planner` at commit `1cfff2f`.

Verdict: **sound with changes**. Issues are numbered as `SYNC.md` cites them (V1-1, V1-2, …). Each was raised against the draft design; `SYNC.md`'s *Verification outcomes* says what changed.

## V1-1 · high · The device signing key works as a signing oracle for the relay, because the login challenge is not domain-separated

**Scenario.** The attacker controls the relay, through a Cloudflare account takeover or a malicious deploy. When the phone next renews its token (the relay can force this by returning 401), the relay sends as the 'challenge' the bytes 'taskdeck/frame/v1' + 'TDF1' + board_id + dev_phone + (last_ctr+1) + len + body, with body.ver = 0xFF. Suppose the phone signs the challenge bytes as given, or prefixes them with a login string that is itself a prefix of 'taskdeck/frame/v1', such as 'taskdeck/'. The relay then holds a valid phone signature over a frame the phone never wrote, and sequences it.

Every replica passes the size, membership and signature checks, then meets an unknown ver. Under §4.2 it halts applying for good ('Update TaskDeck'), although no build will ever understand ver 0xFF. The frame is fsynced into every local log. It is also resubmitted to the standby during a re-home (§6.7 step 4, 'resubmits the entries it holds beyond the seed'), so the wedge follows the board.

If the relay also has the old epoch key of any revoked device (a phone that was stolen and then revoked, a retired laptop), it can do worse. It fills the body with a valid epoch-e ciphertext whose seen_seq and seen_chain predate the rotation. §5.4 accepts that as an entry authored by a current member: arbitrary commands, AddSubscription to an attacker URL, or a DeviceAdded.

**Why it fails.** §5.2 says only that a device 'signs a relay challenge with its Ed25519 key'. The same key signs frames (§4.2), heartbeats and M1 hub frames, and the challenge bytes are chosen by the party the design treats as untrusted. The §4.2 claim that 'an injected frame cannot wedge clients' assumes the relay can never obtain a valid member signature. Two client rules are also missing:
- what to do with a validly signed frame whose AEAD, unpad or serde step fails (if the answer is 'halt', any signer can wedge every device);
- an alarm when a frame carries this device's own dev id but the device never authored it (the echo check covers only the device's acknowledged entries).

**Required change.** - Sign logins over a fixed, prefix-free structure: 'taskdeck/login/v1' + 0x00 + relay origin + board_id + dev + gen + relay_nonce[32] + client_nonce[32]. Refuse any challenge that is not exactly 32 bytes.
- Give every signature use (frame, login, hub request, hub reply, relay admin operation, backup) its own NUL-terminated context string, or derive a separate login key with HKDF.
- Rule: a validly signed frame that fails AEAD, unpad or serde is skipped with an alarm naming its author. It never halts.
- Rule: a frame with this device's dev id that it did not author raises 'someone is signing as this device: revoke and re-pair'.
- Add a conformance test in which the relay offers a frame prefix as the challenge and the client refuses it.

**Evidence.** Design §4.2: the frame signature is Ed25519 over 'taskdeck/frame/v1' + every preceding byte, and 'Only a validly signed frame with an unknown ver … halts applying'. §5.2: login by signing a relay challenge. §5.4: epoch-e entries sequenced after a rotation are accepted if the author is a member and seen_seq predates the rotation. §6.7 step 4: resubmission beyond the seed. Ed25519 signs messages of any length (RFC 8032).

## V1-2 · high · Any one device can silently replace the recovery key, so the attacker keeps access after revocation and new backups no longer open with the paper sheet

**Scenario.** Malware runs for an hour on the Windows desktop, or a thief holds the unlocked phone and has watched the PIN. Using the device's own key and token, with no UI involved, it appends RecoveryKeyChanged{attacker key} together with an EpochRotated. From then on:
- Every nightly backup from the honest Desk and Mac is HPKE-sealed to the attacker's key (§6.6).
- Every later rotation carries a wrap of the new epoch root 'for the recovery key' (§5.4), which now means for the attacker. That includes the rotation the user runs when revoking the culprit.
- The attacker can read those wraps and all later entries in three ways: the read-only relay token embedded in backups (the compromised writer minted or saw it, and it is not tied to that device); any later relay access; or the backup files in OneDrive.

The user's paper sheet no longer opens any backup written after the change. Once the 30 daily and 12 monthly copies roll over, it opens nothing recent, and losing every device then loses that data permanently.

**Why it fails.** - RecoveryKeyChanged is an ordinary ctl kind that any member may author (§4.2).
- 'New recovery key' does not require the old sheet (§5.5).
- A rotation wraps the new root to whatever recovery key the replicated state names.
- At M2 only DeviceAdded gets a banner (§5.3 step 7); alerts for an epoch change arrive in Phase 7 (§10).
- Read-only tokens are not bound to a device or an epoch (§5.2).

**Required change.** 1. RecoveryKeyChanged must be signed by a key derived from the current recovery seed, so the user has to type in the old sheet. The lost-sheet path needs a second device's co-signature and a 72-hour delay, with banners throughout.
2. At M2, show a persistent banner on every other device for RecoveryKeyChanged, EpochRotated, DeviceRevoked, Fence and ResetTo, with the author's key fingerprint.
3. Each backup writer pins the recovery fingerprint confirmed on its own screen. It seals to a new key only after the user confirms it there.
4. Mint read-only tokens per writer device and generation. Revoke them with that device and on every rotation.

**Evidence.** Design §4.2 (the ctl kinds include RecoveryKeyChanged), §5.2 ('Backups carry a read-only relay token'), §5.4 (wraps 'for each remaining device and for the recovery key'), §5.5 ('The sheet is lost: New recovery key rotates the epoch'), §6.6 (backups HPKE-sealed to the recovery public key), §10 Phase 7 alert list.

## V1-3 · high · 'Rotate on suspected compromise' keeps the compromised device's long-term keys and relay token

**Scenario.** Malware on the Windows desktop unwraps L_d. Under DPAPI user scope, any process of that user can do this. It copies the Desk's X25519 and Ed25519 private keys, its relay token and the epoch roots. The user cleans the machine and rotates, as §5.4 and R4 prescribe.

The new root R_{e+1} is HPKE-wrapped to every remaining device, including the Desk's unchanged X25519 key, inside the log. The attacker reads that log from anywhere with the Desk's still-valid token. It can also keep appending entries as the Desk. The rotation achieves nothing, and the user believes the incident is closed.

**Why it fails.** A rotation renews only the epoch root. Device keys 'never leave d' and are never replaced (§5.2); there is no operation that re-keys a device, and only revocation kills a token. §5.6 concedes that the sealed store does not stop malware running as the user.

**Required change.** Make 'suspected compromise' one guided action:
1. Revoke the device.
2. Re-pair it as a new device with a fresh Ed25519 key, X25519 key, L_d, token and device_no.
3. Rotate the epoch and bump the read tokens.

Never offer a rotation that keeps a possibly exposed member. The runbook should say plainly that cleaning a machine is not enough.

**Evidence.** Design §5.2 (key hierarchy), §5.4 ('Rotate on revocation, on suspected compromise'), §5.6 ('not malware running as the user'). R4 adversary table: 'Malware on one device … Rotate keys after cleanup'. R4 fact on DPAPI user scope.

## V1-4 · high · CI can deliver a trojaned release that still passes 'verified': attestation checks are loose and Actions caches are shared

**Scenario.** (a) With control of the GitHub account, the attacker pushes a branch whose workflow builds a trojaned TaskDeck.exe or APK and attests it. 'gh attestation verify --repo O/R' passes, because by default it does not pin the workflow, the ref or the commit.

(b) Without the account, one compromised test-only or tool dependency is enough: a transitive crate of proptest or cargo-fuzz, or of cargo-deny's own tree. Its build.rs runs in a main-branch CI job and poisons the Actions cache, as in Cacheract (and the 2026 Clinejection attack on Cline's release pipeline). The tag build then restores the poisoned cache. The artifact is attested as built from your signed tag's commit; you sign the APK offline and install it with adb. The result is code execution on all three devices.

(c) Tags are signed on whatever main is after 'reviewed PRs', and an attacker who holds the account can change main.

(d) Running 'cargo install' for cargo-deny, cargo-audit, cargo-vet, cargo-cooldown, cargo-ndk or cargo-fuzz without --locked resolves fresh transitive versions and runs their build scripts, outside any cooldown.

**Why it fails.** §8 says only 'provenance attestations that you check against your tag'. It says nothing about caches, about reviewing the diff before signing a tag, or about how tools are installed. The residual §7 lists for updates ('a malicious commit you approve') assumes the attested artifact faithfully reflects the source.

**Required change.** - Spell out the check: gh attestation verify <file> --repo O/R --signer-workflow O/R/.github/workflows/release.yml --source-ref refs/tags/vX --source-digest <commit from git verify-tag> --deny-self-hosted-runners.
- The release workflow runs only on tag push and neither restores nor saves caches: no actions/cache, no rust-cache, no cache options on setup-* actions. Test jobs never write caches that release jobs could read.
- Before git tag -s, review git diff <last-signed-tag>..HEAD locally, looking at Cargo.lock, build.rs files and .github first.
- Install tools only with cargo install --locked --version, or as verified release binaries, and only in the build account.
- For each release, rebuild one artifact locally and compare hashes.

**Evidence.** VERIFIED: the gh manual makes --signer-workflow, --source-ref and --source-digest opt-in; by default only the predicate type is enforced (https://cli.github.com/manual/gh_attestation_verify). Cache-poisoning research: https://adnanthekhan.com/2024/12/21/cacheract-the-monster-in-your-build-cache/ and https://adnanthekhan.com/posts/clinejection. Design §7 'Malware via updates' row; §8 CI.

## V1-5 · medium · One device, or one leaked token, can lock every other device out, and nothing can overrule it

**Scenario.** A phone thief who watched the PIN passes BiometricPrompt's device-credential fallback and revokes the Desk and the Mac. The relay deletes their signing keys and their tokens fail at once, so they cannot even read who did it.

The same happens if malware, or anyone holding a leaked token, calls the relay's revoke or fence operations directly. A token can leak, for example, through a TLS-intercepting proxy on the laptop's network. These are plain relay calls that the relay cannot tie to a signed ctl entry, because the entry's kind is inside the ciphertext.

The user's only route back is 'Reset trust' plus a re-home. That takes Cloudflare access, a new deploy with a fresh CREATE_SECRET, and 1–2 hours. Meanwhile the attacker keeps the old board.

**Why it fails.** - §5.4 lets any device revoke any other device.
- Tokens are described as 'availability only', yet they authorise admin operations.
- §5.5 makes the recovery key deliberately powerless in the live board.
- The Android gate accepts the PIN.

**Required change.** - Relay admin operations (revoke, fence, register device, seed, compaction) require a fresh, domain-separated signature from a member, not only a token.
- Add a recovery signer: an Ed25519 key derived from the paper seed and registered at creation and in the genesis entry. Its signed ctl entries can revoke any device, reinstate devices, and override membership changes signed by devices. It still cannot enrol devices.
- On Android, pairing, revocation, recovery and moves require BIOMETRIC_STRONG with no PIN fallback.

**Evidence.** Design §5.3 step 1 ('BiometricPrompt or the device credential'), §5.4 ('Any device can revoke another … its token and its appends fail at once'), §5.5 ('Recovery never enrols into the live board'), §6.7 (fence to reads only), §2.1 (CREATE_SECRET deleted after creation).

## V1-6 · medium · Pairing: the SAS transcript is under-specified, compare-and-confirm is the weak step, and the code appears on the wall display

**Scenario.** The attacker controls the relay and has read the 16-character code. The code appears on the Windows desktop, which is a fullscreen wall calendar visible to room cameras, video calls and screen recorders.

In slot B, the attacker keeps epk_N but swaps in its own long-term X25519 and Ed25519 keys for N's, then re-MACs the slot with HKDF(s,'mac'). If the SAS covers only X25519(epk_E, epk_N), s and a transcript that leaves out the long-term keys, both screens show the same digits. E then registers the attacker's keys and appends DeviceAdded, and every later rotation wraps epoch roots to the attacker. N fails to log in, the user pairs again, and both banners read 'added by Desk'.

Even with a correct transcript, a user who taps 'match' without comparing the digits completes the MITM.

**Why it fails.** §5.3 step 4 uses the word 'transcript' without defining it, whereas R4's version hashed xpk_N and edpk_N explicitly. Compare-and-confirm is the pairing method that user studies found prone to fatal errors.

**Required change.** - Define the transcript as SHA-256 over slots A–C verbatim, plus board_id, the relay origin, the ticket id, E's dev id, and N's long-term keys and name.
- Put the slot label and the sender's role into every MAC input.
- E registers exactly the keys named in the transcript.
- Make N's step copy-and-enter: the user types E's 6 digits into N, N does the comparison, and the user then confirms on E.
- Show the code and the SAS in a separate window excluded from screen capture, never on the wall layout.
- Banners show key fingerprints, not only device names.

**Evidence.** Design §5.3. R4 §5.4 (SAS over r_E, r_N, epk_E, xpk_N and edpk_N). Kainda, Flechais and Roscoe, SOUPS 2009 (compare-and-confirm is subject to fatal failures; copy-and-enter is secure): https://cups.cs.cmu.edu/soups/2009/proceedings/a11-kainda.pdf

## V1-7 · medium · M1 hub: replies are not bound to requests, the replay window is lost on restart, and nothing rekeys on revocation

**Scenario.** A malicious relay keeps the hub's sealed replies and plays them back out of context:
- When the Mac sends a new Add, the relay answers with an older sealed success reply from a different Add. sync.rs remaps the Mac's temporary id onto that unrelated item (sync.rs:681-682) and pops the queued Add, so the hub never sees it. Later edits of the new item land on the other item.
- An older sealed refusal makes sync.rs drop the queued edit and report it (sync.rs:706-716).
- An old reply sets acked_version = reply.version, which is deliberately not a max (sync.rs:676), so the listener's gate regresses.

After a hub restart, the in-memory five-minute replay window is empty, and replays of recent requests are applied again: an old SetNotes reverts the text, an old Restore brings back an item.

M1 has no in-band rotation, because §5.4 needs the M2 log. If hub traffic is sealed under a shared key, a revoked phone's key together with the relay still opens later traffic.

**Why it fails.** §10 specifies only 'sealed and signed … a random nonce and a five-minute replay window'. Today's reply cache lives in memory, keyed by the request key (phone.rs:287-406), and replies carry nothing that binds them to their request.

**Required change.** - Every hub reply carries the SHA-256 of its request frame and the board version, signed by the hub.
- The client rejects a reply that does not match its outstanding request, and any version lower than the last one it has seen.
- Persist the replay cache, or bind each request to a hub boot id plus a per-device counter that only increases.
- Seal hub traffic under dedicated subkeys ('hub/req', 'hub/resp') and rekey on every M1 revocation.

**Evidence.** VERIFIED: sync.rs:676, 681-682 and 706-716; phone.rs:287-406 (Replies). Design §10 'How the hub works'. §10 also claims M1 is as safe as the end state 'apart from residual risk 5'.

## V1-8 · medium · Code running as the same user on the desktops defeats the key stores: dev tooling, an admin daily account, and signing identities on the daily machines

**Scenario.** The user opens the repository in an editor on the Windows desktop, in the same account that runs TaskDeck. rust-analyzer immediately runs every dependency's build.rs and proc-macros. A crate like arrayref 0.3.10 then calls CryptUnprotectData on TaskDeck's wrapped L_d, decrypts the secrets file, and exfiltrates the board, the epoch roots, and the Desk's signing key and token. From there it can carry out the recovery-key and kept-key attacks above. WSL2 is no barrier, because interop launches Windows processes as the same user.

If the daily account is an administrator, the same malware also reaches the 'dedicated build account', the adb binary and the mounted encrypted USB that holds the APK key (§8). The next 'adb install -r' then puts a trojaned but correctly signed APK on the phone.

On macOS, the 'TaskDeck Local' identity sits in the same login keychain. Once codesign has been granted 'Always Allow', any process can sign a helper that satisfies the Keychain item's ACL and read L_d without a prompt.

**Why it fails.** §8 isolates only release builds ('Between those, build desktops locally in a dedicated non-admin build account'). §5.6 relies on DPAPI and the Keychain ACL. The APK key file and the self-signed identity live on the daily machines.

**Required change.** - Do all cargo, rust-analyzer, Gradle, Android Studio and npm work in a separate standard account or a real VM (not WSL), never in the account that holds TaskDeck keys.
- Make the daily account a standard user, with a separate admin account.
- Keep the APK key on a PKCS#11 or PIV token that needs a touch to sign, not as a file.
- Keep the macOS identity in its own keychain, locked outside releases, and never grant codesign 'Always Allow'.

**Evidence.** ASSUMED (well-known defaults): rust-analyzer runs build scripts and proc-macros (cargo.buildScripts.enable, procMacro.enable). VERIFIED in R4: arrayref 0.3.10 ran malware from build.rs and persisted itself; DPAPI works at user scope. Design §5.6 and §8.

## V1-9 · medium · The WebView's 'no way out' does not hold: WebRTC, link hints and fresh iframe realms escape request interception

**Scenario.** Any XSS in phone.html can read the whole board through the bridge's read routes (tray All, Done, notes) and send it out. Such an XSS could come, for example, from a later change that routes a feed summary into el()'s html sink (phone.html:1073). It proceeds as follows:
1. It creates an iframe. The iframe's initial about:blank document is same-origin and is not a navigation, so neither default-src/frame-src 'none' nor the document-start script applies to it.
2. It takes iframe.contentWindow.RTCPeerConnection, which the deletion in the parent page never touched.
3. It leaks data to an attacker's STUN/TURN host through ICE credentials or hostnames.

WebRTC does not pass through shouldInterceptRequest, Chromium's CSP does not govern it, and a WebView 96 test showed it bypassing even setBlockNetworkLoads(true). A link with rel=preconnect pointing at <data>.attacker.example may leak the same way over DNS and TLS SNI (ASSUMED). The location shim adds the phone's coarse position to what can leak.

**Why it fails.** §7 relies on request interception, on CSP, and on deleting RTCPeerConnection and RTCDataChannel in the main realm, while the app process holds the INTERNET permission.

**Required change.** - At document start, make HTMLIFrameElement, HTMLFrameElement, HTMLObjectElement and HTMLEmbedElement inert: non-configurable contentWindow and contentDocument getters that return null, plus a MutationObserver that removes such nodes synchronously.
- Delete and freeze every RTC* constructor, webkit-prefixed ones included, and allow no link rel values beyond the ones the page itself uses.
- Call setBlockNetworkLoads(true).
- Make the location route ask natively on every request.
- Add a Phase 3 harness that tries each channel against a local listener: WebRTC through an iframe realm, preconnect, dns-prefetch, prefetch, sendBeacon, ping, WebSocket, WebTransport, navigation and window.open. Keep the 'no way out' claim only while the harness passes.
- The robust option: ship the WebView and the core in an APK without the INTERNET permission, plus a second app signed with the same key that moves only ciphertext.

**Evidence.** VERIFIED: phone.html:1068-1075 (el() html sink); phone.rs:1212-1217 (current CSP). WebRTC sent traffic despite setBlockNetworkLoads in WebView 96: https://github.com/rmst/android-webview-exfiltration. WebRTC bypasses connect-src: https://github.com/w3c/webappsec-csp/issues/92. Fresh iframe realms bypass patched globals: https://github.com/LavaMoat/snow. androidx addDocumentStartJavaScript runs only in frames that 'begin loading'. ASSUMED: the about:blank and preconnect behaviour on current WebView; test it in Phase 3.

## V1-10 · medium · A compromised member can replace state outside the command log: snapshots, ResetTo and 'adopt state' are never checked against honest replay

**Scenario.** A compromised Mac posts three snap entries with fabricated BoardData. The relay keeps only the newest three (§4.7), so the honest snapshots are evicted. When the Desk later pairs a phone and names 'the snapshot', the phone loads the forgery and checks only the chain from S to the head (§5.3 step 6). A device away for more than 365 days does the same.

The Mac has two other routes:
- It can append ResetTo{seq, hash, snapshot} pointing at a forged snapshot.
- It can publish a false state hash in its heartbeat. Sync then halts on every device and offers 'adopt Desk's state?' (§6.4), and the attacker's device can simply be named 'Desk'.

**Why it fails.** No rule makes replicas compare a snap or ResetTo hash with their own state at S. The pairing bundle carries 'the snapshot id and hash' rather than a fresh snapshot of E's own state. Adoption is chosen by device name.

**Required change.** - Align S to the heartbeat stride. Every replica checks each snap and ResetTo hash against its own recorded state hash at S; a mismatch raises an alarm naming the author and never leads to adoption.
- E uploads a fresh snapshot of its own state for each pairing.
- ResetTo is applied by local replay, and only to seqs that lie inside every device's retained log.
- On a hash disagreement, halt only the minority device. Offer adoption only from a device the user picks by the fingerprint shown on that device's own screen.
- The relay keeps the newest snapshot of each author.

**Evidence.** Design §4.7 ('The relay keeps the newest three'; 'newest member-signed snapshot'), §5.3 steps 5-6, §6.4 ('offers adopt Desk's state?'), §6.6 (ResetTo).

## V1-11 · medium · One signed entry from a device that is later revoked wedges every replica, and its revocation can then never be applied

**Scenario.** Before it is caught, a compromised member appends either a validly signed frame with an unknown ver, or a command that panics inside apply on every build. Every device halts at that seq (§4.5, §4.8).

The revocation and rotation the user appends next are sequenced after the halt, so no halted device ever applies them. The attacker stays a member in every replica's view. In-band eviction is impossible until a code change ships, or until the user resets trust on a new board.

**Why it fails.** A halt is all-or-nothing. There is no deterministic, member-authorised way to step over a single entry.

**Required change.** Add a ctl entry Skip{seq, frame_hash}, signed by a member other than the halting entry's author, or by the recovery signer. Halted devices read ahead only for Skip and membership entries. Skipping stays deterministic, because no device applied the halting entry. The halt screen offers it and names the entry's author.

**Evidence.** Design §4.5 (on a panic it 'halts sync with entry N crashes this build — update'), §4.8, §6.4 ('sync halts at that seq until the build is updated').

## V1-12 · medium · Gaps in the crypto spec: no key commitment on rotation, HPKE info not bound, and no AD specified for snapshot parts, local files or hub frames

**Scenario.** A compromised member performs EpochRotated but wraps a different R_{e+1} to each device. Every wrap is opaque, so nothing detects this. XChaCha20-Poly1305 is not key-committing, so the attacker can then craft entries that decrypt to different valid plaintexts on different devices, or that fail AEAD on some devices only. Replicas diverge under a valid, unforked chain, which breaks the assumption that a bad frame is 'skipped the same way on every device'. The copy wrapped to the recovery key for backups can differ again.

Lesser gaps:
- Snapshot parts sealed without an AD binding the blob id and part index can be reordered or swapped by the relay, working with a revoked device's old K_snap.
- The recovery X25519 key opens both rotation wraps and backups, and the single L_d seals several local file types. No HPKE info or AD keeps these uses apart.
- The key for hub frames is not specified.
- TOKEN_KEY MACs both device tokens and read-only tokens without a role byte.
- The HPKE suite id is not in the AD, and there is no raise-only floor for the crypto version.

**Why it fails.** §5.2 and §5.4 specify the wraps, but neither a commitment to the new root nor the HPKE info and aad. §4.7 gives no AD for snapshot parts. §5.6 seals everything under one L_d. The only AD the design specifies is the one for entries in §4.2.

**Required change.** - Each rotation entry carries commit = HKDF(R_{e+1}, 'taskdeck/v1/commit'). Every device checks its unwrapped root against it and halts with an alarm on a mismatch.
- HPKE info = 'taskdeck/v1/wrap' or 'taskdeck/v1/backup', followed by board_id, the epoch or seq, and the recipient's dev id. The suite id goes into the AD.
- Seal each snapshot as one AEAD message and only then split it, with the SHA-256 of the sealed bytes in the signed snap entry.
- Local files get AD = file kind + dev + board_id. Hub frames get their own subkey.
- The token MAC input is role + board + dev + gen.
- Add a signed, raise-only MinCryptoVer control, separate from the schema number.

**Evidence.** Design §4.2 (AD = frame header + ver + epoch, the only AD specified), §4.7, §5.1, §5.2, §5.4, §5.6. Non-committing AEADs allow one ciphertext to be valid under several keys (Len et al., USENIX Security 2021, cited in R4).

## V1-13 · low · Backups carry no proof of who wrote them, and restore takes 'the newest' file

**Scenario.** While compromised, a device writes TaskDeck-2099-12-31-999999999.tdbk into the OneDrive folder. It knows the recovery public key, and under HPKE base mode anyone holding that key can produce a valid backup.

Months after cleanup, the user loses every device and runs taskdeck-recover, which 'decrypts the newest backup'. That is the forgery, with attacker-chosen BoardData, device set and relay token, and possibly a relay host of its own. 'Create board from this folder' then adopts it.

**Why it fails.** §6.6 seals backups with HPKE to the recovery key, which gives no sender authentication. §5.5 picks the newest file, and a backup carries its own relay coordinates.

**Required change.** - Sign each backup with the writer's device key and include the full signed membership history back to a self-certifying genesis, with board_id = H('taskdeck/board/v1' + genesis key + salt).
- taskdeck-recover verifies that chain against the board_id printed on the sheet, picks the newest verified backup, and shows its writer and fingerprint.
- The tool takes the relay host and board id from the sheet, never from the file.

**Evidence.** Design §5.5 (the sheet holds the relay host and board id; restore 'decrypts the newest backup') and §6.6. RFC 9180 base mode provides no sender authentication.

## V1-14 · low · Bytes authenticated only by a shared epoch key reach the decompressor and every image decoder

**Scenario.** A revoked device that still holds an old K_snap or K_blob, working with the relay, replaces a snapshot part or the desk-picture blob. The phone then either:
- inflates the snapshot before it can compare the state hash, which makes a decompression bomb possible; or
- decodes the picture with prepare_backdrop_bytes, which guesses among every format the image crate supports (TIFF, EXR, DDS, HDR and others) and enforces only width, height and allocation limits.

**Why it fails.** §4.7 checks the state hash only after decompression, and blob ids are not tied to their content in a signed entry.

**Required change.** - Check the SHA-256 of the sealed bytes against the signed snap or blob entry before decrypting.
- Cap the inflated size.
- Decode synced pictures with with_format(ImageFormat::Jpeg) only; the sending device re-encodes to JPEG anyway.

**Evidence.** VERIFIED: phone.rs:1529-1536 (with_guessed_format, Limits); Cargo.toml uses image 0.25 with its default formats. Design §4.7, and §3 (the desk picture travels as one sealed blob).

## V1-15 · low · ICS feeds: today the secret URL leaks in the Referer header on redirects, feed text becomes signed commands, and the SSRF list is incomplete

**Scenario.** 1. Today reqwest follows up to 3 redirects and sends Referer set to the previous URL, path and query included. A calendar host that redirects off-site therefore hands the secret feed URL to the redirect target.
2. An honest device turns an attacker feed's X-WR-CALNAME into a signed RenameSubscription (ui.rs:2753-2765). Publisher text then enters the replicated log under the user's signature, and different devices can race to set different names.
3. The SSRF guard lists private, loopback, link-local, CGNAT and ULA ranges. It does not list IPv4-mapped IPv6 (::ffff:10.0.0.1), NAT64 (64:ff9b::/96), 6to4 (2002::/16), 0.0.0.0/8, multicast or broadcast.

**Why it fails.** reqwest's referer setting defaults to true (client.rs:312) and make_referer keeps the query (redirect.rs:293-303). better_name runs on every refresh (subscriptions.rs:312-337). §7 enumerates the refused ranges without these.

**Required change.** - In Phase 0, set .referer(false). The hand-written redirect loop for ureq sends no Referer and never carries a query string across origins.
- Auto-rename only on the device that added the subscription, once, and label the name as untrusted.
- Normalise mapped and translated addresses before the range checks, and refuse the extra ranges.

**Evidence.** VERIFIED: reqwest-0.12.28 src/redirect.rs:293-303 and src/async_impl/client.rs:312; subscriptions.rs:736-743 (Policy::limited(3), https_only) and 312-337; ui.rs:2753-2765.

## V1-16 · low · Fields supplied by the relay steer client state without being checked

**Scenario.** - The relay inflates welcome.last_ctr. The device renumbers its pending entries above it (§4.3), and other devices then see a counter gap that looks as if this device's entries were withheld.
- Fabricated 409 conflict replies make the device re-seal over and over and show 'restored from a copy'.
- An entry whose seen_seq is at or above its own seq is causally impossible, but nothing explicitly rejects it.
- If the relay's WebCrypto Ed25519 is laxer than dalek's verify_strict, the relay counts a frame that clients skip, and the author's next frame then looks like a gap.

**Why it fails.** §4.3 trusts last_ctr and the 409 payloads. §5 does not state that seen_seq must be below the entry's seq. The effect of a skipped frame on the author's counter is not specified.

**Required change.** - Accept a last_ctr above the local counter only if the relay produces this device's own signed frame at that counter.
- Rate-limit 409 replies and keep a record of them.
- Reject seen_seq >= s, or seen_chain different from C_{seen_seq}, as tampering.
- Define that a skipped frame consumes no counter value.
- Put Wycheproof edge-case vectors into the relay's conformance suite, so the relay is at least as strict as the clients.

**Evidence.** Design §4.2, §4.3, §5.1 ('the relay's WebCrypto check is only a filter').

## V1-17 · low · Setup checks the recovery sheet by having only its check characters retyped

**Scenario.** A faint or misprinted group on both sheets goes unnoticed until the first quarterly drill, up to three months later. If every device is lost in that window, nothing can be recovered.

**Why it fails.** §5.5: 'Setup finishes only after the check characters are retyped.'

**Required change.** At setup, retype all 52+4 characters from one printed copy rather than from the screen, and decrypt a test backup with them before setup finishes.

**Evidence.** Design §5.5 and §6.6.

## V1-18 · low · The legacy read/write token survives M1, and a rollback brings it back

**Scenario.** The 160-bit token has been in links, QR codes and browser history, and, through /calendar.ics, at Google Calendar (DOCUMENTATION.md:2893). M1 turns the phone server off but never changes the key. Any of these brings back plaintext HTTP with that token, together with the tiny_http stalled-body freeze:
- a rollback ('the old build opens pre-sync-<date>/');
- turning the setting back on;
- an old build configured to bind 0.0.0.0.

**Why it fails.** §9 steps 5 and 8 remove the Google Calendar subscription, but nothing rotates the token.

**Required change.** In M1 step 5, press 'New key' (ui.rs:3597) before switching the phone server off. On conversion, delete the token and the phone_server settings from the legacy copy.

**Evidence.** VERIFIED: DOCUMENTATION.md:2893; ui.rs:3597; initialization.rs:317-322 and 426-427. Design §9.

## V1-19 · low · Notifications: Google Play services displays FCM notification messages without the app's code, and lock-screen actions change state

**Scenario.** If FCM is ever added, whoever holds the service-account credential can send notification-type messages; that means the Windows desktop, or malware on it. On Android 10 and later, Google Play services displays these itself by default ('delegation'), with attacker text such as 'Re-pair TaskDeck: type this code', and fetches any image URL in them, which reveals the phone's IP address.

Separately, the Done and Tomorrow actions on reminder notifications can be tapped from the lock screen by anyone holding the phone.

**Why it fails.** §7 requires 'proof that notification-type messages cannot be displayed' but names no mechanism for it. The Phase 6 notification actions do not require the phone to be unlocked.

**Required change.** - Phase 9: opt out of notification delegation in the manifest, and receive messages only through the app's own service, which drops notification keys.
- Prefer a sender that can only send data messages.
- Mark every state-changing notification action with setAuthenticationRequired(true).

**Evidence.** Firebase docs: display by Play services on behalf of the app is the default on Android Q and later (https://firebase.google.com/docs/cloud-messaging/android/message-priority). Design §7 push row; §10 Phases 6 and 9.

## Confirmed strengths

- The architecture removes today's largest exposures. Under the plan of record the relay serves no code, all clients are native, and the WebView loads only bundled assets. Retired with it: the plaintext tiny_http server, the single read/write token carried in URLs, QR codes and the feed, and a sync client that accepts http:// (VERIFIED: sync.rs:132-136; phone.rs:887-928). In M1 the hub can be reached only by sealed, signed requests from paired devices.
- Authentication comes before parsing: size cap, then membership, Ed25519, ver, AEAD, strict serde and apply. Frames from unknown authors are skipped the same way everywhere. The entry AD binds board_id, dev, ctr, len, ver and epoch. Per-kind HKDF subkeys are salted with board_id.
- Nonce handling is sound: XChaCha20 uses random 192-bit nonces, retries resend identical sealed bytes, and re-seals draw fresh nonces. Restoring a folder or a VM therefore cannot cause nonce reuse, and Ed25519 is deterministic.
- Clients enforce integrity themselves:
- contiguous seq and per-device counters that rise by exactly 1;
- a hash chain with fixed-length inputs and a persisted head;
- seen-heads inside the ciphertext (SUNDR fork consistency);
- the echo check, signed daily heartbeats with state hashes, and the 48-hour silence alarm.
Once issue 1 is fixed, a relay on its own cannot read, forge, replay, reorder one device's entries, roll back or fork without detection.
- Pairing design:
- a typed code, so there is no QR library, camera or deep-link intercept surface;
- slots MAC'd under a key derived from an 80-bit secret the relay never sees;
- commit-then-reveal before the SAS, so a relay-in-the-middle cannot grind;
- a single-use 10-minute ticket, keyboard learning off, overlay-touch filtering, a device-authentication gate, and new-device banners.
- Recovery design: the paper sheet alone grants nothing, since it holds no token and cannot enrol a device. Backups are sealed so that their writers, including a writer compromised later, cannot read them back. The recovery tool is checked against a fingerprint printed on the sheet.
- Version evolution: entries are stamped with the lowest schema that expresses them, the forced-update MinSchema is dropped, and older devices keep sending. Unknown versions halt rather than diverging silently. That matters because older builds silently drop unknown fields (VERIFIED in R1).
- Supply-chain baseline:
- a Phase 0 lockfile refresh for RUSTSEC-2026-0381/-0382 and the rustls advisories;
- --locked, cargo deny, cargo audit, cargo vet and a cooldown on new releases;
- one dependency change per PR, and Gradle dependency verification;
- an Android runtime limited to kotlin-stdlib and androidx.webkit, with no FCM, Play services, ML Kit or CameraX in the plan of record;
- an offline APK signing key and no updater anywhere.
- The WebView bridge accepts only board verbs plus the read, Look and background routes, never subscription, device, key, recovery or relay operations. That closes today's route in which /api/command forwards any non-query Command (VERIFIED: phone.rs:887-928). Today's page has a single innerHTML sink and escapes feed text in text positions only (VERIFIED: phone.html:1073, 3393, 3457-3522). Trusted Types and a script hash raise the bar further.
- ICS hardening reaches the resolved address: a pinning resolver defeats DNS rebinding, redirects are followed by hand and each hop is re-checked. Error text no longer leaks URLs; the leak exists today (VERIFIED: reqwest-0.12.28 error.rs:267-269, subscriptions.rs:545-550). The overlay stays out of apply, and a feed that panics is quarantined.
- Operations: the DO uses hibernation only, with no timers or alarms. The relay costs EUR 0 and fails closed at quota. Devices keep 400-day local logs. Re-homes are fenced and need a move code typed on each device. 'Reset trust' exists as a route. A deterministic network simulator and an adversarial relay are planned as test gates.

## Notes

No critical path found. In the plan of record, none of the relay, the ICS feeds, Open-Meteo, QR codes (there are none) or push (there is none) delivers code. A relay acting alone cannot read data or forge it undetectably, provided issue 1 is closed. The remaining exposure is the per-device authority model: a single compromised or stolen-and-unlocked device can author recovery-key, rotation, revocation, fence, snapshot and ResetTo entries, and the remedies the design offers (rotation, revocation) leave persistent footholds behind (issues 2, 3, 5, 10, 11).

Priority before Phase 1 code:
1. Domain-separate all signatures, add the AEAD-failure rule, and alarm on frames that use this device's dev id but were not written by it (issue 1).
2. Add a recovery signer as the root of membership authority. Make 'suspected compromise' mean re-pair. Show persistent banners for every control entry at M2. Tie read tokens to a device and an epoch (issues 2, 3, 5).
3. Add the missing crypto spec: key commitment on rotation, HPKE info, snapshot AD (issue 12).
4. Make the SAS transcript explicit and switch to copy-and-enter (issue 6).
5. Bind hub replies to their requests (issue 7).

Before the first release, settle the CI cache and attestation rules (issue 4). Before any further cargo work as the desktop account that holds TaskDeck keys, isolate development (issue 8).

Not verified, and to be tested in Phase 3 or on the device:
- whether WebRTC still escapes current WebView versions (shown only on WebView 96);
- whether addDocumentStartJavaScript runs in a dynamically created about:blank iframe;
- whether preconnect is ungoverned by CSP in WebView;
- rust-analyzer's defaults and the macOS codesign 'Always Allow' behaviour (both from general knowledge);
- the exact manifest opt-out for FCM delegation.

Accepted OS-vendor trust worth stating:
- The Android System WebView provider auto-updates code into TaskDeck's own process.
- An opt-in CalendarContract export puts plaintext in the calendar provider's database, which TaskDeck's backup and device-transfer exclusions do not cover and any app holding READ_CALENDAR can read.

Severity calibration: the high items either break an explicit design claim with a cheap fix (1, 3, 4) or give an attacker persistence after revocation (2).
