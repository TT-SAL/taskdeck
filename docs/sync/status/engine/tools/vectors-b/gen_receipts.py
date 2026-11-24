"""engine-receipts.json: what the engine does with one receipt, engine.md §13.1 to §13.4, §24.3.

Each case is one row of the table of §13.1 made concrete. The function `act` below
derives the expected action from the spec's sentences, cited inline.

Run: python3 gen_receipts.py > crates/taskdeck-sync/tests/vectors/engine-receipts.json
"""
import sys
sys.dont_write_bytecode = True  # no __pycache__ beside the generators

import common

# §13.2: refusals that name their author in an alarm (and a cmd parse failure).
NAMED = {"seal/revoked_author", "seal/bad_version", "seal/bad_epoch", "seal/aead",
         "seal/bad_plaintext", "seal/not_authorized", "seal/bad_payload", "seal/bad_ctl_sig",
         "seal/ctl_shape", "seal/ctl_author"}
# §24.3: an own cmd entry refused with one of these is re-sealable.
RESEALABLE = {"seal/stale_epoch", "seal/old_epoch"}

SEQ = 140
AUTHOR = "author"


def receipt(cls, code=None, kind=None, own=False, own_dev_foreign=False,
            based_on_unchecked=False, halt=None, parse=None, apply=None, apply_code=None,
            in_skips=False, reseals=0, first_apply=True, seq=SEQ, revoked=None):
    r = {"seq": seq, "class": cls, "code": code, "kind": kind, "own": own,
         "own_dev_foreign": own_dev_foreign, "based_on_unchecked": based_on_unchecked,
         "halt": halt, "in_skips": in_skips, "reseals": reseals, "first_apply": first_apply}
    if parse is not None:
        r["parse"] = parse
    if apply is not None:
        r["apply"] = apply
        r["apply_code"] = apply_code
    if revoked is not None:
        r["revoked_in_prev"] = revoked
    return r


def act(r):
    """The engine's action for receipt r. Returns (expected, clauses)."""
    cls, code, kind, own, seq = r["class"], r["code"], r["kind"], r["own"], r["seq"]
    e = {"frame_persisted": True, "membership_changes": False, "board": "none",
         "alarm": None, "stop": None, "halt": None, "refusal_record": None,
         "own_record": "dropped" if own else "none", "based_on_unchecked_rises": False,
         "poison": False}
    cl = ["ENG-CLASS-001", "§13.1"]

    def alarm(k, evidence, **fields):
        a = {"kind": k, "evidence": evidence}
        a.update(fields)
        e["alarm"] = a

    if cls == "SealerError":
        # §13.1 last row: seal/missing_root is §24.4 when this device is revoked in
        # M_{s-1}, else the local fault engine/missing_root.
        del e["frame_persisted"]
        e["own_record"] = "untouched" if own else "none"
        if r["revoked_in_prev"]:
            e["paused"] = True  # §24.4 / ENG-EPOCH-005: pause classification, no local fault
            del e["stop"]  # the revoked state is already in force; no new stop is decided here
            cl += ["§24.4", "ENG-EPOCH-005"]
        else:
            e["stop"] = {"kind": "LocalFault", "code": "engine/missing_root"}
            cl += ["§21.4", "ENG-EVID-006"]
        return e, cl

    if cls == "Skip":
        # Membership unchanged; nothing on the board; alarm skipped_frame with seq,
        # code, frame hash; no stop; no record (§13.1; §21.1 SkippedFrame row).
        alarm("SkippedFrame", "Frame", seq=seq, code=code, frame_hash_some=True)
        e["own_record"] = "none"
        return e, cl + ["ENG-CLASS-002", "§21.1"]

    if cls == "Tamper":
        # Evidence counter / fork; the stop of §21; the entry is the cut (§12.3 step 6):
        # it is not persisted, its membership is not committed.
        e["frame_persisted"] = False
        e["own_record"] = "untouched" if own else "none"
        if code in ("seal/counter_replay", "seal/counter_gap"):
            alarm("Counter", "Counter", seq=seq, author=AUTHOR, code=code)
        else:
            alarm("Fork", "Fork", seq=seq, source="BasedOn")
        e["stop"] = {"kind": "Evidence"}
        return e, cl + ["ENG-CLASS-003", "§21.1", "§21.2", "§12.3 step 6"]

    if cls == "NeedFrames":
        # Nothing changes; the engine fetches the frames and classifies again (§17.2).
        del e["frame_persisted"]
        e["own_record"] = "untouched" if own else "none"
        e["need_frames"] = True
        return e, cl + ["§17.2", "ENG-SKIP-001"]

    # Refuse and Accept consume the author's counter (seal.md §13.5): M changes,
    # except a refused entry at seq 1, which leaves M_0.
    e["membership_changes"] = not (cls == "Refuse" and seq == 1)

    if r["in_skips"]:
        # §12.4 step 1: a target in M_K.skips is treated as refused (engine/skipped);
        # nothing is applied. §13.3: an own entry skipped gets engine/skipped.
        if own:
            e["refusal_record"] = "engine/skipped"
        cl += ["§12.4 step 1", "§13.3", "§17.3", "ENG-REF-001"]
        return e, cl

    if cls == "Refuse":
        if code in NAMED:
            alarm("RefusedFrame", "Frame", seq=seq, author=AUTHOR, code=code, frame_hash_some=False)
            cl += ["§13.2"]
        if own:
            if kind == "cmd" and code in RESEALABLE:
                if r["reseals"] >= 3:
                    # §24.3: after a record's third re-seal is refused, it is finally
                    # refused with engine/reseal_limit.
                    e["refusal_record"] = "engine/reseal_limit"
                else:
                    e["own_record"] = "kept_for_reseal"
                cl += ["§24.3", "ENG-RESEAL-001", "ENG-RESEAL-002"]
            else:
                # An own beat, snap or ctl refused with a re-sealable code: ENG-REF-001 and
                # §13.3 exclude re-sealable codes from refusal records, while the §13.1 Refuse
                # row and §24.3 ("dropped"; a ctl "fails its operation's stage") can be read
                # otherwise. Not decided (README question 3): no case may reach it.
                assert code not in RESEALABLE, "own beat/snap/ctl with a re-sealable code is not decided"
                # §13.3 / ENG-REF-001: one refusal record per own entry refused for a
                # reason other than a re-sealable code; §24.3 second paragraph for
                # beat, snap and ctl.
                e["refusal_record"] = code
                cl += ["§13.3", "ENG-REF-001"]
                if kind in ("beat", "snap", "ctl"):
                    cl += ["§24.3"]
        else:
            cl += ["ENG-REF-001"]
        cl += ["ENG-CLASS-004"]
        return e, cl

    assert cls == "Accept"
    if r["own_dev_foreign"]:
        # stop sending with foreign_signer (§21.1); applying goes on.
        alarm("ForeignSigner", "ForeignSigner", seq=seq, author="this_device")
        e["stop"] = {"kind": "ForeignSigner"}
        cl += ["§21.1", "SEAL-RCPT-010"]
    if r["halt"] == "UnknownRules":
        # §16.1 Rules: board stays at seq - 1; classification stops after seq.
        e["halt"] = "Rules"
        e["board"] = "held"
        e["own_record"] = "untouched" if own else "none"
        return e, cl + ["§16.1", "§16.3", "ENG-HALT-005"]
    if kind == "cmd":
        if r["based_on_unchecked"] and r["first_apply"]:
            e["based_on_unchecked_rises"] = True
            cl += ["§13.1 based_on_unchecked row"]
        if r["parse"] == "fail":
            # A parse failure is a refusal everywhere with alarm refused_frame (§13.1,
            # §13.2, ENG-SM-003); an own entry gets a refusal record.
            alarm("RefusedFrame", "Frame", seq=seq, author=AUTHOR, code=r["parse_code"], frame_hash_some=False)
            if own:
                e["refusal_record"] = r["parse_code"]
            return e, cl + ["ENG-SM-003", "ENG-CLASS-004", "§13.2", "§13.3"]
        if r["apply"] in ("ok", "benign"):
            e["board"] = "applied"
            cl += ["§12.4", "§13.3 benign repeats"] if r["apply"] == "benign" else ["§12.4"]
            return e, cl
        if r["apply"] == "refused":
            # ENG-CLASS-005: confirmed state unchanged, no alarm; own: a refusal record.
            if own:
                e["refusal_record"] = r["apply_code"]
            return e, cl + ["ENG-CLASS-005", "§13.3", "ENG-REF-001"]
        if r["apply"] == "panic":
            # §15: the poison-pill rules; nothing is recorded, the record waits.
            e["poison"] = True
            e["own_record"] = "untouched" if own else "none"
            e["based_on_unchecked_rises"] = False
            return e, cl + ["§15.3"]
    if kind == "ctl":
        e["board"] = "ctl_items"
        if r["halt"] == "Commitment":
            # §16.1 Commitment: applies up to seq; stops after seq.
            e["halt"] = "Commitment"
            cl += ["§16.1", "§16.3"]
        return e, cl + ["§14.1"]
    if kind in ("beat", "snap"):
        e["board"] = "none"
        e["registry"] = True
        return e, cl + (["§23.3"] if kind == "beat" else ["§19.5"])
    raise AssertionError(r)


def main():
    rows = []

    def add(name, r, extra=None):
        if extra:
            r.update(extra)
        exp, cl = act(r)
        rows.append({"name": name, "clauses": cl, "receipt": r, "expected": exp})

    # Skip class: every code of steps 1 to 4 of seal.md §14.2.
    for c in ("seal/frame_format", "seal/wrong_board", "seal/unknown_author", "seal/bad_signature"):
        add(f"Skip {c}", receipt("Skip", c))
    # Tamper.
    for c in ("seal/counter_replay", "seal/counter_gap", "seal/based_on_fork", "seal/based_on_future"):
        add(f"Tamper {c}", receipt("Tamper", c, kind=None))
    add("Tamper on an own record's counter leaves the record untouched",
        receipt("Tamper", "seal/counter_gap", kind="cmd", own=True))
    # Refuse: every code of §13.2, from another device.
    for c in sorted(NAMED):
        add(f"Refuse {c} from another device: alarm naming the author, no record",
            receipt("Refuse", c, kind="cmd" if c not in ("seal/bad_ctl_sig", "seal/ctl_shape", "seal/ctl_author") else "ctl"))
    # Refuse: codes outside §13.2, from another device.
    for c, k in (("seal/old_epoch", "cmd"), ("seal/stale_epoch", "cmd"), ("seal/ctl_target", "ctl"),
                 ("seal/ctl_duplicate_key", "ctl"), ("seal/ctl_epoch", "ctl"), ("seal/ctl_support", "ctl"),
                 ("seal/ctl_last_device", "ctl")):
        add(f"Refuse {c} from another device: no alarm, no record", receipt("Refuse", c, kind=k))
    add("Refuse at seq 1 leaves M_0", receipt("Refuse", "seal/bad_epoch", kind="ctl", seq=1))
    # Refuse, own cmd.
    add("own cmd refused seal/stale_epoch: kept for re-sealing",
        receipt("Refuse", "seal/stale_epoch", kind="cmd", own=True))
    add("own cmd refused seal/old_epoch after two re-seals: kept for re-sealing",
        receipt("Refuse", "seal/old_epoch", kind="cmd", own=True, reseals=2))
    add("own cmd refused seal/stale_epoch after its third re-seal: engine/reseal_limit",
        receipt("Refuse", "seal/stale_epoch", kind="cmd", own=True, reseals=3))
    add("own cmd refused seal/revoked_author: alarm and a refusal record with the sealer's code",
        receipt("Refuse", "seal/revoked_author", kind="cmd", own=True))
    add("own cmd refused seal/bad_plaintext: alarm and a refusal record",
        receipt("Refuse", "seal/bad_plaintext", kind="cmd", own=True))
    # Refuse, own beat / snap / ctl.
    # An own beat, snap or ctl refused with seal/stale_epoch or seal/old_epoch is left out
    # (README question 3). A code that is not re-sealable is decided: §24.3 second paragraph
    # (dropped), ENG-REF-001 (a refusal record), §13.2 (seal/bad_payload names its author;
    # seal.md §14.2 step 15 gives it to a beat or snap payload that does not parse).
    add("own beat refused seal/bad_payload: alarm, dropped with a refusal record",
        receipt("Refuse", "seal/bad_payload", kind="beat", own=True))
    add("own snap refused seal/bad_payload: alarm, dropped with a refusal record",
        receipt("Refuse", "seal/bad_payload", kind="snap", own=True))
    add("own ctl refused seal/ctl_target: dropped with a refusal record",
        receipt("Refuse", "seal/ctl_target", kind="ctl", own=True))
    # Accept cmd.
    add("Accept cmd from another device, apply ok", receipt("Accept", kind="cmd", parse="ok", apply="ok"))
    add("Accept own cmd, apply ok: the record is dropped",
        receipt("Accept", kind="cmd", own=True, parse="ok", apply="ok"))
    add("Accept own cmd, benign repeat: a success, no record",
        receipt("Accept", kind="cmd", own=True, parse="ok", apply="benign"))
    add("Accept cmd from another device, apply refused: no alarm, no record",
        receipt("Accept", kind="cmd", parse="ok", apply="refused", apply_code="engine/toy_gone"))
    add("Accept own cmd, apply refused: a refusal record with the apply code",
        receipt("Accept", kind="cmd", own=True, parse="ok", apply="refused", apply_code="engine/toy_fail"))
    add("Accept cmd from another device, parse fails: alarm, no record",
        receipt("Accept", kind="cmd", parse="fail"), {"parse_code": "engine/toy_parse"})
    add("Accept own cmd, parse fails: alarm and a refusal record",
        receipt("Accept", kind="cmd", own=True, parse="fail"), {"parse_code": "engine/toy_parse"})
    add("Accept cmd, apply panics: the poison-pill rules",
        receipt("Accept", kind="cmd", parse="ok", apply="panic"))
    add("Accept own cmd, apply panics: the record waits",
        receipt("Accept", kind="cmd", own=True, parse="ok", apply="panic"))
    # Accept ctl, beat, snap.
    add("Accept ctl", receipt("Accept", kind="ctl"))
    add("Accept own ctl: the record is dropped", receipt("Accept", kind="ctl", own=True))
    add("Accept ctl with the commitment halt", receipt("Accept", kind="ctl", halt="Commitment"))
    add("Accept ctl with the rules halt", receipt("Accept", kind="ctl", halt="UnknownRules"))
    add("Accept beat", receipt("Accept", kind="beat"))
    add("Accept own beat: the record is dropped", receipt("Accept", kind="beat", own=True))
    add("Accept snap", receipt("Accept", kind="snap"))
    add("Accept own snap: the record is dropped", receipt("Accept", kind="snap", own=True))
    # NeedFrames.
    add("NeedFrames", receipt("NeedFrames", kind=None))
    # Flags.
    add("own_dev_foreign: stop sending with foreign_signer, applying goes on",
        receipt("Accept", kind="cmd", own_dev_foreign=True, parse="ok", apply="ok"))
    add("based_on_unchecked on an accepted cmd applied for the first time: the count rises",
        receipt("Accept", kind="cmd", based_on_unchecked=True, parse="ok", apply="ok"))
    add("based_on_unchecked on a replay from a base at Start: the count does not rise",
        receipt("Accept", kind="cmd", based_on_unchecked=True, parse="ok", apply="ok", first_apply=False))
    add("based_on_unchecked on a refused entry: the count does not rise",
        receipt("Refuse", "seal/ctl_target", kind="ctl", based_on_unchecked=True))
    # Skip targets.
    add("an own cmd skipped by an accepted Skip: refusal record engine/skipped",
        receipt("Accept", kind="cmd", own=True, in_skips=True))
    add("another device's cmd skipped by an accepted Skip: no record",
        receipt("Accept", kind="cmd", in_skips=True))
    # missing_root.
    add("seal/missing_root on a device not revoked: the local fault engine/missing_root",
        receipt("SealerError", "seal/missing_root", revoked=False))
    add("seal/missing_root on a device revoked in M_{s-1}: §24.4, no local fault",
        receipt("SealerError", "seal/missing_root", revoked=True))

    common.emit({"spec": "engine.md §13.1, §13.2, §13.3, §13.4, §24.3", "cases": rows})


if __name__ == "__main__":
    main()
