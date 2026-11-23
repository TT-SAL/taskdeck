export const meta = {
  name: 'seal-exit',
  description: 'Stage seal, once per stage (SYNC_PLAN.md section 7): the hygiene pass and the exit review, each by one agent that did no work in the stage, read-only',
  phases: [{ title: 'Exit', detail: 'two read-only agents side by side: hygiene; the exit review', model: 'opus' }],
}
// args: { base: '<the base branch's commit at the stage's start>', only: 'hygiene' | 'review' | undefined }
const W = '/home/user/taskdeck/target/work/seal'
const BASE = args.base
const OPTS = { model: 'opus', effort: 'high' }
const PATHS = 'crates/taskdeck-seal docs/sync/vectors docs/sync/status/seal.md docs/sync/status/seal'
const FINDINGS = { type: 'object', properties: {
    summary: { type: 'string' },
    findings: { type: 'array', items: { type: 'object', properties: {
      title: { type: 'string' }, severity: { type: 'string', enum: ['high', 'medium', 'low'] },
      where: { type: 'string' }, evidence: { type: 'string' }, proposal: { type: 'string' } },
      required: ['title', 'severity', 'where', 'evidence', 'proposal'] } },
    checked: { type: 'string' }, not_checked: { type: 'string' } },
  required: ['summary', 'findings', 'checked', 'not_checked'] }

const FRAME = `You did no work in stage "seal" of the TaskDeck sync build, and you change nothing: this is a read-only task. ` +
  `The working copy is ${W}, at the stage's last commit (start every shell command with \`cd ${W} &&\`). Never run a git command that changes state, ` +
  `and write no file in the repository; scratch files go under ${W}/target/exit/ (git-ignored). ` +
  `The stage built the crate crates/taskdeck-seal (the cryptography and the membership state machine of an end-to-end encrypted log on a blind relay), ` +
  `published test vectors under docs/sync/vectors/ (the files seal-*.json and .gitattributes), keeps its memory in docs/sync/status/seal.md and docs/sync/status/seal/, ` +
  `and changed the specs where they were wrong or silent (docs/sync/spec/seal.md; single lines of device.md and engine.md; the rows of CHANGES.md that name stage seal). ` +
  `The stage started from commit ${BASE} of the base branch: \`git diff --stat ${BASE} HEAD -- ${PATHS}\` lists what it added (about 190 files; do not print whole diffs of large files), ` +
  `\`git log --format='%h %s' ${BASE}..HEAD -- Cargo.lock Cargo.toml .github scripts\` shows who touched the shared files. ` +
  `Another stage, "core", worked on the same base branch in the same hours: crates/taskdeck-core, docs/sync/vectors/core-*, docs/sync/spec/core.md and docs/sync/status/core* are not this stage's and are not yours to judge. ` +
  `Keep the work cheap: every message re-reads all you have read, so issue the reads and commands you can foresee together as parallel calls, never read a spec or a large source file in full (grep, then read the lines), and aim to finish within about 60 tool calls. `

const hygienePrompt = `You make the one hygiene pass of stage "seal" (SYNC_PLAN.md section 7: "One hygiene pass (secrets, anything from a real board, stray files, dependencies §3.4 does not approve)"). ${FRAME}` +
  `This repository is public. First read ${W}/SYNC_PLAN.md lines 90 to 121 (sections 3.2 to 3.5). Then look, over everything the stage added, for:\n` +
  `1. SECRETS AND IDENTIFYING DATA. A key, token or password that is not a test value; a relay address or host name that is not a reserved example name (example.com, example.org, example.net, names ending in .example, .test or .invalid, localhost); ` +
  `an e-mail address, an account or user name, a person's name, an IP address, a link to a private page. Keys in the vectors and tests are fine when they are test values: published by an RFC or by Wycheproof, or produced by the crate's tests from fixed seeds ` +
  `(the vectors are regenerated and compared by crates/taskdeck-seal/tests/vectors; check on a sample that a key-like value in a vector comes from a seed the generating test states). ` +
  `Look as well at the status file, the briefs under docs/sync/status/seal/ and the code comments.\n` +
  `2. ANYTHING FROM A REAL BOARD. Task titles, notes, names, places or dates of a real person's planner in vectors, tests, the budget example, the fuzz targets or the briefs: what the tests put into payloads, device names, host names and \`meta\` must be synthetic.\n` +
  `3. STRAY FILES. Scratch tests (zz_*), logs, backup copies (*.bak, *.orig), editor or system files, binaries, generated output committed by mistake, empty files; a data file that no test or build reads (for each file under crates/taskdeck-seal/tests/kat/data and each docs/sync/vectors/seal-*.json: grep for its name); ` +
  `in crates/taskdeck-seal/src: dbg!, println! or eprintln!, FIXME/TODO/XXX, blocks of commented-out code, and every #[allow(...)] or #![allow(...)] (say for each whether a comment gives its reason); a file the stage's commits changed outside its paths other than the root Cargo.lock.\n` +
  `4. DEPENDENCIES. Every dependency of crates/taskdeck-seal/Cargo.toml ([dependencies], [dev-dependencies], its features) and of crates/taskdeck-seal/fuzz/Cargo.toml against section 3.4 of the plan (pre-approved: the crates SYNC.md sections 2.3 and 5.1 name, at those versions (grep SYNC.md for them); anything that was already in Cargo.lock at ${BASE}; for tests and tooling proptest and libfuzzer-sys): ` +
  `is each one approved, pinned to an exact version, and free of a default feature that brings in what the crate must not have (the crate takes its entropy from its caller, and the one entropy crate it may hold is getrandom, for `OsEntropy`, which SEAL-GEN-005 requires: no rand, no clock, no file or network crate in the non-test dependency tree; \`cargo tree --locked -p taskdeck-seal -e normal\` needs no build, use CARGO_TARGET_DIR=target/exit)? ` +
  `What the stage's own lockfile change added or changed (\`git log --format='%h %s' ${BASE}..HEAD -- Cargo.lock\`, then \`git show <its commit> -- Cargo.lock\`). ` +
  `The fuzz crate has its own lockfile, crates/taskdeck-seal/fuzz/Cargo.lock: does every package in it (name, version and checksum together) stand in the root Cargo.lock, the package taskdeck-seal-fuzz excepted? ` +
  `Third-party test data under crates/taskdeck-seal/tests/kat: does each file or its README name its source and its license?\n` +
  `SEVERITY. High: a secret, a real address, token or account name, data of a real board, or a dependency section 3.4 does not approve. Medium: a stray file that ships or misleads, third-party data with no source or license named, a dependency that is not pinned. Low: everything else. ` +
  `Report only what you have seen yourself: each finding with where (path and line), the evidence (the command and its output, or the line), and what would put it right. In summary say in three lines what the pass found; in checked what you covered and with which commands; in not_checked what you did not.`

const reviewPrompt = `You make the exit review of stage "seal" (SYNC_PLAN.md section 7: "Stage exit: one agent that did no work in the stage reads the status file, the diff and the spec's clause index, and lists what is missing. What it finds of high or medium severity is the next work; the rest is listed."). ${FRAME}` +
  `Read, in this order: ${W}/SYNC_PLAN.md lines 80 to 121 (the rules of section 3), lines 217 to 232 (section 6.3: what stage seal builds, its gate, its security passes) and lines 354 to 372 with the "Stage exit" line near 433 (section 7); ` +
  `${W}/scripts/gates/seal.gate; then ${W}/docs/sync/status/seal.md in full (it is long: read it in three or four ranges). ` +
  `Every clause of the six specs is one line in ${W}/target/clause-index.txt (\`grep -c '(seal' \` and \`grep 'SEAL-' \` it; never read it whole): 164 clauses name seal as an owner. ` +
  `The public API the crate must have is section 26 of docs/sync/spec/seal.md (\`grep -n '^## 26' \` and read from there).\n` +
  `Then check the claims against the tree, and list what is missing:\n` +
  `1. SECTION 6.3. Each thing it says the stage builds (frames and signatures with per-use contexts; the envelope with padding; the recovery seed and its paper encoding; epoch roots, derivation and HPKE wraps; the membership state machine for every control entry, with races, the recovery key's powers and the recovery-key change; the hash chain and based_on checks; both roles of pairing; login and admin signatures; sealed local files; the backup envelope), the vectors, the fuzz targets: find it in the tree (the module, its public items against section 26, its vector family, its fuzz target) and say what is absent or does less than the plan's or the spec's words. ` +
  `Each line of its gate: which test target or script proves it, and is the proof what the line says (read the comment tables at the top of crates/taskdeck-seal/tests/tamper/main.rs, tests/contexts/main.rs and tests/parsers/main.rs against the spec's list of sealed, signed and parsed objects; the relay-in-the-middle test; the budget example and crates/taskdeck-seal/budget-baseline.txt against section 3.3's rule for budgets).\n` +
  `2. THE CLAUSES. Run \`bash scripts/spec-coverage.sh seal\` (cheap). Then take a sample of at least 25 seal-owned clauses spread over the spec's sections, among them the MUST NOTs and the ones about order of checks, and for each read the test that cites it: would the test fail if the clause were violated? Name every citation that proves less than its clause. ` +
  `For the clauses seal owns together with another stage (the status file and section 25 of seal.md say which half is the caller's): is each caller's half written down where the next stage will find it?\n` +
  `3. THE STATUS FILE. Could a fresh session, or the lead of stage engine, device, recovery, relay, desktop, android or release, work from it alone? Check "What the next stages need to know" against the rulings table (a ruling that changes what another stage must build or may rely on, and is not in that section), against the rows of docs/sync/spec/CHANGES.md that name seal (an open point there that the section does not mention), and against the code where a sentence is checkable (sample ten sentences: is each true of the tree as it stands?). ` +
  `Is every decision given with its reason, every low finding listed, every measurement and the budget baseline recorded, the open question for the owner stated so that the owner can decide? Is anything in the file false or stale (a task state, a number, a sentence that an earlier ruling made true and a later one changed)?\n` +
  `4. THE SPEC CHANGES. For each clause the stage changed (the "Clauses changed" column of its rows in CHANGES.md): does the clause text in the spec now say what the row says, and do the vectors and the code agree with it (sample five)? Is there a change to a clause's text in \`git diff ${BASE} HEAD -- docs/sync/spec/seal.md\` that no row names (compare the lines that begin "- **SEAL-" in that diff with the rows)?\n` +
  `5. THE RULES OF SECTION 3. Anything in the stage's diff that breaks one: a file changed outside the stage's paths (\`git log --format='%h %s' ${BASE}..HEAD -- scripts .github Cargo.toml\`), a new checker or scanner script, a gate weakened, a skipped or ignored test, an unimplemented body, unsafe code in the library, a test deleted without its code.\n` +
  `6. THE EXIT. Section 6.3's exit needs the stage gate green: run \`CARGO_TARGET_DIR=target/exit bash scripts/gate.sh seal\` once yourself (about ten minutes cold; nothing else is building) and give its summary lines; if a timing step fails while something else is running in the container, run that step once more alone before you report it. Do not run anything longer.\n` +
  `Not style and not preferences; not the low findings and open points the status file already records with their reasons (say so if a recorded reason does not hold); and not the design question O1, which is the owner's. ` +
  `SEVERITY. High: something section 6.3 or its gate requires is missing or is not what it claims, a MUST of a clause seal owns is not implemented, or a rule of section 3 is broken. Medium: a clause whose only citation does not prove it; the status file says something false, or leaves out something a later stage needs for its work; a spec change without its row. Low: everything else. ` +
  `Report only what you have checked against the tree: each finding with where (path and line), the evidence (command and output, or the lines), and what would put it right. In summary say in five lines whether the stage can exit and what stands in the way; in checked what you covered; in not_checked what you did not.`

phase('Exit')
const jobs = []
if (args.only !== 'review') jobs.push(() => agent(hygienePrompt, { label: 'hygiene', phase: 'Exit', schema: FINDINGS, ...OPTS }))
if (args.only !== 'hygiene') jobs.push(() => agent(reviewPrompt, { label: 'exit-review', phase: 'Exit', schema: FINDINGS, ...OPTS }))
const res = await parallel(jobs)
return { results: res, failed: res.some(r => r === null) }
