"""The toy state machine of engine.md §5.3, written from the spec alone.

Shared by gen_pending.py (and the hash family, for a real board hash). Standard
library only. Every rule below cites the sentence of §5.3 it follows.
"""
import hashlib
import struct

TWO40 = 1 << 40
I64_MIN, I64_MAX = -(1 << 63), (1 << 63) - 1

OPS = {1: ("set", 10), 2: ("add", 10), 3: ("del", 2), 4: ("create", 13),
       5: ("name", 13), 6: ("forget", 9), 7: ("fail", 2), 8: ("panic", 1), 9: ("mul", 10)}


class Refusal(Exception):
    def __init__(self, code, op=None, target=None):
        super().__init__(code)
        self.code, self.op, self.target = code, op, target


class Panic(Exception):
    """`Panic`: calls `panic!` with a constant message (§5.3 apply step 3)."""


class State:
    def __init__(self, rules=1, keys=None, items=None, marks=None):
        self.rules = rules
        self.keys = dict(keys or {})      # key -> (value, stamp_seq, stamp_dev)
        self.items = dict(items or {})    # id -> name
        self.marks = dict(marks or {})    # device_no -> n

    def copy(self):
        return State(self.rules, self.keys, self.items, self.marks)

    def to_bytes(self):
        # §5.3 "Canonical bytes": magic, rules, keys, items, marks; big-endian.
        out = b"TOY1" + struct.pack(">I", self.rules)
        out += struct.pack(">H", len(self.keys))
        for k in sorted(self.keys):
            v, ss, sd = self.keys[k]
            out += struct.pack(">BqQH", k, v, ss, sd)
        out += struct.pack(">I", len(self.items))
        for i in sorted(self.items):
            out += struct.pack(">QI", i, self.items[i])
        out += struct.pack(">H", len(self.marks))
        for d in sorted(self.marks):
            out += struct.pack(">HQ", d, self.marks[d])
        return out


def board_hash(state):
    # §5.1 contract: board_hash = SHA-256(canonical_bytes(state)).
    return hashlib.sha256(state.to_bytes()).digest()


def cmd(op, *fields):
    """Encode a toy command (§5.3 "Commands": one u8 op, then the fields)."""
    if op in ("set", "add", "mul"):
        k, v = fields
        return struct.pack(">BBq", {"set": 1, "add": 2, "mul": 9}[op], k, v)
    if op == "del":
        return struct.pack(">BB", 3, fields[0])
    if op in ("create", "name"):
        i, n = fields
        return struct.pack(">BQI", {"create": 4, "name": 5}[op], i, n)
    if op == "forget":
        return struct.pack(">BQ", 6, fields[0])
    if op == "fail":
        return struct.pack(">BB", 7, fields[0])
    if op == "panic":
        return b"\x08"
    raise ValueError(op)


def parse(payload):
    # §5.3: refuses with engine/toy_parse an unknown op or a payload whose
    # length is not the op's width; accepts op 9 under any rules.
    if not payload or payload[0] not in OPS or len(payload) != OPS[payload[0]][1]:
        raise Refusal("engine/toy_parse")
    op = payload[0]
    name = OPS[op][0]
    if op in (1, 2, 9):
        k, v = struct.unpack(">Bq", payload[1:])
        return (name, k, v)
    if op == 3:
        return (name, payload[1])
    if op in (4, 5):
        i, n = struct.unpack(">QI", payload[1:])
        return (name, i, n)
    if op == 6:
        return (name, struct.unpack(">Q", payload[1:])[0])
    if op == 7:
        return (name, payload[1])
    return (name,)


def target(c):
    # §5.3: target is the key (as u64) or the id, and none for Fail and Panic.
    if c[0] in ("fail", "panic"):
        return None
    return c[1]


def apply(state, c, seq, device_no, based_on):
    """§5.3 apply, in order, stopping at the first failure.

    Returns the list of notices (each a dict); raises Refusal or Panic. On a
    refusal the state is unchanged (the caller passes a copy it may discard).
    """
    op = c[0]
    tgt = target(c)

    def refuse(code):
        # "Every refusal apply returns ... carries op = op_name(command) and
        # target = target(command)".
        raise Refusal(code, op, tgt)

    # 1. ctx.device_no from 1 to 8,191, ctx.seq >= 2, ctx.based_on < ctx.seq.
    if not (1 <= device_no <= 8191 and seq >= 2 and based_on < seq):
        refuse("engine/toy_bad_ctx")
    # 2. The op is allowed by rules (op 9 only under 2).
    if op == "mul" and state.rules < 2:
        refuse("engine/toy_rules")
    notices = []
    if op == "set":
        _, k, v = c
        if k in state.keys:
            _, ss, sd = state.keys[k]
            if ss > based_on and sd != device_no and sd != 0:
                notices.append({"seq": seq, "author": device_no, "other": sd,
                                "other_seq": ss, "key": k})
        state.keys[k] = (v, seq, device_no)
    elif op in ("add", "mul"):
        _, k, d = c
        if k not in state.keys:
            refuse("engine/toy_gone")
        cur = state.keys[k][0]
        r = cur + d if op == "add" else cur * d
        if not (I64_MIN <= r <= I64_MAX):
            refuse("engine/toy_overflow")
        state.keys[k] = (r, seq, device_no)
    elif op == "del":
        state.keys.pop(c[1], None)  # absent key: a benign repeat
    elif op == "create":
        _, i, n = c
        h, lo = divmod(i, TWO40)
        if h != device_no or not (1 <= lo <= TWO40 - 1) or lo <= state.marks.get(h, 0):
            refuse("engine/toy_bad_id")
        if len(state.items) >= 10000:
            refuse("engine/toy_cap")
        state.items[i] = n
        state.marks[h] = lo
    elif op == "name":
        _, i, n = c
        if i not in state.items:
            refuse("engine/toy_gone")
        state.items[i] = n
    elif op == "forget":
        state.items.pop(c[1], None)  # absent item: a benign repeat
    elif op == "fail":
        refuse("engine/toy_fail")
    elif op == "panic":
        raise Panic()
    return notices


def notice_bytes(n):
    # §5.3: u64 seq ‖ u16 author ‖ u16 other ‖ u64 other_seq ‖ u8 key (21 bytes).
    return struct.pack(">QHHQB", n["seq"], n["author"], n["other"], n["other_seq"], n["key"])
