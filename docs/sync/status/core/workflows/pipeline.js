export const meta = {
  name: 'core-pipeline',
  description: 'Stage core: one writer at a time in a working copy; each task checked once on a frozen copy and fixed once; a security pass where a task parses untrusted input',
  phases: [
    { title: 'Implement', model: 'opus' },
    { title: 'Verify', model: 'opus' },
    { title: 'Fix', model: 'opus' },
    { title: 'Security', model: 'opus' },
  ],
}
// args: { root, work, briefs, gate, steps: [ { id, kind: 'impl' | 'verify' | 'security', after: [ids],
//         brief (impl) | briefs (checkers), focus (security), extra, effort } ] }
// A step runs once every step it names in `after` has finished. Writers (implementers and fixers) run one
// at a time in args.work. A checker works on a frozen copy of the commit its last `after` step ended on.
const A = args
const B = `${A.root}/${A.briefs}`
const WRITER = {
  type: 'object',
  properties: {
    commit: { type: 'string' }, summary: { type: 'string' }, gate_tail: { type: 'string' },
    for_lead: { type: 'array', items: { type: 'string' } },
  },
  required: ['commit', 'summary', 'for_lead'],
}
const FINDINGS = {
  type: 'object',
  properties: {
    findings: { type: 'array', items: {
      type: 'object',
      properties: { title: { type: 'string' }, severity: { type: 'string', enum: ['high', 'medium', 'low'] },
                    clause: { type: 'string' }, repro: { type: 'string' } },
      required: ['title', 'severity', 'repro'],
    } },
    checked: { type: 'string' },
    unreproduced: { type: 'array', items: { type: 'string' } },
    for_lead: { type: 'array', items: { type: 'string' } },
  },
  required: ['findings', 'checked'],
}
const FIXED = {
  type: 'object',
  properties: {
    commit: { type: 'string' }, fixed: { type: 'array', items: { type: 'string' } },
    dropped: { type: 'array', items: { type: 'string' } }, for_lead: { type: 'array', items: { type: 'string' } },
  },
  required: ['commit', 'fixed', 'dropped'],
}
const slug = s => s.replace(/[^a-zA-Z0-9]+/g, '-')

// One writer at a time; a fix goes before a waiting implementer.
const waiting = []
let busy = false, order = 0
const pump = () => {
  if (busy || waiting.length === 0) return
  waiting.sort((a, b) => a.prio - b.prio || a.n - b.n)
  const w = waiting.shift()
  busy = true
  w.fn().then(r => r, () => null).then(r => { busy = false; w.resolve(r); pump() })
}
const asWriter = (prio, fn) => new Promise(resolve => { waiting.push({ prio, n: order++, fn, resolve }); pump() })

const implPrompt = s =>
  `You are the implementer of step ${s.id} in stage \`core\` of the TaskDeck sync build.\n` +
  `Read these two files first, both in one message, then do what the second says:\n` +
  `  ${B}/common.md\n  ${B}/${s.brief}\n` +
  `You are a writer: your working copy is ${A.work}, and nobody else writes there while you run. ` +
  `Change only crates/taskdeck-core/**. Write the tests that cite the clauses you implement.\n` +
  `Done means the step gate exits 0 in the working copy: \`${A.gate}\`, and you have committed there. ` +
  `Never weaken or skip a test; delete one only together with the code it tests.\n` +
  (s.extra ? s.extra + '\n' : '') +
  `Return the commit hash, a summary of what you built (files, behaviour, how many tests), the last lines of ` +
  `the gate's output, and under for_lead every spec question and every change you need outside the crate.`

const briefList = s => (s.briefs || []).map(b => `  ${B}/${b}`).join('\n')
const checkPrompt = (s, commit, label, second) => {
  const head =
    `You did not write this work. ` + (s.kind === 'security'
      ? `Try to break it: ${s.focus}\n`
      : `Check it once${second ? ' more: the first check found a high-severity problem, which has been fixed. Look at the fix and at what the first check may have missed' : ''} against its brief and the spec clauses the brief names.\n`) +
    `Read first, all in one message: ${B}/common.md and the brief${(s.briefs || []).length > 1 ? 's' : ''} of the work:\n${briefList(s)}\n` +
    `The work is commit ${commit} of the working copy ${A.work}. Make your frozen copy as common.md says ` +
    `(label: ${slug(label)}) and work only there. If that commit does not exist, stop and report that as one high finding.\n`
  const verify =
    `What to do:\n` +
    `1. Run the step gate in your copy: \`${A.gate}\`. A failure is a high finding.\n` +
    `2. Go through the brief: every deliverable, every point the lead settled, every clause it lists. Is each ` +
    `implemented as the spec's text says, and does a test really prove it? Read the spec lines the brief names ` +
    `for the parts you check; never the whole spec.\n` +
    `3. Probe it yourself: write your own small tests from the spec's text (boundary values, the order of ` +
    `error codes, the examples the spec gives) in your copy and run them. Spend your effort where a mistake ` +
    `would make two devices disagree, lose data, or accept what the spec refuses.\n` +
    `4. Check a sample of the vectors' expected values against the spec by hand: a vector that records the ` +
    `implementation's own mistake is a finding.\n`
  const security =
    `Assume the attacker controls every byte of that input. Look for: a panic; time or memory spent before a ` +
    `limit is checked, or growing faster than the input; input accepted that the spec says to refuse ` +
    `(non-canonical bytes, duplicate keys, values out of range, a limit bypassed); integer overflow; anything ` +
    `that makes two devices read the same bytes differently. Use property tests and hand-made inputs in your copy.\n`
  const tail =
    `Report only what you reproduced: each finding with its severity (common.md), the clause, and the ` +
    `reproduction as text that fails on the commit you checked: a test to paste, with the file it goes in, or a ` +
    `command. What you believe but could not reproduce goes under unreproduced, one line each; it is not a ` +
    `finding. Spec questions go under for_lead. Say under checked what you covered and what you did not. ` +
    (s.extra ? s.extra : '')
  return head + (s.kind === 'security' ? security : verify) + tail
}
const fixPrompt = (s, serious, commit, label) =>
  `You are the fixer for ${label} in stage \`core\` of the TaskDeck sync build. Read ${B}/common.md first` +
  ((s.briefs || []).length ? `; the brief${s.briefs.length > 1 ? 's' : ''} of the work, for reference:\n${briefList(s)}\n` : '.\n') +
  `A checker who did not write the work reported the findings below against commit ${commit} of the working ` +
  `copy ${A.work}; the working copy may have moved on since. You are a writer: work in the working copy.\n` +
  `First add each reproduction below as a test and run it. Name and drop any that does not fail, and say why. ` +
  `Then fix the rest, keeping the step gate green: \`${A.gate}\`. Fix the cause, not the test. A fix that needs ` +
  `a change outside crates/taskdeck-core, or a ruling on the spec, is not yours to make: list it under for_lead. ` +
  `Commit as common.md says and return the commit hash, what you fixed, what you dropped and why.\n` +
  JSON.stringify(serious, null, 2)

const results = {}
const lows = []
const forLead = []
const note = (id, list) => { for (const x of list || []) forLead.push(`${id}: ${x}`) }

const check = async (s, commit) => {
  const phase = s.kind === 'security' ? 'Security' : 'Verify'
  const rec = { kind: s.kind, checked_commit: commit, passes: [] }
  results[s.id] = rec
  let at = commit
  for (let n = 1; n <= 2; n++) {
    const label = n === 1 ? s.id : `${s.id}-2`
    const r = await agent(checkPrompt(s, at, label, n === 2),
      { label, phase, model: 'opus', effort: s.effort || 'high', schema: FINDINGS })
    if (r === null) { rec.ok = false; rec.why = `${label} failed`; return { ok: false } }   // never a clean result
    note(label, r.for_lead)
    const low = r.findings.filter(f => f.severity === 'low')
    const serious = r.findings.filter(f => f.severity !== 'low')
    lows.push(...low.map(f => ({ step: label, title: f.title, clause: f.clause, repro: f.repro })))
    const pass = { label, checked: r.checked, serious, low: low.length, unreproduced: r.unreproduced || [] }
    rec.passes.push(pass)
    if (serious.length === 0) break
    const f = await asWriter(0, () => agent(fixPrompt(s, serious, at, label),
      { label: `fix:${label}`, phase: 'Fix', model: 'opus', effort: 'medium', schema: FIXED }))
    if (f === null) { rec.ok = false; rec.why = `fix:${label} failed`; return { ok: false } }
    note(`fix:${label}`, f.for_lead)
    pass.fix = f
    at = f.commit
    // A second pass only for a verify whose first pass found something of high severity; never a third.
    if (!(s.kind === 'verify' && n === 1 && serious.some(x => x.severity === 'high'))) break
  }
  rec.ok = true
  rec.commit = at
  return { ok: true, commit: at }
}

const done = {}
const run = async s => {
  const deps = await Promise.all((s.after || []).map(id => done[id] || Promise.resolve({ ok: false })))
  if (deps.some(d => !d.ok)) { results[s.id] = { kind: s.kind, ok: false, why: 'a step it follows did not finish' }; return { ok: false } }
  if (s.kind === 'impl') {
    const r = await asWriter(1, () => agent(implPrompt(s),
      { label: s.id, phase: 'Implement', model: 'opus', effort: s.effort || 'high', schema: WRITER }))
    if (r === null) { results[s.id] = { kind: 'impl', ok: false, why: 'the agent failed' }; return { ok: false } }
    note(s.id, r.for_lead)
    results[s.id] = { kind: 'impl', ok: true, commit: r.commit, summary: r.summary, gate_tail: r.gate_tail }
    log(`${s.id} done at ${r.commit}`)
    return { ok: true, commit: r.commit }
  }
  const out = await check(s, deps[deps.length - 1].commit)
  log(`${s.id} ${out.ok ? 'done' : 'did not finish'}`)
  return out
}
for (const s of A.steps) done[s.id] = run(s)
await Promise.all(Object.values(done))
return { steps: results, lows, for_lead: forLead }
