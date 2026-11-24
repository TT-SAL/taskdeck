"""engine-pending.json: the pending view of engine.md §8.2, §8.3, §8.5 over the toy of §5.3.

Run: python3 gen_pending.py > crates/taskdeck-sync/tests/vectors/engine-pending.json
"""
import sys
sys.dont_write_bytecode = True  # no __pycache__ beside the generators

import common

import toy
from toy import cmd

ID = lambda dev, n: (dev << 40) + n  # noqa: E731  an item id: high part dev, low part n


def pending(confirmed, B, d, records):
    """§8.2 steps 1 to 4. Returns (view bytes, per-record results) or None per record list
    when d is None (the last paragraph of §8.2: the confirmed state alone)."""
    if d is None:
        return confirmed.to_bytes(), None
    # 1. U: the cmd records whose seen is absent or above B, in order.
    U = [r for r in sorted(records, key=lambda r: r["order"])
         if r["kind"] == "cmd" and (r["seen"] is None or r["seen"] > B)]
    # 2. P: a copy of the confirmed state.
    P = confirmed.copy()
    out = []
    for i, r in enumerate(U, start=1):
        seq = B + i
        based_on = min(r["based_on_seq"], B)
        c = toy.parse(bytes.fromhex(r["payload"]))
        trial = P.copy()
        try:
            notices = toy.apply(trial, c, seq, d, based_on)
        except toy.Refusal as e:
            # a refusal: doomed with the refusal's code; P is unchanged.
            out.append({"order": r["order"], "provisional_seq": seq, "doomed": e.code,
                        "notices": []})
            continue
        except toy.Panic:
            # a panic: doomed with engine/trial_panic; P is rebuilt from the confirmed
            # state by re-applying the records before it that were not doomed.
            P = confirmed.copy()
            for prev, res in zip(U[:i - 1], out):
                if res["doomed"] is None:
                    toy.apply(P, toy.parse(bytes.fromhex(prev["payload"])),
                              res["provisional_seq"], d, min(prev["based_on_seq"], B))
            out.append({"order": r["order"], "provisional_seq": seq, "doomed": "engine/trial_panic",
                        "notices": []})
            continue
        P = trial
        out.append({"order": r["order"], "provisional_seq": seq, "doomed": None,
                    "notices": [toy.notice_bytes(n).hex() for n in notices]})
    return P.to_bytes(), out


def rec(order, kind, payload=None, at_ms=1_700_000_000_000, based_on_seq=0, seen=None):
    return {"order": order, "kind": kind, "payload": payload.hex() if payload is not None else None,
            "at_ms": at_ms, "based_on_seq": based_on_seq, "seen": seen}


BASE = ["ENG-PEND-001", "§8.2", "§5.3"]


def case(name, confirmed, B, d, records, clauses, rules2=False, note=None):
    view, res = pending(confirmed, B, d, records)
    c = {"name": name, "clauses": BASE + clauses, "rules2": rules2,
         "confirmed": confirmed.to_bytes().hex(), "board_seq": B, "device_no": d,
         "records": records, "view": view.hex()}
    if res is not None:
        c["unapplied"] = res
    if note:
        c["note"] = note
    return c


def main():
    empty = toy.State()
    s1 = toy.State(keys={1: (10, 8, 2), 2: (5, 3, 1), 9: (0, 0, 0)},
                   items={ID(1, 1): 100, ID(2, 4): 200}, marks={1: 1, 2: 4})
    s2 = toy.State(rules=2, keys={3: (6, 4, 1)}, marks={})
    big = toy.State(keys={5: ((1 << 63) - 2, 6, 2)})
    above = toy.State(keys={7: (1, 12, 2)})
    cases = []

    cases.append(case("no record: the view is the confirmed state", s1, 10, 1, [], []))

    cases.append(case("successes: set, add, del, create, name, forget on top of each other", s1, 10, 1, [
        rec(1, "cmd", cmd("set", 4, 44), based_on_seq=10),
        rec(2, "cmd", cmd("add", 2, -7), based_on_seq=10),
        rec(3, "cmd", cmd("del", 9), based_on_seq=10),
        rec(4, "cmd", cmd("create", ID(1, 2), 300), based_on_seq=10),
        rec(5, "cmd", cmd("name", ID(1, 2), 301), based_on_seq=10),
        rec(6, "cmd", cmd("forget", ID(2, 4)), based_on_seq=10),
    ], ["§8.2 step 3"]))

    cases.append(case("benign repeats succeed: del of an absent key, forget of an absent item", s1, 10, 1, [
        rec(1, "cmd", cmd("del", 77), based_on_seq=10),
        rec(2, "cmd", cmd("forget", ID(1, 9)), based_on_seq=10),
        rec(3, "cmd", cmd("set", 77, 1), based_on_seq=10),
    ], ["§8.2 step 3"]))

    cases.append(case("a refusal (Fail) is doomed with its code and leaves the view unchanged", s1, 10, 1, [
        rec(1, "cmd", cmd("set", 4, 1), based_on_seq=10),
        rec(2, "cmd", cmd("fail", 3), based_on_seq=10),
        rec(3, "cmd", cmd("add", 4, 2), based_on_seq=10),
    ], ["ENG-PEND-002", "§8.3"]))

    cases.append(case("a Panic is doomed with engine/trial_panic; the view is rebuilt from the records before it", s1, 10, 1, [
        rec(1, "cmd", cmd("set", 4, 1), based_on_seq=10),
        rec(2, "cmd", cmd("name", ID(1, 1), 111), based_on_seq=10),
        rec(3, "cmd", cmd("panic"), based_on_seq=10),
        rec(4, "cmd", cmd("add", 4, 2), based_on_seq=10),
    ], ["ENG-PEND-002", "§8.2 step 3"]))

    cases.append(case("a Panic after a doomed record: the rebuild skips the doomed one", s1, 10, 1, [
        rec(1, "cmd", cmd("add", 50, 1), based_on_seq=10),
        rec(2, "cmd", cmd("set", 50, 9), based_on_seq=10),
        rec(3, "cmd", cmd("panic"), based_on_seq=10),
        rec(4, "cmd", cmd("add", 50, 1), based_on_seq=10),
    ], ["ENG-PEND-002", "§8.2 step 3"]))

    cases.append(case("a doomed record followed by one that succeeds; provisional seqs count the doomed one", s1, 10, 1, [
        rec(1, "cmd", cmd("name", ID(1, 7), 5), based_on_seq=10),
        rec(2, "cmd", cmd("set", 1, 99), based_on_seq=10),
    ], ["ENG-PEND-001", "§8.3"]))

    cases.append(case("a doomed Create; records that build on another pending record", s1, 10, 1, [
        rec(1, "cmd", cmd("create", ID(1, 1), 5), based_on_seq=10),
        rec(2, "cmd", cmd("create", ID(1, 2), 7), based_on_seq=10),
        rec(3, "cmd", cmd("name", ID(1, 2), 8), based_on_seq=10),
        rec(4, "cmd", cmd("name", ID(1, 3), 9), based_on_seq=10),
    ], ["§8.2 step 3", "§8.3"],
        note="record 1 re-mints n = 1, not above device 1's mark 1: doomed; record 3 names the item record 2 created; record 4 names an item no record created"))

    cases.append(case("records already seen at or below B are excluded; one seen above B is kept", s1, 10, 1, [
        rec(1, "cmd", cmd("set", 4, 1), based_on_seq=8, seen=9),
        rec(2, "cmd", cmd("set", 4, 2), based_on_seq=9, seen=10),
        rec(3, "cmd", cmd("set", 4, 3), based_on_seq=10, seen=11),
        rec(4, "cmd", cmd("add", 4, 10), based_on_seq=10),
    ], ["§8.2 step 1"]))

    cases.append(case("beat, ctl and snap records are excluded and take no provisional seq", s1, 10, 1, [
        rec(1, "beat"),
        rec(2, "cmd", cmd("set", 4, 1), based_on_seq=10),
        rec(3, "ctl"),
        rec(4, "snap"),
        rec(5, "cmd", cmd("add", 4, 1), based_on_seq=10),
    ], ["§8.2 step 1"]))

    cases.append(case("a Set over another device's newer stamp shows a notice (never recorded)", s1, 10, 1, [
        rec(1, "cmd", cmd("set", 1, 3), based_on_seq=5),
    ], ["§8.2 step 3", "ENG-SM-004", "§13.4"],
        note="key 1 stamped (seq 8, device 2); based_on 5 < 8 and device 2 is not this device"))

    cases.append(case("the same Set composed after seeing the stamp shows no notice", s1, 10, 1, [
        rec(1, "cmd", cmd("set", 1, 3), based_on_seq=9),
    ], ["§8.2 step 3"]))

    cases.append(case("a Set over this device's own stamp, or a stamp of device 0, shows no notice", s1, 10, 1, [
        rec(1, "cmd", cmd("set", 2, 3), based_on_seq=1),
        rec(2, "cmd", cmd("set", 9, 3), based_on_seq=0),
    ], ["§8.2 step 3"]))

    cases.append(case("based_on above B is taken as B: no toy_bad_ctx for the first record", s1, 10, 1, [
        rec(1, "cmd", cmd("set", 1, 3), based_on_seq=15),
    ], ["§8.2 step 3"],
        note="based_on = min(15, 10) = 10 < provisional seq 11; with 15 the toy would refuse with toy_bad_ctx"))

    cases.append(case("based_on = min(r.based_on.seq, B) decides a Set notice", above, 10, 1, [
        rec(1, "cmd", cmd("set", 30, 1), based_on_seq=10),
        rec(2, "cmd", cmd("set", 30, 2), based_on_seq=10),
        rec(3, "cmd", cmd("set", 30, 3), based_on_seq=10),
        rec(4, "cmd", cmd("set", 30, 4), based_on_seq=10),
        rec(5, "cmd", cmd("set", 30, 5), based_on_seq=10),
        rec(6, "cmd", cmd("set", 7, 9), based_on_seq=15),
    ], ["§8.2 step 3"],
        note="key 7 carries a stamp seq 12 above B (a state the toy loads); the 6th record has seq 16 and based_on min(15, 10) = 10 < 12, so it shows a notice; with based_on 15 it would not"))

    cases.append(case("a device with no device_no shows the confirmed state alone", s1, 10, None, [
        rec(1, "cmd", cmd("set", 4, 1), based_on_seq=10),
        rec(2, "cmd", cmd("create", ID(1, 2), 3), based_on_seq=10),
    ], ["§8.2 last paragraph"],
        note="the spec gives the view only; provisional seqs and doomed flags of such a device are not stated, so the case has no 'unapplied'"))

    cases.append(case("a Create whose id depends on the device's mark", s1, 10, 2, [
        rec(1, "cmd", cmd("create", ID(2, 4), 1), based_on_seq=10),
        rec(2, "cmd", cmd("create", ID(2, 5), 2), based_on_seq=10),
        rec(3, "cmd", cmd("create", ID(2, 5), 3), based_on_seq=10),
        rec(4, "cmd", cmd("create", ID(1, 9), 4), based_on_seq=10),
        rec(5, "cmd", cmd("create", ID(2, 9), 5), based_on_seq=10),
    ], ["§5.3 apply Create"],
        note="device 2's mark is 4: n = 4 is refused, 5 succeeds and raises the mark, 5 again is refused, a high part of 1 is refused, 9 succeeds"))

    cases.append(case("a Create on a device without a mark", empty, 3, 7, [
        rec(1, "cmd", cmd("create", ID(7, 1), 70), based_on_seq=3),
        rec(2, "cmd", cmd("create", ID(7, 0), 71), based_on_seq=3),
    ], ["§5.3 apply Create"]))

    cases.append(case("Mul under rules 2, Add overflow, Add on an absent key", s2, 20, 1, [
        rec(1, "cmd", cmd("mul", 3, -4), based_on_seq=20),
        rec(2, "cmd", cmd("add", 3, -(1 << 63)), based_on_seq=20),
        rec(3, "cmd", cmd("add", 3, -(1 << 63) + 30), based_on_seq=20),
        rec(4, "cmd", cmd("add", 8, 1), based_on_seq=20),
    ], ["§5.3 apply"], rules2=True))

    cases.append(case("Mul under rules 1 is doomed with toy_rules; overflow of Add", big, 6, 2, [
        rec(1, "cmd", cmd("mul", 5, 2), based_on_seq=6),
        rec(2, "cmd", cmd("add", 5, 1), based_on_seq=6),
        rec(3, "cmd", cmd("add", 5, 1), based_on_seq=6),
    ], ["§5.3 apply"]))

    cases.append(case("B = 1: the first provisional seq is 2", empty, 1, 1, [
        rec(1, "cmd", cmd("set", 1, 1), based_on_seq=1),
    ], ["§8.2 step 3", "§5.3 apply step 1"]))

    common.emit({"spec": "engine.md §8.2, §8.3, §8.5", "cases": cases})


if __name__ == "__main__":
    main()
