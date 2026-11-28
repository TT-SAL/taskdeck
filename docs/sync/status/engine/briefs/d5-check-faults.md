# Task D5: three places where the check blames the engine for what the spec asks of it

Base: the commit that holds this brief. Read `d-simulator.md`, `d2-simulator-followup.md`,
`d3-gate-seeds.md` and `d4-sim-tests.md` in this directory for the rules that still hold: the
invariants are read strictly and nobody loosens one; you do not read
`crates/taskdeck-sync/src/engine/**`; you change only the files this task lists; the simulator's
own tests build the situations they check (`src/sim/script.rs`), none names what a seed produced.

The lead read every violation of 300 `gate` seeds at base 2000 and traced the ones that are not
"lost to a rolled-back relay state", the Skip race, the §28.4 step 4 text or the crash halt on a
Skip's target (those four wait for the owner). Three kinds are left, and in each the engine does
what a clause tells it to do. Seeds move whenever the engine changes, so take each pattern from
its description; the seeds are those of this commit.

## 1. A duplicated answer to a read is not a relay head below the log head

`gate` 2085 (invariant 4 at trace line 5132: "dev 0: applied seq 115 after a relay head 113 below
its log head 114 without an alarm") and 2120 (trace line 8626). The trace of 2120, dev 4:
`welcome seq 180`, `read id 94`, fault `duplication`, `page seq 180 n 4` (the answer), its own
`append`, `append_results`, `page seq 182 n 2` (the push), then `page seq 180 n 4` again: the
second copy of the answer to read 94, delivered after the device's log head had moved on.
ENG-EFFECT-002: an answer whose request id was not issued or was already answered is ignored.
The engine ignores it, so no alarm of §21.1 is owed. The check must not count a page that
answers a request already answered (or never issued) as a relay head the device saw. A push
(no request id) and the first answer to a read stay counted as they are.

## 2. A write that was never applied deletes nothing, and a crash cuts the call

Seen at an earlier commit (`gate` 2210, invariant 5 "outbox Drop of the Register record of … with
no PairFailed in the same or the next call"): the trace was `admin_result` (the register
refused), `persist id 188 n 1` (the Drop), then `crash at before`: the process died before that
write was applied, and before the runtime performed the rest of that call's effects. After the
`start` the record was still in the outbox, the engine sent the register again, and on the second
refusal it emitted `pair_failed`. The check blamed the first call. A `Drop` (and any other
deletion of invariant 5) counts only when its write was applied; and "in the same or the next
call" cannot be asked of a call whose effects a crash cut off before the runtime performed them.

## 3. A `snap` record dropped unsent because the epoch moved

`gate` 2172 (invariant 5 at trace line 3533: "dev 3: outbox Drop of … (kind Snap, seen None, ack
None) with B = 72"). Dev 3's own rotation was accepted at 72; the snapshot it had uploaded just
before was sealed under the epoch that rotation ended. §19.4 step 5 and ENG-SNAP-004: an unsent
`snap` record whose blob epoch is no longer the current epoch MUST be dropped. Invariant 5 lists
"an accepted epoch rotation" among the explanations; the check must accept this drop (a `snap`
record never sent, dropped at or after an accepted rotation that changed the epoch its blob was
sealed under), and nothing wider.

## Also

- `seeds_hold` prints the first five violations of a failing seed, which hid two faults from the
  lead. Add a way to print every violation of every failing seed in one run (an environment
  variable, documented in `api-simulator.md`), without changing the default output.
- Each of the three fixes with a test in `src/sim/script.rs` (or beside the check) that fails
  when the fix is removed, built by hand as task D4's tests are.
- Then run `gate` with `TD_SIM_BASE=2000 SIM_SEEDS=300`, reading every violation, and list every
  failing seed under exactly one cause: rolled-back or forked relay state; the Skip race
  (`classes there: ["refuse"]` for a Skip two devices composed); the §28.4 step 4 text; a crash
  halt on a Skip's target; anything else, reported as seed, first bad trace line, clause and the
  sequence in five lines. The lead's count before this task: 178 fail; 168 only rolled-back or
  forked state; 2019, 2082, 2167 the Skip race; 2075 the pairing; 2159, 2207, 2236 the crash
  halt; 2085, 2120 and 2172 as above.

## Done when

The unit tests, `relay_conformance`, clippy with `-D warnings` and the wasm check pass.
`regressions` passes except the file `1099.json`, which fails at this commit only with "lost to a
rolled-back relay state" (the owner's open question): leave that file as it is and do not make
it pass. Use your own `CARGO_TARGET_DIR`.
