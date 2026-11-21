export const meta = {
  name: 'foundation-wf6-interface-reviews-and-1a',
  description: 'Foundation: nine pairwise interface reviews of the specs, one resolution, six per-spec fixes; attack loop on baseline 1a, then the supply-chain task',
  phases: [
    { title: 'Interface reviews', detail: 'nine pairs, then one resolver', model: 'opus' },
    { title: 'Spec fixes', detail: 'one fixer per spec, applying the resolution', model: 'opus' },
    { title: 'Implement', detail: 'supply chain', model: 'opus' },
    { title: 'Attack', detail: 'three lenses per round, plus the lead inbox', model: 'opus' },
    { title: 'Fix', detail: 'reproduce first, then fix', model: 'opus' },
  ],
}

// Launch with Workflow({ scriptPath: '<this file>', args: { scratch: '<the session scratchpad directory>' } }).
// S holds the lead's inbox (S/lead-findings) and the resolver's output (S/resolutions): create both first.
const S = args.scratch
const BRIEFS = '/home/user/taskdeck/docs/sync/status/foundation/briefs'
const INBOX = `${S}/lead-findings`

const GROUND = `
GROUND RULES for every worker in stage "foundation" of SYNC_PLAN.md (repository /home/user/taskdeck, branch claude/focused-volta-tbqufb):
- First read SYNC_PLAN.md sections 2, 3, 4 and 6.1. Where SYNC_PLAN.md and SYNC.md disagree, SYNC_PLAN.md wins.
- Do NOT run any git command that changes state (no add, commit, push, fetch, pull, checkout, switch, stash, reset, restore, merge, rebase, worktree, clean). Read-only git (status, diff, log, show, ls-files, archive) is fine. The lead commits.
- Change only the paths your task names. Do not edit anything under src/ (unless a lead ruling for your task names the exact files), and do not edit DOCUMENTATION.md, SYNC.md or SYNC_PLAN.md.
- The repository is public: never write a secret, a real relay address, a token, an account name, an email address or anything from a real board. Made-up data only.
- No dependency or tool beyond SYNC_PLAN.md section 3.4's pre-approved list. If you think one is needed, do not add it: report it.
- Never weaken, skip or ignore a test. Never claim something passes unless you ran it and saw it pass; list the exact commands you ran.
- Other agents work in the same checkout on other paths at the same time (spec drafters write under docs/sync/spec/). Touch nothing outside your task; do not tidy files you did not create.
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
const SPEC_REPORT = {
  type: 'object',
  properties: {
    summary: { type: 'string' },
    sections: { type: 'array', items: { type: 'string' } },
    clause_counts: { type: 'array', items: { type: 'object', properties: { owner: { type: 'string' }, count: { type: 'number' } }, required: ['owner', 'count'] } },
    gaps_closed: { type: 'array', items: { type: 'string' } },
    assumptions_about_other_specs: { type: 'array', items: { type: 'string' } },
    questions_for_lead: { type: 'array', items: { type: 'string' } },
  },
  required: ['summary', 'sections', 'clause_counts', 'gaps_closed', 'assumptions_about_other_specs', 'questions_for_lead'],
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

// ------------------------------------------------------------------ Queue A: interface reviews
const PAIRS = [
  ['seal', 'relay'], ['seal', 'engine'], ['core', 'engine'], ['engine', 'relay'], ['device', 'bridge'], ['device', 'engine'],
  ['core', 'device'], ['seal', 'device'], ['relay', 'device'],
]
const REVIEW = {
  type: 'object',
  properties: {
    findings: { type: 'array', items: {
      type: 'object',
      properties: {
        title: { type: 'string' },
        severity: { type: 'string', enum: ['high', 'medium', 'low'] },
        spec_a_ref: { type: 'string' },
        spec_b_ref: { type: 'string' },
        problem: { type: 'string' },
        what_breaks: { type: 'string' },
        proposed_resolution: { type: 'string' },
        specs_to_change: { type: 'array', items: { type: 'string' } },
      },
      required: ['title', 'severity', 'spec_a_ref', 'spec_b_ref', 'problem', 'proposed_resolution', 'specs_to_change'],
    } },
    sound: { type: 'array', items: { type: 'string' } },
  },
  required: ['findings', 'sound'],
}
const RESOLUTION = {
  type: 'object',
  properties: {
    summary: { type: 'string' },
    accepted: { type: 'number' }, merged: { type: 'number' }, rejected: { type: 'number' },
    changes_per_spec: { type: 'array', items: { type: 'object', properties: { spec: { type: 'string' }, changes: { type: 'number' } }, required: ['spec', 'changes'] } },
    needs_lead_ruling: { type: 'array', items: { type: 'string' } },
  },
  required: ['summary', 'accepted', 'merged', 'rejected', 'changes_per_spec', 'needs_lead_ruling'],
}
const RES_FILE = `${S}/resolutions/round1.md`

const review = ([a, b]) => agent(`You review the INTERFACE between two specs of the TaskDeck sync build (repository /home/user/taskdeck): docs/sync/spec/${a}.md and docs/sync/spec/${b}.md. You did not write either. You change no file and run no git command that changes state.
Read ${BRIEFS}/reviews.md, section A ("Pairwise interface reviewer"): it is your task description. Then read docs/sync/spec/README.md (conventions), docs/sync/status/foundation.md (the sections "Rulings for the specs" with every ruling S1 to S31 and the settled form of S15, and "Decisions"), then both specs in full, then the parts of SYNC.md and SYNC_PLAN.md they cite where you need the intent. The other four specs exist too (seal, core, relay, engine, device, bridge): open them only to check a reference.
Besides what section A lists, report every place where either spec does not yet reflect a lead ruling that touches this pair (for example the settled Skip floor S15, reinstatement carrying missed roots S24, the two snapshot triggers S25, the control history in the bootstrap snapshot S27, the backup name S28, the host length cap, the recovery read epoch, the sealed file kinds device.md adds, the phone's own picture S29), with the exact change each spec needs.
Every finding cites both sides by section or clause id and says what breaks. Propose the resolution: which spec changes, and to exactly what. Do not report style. Order findings by severity. Return the structured result.`,
  { label: `review:${a}-${b}`, phase: 'Interface reviews', model: 'opus', schema: REVIEW })

const queueA = async () => {
  const out = {}
  const reviews = await parallel(PAIRS.map(p => () => review(p)))
  const ok = reviews.filter(Boolean)
  out.reviewed = ok.length
  out.findings = ok.reduce((n, r) => n + r.findings.length, 0)
  log(`interface reviews: ${ok.length}/${PAIRS.length} reported, ${out.findings} findings`)
  if (ok.length < PAIRS.length) { out.failed = PAIRS.filter((_, i) => !reviews[i]).map(p => p.join('-')); return out }
  const all = reviews.flatMap((r, i) => r.findings.map((f, k) => ({ id: `${PAIRS[i][0]}-${PAIRS[i][1]}-${k + 1}`, pair: PAIRS[i].join('-'), ...f })))
  out.all = all.map(f => ({ id: f.id, severity: f.severity, title: f.title, specs_to_change: f.specs_to_change }))
  out.resolution = await agent(`You are the resolver for the interface reviews of the six TaskDeck sync specs (repository /home/user/taskdeck, docs/sync/spec/). Read ${BRIEFS}/reviews.md, section B ("Resolver"): it is your task description. Read docs/sync/spec/README.md and docs/sync/status/foundation.md ("Rulings for the specs", S1 to S31, and "Decisions": the rulings are binding and settle several findings already), then for each finding the sections of the specs it cites.
You change no spec and run no git command that changes state. Write the resolution list to ${RES_FILE} (create the directory if needed): one level-two heading per spec that must change ("## seal.md", "## core.md", "## relay.md", "## engine.md", "## device.md", "## bridge.md"); under it one numbered change per item, each with the finding ids it resolves, the section to change, and the exact new rule, layout, field, name or clause text (so that the fixer designs nothing), including clause ids to add (give new ids after the highest existing number of their topic; never reuse or renumber an id). A change that needs matching edits in two specs appears under both, with the same wording for the shared part. End the file with "## Rejected" (finding id and the reason) and "## For the lead" (anything that needs a ruling, with your recommended answer; do not leave the specs inconsistent meanwhile: pick the conservative reading and say so).
The findings (JSON):
${JSON.stringify(all, null, 1)}
Return the structured summary.`,
    { label: 'resolve:round1', phase: 'Interface reviews', model: 'opus', schema: RESOLUTION })
  if (out.resolution === null) return out
  out.fixes = await parallel(['seal', 'core', 'relay', 'engine', 'device', 'bridge'].map(name => () => agent(`You apply the interface-review resolution to ONE spec of the TaskDeck sync build: docs/sync/spec/${name}.md (repository /home/user/taskdeck). Edit only that file. Do not run git commands that change state. Another agent edits each of the other five specs at the same time; never touch theirs.
Read ${BRIEFS}/reviews.md, section C ("Per-spec fixer"), ${BRIEFS}/drafter-common.md (the rules the spec was drafted under, including "Before you finish"), docs/sync/spec/README.md, and the resolution list ${RES_FILE} : apply exactly the changes under the heading "## ${name}.md", and read the other headings so that shared wording matches what the other specs will say. If that heading is absent or empty, change nothing and say so.
Keep the README's clause format; never renumber or reuse an existing id; keep the sections in order; update the error catalogue, limits, interfaces and public API sections where a change touches them. Afterwards run the "Before you finish" checks on the file (clause regular expression, unique ids, layouts add up, cross-references resolve). Return the structured report: in the summary, each resolution item as applied (with the section) or not applied (with the reason).`,
    { label: `specfix:${name}`, phase: 'Spec fixes', model: 'opus', schema: REPORT })))
  out.fixed = out.fixes.filter(Boolean).length
  return out
}

// ------------------------------------------------------------------ Queue B: baseline 1a again, then 1b
const T1A = {
  id: 'baseline-1a', brief: `${BRIEFS}/task-1a.md`,
  gate: 'scripts/check-release-panic.sh && scripts/check-build-date.sh && cargo build --locked --no-default-features --bin taskdeck-server && cargo test --locked --no-default-features --lib (about 4 minutes; must report 265 passed, 1 ignored once the panic hook and its test are in)',
  lenses: [
    { key: 'behaviour-preserved', text: 'what an older or newer version of the app would do: today\'s app must build and behave as before on every command a developer or CI runs; in particular the new panic hook must restore, in release builds of TaskDeck and taskdeck-server, exactly the old "any panic ends the process" behaviour (main thread and every background thread), must leave debug builds as they were, must not swallow or duplicate the panic message, and must not break the library when it is used by other crates (the fixture generator will link it) or the tests on Linux, macOS and Windows' },
    { key: 'checks-can-fail', text: 'adversarial inputs: make each property fail on purpose in a scratch copy (panic back to "abort"; the hook removed from one main; the hook installed in debug builds; build.rs using local time or dropping a rerun line) and see whether the task\'s scripts and tests notice in the way CI runs them (a fresh checkout, the default environment). Only what CI or the documented local use would be misled by counts (lead ruling 2 in the brief)' },
    { key: 'reproducible-and-pinned', text: 'determinism across devices and supply chain: the toolchain pin on a machine whose rustup default differs; every moved crate past its advisory and outside the 7-day cooldown; nothing else changed in Cargo.lock against origin/day-planner; the build script reruns exactly when it says; the release probe measures the profile the shipped binaries use; the new hook test is deterministic (no timing flake on a slow or loaded machine, no leftover child processes, no dependence on the test thread count)' },
  ],
}
const T1B = { id: 'baseline-1b', brief: `${BRIEFS}/supply-chain.md`, gate: 'bash scripts/tests/run.sh && bash scripts/supply-chain.sh' }

const COMPILE_IMPL = 'You are the only implementer or fixer compiling right now: use the default target directory (do not set CARGO_TARGET_DIR). Attackers use their own directories under target/attack-N; leave those alone.'
const rulingsNote = (t) => `The lead's standing rulings for this task are at the end of the brief and, if the file exists, in ${INBOX}/${t.id}.rulings.md : read them and never contradict them.`

const fix = (t, found, round, fromFile) => agent(`${GROUND}
YOUR TASK: handle findings against the task described in ${t.brief} (read it in full first).
${rulingsNote(t)}
${COMPILE_IMPL}
For each finding: reproduce it first. If it does not reproduce, say so and drop it. If it reproduces but the task brief or a lead ruling puts it out of scope, drop it and say why. If it is a real defect within the task's owned paths (as the brief and the rulings extend them), fix it in the smallest correct way and add or keep a check that would have caught it. If the right fix lies outside those paths, do not make it: describe it under questions_for_lead. Never weaken a check to make it pass. Keep scripts short: prefer removing a fragile mechanism to guarding it with more code.
${fromFile ? `The findings are in the file ${fromFile} (read it; each section headed "## " is one finding written by the lead). When you have finished, rename that file (mv) to ${fromFile.replace('.md', '.consumed-by-fix.md')} .` : `Findings (JSON):\n${JSON.stringify(found, null, 2)}`}
Finish by running the task's gate: ${t.gate}. Report every command and result. In your summary list each finding as fixed, dropped (with the reason) or deferred to the lead.`,
  { label: `fix:${t.id}:${fromFile ? 'lead' : 'r' + round}`, phase: fromFile ? 'Implement' : 'Fix', model: 'opus', effort: 'high', schema: REPORT })

const attackLoop = async (t) => {
  let dry = 0, round = 0
  const seen = []
  while (dry < 2 && round < 6) {
    round++
    const reports = await parallel([
      ...t.lenses.map((lens, i) => () => agent(`${GROUND}
YOU ARE AN ATTACKER, not an implementer. You did not write this code. Try to break the task below through ONE lens.
The task, its owned paths, the lead's standing rulings and the list of findings already raised and handled are in ${t.brief} : read it in full first. ${rulingsNote(t)}
The thing under attack is the working tree as it is now ("git status", "git diff"; it may be ahead of HEAD).
YOUR LENS: ${lens.text}.
Rules:
- Do not edit tracked files and create nothing in the repository outside your scratch directory /home/user/taskdeck/target/attack-${i} . Use CARGO_TARGET_DIR=/home/user/taskdeck/target/attack-${i} for every cargo command (it holds a warm debug build of the root package) and keep scratch files under /home/user/taskdeck/target/attack-${i}/scratch (the workspace excludes target/, so a copied tree there builds on its own). To test a modified file, copy what you need there ("git archive HEAD | tar -x -C <scratch>/tree", then copy the working-tree versions over it); "git worktree" is not allowed.
- Report only what you reproduced: the exact command or small test, the output you saw, and what should have happened. A doubt you could not reproduce goes under checked_and_fine with a note.
- No style preferences, nothing the brief or a lead ruling puts out of scope, nothing in the brief's "Already raised and handled" list, and none of these, raised in this run: ${JSON.stringify(seen)}.
- Severity: high = today's app, a later stage or a security rule of SYNC_PLAN.md section 3.4 would be broken; medium = a check can pass when it should fail in CI or documented use, or behaviour differs from what the task states; low = robustness within the documented usage.
- Spend your effort on the most likely real defects; about fifteen minutes of work is right.
Return the structured findings. An empty findings list after a real attempt is a valid and useful answer; list what you checked.`,
        { label: `attack:${t.id}:${lens.key}:r${round}`, phase: 'Attack', model: 'opus', effort: 'high', schema: FINDINGS })),
      () => agent(`You are the lead's inbox reader for task "${t.id}" in stage "foundation" (repository /home/user/taskdeck). Do only this: check whether the file ${INBOX}/${t.id}.md exists and is non-empty. If it does not exist or is empty, return an empty findings list with checked_and_fine ["no lead findings"]. If it exists: read it. Each section that starts with a line beginning "## " is one finding written by the lead: return each as a finding (title = the heading text; severity = the value after "severity:" in the section if present, otherwise "medium"; repro = the whole section text verbatim; expected = the text after "expected:" if present, otherwise "see repro"). Then rename the file (mv) to ${INBOX}/${t.id}.consumed-r${round}.md so it is not read twice. Change nothing else anywhere.`,
        { label: `inbox:${t.id}:r${round}`, phase: 'Attack', model: 'opus', effort: 'low', schema: FINDINGS }),
    ])
    const found = reports.filter(Boolean).flatMap(r => r.findings)
    log(`${t.id}, attack round ${round}: ${found.length} finding(s), ${reports.filter(Boolean).length}/${reports.length} agents reported`)
    if (!reports.every(Boolean)) { log(`${t.id}: an agent failed in round ${round}; stopping the loop so nothing is counted as clean`); break }
    if (found.length === 0) { dry++; continue }
    dry = 0
    seen.push(...found.map(f => `[r${round} ${f.severity}] ${f.title}`))
    const fixed = await fix(t, found, round, null)
    seen.push(`fix round ${round}: ${fixed ? fixed.summary.slice(0, 400) : 'FIX AGENT FAILED'}`)
    if (fixed === null) break
  }
  log(`${t.id}: ${round} attack rounds, ${dry >= 2 ? 'converged' : 'NOT converged'}`)
  return { id: t.id, converged: dry >= 2, rounds: round, seen }
}

const queueB = async () => {
  const out = {}
  out.loop1a = await attackLoop(T1A)
  out.impl1b = await agent(`${GROUND}
YOUR TASK is described in ${BRIEFS}/supply-chain.md . Read it in full and do exactly what it says; where it gives a design, implement that design and report anything that cannot work instead of substituting your own.
${COMPILE_IMPL}
When you are done, run the task's gate (${T1B.gate}) and every proof the brief asks for; the supply-chain script is expected to report the advisories the brief describes until the lead decides on them: report them exactly, do not change Cargo.lock. Return the structured report: what you built, each command and its result, the decisions you took, what you could not verify, and questions for the lead.`,
    { label: 'impl:baseline-1b', phase: 'Implement', model: 'opus', effort: 'high', schema: REPORT })
  return out
}

const [a, b] = await parallel([queueA, queueB])
return { specs: a, tasks: b }
