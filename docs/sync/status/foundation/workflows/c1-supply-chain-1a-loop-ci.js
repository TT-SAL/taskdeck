export const meta = {
  name: 'foundation-c1-supply-chain-1a-loop-ci',
  description: 'Foundation, code track: implement the supply-chain task (1b), run the attack loop on baseline 1a until two dry rounds, then implement the CI restructure (1c); one agent at a time',
  phases: [
    { title: 'Implement', detail: 'supply chain (1b), then the CI restructure (1c)', model: 'opus' },
    { title: 'Attack', detail: 'baseline 1a: three lenses per round, plus the lead inbox', model: 'opus' },
    { title: 'Fix', detail: 'reproduce first, then fix', model: 'opus' },
  ],
}

// args: { scratch: '<session scratchpad>' }. S/lead-findings is the lead's inbox.
const S = args.scratch
const REPO = '/home/user/taskdeck'
const BRIEFS = `${REPO}/docs/sync/status/foundation/briefs`
const INBOX = `${S}/lead-findings`

const GROUND = `
GROUND RULES for every worker in stage "foundation" of SYNC_PLAN.md (repository ${REPO}, branch claude/upbeat-hamilton-klp7n9):
- First read SYNC_PLAN.md sections 2, 3, 4 and 6.1. Where SYNC_PLAN.md and SYNC.md disagree, SYNC_PLAN.md wins.
- Do NOT run any git command that changes state (no add, commit, push, fetch, pull, checkout, switch, stash, reset, restore, merge, rebase, worktree, clean). Read-only git (status, diff, log, show, ls-files, archive) is fine. The lead commits.
- Change only the paths your task names. Do not edit anything under src/ (unless a lead ruling for your task names the exact files), and do not edit DOCUMENTATION.md, SYNC.md or SYNC_PLAN.md.
- The repository is public: never write a secret, a real relay address, a token, an account name, an email address or anything from a real board. Made-up data only.
- No dependency or tool beyond SYNC_PLAN.md section 3.4's pre-approved list. If you think one is needed, do not add it: report it.
- Never weaken, skip or ignore a test. Never claim something passes unless you ran it and saw it pass; list the exact commands you ran.
- Another agent works in the same checkout at the same time on other paths (the specs under docs/sync/spec/). Touch nothing outside your task; do not tidy files you did not create. Files of other tasks may be uncommitted in the working tree: leave them alone.
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
    model_self_report: { type: 'string' },
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

const T1A = {
  id: 'baseline-1a', brief: `${BRIEFS}/task-1a.md`,
  gate: 'scripts/check-release-panic.sh && scripts/check-build-date.sh && cargo build --locked --no-default-features --bin taskdeck-server && cargo test --locked --no-default-features --lib (about 4 minutes; must report 265 passed, 1 ignored)',
  lenses: [
    { key: 'behaviour-preserved', text: 'what an older or newer version of the app would do: today\'s app must build and behave as before on every command a developer or CI runs; in particular the panic hook must restore, in release builds of TaskDeck and taskdeck-server, exactly the old "any panic ends the process" behaviour (main thread and every background thread), must leave debug builds as they were, must not swallow or duplicate the panic message, and must not break the library when it is used by other crates (the fixture generator will link it) or the tests on Linux, macOS and Windows' },
    { key: 'checks-can-fail', text: 'adversarial inputs: make each property fail on purpose in a scratch copy, using only the closed list of mutations in the brief\'s lead rulings 5 and 6 (the profile\'s panic setting; the probe no longer panicking; build.rs taking the date from local time, accepting one of the listed invalid SOURCE_DATE_EPOCH values, or dropping or narrowing a listed rerun input; a main() not starting with the guarded hook call or naming the hook twice; the hook not printing as the default hook does, or not aborting), and see whether the task\'s scripts and tests notice in the way CI runs them (a fresh checkout, the default environment). Only what CI or the documented local use would be misled by counts' },
    { key: 'reproducible-and-pinned', text: 'determinism across devices and supply chain: the toolchain pin on a machine whose rustup default differs; every moved crate past its advisory and outside the 7-day cooldown; nothing else changed in Cargo.lock against the commit before the refresh (git log -- Cargo.lock shows it); the build script reruns exactly when it says; the release probe measures the profile the shipped binaries use; the hook test is deterministic (no timing flake on a slow or loaded machine, no leftover child processes, no dependence on the test thread count)' },
  ],
}
const T1B = { id: 'baseline-1b', brief: `${BRIEFS}/supply-chain.md`, gate: 'bash scripts/tests/run.sh && bash scripts/supply-chain.sh' }
const T1C = { id: 'baseline-1c', brief: `${BRIEFS}/ci.md`, gate: 'bash scripts/tests/run.sh' }

const COMPILE_IMPL = 'You are the only implementer or fixer compiling right now: use the default target directory (do not set CARGO_TARGET_DIR). Attackers use their own directories under target/attack-N; leave those alone.'
const rulingsNote = (t) => `The lead's standing rulings for this task are at the end of the brief and, if the file exists, in ${INBOX}/${t.id}.rulings.md : read them and never contradict them.`
const safe = async (thunk) => { try { return await thunk() } catch (e) { return null } }

const fix = (t, found, round) => safe(() => agent(`${GROUND}
YOUR TASK: handle findings against the task described in ${t.brief} (read it in full first).
${rulingsNote(t)}
${COMPILE_IMPL}
For each finding: reproduce it first. If it does not reproduce, say so and drop it. If it reproduces but the task brief or a lead ruling puts it out of scope, drop it and say why. If it is a real defect within the task's owned paths (as the brief and the rulings extend them), fix it in the smallest correct way and add or keep a check that would have caught it. If the right fix lies outside those paths, do not make it: describe it under questions_for_lead. Never weaken a check to make it pass. Keep scripts short: prefer removing a fragile mechanism to guarding it with more code.
Findings (JSON):
${JSON.stringify(found, null, 2)}
Finish by running the task's gate: ${t.gate}. Report every command and result. In your summary list each finding as fixed, dropped (with the reason) or deferred to the lead.`,
  { label: `fix:${t.id}:r${round}`, phase: 'Fix', model: 'opus', effort: 'high', schema: REPORT }))

const attacker = (t, lens, i, round, seen) => safe(() => agent(`${GROUND}
YOU ARE AN ATTACKER, not an implementer. You did not write this code. Try to break the task below through ONE lens.
The task, its owned paths, the lead's standing rulings and the list of findings already raised and handled are in ${t.brief} : read it in full first. ${rulingsNote(t)}
The thing under attack is the working tree as it is now ("git status", "git diff"; it may be ahead of HEAD). Uncommitted files of other tasks (deny.toml, .cargo/, scripts/dev-setup.sh, scripts/supply-chain.sh, scripts/cooldown*, scripts/tests/, scripts/ci/, .github/) are not part of this task: do not attack them.
YOUR LENS: ${lens.text}.
Rules:
- Do not edit tracked files and create nothing in the repository outside your scratch directory ${REPO}/target/attack-${i} . Use CARGO_TARGET_DIR=${REPO}/target/attack-${i} for every cargo command (it holds a warm debug build of the root package's library tests) and keep scratch files under ${REPO}/target/attack-${i}/scratch (the workspace excludes target/, so a copied tree there builds on its own). To test a modified file, copy what you need there ("git archive HEAD | tar -x -C <scratch>/tree", then copy the working-tree versions of this task's files over it); "git worktree" is not allowed.
- Report only what you reproduced: the exact command or small test, the output you saw, and what should have happened. A doubt you could not reproduce goes under checked_and_fine with a note.
- No style preferences, nothing the brief or a lead ruling puts out of scope, nothing in the brief's "Already raised and handled" lists, and none of these, raised in this run: ${JSON.stringify(seen)}.
- Severity: high = today's app, a later stage or a security rule of SYNC_PLAN.md section 3.4 would be broken; medium = a check can pass when it should fail in CI or documented use, or behaviour differs from what the task states; low = robustness within the documented usage.
- Spend your effort on the most likely real defects; about fifteen minutes of work is right.
Return the structured findings. An empty findings list after a real attempt is a valid and useful answer; list what you checked.`,
  { label: `attack:${t.id}:${lens.key}:r${round}`, phase: 'Attack', model: 'opus', effort: 'high', schema: FINDINGS }))

const inbox = (t, round) => safe(() => agent(`You are the lead's inbox reader for task "${t.id}" in stage "foundation" (repository ${REPO}). Do only this: check whether the file ${INBOX}/${t.id}.md exists and is non-empty. If it does not exist or is empty, return an empty findings list with checked_and_fine ["no lead findings"]. If it exists: read it. Each section that starts with a line beginning "## " is one finding written by the lead: return each as a finding (title = the heading text; severity = the value after "severity:" in the section if present, otherwise "medium"; repro = the whole section text verbatim; expected = the text after "expected:" if present, otherwise "see repro"). Then rename the file (mv) to ${INBOX}/${t.id}.consumed-r${round}.md so it is not read twice. Change nothing else anywhere.`,
  { label: `inbox:${t.id}:r${round}`, phase: 'Attack', model: 'opus', effort: 'low', schema: FINDINGS }))

// One agent at a time: the lenses of a round run one after another.
const attackLoop = async (t) => {
  let dry = 0, round = 0
  const seen = []
  let stopped = null
  while (dry < 2 && round < 6) {
    round++
    const reports = []
    for (let i = 0; i < t.lenses.length; i++) reports.push(await attacker(t, t.lenses[i], i, round, seen))
    reports.push(await inbox(t, round))
    const found = reports.filter(Boolean).flatMap(r => r.findings)
    log(`${t.id}, attack round ${round}: ${found.length} finding(s), ${reports.filter(Boolean).length}/${reports.length} agents reported`)
    if (!reports.every(Boolean)) { stopped = `an agent failed in round ${round}`; log(`${t.id}: ${stopped}; stopping the loop so nothing is counted as clean`); break }
    if (found.length === 0) { dry++; continue }
    dry = 0
    seen.push(...found.map(f => `[r${round} ${f.severity}] ${f.title}`))
    const fixed = await fix(t, found, round)
    seen.push(`fix round ${round}: ${fixed ? fixed.summary.slice(0, 400) : 'FIX AGENT FAILED'}`)
    if (fixed === null) { stopped = `the fix agent failed in round ${round}`; break }
  }
  log(`${t.id}: ${round} attack rounds, ${dry >= 2 ? 'converged' : 'NOT converged'}`)
  return { id: t.id, converged: dry >= 2, rounds: round, stopped, seen }
}

const out = {}

phase('Implement')
out.impl1b = await safe(() => agent(`${GROUND}
YOUR TASK is described in ${T1B.brief} . Read it in full and do exactly what it says; where it gives a design, implement that design and report anything that cannot work instead of substituting your own.
${COMPILE_IMPL}
When you are done, run the task's gate (${T1B.gate}) and every proof the brief asks for. The supply-chain script is expected to report the advisories the brief describes until the lead decides on them: report them exactly, and do not change Cargo.lock or Cargo.toml. Return the structured report: what you built, each command and its result, the decisions you took, what you could not verify, and questions for the lead. In model_self_report give the exact model name or id your own system prompt says you are.`,
  { label: 'impl:baseline-1b', phase: 'Implement', model: 'opus', effort: 'high', schema: REPORT }))
log(`baseline-1b implementation: ${out.impl1b ? 'reported' : 'FAILED'}`)

phase('Attack')
out.loop1a = await attackLoop(T1A)

phase('Implement')
if (out.impl1b !== null) {
  out.impl1c = await safe(() => agent(`${GROUND}
YOUR TASK is described in ${T1C.brief} . Read it in full and do exactly what it says; where it gives a design, implement that design and report anything that cannot work instead of substituting your own. The scripts of the supply-chain task (scripts/dev-setup.sh, scripts/supply-chain.sh, scripts/cooldown.sh, scripts/tests/run.sh) exist in the working tree now: read them and wire the CI job to their real interface; do not change them except to add your own test files under scripts/tests/.
${COMPILE_IMPL}
When you are done, run the task's gate (${T1C.gate}) and every validation the brief asks for. Return the structured report: what you built, each command and its result, the decisions you took, precisely what you could not verify locally, and questions for the lead.`,
    { label: 'impl:baseline-1c', phase: 'Implement', model: 'opus', effort: 'high', schema: REPORT }))
  log(`baseline-1c implementation: ${out.impl1c ? 'reported' : 'FAILED'}`)
}
return out