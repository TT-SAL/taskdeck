#!/usr/bin/env python3
"""Extracts the known-answer vectors of seal.md §24 from their official sources.

    python3 -I extract.py <dir with the fetched files> <output dir>

The fetched files and their sources are listed in README.md. Nothing here is
typed from memory: every value is read from the fetched text by this program.
The Wycheproof files are copied unchanged; the other sources are reduced to the
cases the suites use, as JSON with lowercase hex.
"""
import json
import os
import re
import shutil
import sys

HEX = re.compile(r"^[0-9a-f]+$")


def rfc8032(path):
    text = open(path, encoding="utf-8-sig").read()
    sec = text[text.index("\n7.1.  Test Vectors for Ed25519\n"):text.index("\n7.2.  Test Vectors for Ed25519ctx\n")]
    lines = [l.strip() for l in sec.splitlines()]
    # Page footers and headers are not data.
    lines = [l for l in lines if not l.startswith("Josefsson & Liusvaara") and not l.startswith("RFC 8032") and "\f" not in l]
    cases, cur, field = [], None, None
    for l in lines:
        if l.startswith("-----TEST "):
            cur = {"name": "RFC 8032 7.1 " + l[len("-----"):], "secret_key": "", "public_key": "", "message": "", "signature": ""}
            cases.append(cur)
            field = None
        elif cur is None:
            continue
        elif l.startswith("SECRET KEY:"):
            field = "secret_key"
        elif l.startswith("PUBLIC KEY:"):
            field = "public_key"
        elif l.startswith("MESSAGE"):
            field = "message"
        elif l.startswith("SIGNATURE:"):
            field = "signature"
        elif l.startswith("ALGORITHM:") or l == "Ed25519":
            field = None
        elif l and HEX.match(l) and field:
            cur[field] += l
        elif l:
            field = None
    for c in cases:
        assert len(c["secret_key"]) == 64 and len(c["public_key"]) == 64 and len(c["signature"]) == 128, c["name"]
    assert len(cases) == 5, len(cases)
    return {"source": "RFC 8032 §7.1", "cases": cases}


def rfc5869(path):
    text = open(path, encoding="utf-8-sig").read()
    cases = []
    for n in (1, 2, 3):
        start = text.index("\nA.%d.  Test Case %d\n" % (n, n))
        end = text.index("\nA.%d.  Test Case %d\n" % (n + 1, n + 1))
        sec = [l.strip() for l in text[start:end].splitlines()]
        c, field = {"name": "RFC 5869 A.%d" % n}, None
        for l in sec:
            m = re.match(r"^(IKM|salt|info|L|PRK|OKM)\s*=\s*(.*)$", l)
            if m:
                field, rest = m.group(1).lower(), m.group(2)
                if field == "l":
                    c["l"] = int(rest)
                    field = None
                    continue
                c[field] = ""
            elif field and l.startswith("Hash"):
                field = None
                continue
            elif field is None:
                continue
            else:
                rest = l
            rest = re.sub(r"\(\d+ octets\)", "", rest).strip()
            if rest.startswith("0x"):
                rest = rest[2:]
            if rest and HEX.match(rest):
                c[field] += rest
            elif rest:
                field = None
        assert len(c["okm"]) == 2 * c["l"], c
        cases.append(c)
    return {"source": "RFC 5869 Appendix A, test cases 1 to 3", "cases": cases}


def hpke(json_path, rfc_path):
    doc = json.load(open(json_path))
    sel = [x for x in doc if x["mode"] == 0 and x["kem_id"] == 0x20 and x["kdf_id"] == 1 and x["aead_id"] == 3]
    assert len(sel) == 1, len(sel)
    c = sel[0]
    # Cross-check against RFC 9180 A.2.1 (the same suite, base mode).
    text = open(rfc_path, encoding="utf-8-sig").read()
    a21 = text[text.index("\nA.2.1.  Base Setup Information\n"):text.index("\nA.2.2.  PSK Setup Information\n")]
    flat = re.sub(r"\s+", "", a21)
    for k in ("ikmE", "pkEm", "skEm", "ikmR", "pkRm", "skRm", "enc", "shared_secret", "key", "base_nonce", "exporter_secret"):
        assert c[k] in flat, k
    for e in c["encryptions"][:3]:
        assert e["ct"] in flat
    out = {k: c[k] for k in ("mode", "kem_id", "kdf_id", "aead_id", "info", "ikmR", "ikmE", "skRm", "skEm", "pkRm", "pkEm", "enc",
                             "shared_secret", "key_schedule_context", "secret", "key", "base_nonce", "exporter_secret")}
    out["encryptions"] = c["encryptions"]
    out["exports"] = c["exports"]
    out["name"] = "RFC 9180 A.2.1 DHKEM(X25519, HKDF-SHA256), HKDF-SHA256, ChaCha20-Poly1305, base mode"
    return {"source": "cfrg/draft-irtf-cfrg-hpke test-vectors.json, the one case mode 0, kem 0x0020, kdf 0x0001, aead 0x0003; checked against RFC 9180 A.2.1",
            "cases": [out]}


def xchacha(path):
    html = open(path, encoding="utf-8").read()
    text = re.sub(r"<[^>]*>", "", html)
    def section(a, b):
        # The last occurrence of a heading is the body's (the first is the contents).
        start = text.rindex(a)
        return text[start:text.index(b, start)]
    def fields(sec, names):
        out, field = {}, None
        for l in (x.strip() for x in sec.splitlines()):
            if l.endswith(":") and l[:-1] in names:
                field = names[l[:-1]]
                out[field] = ""
            elif field and l and HEX.match(l):
                out[field] += l
            elif l and not l.startswith("Arciszewski") and not l.startswith("Internet-Draft"):
                field = None
        return out
    names = {"Plaintext": "plaintext", "AAD": "aad", "Key": "key", "IV": "iv", "Ciphertext": "ciphertext", "Tag": "tag",
             "Keystream": "keystream", "Counter": None}
    names = {k: v for k, v in names.items() if v}
    aead = fields(section("A.3.1.  AEAD_XCHACHA20_POLY1305", "A.3.2.  XChaCha20"), names)
    aead["name"] = "draft-irtf-cfrg-xchacha-03 A.3.1 AEAD_XCHACHA20_POLY1305"
    stream = fields(section("A.3.2.2.  Block Counter = 1", "Author&#x27;s Address" if "Author&#x27;s Address" in text else "Author's Address"), names)
    stream["name"] = "draft-irtf-cfrg-xchacha-03 A.3.2.2 XChaCha20, block counter 1"
    for c in (aead, stream):
        for k, v in c.items():
            if k != "name":
                assert HEX.match(v), (c["name"], k)
    assert len(aead["tag"]) == 32 and len(aead["iv"]) == 48
    assert len(stream["ciphertext"]) == len(stream["plaintext"]) == len(stream["keystream"])
    return {"source": "draft-irtf-cfrg-xchacha-03 Appendix A.3.1 and A.3.2.2", "aead": [aead], "stream_counter_1": [stream]}


def main():
    src, out = sys.argv[1], sys.argv[2]
    os.makedirs(out, exist_ok=True)
    def write(name, doc):
        with open(os.path.join(out, name), "w", encoding="ascii") as f:
            json.dump(doc, f, indent=1, sort_keys=True)
            f.write("\n")
    write("rfc8032.json", rfc8032(os.path.join(src, "rfc8032.txt")))
    write("rfc5869.json", rfc5869(os.path.join(src, "rfc5869.txt")))
    write("rfc9180.json", hpke(os.path.join(src, "test-vectors.json"), os.path.join(src, "rfc9180.txt")))
    write("xchacha.json", xchacha(os.path.join(src, "draft-irtf-cfrg-xchacha-03")))
    for f in ("ed25519_test.json", "xchacha20_poly1305_test.json"):
        shutil.copyfile(os.path.join(src, f), os.path.join(out, f))


if __name__ == "__main__":
    main()
