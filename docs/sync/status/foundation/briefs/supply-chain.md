# Task: baseline 1b, supply chain (stage foundation)

YOUR TASK: baseline task 1b of SYNC_PLAN.md section 6.1 item 1: "Add deny.toml, cargo-deny, cargo-audit and the seven-day dependency cooldown." Background: SYNC.md section 8 ("Supply chain" and "CI" paragraphs) and section 7 (the dependencies row); docs/sync/research/R4 section 4.2; docs/sync/research/V1 finding V1-4 (tools installed only at pinned versions, verified).
You are the only agent compiling in the default target directory right now; do not set CARGO_TARGET_DIR.
Paths you may create or change: deny.toml, .cargo/audit.toml, scripts/dev-setup.sh, scripts/supply-chain.sh, scripts/cooldown.sh (and helper files next to it under scripts/, including scripts/cooldown-exemptions.txt), scripts/tests/** . Do not change Cargo.toml or Cargo.lock: if a check fails because of what the lockfile contains, report the finding and your recommended resolution; the lead decides.

1. scripts/dev-setup.sh <stage>. A later task extends this script for every stage (Android SDK and NDK, cargo-ndk, wrangler). Create it now with a clean structure for that (one function per tool, a table of which stage needs which tool, "foundation" needing cargo-deny and cargo-audit) and implement those two tools: install cargo-deny and cargo-audit as official prebuilt release binaries for x86_64 Linux from their official GitHub releases (EmbarkStudios/cargo-deny and rustsec/rustsec), at versions pinned in the script, verifying a SHA-256 that is written in the script (not fetched from the same place as the download). Install into a directory the script prints and that is on PATH for the user (for example ~/.cargo/bin); be idempotent (skip when the right version is already there); fail loudly on a checksum mismatch; support "--check" to only report what is missing. Choose versions under the same seven-day rule the project applies to dependencies: published at least 7 days before 2026-11-21 (cargo-deny 0.22.0 and 0.21.0 are too new; find the newest release that is old enough AND can read today's advisory database and this project's Cargo.lock; cargo-audit 0.23.0 from 2026-11-10 is old enough if it works). If a tool has no usable prebuilt binary, fall back to "cargo install --locked --version X" and say so. On macOS and Windows the script may say plainly that it supports Linux only for now.
2. deny.toml for cargo-deny: sources limited to crates.io (unknown registries and all git sources denied); advisories on, with exactly one reviewed ignore, RUSTSEC-2026-0543 (chrono is unmaintained; the design keeps chrono for now and plans jiff later: SYNC.md sections 8 and 11), written with its reason; yanked crates denied; a ban on duplicate versions of the cryptographic crates the design uses, so that later stages cannot pull in a second RustCrypto generation (SYNC.md section 5.1 "One RustCrypto generation"): at least chacha20poly1305, chacha20, poly1305, aead, cipher, ed25519-dalek, ed25519, curve25519-dalek, x25519-dalek, hpke, hkdf, hmac, sha2, digest, crypto-common, zeroize, subtle, rustls, rustls-webpki, ring. Most of these are not in the lockfile yet; the ban must still be expressed so that it bites when they arrive (use cargo-deny's per-crate deny-multiple-versions form, and check the exact syntax against the cargo-deny version you pinned by reading its documentation or source, not from memory). Do not turn on the licence check; the plan does not ask for it. The script must run the checks it configures: advisories, bans, sources.
3. cargo-audit: configure it (.cargo/audit.toml) with the same single ignore and its reason, and make warnings (unmaintained, unsound, yanked) fail.
4. The seven-day cooldown, as a script of our own (the lead decided against a new tool): scripts/cooldown.sh. The crates.io sparse index has a "pubtime" field for every version (try: curl -s https://index.crates.io/se/rd/serde | tail -1). Behaviour: compare the working tree's Cargo.lock with the Cargo.lock at a base revision (option --base <rev>; default: the merge base of HEAD with origin/day-planner). For every crates.io package (name, version) present now and absent in the base, look up its publish time and fail if it is younger than 7 days (604800 seconds) at the time of the check (option --now <RFC 3339> for tests), or if that version is yanked, or if the version cannot be found. Option --all checks every crates.io package in the lockfile instead of only the new ones. A file scripts/cooldown-exemptions.txt may list exemptions, one per line: name, version, an expiry date, and a reason; an exemption without all four, or past its expiry, is an error; the file starts empty apart from a comment that explains the format and that every line needs the lead's or the owner's written reason. Network access must go through curl (it honours the proxy environment here; node's fetch does not); parsing may use python3 or node (both are present here and on GitHub's Ubuntu runners), no third-party packages. An option --index-dir <dir> reads index files from a local directory laid out like the sparse index, so the script can be tested offline. Print one line per violation and a summary; exit 0 only when clean.
5. scripts/supply-chain.sh: runs, in order, cargo deny check (advisories, bans, sources), cargo audit, and the cooldown against its default base, each clearly labelled; exits non-zero if any failed, after running all three; says what to install (scripts/dev-setup.sh foundation) when a tool is missing.
6. Tests under scripts/tests/ (plain bash, runnable as "bash scripts/tests/run.sh", no test framework to install): for the cooldown script, using --index-dir and --now with small made-up lockfiles and index files: a new crate younger than 7 days fails; exactly 7 days old passes; an unchanged old crate is not looked up; a yanked version fails; a missing version fails; a valid exemption passes; an expired or malformed exemption fails; a git or path dependency in the lockfile is ignored by the cooldown (cargo-deny's sources check owns that). For dev-setup.sh: "--check" reports correctly, and a wrong checksum is refused (test this without downloading twice if you can).
Run everything for real in this container: install the tools with your script, run scripts/supply-chain.sh, and report the full output. Expect findings: R4 section 4.2 lists advisories on crates other than the four that were refreshed (crossbeam-epoch, quick-xml, quinn-proto, quinn-udp, and chrono's unmaintained notice). For each remaining advisory or warning give: the id, the crate and version in the lockfile, what it is, the lowest fixed version at least 7 days old, whether the crate is actually compiled into any of today's binaries on any platform ("cargo tree -i <crate> -e features" with and without default features, and --target all), and your recommendation (a targeted "cargo update -p" the lead could approve, or a reviewed ignore with its reason). Do not apply either. Also run the cooldown with --all and report anything younger than 7 days in today's lockfile.

## State after the implementation (2026-11-21, second session)

Implemented and committed: `deny.toml`, `.cargo/audit.toml`, `scripts/dev-setup.sh`, `scripts/supply-chain.sh`,
`scripts/cooldown.sh`, `scripts/cooldown.py`, `scripts/cooldown-exemptions.txt`, `scripts/tests/` (`run.sh`,
`lib.sh`, `test-cooldown.sh`, `test-dev-setup.sh`). cargo-deny is pinned at 0.20.2 and cargo-audit at 0.23.0.
`bash scripts/tests/run.sh` passes. `bash scripts/supply-chain.sh` failed on the lockfile as it stood: the
rulings below settle every finding.

# Lead rulings for task baseline-1b (2026-11-21)

SYNC_PLAN.md §6.1 names four crates for the lockfile refresh and forbids a blanket update, because the
desktop's look depends on the pinned egui, wgpu and winit. It also requires cargo-deny and cargo-audit, which
cannot pass on today's lockfile. The lead's decision: a crate with an advisory and a fixed release moves by a
targeted `cargo update -p <crate> --precise <version>` to the lowest fixed release that is at least seven days
old, one crate at a time; a crate with no fixed release gets a reviewed ignore. egui, wgpu, winit and every
other entry stay where they are.

1. **Targeted updates** (this ruling overrides "do not change Cargo.lock" for exactly these moves):
   - `crossbeam-epoch` 0.9.18 to 0.9.20 (RUSTSEC-2026-0204; compiled into both binaries);
   - `rustix` 1.1.4 to 1.1.6 (RUSTSEC-2026-0486; `cargo update -p rustix@1.1.4 --precise 1.1.6`; any other
     rustix version in the lockfile stays);
   - `webbrowser` 1.2.1 to 1.2.2 (RUSTSEC-2026-0257; desktop binary). If the resolver also drops a duplicate
     `core-foundation` that only the old webbrowser used, that removal is accepted: report it;
   - `clipboard-win` 5.4.1 to 5.4.2 (the locked version is yanked; Windows desktop);
   - `wayland-scanner` 0.31.10 to 0.31.11, which brings `quick-xml` 0.39.4 to 0.41.0 (RUSTSEC-2026-0194 and
     -0195; a build-time code generator on the Linux desktop only). If the Linux desktop job fails in CI
     because of it, the lead falls back to a reviewed ignore;
   - `quinn-proto` 0.11.16 to 0.11.19, `quinn-udp` 0.5.15 to 0.5.16, `chacha20` 0.10.1 to 0.10.2 (seven
     advisories, one more, and a yanked version; lockfile entries that no target compiles).
   After each update, read the lockfile diff: exactly the named entries may differ. Put back anything else the
   resolver moved (the first refresh had to restore `windows-sys` edges, decision D7), and list the final diff
   entry by entry in the report.
2. **Reviewed ignores**, in `deny.toml` (with the reason) and in `.cargo/audit.toml` (the id, the reason in a
   comment), beside the chrono one. None has a fixed release:
   - RUSTSEC-2024-0436, `paste`: a build-time macro under `rav1e`, which the `image` crate's `avif` feature
     pulls in; no runtime code. Stage `desktop` removes the AVIF encoder with the phone server (SYNC.md §2.1);
   - RUSTSEC-2024-0370, `proc-macro-error`, and RUSTSEC-2022-0104, `structopt`: the command-line helper that
     `kmeans_colors` enables by default; the app never calls it. Stage `desktop` turns that default feature off
     when it reworks the root package's dependencies. Not done here: SYNC_PLAN.md §2 keeps of the old hygiene
     phase only the lockfile refresh, and the root package's dependency list is today's app;
   - RUSTSEC-2026-0192, `ttf-parser`: Wayland title-bar fonts on the Linux desktop only, read from local font
     files, under the pinned winit.
3. **Duplicate versions outside the cryptographic list are allowed, not warned.** The pinned egui, wgpu and
   winit tree carries 41; their warnings are 2,650 of the gate's 3,500 output lines and bury the errors. Set
   `multiple-versions = "allow"`; the per-crate `deny-multiple-versions` entries stay and still fail.
4. **cargo-audit fails on unmaintained, unsound and yanked, as the brief says, not on `notice`.** The two tools
   then agree; a notice is printed and does not fail the run.
5. **The shell scripts this task added are executable** (mode 755), like the two older check scripts.
6. **Done means:** `bash scripts/tests/run.sh` passes; `bash scripts/supply-chain.sh` exits 0;
   `bash scripts/cooldown.sh --all` reports no violation; the server builds and the library tests report
   265 passed, 1 ignored (`cargo build --locked --no-default-features --bin taskdeck-server`,
   `cargo test --locked --no-default-features --lib`); and `cargo check --locked --lib` with default features
   passes on this Linux machine, or the report says exactly why it cannot run here.
7. **`scripts/supply-chain.sh` accepts `--base <rev>` and passes it to `scripts/cooldown.sh`** (no other
   argument; an unknown one is an error). With it, `scripts/ci/supply-chain.sh` makes one call to
   `scripts/supply-chain.sh --base <rev>` when the event gives a usable base commit, and one call without
   it otherwise, instead of running the cooldown a second time; its test follows. This ruling lets the
   agent that applies it change `scripts/ci/supply-chain.sh` and `scripts/tests/test-ci-supply-chain.sh`
   for exactly that.

## State after the rulings (2026-11-22)

Rulings 1 to 5 and 7 are applied and committed; ruling 6's checks were run by the lead in the main checkout and
pass. `scripts/tests/test-supply-chain.sh` (the `--base` option) joined the tests. `scripts/tests/lib.sh` and
`scripts/cooldown.py` stay mode 644: one is only sourced, the other is run through `python3 -I`.
