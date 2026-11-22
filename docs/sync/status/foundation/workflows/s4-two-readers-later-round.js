export const meta = {
  name: 'foundation-s4-two-readers-later-round',
  description: 'Foundation, spec track: a later round of the two-readers test (SYNC_PLAN.md 6.1): two readers who see only the specs each write all six scenarios, a comparer per scenario group writes the exact spec change for each difference, then one fixer per spec; one agent at a time',
  phases: [
    { title: 'Follow-ups', detail: 'only when asked for: open follow-ups as exact changes', model: 'opus' },
    { title: 'Read', detail: 'two readers, alone, from the specs only, six scenarios each', model: 'opus' },
    { title: 'Compare', detail: 'one comparer per group: differences and exact changes', model: 'opus' },
    { title: 'Spec fixes', detail: 'one fixer per spec that has changes', model: 'opus' },
  ],
}

// args: { scratch: '<session scratchpad>', round: 2, until: 'read' | 'compare' | 'fix', followups: '' | '<which items of r1-followups.md, and hints>', no_readers: false }
// 'until' names the last phase this launch runs; a later launch with resumeFromRunId replays the finished agents
// from the cache and goes on, so the lead can check the usage window between the phases.
const S = args.scratch
const ROUND = args.round
const UNTIL = args.until || 'fix'
const MAIN = '/home/user/taskdeck'
const WORK = `${MAIN}/target/work/s`
const BRIEF = `${MAIN}/docs/sync/status/foundation/briefs/two-readers.md`
const SPECS = `${MAIN}/docs/sync/spec`
const TR = `${S}/two-readers/r${ROUND}`
const CHECKTOOL = `python3 ${MAIN}/docs/sync/status/foundation/tools/speccheck.py ${WORK}/docs/sync/spec 300`
const ORDER = ['seal', 'core', 'relay', 'engine', 'device', 'bridge', 'README']
const GROUPS = [
  { key: 'edit', scenarios: '1 (first edit) and 2 (offline edit replayed)' },
  { key: 'join', scenarios: '3 (pairing) and 6 (snapshot bootstrap)' },
  { key: 'trust', scenarios: '4 (revocation with rotation) and 5 (relay rollback)' },
]
const safe = async (thunk) => { try { return await thunk() } catch (e) { return null } }
const out = { round: ROUND, readers: {}, groups: {}, fixes: {} }

const CHANGES = {
  type: 'object',
  properties: {
    summary: { type: 'string' },
    changes_per_spec: { type: 'array', items: { type: 'object', properties: { spec: { type: 'string' }, changes: { type: 'number' } }, required: ['spec', 'changes'] } },
    needs_lead_ruling: { type: 'array', items: { type: 'string' } },
  },
  required: ['summary', 'changes_per_spec', 'needs_lead_ruling'],
}
const READ = {
  type: 'object',
  properties: {
    files: { type: 'array', items: { type: 'string' } },
    steps: { type: 'array', items: { type: 'object', properties: { scenario: { type: 'number' }, steps: { type: 'number' }, ambiguous: { type: 'number' } }, required: ['scenario', 'steps', 'ambiguous'] } },
    specs_read: { type: 'array', items: { type: 'string' } },
  },
  required: ['files', 'steps', 'specs_read'],
}
const DIFF = {
  type: 'object',
  properties: {
    summary: { type: 'string' },
    spec_differences: { type: 'number' },
    reader_errors_clarified: { type: 'number' },
    reader_errors_plain: { type: 'number' },
    format_only: { type: 'number' },
    both_wrong: { type: 'number' },
    titles: { type: 'array', items: { type: 'string' } },
    changes_per_spec: { type: 'array', items: { type: 'object', properties: { spec: { type: 'string' }, changes: { type: 'number' } }, required: ['spec', 'changes'] } },
    needs_lead_ruling: { type: 'array', items: { type: 'string' } },
  },
  required: ['summary', 'spec_differences', 'reader_errors_clarified', 'reader_errors_plain', 'format_only', 'both_wrong', 'titles', 'changes_per_spec', 'needs_lead_ruling'],
}
const FIX = {
  type: 'object',
  properties: {
    summary: { type: 'string' },
    applied: { type: 'array', items: { type: 'string' } },
    not_applied: { type: 'array', items: { type: 'object', properties: { id: { type: 'string' }, reason: { type: 'string' } }, required: ['id', 'reason'] } },
    format_check: { type: 'string' },
    questions_for_lead: { type: 'array', items: { type: 'string' } },
  },
  required: ['summary', 'applied', 'not_applied', 'format_check', 'questions_for_lead'],
}
const RESOLVER_RULES = `The rules for writing a change: name the section; give the full new text (a clause is one list item "- **SPEC-TOPIC-NNN** (owner stages) text with MUST or MUST NOT"; a new id continues after the highest number of its topic in that spec; never reuse or renumber an id; a new clause may stand after the clause it belongs with, ids need not be in number order); give every table row, error-catalogue row, limit, interface entry and public-API signature the change touches; one owner per rule; a change that needs matching edits in two specs appears under both specs' headings with identical wording for the shared part. Group the changes under level-two headings named exactly "## seal.md", "## core.md", "## relay.md", "## engine.md", "## device.md" or "## bridge.md" (and "## README.md" for a change to the specs\' README, which has no clauses of its own).`
const BATCH = `Keep the work cheap: every message you send re-reads everything you have read so far, so use few messages. Issue the tool calls you can foresee together, as parallel calls in one message (for example every Grep and Read of a look-up round at once), not one call per message, and do not re-read what you have already read.`
const changeFiles = []

// ------------------------------------------------------------------ 0. follow-ups (only when args.followups names some)
if (args.followups) {
  phase('Follow-ups')
  const file = `${TR}/followups.md`
  const r = await safe(() => agent(`You write exact spec changes for a few small follow-ups of the TaskDeck sync specs (${SPECS}/: seal.md, core.md, relay.md, engine.md, device.md, bridge.md). You change no spec and run no git command that changes state.
The follow-ups: ${args.followups}
They are listed in ${MAIN}/docs/sync/status/foundation/resolutions/r1-followups.md (read that file). The lead's rulings they refer to are in ${MAIN}/docs/sync/status/foundation/resolutions/r1-lead.md (L-numbers), ${MAIN}/docs/sync/status/foundation/two-readers/r1/lead.md and r2/lead.md (T-numbers) and ${MAIN}/docs/sync/status/foundation.md (S-numbers: find the one named with Grep and read only that paragraph): read only the rulings named. Do not read any spec in full; find the places with Grep and read around them. ${BATCH}
Write the changes to ${file} (create the directory). ${RESOLVER_RULES} Head each change "### TR${ROUND}-FOLLOW-<n> (<follow-up item>)". End with "## For the lead" (only what cannot be settled from the specs and rulings, with your recommendation and the conservative reading written as the change).
Return the structured summary (changes_per_spec uses the bare names seal, core, relay, engine, device, bridge, README).`,
    { label: `resolve:followups:r${ROUND}`, phase: 'Follow-ups', model: 'opus', effort: 'high', schema: CHANGES }))
  if (r === null) { out.failed = 'resolve:followups'; return out }
  out.followups = r
  changeFiles.push({ file, per: r.changes_per_spec })
  log(`follow-ups: ${r.changes_per_spec.map(c => `${c.spec} ${c.changes}`).join(', ')}`)
}

// ------------------------------------------------------------------ 1. two readers, six scenarios each
// args.no_readers: a pass that only writes and applies the follow-ups (no readers, no comparers).
if (!args.no_readers) {
phase('Read')
for (const who of ['A', 'B']) {
  const dir = `${TR}/reader-${who}`
  const r = await safe(() => agent(`You are reader ${who} in the two-readers test of the TaskDeck sync specs. Your instructions are in ${BRIEF} : its sections "A. Reader", "The world" and "The scenarios" are for you (ignore the introduction and section B). All six scenarios are yours.
You are given ONLY the specs: the files under ${SPECS}/ (README.md, seal.md, core.md, relay.md, engine.md, device.md, bridge.md, wordlist.txt). Apart from the brief, open no other file anywhere: no SYNC.md, no plan, no status or resolution files, no source code, nothing else under ${S}. Run no git command and no search outside ${SPECS}/.
How to read, so that the work stays cheap and you keep room to write (every message you send re-reads all you have read so far; the six specs together are about 540,000 tokens, more than you can hold and still write well):
- In your FIRST message issue these calls together, as parallel calls in one message: Read the brief; Read README.md in full; and one Grep per spec (seal.md, core.md, relay.md, engine.md, device.md, bridge.md) for the pattern "^#{2,3} " with line numbers, which gives you every section heading and where it starts.
- From the headings choose the sections that govern the six scenarios, and read them in your SECOND message, again as parallel Read calls in one message, each a line range of at most 600 lines (offset and limit). Read a section once and to the byte. Leave out what cannot bear on the scenarios (for example fetchers, the look, the timing harness, release rules).
- A third message of reads for what you then find you still need is fine; after that, search with Grep rather than reading on. Open wordlist.txt only if a scenario needs a word from it.
Then work through the three groups one after the other. For each group think its two scenarios through to the byte, then write its file with a single Write call:
- ${dir}/edit.md : scenarios 1 and 2
- ${dir}/join.md : scenarios 3 and 6
- ${dir}/trust.md : scenarios 4 and 5
Each file is in the format section A fixes, and that format is binding: every byte string that is sealed, signed, hashed, stored or sent laid out as a table (field, width, value), every JSON body written out in full, every step with its clause or section; exact, complete for both of its scenarios, no introduction. Do not shorten a layout into running text to save space. The three files are your whole product.
Return the structured summary: the three files, for each scenario its number of steps and of AMBIGUOUS lines, and the specs you read.`,
    { label: `read:${who}:r${ROUND}`, phase: 'Read', model: 'opus', effort: 'high', schema: READ }))
  if (r === null) { out.failed = `read:${who}`; return out }
  out.readers[who] = r
  log(`reader ${who}: ${r.steps.map(s => `s${s.scenario} ${s.steps} steps, ${s.ambiguous} ambiguous`).join('; ')}`)
}

if (UNTIL === 'read') { out.stopped_after = 'read'; return out }

// ------------------------------------------------------------------ 2. comparers
phase('Compare')
let counting = 0
for (const g of GROUPS) {
  const file = `${TR}/${g.key}-diff.md`
  const tag = `TR${ROUND}-${g.key.toUpperCase()}`
  const earlier = changeFiles.map(c => c.file)
  const r = await safe(() => agent(`You are the comparer for group "${g.key}" (scenarios ${g.scenarios}) in round ${ROUND} of the two-readers test of the TaskDeck sync specs. Your instructions are in ${BRIEF} : read its section "B. Comparer", "The world" and the two scenarios. You change no spec and run no git command.
The two readers' files are ${TR}/reader-A/${g.key}.md and ${TR}/reader-B/${g.key}.md : read both in full. The specs are under ${SPECS}/ ; open them at the places the readers cite (do not read a spec in full). ${BATCH}
The lead's rulings of the earlier rounds stand (${MAIN}/docs/sync/status/foundation/two-readers/r1/lead.md and r2/lead.md , short: read them): write no change that contradicts one; if a difference comes from one, say so under "For the lead".
${earlier.length ? `Changes already written in this round, which the fixers will apply before yours: ${earlier.join(' , ')} . Read their headings and, where a difference of yours touches the same place, build on them and do not contradict them.` : ''}
Write to ${file} :
1. "# Differences": one numbered entry per difference or AMBIGUOUS line: a short title, what reader A wrote and what reader B wrote (quote the lines), the spec text each relied on (section or clause), and the verdict: spec, reader (say which reader, cite the clear text; add "clarified" if you also write a clarifying change), format, or both-wrong.
2. The exact spec changes, for every "spec", "reader ... clarified" and "both-wrong" entry. ${RESOLVER_RULES} Head each change "### ${tag}-<n> (difference <numbers>)".
3. "## For the lead": only what needs a ruling, with your recommendation and the conservative reading written as the change.
Return the structured summary: the counts by verdict (spec_differences, reader_errors_clarified, reader_errors_plain, format_only, both_wrong), the titles of the spec differences, changes_per_spec (bare names seal, core, relay, engine, device, bridge) and needs_lead_ruling.`,
    { label: `compare:${g.key}:r${ROUND}`, phase: 'Compare', model: 'opus', effort: 'high', schema: DIFF }))
  if (r === null) { out.failed = `compare:${g.key}`; return out }
  out.groups[g.key] = { diff: r }
  counting += r.spec_differences + r.reader_errors_clarified + r.both_wrong
  changeFiles.push({ file, per: r.changes_per_spec })
  log(`compare ${g.key}: spec ${r.spec_differences}, reader clarified ${r.reader_errors_clarified}, reader plain ${r.reader_errors_plain}, format ${r.format_only}, both wrong ${r.both_wrong}`)
}
out.counting_differences = counting
if (UNTIL === 'compare') { out.stopped_after = 'compare'; return out }
}

// ------------------------------------------------------------------ 3. fixers, one per spec with changes
phase('Spec fixes')
const norm = s => String(s).toLowerCase().replace(/\.md$/, '').trim()
for (const spec of ORDER) {
  const files = changeFiles.filter(c => (c.per || []).some(p => norm(p.spec) === spec.toLowerCase() && p.changes > 0)).map(c => c.file)
  if (files.length === 0) continue
  const n = changeFiles.reduce((a, c) => a + ((c.per || []).find(p => norm(p.spec) === spec.toLowerCase()) || { changes: 0 }).changes, 0)
  const r = await safe(() => agent(`YOUR TASK: apply written changes to ONE spec of the TaskDeck sync build, ${WORK}/docs/sync/spec/${spec}.md . ${WORK} is the spec track's working copy (a clone of the repository); edit only that one file there. ${MAIN} outside ${WORK} is read-only for you. Run no git command that changes state.
Apply exactly the changes under the heading "## ${spec}.md" of these files, in this order (about ${n} changes, each headed "### TR..."): ${files.join(' , ')} .
Be economical with reading: do NOT read the spec or the change files in full. In each change file find the heading "## ${spec}.md" with Grep (-n) and read from there to the next level-two heading; in the spec, find each place a change names with Grep and read only around it. ${BATCH} Conventions: a clause is one list item "- **SPEC-TOPIC-NNN** (owner stages) text with MUST or MUST NOT"; ids are unique, never reused or renumbered; each spec ends with the sections Error catalogue, Limits, Test vectors, Interfaces, Public API, Open questions, in that order; every layout's widths add up to its stated total.
Rules: apply each change exactly as written (full clause text verbatim); a later change in your list wins over an earlier one; if a change cannot be applied as written, apply what is unambiguous and report the rest under not_applied with the exact reason; change nothing that no change asks for. Afterwards run: ${CHECKTOOL} ; no problem line may name ${spec}.md except a dangling reference to an id that a change for another spec adds (say which).
Return the structured report: applied (the change ids), not_applied (id and reason), format_check (the tool's last lines), questions_for_lead.`,
    { label: `specfix:${spec}:r${ROUND}`, phase: 'Spec fixes', model: 'opus', effort: 'medium', schema: FIX }))
  if (r === null) { out.failed = `specfix:${spec}`; return out }
  out.fixes[spec] = { files, applied: r.applied.length, not_applied: r.not_applied, questions: r.questions_for_lead, format_check: r.format_check }
  log(`${spec}: applied ${r.applied.length}, not applied ${r.not_applied.length}`)
}
return out
