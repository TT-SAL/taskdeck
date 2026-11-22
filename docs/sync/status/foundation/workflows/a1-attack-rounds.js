export const meta = {
  name: 'foundation-a1-attack-rounds',
  description: 'Foundation: attack rounds for finished tasks in one working copy, one fresh attacker per round with three lenses, a fixer after a round with a high or medium finding, until two dry rounds in a row; one agent at a time',
  phases: [
    { title: 'Attack', detail: 'one fresh attacker per round, three lenses, the lead inbox', model: 'opus' },
    { title: 'Fix', detail: 'reproduce first, then fix', model: 'opus' },
  ],
}

// args: { scratch: '<session scratchpad>', clone: 'c' | 's', tasks: ['baseline-1c', 'workspace', 'fixtures', 'tooling', 'skeletons'] }
// The loops run in the order given. A later launch with more tasks at the END and resumeFromRunId replays what finished.
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
- Two things that damage this machine when run as root, as you are: never give /dev/null as an OUTPUT file to a compiler or linker (rustc or cc with -o /dev/null deletes the device and leaves a file), and never run \"git config --global\" (tests set GIT_CONFIG_GLOBAL=/dev/null). If \"test -c /dev/null\" fails, say so in your report.
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

const T2 = {
  id: 'workspace', brief: `${BRIEFS}/workspace.md`,
  gate: 'cargo test --locked --workspace --exclude task_deck && cargo clippy --locked --workspace --exclude task_deck --all-targets -- -D warnings && cargo check --locked -p taskdeck-core -p taskdeck-sync --target wasm32-unknown-unknown && bash scripts/supply-chain.sh && bash scripts/tests/run.sh && bash scripts/ci/reads.sh && cargo build --locked --no-default-features --bin taskdeck-server',
  others: 'Files of other tasks (scripts/, .github/, deny.toml, .cargo/, build.rs, src/, the specs, fixtures/) are not part of this task: do not attack them, only how this task fits them',
  lenses: [
    { key: 'plan-conformance', text: 'spec conformance, clause by clause: SYNC_PLAN.md section 6.1 item 2, section 3.2 (shared files), section 3.4 (pre-approved dependencies: the crates named in SYNC.md sections 2.3 and 5.1 at those versions, anything already in Cargo.lock, security-framework, webpki-roots, and for tests proptest, libfuzzer-sys, cargo-fuzz) and the brief point by point: is every declared dependency pre-approved and at its pinned version; compare Cargo.lock with origin/day-planner entry by entry (nothing that existed may have moved; every new entry at least seven days old on 2026-11-21, see bash scripts/cooldown.sh --help); no aws-lc-rs, aws-lc-sys, openssl, native-tls or cmake-built crate (cargo tree -i); no second version of a cryptographic crate (cargo tree -d); the root manifest\'s [package], [features], [dependencies] and profiles untouched; the root package still the only default member; each crate\'s root attribute, doc comment, trivial test and README as the brief says' },
    { key: 'builds-everywhere', text: 'feature sets and targets: each crate alone (-p) and the workspace without the root package, with default features, with --no-default-features and with each declared feature (features that exclude each other one at a time); wasm32-unknown-unknown for core and sync; the Android targets of rust-toolchain.toml for the crates without C code; "cargo tree --target" for a Windows, a macOS and an Android target, to see that platform-only dependencies resolve and that rustls uses the ring provider only on each; the clippy.toml of core and of sync really forbids what the brief lists (prove it with a scratch file in your copy); cargo doc without warnings; the same commands from a copy whose path holds a space' },
    { key: 'later-stages', text: 'what the stages of waves 1 to 3 will need: in the six specs under docs/sync/spec/ search (Grep, do not read a spec in full) for the clauses and "Interfaces" entries that name a crate, a crate feature or a cargo feature a stage must use ("dependencies MUST", "depends on", "feature", the crate names), and read SYNC_PLAN.md sections 6.2 to 6.10 for the same; list everything a later stage is told to use that is not declared here at the stated version or with the needed feature (a later stage may add only one dependency per pull request, so every gap costs a stage a change to a shared file); whether scripts/ci/stage.sh and scripts/ci/changes.sh (in its test mode, with paths under each new directory) treat the new directories as scripts/ci/README.md says; whether a crate that cannot build without a feature choice breaks a workspace-wide command; whether the READMEs tell a new stage lead where to start' },
  ],
}
const T5 = {
  id: 'fixtures', brief: `${BRIEFS}/fixtures.md`,
  gate: 'bash scripts/check-fixtures.sh && bash scripts/tests/run.sh && bash scripts/ci/reads.sh && bash scripts/supply-chain.sh && cargo test --locked -p <the generator package> (its name is in fixtures/generator/Cargo.toml) && cargo build --locked --no-default-features --bin taskdeck-server',
  others: 'Files of other tasks (crates/, scripts/ci/, the supply-chain scripts, .github/, src/, the specs) are not part of this task: do not attack them. Today\'s code under src/ is what the fixtures record: a quirk of today\'s output is a fact to record, not a defect of this task',
  lenses: [
    { key: 'deterministic', text: 'determinism across machines and over time: run the generator twice and compare byte for byte; run it under another locale, from another working directory, from a copy whose path holds a space, with TZ unset and with a wrong TZ (it must refuse); look for every read of the clock, the local zone, randomness, HashMap or HashSet iteration order, directory listing order, float formatting and the user name or home directory in the generator and on the paths of today\'s code it calls, and for each either show that it cannot reach a file or show the file it changes; check fixtures/MANIFEST.sha256 against the files; scripts/check-fixtures.sh must fail when a committed file is changed, missing or extra (mutate your copy, three small mutations)' },
    { key: 'plan-and-content', text: 'spec conformance, clause by clause: SYNC_PLAN.md section 6.1 item 5 and the brief point by point, counted in the data itself, not taken from the README: every board; each item kind and field, each archive verdict, the daylight-saving instants, each id boundary, each kind of unreadable line, the id that is both live and archived; the older main build\'s formats (how they were established, and that today\'s code opens each); the snapshot requests; transcripts that cover every command and every refusal (list the commands and refusals today\'s code has under src/ and check each occurs); the sizes; the README\'s promises. Made-up data only: search the whole of fixtures/ for anything that looks real (a person\'s name, an address, coordinates of a real home, a URL that resolves, an e-mail address, a token or key)' },
    { key: 'usable-without-old-code', text: 'what a later stage would do with this data, with no access to today\'s code: write a small python3 script (standard library only, in your scratch directory) that parses every transcript, snapshot and board file from the README\'s description alone and checks what can be checked without the old code (well-formed JSON Lines, each step has the stated fields, the state after a refused command equals the state before it, ids unique where the format says so, no NaN, no invalid UTF-8 except where the README says so); check the data answers what SYNC_PLAN.md sections 6.2, 6.6 and 6.9 say those stages test against (the 10,000-row board for the budgets, the legacy import with palette and home place, the 31-day snapshot, the bridge answering from fixtures); name everything that would make a later stage\'s test ambiguous or flaky (an undocumented normalisation, a field whose meaning the README does not give, an order that is not stated)' },
  ],
}
const T6 = {
  id: 'tooling', brief: `${BRIEFS}/tooling.md`,
  gate: 'bash scripts/tests/run.sh && bash scripts/spec-lint.sh --allow-empty-map && bash scripts/gate.sh --list foundation && bash scripts/ci/reads.sh',
  others: 'Files of other tasks (crates/, fixtures/, .github/, scripts/ci/, the supply-chain scripts and their tests, src/, the specs) are not part of this task: do not attack them, only how these tools read them',
  lenses: [
    { key: 'checks-can-fail', text: 'adversarial inputs: try to get past each tool what it exists to stop, on made-up trees in your scratch directory. spec-lint: a duplicate id, a clause inside a code block or a table, a MUST outside a clause, a dangling reference, a bad owner, closing sections out of order, a malformed row of the requirement map. spec-coverage: a citation in a file that is not a test, an id that is a prefix of another, a withdrawn clause, an id cited only in a skipped or commented-out test, a test file outside the stage\'s paths, a stage with no clause. The gate runner: a failing run item at any position must fail the gate; a malformed line, an empty gate file, an unknown stage; a command with quotes, pipes or a cd; an item whose command prints PASS and exits 1. The two scans: unimplemented!, todo! and skipped tests in odd spellings, across lines, and in paths with spaces. Report only what passes when it should fail, or fails when it should pass' },
    { key: 'plan-conformance', text: 'spec conformance, clause by clause: SYNC_PLAN.md section 6 (what "gate" means), sections 3.3 and 3.4, and each stage\'s gate text in sections 6.1 to 6.12 against scripts/gates/<stage>.gate item by item: does every bullet or clause of the plan\'s gate text have an item that quotes it faithfully, is anything weaker than the plan, do the commands name things that exist or that scripts/gates/README.md defines as a convention; scripts/dev-setup.sh: versions pinned, official sources, checksums where published, idempotent, --check truthful; the brief point by point' },
    { key: 'fresh-machine', text: 'a fresh container and the CI runners: scripts/dev-setup.sh for each stage with a fresh TASKDECK_TOOLS_DIR under your scratch directory (downloads are allowed; an unreachable host is a fact to report, not a finding), run twice, with --check before and after, and with the network cut (a dead proxy): it must fail clearly, never report success; the scripts under the bash 3.2 rules of macOS where CI runs them there; from a copy whose path holds a space; under another locale; scripts/gate.sh foundation in a fresh clone of your copy' },
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
const ALL = { 'baseline-1c': T1C, workspace: T2, fixtures: T5, tooling: T6, skeletons: T4 }
for (const id of (args.tasks || [])) {
  const t = ALL[id]
  if (!t) { log(`unknown task ${id}`); continue }
  phase('Attack')
  out[id] = await attackLoop(t)
  if (out[id].stopped) return out
}
return out
