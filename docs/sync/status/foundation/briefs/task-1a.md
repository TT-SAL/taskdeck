# Task: baseline 1a (stage foundation)

Baseline task 1a of SYNC_PLAN.md section 6.1 item 1 (first four bullets), already implemented and committed as HEAD (see "git show --stat HEAD"):
- rust-toolchain.toml pins Rust 1.97.0, profile minimal, clippy, targets wasm32-unknown-unknown, aarch64-linux-android, armv7-linux-androideabi, x86_64-linux-android, aarch64-apple-darwin.
- Cargo.lock: only rustls, rustls-webpki, webpki-roots and tokio may move ("cargo update -p", no blanket update), each to a version published at least 7 days before 2026-11-21 (crates.io sparse index "pubtime"), past RUSTSEC-2026-0381, -0382 and -0386.
- Cargo.toml: the root manifest is a workspace root (members: crates/panic-probe; default-members: the root package), and [profile.release] has panic = "unwind". crates/panic-probe plus scripts/check-release-panic.sh are the plan's "test that builds in release and checks a panic is caught".
- build.rs takes BUILD_DATE from SOURCE_DATE_EPOCH when set, fails the build on an unusable value, and falls back to the current UTC date; scripts/check-build-date.sh checks it.
Owned paths for this task: rust-toolchain.toml, Cargo.toml, Cargo.lock, build.rs, crates/panic-probe/**, scripts/check-release-panic.sh, scripts/check-build-date.sh.
The task's gate, run from the repository root: scripts/check-release-panic.sh && scripts/check-build-date.sh && cargo build --locked --no-default-features --bin taskdeck-server && cargo test --locked --no-default-features --lib   (the test run takes about 4 minutes and must report 264 passed, 1 ignored).
CI (.github/workflows/ci.yml) is restructured by a later task; today it runs "rustup update stable; rustup default stable" and then cargo, on Linux, Windows and macOS.

Lead rulings that stand (do not undo, do not re-report): tokio is pinned at 1.53.3 (the lowest patched release that is not a downgrade); build.rs emits a short explicit rerun list (build.rs, resources.rc, icon.ico, Cargo.toml, Cargo.lock, src, and SOURCE_DATE_EPOCH); Rust 1.97.0 stays the pinned toolchain; the root package stays the only default member; the workspace excludes target/ (a nested package elsewhere must be a member, be excluded, or carry its own [workspace] table); the two check scripts support Linux, macOS and Git-Bash and need not defend against every hostile environment variable: a finding about a script counts only if a developer or CI following the documented usage would be misled (a check passing when it should fail, or failing with a wrong or missing reason).


## State after five attack rounds (2026-11-21)

The task went through five attack rounds; every finding below was reproduced and fixed, or ruled on by the lead. The loop did not reach two dry rounds, so it is being run again from the start.

# Lead rulings for task baseline-1a (added 2026-11-21, after attack round 5)

1. **Today's binaries keep "any panic ends the process" in release builds.** Round 5 showed that with
   `panic = "unwind"` a panic on a background thread of today's app (phone workers, the pulse thread, the
   calendar and weather threads, the sync sender and listener) no longer ends the process, so
   `deploy/taskdeck-server.service` (`Restart=always`) no longer brings a broken server back. SYNC_PLAN.md
   §3.2: "Every merge keeps today's app working." The lead's decision: the profile stays `unwind` (the plan
   requires it), and today's two binaries install a panic hook that aborts.
   This ruling OVERRIDES the ground rule "do not edit anything under src/" for exactly these files and
   nothing else: `src/main.rs`, `src/server_main.rs`, and ONE small new or existing module under `src/`
   that holds the hook function and its test (prefer a new `src/panic_abort.rs` declared in `src/lib.rs`
   with one `pub mod` line). Constraints:
   - A function `abort_on_panic()` installs a hook that first calls the previously installed hook (so the
     panic message is still printed exactly as before) and then calls `std::process::abort()`.
   - Each `main()` calls it as its first statement, and only in builds without `debug_assertions`
     (`#[cfg(not(debug_assertions))]`), because debug builds unwound before this stage too and must behave
     as before.
   - No other change to `src/`. No new dependency.
   - A test proves it: the test re-runs its own test executable as a child process with an environment
     variable that makes the child install the hook, panic on a spawned thread and then sleep; the parent
     asserts the child ended by abort (on Unix: killed by SIGABRT; on Windows: a non-zero exit that is not
     the normal panic exit) within a few seconds, and a second case without the hook shows the child
     outliving the thread's panic. The test must be robust on Linux, macOS and Windows (CI runs all three)
     and must not print a confusing backtrace into the test output of the parent.
   - Update the comment on `panic = "unwind"` in Cargo.toml to say that today's binaries restore the old
     behaviour with the hook, and that stage `desktop` removes or narrows the hook when the new runtime's
     `catch_unwind` guards need unwinding inside the app.
   - The task's gate now expects one more passing library test than before (265 passed, 1 ignored).
2. **Scope of findings against the two check scripts.** `scripts/check-release-panic.sh` and
   `scripts/check-build-date.sh` are checks for CI (a fresh checkout, the default environment, Linux, macOS
   and Windows runners) and for a developer who runs them as documented. From now on a finding against
   them counts only if one of those two users would be misled: the check passes when the property is
   broken, or fails or misreports when it holds. Exotic local setups (a target directory shared between
   checkouts, hostile environment variables) are out of scope; do not add more machinery for them.


## Already raised and handled (do not re-report)

- [low] check-build-date.sh fails, and prints no reason, when the checkout directory is named task_deck
- [low] The new [workspace] root now claims every non-member Cargo package below the repository, including ones under target/
- [low] check-release-panic.sh passes when Cargo.toml says panic = "abort" if the environment overrides the profile (CARGO_PROFILE_RELEASE_PANIC or RUSTFLAGS)
- [low] check-release-panic.sh fails with a misleading 'panic was not caught' message when CARGO_BUILD_TARGET names a target the host cannot run
- [low] check-release-panic.sh resolves a relative CARGO_TARGET_DIR against the repository root, not the caller's directory
- [low] check-release-panic.sh mangles an executable path that contains a backslash on Linux/macOS
- [medium] check-build-date.sh does not notice build.rs writing the build date in local time instead of UTC when the checker runs in UTC (the usual CI case)
- [low] check-build-date.sh does not neutralise CARGO_BUILD_TARGET, so a cross target in the environment makes every check fail with an unrelated compile error
- [low] build.rs accepts SOURCE_DATE_EPOCH values past year 9999 and writes a BUILD_DATE that is not in YYYY-MM-DD form
- [medium] check-build-date.sh's new TZ=XXX-14 misses a build.rs that uses local time only when SOURCE_DATE_EPOCH is unset, whenever the check runs between 00:00 and 09:59 UTC
- [medium] check-build-date.sh does not notice a build.rs whose fallback (SOURCE_DATE_EPOCH unset) uses the local date, if the gate runs between 00:00 and 10:00 UTC
- [medium] check-build-date.sh (and the BUILD_DATE in any build) uses whichever checkout's build.rs was compiled last when two checkouts share a target dir, so a broken build.rs passes the check
- [low] check-build-date.sh passes a build.rs that refuses every SOURCE_DATE_EPOCH after 2038-01-19 (seconds parsed as i32)
- [medium] Release builds now unwind on panic, so a panic in a background thread of today's app (phone workers, sync sender or listener, weather) no longer ends the process: the thread dies silently, and systemd's Restart=always no longer brings taskdeck-server back
- [low] check-build-date.sh never tries a non-UTF-8 SOURCE_DATE_EPOCH, so a build.rs that quietly treats one as unset (and stamps today's date) passes

After the panic hook lands the gate's test count is 265 passed, 1 ignored (it was 264).

## Raised in the second loop (rounds 1 to 5, 2026-11-21), all handled; do not re-report

- [medium] No check notices if a main() stops installing the abort hook, or installs it in debug builds as well
- [low] check-build-date.sh does not notice build.rs dropping the `src` rerun-if-changed line
- [low] The new panic-hook test leaves a multi-megabyte core file in the repository root when core dumps are enabled
- [low] The panic_abort test still passes when the hook prints the panic message twice
- [medium] The hook test passes when abort_on_panic replaces the previous hook instead of calling it, so the panic is not printed as it was before
- [medium] The hook test passes when a main() installs the aborting hook a second time without the cfg, so debug builds abort on any panic
- [low] The hook test fails with a bare NotFound unwrap while server_main.rs or main.rs is open in Emacs
- [low] check-build-date.sh's rerun check passes when build.rs narrows the `src` rerun line to `src/lib.rs`, so edits to any other source file leave BUILD_DATE stale
- [medium] Round 4's module-list parser stops the hook test from checking any module whose `mod` line has a trailing comment or a same-line attribute, so a second, unguarded abort_on_panic() install in that module passes

## Further lead rulings (2026-11-21, after the second loop's round 5)

3. **What the hook's test must guard, and no more.** The property is: (a) each `main()` installs the
   hook as its first statement, guarded by `#[cfg(not(debug_assertions))]`, and installs it nowhere
   else; (b) the hook prints the panic exactly as the default hook does and then aborts. A finding of
   the form "the test still passes if someone later adds other code to main() or to the library" counts
   only if that mutation breaks (a) or (b) as stated here. Do not grow the test into a parser of
   arbitrary source changes.
4. **The "nowhere else" check covers the two main files only (this replaces "installs it nowhere else"
   in ruling 3(a)).** Rounds 3 to 5 grew the hook's test from a check of the two mains into a scan of
   `src/` and then a parser of `mod` lines, and each step opened a new gap. The lead's decision: the test
   checks, through `include_str!` of `src/main.rs` and `src/server_main.rs` only, that (i) `main()` begins
   with `#[cfg(not(debug_assertions))]` followed by `task_deck::panic_abort::abort_on_panic();`, and (ii) the
   name `abort_on_panic` occurs exactly once in each of the two files (a plain substring count). It reads
   no other file, lists no directory and parses no module list. A call added somewhere in the library is a
   code change like any other and is out of scope for this test; do not report it and do not guard it.
5. **Scope of findings from here on.** A finding counts only if it shows one of the task's stated
   properties broken in today's working tree, or a check that passes while one of exactly these mutations
   is applied: the profile's panic setting changed; the probe no longer panicking; build.rs taking the date
   from local time, accepting an invalid SOURCE_DATE_EPOCH, or dropping or narrowing one of its listed rerun
   inputs; a main() no longer starting with the guarded hook call, or naming the hook a second time; the
   hook not printing as the default hook does, or not aborting. Other hypothetical edits are out of scope.


6. **The invalid SOURCE_DATE_EPOCH values the date check must try are a closed list:** empty; not a
   number; a leading plus sign; whitespace before or after; a fraction; a value outside a 64-bit integer;
   a year outside 0 to 9999; bytes that are not UTF-8. The check script covers these now. Any other
   spelling of an invalid value (other bases, digit separators, non-ASCII digits and so on) is out of
   scope: build.rs accepts only an optional minus sign followed by ASCII digits, and reviewers of build.rs
   can see that. Do not report further variants and do not add cases for them.


7. **The hook's test runs in the debug profile, by design.** A mutation that makes the hook itself depend
   on the build profile (for example its `abort()` under `#[cfg(debug_assertions)]`) is out of scope: the
   function is six lines with no conditional compilation, reviewers see it, and guarding it would mean a
   release build of the whole library in the gate. Do not report it and do not add a release-mode test run.


## Raised in the third loop (rounds 1 to 3), all handled; do not re-report

- [low] check-build-date.sh passes when build.rs accepts a SOURCE_DATE_EPOCH with a leading '+'
- [low] check-build-date.sh passes when build.rs drops the Cargo.toml, Cargo.lock, resources.rc and icon.ico rerun-if-changed lines
- [low] check-build-date.sh passes when build.rs trims trailing whitespace from SOURCE_DATE_EPOCH
- [medium] check-build-date.sh never tries a SOURCE_DATE_EPOCH outside a 64-bit integer
- [low] The hook test only runs in debug builds (ruled out of scope: ruling 7)

## Raised in the fourth loop (second session, 2026-11-21), handled; do not re-report

- [low] check-build-date.sh passes a build.rs that strips a trailing carriage return or a tab from SOURCE_DATE_EPOCH (fixed: the check now tries space, tab, LF and CR before and after the value)

## Further lead ruling (2026-11-21, second session)

8. **"Whitespace before or after" in ruling 6 means these four characters: space, tab, line feed and
   carriage return, at either end.** The check tries all eight combinations now. Any other character that
   some definition calls whitespace (vertical tab, form feed, a no-break space or other Unicode space) is
   out of scope, for the reason ruling 6 gives: build.rs accepts only an optional minus sign followed by
   ASCII digits, and a reviewer of build.rs can see that. Do not report further characters and do not add
   cases for them.
