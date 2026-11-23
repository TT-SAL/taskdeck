export const meta = {
  name: 'seal-task',
  description: 'Stage seal: implement one task, verify once, fix once, two security passes side by side, one fix',
  phases: [
    { title: 'Implement', model: 'opus' },
    { title: 'Verify', model: 'opus' },
    { title: 'Fix', model: 'opus' },
    { title: 'Security', model: 'opus' },
  ],
}
// args: { id, brief, lenses: [string, string], until: 'implement' | 'verify' | 'all' }
// `until` never enters a prompt, so a resume with another value replays the finished agents from the cache.
const W = '/home/user/taskdeck/target/work/seal'
const COMMON = `${W}/docs/sync/status/seal/briefs/common.md`
const BRIEF = `${W}/${args.brief}`
const ID = args.id
const OPTS = { model: 'opus', effort: 'high' }

const REPORT = {
  type: 'object',
  properties: {
    summary: { type: 'string' },
    gate_passed: { type: 'boolean' },
    gate_tail: { type: 'string' },
    files: { type: 'array', items: { type: 'string' } },
    not_done: { type: 'array', items: { type: 'string' } },
    spec_questions: { type: 'array', items: { type: 'object', properties: {
      section: { type: 'string' }, question: { type: 'string' }, reading_taken: { type: 'string' } },
      required: ['section', 'question', 'reading_taken'] } },
    caller_clauses: { type: 'array', items: { type: 'string' } },
    notes: { type: 'string' },
  },
  required: ['summary', 'gate_passed', 'files', 'not_done', 'spec_questions', 'caller_clauses'],
}
const FINDINGS = {
  type: 'object',
  properties: {
    findings: { type: 'array', items: {
      type: 'object',
      properties: { title: { type: 'string' }, severity: { type: 'string', enum: ['high', 'medium', 'low'] },
                    clause: { type: 'string' }, where: { type: 'string' }, repro: { type: 'string' } },
      required: ['title', 'severity', 'where', 'repro'],
    } },
    checked: { type: 'string' },
    not_checked: { type: 'string' },
  },
  required: ['findings', 'checked', 'not_checked'],
}
const FIXED = {
  type: 'object',
  properties: {
    fixed: { type: 'array', items: { type: 'object', properties: { title: { type: 'string' }, test: { type: 'string' } }, required: ['title', 'test'] } },
    dropped: { type: 'array', items: { type: 'object', properties: { title: { type: 'string' }, why: { type: 'string' } }, required: ['title', 'why'] } },
    gate_passed: { type: 'boolean' },
    notes: { type: 'string' },
  },
  required: ['fixed', 'dropped', 'gate_passed'],
}

const FIRST = `First read, as one batch of parallel reads: ${COMMON}, ${BRIEF}, and what the brief lists under "Read".`

const implPrompt = `You are the implementer of task ${ID} of stage seal (the crate taskdeck-seal). ${FIRST} ` +
  `Then build what the brief says, with the tests that cite its clauses and the vectors it names. ` +
  `Done means the task gate of common.md exits 0, run by you from ${W} with CARGO_TARGET_DIR=target/impl. ` +
  `Never weaken or skip a test; delete one only together with the code it tests. ` +
  `Your report: summary (what exists now, in a few lines); gate_passed and gate_tail (the last lines of the gate's output as you ran it); ` +
  `files (every file you created or changed); not_done (anything the brief asks that is not there, plainly); ` +
  `spec_questions (each place the spec was silent, contradictory or not implementable, with the reading you took); ` +
  `caller_clauses (clauses that state a duty of the crate's caller and got no test); notes (what the lead should look at first).`

const verifyPrompt = (n) => `You are the verifier of task ${ID} of stage seal (pass ${n}): you did not write it. ${FIRST} ` +
  `The section "Reviewers" of common.md binds you. Then read the task's code and tests in ${W} ` +
  `(\`git -C ${W} status --short\` and \`git -C ${W} diff\` show what the task changed). ` +
  `Check the work once against its brief and the clauses it names: is everything the brief asks built; does the code do what each clause says ` +
  `(layouts byte for byte, context strings, the order of checks, error codes, limits, draw order and amounts of entropy); ` +
  `does each test prove the clauses it cites (a citation claims the test fails when the clause is violated: name hollow ones); ` +
  `are the published vectors right (recompute a sample independently, from the spec text); are the known-answer data what their sources publish. ` +
  `Run the task gate yourself with CARGO_TARGET_DIR=target/verify. Scratch tests are named zz_verify_<n>.rs. ` +
  `Report only what you reproduced, each finding with its severity by common.md's scale, the clause, where (file and line), and repro ` +
  `(the failing test or command as text, with its output). Also say in checked what you covered and in not_checked what you did not.`

const secPrompt = (n, lens) => `You are security reviewer ${n} of task ${ID} of stage seal: you did not write it. Try to break it. ` +
  `Your lens: ${lens}. ${FIRST} The section "Reviewers" of common.md binds you. Then read the task's code in ${W} ` +
  `(\`git -C ${W} status --short\` and \`git -C ${W} diff\` show what the task changed). ` +
  `Think as a malicious relay that chooses every byte a device receives and their order, as a revoked device that kept its old keys, ` +
  `as a compromised member, and as someone typing hostile text. Look for: checks in the wrong order; parsing before authentication; ` +
  `missing length or range checks; allocation sized by an attacker; panics (indexing, slicing, unwrap, overflow); comparisons of ` +
  `secret-derived values that are not constant-time; secrets not zeroized or leaking through Debug, Display or errors; a context string ` +
  `missing, reused or shared between uses; a value made for one use accepted for another; a nonce or key reused; entropy drawn in another ` +
  `order or amount than the spec states; any formula that differs from the spec's. ` +
  `Use CARGO_TARGET_DIR=target/sec${n}; scratch tests are named zz_sec${n}_<n>.rs. ` +
  `Report only what you reproduced, each finding with its severity by common.md's scale, the clause, where (file and line), and repro ` +
  `(the failing test or command as text, with its output). Also say in checked what you covered and in not_checked what you did not.`

const fixPrompt = (findings) => `You are the fixer for task ${ID} of stage seal. ${FIRST} ` +
  `Below are findings by reviewers who did not write the code. For each one: first add its reproduction as a test ` +
  `(in the task's test files, citing its clause) and run it; name and drop any that does not fail. Then fix the rest, ` +
  `the cause and not the symptom, and change nothing a finding does not require. Two findings may describe one defect. ` +
  `Keep the task gate of common.md green, run by you from ${W} with CARGO_TARGET_DIR=target/impl. ` +
  `Report fixed (each with the test that now passes), dropped (each with why), gate_passed, notes.\n` +
  JSON.stringify(findings, null, 2)

const out = { id: ID, done: false, lows: [], fixes: [] }
const serious = (r) => r.findings.filter(f => f.severity !== 'low')
const keepLows = (r) => out.lows.push(...r.findings.filter(f => f.severity === 'low'))

phase('Implement')
const built = await agent(implPrompt, { label: `impl:${ID}`, phase: 'Implement', schema: REPORT, ...OPTS })
if (built === null) return { ...out, failed: 'implement' }
out.built = built
if (args.until === 'implement') return { ...out, stopped: 'implement' }

phase('Verify')
const v1 = await agent(verifyPrompt(1), { label: `verify:${ID}`, phase: 'Verify', schema: FINDINGS, ...OPTS })
if (v1 === null) return { ...out, failed: 'verify' }      // a failed agent never counts as a clean result
out.verify = v1
keepLows(v1)
if (serious(v1).length > 0) {
  const f = await agent(fixPrompt(serious(v1)), { label: `fix:verify:${ID}`, phase: 'Fix', schema: FIXED, ...OPTS })
  if (f === null) return { ...out, failed: 'fix:verify' }
  out.fixes.push({ after: 'verify', ...f })
}
if (v1.findings.some(f => f.severity === 'high')) {         // a second verify pass only after a high finding
  const v2 = await agent(verifyPrompt(2), { label: `verify-2:${ID}`, phase: 'Verify', schema: FINDINGS, ...OPTS })
  if (v2 === null) return { ...out, failed: 'verify-2' }
  out.verify2 = v2
  keepLows(v2)
  if (serious(v2).length > 0) {
    const f = await agent(fixPrompt(serious(v2)), { label: `fix:verify-2:${ID}`, phase: 'Fix', schema: FIXED, ...OPTS })
    if (f === null) return { ...out, failed: 'fix:verify-2' }
    out.fixes.push({ after: 'verify-2', ...f })
  }
}
if (args.until === 'verify') return { ...out, stopped: 'verify' }

phase('Security')
const passes = await parallel(args.lenses.map((lens, i) => () =>
  agent(secPrompt(i + 1, lens), { label: `security-${i + 1}:${ID}`, phase: 'Security', schema: FINDINGS, ...OPTS })))
if (passes.some(p => p === null)) return { ...out, failed: 'security' }
out.security = passes
passes.forEach(keepLows)
const broke = passes.flatMap((p, i) => serious(p).map(f => ({ ...f, from: `security-${i + 1}` })))
if (broke.length > 0) {
  const f = await agent(fixPrompt(broke), { label: `fix:security:${ID}`, phase: 'Fix', schema: FIXED, ...OPTS })
  if (f === null) return { ...out, failed: 'fix:security' }
  out.fixes.push({ after: 'security', ...f })
}
return { ...out, done: true }
