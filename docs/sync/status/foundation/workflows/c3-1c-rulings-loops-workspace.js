export const meta = {
  name: 'foundation-c3-1c-ruling-loops-workspace',
  description: 'Foundation, code track, in a working copy: apply lead ruling 11 to the CI reads check, attack tasks 1b and 1c again until two dry rounds each (one attacker per round, three lenses), then implement the workspace (task 2); one agent at a time',
  phases: [
    { title: 'Implement', detail: '1b rulings; later the workspace (task 2)', model: 'opus' },
    { title: 'Attack', detail: 'one fresh attacker per round, covering the three lenses and the lead inbox', model: 'opus' },
    { title: 'Fix', detail: 'reproduce first, then fix', model: 'opus' },
  ],
}

// args: { scratch: '<session scratchpad>', steps: ['1c-ruling-11', '1b-loop', '1c-loop', 'workspace'] }
// Writers and attackers work on WORK, a clone of the repository under target/ (ignored by git in the main
// checkout). The main checkout stays clean: the lead copies verified work into it and commits.
const S = args.scratch
const STEPS = args.steps || ['1c-ruling-11', '1b-loop', '1c-loop', 'workspace']
const MAIN = '/home/user/taskdeck'
const WORK = `${MAIN}/target/work/c`
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

const T1B = {
  id: 'baseline-1b', brief: `${BRIEFS}/supply-chain.md`,
  gate: 'bash scripts/tests/run.sh && bash scripts/supply-chain.sh && bash scripts/cooldown.sh --all ; and, if Cargo.lock or Cargo.toml changed, also: cargo build --locked --no-default-features --bin taskdeck-server && cargo test --locked --no-default-features --lib (265 passed, 1 ignored)',
  others: 'Files of other tasks (rust-toolchain.toml, build.rs, crates/panic-probe, scripts/check-release-panic.sh, scripts/check-build-date.sh, src/, .github/, scripts/ci/) are not part of this task: do not attack them',
  lenses: [
    { key: 'checks-can-fail', text: 'adversarial inputs: try to get something past the gate that it exists to stop, the way CI or a developer runs it (bash scripts/supply-chain.sh, bash scripts/cooldown.sh, bash scripts/dev-setup.sh): a crate version younger than seven days, a yanked or missing version, a git or foreign-registry source, a second version of a crate on the cryptographic list, a crate with an advisory, a tampered or wrong tool archive or binary, a malformed, expired or over-broad exemption, a lockfile or index answer shaped to confuse the parser, a network or index failure during the check (it must fail closed, never pass), an environment variable that redirects a tool. Use made-up lockfiles, a local index directory and scratch projects; report only what passes when it should fail, or fails or misreports when it should pass' },
    { key: 'plan-conformance', text: 'spec conformance, clause by clause: SYNC_PLAN.md section 6.1 item 1 (deny.toml, cargo-deny, cargo-audit, the seven-day cooldown), section 3.4 (pre-approved tools only, official sources, pinned versions, checksums where published), SYNC.md section 8 "Supply chain" and section 7 (the dependencies row), R4 section 4.2, V1-4 (d), the task brief point by point, and the lead rulings at the end of the brief: is every targeted lockfile move exactly the ruled one and nothing else moved (compare Cargo.lock with origin/day-planner entry by entry), is every ignore one the lead ruled, with its reason, in both configuration files, do the two tools agree on what fails, is anything the brief asks for missing or done differently' },
    { key: 'fresh-machine-and-time', text: 'determinism across machines and over time: what happens on a fresh CI runner or a new container (no tools installed, an empty cargo home, no advisory database yet, a shallow checkout of a pull request merge commit where origin/day-planner may be absent, a different working directory, another locale or time zone, a python3 older than 3.11), when run twice, when run from a subdirectory, with spaces in the path, and as time passes (the --now option at the seven-day boundary, an exemption on its expiry day, a tool pin that is later yanked upstream). The gate must give the same verdict everywhere or say clearly why it cannot run' },
  ],
}
const T1C = {
  id: 'baseline-1c', brief: `${BRIEFS}/ci.md`,
  gate: 'bash scripts/tests/run.sh',
  others: 'Files of other tasks (deny.toml, .cargo/, scripts/dev-setup.sh, scripts/supply-chain.sh, scripts/cooldown*, the two check scripts, Cargo.lock, src/) are not part of this task: do not attack them, only how CI calls them',
  lenses: [
    { key: 'plan-conformance', text: `spec conformance, clause by clause: SYNC_PLAN.md section 6.1 item 1 last bullet (jobs filtered by path at job level; an aggregate job named gate that always runs, fails if any job failed or was cancelled, and treats skipped jobs as passed), section 3.2 (pushes to a pull request run the full matrix; other pushes only the Linux job unless the commit message contains "[ci full]"), section 1.4 (a branch protection that requires the check "gate"), SYNC.md section 8 "CI" (no secrets, contents: read, actions pinned by commit, no caches) and V1-4, and the brief's design point by point. Real evidence from GitHub is in ${S}/ci-evidence/1c.md if that file exists (the lead writes down what the real runs did): read it and check the design against what really happened. Report every place where the workflow or its scripts do something else than the plan and the brief say` },
    { key: 'hostile-events', text: 'adversarial inputs: event data an outsider controls reaches the scripts (a commit message, a branch name, file names in a pull request from a fork, the base and head commits). Try injection and confusion through scripts/ci/changes.sh in its test mode and by reading the YAML: a commit message with quotes, newlines, "$(...)", or "[ci full]" in odd places; changed paths with spaces, newlines, leading dashes, non-ASCII bytes, or names built to match a "needs no CI" rule while holding code (for example a file under docs/sync/status/ that a build reads, or a renamed file); a pull request that changes only the workflow or scripts/ci itself; a push with an all-zero or unknown "before" commit; a merge commit with one parent. The filter must fail safe: when in doubt, everything runs. Also: can a job be made to pass or be skipped so that gate reports success although something was not checked' },
    { key: 'later-stages-and-runners', text: 'what an older or newer version of the app, a later stage, or another runner would do: the per-stage jobs and scripts/ci/stage.sh when the crates arrive (a crate directory with and without ci.sh, a package name that differs from its directory, a stage with two directories, Windows and macOS runners with their bash and path forms), the toolchain step on runners whose rustup is older or newer or already has a different default, the concurrency group cancelling a superseded run and what gate then reports, re-runs of single jobs, the required-check name on pull requests against the push run of the same commit, and whether the path lists stay truthful when a later stage adds a file kind nobody listed' },
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
- You change NOTHING in the working copy: no edits, and do not run the task's scripts or cargo inside it. Your scratch directory is ${MAIN}/target/attack-0 : make a copy of the working copy's tree there ("rm -rf ${MAIN}/target/attack-0/scratch && mkdir -p ${MAIN}/target/attack-0/scratch/tree && tar -C ${WORK} --exclude=./target --exclude=./.git -cf - . | tar -C ${MAIN}/target/attack-0/scratch/tree -xf -"; the main checkout's workspace excludes target/, so the copy builds on its own) and run everything in the copy, with CARGO_TARGET_DIR=${MAIN}/target/attack-0 for every cargo command. Mutate only your copy. ${SCRATCH_GIT(0)} Tools that install into the cargo home (scripts/dev-setup.sh) get their own directory through TASKDECK_TOOLS_DIR under your scratch directory; the pinned cargo-deny and cargo-audit are already in ~/.cargo/bin and may be used, not replaced.
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
  let stopped = null
  while (dry < 2 && round < 6) {
    round++
    const reports = [await attacker(t, round, seen)]
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

if (STEPS.includes('1c-ruling-11')) {
  phase('Implement')
  out.ruling11 = await safe(() => agent(`${GROUND}
YOUR TASK: apply lead ruling 11 of the CI restructure. The task is described in ${T1C.brief} : read "Lead rulings for task baseline-1c" (rulings 7, 10 and 11) and the "Already raised and handled" list at its end; you need not read the rest.
Replace the hand-written reader of Cargo.toml in scripts/ci/reads.sh with python3's tomllib (run as "python3 -I", standard library only, as scripts/ci/gate.sh does): every tracked manifest is parsed as TOML, and a manifest that does not parse fails the check and is named. From the parsed data take what ruling 10 lists: path dependencies in every dependencies table (dependencies, dev-dependencies, build-dependencies, the target-specific ones, and [patch]), written inline or as tables, and those inherited with "workspace = true" from the root manifest's [workspace.dependencies]; each target's "path" ([lib], [[bin]], [[test]], [[example]], [[bench]]) and the package's "build". The Rust-source part (includes, the path attribute, symlinks) stays as it is. The result must be shorter and simpler than what it replaces; delete the awk manifest reader entirely.
Keep every case of scripts/tests/test-ci-reads.sh meaning what it means today (change a case only where it pins a message text of the old reader), and add cases for: a path written as a multi-line or literal TOML string, a dotted key (dependencies.foo.path = "..."), a target-specific dependencies table, and a manifest that is not valid TOML.
Change only scripts/ci/reads.sh, scripts/ci/README.md (if it describes the reader) and scripts/tests/test-ci-reads.sh. Then run "bash scripts/tests/run.sh" and "bash scripts/ci/reads.sh" in the working copy and report each command and its result. In files_changed list every path you changed, relative to the working copy.`,
    { label: 'impl:1c-ruling-11', phase: 'Implement', model: 'opus', effort: 'high', schema: REPORT }))
  log(`1c ruling 11: ${out.ruling11 ? 'reported' : 'FAILED'}`)
  if (out.ruling11 === null) return out
}

if (STEPS.includes('1b-loop')) { phase('Attack'); out.loop1b = await attackLoop(T1B) }
if (STEPS.includes('1c-loop')) { phase('Attack'); out.loop1c = await attackLoop(T1C) }

if (STEPS.includes('workspace')) {
  phase('Implement')
  out.workspace = await safe(() => agent(`${GROUND}
YOUR TASK is described in ${BRIEFS}/workspace.md . Read it in full and do exactly what it says; where it says "repository" or names a path, that is in your working copy ${WORK}. Where it gives a design, implement that design and report anything that cannot work instead of substituting your own.
${COMPILE_IMPL}
The supply-chain scripts the brief names exist (scripts/cooldown.sh, scripts/supply-chain.sh, scripts/dev-setup.sh; the pinned tools are installed in ~/.cargo/bin), and so do the CI scripts (scripts/ci/stage.sh and its README): read their headers and follow the hook convention they document.
When you are done, run every proof the brief lists and report each command and its result, the final dependency table with versions and features, every new lockfile entry with its publish date, the duplicate check, the TLS-provider check, and what you could not build here. In files_changed list every path you created or changed, relative to the working copy.`,
    { label: 'impl:workspace', phase: 'Implement', model: 'opus', effort: 'high', schema: REPORT }))
  log(`workspace implementation: ${out.workspace ? 'reported' : 'FAILED'}`)
}
return out
