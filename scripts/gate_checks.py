#!/usr/bin/env python3
"""The checks of scripts/gate-lib.sh that read JSON or TOML. Run from the root
of the tree being checked:

  gate_checks.py workspace-sanity          (cargo metadata JSON on stdin)
  gate_checks.py clippy-config <clippy.toml> <required list>
  gate_checks.py vectors <dir>
  gate_checks.py npm-no-deps <package.json>
  gate_checks.py scan <regex> [--js <regex>] <path>...
                                           (a path starting with ! is left out; the
                                           --js regex is also matched in JavaScript
                                           and HTML files)
"""
import json
import os
import re
import sys
import tomllib

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import speclib  # noqa: E402

# The root package and the crates of SYNC_PLAN.md §4.
WORKSPACE_CRATES = ["task_deck", "taskdeck-core", "taskdeck-seal", "taskdeck-sync", "taskdeck-device",
                    "taskdeck-backup", "taskdeck-recover", "taskdeck-android"]


def workspace_sanity():
    try:
        m = json.load(sys.stdin)
    except ValueError as e:
        print(f"workspace-sanity: FAILED: cargo metadata printed no JSON: {e}")
        return 1
    members = {p["name"] for p in m.get("packages", []) if p.get("id") in m.get("workspace_members", [])}
    missing = [w for w in WORKSPACE_CRATES if w not in members]
    for w in missing:
        print(f"workspace-sanity: {w} is not a workspace member (SYNC_PLAN.md §4, §6.1 item 2)")
    if missing:
        print("workspace-sanity: FAILED")
        return 1
    print(f"workspace-sanity: ok ({len(members)} workspace members, among them the {len(WORKSPACE_CRATES)} of §4)")
    return 0


# A lint attribute that turns the disallowed-* lints off for the code it covers
# (they are in clippy::style; `warnings` covers every lint), and the Cargo.toml
# and .cargo settings that do the same for a whole crate.
LINT_OFF = re.compile(r"\b(?:allow|expect)\s*\(([^)]*)\)", re.S)
LINTS_OFF = re.compile(r"\b(?:clippy\s*::\s*(?:disallowed_\w+|style|all)|warnings)\b")
CARGO_FLAGS_OFF = re.compile(r"cap-lints|(?:^|[\s\"'=\[,])(?:-A|--allow)", re.M)
CLIPPY_OFF_KEYS = re.compile(r"(?:disallowed[-_]\w+|style|all)\Z")
# Clippy reads the file CLIPPY_CONF_DIR points at, else .clippy.toml before
# clippy.toml in the crate's directory.
CLIPPY_CONF_DIR = re.compile(r"\bCLIPPY_CONF_DIR\b")


def _cargo_lints_off(toml_path, table):
    """The clippy lints a Cargo.toml table ("lints" or "workspace.lints") sets
    to allow."""
    try:
        with open(toml_path, "rb") as f:
            d = tomllib.load(f)
    except (OSError, tomllib.TOMLDecodeError):
        return [], False
    for k in table.split("."):
        d = d.get(k, {}) if isinstance(d, dict) else {}
    off = []
    for k, v in (d.get("clippy", {}) or {}).items():
        level = v.get("level") if isinstance(v, dict) else v
        if level == "allow" and CLIPPY_OFF_KEYS.match(k):
            off.append(f"clippy::{k}")
    return off, d.get("workspace") is True


def lints_kept_on(crate):
    """The crate does not turn its disallowed-* lints off: no allow or expect
    of them in its Rust files, its Cargo.toml lints, the workspace lints it
    takes, or a .cargo config of the crate or the root."""
    bad = []
    for d, dirs, files in os.walk(crate):
        dirs[:] = sorted(x for x in dirs if x not in ("target", ".git"))
        for f in sorted(files):
            if not f.endswith(".rs"):
                continue
            p = os.path.join(d, f)
            text = open(p, encoding="utf-8", errors="replace").read()
            for m in LINT_OFF.finditer(text):
                if LINTS_OFF.search(m.group(1)):
                    n = text.count("\n", 0, m.start()) + 1
                    bad.append(f"{p}:{n}: turns the disallowed lints off: {' '.join(m.group(0).split())}")
    off, from_workspace = _cargo_lints_off(os.path.join(crate, "Cargo.toml"), "lints")
    if from_workspace:
        off += _cargo_lints_off("Cargo.toml", "workspace.lints")[0]
    for k in off:
        bad.append(f"{crate}/Cargo.toml: {k} is allowed by its lints")
    for c in sorted({os.path.normpath(os.path.join(crate, ".cargo")), ".cargo"}):
        for name in ("config.toml", "config"):
            p = os.path.join(c, name)
            if not os.path.isfile(p):
                continue
            text = open(p, encoding="utf-8", errors="replace").read()
            if CARGO_FLAGS_OFF.search(text):
                bad.append(f"{p}: lowers lint levels (-A, --allow or --cap-lints)")
            if CLIPPY_CONF_DIR.search(text):
                bad.append(f"{p}: sets CLIPPY_CONF_DIR, so clippy may read another config")
    return bad


def clippy_config(conf, req):
    try:
        with open(conf, "rb") as f:
            d = tomllib.load(f)
    except (OSError, tomllib.TOMLDecodeError) as e:
        print(f"clippy-config: FAILED: cannot read {conf}: {e}")
        return 1
    have = {}
    for t in ("disallowed-types", "disallowed-methods"):
        have[t] = {e if isinstance(e, str) else e.get("path") for e in d.get(t, [])}
    bad = 0
    try:
        lines = open(req, encoding="utf-8").read().split("\n")
    except OSError as e:
        print(f"clippy-config: FAILED: cannot read {req}: {e.strerror}")
        return 1
    count = 0
    for n, line in enumerate(lines, 1):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split()
        if len(parts) != 2 or parts[0] not in have:
            print(f"clippy-config: {req}:{n}: cannot read {line!r}")
            bad += 1
            continue
        count += 1
        if parts[1] not in have[parts[0]]:
            print(f"clippy-config: {conf} no longer has {parts[1]} in {parts[0]}")
            bad += 1
    if count == 0:
        print(f"clippy-config: {req} lists nothing")
        bad += 1
    crate = os.path.dirname(conf) or "."
    shadow = os.path.join(crate, ".clippy.toml")
    if os.path.basename(conf) == "clippy.toml" and os.path.lexists(shadow):
        print(f"clippy-config: {shadow}: clippy reads it instead of {conf}")
        bad += 1
    if "CLIPPY_CONF_DIR" in os.environ:
        print("clippy-config: CLIPPY_CONF_DIR is set, so clippy may read another config")
        bad += 1
    for b in lints_kept_on(crate):
        print(f"clippy-config: {b}")
        bad += 1
    if bad:
        print(f"clippy-config: FAILED: {bad} problem(s)")
        return 1
    print(f"clippy-config: ok ({conf} keeps all {count} entries of {req}, and its crate turns none of the lints off)")
    return 0


def _no_duplicates(pairs):
    keys = [k for k, _ in pairs]
    if len(set(keys)) != len(keys):
        raise ValueError("duplicate key")
    return dict(pairs)


def _strings(v):
    if isinstance(v, str):
        yield v
    elif isinstance(v, dict):
        for x in v.values():
            yield from _strings(x)
    elif isinstance(v, list):
        for x in v:
            yield from _strings(x)


def vectors(d):
    """docs/sync/spec/README.md §7: JSON files, one family each; a top-level
    object with "spec" ("seal.md §4") and a non-empty "cases" array; each case
    an object with a "name", its inputs and outputs; byte strings lowercase hex."""
    if not os.path.isdir(d):
        print(f"vectors: FAILED: {d} does not exist")
        return 1
    files = sorted(f for f in os.listdir(d) if f.endswith(".json"))
    if not files:
        print(f"vectors: FAILED: no .json file in {d}")
        return 1
    bad = []
    for f in files:
        p = os.path.join(d, f)
        try:
            with open(p, encoding="utf-8") as fh:
                doc = json.load(fh, object_pairs_hook=_no_duplicates)
        except (OSError, ValueError) as e:
            bad.append(f"{p}: not strict JSON: {e}")
            continue
        if not isinstance(doc, dict):
            bad.append(f"{p}: the top level is not an object")
            continue
        spec = doc.get("spec")
        if not isinstance(spec, str) or not re.fullmatch(r"[a-z]+\.md §\d+(\.\d+)*", spec):
            bad.append(f'{p}: "spec" must name a spec and section, such as "seal.md §4"')
        cases = doc.get("cases")
        if not isinstance(cases, list) or not cases:
            bad.append(f'{p}: "cases" must be a non-empty array')
            continue
        names = set()
        for i, c in enumerate(cases):
            name = c.get("name") if isinstance(c, dict) else None
            if not isinstance(name, str) or not name:
                bad.append(f'{p}: case {i} is not an object with a "name"')
                continue
            if name in names:
                bad.append(f"{p}: case name {name!r} appears twice")
            names.add(name)
            if len(c) < 2:
                bad.append(f"{p}: case {name!r} has no inputs or outputs")
            for s in _strings(c):
                if len(s) >= 2 and re.fullmatch(r"[0-9a-fA-F]+", s) and re.search(r"[A-F]", s):
                    bad.append(f"{p}: case {name!r}: hex {s[:24]!r} is not lowercase")
    for b in bad:
        print("vectors: " + b)
    if bad:
        print(f"vectors: FAILED: {len(bad)} problem(s)")
        return 1
    print(f"vectors: ok ({len(files)} file(s) in {d})")
    return 0


def npm_no_deps(p):
    try:
        with open(p, encoding="utf-8") as f:
            d = json.load(f)
    except (OSError, ValueError) as e:
        print(f"npm-no-deps: FAILED: cannot read {p}: {e}")
        return 1
    if not isinstance(d, dict):
        print(f"npm-no-deps: FAILED: {p} is not a JSON object")
        return 1
    # Every field npm installs for a package that is used. devDependencies are
    # installed only to develop the package itself.
    bad = [k for k in ("dependencies", "optionalDependencies", "peerDependencies",
                       "bundleDependencies", "bundledDependencies") if d.get(k)]
    if bad:
        print(f"npm-no-deps: FAILED: {p} has {', '.join(bad)}")
        return 1
    print(f"npm-no-deps: ok ({p} has no runtime dependency)")
    return 0


# The source files the scans of gate-lib.sh read (see there).
SCAN_EXTS = (".rs", ".js", ".mjs", ".cjs", ".ts", ".html", ".kt", ".kts", ".java")
JS_EXTS = (".js", ".mjs", ".cjs", ".ts", ".html")


def scan(pattern, *paths):
    """Prints path:line:text for every line where the pattern starts a match.
    The pattern is matched against whole files, so a match may span lines."""
    rx = js_rx = re.compile(pattern)
    if paths[:1] == ("--js",) and len(paths) >= 2:
        js_rx = re.compile(f"(?:{pattern})|(?:{paths[1]})")
        paths = paths[2:]
    leave_out = tuple(os.path.normpath(p[1:]) + "/" for p in paths if p.startswith("!"))
    files = []
    for p in (p for p in paths if not p.startswith("!")):
        if os.path.isfile(p):
            files.append(p)
        for d, dirs, names in os.walk(p):
            dirs[:] = sorted(x for x in dirs if not speclib.is_output_dir(d, x))
            files += [os.path.join(d, n) for n in sorted(names)]
    found = 0
    for f in files:
        rel = os.path.normpath(f)
        if not rel.endswith(SCAN_EXTS) or rel.startswith(leave_out):
            continue
        try:
            with open(f, "rb") as fh:
                data = fh.read()
        except OSError:
            continue
        if b"\0" in data[:8192]:
            continue
        text = data.decode("utf-8", "replace")
        lines = text.split("\n")
        seen = set()
        for m in (js_rx if rel.endswith(JS_EXTS) else rx).finditer(text):
            n = text.count("\n", 0, m.start()) + 1
            if n not in seen:
                seen.add(n)
                print(f"{rel}:{n}:{lines[n - 1]}")
                found += 1
    return 1 if found else 0


def main(argv):
    if argv[:1] == ["scan"] and len(argv) >= 3:
        return scan(*argv[1:])
    checks = {
        "workspace-sanity": (workspace_sanity, 0),
        "clippy-config": (clippy_config, 2),
        "vectors": (vectors, 1),
        "npm-no-deps": (npm_no_deps, 1),
    }
    if len(argv) < 1 or argv[0] not in checks or len(argv) - 1 != checks[argv[0]][1]:
        print(__doc__.strip(), file=sys.stderr)
        return 2
    fn, _ = checks[argv[0]]
    return fn(*argv[1:])


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
