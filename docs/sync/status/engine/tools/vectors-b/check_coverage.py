"""Checks coverage the brief of task B asks for that a fix pass could lose (review of task B).

Run from the repository root: python3 docs/sync/status/engine/tools/vectors-b/check_coverage.py
It exits non-zero and names what is missing.

1. Receipts: the brief's Cover list asks for "own beat/snap/ctl refused". Each of the three kinds
   must have an own entry refused with a code that is not re-sealable (§24.3 second paragraph,
   ENG-REF-001, §13.1 Refuse row), so the case asserts only what the spec decides.
"""
import json
import sys

sys.dont_write_bytecode = True

VEC = "crates/taskdeck-sync/tests/vectors/"
RESEALABLE = ("seal/stale_epoch", "seal/old_epoch")


def receipts():
    seen = set()
    for c in json.load(open(VEC + "engine-receipts.json"))["cases"]:
        r = c["receipt"]
        if r["own"] and r["class"] == "Refuse" and r["code"] not in RESEALABLE and not r["in_skips"]:
            seen.add(r["kind"])
    return ["receipts: no own %s refused with a non-re-sealable code" % k
            for k in ("beat", "snap", "ctl") if k not in seen]


def main():
    bad = receipts()
    for b in bad:
        print(b)
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
