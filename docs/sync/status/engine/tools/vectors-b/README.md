# Generators of seven engine vector families (stage `engine`, task B)

Each `gen_<family>.py` writes `crates/taskdeck-sync/tests/vectors/engine-<family>.json` from the
spec alone (standard library only, no network, no clock, no randomness). From the repository root:

    for f in hash pending receipts poison duties clock reconcile; do python3 docs/sync/status/engine/tools/vectors-b/gen_$f.py > crates/taskdeck-sync/tests/vectors/engine-$f.json; done

Running them again gives the committed files byte for byte. `toy.py` is the toy state machine of
`engine.md` §5.3 written from the spec (used by `gen_pending.py` and `gen_hash.py`); `common.py`
writes the JSON. The meaning of every field is in `crates/taskdeck-sync/tests/vectors/README.md`.
