# Briefs: pairwise interface reviews, resolution, per-spec fixes, requirement map

## A. Pairwise interface reviewer (one agent per pair)
You review the INTERFACE between two specs of the TaskDeck sync build: `docs/sync/spec/<A>.md` and
`docs/sync/spec/<B>.md`. You did not write either. You change no file.
Read `docs/sync/spec/README.md` (conventions) and `docs/sync/status/foundation.md` ("Rulings for the
specs"), then both specs in full, then the parts of SYNC.md and SYNC_PLAN.md they cite where you need the
intent. Find every place where the two specs, read by two teams that never talk, would produce code that
does not interoperate or that leaves work undone:
- the same thing defined twice differently (a layout, a field name or width, an order of fields, a context
  string, an error code, a limit, a state name, a default);
- something one spec requires from the other ("Interfaces" sections, cross-references) that the other does
  not provide, or provides under another name or shape;
- something neither spec owns although one of them must (a gap between them);
- a rule both own (duplicated normative text that can drift): say which should keep it;
- a sequence in which the two specs disagree about who acts, in what order, or what is durable first;
- a clause whose owner stages are wrong for this pair (a relay-side check owned only by `seal`, say);
- a cross-reference that names a section or clause id that does not exist;
- a contradiction with a ruling (S1–S10), with SYNC_PLAN.md §2, or with a verifier outcome in SYNC.md.
Do not report style, wording you merely dislike, or problems inside one spec that do not touch the other
(unless it makes the interface undecidable). Every finding must quote or cite both sides by section or
clause id and say what breaks. Propose the resolution: which spec changes, and to exactly what.
Return findings as structured data: id, title, spec_a_ref, spec_b_ref, problem, what_breaks,
proposed_resolution, specs_to_change, severity (high: bytes or protocol steps would not match;
medium: behaviour would differ or work is unowned; low: a dangling reference or duplicated rule).
Also return `sound`: the interface points you checked and found consistent (briefly).

## B. Resolver (one agent, after all six reviews)
You are given every finding of the six pairwise interface reviews. Several findings touch the same spec;
some conflict. Read the six specs' relevant sections, the README conventions, the rulings and the cited
parts of SYNC.md and SYNC_PLAN.md. For each finding decide: accept (with the exact change per spec),
merge with another finding, or reject (with the reason: the reviewer misread, cite the text). Where two
findings pull in different directions, choose the resolution that (1) follows SYNC_PLAN.md and SYNC.md,
(2) keeps one owner per rule, (3) changes the fewest bytes on the wire. You change no spec. Write the
resolution list to the file you are told, grouped by the spec that must change: for each change the
finding ids it resolves, the section, the exact new rule or layout (so the fixer need not design
anything), and any clause ids to add. Flag anything that needs the lead's ruling.

## C. Per-spec fixer (one agent per spec; edits only its spec)
Apply to `docs/sync/spec/<name>.md` exactly the changes the resolution list assigns to it. Keep the
README's clause format; never renumber or reuse an existing id (add new ids at the end of their topic's
range); keep sections in order. After editing, re-run the checks in "Before you finish" of the drafter
rules. Report each resolution as applied (with the section) or not applied (with the reason).

## D. Requirement map (task 3e)
Fill the table between `<!-- requirement-map:begin -->` and `<!-- requirement-map:end -->` in
`docs/sync/spec/README.md`. The foundation gate (SYNC_PLAN.md §6.1): "a table in `spec/README.md` maps
every requirement in SYNC.md §3–§9 to a clause id, a stage in this plan, or a row of §2 above."
Method: walk SYNC.md §3, §4, §5, §6, §7, §8 and §9 from top to bottom, sentence by sentence and table
row by table row. Every statement that requires the system, a device, the relay, the build or the owner's
tooling to do or be something is a requirement (features in §3's table; protocol rules in §4; algorithms,
keys and procedures in §5; hosting rules, quotas, backups and the exit plan in §6; every control in §7's
table and the residual-risk notes that imply behaviour; §8's build, distribution and update rules; §9's
migration steps). One row per requirement:
`| SYNC.md ref | Requirement (a short exact paraphrase) | Maps to |`
where "Maps to" is one or more of: clause ids that state it (prefer these: read the specs and cite the
clauses that actually say it, not ones that merely mention the topic); a stage name of SYNC_PLAN.md when
the requirement is that stage's work and no spec clause governs it (UI screens, runbooks, release
pipeline, owner procedures): write `stage: desktop` and so on; or `plan §2: <row topic>` when
SYNC_PLAN.md §2 changed or dropped it (no M1, no always-on device, no `.ics` feed or exports, …).
A requirement that maps to nothing is a finding: do not invent a mapping. List such requirements
separately in your report with a proposal (which spec should gain a clause, or which stage owns it).
Do not change the specs. Keep the table sorted by SYNC.md section. Expect several hundred rows.
