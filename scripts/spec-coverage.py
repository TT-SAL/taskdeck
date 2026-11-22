#!/usr/bin/env python3
"""Spec coverage (scripts/spec-coverage.sh runs it and holds the rules).

  spec-coverage.py <check|list> <stage> <root> <pattern>...
"""
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import speclib  # noqa: E402

# Test files are source files: a data file or a note under tests/ cites nothing.
SOURCE_EXTS = (".rs", ".js", ".mjs", ".cjs", ".ts", ".kt", ".kts", ".java", ".html", ".sh", ".py")
# The marker `spec: <ID>` (README §4.3): a token after it counts only when it
# starts like an id, so a field or binding such as `spec: String` is none.
MARKER = re.compile(r"(?<![A-Za-z0-9_-])spec:\s*((?:%s)-[A-Za-z0-9_-]*)" % speclib.PREFIXES)
# A line that declares a test whose name may hold an id.
TEST_DECL = re.compile(r"\b(?:it|test|describe)(?:\.only)?\s*\(\s*['\"`]|\bfun\s+`|@DisplayName\s*\(")
NAME_TOKEN = re.compile(r"(?<![A-Za-z0-9_-])((?:%s)-[A-Za-z0-9_-]+)" % speclib.PREFIXES)


def glob_re(pattern):
    out, i = "", 0
    while i < len(pattern):
        if pattern.startswith("**/", i):
            out += "(?:.*/)?"
            i += 3
        elif pattern.startswith("**", i):
            out += ".*"
            i += 2
        elif pattern[i] == "*":
            out += "[^/]*"
            i += 1
        elif pattern[i] == "?":
            out += "[^/]"
            i += 1
        else:
            out += re.escape(pattern[i])
            i += 1
    return re.compile(out + r"\Z")


def base_of(pattern):
    parts = []
    for p in pattern.split("/"):
        if any(ch in p for ch in "*?["):
            break
        parts.append(p)
    return "/".join(parts)


def test_files(root, patterns):
    include = [p for p in patterns if not p.startswith("!")]
    exclude = [glob_re(p[1:]) for p in patterns if p.startswith("!")]
    found = set()
    for p in include:
        rx = glob_re(p)
        base = base_of(p)
        start = os.path.join(root, base) if base else root
        if os.path.isfile(start):
            cands = [base]
        elif os.path.isdir(start):
            cands = []
            for d, dirs, files in os.walk(start):
                dirs[:] = sorted(x for x in dirs if not speclib.is_output_dir(d, x))
                for f in files:
                    cands.append(os.path.relpath(os.path.join(d, f), root).replace(os.sep, "/"))
        else:
            continue
        for rel in cands:
            if rel.endswith(SOURCE_EXTS) and rx.match(rel) and not any(e.match(rel) for e in exclude):
                found.add(rel)
    return sorted(found)


def citations(path):
    """(token, line) pairs a test file cites."""
    try:
        with open(path, "rb") as f:
            data = f.read()
    except OSError:
        return []
    if b"\0" in data[:8192]:
        return []
    out = []
    for n, line in enumerate(data.decode("utf-8", "replace").split("\n"), 1):
        for m in MARKER.finditer(line):
            out.append((m.group(1).rstrip("-"), n))
        if TEST_DECL.search(line):
            for m in NAME_TOKEN.finditer(line):
                out.append((m.group(1).rstrip("-"), n))
    return out


def main():
    mode, stage, root = sys.argv[1], sys.argv[2], sys.argv[3]
    patterns = sys.argv[4:]
    spec_dir = os.path.join(root, "docs", "sync", "spec")
    specs = speclib.load(spec_dir)
    if specs.rules.clause_re is None:
        for p in specs.problems:
            print(p)
        print("spec-coverage: cannot read the clause format from the README")
        return 2
    owned = [c for c in specs.clauses.values() if not c.withdrawn and stage in c.owners]
    files = test_files(root, patterns)
    cited_by = {}
    bad = []
    for rel in files:
        for tok, n in citations(os.path.join(root, rel)):
            c = specs.clauses.get(tok)
            if c is None:
                bad.append(f"{rel}:{n}: cites {tok}, which is no clause")
            elif c.withdrawn:
                bad.append(f"{rel}:{n}: cites {tok}, which is withdrawn")
            else:
                cited_by.setdefault(tok, [])
                if rel not in cited_by[tok]:
                    cited_by[tok].append(rel)

    uncovered = [c for c in owned if c.id not in cited_by]
    if mode == "list":
        for c in owned:
            where = f"{c.path}:{c.line}"
            print(f"{c.id:<18} {where:<34} {' '.join(cited_by.get(c.id, [])) or '(no test cites it)'}")
    else:
        for c in uncovered:
            print(f"{c.id}  {c.path}:{c.line}")
    for b in bad:
        print(b)
    summary = (f"spec-coverage {stage}: {len(owned) - len(uncovered)} of {len(owned)} owned clauses cited, "
               f"{len(uncovered)} uncovered, {len(bad)} bad citation(s), in {len(files)} test file(s)")
    if not owned:
        summary = f"spec-coverage {stage}: the stage owns no clause; {len(bad)} bad citation(s) in {len(files)} test file(s)"
    print(summary)
    return 1 if uncovered or bad else 0


if __name__ == "__main__":
    sys.exit(main())
