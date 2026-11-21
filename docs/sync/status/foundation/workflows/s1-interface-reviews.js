export const meta = {
  name: 'foundation-s1-interface-reviews',
  description: 'Foundation, spec track: nine pairwise interface reviews of the six specs, then one resolver per pair, one after another (one agent at a time; the lead rules before any spec is edited)',
  phases: [
    { title: 'Interface reviews', detail: 'nine pairs, each reviewer reads both specs in full', model: 'opus' },
    { title: 'Resolve', detail: 'one resolver per pair, each building on the earlier ones', model: 'opus' },
  ],
}

// args: { scratch: '<session scratchpad>', width: 1 }
const S = args.scratch
const WIDTH = Math.max(1, args.width || 1)
const REPO = '/home/user/taskdeck'
const BRIEFS = `${REPO}/docs/sync/status/foundation/briefs`
const RES = `${S}/resolutions`
const ORDER = ['seal', 'core', 'relay', 'engine', 'device', 'bridge']
const PAIRS = [
  ['seal', 'relay'], ['seal', 'engine'], ['core', 'engine'], ['engine', 'relay'], ['device', 'engine'], ['device', 'bridge'],
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
    model_self_report: { type: 'string' },
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

// Run thunks with at most `width` in flight, in order. A thunk that throws gives null.
const pool = async (thunks, width) => {
  const results = new Array(thunks.length).fill(null)
  let next = 0
  const worker = async () => {
    while (next < thunks.length) {
      const i = next++
      try { results[i] = await thunks[i]() } catch (e) { results[i] = null }
    }
  }
  await Promise.all(Array.from({ length: Math.min(width, thunks.length) }, () => worker()))
  return results
}

const review = ([a, b]) => agent(`You review the INTERFACE between two specs of the TaskDeck sync build (repository ${REPO}): docs/sync/spec/${a}.md and docs/sync/spec/${b}.md. You wrote neither. You change no file in the repository and run no git command that changes state.

Read, in this order:
1. ${BRIEFS}/reviews.md, section A ("Pairwise interface reviewer"): it is your task description.
2. docs/sync/spec/README.md (the conventions every spec follows).
3. docs/sync/status/foundation.md: the sections "Decisions" and "Rulings for the specs" (every ruling S1 to S31; S15 has a settled form further down that replaces its first form). The rulings are binding on the specs.
4. Both specs in full. They are long: read them in ranges until you have read every line of both.
5. The parts of SYNC.md and SYNC_PLAN.md they cite, where you need the intent. Where the two disagree, SYNC_PLAN.md wins (its section 2 lists every difference).
The other four specs exist too (the six are seal, core, relay, engine, device, bridge): open them only to check a reference.

Besides what section A lists, report every place where either spec does not yet reflect a lead ruling that touches this pair (for example: the settled Skip floor S15; reinstatement carrying missed roots S24; the two snapshot triggers S25; the control history in the bootstrap snapshot S27; the backup file name S28; the 255-byte host cap; the recovery tool learning the read epoch from the relay's answer; the sealed file kinds device.md adds; the phone's own picture S29; unsafe only in platform FFI S26), with the exact change each spec needs.

Every finding cites both sides by section or clause id, says what breaks, and proposes the resolution: which spec changes, and to exactly what (the layout, field, name, width, value, error code or clause text). Write each finding so that someone who has not read your pair can act on it. One finding per distinct problem: merge repeats of the same mismatch into one. Do not report style. Order findings by severity.
Return the structured result. In specs_to_change use the bare names seal, core, relay, engine, device, bridge. In model_self_report give the exact model name or id your own system prompt says you are.`,
  { label: `review:${a}-${b}`, phase: 'Interface reviews', model: 'opus', schema: REVIEW })

phase('Interface reviews')
const reviews = await pool(PAIRS.map(p => () => review(p)), WIDTH)
const out = { reviewed: reviews.filter(Boolean).length }
out.per_pair = PAIRS.map((p, i) => ({ pair: p.join('-'), findings: reviews[i] ? reviews[i].findings.length : null, model: reviews[i] ? reviews[i].model_self_report : null }))
log(`interface reviews: ${out.reviewed}/${PAIRS.length} reported, ${reviews.filter(Boolean).reduce((n, r) => n + r.findings.length, 0)} findings`)
if (out.reviewed < PAIRS.length) {
  out.failed = PAIRS.filter((_, i) => !reviews[i]).map(p => p.join('-'))
  return out
}

const all = reviews.flatMap((r, i) => r.findings.map((f, k) => {
  const pair = PAIRS[i]
  return { id: `${pair[0]}-${pair[1]}-${k + 1}`, pair: pair.join('-'), ...f }
}))
out.all = all.map(f => ({ id: f.id, severity: f.severity, title: f.title, specs_to_change: f.specs_to_change }))

// One resolver per pair, one after another; each builds on what the earlier ones settled.
phase('Resolve')
out.resolutions = []
const written = []
for (const pair of PAIRS) {
  const [a, b] = pair
  const group = all.filter(f => f.pair === pair.join('-'))
  if (group.length === 0) { log(`resolve ${a}-${b}: no findings`); continue }
  const file = `${RES}/r1-${a}-${b}.md`
  const TAG = `R1-${a.toUpperCase()}-${b.toUpperCase()}`
  let r = null
  try {
    r = await agent(`You are a resolver for the interface reviews of the six TaskDeck sync specs (repository ${REPO}, docs/sync/spec/: seal.md, core.md, relay.md, engine.md, device.md, bridge.md). Nine reviewers each compared two specs. Nine resolvers, one per pair and one after another, turn the findings into exact changes; afterwards one fixer per spec applies them. You resolve the review of ${a}.md against ${b}.md: ${group.length} findings, given at the end.

Read first: ${BRIEFS}/reviews.md, section B ("Resolver": your task description); docs/sync/spec/README.md; docs/sync/status/foundation.md ("Decisions" and "Rulings for the specs", S1 to S31 with the settled form of S15: they are binding and settle several findings already); SYNC_PLAN.md sections 2 and 3. Then, for each finding, the sections of the specs it cites, with enough of each spec around them to be sure of the context: you will need large parts of ${a}.md and ${b}.md, including their Error catalogue, Limits, Interfaces and Public API sections wherever a change touches them.
${written.length ? `Resolvers before you have written: ${written.join(' , ')} . Read them in full before deciding anything: what they decided is settled, and the fixers will apply it. Where one of your findings touches something they decided (the same clause, layout, field, name or section), build on it: name the earlier change id, keep its names, layouts and wording, and give only what must be added or adjusted. If you think an earlier decision is wrong or conflicts with a finding of yours, do not override it: say so under "For the lead" with both options and your recommendation.` : 'You are the first resolver; eight more follow you and will treat what you decide as settled.'}

You change no spec and run no git command that changes state. For each finding decide: accept (with the exact change per spec), merge with another finding of your group, or reject (the reviewer misread: cite the text). Where two findings pull in different directions choose the resolution that (1) follows SYNC_PLAN.md, then SYNC.md, then the intent of the lead's rulings, (2) keeps one owner per rule, (3) changes the fewest bytes on the wire and the fewest clauses.

Write your resolution list to ${file} (create the directory if needed):
- one level-two heading per spec that must change, exactly "## seal.md", "## core.md", "## relay.md", "## engine.md", "## device.md" or "## bridge.md" (a finding may need a change in a spec outside your pair: give it under that spec's heading);
- under it, numbered changes, each headed "### ${TAG}-<n> (<the finding ids it resolves>)", giving the section to change and the exact new rule, layout, field, name, width, value, error code or clause text, so that the fixer designs nothing: clause ids to add (a new id continues after the highest existing number of its topic in that spec, and after any id an earlier resolver already assigned in that topic: check the spec and the earlier files; never reuse or renumber an id), clauses to reword (give the full new text), clauses to withdraw (README section 4.1), and every table row, error-catalogue row, limit, interface entry and public-API signature the change touches;
- each change is self-contained: the fixer of a spec reads the changes under its own heading and must not have to guess anything from the finding;
- a change that needs matching edits in two specs appears under both headings with identical wording for the shared part (the same field names, widths, order and values);
- then "## Rejected" (finding id and the reason) and "## For the lead" (anything that needs a ruling, each with your recommended answer; meanwhile do not leave the specs inconsistent: write the conservative reading as the change and say that you did).
Every finding id of your group must appear exactly once: in a change heading, as merged into one, or under Rejected.

The findings (JSON):
${JSON.stringify(group, null, 1)}

Return the structured summary; needs_lead_ruling repeats the "For the lead" items, one string each, with your recommendation.`,
      { label: `resolve:${a}-${b}`, phase: 'Resolve', model: 'opus', schema: RESOLUTION })
  } catch (e) { r = null }
  if (r === null) { out.resolver_failed = pair.join('-'); log(`resolve ${a}-${b}: the resolver failed; stopping so the lead can resume`); return out }
  written.push(file)
  out.resolutions.push({ pair: pair.join('-'), file, findings: group.length, accepted: r.accepted, merged: r.merged, rejected: r.rejected, changes_per_spec: r.changes_per_spec, needs_lead_ruling: r.needs_lead_ruling, summary: r.summary })
  log(`resolve ${a}-${b}: ${group.length} findings -> accepted ${r.accepted}, merged ${r.merged}, rejected ${r.rejected}; ${r.needs_lead_ruling.length} for the lead`)
}
return out
