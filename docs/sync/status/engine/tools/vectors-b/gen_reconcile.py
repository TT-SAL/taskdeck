"""engine-reconcile.json: welcome checks, reconcile, conflicts and gaps,
engine.md §10.3, §10.4, §11.2, §11.3, §11.4.

Run: python3 gen_reconcile.py > crates/taskdeck-sync/tests/vectors/engine-reconcile.json
"""
import sys
sys.dont_write_bytecode = True  # no __pycache__ beside the generators

import common


def ch(n):
    """A made-up 32-byte chain value."""
    return f"{n:02x}" * 32


# ---------------------------------------------------------------- welcome
def welcome_case(name, a, has_record, records, L, local_chains, head, r, q,
                 a_after=None, unresolved=(), clauses=()):
    """local_chains: {seq: chain} for the seqs this device holds that the case needs
    (L and head.seq when head.seq <= L)."""
    a_eff = a if has_record else 0                                   # ENG-RECON-009
    framed = [x["ctr"] for x in records if x["frame"]]
    h = max([a_eff] + framed)                                        # §10.3, ENG-RECON-008
    found = []                                                       # kinds found, in order
    fork = None
    # §11.2 step 2.
    if head["seq"] < L:
        found.append("RelayBehind")
    if head["seq"] <= L and local_chains[str(head["seq"])] != head["chain"]:
        found.append("Fork")
        fork = {"kind": "Fork", "seq": head["seq"],
                "evidence": {"local_chain": local_chains[str(head["seq"])],
                             "relay_chain": head["chain"], "source": "Welcome"}}
    # §11.2 step 3.
    above = [x["ack"] for x in records if x["ack"] is not None and x["ack"][0] > head["seq"]]
    if above:
        found.append("RelayBehind")
    evidence_211 = bool(found)
    # §11.3 step 1 runs in every case.
    if r < a_eff:
        found.append("RelayBehind")
        step = 1
    elif r <= h:
        step = 2
    else:
        step = 3
    alarms = []
    if "RelayBehind" in found and "RelayBehind" not in unresolved:
        ack = max(above, key=lambda x: x[0]) if above else None
        alarms.append({"kind": "RelayBehind", "evidence": {
            "relay_head": head, "relay_last_ctr": r,
            "local_head": {"seq": L, "chain": local_chains[str(L)]}, "ack": ack}})
    if fork is not None and "Fork" not in unresolved:
        alarms.append(fork)
    evidence = bool(found)
    c = {"name": name, "kind": "welcome",
         "clauses": ["ENG-SESS-002", "ENG-SESS-008", "§11.2", "§11.3", "§10.3"] + list(clauses),
         "a": a, "has_record": has_record, "records": records, "L": L,
         "local_chains": local_chains, "welcome": {"head": head, "last_ctr": r, "last_seq": q},
         "unresolved_alarms": list(unresolved), "h": h, "evidence_found": sorted(set(found)),
         "alarms": alarms}
    if evidence:
        # "When any step found evidence, §11.3 runs only its step 1": no read, no
        # registry.restored, no w.
        c["step"] = 1 if step == 1 else None
        c["reconcile"] = "step_1_only"
        c["restored"] = None
        return c
    c["step"] = step
    c["reconcile"] = "full"
    c["seal_before_q"] = False   # step 2: "reads up to at least q before it seals anything new"; step 3: "seals nothing until the log reaches q"
    c["restored"] = [h + 1, r] if step == 3 else None
    if a_after is not None:
        c["a_after"] = a_after
        if step == 3 and a_after < r:
            c["withheld"] = True            # §11.3 step 3, ENG-RECON-004
        else:
            w = r if step == 3 else 0       # step 3: "next_ctr uses w = r"
            h_after = max([a_after] + framed)
            c["withheld"] = False
            c["next_ctr"] = 1 + max(a_after, h_after, w)
    return c


def rec(ctr, frame=True, ack=None):
    return {"ctr": ctr, "frame": frame, "ack": ack}


# ---------------------------------------------------------------- conflict
def conflict_case(name, seq, open_own, before):
    e = {"acknowledged": None, "alarm": None, "rehello": True}
    cleared = None
    inc = False
    if seq != 0 and open_own == "x":
        e["acknowledged"] = "x"
        after = 0
    elif seq != 0 and open_own == "other":
        e["acknowledged"] = "y"
        cleared = "x"
        after = 0
    elif seq != 0 and open_own == "none":
        e["alarm"] = "RestoredCopy"
        cleared = "x"
        inc = True
    elif seq != 0 and open_own == "error":
        e["alarm"] = "RelayProtocol"
        cleared = "x"
        inc = True
    else:
        cleared = "x"
        inc = True
    if inc:
        after = before + 1
    e["conflicts_after"] = after
    e["identity_clone"] = after >= 3
    if not e["identity_clone"]:
        # When conflicts reaches 3 "the engine stops with identity_clone instead of
        # re-sealing": whether x's frame is still cleared is not decided (README).
        e["cleared"] = cleared
    return {"name": name, "kind": "conflict",
            "clauses": ["ENG-RECON-005", "ENG-RECON-006", "§11.4"],
            "seq": seq, "open_own": open_own, "conflicts_before": before, "expected": e}


# ---------------------------------------------------------------- gap
def gap_case(name, r, q, records):
    behind = any(x["ack"] and x["ctr"] > r for x in records)
    e = {"relay_behind": behind}
    if not behind:
        assert any(x["ctr"] == r + 1 for x in records)
        e["resend_from_ctr"] = r + 1
    return {"name": name, "kind": "gap", "clauses": ["ENG-RECON-007", "§11.4", "§10.4"],
            "last_ctr": r, "last_seq": q, "records": records, "expected": e}


def main():
    cases = []
    lc = lambda *pairs: {str(s): c for s, c in pairs}  # noqa: E731
    H20 = {"seq": 20, "chain": ch(20)}
    cases += [
        welcome_case("normal: a <= r <= h", 5, True, [rec(6, ack=[22, ch(22)]), rec(7), rec(None, frame=False)],
                     20, lc((20, ch(20))), {"seq": 25, "chain": ch(25)}, 7, 25, a_after=7),
        welcome_case("normal: r = a and no record holds a frame", 5, True, [rec(None, frame=False)],
                     20, lc((20, ch(20))), H20, 5, 20, a_after=5, clauses=["ENG-RECON-008"]),
        welcome_case("normal: r between a and h; frames above r are resent", 5, True, [rec(6), rec(7), rec(8)],
                     20, lc((20, ch(20))), H20, 6, 20, a_after=6),
        welcome_case("relay head above L: a read follows", 5, True, [rec(6)],
                     20, lc((20, ch(20))), {"seq": 25, "chain": ch(25)}, 6, 25, a_after=6),
        welcome_case("r < a: relay_behind (§11.3 step 1)", 9, True, [],
                     20, lc((20, ch(20))), H20, 8, 20),
        welcome_case("head below L: relay_behind; the chain at head.seq is compared too", 5, True, [],
                     20, lc((20, ch(20)), (18, ch(18))), {"seq": 18, "chain": ch(18)}, 5, 18),
        welcome_case("head below L and a different chain there: relay_behind and fork", 5, True, [],
                     20, lc((20, ch(20)), (18, ch(18))), {"seq": 18, "chain": ch(99)}, 5, 18),
        welcome_case("head = L with a different chain: fork only; no reconcile beyond step 1", 5, True, [rec(6)],
                     20, lc((20, ch(20))), {"seq": 20, "chain": ch(77)}, 9, 20),
        welcome_case("an acknowledgement above the head: relay_behind with the largest such ack", 8, True,
                     [rec(6, ack=[19, ch(19)]), rec(7, ack=[22, ch(22)]), rec(8, ack=[23, ch(23)]), rec(9)],
                     23, lc((23, ch(23)), (21, ch(21))), {"seq": 21, "chain": ch(21)}, 8, 21),
        welcome_case("every relay_behind source at once raises one alarm", 10, True,
                     [rec(10, ack=[24, ch(24)])],
                     24, lc((24, ch(24)), (20, ch(20))), H20, 7, 20),
        welcome_case("an unresolved RelayBehind is held: no new alarm, still step 1 only", 9, True, [],
                     20, lc((20, ch(20))), H20, 8, 20, unresolved=["RelayBehind"]),
        welcome_case("r > h: a restored copy; nothing is sealed until the log reaches q", 5, True, [rec(6)],
                     20, lc((20, ch(20))), {"seq": 30, "chain": ch(30)}, 9, 30, a_after=9,
                     clauses=["ENG-RECON-001", "ENG-RECON-004"]),
        welcome_case("r > h and the log does not reach r: withheld", 5, True, [],
                     20, lc((20, ch(20))), {"seq": 30, "chain": ch(30)}, 9, 30, a_after=8,
                     clauses=["ENG-RECON-004"]),
        welcome_case("no record of this device in M_K: a = 0", 0, False, [],
                     12, lc((12, ch(12))), {"seq": 12, "chain": ch(12)}, 0, 0, a_after=0,
                     clauses=["ENG-RECON-009"]),
        welcome_case("no record of this device in M_K and the relay holds counters: restored", 0, False, [],
                     12, lc((12, ch(12))), {"seq": 15, "chain": ch(15)}, 3, 15, a_after=3,
                     clauses=["ENG-RECON-009"]),
        welcome_case("evidence and r > h: no registry.restored", 5, True, [],
                     20, lc((20, ch(20)), (18, ch(18))), {"seq": 18, "chain": ch(18)}, 9, 18),
    ]
    cases += [
        conflict_case("the relay holds x as sealed by an earlier copy", 41, "x", 1),
        conflict_case("the relay holds another record y", 41, "other", 2),
        conflict_case("no record's entry: restored_copy", 41, "none", 0),
        conflict_case("no record's entry, second in a row", 41, "none", 1),
        conflict_case("no record's entry, third in a row: identity_clone", 41, "none", 2),
        conflict_case("open_own fails: relay_protocol", 41, "error", 0),
        conflict_case("open_own fails, third in a row: identity_clone", 41, "error", 2),
        conflict_case("seq = 0: the relay no longer holds the counter", 0, "none", 0),
        conflict_case("seq = 0, third in a row: identity_clone", 0, "none", 2),
        conflict_case("a match resets the count", 41, "x", 2),
    ]
    R = lambda *spec: [{"ctr": c, "ack": a} for c, a in spec]  # noqa: E731
    cases += [
        gap_case("resend from the counter after last_ctr", 6, 30, R((5, True), (6, True), (7, False), (8, False))),
        gap_case("an ack above last_ctr: relay_behind", 5, 30, R((5, True), (6, True), (7, False))),
        gap_case("an ack two above last_ctr: relay_behind", 4, 30, R((5, False), (6, True))),
        gap_case("no ack at all: resend from last_ctr + 1", 4, 30, R((5, False), (6, False), (7, False))),
        gap_case("unacknowledged records the relay holds are skipped", 7, 30, R((5, True), (6, False), (7, False), (8, False))),
    ]
    common.emit({"spec": "engine.md §10.3, §11.2, §11.3, §11.4", "cases": cases})


if __name__ == "__main__":
    main()
