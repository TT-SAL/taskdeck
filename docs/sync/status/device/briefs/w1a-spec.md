# W1a-spec: one spec change for everything workflow W1a found

Common rules: `README.md` beside this file, except where this brief differs. Task id `s1`.

## Goal

The owner's decision 5 (status file, "How this stage is run"): every place where the real parts did
not fit, or a spec is wrong, unclear or silent, is fixed through one spec-change pull request per
workflow. W1a found 31: `docs/sync/status/device/fit.md`, entries h-1 to h-3, f1-1 to f1-15, f2-1 to
f2-13. Judge each, and write the spec text for those that need it, as plan §3.3 says (spec, vectors
where bytes change, one row in `docs/sync/spec/CHANGES.md`).

## Where

`git -C /home/user/taskdeck worktree add /home/user/wt/s1 -b spec/w1a claude/optimistic-lovelace-6z2f55`;
work only in `/home/user/wt/s1`; change only `docs/sync/spec/**`; commit on `spec/w1a`; never push.
No cargo build is needed; if you run one, `CARGO_TARGET_DIR=/home/user/td-target/s1`.

## For each finding

1. Read its entry in `fit.md`, the spec section it names (by line range, never a whole spec), and
   only as much of the code it names as you need to state what the device does.
2. Give it one verdict:
   - **spec**: the text changes: it was wrong, unclear or silent, or it records a choice `device`
     made that other stages or later device work must follow (an error code, a layout, an order).
   - **none**: the spec already says what the code does, or the matter is an implementation detail
     no reader of the spec needs; say why in one line.
   - **crate**: the right fix is a change to another crate's code or API (`seal`, `core`, `relay`,
     `engine`), not made now; the adapter's workaround stays; give the change in one or two lines.
     Change spec text only if the spec itself must state something today.
   - **owner**: the fix would weaken a MUST or a budget, remove a feature, or is otherwise the
     owner's (plan §3.5). Write no text; draft the question in two or three sentences with your
     recommendation.
3. Write the text for each **spec** verdict where the rule lives (mostly `device.md`; `seal.md`,
   `engine.md`, `core.md` or `relay.md` where the finding is theirs). Follow the clause conventions of
   `docs/sync/spec/README.md`: a new MUST gets the next free id of its area and names its owning
   stage; a changed clause keeps its id; nothing is renumbered; a new error code goes into the error
   catalogue of its spec, with its status where the catalogue has one. Keep the wording style of the
   surrounding text: short sentences, the facts, no history.

Weigh especially:
- **f1-5** (a genesis snapshot of a non-empty board is circular): find the smallest sound rule
  (`seal.md` §12.2 and §20.4, `device.md` §18.1 and §18.2). If every sound rule changes seal's
  layouts, code or vectors, give it the verdict **crate** with the rule you propose and what it
  changes in seal, and write no text: the lead decides whether it waits for the migration
  (`desktop`'s "Create board from this folder") or goes to seal now.
- **h-3** (`device.md` §19.2): the next task writes `taskdeck-devicectl` against this text, so make
  it exact (where a result's fields sit, `wait`'s arguments and answer, what an unknown `cmd` and an
  invalid line answer). Match what `tests/e2e/harness/ctl.rs` already assumes unless the spec has a
  reason to differ.
- Findings that say a seal or core type gives the adapter too little (f1-1, f1-2, f1-3, f1-10):
  usually **crate** (the workaround is honest), unless the spec's interface text is wrong.

## Then

- `CHANGES.md`: one row, dated 2026-11-28: the specs, the clause ids added, changed and withdrawn,
  why (one sentence per group of findings, naming the fit ids), the stages affected.
- `bash scripts/spec-lint.sh` passes in your worktree.
- One commit on `spec/w1a`: subject `spec: what wiring the real core and seal into the engine found
  (device W1a)`, the README's two closing lines.

## Your answer

The structured output's `findings` holds one entry per fit id: `id`, `kind` = your verdict,
`title` = one line (for **spec**: where the text went and the clause ids; for **crate**: the change
proposed; for **owner**: the question), `proposed_fix` = anything the lead must know. `next` names
the commit and anything left unclear. `done` is true only if `spec-lint.sh` passes.
