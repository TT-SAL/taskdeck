#!/usr/bin/env bash
# Checks .github/workflows/ci.yml against the rules in scripts/ci/README.md, by
# reading its lines (no YAML library is assumed): the gate needs every job, the
# token is read-only, no secrets, no caches, actions pinned by commit, every
# job has a timeout, matrices do not fail fast, every bash script runs under
# `shell: bash`, no event data in shell text, the toolchain step before cargo,
# and the stage jobs wired to scripts/ci/stage.sh.
set -uo pipefail
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
WF="$REPO_DIR/.github/workflows/ci.yml"
command -v python3 >/dev/null || { echo "FAIL  python3 is needed"; exit 1; }

check_workflow() {
    python3 -I - "$1" "$SCRIPTS_DIR/ci/changes.sh" <<'PY'
import re, sys

wf_path, changes_path = sys.argv[1], sys.argv[2]
lines = open(wf_path, encoding="utf-8").read().split("\n")
fails, passes = [], []

def check(name, ok, detail=""):
    (passes if ok else fails).append(name if ok else f"{name}: {detail}")

def indent(s):
    return len(s) - len(s.lstrip(" "))

text = "\n".join(lines)
check("no tabs", "\t" not in text, "tab characters")
check("no secrets", "secrets." not in text and "secrets[" not in text, "mentions secrets")
check("no caches", not re.search(r"actions/cache|^\s*cache:|cache-dependency|save-always", text, re.M), "a cache")
check("no rustup default or update", not re.search(r"rustup +(default|update)", text), "found")

# Top-level permissions: exactly contents: read.
try:
    i = lines.index("permissions:")
    block = []
    for l in lines[i + 1:]:
        if l.strip() == "" or indent(l) == 0:
            break
        block.append(l.strip())
    check("permissions: contents: read only", block == ["contents: read"], str(block))
except ValueError:
    check("permissions: contents: read only", False, "no top-level permissions")
# Only pull-request runs: a push run tests only what changed since `before`,
# so a cancelled push's changes would reach no run (scripts/ci/README.md). A
# pending run is cancelled by a newer one in its group even without
# cancel-in-progress, so each push run needs a group of its own.
check("concurrency cancels superseded pull-request runs, never push runs",
      re.search(r"^concurrency:\n  group: ci-\$\{\{ github\.event_name \}\}-\$\{\{ github\.ref \}\}"
                r"\$\{\{ github\.event_name == 'push' && format\('-\{0\}', github\.run_id\) \|\| '' \}\}\n"
                r"  cancel-in-progress: \$\{\{ github\.event_name == 'pull_request' \}\}$",
                text, re.M) is not None, "missing")

# Jobs: id -> its lines.
j0 = lines.index("jobs:")
jobs, order, cur = {}, [], None
for l in lines[j0 + 1:]:
    m = re.match(r"^  ([A-Za-z0-9_-]+):\s*$", l)
    if m:
        cur = m.group(1)
        jobs[cur] = []
        order.append(cur)
    elif cur is not None and (l.strip() == "" or indent(l) >= 4 or l.lstrip().startswith("#")):
        jobs[cur].append(l)
    elif l.strip() and indent(l) == 0:
        break

def key(job, k):
    for l in jobs[job]:
        if indent(l) == 4 and l.strip().startswith(k + ":"):
            return l.strip()[len(k) + 1:].strip()
    return None

def steps(job):
    out, cur = [], None
    for l in jobs[job]:
        if re.match(r"^      - ", l):
            cur = [l]
            out.append(cur)
        elif cur is not None and (indent(l) >= 8 or l.strip() == ""):
            cur.append(l)
        elif l.strip() and indent(l) <= 4:
            cur = None
    return out

check("the jobs are there", {"changes", "gate", "linux", "desktop", "emulator", "supply-chain",
      "foundation", "core", "seal", "engine", "device", "recovery", "relay", "android"} <= set(jobs),
      str(order))

for job in order:
    check(f"{job}: timeout-minutes", re.fullmatch(r"[0-9]+", key(job, "timeout-minutes") or "") is not None,
          "missing")
    body = "\n".join(jobs[job])
    if re.search(r"^    strategy:", body, re.M):
        check(f"{job}: fail-fast: false", re.search(r"^      fail-fast: false$", body, re.M) is not None, "missing")
    if job != "changes":
        needs = key(job, "needs")
        check(f"{job}: needs changes", needs == "changes" or (job == "gate" and needs == ""), str(needs))
    uses_cargo = re.search(r"\bcargo |scripts/ci/stage\.sh|check-release-panic|check-build-date|"
                           r"scripts/tests/run\.sh|scripts/dev-setup\.sh", body)
    if uses_cargo:
        st = steps(job)
        first_tc = next((n for n, s in enumerate(st) if "bash scripts/ci/toolchain.sh" in "\n".join(s)), None)
        first_use = next((n for n, s in enumerate(st) if re.search(
            r"\bcargo |scripts/ci/stage\.sh|check-release-panic|check-build-date|scripts/tests/run\.sh|"
            r"scripts/dev-setup\.sh", "\n".join(s))), None)
        check(f"{job}: the toolchain step comes first", first_tc is not None and first_tc < first_use,
              f"toolchain step {first_tc}, first use {first_use}")
    for n, s in enumerate(steps(job)):
        st = "\n".join(s)
        for m in re.finditer(r"uses: ([^@\s]+)@(\S+)(.*)", st):
            check(f"{job} step {n}: {m.group(1)} is allowed",
                  m.group(1) in ("actions/checkout", "reactivecircus/android-emulator-runner"), m.group(1))
            check(f"{job} step {n}: pinned by commit with the version in a comment",
                  re.fullmatch(r"[0-9a-f]{40}", m.group(2)) is not None and re.match(r"\s+# v\d", m.group(3)),
                  m.group(0))
        run_at = next((k for k, l in enumerate(s) if re.match(r"^\s*(- )?run:", l)), None)
        if run_at is not None:
            run_lines = [s[run_at].split("run:", 1)[1]]
            base = indent(s[run_at].replace("- ", "  ", 1))
            for l in s[run_at + 1:]:
                if l.strip() and indent(l) <= base:
                    break
                run_lines.append(l)
            run_text = "\n".join(run_lines)
            check(f"{job} step {n}: no ${{{{ }}}} in shell text", "${{" not in run_text, run_text.strip())
            if re.search(r"\bbash\b", run_text):
                check(f"{job} step {n}: a bash script runs under shell: bash",
                      re.search(r"^\s*shell: bash$", st, re.M) is not None, run_text.strip())

# The changes job: its outputs are exactly the areas of changes.sh, wide and oses.
areas = re.search(r"^AREAS=\(([^)]*)\)", open(changes_path, encoding="utf-8").read(), re.M).group(1).split()
outs = []
in_out = False
for l in jobs["changes"]:
    if l.startswith("    outputs:"):
        in_out = True
        continue
    if in_out:
        m = re.match(r"^      ([a-z-]+): \$\{\{ steps\.decide\.outputs\.([a-z-]+) \}\}$", l)
        if not m:
            break
        check(f"changes: output {m.group(1)} is the step's", m.group(1) == m.group(2), l)
        outs.append(m.group(1))
check("changes: outputs are the areas, wide and oses", outs == areas + ["wide", "oses"], str(outs))
ch = "\n".join(jobs["changes"])
check("changes: full history", "fetch-depth: 0" in ch, "no fetch-depth: 0")
check("changes: no condition", key("changes", "if") is None, "has an if")
check("changes: checks what the code reads", re.search(r"^        run: bash scripts/ci/reads\.sh$", ch, re.M) is not None,
      "no reads.sh step")

# Each area job runs on its area.
area_job = {"linux": "root", "desktop": "root", "emulator": "android", "supply-chain": "supply",
            "foundation": "foundation", "core": "core", "seal": "seal", "engine": "engine",
            "device": "device", "recovery": "recovery", "relay": "relay", "android": "android"}
for job, area in area_job.items():
    cond = key(job, "if") or ""
    check(f"{job}: runs when {area} is true", f"needs.changes.outputs.{area} == 'true'" in cond, cond)
for job in ("desktop", "emulator"):
    check(f"{job}: only on wide runs", "needs.changes.outputs.wide == 'true'" in (key(job, "if") or ""), "")

# The stage jobs.
for job in ("core", "seal", "engine", "device", "recovery", "relay", "android"):
    body = "\n".join(jobs[job])
    check(f"{job}: runs scripts/ci/stage.sh {job}",
          re.search(rf"^\s+run: bash scripts/ci/stage\.sh {job}$", body, re.M) is not None, "")
    check(f"{job}: passes CI_WIDE", "CI_WIDE: ${{ needs.changes.outputs.wide }}" in body, "")
    if job == "relay":
        check(f"{job}: Ubuntu only", key(job, "runs-on") == "ubuntu-latest", str(key(job, "runs-on")))
    else:
        check(f"{job}: a matrix over oses", "os: ${{ fromJSON(needs.changes.outputs.oses) }}" in body
              and key(job, "runs-on") == "${{ matrix.os }}", "")

# Desktop: the release-panic check on every system.
check("desktop: checks release panics", "run: bash scripts/check-release-panic.sh" in "\n".join(jobs["desktop"]), "")
fd = "\n".join(jobs["foundation"])
for s in ("bash scripts/tests/run.sh", "bash scripts/check-release-panic.sh", "bash scripts/check-build-date.sh"):
    check(f"foundation: runs {s}", f"run: {s}" in fd, "")
check("foundation: a marked place for later tasks", "LATER FOUNDATION TASKS" in fd, "")
check("foundation: runs the spec lint", re.search(r"^        run: bash scripts/spec-lint\.sh$", fd, re.M) is not None, "")
# The supply-chain job: the toolchain, then scripts/ci/supply-chain.sh, which
# installs its own tools; nothing else that could fail it.
sc_runs = [l.strip() for l in jobs["supply-chain"] if re.match(r"^\s+(- )?run: ", l)]
check("supply-chain: only the toolchain and scripts/ci/supply-chain.sh",
      sc_runs == ["run: bash scripts/ci/toolchain.sh", "run: bash scripts/ci/supply-chain.sh"], str(sc_runs))

# The gate.
g = jobs["gate"]
check("gate: if: always()", key("gate", "if") == "always()", str(key("gate", "if")))
check("gate: named gate on pull requests only",
      key("gate", "name") == "${{ github.event_name == 'pull_request' && 'gate' || 'gate (push)' }}",
      str(key("gate", "name")))
needs, in_needs = [], False
for l in g:
    if l.startswith("    needs:"):
        in_needs = True
        continue
    if in_needs:
        m = re.match(r"^      - ([A-Za-z0-9_-]+)$", l)
        if not m:
            break
        needs.append(m.group(1))
others = [j for j in order if j != "gate"]
check("gate: needs every other job, one per line", sorted(needs) == sorted(others),
      f"missing {sorted(set(others) - set(needs))}, extra {sorted(set(needs) - set(others))}")
check("gate: no job twice", len(needs) == len(set(needs)), str(needs))
check("gate: hands the needs context to gate.sh",
      "NEEDS_JSON: ${{ toJSON(needs) }}" in "\n".join(g) and "run: bash scripts/ci/gate.sh" in "\n".join(g), "")
check("gate: is the last job", order[-1] == "gate", order[-1])

for f in fails:
    print("FAIL  workflow: " + f)
print(f"ok    workflow: {len(passes)} checks passed" if not fails else f"      ({len(passes)} other checks passed)")
sys.exit(1 if fails else 0)
PY
}

expect "ci.yml follows the rules" 0 check_workflow "$WF"
[[ "$OUT" == *FAIL* ]] && sed 's/^/        /' <<<"$OUT"

# The checker itself catches what it is for: break a copy, one rule at a time.
broken() {   # broken <case> <sed expression>
    sed -e "$2" "$WF" >"$TMP/ci.yml"
    if cmp -s "$WF" "$TMP/ci.yml"; then
        FAILS=$((FAILS + 1)); echo "FAIL  $1: the sed expression changed nothing"; return
    fi
    expect "$1" 1 check_workflow "$TMP/ci.yml"
}
broken "a job missing from the gate's needs" '/^      - relay$/d'
broken "a job without a timeout" '0,/timeout-minutes: 45/{/timeout-minutes: 45/d}'
broken "an unpinned action" 's/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1/checkout@v7/'
broken "a new third-party action" '0,/uses: actions\/checkout/s/uses: actions\/checkout/uses: someone\/checkout/'
broken "a write permission" 's/^  contents: read$/  contents: write/'
broken "a bash script without shell: bash" '0,/        shell: bash/{/        shell: bash/d}'
broken "event data in shell text" 's/run: bash scripts\/ci\/stage.sh core/run: bash scripts\/ci\/stage.sh core "${{ github.event.head_commit.message }}"/'
broken "a cache" 's/^          fetch-depth: 0$/          cache: true/'
broken "rustup default" 's/run: bash scripts\/ci\/toolchain.sh/run: rustup default stable/'
broken "a gate that does not always run" 's/^    if: always()$/    if: success()/'
broken "the gate named gate on pushes" "s/'gate (push)'/'gate'/"
broken "a stage job on the wrong area" "s/needs.changes.outputs.seal == 'true'/needs.changes.outputs.core == 'true'/"
broken "a matrix that fails fast" '0,/fail-fast: false/s/fail-fast: false/fail-fast: true/'
broken "no check of what the code reads" '/^        run: bash scripts\/ci\/reads.sh$/d'
broken "an android job on Ubuntu only" '/^  android:$/,/^  gate:$/s/runs-on: \${{ matrix.os }}/runs-on: ubuntu-latest/'
broken "no spec lint" '/^        run: bash scripts\/spec-lint.sh$/d'
broken "an extra tools step in supply-chain" 's/^        run: bash scripts\/ci\/supply-chain.sh$/        run: bash scripts\/dev-setup.sh foundation \&\& bash scripts\/ci\/supply-chain.sh/'
broken "a lost changes output" '/^      oses: \${{ steps.decide.outputs.oses }}$/d'

finish
