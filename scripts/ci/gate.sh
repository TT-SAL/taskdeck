#!/usr/bin/env bash
# The `gate` job's verdict (.github/workflows/ci.yml; scripts/ci/README.md).
#
#   bash scripts/ci/gate.sh     with NEEDS_JSON in the environment
#
# The job sets NEEDS_JSON to ${{ toJSON(needs) }} through `env:`: the gate
# job's `needs` context, an object with one entry per job the gate needs, each
# with a `result` of success, failure, cancelled or skipped. Prints a table of every job and its result, then passes only if:
#   - the `changes` job is there and its result is success, and
#   - no job's result is failure or cancelled.
# skipped counts as passed (a job the changed paths did not ask for). A result
# this script does not know, or input it cannot read, fails: the gate never
# passes by not understanding what it was given.
set -euo pipefail
command -v python3 >/dev/null || { echo "gate: error: python3 is needed" >&2; exit 2; }
exec python3 -I - <<'PY'
import json, os, sys

raw = os.environ.get("NEEDS_JSON", "")
try:
    needs = json.loads(raw)
except ValueError as e:
    print(f"gate: error: NEEDS_JSON is not JSON: {e}")
    sys.exit(1)
if not isinstance(needs, dict) or not needs:
    print("gate: error: NEEDS_JSON is not a non-empty object of jobs")
    sys.exit(1)

rows, bad = [], []
for job in sorted(needs):
    entry = needs[job]
    result = entry.get("result") if isinstance(entry, dict) else None
    result = result if isinstance(result, str) else "(none)"
    if job == "changes":
        verdict = "ok" if result == "success" else "FAIL: must succeed"
    elif result in ("success", "skipped"):
        verdict = "ok"
    elif result in ("failure", "cancelled"):
        verdict = "FAIL"
    else:
        verdict = "FAIL: unknown result"
    rows.append((job, result, verdict))
    if verdict != "ok":
        bad.append(job)
if "changes" not in needs:
    rows.append(("changes", "(missing)", "FAIL: must succeed"))
    bad.append("changes")

w = max(len("job"), *(len(r[0]) for r in rows))
v = max(len("result"), *(len(r[1]) for r in rows))
print("job".ljust(w) + "  " + "result".ljust(v) + "  verdict")
print("-" * w + "  " + "-" * v + "  -------")
for job, result, verdict in rows:
    print(f"{job:<{w}}  {result:<{v}}  {verdict}")
print()
if bad:
    print("gate: FAILED: " + ", ".join(bad))
    sys.exit(1)
print("gate: passed (skipped jobs count as passed)")
PY
