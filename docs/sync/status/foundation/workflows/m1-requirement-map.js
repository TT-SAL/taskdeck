export const meta = {
  name: 'foundation-m1-requirement-map',
  description: 'Foundation, task 3e: two mappers write the requirement map (every requirement of SYNC.md sections 3 to 9 to a clause id, a stage or a row of SYNC_PLAN.md section 2), then a checker who did not write it looks for missing rows and wrong clauses; one agent at a time; nothing in the repository is changed',
  phases: [
    { title: 'Map', detail: 'two mappers, each a part of SYNC.md', model: 'opus' },
    { title: 'Check', detail: 'one checker over both parts', model: 'opus' },
  ],
}

// args: { scratch: '<session scratchpad>', steps: ['map', 'check'] }
const S = args.scratch
const STEPS = args.steps || ['map', 'check']
const MAIN = '/home/user/taskdeck'
const OUT = `${S}/reqmap`
const INDEX = `${S}/clause-index.md`
const PARTS = [
  { key: 'a', sections: '3 (Feature parity), 4 (Sync protocol) and 5 (Cryptography and keys)', lines: 'lines 77 to 283' },
  { key: 'b', sections: '6 (Hosting and uptime), 7 (Security), 8 (Build, distribution and updates) and 9 (Migration)', lines: 'lines 284 to 388' },
]
const safe = async (thunk) => { try { return await thunk() } catch (e) { return null } }
const BATCH = 'Keep the work cheap: every message you send re-reads everything you have read so far, so use few messages: issue the reads you can foresee together, as parallel calls in one message, and do not re-read.'
const MAP = {
  type: 'object',
  properties: {
    file: { type: 'string' }, rows: { type: 'number' },
    by_kind: { type: 'object', properties: { clause: { type: 'number' }, stage: { type: 'number' }, plan2: { type: 'number' } }, required: ['clause', 'stage', 'plan2'] },
    unmapped: { type: 'array', items: { type: 'object', properties: { ref: { type: 'string' }, requirement: { type: 'string' }, proposal: { type: 'string' } }, required: ['ref', 'requirement', 'proposal'] } },
    notes: { type: 'array', items: { type: 'string' } },
  },
  required: ['file', 'rows', 'by_kind', 'unmapped', 'notes'],
}
const CHECK = {
  type: 'object',
  properties: {
    summary: { type: 'string' },
    rows_added: { type: 'number' }, rows_corrected: { type: 'number' }, rows_checked_against_the_spec: { type: 'number' },
    unmapped: { type: 'array', items: { type: 'object', properties: { ref: { type: 'string' }, requirement: { type: 'string' }, proposal: { type: 'string' } }, required: ['ref', 'requirement', 'proposal'] } },
  },
  required: ['summary', 'rows_added', 'rows_corrected', 'rows_checked_against_the_spec', 'unmapped'],
}
const RULES = `One row per requirement, in SYNC.md's order: "| SYNC.md ref | Requirement | Maps to |". The table is checked by a script (scripts/spec-lint.sh), so its form is exact: the header row is "| SYNC.md ref | Requirement | Maps to |" followed by the separator row; every "SYNC.md ref" cell STARTS with the number of the SYNC.md section the requirement stands in, written "§4.3" (the deepest numbered heading that contains it; "§3", "§7" or "§9" for a section without subsections), then a few words for the place within it (for example "§4.3 sequencing" or "§7 row 12"); rows are in ascending order of that section number; every numbered section of SYNC.md sections 3 to 9 that has no subsections has at least one row; no cell holds a "|" character. "Requirement" is a short exact paraphrase, one line. "Maps to" is one or more of the following, separated by ", ": clause ids that STATE the requirement, written bare, without backticks (prefer these; cite the clauses that actually say it, not ones that only mention the topic; at most four ids per row); "stage: <name>" when the requirement is that stage's work and no spec clause governs it (screens, runbooks, the release pipeline, owner procedures); the stage names are those of SYNC_PLAN.md section 6 (foundation, core, seal, relay, engine, android-shell, device, recovery, desktop, android, release, rc); or "plan §2: <topic>" when SYNC_PLAN.md section 2 changed or dropped it, where <topic> is exactly the first cell of that row of section 2's table (for example "plan §2: Milestones"). A statement that requires the system, a device, the relay, the build or the owner's tooling to do or be something is a requirement: features in section 3's table; protocol rules; algorithms, keys and procedures; hosting rules, quotas, backups and the exit plan; every control in section 7's table and each residual-risk note that implies behaviour; build, distribution and update rules; migration steps. Explanations, history and rationale are not. A requirement that maps to nothing is NOT given an invented mapping: leave it out of the table and report it under unmapped with a proposal (which spec should gain a clause, or which stage owns it).`
const out = { parts: {} }

if (STEPS.includes('map')) {
  phase('Map')
  for (const p of PARTS) {
    const file = `${OUT}/part-${p.key}.md`
    const r = await safe(() => agent(`You write part of the requirement map of the TaskDeck sync build (stage foundation's gate, SYNC_PLAN.md section 6.1: "a table in spec/README.md maps every requirement in SYNC.md §3–§9 to a clause id, a stage in this plan, or a row of §2 above"). You change nothing in the repository; you write one file, ${file} (create the directory).
Your part: SYNC.md sections ${p.sections}: ${MAIN}/SYNC.md ${p.lines}. Read: that part of SYNC.md; ${MAIN}/SYNC_PLAN.md lines 56 to 79 (section 2, what changed since SYNC.md); and the clause index ${INDEX} in full (one line per clause of the six specs under its section heading, 941 clauses, about 190 KB: read it in three or four parallel Read calls by line range). The index is how you find the clause that states a requirement; open a spec itself (${MAIN}/docs/sync/spec/<name>.md) only when two candidate clauses must be told apart. ${BATCH}
${RULES}
Walk your part from top to bottom, sentence by sentence and table row by table row. Write the rows to ${file} as a Markdown table with the header row, nothing else in the file. Expect well over a hundred rows.
Return the structured summary: the file, the number of rows, how many map to a clause, to a stage, to plan §2, the unmapped requirements, and notes (places where SYNC.md and a spec seem to disagree).`,
      { label: `map:${p.key}`, phase: 'Map', model: 'opus', effort: 'high', schema: MAP }))
    if (r === null) { out.failed = `map:${p.key}`; return out }
    out.parts[p.key] = r
    log(`part ${p.key}: ${r.rows} rows (${r.by_kind.clause} clause, ${r.by_kind.stage} stage, ${r.by_kind.plan2} plan), ${r.unmapped.length} unmapped`)
  }
}
if (STEPS.includes('check')) {
  phase('Check')
  const r = await safe(() => agent(`You check the requirement map of the TaskDeck sync build. You did not write it. The map is two files of table rows, ${OUT}/part-a.md (SYNC.md sections 3 to 5) and ${OUT}/part-b.md (sections 6 to 9). You change nothing in the repository; you may correct those two files in place.
The rules the mappers had: ${RULES}
Do two things. (1) Completeness: read ${MAIN}/SYNC.md lines 77 to 388 and both files; for every requirement in SYNC.md that has no row, add the row in its place (find its clause in the clause index ${INDEX}, which lists every clause of the six specs; read it in full, in three or four parallel Read calls) or, if nothing states it, report it under unmapped. (2) Truth of the mappings: take every fourth row of each file, open the first clause id it cites in its spec (${MAIN}/docs/sync/spec/<name>.md, Grep for the id) and judge whether that clause states the requirement; where it does not, replace it with the clause that does, or report the row under unmapped and remove it. Also correct any id that does not exist in the index. ${BATCH}
Return the structured summary: rows added, rows corrected, how many rows you checked against the spec text, and what remains unmapped (with a proposal each).`,
    { label: 'check:map', phase: 'Check', model: 'opus', effort: 'high', schema: CHECK }))
  if (r === null) { out.failed = 'check:map'; return out }
  out.check = r
}
return out
