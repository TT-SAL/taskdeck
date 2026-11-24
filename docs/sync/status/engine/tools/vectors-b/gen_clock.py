"""engine-clock.json: the clock guard, engine.md §27.1, §27.2.

Run: python3 gen_clock.py > crates/taskdeck-sync/tests/vectors/engine-clock.json
"""
import sys
sys.dont_write_bytecode = True  # no __pycache__ beside the generators

import common

FIVE_MIN, HOUR, DAY, WEEK = 300_000, 3_600_000, 86_400_000, 604_800_000
W, M = 1_800_000_000_000, 50_000_000_000


def guard(measurements, confirmation, now):
    # §27.1: skew = wall_ms - relay_now; the latest measurement counts, and it expires
    # after 86,400,000 ms of mono_ms without a new one.
    skew = None
    if measurements:
        last = measurements[-1]
        elapsed = now["mono_ms"] - last["mono_ms"]
        assert elapsed != DAY, "the exact expiry instant is not decided (see README)"
        if elapsed < DAY:
            skew = last["wall_ms"] - last["relay_now"]
    if skew is None:
        # "with no valid measurement there is neither warning nor block"
        return None, False, False, None
    warning = abs(skew) > FIVE_MIN
    valid_conf = None
    if confirmation is not None:
        c_wall, c_skew = confirmation["wall_ms"], confirmation["skew_ms"]
        valid_conf = now["wall_ms"] - c_wall <= WEEK and abs(skew - c_skew) <= FIVE_MIN
    blocked = abs(skew) > HOUR and not valid_conf
    return skew, warning, blocked, valid_conf


def case(name, measurements, confirmation, now, clauses):
    skew, warning, blocked, valid = guard(measurements, confirmation, now)
    c = {"name": name, "clauses": ["ENG-CLOCK-002", "ENG-CLOCK-003", "§27.1", "§27.2"] + clauses,
         "measurements": measurements, "confirmation": confirmation, "now": now,
         "skew_ms": skew, "warning": warning, "blocked": blocked}
    if confirmation is not None and skew is not None:
        c["confirmation_valid"] = valid
    return c


def meas(skew, wall=W, mono=M):
    return {"wall_ms": wall, "relay_now": wall - skew, "mono_ms": mono}


NOW = {"wall_ms": W + 60_000, "mono_ms": M + 60_000}


def main():
    cases = []
    for sign, word in ((1, "ahead"), (-1, "behind")):
        cases += [
            case(f"skew 300,000 {word}: no warning", [meas(sign * FIVE_MIN)], None, NOW, []),
            case(f"skew 300,001 {word}: warning", [meas(sign * (FIVE_MIN + 1))], None, NOW, []),
            case(f"skew 3,600,000 {word}: warning, no block", [meas(sign * HOUR)], None, NOW, []),
            case(f"skew 3,600,001 {word}: warning and block", [meas(sign * (HOUR + 1))], None, NOW, []),
        ]
    cases += [
        case("no measurement: no warning, no block", [], None, NOW, []),
        case("zero skew", [meas(0)], None, NOW, []),
        case("the latest measurement counts", [meas(2 * HOUR, mono=M - 10), meas(1_000, mono=M)], None, NOW, []),
        case("an older large skew is replaced by a later small one; the old one would block",
             [meas(1_000, mono=M - 10), meas(-2 * HOUR, mono=M)], None, NOW, []),
        case("a measurement 86,399,999 ms of mono_ms old is still valid", [meas(2 * HOUR)], None,
             {"wall_ms": W + DAY, "mono_ms": M + DAY - 1}, []),
        case("a measurement 86,400,001 ms of mono_ms old has expired: neither warning nor block",
             [meas(2 * HOUR)], None, {"wall_ms": W + DAY, "mono_ms": M + DAY + 1}, []),
        case("expiry follows mono_ms, not wall_ms", [meas(2 * HOUR)], None,
             {"wall_ms": W + 3 * DAY, "mono_ms": M + 1_000}, []),
        case("a valid confirmation lifts the block; the warning stays", [meas(2 * HOUR)],
             {"wall_ms": W, "skew_ms": 2 * HOUR}, NOW, []),
        case("a confirmation exactly 604,800,000 ms old is still valid", [meas(2 * HOUR, mono=M + WEEK)],
             {"wall_ms": W, "skew_ms": 2 * HOUR}, {"wall_ms": W + WEEK, "mono_ms": M + WEEK + 10}, []),
        case("a confirmation 604,800,001 ms old is too old: blocked", [meas(2 * HOUR, mono=M + WEEK)],
             {"wall_ms": W, "skew_ms": 2 * HOUR}, {"wall_ms": W + WEEK + 1, "mono_ms": M + WEEK + 10}, []),
        case("the skew drifted exactly 300,000 from the confirmed one: still valid", [meas(2 * HOUR + FIVE_MIN)],
             {"wall_ms": W, "skew_ms": 2 * HOUR}, NOW, []),
        case("the skew drifted 300,001 from the confirmed one: blocked", [meas(2 * HOUR + FIVE_MIN + 1)],
             {"wall_ms": W, "skew_ms": 2 * HOUR}, NOW, []),
        case("the skew drifted 300,001 the other way: blocked", [meas(2 * HOUR - FIVE_MIN - 1)],
             {"wall_ms": W, "skew_ms": 2 * HOUR}, NOW, []),
        case("a confirmation of a negative skew", [meas(-2 * HOUR)],
             {"wall_ms": W - 1_000, "skew_ms": -2 * HOUR - 1_000}, NOW, []),
        case("a confirmation does not lift a warning; with a small skew nothing is blocked anyway",
             [meas(FIVE_MIN + 5)], {"wall_ms": W, "skew_ms": 2 * HOUR}, NOW, []),
        case("a confirmation with no valid measurement: neither warning nor block", [],
             {"wall_ms": W, "skew_ms": 2 * HOUR}, NOW, []),
    ]
    common.emit({"spec": "engine.md §27.1, §27.2", "cases": cases})


if __name__ == "__main__":
    main()
