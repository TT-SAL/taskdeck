# Spec changes

Every change to a spec in this folder after stage `foundation` merges gets one row here: its date, the spec, the clause ids added, changed or withdrawn, why, and the stages affected. From `foundation` itself, the ambiguities the two-readers test (SYNC_PLAN.md §6.1) left open are recorded here too.

| Date | Spec | Clauses added | Clauses changed | Clauses withdrawn | Why | Stages affected |
|---|---|---|---|---|---|---|
| 2026-11-22 | all six | none | none | none | Left open by stage `foundation`: the two-readers test of SYNC_PLAN.md §6.1 ran two rounds of at most four. Round 1 found 40 differences that needed a spec change and round 2 found 12, every one in behaviour the specs had left open; in both rounds the readers agreed on every byte layout, counter, seq and chain value. All 52 were applied before the exit. What a third and a fourth round would have found is not known. A stage that meets a behaviour the specs do not settle raises a spec change. | all |
