export const meta = {
  name: 'foundation-c5-skeletons',
  description: 'Foundation, second working copy: apply three lead findings to the workspace, then write the skeleton crates from the specs (task 4): the public surface of each crate, the toy state machine and the fake sealer, then attack rounds; one agent at a time',
  phases: [
    { title: 'Implement', detail: 'workspace fix-up; the skeletons crate by crate; the toy state machine and the fake sealer', model: 'opus' },
    { title: 'Attack', detail: 'one fresh attacker per round, three lenses', model: 'opus' },
    { title: 'Fix', detail: 'reproduce first, then fix', model: 'opus' },
  ],
}

// args: { scratch: '<session scratchpad>', clone: 's', steps: ['ws-fixup', 'skel-core', 'skel-seal', 'skel-sync', 'skel-device', 'skel-fake', 'skel-loop'] }
// A launch runs the steps it names, in this order; a later launch with more steps at the END and resumeFromRunId
// replays the finished agents from the cache.
// Writers and attackers work on WORK, a clone of the repository under target/ (ignored by git in the main
// checkout). The main checkout stays clean: the lead copies verified work into it and commits.
const S = args.scratch
const STEPS = args.steps || []
const MAIN = '/home/user/taskdeck'
const CLONE = args.clone || 's'
const WORK = `${MAIN}/target/work/${CLONE}`
const BRIEFS = `${MAIN}/docs/sync/status/foundation/briefs`
const INBOX = `${S}/lead-findings`

const GROUND = `
GROUND RULES for every worker in stage "foundation" of SYNC_PLAN.md:
- TWO DIRECTORIES. ${MAIN} is the lead's main checkout: READ-ONLY for you (the plan, SYNC.md, the specs and the task briefs are read there). ${WORK} is YOUR WORKING COPY, a full clone of the same repository (branch claude/upbeat-hamilton-klp7n9) that also holds the uncommitted work of the tasks in progress: every file you create or change is under ${WORK}, and every command you run for the task runs there (cd ${WORK} first). Never create, change or delete anything under ${MAIN} outside ${WORK} (attackers may also use their own scratch directory, named in their task). The lead copies verified work from the working copy into the main checkout and commits it.
- Be economical with reading: your task's brief quotes what SYNC_PLAN.md requires. Open SYNC_PLAN.md, SYNC.md or a research paper only for the passage the brief or your lens sends you to, never in full. Where SYNC_PLAN.md and SYNC.md disagree, SYNC_PLAN.md wins.
- Do NOT run any git command that changes state, in either directory (no add, commit, push, fetch, pull, checkout, switch, stash, reset, restore, merge, rebase, worktree, clean). Read-only git (status, diff, log, show, ls-files, archive) is fine. In the working copy, "git status" shows the uncommitted work; "origin/day-planner" there is the base branch.
- Change only the paths your task names. Do not edit anything under src/ (unless a lead ruling for your task names the exact files), and do not edit DOCUMENTATION.md, SYNC.md or SYNC_PLAN.md.
- The repository is public: never write a secret, a real relay address, a token, an account name, an email address, a path that names a person, or anything from a real board. Made-up data only.
- No dependency or tool beyond SYNC_PLAN.md section 3.4's pre-approved list. If you think one is needed, do not add it: report it.
- Never weaken, skip or ignore a test. Never claim something passes unless you ran it and saw it pass; list the exact commands you ran.
- Files of other tasks may be uncommitted in the working copy: leave them alone; do not tidy files you did not create.
- Be economical: do the task well, then stop. Do not re-verify what your task did not change.
`

const REPORT = {
  type: 'object',
  properties: {
    summary: { type: 'string' },
    files_changed: { type: 'array', items: { type: 'string' } },
    commands_run: { type: 'array', items: { type: 'object', properties: { cmd: { type: 'string' }, result: { type: 'string' } }, required: ['cmd', 'result'] } },
    decisions: { type: 'array', items: { type: 'string' } },
    unverified: { type: 'array', items: { type: 'string' } },
    questions_for_lead: { type: 'array', items: { type: 'string' } },
  },
  required: ['summary', 'files_changed', 'commands_run', 'unverified', 'questions_for_lead'],
}
const FINDINGS = {
  type: 'object',
  properties: {
    findings: { type: 'array', items: {
      type: 'object',
      properties: {
        title: { type: 'string' },
        severity: { type: 'string', enum: ['high', 'medium', 'low'] },
        repro: { type: 'string' },
        expected: { type: 'string' },
        suggested_fix: { type: 'string' },
      },
      required: ['title', 'severity', 'repro', 'expected'],
    } },
    checked_and_fine: { type: 'array', items: { type: 'string' } },
  },
  required: ['findings', 'checked_and_fine'],
}

const SCRATCH_GIT = (i) => `Where a check needs a git repository with the base branch (the cooldown's default base, the CI path filter), make one in your scratch directory: "git clone -q ${WORK} ${MAIN}/target/attack-${i}/scratch/repo && git -C ${MAIN}/target/attack-${i}/scratch/repo fetch -q ${MAIN} day-planner:refs/remotes/origin/day-planner", then copy the working copy's uncommitted files of this task over it. Inside your scratch repositories, and only there, git commands that change state are allowed.`

const BATCH = 'Keep the work cheap: every message you send re-reads everything you have read so far, so use few messages: issue the tool calls you can foresee together, as parallel calls in one message, and do not re-read what you have read.'
const SPECS = `${WORK}/docs/sync/spec`
const BRIEF4 = `${BRIEFS}/skeletons.md`
const T4 = {
  id: 'skeletons', brief: BRIEF4,
  gate: 'cargo test --locked --workspace --exclude task_deck && cargo test --locked -p taskdeck-sync --features sim && cargo clippy --locked --workspace --exclude task_deck --all-targets -- -D warnings && cargo clippy --locked -p taskdeck-sync --features sim --all-targets -- -D warnings && cargo check --locked -p taskdeck-core -p taskdeck-sync --target wasm32-unknown-unknown && bash scripts/tests/run.sh && bash scripts/ci/reads.sh',
  others: 'Files of other tasks (the root Cargo.toml and Cargo.lock, scripts/, .github/, src/, fixtures/, the specs themselves) are not part of this task: do not attack them. A doubt about what a spec says is not a finding against the skeleton: note it under checked_and_fine',
  lenses: [
    { key: 'surface-equals-spec', text: 'spec conformance, item by item: for each crate, compare its public items with the "Public API" section of its spec (core.md, seal.md, engine.md, device.md, bridge.md under docs/sync/spec/ of the working copy; find the section with Grep and read it in full, nothing else of the spec unless it points there): path, name, generics, fields, variants, signature, the cargo feature that gates it, the derives the spec states. The strongest check is a scratch crate in your scratch directory that depends on the skeleton crates by path and names each item with the signature the spec writes (use statements, function-pointer coercions, struct literals), and must compile; do that for the two crates with the largest API first. Report an item that is missing, renamed, differently typed or gated differently' },
    { key: 'toy-and-fake', text: 'the toy state machine and the fake sealer (taskdeck-sync, feature sim) against engine.md section 5.3 and section 6.3 and the seal.md sections 6.3 keeps: write scratch tests against the public API: the examples and table rows of 5.3; seal, then classify and open, for each kind of entry; each refusal 6.3 keeps; a membership change of each kind; determinism (the same seed twice gives the same bytes; no HashMap or HashSet order reaches an output; no usize is encoded; it builds for wasm32-unknown-unknown); what is still unimplemented!() in these two and whether the "Not yet" list at the top of the module says so truthfully' },
    { key: 'later-stages-and-targets', text: 'what the stages of wave 1 need on day one: every proof in the brief, run in your scratch copy with a fresh target directory; each cargo feature on and off; clippy with -D warnings for each feature set; the Android hook crates/taskdeck-android/ci.sh with and without an NDK variable; that every body outside the toy and the fake is exactly unimplemented!() (search for todo!, panic!, unreachable! and placeholder return values); that no default feature enables a test feature; and a scratch test that builds, from the public API alone, what an Engine<ToyMachine, FakeSealer> will be handed (a genesis, the traits\' methods called in the order engine.md section 4.5 gives), to see that stage engine can start' },
  ],
}

const COMPILE_IMPL = `You are the only implementer or fixer compiling right now: in the working copy use its default target directory (${WORK}/target; do not set CARGO_TARGET_DIR). It holds a warm debug build.`
const rulingsNote = (t) => `The lead's standing rulings for this task are at the end of the brief and, if the file exists, in ${INBOX}/${t.id}.rulings.md : read them and never contradict them.`
const safe = async (thunk) => { try { return await thunk() } catch (e) { return null } }

const fix = (t, found, round) => safe(() => agent(`${GROUND}
YOUR TASK: handle findings against the task described in ${t.brief} (read it in full first; where it says "repository root" or names a path, that is in your working copy ${WORK}).
${rulingsNote(t)}
${COMPILE_IMPL}
For each finding: reproduce it first. If it does not reproduce, say so and drop it. If it reproduces but the task brief or a lead ruling puts it out of scope, drop it and say why. If it is a real defect within the task's owned paths (as the brief and the rulings extend them), fix it in the smallest correct way and add or keep a check that would have caught it. If the right fix lies outside those paths, do not make it: describe it under questions_for_lead. Never weaken a check to make it pass. Keep scripts short: prefer removing a fragile mechanism to guarding it with more code.
Findings (JSON):
${JSON.stringify(found, null, 2)}
Finish by running the task's gate in the working copy: ${t.gate}. Report every command and result. In files_changed list every path you changed, relative to the working copy. In your summary list each finding as fixed, dropped (with the reason) or deferred to the lead.`,
  { label: `fix:${t.id}:r${round}`, phase: 'Fix', model: 'opus', effort: 'high', schema: REPORT }))

const attacker = (t, round, seen) => safe(() => agent(`${GROUND}
YOU ARE AN ATTACKER, not an implementer. You did not write this code. Try to break the task below through three lenses, one after the other.
The task, its owned paths, the lead's standing rulings and the list of findings already raised and handled are in ${t.brief} : read it in full first. ${rulingsNote(t)}
The thing under attack is the working copy ${WORK} as it is now (its "git status" and "git diff" show what is ahead of the last commit). ${t.others}.
THE THREE LENSES (give each its own pass; say in checked_and_fine what you tried under each):
${t.lenses.map((l, k) => `${k + 1}. ${l.key}: ${l.text}.`).join('\n')}
LEAD FINDINGS: if the file ${INBOX}/${t.id}.md exists and is not empty, each section in it that starts with "## " is a finding written by the lead. Check each one yourself (reproduce it or confirm it from the evidence it cites); report those that hold as findings of yours, with the lead's text in the repro; then rename the file (mv) to ${INBOX}/${t.id}.consumed-r${round}.md .
Rules:
- You change NOTHING in the working copy: no edits, and do not run the task's scripts or cargo inside it. Your scratch directory is ${MAIN}/target/attack-${CLONE} : make a copy of the working copy's tree there ("rm -rf ${MAIN}/target/attack-${CLONE}/scratch && mkdir -p ${MAIN}/target/attack-${CLONE}/scratch/tree && tar -C ${WORK} --exclude=./target --exclude=./.git -cf - . | tar -C ${MAIN}/target/attack-${CLONE}/scratch/tree -xf -"; the main checkout's workspace excludes target/, so the copy builds on its own) and run everything in the copy, with CARGO_TARGET_DIR=${MAIN}/target/attack-${CLONE} for every cargo command. Mutate only your copy. ${SCRATCH_GIT(CLONE)} Tools that install into the cargo home (scripts/dev-setup.sh) get their own directory through TASKDECK_TOOLS_DIR under your scratch directory; the pinned cargo-deny and cargo-audit are already in ~/.cargo/bin and may be used, not replaced.
- Report only what you reproduced: the exact command or small test, the output you saw, and what should have happened. A doubt you could not reproduce goes under checked_and_fine with a note.
- No style preferences, nothing the brief or a lead ruling puts out of scope, nothing in the brief's "Already raised and handled" lists, and none of these, raised in this run: ${JSON.stringify(seen)}.
- Severity: high = today's app, a later stage or a security rule of SYNC_PLAN.md section 3.4 would be broken; medium = a check can pass when it should fail in CI or documented use, or behaviour differs from what the task states; low = robustness within the documented usage.
- Spend your effort on the most likely real defects; about twenty-five minutes of work in all is right.
Return the structured findings. An empty findings list after a real attempt is a valid and useful answer; list what you checked.`,
  { label: `attack:${t.id}:r${round}`, phase: 'Attack', model: 'opus', effort: 'high', schema: FINDINGS }))

// One fresh attacker per round covers the three lenses (decision D20). Two rounds without a finding end the loop.
const attackLoop = async (t) => {
  let dry = 0, round = 0
  const seen = []
  const lows = []
  let stopped = null
  while (dry < 2 && round < 6) {
    round++
    const reports = [await attacker(t, round, seen)]
    const found = reports.filter(Boolean).flatMap(r => r.findings)
    const serious = found.filter(f => f.severity !== 'low')
    log(`${t.id}, attack round ${round}: ${found.length} finding(s), ${serious.length} of them high or medium, ${reports.filter(Boolean).length}/${reports.length} agents reported`)
    if (!reports.every(Boolean)) { stopped = `an agent failed in round ${round}`; log(`${t.id}: ${stopped}; stopping the loop so nothing is counted as clean`); break }
    // Decision D24: a round with no high or medium finding is dry. Its low findings go to the lead's list.
    lows.push(...found.filter(f => f.severity === 'low').map(f => ({ round, title: f.title, repro: f.repro, suggested_fix: f.suggested_fix })))
    if (serious.length === 0) { dry++; continue }
    dry = 0
    seen.push(...found.map(f => `[r${round} ${f.severity}] ${f.title}`))
    const fixed = await fix(t, found, round)
    seen.push(`fix round ${round}: ${fixed ? fixed.summary.slice(0, 400) : 'FIX AGENT FAILED'}`)
    if (fixed === null) { stopped = `the fix agent failed in round ${round}`; break }
  }
  log(`${t.id}: ${round} attack rounds, ${dry >= 2 ? 'converged' : 'NOT converged'}`)
  return { id: t.id, converged: dry >= 2, rounds: round, stopped, seen, lows }
}

const out = {}
const skel = async (key, what, gate) => safe(() => agent(`${GROUND}
YOUR TASK is part of task 4, the skeleton crates. The brief is ${BRIEF4} : read it in full. Your part: ${what}
The specs are in your working copy: ${SPECS}/ . Find the sections the brief names with Grep and read those in full; do not read a spec in full. ${BATCH}
${rulingsNote(T4)}
${COMPILE_IMPL} Another agent may be compiling in a different working copy at the same time; that does not concern you.
Earlier parts of this task may already have filled other crates in the working copy: build on them, do not change them (if a type you need from another crate is missing or differs from its spec, report it under questions_for_lead and use the spec's path, so that it compiles once the other crate is right; if it cannot compile, say exactly why).
Done means these pass in the working copy: ${gate} . Report each command and its result, what the brief asks you to report, and what you could not do here. In files_changed list every path you created or changed, relative to the working copy.`,
  { label: `impl:${key}`, phase: 'Implement', model: 'opus', effort: 'high', schema: REPORT }))
const WS = 'cargo test --locked --workspace --exclude task_deck && cargo clippy --locked --workspace --exclude task_deck --all-targets -- -D warnings'

if (STEPS.includes('ws-fixup')) {
  phase('Implement')
  out.fixup = await safe(() => agent(`${GROUND}
YOUR TASK: three small changes to the workspace crates (task 2), each a finding the lead wrote. Read ${INBOX}/workspace.md in full (the three findings) and, in ${BRIEFS}/workspace.md , only the section "Lead rulings for task workspace" (rulings 2, 3 and 6). ${BATCH}
For each finding: check it, then make the change in the smallest correct way. Finding 1: crates/taskdeck-android/Cargo.toml and its README. Finding 2: crates/taskdeck-recover (manifest, and the dependency named once in the source like the others), Cargo.lock (only the new edge: run cargo without --locked once so that it adds it, then check with "git diff Cargo.lock" that nothing else moved), and the one rule in scripts/ci/changes.sh with its row in scripts/ci/README.md and a case in scripts/tests/test-ci-changes.sh. Finding 3: read hpke 0.15.1's source under ~/.cargo/registry/src/ ; change the root Cargo.toml's [workspace.dependencies] and crates/taskdeck-seal only if the ruling's condition holds, and say which case it was.
${COMPILE_IMPL} Another agent may be compiling in a different working copy at the same time; that does not concern you.
Done means these pass in the working copy: ${WS} && cargo test --locked -p taskdeck-android --no-default-features --features backend-echo && cargo clippy --locked -p taskdeck-android --no-default-features --features backend-echo --all-targets -- -D warnings && bash scripts/tests/run.sh && bash scripts/ci/reads.sh && bash scripts/supply-chain.sh . Report each command and its result. In files_changed list every path you changed, relative to the working copy. In your summary say for each finding what you did.`,
    { label: 'fix:workspace:lead-findings', phase: 'Implement', model: 'opus', effort: 'medium', schema: REPORT }))
  log(`workspace fix-up: ${out.fixup ? 'reported' : 'FAILED'}`)
  if (out.fixup === null) return out
}
const PARTS = [
  ['skel-core', 'part A for crates/taskdeck-core (core.md, "Public API").', `${WS} && cargo check --locked -p taskdeck-core --target wasm32-unknown-unknown`],
  ['skel-seal', 'part A for crates/taskdeck-seal (seal.md, "Public API"), with its test-entropy feature if the section names one.', `${WS} && cargo clippy --locked -p taskdeck-seal --all-features --all-targets -- -D warnings`],
  ['skel-sync', 'part A for crates/taskdeck-sync (engine.md, "Public API"), and of part B the toy state machine only (ToyMachine, engine.md section 5.3) with its tests; declare the modules fake and sim as the section writes them, with the fake sealer left for the next agent (FakeSealer and FakeMembership declared with unimplemented!() bodies and a "Not yet" note).', `${WS} && cargo test --locked -p taskdeck-sync --features sim && cargo clippy --locked -p taskdeck-sync --features sim --all-targets -- -D warnings && cargo check --locked -p taskdeck-core -p taskdeck-sync --target wasm32-unknown-unknown && cargo check --locked -p taskdeck-sync --features sim --target wasm32-unknown-unknown`],
  ['skel-device', 'part A for crates/taskdeck-device with its binary taskdeck-devicectl (device.md, "Public API") and for crates/taskdeck-android (bridge.md, "Public API", the Rust part), the hook crates/taskdeck-android/ci.sh, and the doc comments of taskdeck-backup and taskdeck-recover.', `${WS} && cargo clippy --locked -p taskdeck-device --features test-trust --all-targets -- -D warnings && cargo test --locked -p taskdeck-android --no-default-features --features backend-echo && cargo clippy --locked -p taskdeck-android --no-default-features --features backend-echo --all-targets -- -D warnings && CI_STAGE=android CI_OS=linux CI_WIDE=false bash crates/taskdeck-android/ci.sh && bash scripts/tests/run.sh && bash scripts/ci/reads.sh`],
  ['skel-fake', 'of part B the fake sealer: FakeSealer and FakeMembership in crates/taskdeck-sync (feature sim), engine.md section 6.3, working and tested as the brief says. The traits and the toy state machine are already there.', `${WS} && cargo test --locked -p taskdeck-sync --features sim && cargo clippy --locked -p taskdeck-sync --features sim --all-targets -- -D warnings && cargo check --locked -p taskdeck-sync --features sim --target wasm32-unknown-unknown`],
]
for (const [key, what, gate] of PARTS) {
  if (!STEPS.includes(key)) continue
  phase('Implement')
  out[key] = await skel(key, what, gate)
  log(`${key}: ${out[key] ? 'reported' : 'FAILED'}`)
  if (out[key] === null) return out
}
if (STEPS.includes('skel-loop')) { phase('Attack'); out.loop4 = await attackLoop(T4) }
return out
