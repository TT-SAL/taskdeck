#!/usr/bin/env python3
"""The spec lint (scripts/spec-lint.sh runs it; see that file for the rules)."""
import argparse
import collections
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import speclib  # noqa: E402

# The stages of SYNC_PLAN.md §6, for the requirement map's `stage: <name>`.
STAGES = ["foundation", "core", "seal", "relay", "engine", "android-shell", "device",
          "recovery", "desktop", "android", "release", "rc"]
# The map may also name the plan's backlog (§6.13); no clause may have it as owner.
MAP_STAGES = STAGES + ["backlog"]
MAP_HEADER = ["SYNC.md ref", "Requirement", "Maps to"]
SEPARATOR = re.compile(r"^\|?\s*:?-{3,}:?\s*(\|\s*:?-{3,}:?\s*)*\|?\s*$")
# The whole reference cell: one section reference and nothing else.
SECTION_REF = re.compile(r"(?:SYNC\.md\s+)?§(\d+(?:\.\d+)*)")


def sync_sections(path):
    """Numbered headings of SYNC.md, as tuples: (3,), (4, 1), ..."""
    out = []
    for line in speclib.read_lines(path):
        m = re.match(r"^#{2,6} (\d+(?:\.\d+)*)\.?\s", line)
        if m:
            out.append(tuple(int(x) for x in m.group(1).split(".")))
    return out


def plan_topics(path):
    """The first column of the table in SYNC_PLAN.md §2."""
    lines = speclib.read_lines(path)
    start = next((i for i, l in enumerate(lines) if re.match(r"^## 2\. ", l)), None)
    if start is None:
        return None
    topics, rows = [], 0
    for l in lines[start + 1:]:
        if l.startswith("## "):
            break
        if speclib.TABLE.match(l):
            rows += 1
            if rows > 2:
                cell = speclib._cells(l)[0]
                topics.append(re.sub(r"[*`]", "", cell).strip())
        elif rows:
            break
    return topics


def norm(s):
    return re.sub(r"\s+", " ", re.sub(r"[*`]", "", s)).strip().lower()


def parse_targets(cell, topics):
    """Splits a 'Maps to' cell into (ids, stages, plan rows, leftover text)."""
    ids, stages, rows = [], [], []
    rest = cell
    leftover = []
    while True:
        rest = rest.lstrip(" ,;")
        if not rest:
            break
        m = re.match(r"(?:%s)-[A-Za-z0-9_-]+" % speclib.PREFIXES, rest)
        if m:
            ids.append(m.group(0))
            rest = rest[m.end():]
            continue
        m = re.match(r"`?stage:\s*([a-z0-9-]+)`?", rest)
        if m:
            stages.append(m.group(1))
            rest = rest[m.end():]
            continue
        m = re.match(r"`?(?:plan|SYNC_PLAN\.md) §2:\s*", rest)
        if m:
            tail = rest[m.end():]
            best = None
            for t in topics or []:
                tn = norm(t)
                if norm(tail).startswith(tn) and (best is None or len(tn) > len(norm(best))):
                    best = t
            if topics is None:
                # No plan to check against: the topic runs to the next ';'.
                topic, _, rest = tail.partition(";")
                rows.append(topic.strip())
                continue
            if best is None:
                leftover.append(f"plan §2: {tail.split(';')[0].strip()}")
                rest = tail.partition(";")[2]
                continue
            rows.append(best)
            # Consume as many characters of tail as the topic has, ignoring markup.
            want, i, got = norm(best), 0, ""
            while i < len(tail) and norm(got) != want:
                got += tail[i]
                i += 1
            rest = tail[i:].lstrip("`")
            continue
        word = re.match(r"\S+", rest).group(0)
        leftover.append(word)
        rest = rest[len(word):]
    return ids, stages, rows, leftover


def check_map(readme, rel, specs, allow_empty, sync_path, plan_path, problems):
    lines = speclib.read_lines(readme)
    begins = [i for i, l in enumerate(lines) if l.strip() == speclib.MAP_BEGIN]
    ends = [i for i, l in enumerate(lines) if l.strip() == speclib.MAP_END]
    if len(begins) != 1 or len(ends) != 1 or begins[0] > ends[0]:
        problems.append(f"{rel}: the requirement map needs exactly one '{speclib.MAP_BEGIN}' followed by one '{speclib.MAP_END}'")
        return 0
    body = [(i + 1, lines[i]) for i in range(begins[0] + 1, ends[0])]
    if not any(l.strip() for _, l in body):
        if not allow_empty:
            problems.append(f"{rel}:{begins[0] + 1}: the requirement map is empty (SYNC_PLAN.md §6.1 gate; --allow-empty-map accepts that)")
        return 0
    try:
        sections = sync_sections(sync_path)
    except OSError as e:
        problems.append(f"{sync_path}: cannot read SYNC.md to check the map: {e.strerror}")
        sections = None
    try:
        topics = plan_topics(plan_path)
        if topics is None:
            problems.append(f"{plan_path}: no '## 2.' section with a table: cannot check 'plan §2' rows")
    except OSError as e:
        problems.append(f"{plan_path}: cannot read SYNC_PLAN.md to check the map: {e.strerror}")
        topics = None
    state = "header"
    rows = 0
    last = None
    seen_blank_after_table = False
    covered = set()
    for n, l in body:
        where = f"{rel}:{n}"
        if not l.strip():
            if state != "header":
                seen_blank_after_table = True
            continue
        if not speclib.TABLE.match(l):
            problems.append(f"{where}: only the map table may stand between the markers")
            continue
        if seen_blank_after_table:
            problems.append(f"{where}: a blank line splits the map table")
            seen_blank_after_table = False
        cells = speclib._cells(l)
        if state == "header":
            if cells != MAP_HEADER:
                problems.append(f"{where}: the map header must be '| {' | '.join(MAP_HEADER)} |'")
            state = "separator"
            continue
        if state == "separator":
            if not SEPARATOR.match(l.strip()) or len(cells) != len(MAP_HEADER):
                problems.append(f"{where}: the map's second row must be the separator row")
            state = "rows"
            continue
        rows += 1
        if len(cells) != len(MAP_HEADER):
            problems.append(f"{where}: map row has {len(cells)} cells, the header {len(MAP_HEADER)}")
            continue
        ref, req, target = cells
        m = SECTION_REF.fullmatch(ref)
        if not m:
            problems.append(f"{where}: {ref!r} is not a SYNC.md section reference such as '§4.2'")
        else:
            sec = tuple(int(x) for x in m.group(1).split("."))
            if not 3 <= sec[0] <= 9:
                problems.append(f"{where}: §{m.group(1)} is outside SYNC.md §3–§9")
            elif sections is not None and sec not in sections:
                problems.append(f"{where}: SYNC.md has no section §{m.group(1)}")
            if last is not None and sec < last:
                problems.append(f"{where}: the map is not sorted by SYNC.md section (§{m.group(1)} after §{'.'.join(map(str, last))})")
            last = sec
            covered.add(sec)
        if not req:
            problems.append(f"{where}: the requirement cell is empty")
        ids, stages, plan_rows, leftover = parse_targets(target, topics)
        for w in leftover:
            problems.append(f"{where}: cannot read {w!r} in 'Maps to': write clause ids, 'stage: <name>' or 'plan §2: <row topic>'")
        good = 0
        for cid in ids:
            c = specs.clauses.get(cid)
            if not speclib.ID_TOKEN.fullmatch(cid):
                problems.append(f"{where}: malformed clause id {cid!r}")
            elif c is None:
                problems.append(f"{where}: dangling reference {cid} (no such clause)")
            elif c.withdrawn:
                problems.append(f"{where}: maps to {cid}, which is withdrawn")
            else:
                good += 1
        for st in stages:
            if st in MAP_STAGES:
                good += 1
            else:
                problems.append(f"{where}: 'stage: {st}' names no stage of SYNC_PLAN.md")
        good += len(plan_rows)
        if good == 0 and not leftover:
            problems.append(f"{where}: the row maps to no clause id, stage or 'plan §2' row")
    if state != "rows" or rows == 0:
        problems.append(f"{rel}: the requirement map has no data rows")
    if sections is not None and rows:
        in_scope = [s for s in sections if 3 <= s[0] <= 9]
        leaves = [s for s in in_scope if not any(t != s and t[:len(s)] == s for t in in_scope)]
        for s in leaves:
            # Only a row for the leaf itself covers it: a row for §4 does not
            # stand in for §4.1 and §4.2 (lead ruling 3).
            if not any(c[:len(s)] == s for c in covered):
                problems.append(f"{rel}: no map row for SYNC.md §{'.'.join(map(str, s))}")
    return rows


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    repo = os.path.dirname(here)
    ap = argparse.ArgumentParser(prog="spec-lint.sh", add_help=True)
    ap.add_argument("--summary", action="store_true")
    ap.add_argument("--allow-empty-map", action="store_true")
    ap.add_argument("--spec-dir", default=os.path.join(repo, "docs", "sync", "spec"))
    ap.add_argument("--sync", default=os.path.join(repo, "SYNC.md"))
    ap.add_argument("--plan", default=os.path.join(repo, "SYNC_PLAN.md"))
    a = ap.parse_args()

    rel = "docs/sync/spec"
    specs = speclib.load(a.spec_dir, rel)
    problems = list(specs.problems)
    for sf in specs.files:
        problems.extend(sf.problems)

    # Error codes appear once across all specs.
    seen = {}
    for sf in specs.files:
        for code, n in sf.codes:
            if code in seen:
                problems.append(f"{sf.rel}:{n}: error code {code} appears twice (first at {seen[code]})")
            else:
                seen[code] = f"{sf.rel}:{n}"

    # References.
    for sf in specs.files:
        for cid, n, withdrawn_ok in sf.refs:
            c = specs.clauses.get(cid)
            if c is None:
                problems.append(f"{sf.rel}:{n}: dangling reference {cid} (no such clause)")
            elif c.withdrawn and not withdrawn_ok:
                problems.append(f"{sf.rel}:{n}: refers to {cid} as live, but it is withdrawn")

    # The README holds no clauses, and its requirement map.
    readme = os.path.join(a.spec_dir, "README.md")
    readme_rel = f"{rel}/README.md"
    map_rows = 0
    if os.path.exists(readme) and specs.rules.clause_re is not None:
        in_code, fence = False, ""
        for n, l in enumerate(speclib.read_lines(readme), 1):
            fm = speclib.FENCE.match(l)
            if fm:
                if not in_code:
                    in_code, fence = True, fm.group(1)
                elif fm.group(1)[0] == fence[0] and l.strip() == fm.group(1):
                    in_code = False
                continue
            if not in_code and (specs.rules.clause_re.match(l) or speclib.BOLD_ID.search(l)):
                problems.append(f"{readme_rel}:{n}: clause-shaped line in the README, which holds no clauses")
        map_rows = check_map(readme, readme_rel, specs, a.allow_empty_map, a.sync, a.plan, problems)

    if a.summary:
        per_spec = collections.OrderedDict((sf.name, [0, 0]) for sf in specs.files)
        per_owner = collections.Counter()
        for c in specs.clauses.values():
            per_spec[c.spec][1 if c.withdrawn else 0] += 1
            for o in c.owners or []:
                per_owner[o] += 1
        print("clauses per spec (live, withdrawn):")
        for name, (live, wd) in per_spec.items():
            print(f"  {name + '.md':<10} {live:>5} {wd:>4}")
        live = sum(v[0] for v in per_spec.values())
        print(f"  {'total':<10} {live:>5} {sum(v[1] for v in per_spec.values()):>4}")
        print("live clauses per owner stage (a clause with several owners counts for each):")
        for o in specs.rules.owners:
            print(f"  {o:<14} {per_owner.get(o, 0):>5}")
        print(f"requirement map rows: {map_rows}")

    for p in problems:
        print(p)
    if problems:
        print(f"spec-lint: FAILED: {len(problems)} problem(s)")
        return 1
    print(f"spec-lint: ok ({len(specs.clauses)} clauses in {len(specs.files)} specs; requirement map: {map_rows} rows)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
