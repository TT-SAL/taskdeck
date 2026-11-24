"""Checks that the vectors leave out what the spec does not decide (review of task B).

Run from the repository root: python3 docs/sync/status/engine/tools/vectors-b/check_left_out.py
It exits non-zero and names the case when a vector asserts an undecided value.

1. Receipts: an own `beat`, `snap` or `ctl` refused with a re-sealable code (§24.3) must not
   carry an expected `refusal_record`: ENG-REF-001 and §13.3 exclude re-sealable codes, while the
   §13.1 Refuse row and §24.3 can be read otherwise (README question 3).
2. Poison: a `Start` must not replay more than one entry page-wise, since §15.2 sets no page
   size for that replay (README question 7). A `set_marker` written during a `start` step that
   covers more than one entry is only allowed for the single-entry markers of §15.2 step 2, so
   any multi-entry marker there is undecided.
"""
import json
import sys

sys.dont_write_bytecode = True

VEC = "crates/taskdeck-sync/tests/vectors/"
RESEALABLE = ("seal/stale_epoch", "seal/old_epoch")


def receipts():
    bad = []
    for c in json.load(open(VEC + "engine-receipts.json"))["cases"]:
        r = c["receipt"]
        if (r["own"] and r["class"] == "Refuse" and r["code"] in RESEALABLE
                and r["kind"] != "cmd" and "refusal_record" in c["expected"]):
            bad.append(c["name"])
    return bad


def poison():
    bad = []
    for c in json.load(open(VEC + "engine-poison.json"))["cases"]:
        in_start = False
        for e in c["events"]:
            if "step" in e:
                in_start = e["step"] == "start"
            elif "set_marker" in e and in_start:
                first, last, _ = e["set_marker"]
                if last > first:
                    bad.append("%s: %s" % (c["name"], e["set_marker"]))
    return bad


def main():
    bad = ["receipts: " + n for n in receipts()] + ["poison: " + n for n in poison()]
    for b in bad:
        print(b)
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
