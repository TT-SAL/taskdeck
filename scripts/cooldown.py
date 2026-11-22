"""The seven-day dependency cooldown. Run it through scripts/cooldown.sh.

Every crates.io package (name, version) in the working tree's Cargo.lock that
is absent from the base revision's Cargo.lock must have been published at least
seven days (604800 seconds) before now, must not be yanked, and must exist in
the crates.io index. Publish times come from the sparse index's "pubtime" field.
Git and path packages are not checked here: cargo-deny's sources check owns
those. Any other source that is not crates.io written exactly as cargo writes
it is a violation, never a skip: cargo accepts variants of the crates.io URL
(a trailing slash, ".git", other letter case) and cargo-deny passes them.

Network access goes through curl, which honours the proxy environment.
Standard library only; run with `python3 -I`.
"""

import argparse
import datetime as dt
import json
import os
import re
import subprocess
import sys
import tempfile
import tomllib

COOLDOWN_SECONDS = 7 * 24 * 60 * 60
CRATES_IO_SOURCES = {
    "registry+https://github.com/rust-lang/crates.io-index",
    "sparse+https://index.crates.io/",
}
INDEX_URL = "https://index.crates.io/"
EXEMPTION_DATE = re.compile(r"^\d{4}-\d{2}-\d{2}$")


class Fatal(Exception):
    pass


def parse_time(text, what):
    try:
        t = dt.datetime.fromisoformat(text.strip().replace("Z", "+00:00"))
    except ValueError:
        raise Fatal(f"{what}: not an RFC 3339 time: {text!r}")
    if t.tzinfo is None:
        raise Fatal(f"{what}: RFC 3339 time needs an offset or Z: {text!r}")
    return t.astimezone(dt.timezone.utc)


def crates_io_packages(lock_text, what):
    """(name, version) pairs from crates.io, the count of git and path
    packages, and the (name, version, source) of every other package."""
    try:
        lock = tomllib.loads(lock_text)
    except tomllib.TOMLDecodeError as e:
        raise Fatal(f"{what}: cannot parse: {e}")
    found, other, unknown = set(), 0, []
    for pkg in lock.get("package", []):
        source = pkg.get("source")
        if source in CRATES_IO_SOURCES:
            found.add((pkg["name"], pkg["version"]))
        elif source is None or source.startswith("git+"):
            other += 1
        else:
            unknown.append((pkg["name"], pkg["version"], source))
    return found, other, unknown


def index_path(name):
    n = name.lower()
    if len(n) == 1:
        return f"1/{n}"
    if len(n) == 2:
        return f"2/{n}"
    if len(n) == 3:
        return f"3/{n[0]}/{n}"
    return f"{n[0:2]}/{n[2:4]}/{n}"


def git(repo, *args):
    r = subprocess.run(["git", "-C", repo, *args], capture_output=True, text=True)
    if r.returncode != 0:
        raise Fatal(f"git {' '.join(args)}: {r.stderr.strip() or 'failed'}")
    return r.stdout


def base_lock_text(repo, base):
    if base is None:
        try:
            git(repo, "rev-parse", "--verify", "--quiet", "origin/day-planner^{commit}")
        except Fatal:
            raise Fatal(
                "origin/day-planner is not available here: fetch it "
                "(git fetch origin day-planner) or pass --base <rev>"
            )
        base = git(repo, "merge-base", "HEAD", "origin/day-planner").strip()
    else:
        git(repo, "rev-parse", "--verify", "--quiet", f"{base}^{{commit}}")
    r = subprocess.run(
        ["git", "-C", repo, "show", f"{base}:Cargo.lock"], capture_output=True, text=True
    )
    if r.returncode != 0:
        raise Fatal(f"no Cargo.lock at base revision {base}: {r.stderr.strip()}")
    return base, r.stdout


def load_index_local(index_dir, names):
    files = {}
    for name in names:
        p = os.path.join(index_dir, index_path(name))
        if os.path.isfile(p):
            with open(p, encoding="utf-8") as f:
                files[name] = f.read()
        else:
            files[name] = None
    return files


def load_index_curl(names):
    """Fetch index files with one parallel curl run. None means 404."""
    files = {}
    names = sorted(names)
    if not names:
        return files
    with tempfile.TemporaryDirectory() as tmp:
        cfg_lines, outputs = [], {}
        for i, name in enumerate(names):
            out = os.path.join(tmp, f"{i}.idx")
            outputs[name] = out
            cfg_lines.append(f'url = "{INDEX_URL}{index_path(name)}"')
            cfg_lines.append(f'output = "{out}"')
        cfg = os.path.join(tmp, "curl.cfg")
        with open(cfg, "w", encoding="utf-8") as f:
            f.write("\n".join(cfg_lines) + "\n")
        r = subprocess.run(
            [
                "curl", "--proto", "=https", "--tlsv1.2", "-sS", "--retry", "3",
                "--parallel", "--parallel-max", "8",
                "-A", "taskdeck-cooldown",
                "-w", "%{http_code} %{filename_effective}\\n",
                "-K", cfg,
            ],
            capture_output=True, text=True,
        )
        codes = {}
        for line in r.stdout.splitlines():
            code, _, fname = line.partition(" ")
            codes[fname] = code
        for name, out in outputs.items():
            code = codes.get(out)
            if code == "200" and os.path.isfile(out):
                with open(out, encoding="utf-8") as f:
                    files[name] = f.read()
            elif code in ("404", "410"):
                files[name] = None
            else:
                raise Fatal(
                    f"index fetch for {name} failed (HTTP {code or 'none'}): "
                    f"{r.stderr.strip()}"
                )
    return files


def read_exemptions(path, now):
    """{(name, version): reason}, or raises Fatal on any bad or expired line."""
    exemptions, errors = {}, []
    if not os.path.exists(path):
        return exemptions
    with open(path, encoding="utf-8") as f:
        for n, raw in enumerate(f, 1):
            line = raw.strip()
            if not line or line.startswith("#"):
                continue
            parts = line.split(None, 3)
            if len(parts) < 4 or not parts[3].strip():
                errors.append(f"{path}:{n}: needs name, version, expiry date and a reason")
                continue
            name, version, expiry, reason = parts
            if not EXEMPTION_DATE.match(expiry):
                errors.append(f"{path}:{n}: expiry {expiry!r} is not YYYY-MM-DD")
                continue
            try:
                day = dt.date.fromisoformat(expiry)
            except ValueError:
                errors.append(f"{path}:{n}: expiry {expiry!r} is not a date")
                continue
            # Valid through the whole expiry day, in UTC.
            if now.date() > day:
                errors.append(f"{path}:{n}: exemption for {name} {version} expired on {expiry}")
                continue
            exemptions[(name, version)] = reason
    if errors:
        raise Fatal("bad exemptions file:\n  " + "\n  ".join(errors))
    return exemptions


def main(argv):
    ap = argparse.ArgumentParser(prog="cooldown.sh", description=__doc__.split("\n\n")[0])
    ap.add_argument("--base", help="base revision (default: merge base of HEAD and origin/day-planner)")
    ap.add_argument("--all", action="store_true", help="check every crates.io package, not only new ones")
    ap.add_argument("--now", help="the time of the check, RFC 3339 (default: now)")
    ap.add_argument("--index-dir", help="read index files from this directory (sparse-index layout)")
    ap.add_argument("--base-lockfile", help="compare with this lockfile instead of a revision (tests)")
    ap.add_argument("--lockfile", help="the lockfile to check (default: Cargo.lock at the repository root)")
    ap.add_argument("--exemptions", help="exemptions file (default: scripts/cooldown-exemptions.txt)")
    ap.add_argument("--repo", help="repository root (default: the one this script is in)")
    args = ap.parse_args(argv)

    here = os.path.dirname(os.path.abspath(__file__))
    repo = args.repo or os.path.dirname(here)
    lockfile = args.lockfile or os.path.join(repo, "Cargo.lock")
    exemptions_path = args.exemptions or os.path.join(here, "cooldown-exemptions.txt")
    now = parse_time(args.now, "--now") if args.now else dt.datetime.now(dt.timezone.utc)

    exemptions = read_exemptions(exemptions_path, now)

    with open(lockfile, encoding="utf-8") as f:
        current, other, unknown = crates_io_packages(f.read(), lockfile)

    if args.all:
        to_check = set(current)
        scope = "all"
    elif args.base_lockfile:
        with open(args.base_lockfile, encoding="utf-8") as f:
            before, _, _ = crates_io_packages(f.read(), args.base_lockfile)
        to_check = current - before
        scope = f"new since {args.base_lockfile}"
    else:
        base, text = base_lock_text(repo, args.base)
        before, _, _ = crates_io_packages(text, f"{base}:Cargo.lock")
        to_check = current - before
        scope = f"new since {base[:12]}"

    names = {name for name, _ in to_check}
    if args.index_dir:
        files = load_index_local(args.index_dir, names)
    else:
        files = load_index_curl(names)

    violations, exempted = [], []
    for name, version, source in sorted(unknown):
        violations.append(
            f"{name} {version}: source {source!r} is not crates.io as cargo writes it, "
            f"nor git or path; cannot check"
        )
    for name, version in sorted(to_check):
        text = files.get(name)
        if text is None:
            violations.append(f"{name} {version}: crate not found in the crates.io index")
            continue
        entry = None
        for line in text.splitlines():
            line = line.strip()
            if not line:
                continue
            try:
                e = json.loads(line)
            except ValueError:
                raise Fatal(f"index file for {name}: a line is not JSON")
            if e.get("vers") == version:
                entry = e
        if entry is None:
            violations.append(f"{name} {version}: version not found in the crates.io index")
            continue
        if entry.get("yanked"):
            violations.append(f"{name} {version}: yanked")
            continue
        pubtime = entry.get("pubtime")
        if not pubtime:
            violations.append(f"{name} {version}: the index gives no publish time")
            continue
        published = parse_time(pubtime, f"pubtime of {name} {version}")
        age = (now - published).total_seconds()
        if age < COOLDOWN_SECONDS:
            if (name, version) in exemptions:
                exempted.append(f"{name} {version}: exempt ({exemptions[(name, version)]})")
                continue
            days = age / 86400
            violations.append(
                f"{name} {version}: published {pubtime}, {days:.1f} days before the check; "
                f"needs 7"
            )

    for line in exempted:
        print(f"cooldown: note: {line}")
    for line in violations:
        print(f"cooldown: VIOLATION: {line}")
    print(
        f"cooldown: {len(to_check)} crates.io package(s) checked ({scope}), "
        f"{len(violations)} violation(s), {len(exempted)} exempted, "
        f"{other} git or path package(s) skipped; now = {now.strftime('%Y-%m-%dT%H:%M:%SZ')}"
    )
    return 1 if violations else 0


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except Fatal as e:
        print(f"cooldown: error: {e}", file=sys.stderr)
        sys.exit(2)
    except OSError as e:
        # A missing or unreadable lockfile, exemptions or index file means the
        # check could not run: exit 2, never 1 (which means violations).
        print(f"cooldown: error: {e}", file=sys.stderr)
        sys.exit(2)
