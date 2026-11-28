"""engine-duties.json: duty order, beats and snapshots, engine.md §29.1, §29.2, §19.2, §19.3, §23.1.

Run: python3 gen_duties.py > crates/taskdeck-sync/tests/vectors/engine-duties.json
"""
import sys
sys.dont_write_bytecode = True  # no __pycache__ beside the generators

import common

DAY, WEEK, TWO_DAYS, HOUR = 86_400_000, 604_800_000, 172_800_000, 3_600_000


def dev(b):
    """A made-up 16-byte dev whose first byte is b and the rest a fixed pattern."""
    return bytes([b]).hex() + "a5" * 15


# ---------------------------------------------------------------- rank (§29.2)
def duty_order(devices):
    cur = [d for d in devices if d["status"] == "current"]
    # beat_ms descending, then dev ascending as unsigned bytes.
    cur.sort(key=lambda d: (-d["beat_ms"], bytes.fromhex(d["dev"])))
    return [d["dev"] for d in cur]


def rank_case(name, devices):
    order = duty_order(devices)
    return {"name": name, "kind": "rank", "clauses": ["ENG-DUTY-001", "§29.2"],
            "devices": devices, "order": order,
            "ranks": [{"dev": d, "rank": i} for i, d in enumerate(order)]}


# ---------------------------------------------------------------- beat (§23.1)
def beat_case(name, wall_ms, last_beat_ms, eligible, unsent_beat, build, record, pending_verdicts):
    conds = []
    if wall_ms - last_beat_ms >= DAY:
        conds.append(1)
    if build != record:
        conds.append(2)
    if pending_verdicts > 0 and wall_ms - last_beat_ms >= HOUR:
        conds.append(3)
    due = eligible and not unsent_beat and bool(conds)
    return {"name": name, "kind": "beat", "clauses": ["ENG-BEAT-001", "§23.1", "§29.1"],
            "wall_ms": wall_ms, "last_beat_ms": last_beat_ms, "eligible": eligible,
            "unsent_beat": unsent_beat, "build": build, "record": record,
            "pending_verdicts": pending_verdicts, "conditions": conds, "due": due}


LISTS = {"rules": [1], "envelopes": [[1, 1]], "hash_versions": [1]}


# ---------------------------------------------------------------- snapshot (§19.2, §19.3)
def usable(s):
    """§19.3: at least one Ack from a device other than its writer, and no Mismatch."""
    verdicts = [(v["dev"], v["verdict"]) for v in s["verdicts"]]
    if s["own"] is not None:
        verdicts.append((SELF, s["own"]))
    if any(v == "Mismatch" for _, v in verdicts):
        return False
    return any(v == "Ack" and d != s["author"] for d, v in verdicts)


def waiting(s):
    """§19.2 condition 1: still waiting for verdicts = no verdict at all."""
    return not s["verdicts"] and s["own"] is None


SELF = dev(0x20)
OTHER = dev(0x30)
THIRD = dev(0x90)


def snap_case(name, S, tau_S, snaps, own_snap_in_outbox, owed_mismatch, write=None, clauses=()):
    reasons = []
    own = [s for s in snaps if s["author"] == SELF]
    newest_own = max(own, key=lambda s: s["entry_seq"]) if own else None
    blocked = False
    if own_snap_in_outbox:
        reasons.append("own_snap_in_outbox")
        blocked = True
    if newest_own is not None and tau_S - newest_own["entry_clock"] < DAY:
        reasons.append("within_24_hours")
        blocked = True
    covered_usable = any(usable(s) and tau_S - s["entry_clock"] < WEEK and S - s["info_seq"] < 1000
                         for s in snaps)
    covered_waiting = any(waiting(s) and tau_S - s["entry_clock"] < TWO_DAYS for s in snaps)
    c1 = not covered_usable and not covered_waiting
    if covered_usable:
        reasons.append("recent_usable_snapshot")
    if covered_waiting:
        reasons.append("recent_snapshot_waiting_for_verdicts")
    if c1:
        reasons.append("condition_1")
    if owed_mismatch:
        reasons.append("condition_2")
    due = not blocked and (c1 or owed_mismatch)
    c = {"name": name, "kind": "snapshot",
         "clauses": ["ENG-SNAP-001", "ENG-SNAP-006", "§19.2", "§19.3"] + list(clauses),
         "self": SELF, "S": S, "tau_S": tau_S, "snaps": snaps,
         "own_snap_in_outbox": own_snap_in_outbox, "owed_mismatch": owed_mismatch,
         "due": due, "reasons": reasons}
    if write is not None:
        assert due
        wall_ms, rank, eligible, accepted = write
        earliest = tau_S + rank * HOUR
        c["write"] = {"wall_ms": wall_ms, "rank": rank, "eligible": eligible,
                      "snap_accepted_at_or_above_S": accepted,
                      "earliest_wall_ms": earliest,
                      "writes_now": eligible and wall_ms >= earliest and not accepted}
        c["clauses"] = c["clauses"] + ["ENG-SNAP-002"]
    return c


def snap(entry_seq, author, info_seq, entry_clock, verdicts=(), own=None):
    return {"entry_seq": entry_seq, "author": author, "info_seq": info_seq,
            "entry_clock": entry_clock,
            "verdicts": [{"dev": d, "verdict": v, "beat_seq": b} for d, v, b in verdicts],
            "own": own}


def main():
    cases = []
    # ---- rank
    cases.append(rank_case("sorted by beat_ms descending", [
        {"dev": dev(0x01), "status": "current", "beat_ms": 1_000},
        {"dev": dev(0x02), "status": "current", "beat_ms": 3_000},
        {"dev": dev(0x03), "status": "current", "beat_ms": 2_000}]))
    cases.append(rank_case("ties on beat_ms by dev ascending as unsigned bytes", [
        {"dev": dev(0x80), "status": "current", "beat_ms": 5_000},
        {"dev": dev(0x7f), "status": "current", "beat_ms": 5_000},
        {"dev": dev(0xff), "status": "current", "beat_ms": 5_000},
        {"dev": dev(0x00), "status": "current", "beat_ms": 4_999}]))
    cases.append(rank_case("revoked devices are left out", [
        {"dev": dev(0x10), "status": "revoked", "beat_ms": 9_000},
        {"dev": dev(0x11), "status": "current", "beat_ms": 8_000},
        {"dev": dev(0x12), "status": "current", "beat_ms": 0}]))
    cases.append(rank_case("devs that differ only in the last byte", [
        {"dev": "00" * 15 + "02", "status": "current", "beat_ms": 7},
        {"dev": "00" * 15 + "01", "status": "current", "beat_ms": 7},
        {"dev": "00" * 15 + "fe", "status": "current", "beat_ms": 8}]))
    cases.append(rank_case("one current device", [
        {"dev": dev(0x42), "status": "current", "beat_ms": 0},
        {"dev": dev(0x43), "status": "revoked", "beat_ms": 0}]))

    # ---- beat
    W = 1_800_000_000_000
    other = {"rules": [1, 2], "envelopes": [[1, 1]], "hash_versions": [1]}
    cases += [
        beat_case("a day since the last beat: due", W, W - DAY, True, False, LISTS, LISTS, 0),
        beat_case("one ms short of a day: not due", W, W - DAY + 1, True, False, LISTS, LISTS, 0),
        beat_case("first beat of a new replica (last_beat_ms 0)", W, 0, True, False, LISTS, LISTS, 0),
        beat_case("an update changed the build's rules: due at once", W, W - 1, True, False, other, LISTS, 0),
        beat_case("an update changed the build's envelopes", W, W - 1, True, False,
                  {"rules": [1], "envelopes": [[1, 1], [2, 1]], "hash_versions": [1]}, LISTS, 0),
        beat_case("an update changed the build's hash versions", W, W - 1, True, False,
                  {"rules": [1], "envelopes": [[1, 1]], "hash_versions": [1, 2]}, LISTS, 0),
        beat_case("a pending verdict and an hour since the last beat: due", W, W - HOUR, True, False, LISTS, LISTS, 1),
        beat_case("a pending verdict one ms short of an hour: not due", W, W - HOUR + 1, True, False, LISTS, LISTS, 2),
        beat_case("an hour but no pending verdict: not due", W, W - HOUR, True, False, LISTS, LISTS, 0),
        beat_case("not eligible: not due though a day has passed", W, W - 2 * DAY, False, False, LISTS, LISTS, 3),
        beat_case("an unsent beat record: not due", W, W - 2 * DAY, True, True, other, LISTS, 3),
        beat_case("all three conditions", W, W - DAY - 5, True, False, other, LISTS, 4),
    ]

    # ---- snapshot
    S, T = 64 * 40, 1_700_000_000_000
    cases += [
        snap_case("no snapshot known: due (condition 1)", S, T, [], False, False,
                  write=(T, 0, True, False)),
        snap_case("rank 2 waits two hours: one ms early", S, T, [], False, False,
                  write=(T + 2 * HOUR - 1, 2, True, False)),
        snap_case("rank 2 waits two hours: on time", S, T, [], False, False,
                  write=(T + 2 * HOUR, 2, True, False)),
        snap_case("due but not eligible: does not write", S, T, [], False, False,
                  write=(T + 5 * HOUR, 1, False, False), clauses=["§29.1"]),
        snap_case("due but a snap entry at or above S was accepted since: does not write", S, T, [], False, False,
                  write=(T + 5 * HOUR, 0, True, True)),
        snap_case("a usable snapshot 7 days old minus 1 ms: not due", S, T,
                  [snap(S - 10, OTHER, S - 64, T - WEEK + 1, [(THIRD, "Ack", S - 20)])], False, False),
        snap_case("a usable snapshot exactly 7 days old: due", S, T,
                  [snap(S - 10, OTHER, S - 64, T - WEEK, [(THIRD, "Ack", S - 20)])], False, False,
                  write=(T, 0, True, False)),
        snap_case("a usable snapshot 999 entries back: not due", S, T,
                  [snap(S - 10, OTHER, S - 999, T - HOUR, [(THIRD, "Ack", S - 20)])], False, False),
        snap_case("a usable snapshot 1,000 entries back: due", S, T,
                  [snap(S - 10, OTHER, S - 1000, T - HOUR, [(THIRD, "Ack", S - 20)])], False, False),
        snap_case("own verdict Ack makes another writer's snapshot usable: not due", S, T,
                  [snap(S - 10, OTHER, S - 64, T - DAY, [], own="Ack")], False, False),
        snap_case("an Ack only from its writer is not usable; it has a verdict, so it is not waiting: due", S, T,
                  [snap(S - 10, OTHER, S - 64, T - DAY, [(OTHER, "Ack", S - 5)])], False, False),
        snap_case("a Mismatch makes a snapshot not usable: due", S, T,
                  [snap(S - 10, OTHER, S - 64, T - DAY, [(THIRD, "Ack", S - 5), (dev(0x31), "Mismatch", S - 4)])],
                  False, False),
        snap_case("a snapshot waiting for verdicts, 2 days minus 1 ms old: not due", S, T,
                  [snap(S - 10, OTHER, S - 64, T - TWO_DAYS + 1)], False, False),
        snap_case("a snapshot waiting for verdicts, exactly 2 days old: due", S, T,
                  [snap(S - 10, OTHER, S - 64, T - TWO_DAYS)], False, False),
        snap_case("own snapshot 24 hours minus 1 ms of log clock ago: not due (24-hour limit)", S, T,
                  [snap(S - 10, SELF, S - 64, T - DAY + 1)], False, True),
        snap_case("own snapshot exactly 24 hours ago, waiting for verdicts: not due (condition 1 fails)", S, T,
                  [snap(S - 10, SELF, S - 64, T - DAY)], False, False),
        snap_case("own snapshot exactly 24 hours ago and an owed mismatch: due (condition 2)", S, T,
                  [snap(S - 10, SELF, S - 64, T - DAY)], False, True, write=(T + HOUR, 1, True, False)),
        snap_case("the newest own snapshot decides the 24-hour limit", S, T,
                  [snap(S - 300, SELF, S - 320, T - 3 * DAY), snap(S - 10, SELF, S - 64, T - HOUR)], False, True),
        snap_case("an own snap record in the outbox: not due", S, T, [], True, True),
        snap_case("an owed mismatch with a recent usable snapshot: due (condition 2)", S, T,
                  [snap(S - 10, OTHER, S - 64, T - HOUR, [(THIRD, "Ack", S - 5)])], False, True,
                  write=(T, 0, True, False)),
    ]
    common.emit({"spec": "engine.md §29.1, §29.2, §19.2, §19.3, §23.1", "cases": cases})


if __name__ == "__main__":
    main()
