# Rules for every spec drafter (stage foundation)

You write ONE file: `docs/sync/spec/<name>.md`. Nothing else in the repository. No git state changes.

## Read first, in this order
1. `docs/sync/spec/README.md` — the conventions. Binding: clause format, owner tags, section order, encodings.
2. `docs/sync/status/foundation.md`, section "Rulings for the specs" (every ruling from S1 on, including the
   ones added after the seal draft, S11–S15) and "Decisions". Binding.
3. `SYNC_PLAN.md` in full (it is short). Where it and `SYNC.md` disagree, the plan wins; its §2 lists every difference:
   no M1 hub (no hub contexts, no forwarding, no `Remote::Tunnel`), no always-on device, no `.ics` feed or export,
   minSdk 26, a forked phone page, limited administration from the phone, reminders in the `android` stage,
   capture exclusion on the main window instead of a second window, no legacy-folder watch, separate crates.
4. The sources your brief names. Do not read huge files whole (`src/phone.html` is 288 KB, `src/ui.rs` 397 KB,
   `DOCUMENTATION.md` 249 KB): use Grep and ranged reads.
5. Any spec already present in `docs/sync/spec/` (drafts by other agents). Align with them: use their names,
   layouts and clause ids where you depend on them, and list every assumption you make about them.

## What a spec is for
A later stage's lead and its agents implement from your spec alone, in parallel with other stages that never
talk to each other. Two independent readers must derive the same bytes and the same message sequence from it.
So: precision over prose.
- Every byte layout is a table: field, width, type, value or rule. State totals. State the byte order only if
  it differs from the README (it must not).
- Every JSON message or record is a schema table: field, type, required or optional, constraints, default,
  meaning. Say what a parser does with each violation.
- Every state machine is a table of states, events, guards, effects and next states. Every "in order" is a
  numbered list.
- Every limit is a number with a unit. Every error has a code in the error catalogue.
- No "as appropriate", "as needed", "should handle", "etc.". If you cannot make something exact, decide it,
  write the decision into the text as normative, and list it under "Open questions" as
  "decided here: … (SYNC.md is silent)".
- One owner per rule: where another spec owns a rule, point to it instead of restating it as a clause.
  Where you need something from another spec that may not say it yet, state the requirement in your
  "Interfaces" section.
- Clauses: only MUST / MUST NOT, one requirement each, testable as written, with the owner stages whose
  tests must prove it. Default owner is your spec's stage; add another stage when that stage must prove the
  clause too (README section on clause format). Number as you write; keep ids unique within the file.
- Security rules that apply to all code implementing your spec (SYNC_PLAN.md §3.4): named primitives composed
  exactly as specified, no hand-written primitives, no `unsafe`, secrets zeroized, constant-time comparison
  of secrets and MACs, every size checked before allocation, nothing parsed before it is authenticated.
  Write the concrete clauses these imply for your spec.
- Close every design gap your brief lists, and any other you find. Never leave a "TBD".
- Do not copy SYNC.md's prose. Cite it ("SYNC.md §4.3", "V1-12") where a rule comes from.

## Before you finish
1. Check every item of your scope list in SYNC_PLAN.md §6.1 item 3 and in your brief is covered.
2. Check every verifier finding your brief names is reflected, or say why not.
3. Run a grep over your file with the README's clause regular expression: every clause line matches,
   no id appears twice, every owner is an allowed stage name.
4. Check every byte layout's widths add up to its stated total and every cross-reference resolves.
5. Re-read the file once as an implementer who has nothing else, and fix what you would have had to guess.

## Return (structured)
A summary; the section list; the clause count per owner stage; the design gaps you closed (one line each);
every assumption about another spec ("I assume relay.md …"); questions for the lead; model_self_report
(the exact model name or id your own system prompt says you are).
