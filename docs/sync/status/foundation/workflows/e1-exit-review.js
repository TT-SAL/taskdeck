export const meta = {
  name: 'foundation-e1-exit-review',
  description: 'Foundation, stage exit (SYNC_PLAN.md section 7, "Stage exit"): one agent that did no work in the stage reads the plan, the status file, the diff and the specs, and lists what is missing',
  phases: [ { title: 'Review', detail: 'one reviewer who did no work in the stage', model: 'opus' } ],
}

// args: { scratch: '<session scratchpad>' }
const MAIN = '/home/user/taskdeck'
const OUT = `${args.scratch}/exit-review.md`
const REVIEW = {
  type: 'object',
  properties: {
    summary: { type: 'string' },
    missing: { type: 'array', items: { type: 'object', properties: {
      title: { type: 'string' }, severity: { type: 'string', enum: ['blocks-exit', 'should-fix', 'note'] },
      plan_text: { type: 'string' }, evidence: { type: 'string' }, proposal: { type: 'string' },
    }, required: ['title', 'severity', 'plan_text', 'evidence', 'proposal'] } },
    checked_and_fine: { type: 'array', items: { type: 'string' } },
  },
  required: ['summary', 'missing', 'checked_and_fine'],
}
phase('Review')
const r = await agent(`You review stage "foundation" of the TaskDeck sync build before it exits. You did no work in this stage. SYNC_PLAN.md section 7: "Stage exit: one agent that did no work in the stage reads the status file, the diff and the spec, and lists what is missing. What it finds is the next round of work." You change nothing; you write your list to ${OUT} and return it.
The repository is ${MAIN} (branch claude/upbeat-hamilton-klp7n9; the base branch is origin/day-planner). Read, in this order: ${MAIN}/SYNC_PLAN.md sections 3, 4, 5 and 6.1 (lines 80 to 191; section 6.1 is the stage's task list and its gate) and section 7's last lines; then ${MAIN}/docs/sync/status/foundation.md from its start to the end of the section "Deviations from the plan" (the task table says what is claimed done). Then check the claims against the repository itself: "git -C ${MAIN} diff --stat origin/day-planner...HEAD" for what the stage changed; and for each item of section 6.1 (1 baseline with its six bullets, 2 workspace, 3 contracts, 4 skeletons, 5 fixtures, 6 tooling) and each part of its gate, find the thing in the tree and judge whether it is what the plan asks: read the top of the file, run its --help or --list, run the cheap checks yourself (bash scripts/spec-lint.sh ; bash scripts/gate.sh --list foundation ; bash scripts/spec-coverage.sh foundation ; cargo metadata --locked --no-deps --format-version 1 ; grep for unimplemented bodies ; ls fixtures ; bash scripts/dev-setup.sh --list). Do not run the long checks (builds, the test suites, bash scripts/gate.sh foundation without --list): the lead has run them and CI runs them; say which you relied on. Keep the work cheap: every message re-reads all you have read, so issue the reads and commands you can foresee together as parallel calls, and do not read whole specs or whole source files (the six specs are 1.3 MB: look at their section lists and closing sections only).
What to look for: a task or a bullet of section 6.1 with nothing in the tree, or with something that does less than the plan's words; a gate item that nothing checks; a rule of section 3 that the stage's own work breaks (a secret or anything from a real board in a committed file, a dependency outside section 3.4, an edit under src/ beyond the deviation the status file names, DOCUMENTATION.md section 14 touched); a claim in the status file that the tree does not bear out; something a wave-1 stage (core, seal, relay, engine, android-shell: sections 6.2 to 6.6, read their "Read:" and first lines only) needs on its first day and would not find. Not style, not preferences, and not the deviations the status file already records with their reasons (say if a recorded reason does not hold).
Write to ${OUT}: one entry per missing or wrong thing, with the plan's words, the evidence (path, command and output), a severity ("blocks-exit": the stage's gate or a rule of section 3 is not met; "should-fix": a later stage would stumble; "note") and what would fix it. Then what you checked and found in order. Return the same as the structured result.`,
  { label: 'exit-review', phase: 'Review', model: 'opus', effort: 'high', schema: REVIEW })
return r
