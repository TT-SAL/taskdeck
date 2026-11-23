# Known-answer suites (seal.md §24)

The suites of seal.md §24's last paragraph, run by the target `kat`
(`main.rs`) against the pinned crates the way `taskdeck-seal` calls them.
Every vector was fetched from its official source with `curl` on 2026-11-23
and extracted by program with `extract.py` (`python3 -I extract.py <fetched> data`);
none was typed by hand.

| File in `data/` | Source (fetched 2026-11-23) | SHA-256 of what was fetched | Licence |
|---|---|---|---|
| `rfc8032.json` | RFC 8032 §7.1, <https://www.rfc-editor.org/rfc/rfc8032.txt> (the five Ed25519 tests) | `ed63657ff389301282b169b0abde9b5dd2c7e4d524fdfa5da6ff3094fc93c4c3` | IETF Trust Legal Provisions; code components under the Revised BSD Licence |
| `rfc5869.json` | RFC 5869 Appendix A, test cases 1 to 3, <https://www.rfc-editor.org/rfc/rfc5869.txt> | `7a40eb3835b35fc947eb12a2ed614db079d43b26e50dbc537c31fba16397089c` | IETF Trust Legal Provisions (Revised BSD for code components) |
| `rfc9180.json` | the one case of mode 0, KEM 0x0020, KDF 0x0001, AEAD 0x0003 from <https://raw.githubusercontent.com/cfrg/draft-irtf-cfrg-hpke/master/test-vectors.json>, checked by `extract.py` against RFC 9180 Appendix A.2.1, <https://www.rfc-editor.org/rfc/rfc9180.txt> | JSON `61fc662f01996cd06d713dacf5e133167bd309a1f329442d53f1e21a47b3ede6`; RFC `f45a8b7c30231b26d668b1e8168074e5ab514b174d656b67e859895117f1f8f6` | IETF Trust Legal Provisions (the RFC and the draft repository's vectors, published with it) |
| `xchacha.json` | draft-irtf-cfrg-xchacha-03 Appendix A.3.1 (AEAD) and A.3.2.2 (XChaCha20, block counter 1), <https://datatracker.ietf.org/doc/html/draft-irtf-cfrg-xchacha-03> | `a259240e63b1c406cf1b0d1b2a9c2f530707042b7f7185b6e6d0ccecacd8c537` | IETF Trust Legal Provisions (Revised BSD for code components) |
| `ed25519_test.json` | Wycheproof, <https://raw.githubusercontent.com/C2SP/wycheproof/main/testvectors_v1/ed25519_test.json>, unchanged | `ea80ffebac085e21cb9d3e903e7fa2b448d96702da5b31f061d0b83b430b74b3` | Apache License 2.0 |
| `xchacha20_poly1305_test.json` | Wycheproof, <https://raw.githubusercontent.com/C2SP/wycheproof/main/testvectors_v1/xchacha20_poly1305_test.json>, unchanged | `a79de072571b90eb40c3a63ce0c7f75dcb4b62323c8870228e1f61dcc61d63a9` | Apache License 2.0 |

## What runs, and what is set aside

- RFC 8032 §7.1: the public key and signature of each of the five secret keys,
  and `verify_strict` of each signature.
- RFC 5869: PRK and OKM of cases 1 to 3.
- RFC 9180 A.2.1: `DeriveKeyPair` of `ikmE` and `ikmR` (hpke serialises the
  private key clamped, RFC 9180 §7.1.2, so the vector's scalar is compared
  clamped); `enc` from 32 bytes `ikmE`; all 257 encryptions of the sender's
  context in order (which proves the key schedule's key and base nonce); the
  three exports (the exporter secret); the receiver's opening of all 257; the
  single-shot seal and open that `wrap_root` and `unwrap_root` call. The
  intermediate values (`shared_secret`, `key`, `base_nonce`,
  `exporter_secret`) are not exposed by hpke and are checked only through
  those outputs.
- draft-irtf-cfrg-xchacha-03: A.3.1 (the same AEAD vector as A.1), and A.3.2.2,
  whose keystream at block counter 1 is what the AEAD encrypts with. Set
  aside: A.2.1 and A.3.2.1 (block counter 0, the Poly1305 key block, which
  the AEAD API does not expose) and the HChaCha20 vector of the draft's §2.2.1
  (`chacha20poly1305` exposes no HChaCha20, and `chacha20` is not a dependency
  of this crate). A.2.2 is the same keystream as A.3.2.2 and is not repeated.
- Wycheproof Ed25519: all 210 cases. `valid` must verify and `invalid` must
  not. Of the 29 `acceptable` cases, the 8 `SmallOrderPublicKey` cases must
  fail (README §5.5); the 21 `MixedOrderPublicKey` cases (a prime-order
  point plus a torsion point, not of small order) must verify, as
  `verify_strict` does.
- Wycheproof XChaCha20-Poly1305: all 315 cases. A valid case seals to its
  ciphertext and tag and opens; an invalid case does not open (a nonce of
  another size cannot be given to the API at all).
