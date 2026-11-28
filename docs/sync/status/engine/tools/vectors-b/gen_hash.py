"""engine-hash.json: the state hash of engine.md §18.3 (ruling S7).

    state_hash = SHA-256( ctx("state") ‖ u32 hash_version ‖ board_hash ‖ SHA-256(membership bytes) )
    ctx("state") = "taskdeck/v1/state" followed by one 0x00 byte (18 bytes)

Run: python3 gen_hash.py > crates/taskdeck-sync/tests/vectors/engine-hash.json
"""
import sys
sys.dont_write_bytecode = True  # no __pycache__ beside the generators

import hashlib
import common
import struct

import toy

CTX_STATE = b"taskdeck/v1/state\x00"
assert len(CTX_STATE) == 18


def stream(label, n):
    """n deterministic bytes: SHA-256(label ‖ u32 i) blocks, concatenated."""
    out = b""
    i = 0
    while len(out) < n:
        out += hashlib.sha256(label + struct.pack(">I", i)).digest()
        i += 1
    return out[:n]


CLAUSES = ["ENG-HASH-001", "§18.3"]


def case(name, membership, board=None, board_hash=None, note=None):
    if board is not None:
        board_hash = hashlib.sha256(board).digest()
    inp = CTX_STATE + struct.pack(">I", 1) + board_hash + hashlib.sha256(membership).digest()
    assert len(inp) == 86
    c = {"name": name, "clauses": CLAUSES, "hash_version": 1}
    if board is not None:
        c["board"] = board.hex()
    c["board_hash"] = board_hash.hex()
    c["membership"] = membership.hex()
    c["membership_hash"] = hashlib.sha256(membership).hexdigest()
    c["input"] = inp.hex()
    c["state_hash"] = hashlib.sha256(inp).hexdigest()
    if note:
        c["note"] = note
    return c


def main():
    empty = toy.State()
    rich = toy.State(rules=2, keys={1: (-5, 9, 2), 200: (7, 12, 1)},
                     items={(1 << 40) + 3: 77, (2 << 40) + 1: 5}, marks={1: 3, 2: 1})
    cases = [
        case("empty membership bytes, the empty toy board", b"", board=empty.to_bytes(),
             note="the empty toy state of engine.md §5.3 (16 bytes); SHA-256 of zero membership bytes"),
        case("all-zero board hash, one membership byte", b"\x00", board_hash=bytes(32)),
        case("all-ones board hash, 255 counting membership bytes", bytes(range(1, 256)),
             board_hash=b"\xff" * 32),
        case("a toy board with keys, items and marks", stream(b"td-vec-membership-a", 100),
             board=rich.to_bytes()),
        case("membership of 55 bytes (one SHA-256 block after padding)",
             stream(b"td-vec-membership-55", 55), board_hash=stream(b"td-vec-board-55", 32)),
        case("membership of 56 bytes (two SHA-256 blocks after padding)",
             stream(b"td-vec-membership-56", 56), board_hash=stream(b"td-vec-board-56", 32)),
        case("membership of 64 bytes", stream(b"td-vec-membership-64", 64),
             board_hash=stream(b"td-vec-board-64", 32)),
        case("a long membership (10,000 bytes)", stream(b"td-vec-membership-long", 10000),
             board_hash=stream(b"td-vec-board-long", 32)),
    ]
    common.emit({"spec": "engine.md §18.3", "cases": cases})


if __name__ == "__main__":
    main()
