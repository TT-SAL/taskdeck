export const meta = {
  name: 'foundation-s2-spec-fixes',
  description: 'Foundation, spec track: turn the lead rulings into exact changes, apply every round-1 resolution spec by spec in a working copy, and have a second agent check each spec (one agent at a time; short reading lists, medium effort for appliers and checkers)',
  phases: [
    { title: 'Lead changes', detail: 'one resolver writes the exact changes the lead rulings need', model: 'opus' },
    { title: 'Spec fixes', detail: 'per spec, in chunks, in resolution order', model: 'opus' },
    { title: 'Check', detail: 'a second agent checks every change against the spec', model: 'opus' },
  ],
}

// args: { scratch: '<session scratchpad>', plan: { seal: [ { files: ['r1-seal-relay.md'], n: 14 }, ... ], core: [...], ... } }
// plan: for each spec, the chunks of resolution files whose "## <spec>.md" heading holds changes, in resolution
// order. The lead-changes file is appended here as each spec's last chunk when it holds changes for that spec.
const S = args.scratch
const PLAN = args.plan
const MAIN = '/home/user/taskdeck'
const WORK = `${MAIN}/target/work/s`
const BRIEFS = `${MAIN}/docs/sync/status/foundation/briefs`
const RES = `${S}/resolutions`
const CHECKTOOL = `python3 ${MAIN}/docs/sync/status/foundation/tools/speccheck.py ${WORK}/docs/sync/spec 300`
const ORDER = ['seal', 'core', 'relay', 'engine', 'device', 'bridge']
const NINE = ['r1-seal-relay.md', 'r1-seal-engine.md', 'r1-core-engine.md', 'r1-engine-relay.md', 'r1-device-engine.md', 'r1-device-bridge.md', 'r1-core-device.md', 'r1-seal-device.md', 'r1-relay-device.md']
const LEADFILE = 'r1-lead-changes.md'

const GROUND = `
GROUND RULES (stage "foundation" of SYNC_PLAN.md, spec track):
- TWO DIRECTORIES. ${MAIN} is the lead's main checkout: READ-ONLY for you. ${WORK} is the spec track's WORKING COPY, a full clone of the same repository: the spec you edit is there, under ${WORK}/docs/sync/spec/. Never create, change or delete anything under ${MAIN} outside ${WORK}. The lead copies checked specs into the main checkout and commits them.
- Do NOT run any git command that changes state, in either directory. Read-only git (status, diff, log, show) is fine: "git -C ${WORK} diff -- docs/sync/spec/<name>.md" shows what the working copy has changed since the last commit.
- The resolution files are in ${RES} : nine files named r1-<a>-<b>.md (one per reviewed pair, written in this order: ${NINE.join(', ')}; a later file builds on the earlier ones), the lead's rulings r1-lead.md (binding), and ${LEADFILE} (the exact changes the rulings need; written last, applied last).
- The repository is public: made-up data only, no secret, address, token or account name.
`

const RESOLUTION = {
  type: 'object',
  properties: {
    summary: { type: 'string' },
    changes_per_spec: { type: 'array', items: { type: 'object', properties: { spec: { type: 'string' }, changes: { type: 'number' } }, required: ['spec', 'changes'] } },
    already_written: { type: 'array', items: { type: 'string' } },
    id_conflicts: { type: 'array', items: { type: 'string' } },
    needs_lead_ruling: { type: 'array', items: { type: 'string' } },
  },
  required: ['summary', 'changes_per_spec', 'already_written', 'id_conflicts', 'needs_lead_ruling'],
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
const CHECK = {
  type: 'object',
  properties: {
    summary: { type: 'string' },
    total_changes: { type: 'number' },
    applied_exactly: { type: 'number' },
    problems: { type: 'array', items: {
      type: 'object',
      properties: {
        id: { type: 'string' },
        verdict: { type: 'string', enum: ['not_applied', 'partly_applied', 'applied_differently', 'conflict'] },
        detail: { type: 'string' },
        fix: { type: 'string' },
      },
      required: ['id', 'verdict', 'detail', 'fix'],
    } },
    unrequested_edits: { type: 'array', items: { type: 'string' } },
    format_problems: { type: 'array', items: { type: 'string' } },
  },
  required: ['summary', 'total_changes', 'applied_exactly', 'problems', 'unrequested_edits', 'format_problems'],
}
const safe = async (thunk) => { try { return await thunk() } catch (e) { return null } }
const out = { specs: {} }

// ------------------------------------------------------------------ 1. the lead's rulings as exact changes
phase('Lead changes')
const lead = await safe(() => agent(`You are the last resolver of round 1 of the interface reviews of the six TaskDeck sync specs (repository ${MAIN}, docs/sync/spec/: seal.md, core.md, relay.md, engine.md, device.md, bridge.md). You change no spec and run no git command that changes state.

Nine resolvers, one per reviewed pair and one after another, wrote the exact changes for 152 findings into ${RES}/ (${NINE.join(', ')}). Each ended with a list "For the lead". The lead has ruled on every item: ${RES}/r1-lead.md (rulings L1, L2, ...; binding). While they waited for a ruling, several resolvers wrote a conservative reading into their changes, and some left text to "the lead's change file". You write that file: ${RES}/${LEADFILE} .

Be economical with reading: do NOT read the nine files or the specs in full. Read ${RES}/r1-lead.md in full (it is short). Then use Grep over ${RES}/r1-*-*.md to find the "For the lead" sections and every change that touches a ruling's subject (search for the subject's words, for example: reinstat, S24, missing_root, ENG-EPOCH, AdminRequest, compact, at_ms, own-picture, adopt, forget_identity, set_look_dials, legacy_unreadable, relay/slow), and read only those changes and the spec passages they name. Clause format (docs/sync/spec/README.md section 4): a list item "- **SPEC-TOPIC-NNN** (owner stages) text with MUST"; ids are never reused or renumbered.

For each ruling:
- if the nine files already contain exactly what the ruling decides, write no change: add one line under "## Already written" naming the change ids that carry it;
- otherwise write the exact changes that make the specs say what the ruling decides. The fixers apply the nine files in order and yours last, so your text lands on top of theirs: for each change name the earlier change ids it supersedes or amends and say precisely which text of theirs to replace or remove. Same exactness as theirs: layouts, fields, widths, order, values, error codes, full clause text with ids (a new id continues after the highest id that the spec or any of the nine files uses in that topic; never reuse or renumber an id), and every table row, error-catalogue row, limit, interface entry and public-API signature the change touches. A change that needs matching edits in two specs appears under both headings with identical wording for the shared part.
Ruling L1 is the large one: the channel for ruling S24 across seal.md (the admin registry's args for the reinstate operation, the unwrapping and the commitment check of each missed root, removal of the "a reinstated device pairs again" text that R1-SEAL-RELAY-8 and R1-SEAL-ENGINE-13 wrote, including the open-question items they added), relay.md (the reinstate args and what the relay checks, the devices row column, the login answer field, its limits, rows per operation and errors), engine.md (section 24.4, the clause ENG-EPOCH-005, the sealer method that takes the wraps, the removal of the conservative text), device.md (the hand-over R1-DEVICE-ENGINE-13 wrote: give it the relay field's name; section 18.4), and bridge.md if anything there says a reinstated phone pairs again.

Ids: the lead has checked mechanically that no new clause id is assigned twice across the nine files. Two existing clauses are reworded by two files each, ENG-MOVE-003 and SEAL-ENC-006: read both rewordings of each; the later file's text is applied last and wins, so if it loses something the earlier one added, write the merged text as a change. List anything else you notice under "## Id conflicts"; do not search for more.

Layout of ${RES}/${LEADFILE} : one level-two heading per spec that must change, exactly "## seal.md", "## core.md", "## relay.md", "## engine.md", "## device.md" or "## bridge.md"; under it numbered changes headed "### R1-LEAD-<n> (<ruling ids, or 'id conflict'>)"; then "## Already written", "## Id conflicts", and "## For the lead" (only what still cannot be decided from the rulings, with your recommendation, and the conservative reading written as the change).
Return the structured summary (changes_per_spec uses the bare names seal, core, relay, engine, device, bridge).`,
  { label: 'resolve:lead-changes', phase: 'Lead changes', model: 'opus', effort: 'high', schema: RESOLUTION }))
if (lead === null) { out.failed = 'resolve:lead-changes'; return out }
out.lead = lead
log(`lead changes: ${lead.changes_per_spec.map(c => `${c.spec} ${c.changes}`).join(', ')}; id conflicts ${lead.id_conflicts.length}; for the lead ${lead.needs_lead_ruling.length}`)
const leadCount = (spec) => (lead.changes_per_spec.find(c => String(c.spec).replace(/\.md$/, '') === spec) || { changes: 0 }).changes

// ------------------------------------------------------------------ 2. fixers and 3. checkers, spec by spec
const fixer = (spec, chunk, k, total, done, later) => safe(() => agent(`${GROUND}
YOUR TASK: apply resolved changes to ONE spec, ${WORK}/docs/sync/spec/${spec}.md . Edit only that file. You are run ${k} of ${total} for this spec.
Be economical with reading: do NOT read the spec, the resolution files or any brief in full. In each resolution file, find the heading "## ${spec}.md" with Grep (-n) and read only from there to the next level-two heading; in the spec, find each place a change names with Grep and read only around it. The conventions you need: a clause is one list item "- **SPEC-TOPIC-NNN** (owner stages) text with MUST or MUST NOT"; ids are unique, never reused or renumbered; each spec ends with the sections Error catalogue, Limits, Test vectors, Interfaces, Public API, Open questions, in that order; every layout's widths must add up to its stated total.
In this run apply exactly the changes under the heading "## ${spec}.md" of: ${chunk.files.map(f => `${RES}/${f}`).join(' , ')} , in that order (about ${chunk.n} changes), each headed "### R1-...".
${done.length ? `Already applied to this spec by earlier runs (do not apply again; the spec in the working copy already holds them): the "## ${spec}.md" changes of ${done.join(', ')}.` : 'No resolution change has been applied to this spec yet.'}
${later.length ? `Not yet applied (a later run does it; do NOT apply them now, but read their "## ${spec}.md" headings so that nothing you write contradicts them): ${later.join(', ')}.` : 'These are the last changes for this spec.'}
Read each change's whole text, and the other specs' headings in the same file where the change says the wording is shared, so the shared part is identical.
Rules:
- Apply each change exactly as written: the resolvers designed, you do not. Where a change gives full clause text, use it verbatim. Keep the README's clause format; never renumber or reuse an existing id; keep the sections in order; update the Error catalogue, Limits, Test vectors, Interfaces, Public API and Open questions sections wherever the change says so.
- A later change in your list may amend an earlier one: the later text wins.
- If a change cannot be applied as written (the text it names is not there, it contradicts a change already applied or a lead ruling, an id it assigns is already taken), do not improvise: apply what is unambiguous and report the rest under not_applied with the exact reason. Never drop a change silently.
- Change nothing that no change asks for.
- Afterwards run: ${CHECKTOOL} . No problem line may name ${spec}.md, except a dangling reference to an id that a change for ANOTHER spec adds (say which change).
Return the structured report: applied (the change ids), not_applied (id and reason), format_check (the tool's last lines and what remains), questions_for_lead.`,
  { label: `specfix:${spec}:${k}`, phase: 'Spec fixes', model: 'opus', effort: 'medium', schema: FIX }))

const checker = (spec, files, round) => safe(() => agent(`${GROUND}
YOUR TASK: check, as someone who made none of the edits, that every resolved change for ONE spec has been applied exactly. The spec is ${WORK}/docs/sync/spec/${spec}.md ; "git -C ${WORK} diff -- docs/sync/spec/${spec}.md" shows everything the fixers changed. You change nothing.
The changes for this spec are the items headed "### R1-..." under the heading "## ${spec}.md" in: ${files.map(f => `${RES}/${f}`).join(' , ')} (applied in that order; a later change may amend or supersede an earlier one, and ${LEADFILE} wins over all, as do the rulings in ${RES}/r1-lead.md).
Be economical with reading: do NOT read the spec or the resolution files in full. In each resolution file, find the heading "## ${spec}.md" with Grep (-n) and read only from there to the next level-two heading. Work from the diff; open the spec itself only where the diff's context is not enough.
For every change: find it in the diff and compare with the change's text (clause ids and text, layouts, widths, values, codes, table rows, the closing sections it names). Count it as applied exactly, or report it: not_applied, partly_applied (say what is missing), applied_differently (say how the spec differs from the change's text, quoting both), or conflict (two changes or a ruling contradict each other in the spec as it stands). For each problem give the exact fix. A change that a later change or a ruling superseded is not a problem if the spec holds the later text.
Also list unrequested_edits: anything in the diff that no change asked for (a reworded clause, a renumbered id, a moved section, a dropped paragraph).
Also run ${CHECKTOOL} and list under format_problems every problem line that names ${spec}.md, except dangling references to ids that a change for another spec adds (the other specs are fixed after this one${round > 1 ? '' : ', in the order seal, core, relay, engine, device, bridge'}).
Be exact and complete: this check is what the lead relies on. Return the structured result.`,
  { label: `speccheck:${spec}:${round}`, phase: 'Check', model: 'opus', effort: 'medium', schema: CHECK }))

const fixup = (spec, files, check) => safe(() => agent(`${GROUND}
YOUR TASK: repair ONE spec, ${WORK}/docs/sync/spec/${spec}.md , after an independent check of the resolved changes found the problems below. Edit only that file.
Be economical with reading: read only the changes the problems name (find them with Grep in the resolution files) and the places in the spec they concern. A clause is one list item "- **SPEC-TOPIC-NNN** (owner stages) text with MUST"; ids are never reused or renumbered. The changes for this spec are under the heading "## ${spec}.md" in: ${files.map(f => `${RES}/${f}`).join(' , ')} (a later file wins over an earlier one; ${LEADFILE} and the rulings win over all).
For each problem: read the change it names, verify the problem against the spec, then make the spec say exactly what the change says. Revert each unrequested edit unless a change requires it (say which). Fix each format problem. If a reported problem is wrong (the spec already holds the right text), say so and change nothing for it. Do not improvise: what cannot be settled from the change files goes under questions_for_lead.
Afterwards run: ${CHECKTOOL} .
Problems (JSON):
${JSON.stringify({ problems: check.problems, unrequested_edits: check.unrequested_edits, format_problems: check.format_problems }, null, 1)}
Return the structured report: applied (the problem ids you fixed), not_applied (id and reason), format_check, questions_for_lead.`,
  { label: `specfixup:${spec}`, phase: 'Spec fixes', model: 'opus', effort: 'medium', schema: FIX }))

const dirty = (c) => c.problems.length + c.unrequested_edits.length + c.format_problems.length

for (const spec of ORDER) {
  const chunks = (PLAN[spec] || []).map(c => ({ files: [...c.files], n: c.n }))
  if (leadCount(spec) > 0) {
    if (chunks.length) { const last = chunks[chunks.length - 1]; last.files.push(LEADFILE); last.n += leadCount(spec) }
    else chunks.push({ files: [LEADFILE], n: leadCount(spec) })
  }
  const rec = { runs: [], check: null }
  out.specs[spec] = rec
  if (chunks.length === 0) { log(`${spec}: no changes`); continue }
  const allFiles = chunks.flatMap(c => c.files)
  let failed = false
  for (let i = 0; i < chunks.length; i++) {
    const done = chunks.slice(0, i).flatMap(c => c.files)
    const later = chunks.slice(i + 1).flatMap(c => c.files)
    const r = await fixer(spec, chunks[i], i + 1, chunks.length, done, later)
    if (r === null) { failed = true; break }
    rec.runs.push({ files: chunks[i].files, applied: r.applied.length, not_applied: r.not_applied, questions: r.questions_for_lead, format_check: r.format_check })
    log(`${spec}: run ${i + 1}/${chunks.length} applied ${r.applied.length}, not applied ${r.not_applied.length}`)
  }
  if (failed) { out.failed = `specfix:${spec}`; return out }
  let c = await checker(spec, allFiles, 1)
  if (c === null) { out.failed = `speccheck:${spec}`; return out }
  rec.check = c
  log(`${spec}: check 1: ${c.applied_exactly}/${c.total_changes} exact, ${c.problems.length} problems, ${c.unrequested_edits.length} unrequested, ${c.format_problems.length} format`)
  if (dirty(c) > 0) {
    const f = await fixup(spec, allFiles, c)
    if (f === null) { out.failed = `specfixup:${spec}`; return out }
    rec.fixup = { applied: f.applied, not_applied: f.not_applied, questions: f.questions_for_lead, format_check: f.format_check }
    log(`${spec}: fix-up handled ${f.applied.length}, left ${f.not_applied.length} (the lead checks what is left)`)
  }
}
return out
