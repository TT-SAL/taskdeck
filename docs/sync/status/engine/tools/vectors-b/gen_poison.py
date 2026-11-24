"""engine-poison.json: markers, attempts and the crash halt, engine.md §15, §16.1, §16.2, D5, D6.

A small model of §15.2 (Start) and §15.3 (a caught panic), driven by a script. The
model refuses (AssertionError) any script that would reach a point the spec leaves
open (see the README of the vectors: the rebuild of §15.3 over a non-empty range,
and the rest of a page after a §15.3 single apply), so every case stays decidable.

Run: python3 gen_poison.py > crates/taskdeck-sync/tests/vectors/engine-poison.json
"""
import sys
sys.dont_write_bytecode = True  # no __pycache__ beside the generators

import common


class Killed(Exception):
    pass


class Halted(Exception):
    pass


class Model:
    def __init__(self, start, outcomes):
        self.attempt = start["attempt"]
        self.last_failed = tuple(start["last_failed"]) if start["last_failed"] else None
        m = start["marker"]
        self.marker = tuple(m) if m else None  # (first, last, attempt)
        self.ckpt = start["checkpoint_seq"]    # B at Start: the checkpoint's seq
        self.L = start["log_head"]
        self.board = self.ckpt
        self.outcomes = list(outcomes)
        self.events = []
        self.halted = False

    # ---- writes
    def ev(self, **kw):
        self.events.append(kw)

    def raise_attempt(self):
        self.attempt += 1
        self.ev(attempt=self.attempt)            # D5: persisted before any apply in it

    def set_lf(self, v):
        self.last_failed = v
        self.ev(last_failed=list(v) if v else None)  # D6

    def set_marker(self, f, l):
        self.marker = (f, l, self.attempt)
        self.ev(set_marker=[f, l, self.attempt])

    def clear_marker(self):
        self.marker = None
        self.ev(clear_marker=True)

    def halt(self, x):
        self.halted = True
        self.ev(halt=x, code="engine/crash_halt")
        raise Halted()

    # ---- applying
    def outcome(self):
        return self.outcomes.pop(0)

    def apply_one(self, s, mode):
        assert s == self.board + 1, (s, self.board)
        o = self.outcome()
        e = {"apply": s, "mode": mode, "outcome": o}
        if o == "kill":
            self.ev(**e)
            raise Killed()
        if o == "ok":
            # §15.3 last sentence: an entry that applies cleanly in single mode clears
            # last_failed when it names that entry.
            clears = mode == "single" and self.last_failed is not None and self.last_failed[0] == s
            if clears:
                e["clears_last_failed"] = True
            self.ev(**e)
            if clears:
                self.last_failed = None
            self.board = s
            return True
        self.ev(**e)
        return False  # panic

    def page(self, seqs):
        """Page mode: one marker over the page (ENG-POISON-001), ClearMarker after it."""
        if not seqs:
            return
        self.set_marker(seqs[0], seqs[-1])
        for i, s in enumerate(seqs):
            if not self.apply_one(s, "page"):
                assert i == len(seqs) - 1, "a page continuing after a §15.3 single apply is not decided"
                self.caught_panic(s)
                return
        self.clear_marker()

    def replay(self, seqs):
        """The page-wise replay of §15.2 at Start. §15.2 sets no page size for it (README
        question 7), so a case may only replay at most one entry this way."""
        assert len(seqs) <= 1, "the page size of the replay at Start is not decided"
        self.page(seqs)

    def single(self, s):
        self.set_marker(s, s)
        if self.apply_one(s, "single"):
            self.clear_marker()
        else:
            self.caught_panic(s)

    def caught_panic(self, x):
        a = self.attempt
        # §15.3 step 1.
        if self.last_failed == (x, a - 1):
            self.halt(x)
        # §15.3 step 2: last_failed (D6), attempt a + 1 (D5), rebuild at x - 1, x alone.
        self.set_lf((x, a))
        self.raise_attempt()
        assert self.ckpt == x - 1, "a rebuild over a non-empty range is not decided (markers)"
        self.board = self.ckpt
        self.single(x)

    def start(self):
        self.board = self.ckpt
        self.raise_attempt()                      # §15.2 step 1
        m = self.marker
        if m and m[1] > self.ckpt:                # §15.2 step 2
            first, last, matt = m
            if first == last:
                x = first
                if self.last_failed == (x, matt - 1):
                    assert self.ckpt == x - 1, "entries before the halting one: order not decided"
                    self.halt(x)
                self.set_lf((x, matt))
            self.replay(list(range(self.ckpt + 1, first)))
            for s in range(max(first, self.board + 1), last + 1):
                self.single(s)
            self.replay(list(range(last + 1, self.L + 1)))
        else:                                     # §15.2 step 3
            self.replay(list(range(self.ckpt + 1, self.L + 1)))

    def run(self, script):
        for step in script:
            try:
                if step == "start":
                    self.events.append({"step": "start"})
                    self.start()
                else:
                    seqs = step["page"]
                    self.events.append({"step": "page", "seqs": seqs})
                    assert seqs[0] == self.L + 1
                    self.L = seqs[-1]                 # AppendLog + SetMarker in one Persist
                    self.page(seqs)
            except Killed:
                pass
            except Halted:
                break
        assert not self.outcomes, "unused outcomes"
        return self.events


def case(name, start, script, outcomes, clauses):
    ev = Model(start, outcomes).run(script)
    halt = next((e["halt"] for e in ev if "halt" in e), None)
    return {"name": name, "clauses": ["ENG-POISON-001", "§15"] + clauses, "start": start,
            "script": script, "outcomes": outcomes, "events": ev, "halt": halt}


def st(attempt=0, last_failed=None, marker=None, ckpt=4, L=4):
    return {"attempt": attempt, "last_failed": last_failed, "marker": marker,
            "checkpoint_seq": ckpt, "log_head": L}


def main():
    cases = [
        case("a clean page", st(), ["start", {"page": [5, 6, 7]}], ["ok"] * 3,
             ["§15.1", "ENG-POISON-005"]),
        case("two clean pages", st(attempt=7), ["start", {"page": [5, 6]}, {"page": [7, 8, 9]}],
             ["ok"] * 5, ["§15.1"]),
        case("Start replays the entry above the checkpoint", st(attempt=2, L=5), ["start"],
             ["ok"], ["§15.2 step 3", "ENG-STATE-002"]),
        case("an OS kill in a page: single mode on the next start, no last_failed, no halt",
             st(), ["start", {"page": [5, 6, 7]}, "start"], ["ok", "kill", "ok", "ok", "ok"],
             ["§15.2 step 2", "ENG-POISON-002", "ENG-POISON-003"]),
        case("an OS kill in the second page: the first is replayed page-wise, the second entry by entry",
             st(), ["start", {"page": [5]}, {"page": [6, 7, 8]}, "start"],
             ["ok", "ok", "kill", "ok", "ok", "ok", "ok"],
             ["§15.2 step 2", "ENG-POISON-002"]),
        case("an OS kill in a page, then a kill on the same entry in single mode, then it applies",
             st(), ["start", {"page": [5, 6, 7]}, "start", "start"],
             ["ok", "kill", "ok", "kill", "ok", "ok", "ok"],
             ["§15.2 step 2", "ENG-POISON-003"]),
        case("a deterministic panic halts within one run after its second attempt",
             st(), ["start", {"page": [5]}], ["panic", "panic"],
             ["§15.3", "§16.1", "ENG-POISON-003", "ENG-POISON-004", "§7.12 D5", "§7.12 D6"]),
        case("a panic that does not repeat clears last_failed; the next page applies",
             st(), ["start", {"page": [5]}, {"page": [6, 7]}], ["panic", "ok", "ok", "ok"],
             ["§15.3", "ENG-POISON-004"]),
        case("a panic on the last entry of a page right after the checkpoint",
             st(attempt=3), ["start", {"page": [5]}], ["panic", "ok"],
             ["§15.3"]),
        case("a single-entry marker left by a kill, twice in a row: halt",
             st(attempt=2, marker=[6, 6, 2], ckpt=5, L=7), ["start", "start"], ["kill"],
             ["§15.2 step 2", "§16.1", "ENG-POISON-003", "§7.12 D6"]),
        case("a single-entry marker left by a kill once: last_failed set, then cleared",
             st(attempt=2, marker=[6, 6, 2], ckpt=5, L=7), ["start"], ["ok", "ok"],
             ["§15.2 step 2", "§15.3"]),
        case("a marker at or below the checkpoint is ignored",
             st(attempt=4, marker=[3, 4, 4], ckpt=4, L=5), ["start"], ["ok"],
             ["§15.2 step 3"]),
        case("a stale last_failed from a non-consecutive attempt does not halt",
             st(attempt=3, last_failed=[5, 1]), ["start", {"page": [5]}], ["panic", "ok"],
             ["§15.3", "ENG-POISON-003"]),
        case("a kill, then a panic on another entry: no halt",
             st(attempt=1, marker=[7, 7, 1], ckpt=5, L=7), ["start"], ["panic", "ok", "ok"],
             ["§15.2 step 2", "§15.3", "ENG-POISON-003"]),
        case("a kill, then a panic on another entry that repeats: halt on that entry",
             st(attempt=1, marker=[7, 7, 1], ckpt=5, L=7), ["start"], ["panic", "panic"],
             ["§15.2 step 2", "§15.3", "§16.1"]),
    ]
    common.emit({"spec": "engine.md §15, §16.1, §16.2", "cases": cases})


if __name__ == "__main__":
    main()
