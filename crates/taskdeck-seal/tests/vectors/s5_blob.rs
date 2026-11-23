//! `seal-blob.json` (seal.md §20): a snapshot and a blob of kind 2 sealed
//! from fixed inputs (blob kind, epoch root, `S`, writer, the body with its
//! history, the 40 bytes of entropy: the blob id, then the nonce), with the
//! inflated body, the sealed bytes, `sha256`, the size and the part count;
//! the sealing refusals; the opening failures in the order of §20.5 for a
//! snapshot and for a blob of kind 2 (an `open_snapshot` case gives the
//! expected `state_hash` and `hash_version` as inputs), including a history
//! that fails the projection or disagrees with the membership and a
//! decompression bomb;
//! declared-mode openings with the returned epoch and inflated length, and
//! their failures.
//!
//! Every case is run from its JSON record (`run`), so that what is published
//! is exactly what the crate was given. Sealed bytes the public sealer
//! refuses to make (a bad padding, a bad stream, a bad body) are sealed here
//! with the pinned crates (`common/blob.rs`), with id and nonce from the
//! simulator's bytes.

use crate::backup::{deflate, sha};
use crate::blob::*;
use crate::board::*;
use crate::check_or_write;
use crate::common::{hex, unhex, unhex_n};
use serde_json::{Map, Value, json};
use taskdeck_seal::*;

fn body_json(b: &SnapshotBody) -> Value {
    json!({
        "seq": b.seq, "chain": hex(&b.chain.0), "hash_version": b.hash_version, "state_hash": hex(&b.state_hash),
        "board": hex(&b.board), "membership": hex(&b.membership),
        "history": b.history.iter().map(|r| hex(&r.to_bytes())).collect::<Vec<_>>(),
    })
}

fn body_from(v: &Value) -> SnapshotBody {
    SnapshotBody {
        seq: v["seq"].as_u64().unwrap(),
        chain: Chain(unhex_n(v["chain"].as_str().unwrap())),
        hash_version: v["hash_version"].as_u64().unwrap() as u32,
        state_hash: unhex_n(v["state_hash"].as_str().unwrap()),
        board: unhex(v["board"].as_str().unwrap()),
        membership: unhex(v["membership"].as_str().unwrap()),
        history: v["history"].as_array().unwrap().iter().map(|r| HistoryRecord::from_bytes(&unhex(r.as_str().unwrap())).unwrap()).collect(),
    }
}

fn kind_of(k: &str) -> KeyKind {
    match k {
        "snap" => KeyKind::Snap,
        "blob" => KeyKind::Blob,
        _ => KeyKind::Entry,
    }
}

/// The key a case names: `{root, epoch, kind}`.
fn key_from(c: &Value, board: &BoardId) -> SymKey {
    let k = &c["key"];
    let root = EpochRoot::from_bytes(unhex_n(k["root"].as_str().unwrap()));
    derive_epoch_key(&root, board, k["epoch"].as_u64().unwrap() as u32, kind_of(k["kind"].as_str().unwrap()))
}

/// Runs a case from its JSON inputs; returns its outputs.
fn run(c: &Value) -> Map<String, Value> {
    let mut out = Map::new();
    let board = BoardId(unhex_n(c["board_id"].as_str().unwrap()));
    let writer = DevId(unhex_n(c["writer"].as_str().unwrap()));
    let parts: Vec<Vec<u8>> =
        c.get("parts").and_then(|p| p.as_array()).map(|a| a.iter().map(|p| unhex(p.as_str().unwrap())).collect()).unwrap_or_default();
    let parts: Vec<&[u8]> = parts.iter().map(|p| p.as_slice()).collect();
    let expected = |c: &Value| {
        let e = &c["expected"];
        BlobRef {
            id: BlobId(unhex_n(e["blob_id"].as_str().unwrap())),
            sha256: unhex_n(e["sha256"].as_str().unwrap()),
            size: e["size"].as_u64().unwrap(),
            parts: e["parts"].as_u64().unwrap() as u32,
            inflated: e["inflated"].as_u64().unwrap(),
            epoch: e["epoch"].as_u64().unwrap() as u32,
        }
    };
    let err = |out: &mut Map<String, Value>, e: SealError| {
        out.insert("error".into(), json!(e.code()));
    };
    let sealed = |out: &mut Map<String, Value>, sb: SealedBlob| {
        out.insert("blob_id".into(), json!(hex(&sb.id.0)));
        out.insert("sealed".into(), json!(hex(&sb.bytes)));
        out.insert("sha256".into(), json!(hex(&sb.sha256)));
        out.insert("size".into(), json!(sb.bytes.len()));
        out.insert("parts".into(), json!(sb.parts));
    };
    match c["op"].as_str().unwrap() {
        "seal_snapshot" => {
            let body = body_from(&c["body"]);
            let mut rng = source(unhex(c["entropy"].as_str().unwrap()));
            let epoch = c["epoch"].as_u64().unwrap() as u32;
            match seal_snapshot(&body, &key_from(c, &board), &board, epoch, &writer, &mut *rng) {
                Ok(sb) => sealed(&mut out, sb),
                Err(e) => err(&mut out, e),
            }
        }
        "seal_blob" => {
            let body = unhex(c["body"].as_str().unwrap());
            let mut rng = source(unhex(c["entropy"].as_str().unwrap()));
            let epoch = c["epoch"].as_u64().unwrap() as u32;
            match seal_blob(&body, &key_from(c, &board), &board, epoch, &writer, &mut *rng) {
                Ok(sb) => sealed(&mut out, sb),
                Err(e) => err(&mut out, e),
            }
        }
        "open_snapshot" => {
            let s = c["s"].as_u64().unwrap();
            let hash: [u8; 32] = unhex_n(c["state_hash"].as_str().unwrap());
            let hv = c["hash_version"].as_u64().unwrap() as u32;
            match open_snapshot(&parts, &expected(c), s, &hash, hv, &writer, &key_from(c, &board), &board) {
                Ok(b) => {
                    out.insert("body".into(), body_json(&b));
                }
                Err(e) => err(&mut out, e),
            }
        }
        "open_blob" => match open_blob(&parts, &expected(c), &writer, &key_from(c, &board), &board) {
            Ok(b) => {
                out.insert("body".into(), json!(hex(&b)));
            }
            Err(e) => err(&mut out, e),
        },
        "open_snapshot_declared" => {
            let roots: Vec<(u32, EpochRoot)> = c["roots"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(e, r)| (e.parse().unwrap(), EpochRoot::from_bytes(unhex_n(r.as_str().unwrap()))))
                .collect();
            let f = |e: u32| roots.iter().find(|(x, _)| *x == e).map(|(_, r)| r);
            let sha256 = unhex_n(c["sha256"].as_str().unwrap());
            let size = c["size"].as_u64().unwrap();
            match open_snapshot_declared(&parts, &sha256, size, c["s"].as_u64().unwrap(), &writer, &f, &board) {
                Ok((b, e, n)) => {
                    out.insert("body".into(), body_json(&b));
                    out.insert("epoch".into(), json!(e));
                    out.insert("inflated".into(), json!(n));
                }
                Err(e) => err(&mut out, e),
            }
        }
        op => panic!("unknown op {op}"),
    }
    out
}

/// A case from its inputs: runs it, checks the error it is meant to show,
/// and records the outputs.
fn case(inputs: Value, want: Option<&str>) -> Value {
    let mut m = inputs.as_object().unwrap().clone();
    let out = run(&inputs);
    assert_eq!(out.get("error").and_then(|e| e.as_str()), want, "{}", inputs["name"]);
    m.extend(out);
    Value::Object(m)
}

fn expected_json(r: &BlobRef) -> Value {
    json!({ "blob_id": hex(&r.id.0), "sha256": hex(&r.sha256), "size": r.size, "parts": r.parts, "inflated": r.inflated, "epoch": r.epoch })
}

fn key_json(root: &[u8; 32], epoch: u32, kind: &str) -> Value {
    json!({ "root": hex(root), "epoch": epoch, "kind": kind })
}

/// The fixed inputs every opening case shares; `hash` is the state hash
/// the signed entry names (version 1), which `open_snapshot` expects.
struct Fx {
    board: BoardId,
    writer: DevId,
    s: u64,
    root: [u8; 32],
    hash: [u8; 32],
}

/// The expected `state_hash` and `hash_version` of an `open_snapshot` case
/// (§20.5 step 7, §26).
type Expect<'a> = (&'a [u8; 32], u32);

impl Fx {
    #[allow(clippy::too_many_arguments)]
    fn snap_expecting(
        &self,
        name: &str,
        parts: &[&[u8]],
        r: &BlobRef,
        s: u64,
        (hash, hash_version): Expect<'_>,
        writer: &DevId,
        key: Value,
        want: Option<&str>,
    ) -> Value {
        case(
            json!({
                "name": name, "op": "open_snapshot", "parts": parts.iter().map(|p| hex(p)).collect::<Vec<_>>(),
                "expected": expected_json(r), "s": s, "state_hash": hex(hash), "hash_version": hash_version,
                "writer": hex(&writer.0), "board_id": hex(&self.board.0), "key": key,
            }),
            want,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn snap(&self, name: &str, parts: &[&[u8]], r: &BlobRef, s: u64, writer: &DevId, key: Value, want: Option<&str>) -> Value {
        self.snap_expecting(name, parts, r, s, (&self.hash, 1), writer, key, want)
    }

    fn snap_ok(&self, name: &str, bytes: &[u8], r: &BlobRef, want: Option<&str>) -> Value {
        self.snap(name, &split_parts(bytes), r, self.s, &self.writer, key_json(&self.root, 1, "snap"), want)
    }

    fn blob(&self, name: &str, parts: &[&[u8]], r: &BlobRef, writer: &DevId, key: Value, want: Option<&str>) -> Value {
        case(
            json!({
                "name": name, "op": "open_blob", "parts": parts.iter().map(|p| hex(p)).collect::<Vec<_>>(),
                "expected": expected_json(r), "writer": hex(&writer.0), "board_id": hex(&self.board.0), "key": key,
            }),
            want,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn declared(&self, name: &str, parts: &[&[u8]], sha256: &[u8; 32], size: u64, s: u64, roots: Value, want: Option<&str>) -> Value {
        case(
            json!({
                "name": name, "op": "open_snapshot_declared", "parts": parts.iter().map(|p| hex(p)).collect::<Vec<_>>(),
                "sha256": hex(sha256), "size": size, "s": s, "writer": hex(&self.writer.0), "board_id": hex(&self.board.0),
                "roots": roots,
            }),
            want,
        )
    }
}

// spec: SEAL-BLOB-001
// spec: SEAL-BLOB-002
// spec: SEAL-BLOB-004
// spec: SEAL-BLOB-006
// spec: SEAL-BLOB-007
// spec: SEAL-BLOB-008
// spec: SEAL-BLOB-009
// spec: SEAL-BLOB-010
#[test]
fn seal_blob_vectors() {
    // A board of two devices; the membership after Genesis alone, for a
    // history that disagrees.
    let mut s = Sim::new(88, 1);
    let older = s.m.to_canonical_bytes();
    s.add_device();
    let w = s.devs[1].public().dev;
    let body = snap_body(&s);
    let fx = Fx { board: s.board, writer: w, s: s.seq(), root: s.roots[&1], hash: body.state_hash };
    let snap_key_json = key_json(&fx.root, 1, "snap");
    let mut cases = Vec::new();

    // Sealing.
    let inflated = body_bytes(&body);
    let entropy = s.rng.bytes(40);
    let seal_in = |name: &str, b: &SnapshotBody, entropy: &[u8]| {
        json!({
            "name": name, "op": "seal_snapshot", "blob_kind": 1, "board_id": hex(&fx.board.0), "epoch": 1,
            "key": snap_key_json, "s": b.seq, "writer": hex(&w.0), "body": body_json(b), "entropy": hex(entropy),
        })
    };
    let mut c = case(seal_in("seal_snapshot", &body, &entropy), None);
    c.as_object_mut().unwrap().insert("inflated".into(), json!(hex(&inflated)));
    let snapshot = unhex(c["sealed"].as_str().unwrap());
    cases.push(c);
    let r = blob_ref(&snapshot, inflated.len() as u64, 1);
    let mut short = copy_body(&body);
    short.history.pop();
    cases.push(case(seal_in("seal_snapshot: a history without the last ctl seq", &short, &entropy), Some("seal/blob_open")));
    let mut bad_m = copy_body(&body);
    bad_m.membership.push(0);
    cases.push(case(seal_in("seal_snapshot: membership bytes that do not parse", &bad_m, &entropy), Some("seal/membership_format")));
    cases.push(case(seal_in("seal_snapshot: 39 bytes of entropy", &body, &entropy[..39]), Some("seal/entropy")));
    let jpeg: Vec<u8> = (0..700u32).map(|i| (i * 31 % 256) as u8).collect();
    let blob_entropy = s.rng.bytes(40);
    let c = case(
        json!({
            "name": "seal_blob", "op": "seal_blob", "blob_kind": 2, "board_id": hex(&fx.board.0), "epoch": 1,
            "key": key_json(&fx.root, 1, "blob"), "s": 0, "writer": hex(&w.0), "body": hex(&jpeg), "entropy": hex(&blob_entropy),
        }),
        None,
    );
    let blob = unhex(c["sealed"].as_str().unwrap());
    cases.push(c);
    let br = blob_ref(&blob, 0, 1);

    // open_snapshot, the steps of §20.5 in order.
    let raw = |s: &mut Sim, kind: u8, seq: u64, padded: &[u8]| {
        let id = s.rng.arr::<16>();
        let nonce = s.rng.arr::<24>();
        let k = kind_key(&fx.board, &fx.root, 1, if kind == 1 { "snap" } else { "blob" });
        Raw::new(kind, 1, fx.board, seq, w, id, nonce).seal(&k, padded)
    };
    cases.push(fx.snap_ok("open_snapshot", &snapshot, &r, None));
    cases.push(fx.snap_ok("step 1: two parts expected", &snapshot, &BlobRef { parts: 2, ..blob_ref(&snapshot, r.inflated, 1) }, Some("seal/blob_size")));
    cases.push(fx.snap_ok("step 1: size one more", &snapshot, &BlobRef { size: r.size + 1, ..blob_ref(&snapshot, r.inflated, 1) }, Some("seal/blob_size")));
    cases.push(fx.snap(
        "step 1: an empty last part",
        &[&snapshot, &[]],
        &BlobRef { parts: 2, ..blob_ref(&snapshot, r.inflated, 1) },
        fx.s,
        &w,
        snap_key_json.clone(),
        Some("seal/blob_size"),
    ));
    let mut f = snapshot.clone();
    f[100] ^= 1;
    cases.push(fx.snap_ok("step 2: a ciphertext byte flipped", &f, &r, Some("seal/blob_hash")));
    for (name, i, v) in [("step 3: kind 2", 6usize, 2u8), ("step 3: ver 2", 4, 2), ("step 3: magic", 3, b'2'), ("step 3: epoch 2 in the header", 10, 2)] {
        let mut f = snapshot.clone();
        f[i] = v;
        cases.push(fx.snap_ok(&format!("{name}, the hash recomputed"), &f, &BlobRef { sha256: sha(&[&f]), ..blob_ref(&f, r.inflated, 1) }, Some("seal/blob_header")));
    }
    cases.push(fx.snap_ok("step 3: epoch 2 expected", &snapshot, &BlobRef { epoch: 2, ..blob_ref(&snapshot, r.inflated, 1) }, Some("seal/blob_header")));
    let mut f = snapshot.clone();
    f[100] ^= 1;
    cases.push(fx.snap_ok("step 4: a ciphertext byte flipped, the hash recomputed", &f, &blob_ref(&f, r.inflated, 1), Some("seal/blob_open")));
    let mut f = snapshot.clone();
    f[12] ^= 1;
    cases.push(fx.snap_ok("step 4: a blob id byte flipped, the hash recomputed", &f, &blob_ref(&f, r.inflated, 1), Some("seal/blob_open")));
    let other_w = s.devs[0].public().dev;
    let parts = split_parts(&snapshot);
    cases.push(fx.snap("step 4: another writer", &parts, &r, fx.s, &other_w, snap_key_json.clone(), Some("seal/blob_open")));
    cases.push(fx.snap("step 4: another S", &parts, &r, fx.s + 64, &w, snap_key_json.clone(), Some("seal/blob_open")));
    cases.push(fx.snap("step 4: K_blob in place of K_snap", &parts, &r, fx.s, &w, key_json(&fx.root, 1, "blob"), Some("seal/blob_open")));
    let compressed = deflate(&inflated);
    let mut dirty = pad(&compressed);
    *dirty.last_mut().unwrap() = 1;
    let f = raw(&mut s, 1, fx.s, &dirty);
    cases.push(fx.snap_ok("step 5: a padding byte not zero", &f, &blob_ref(&f, r.inflated, 1), Some("seal/blob_open")));
    let mut over = pad(&compressed);
    let l = over.len() as u32;
    over[..4].copy_from_slice(&(l - 3).to_be_bytes());
    let f = raw(&mut s, 1, fx.s, &over);
    cases.push(fx.snap_ok("step 5: body_len past the padded length", &f, &blob_ref(&f, r.inflated, 1), Some("seal/blob_open")));
    let f = raw(&mut s, 1, fx.s, &[pad(&compressed), vec![0; 512]].concat());
    cases.push(fx.snap_ok("step 5: a padded length other than P(U)", &f, &blob_ref(&f, r.inflated, 1), Some("seal/blob_open")));
    cases.push(fx.snap_ok("step 6: inflated one more", &snapshot, &blob_ref(&snapshot, r.inflated + 1, 1), Some("seal/inflate")));
    cases.push(fx.snap_ok("step 6: inflated one less", &snapshot, &blob_ref(&snapshot, r.inflated - 1, 1), Some("seal/inflate")));
    let f = raw(&mut s, 1, fx.s, &pad(&[&compressed[..], &[0]].concat()));
    cases.push(fx.snap_ok("step 6: a byte after the stream", &f, &blob_ref(&f, r.inflated, 1), Some("seal/inflate")));
    let f = raw(&mut s, 1, fx.s, &pad(&compressed[..compressed.len() - 1]));
    cases.push(fx.snap_ok("step 6: the stream cut short", &f, &blob_ref(&f, r.inflated, 1), Some("seal/inflate")));
    let bomb = deflate(&[&inflated[..], &vec![0u8; 1 << 20][..]].concat());
    let f = raw(&mut s, 1, fx.s, &pad(&bomb));
    cases.push(fx.snap_ok("step 6: a decompression bomb (1 MiB of zeros after the body)", &f, &blob_ref(&f, r.inflated, 1), Some("seal/inflate")));
    let sealed_body = |s: &mut Sim, b: &[u8]| {
        let f = raw(s, 1, fx.s, &pad(&deflate(b)));
        let r = blob_ref(&f, b.len() as u64, 1);
        (f, r)
    };
    // Each body is opened expecting the state hash it carries (so that the
    // step named fails), except the one whose state hash is not the
    // expected one.
    let mut bodies: Vec<(&str, Vec<u8>, [u8; 32])> = Vec::new();
    bodies.push(("step 7: a byte after the body", [&inflated[..], &[0]].concat(), fx.hash));
    let mut x = inflated.clone();
    x[0] = b'X';
    bodies.push(("step 7: body magic", x, fx.hash));
    let mut b2 = copy_body(&body);
    b2.seq += 64;
    bodies.push(("step 7: S of the body is not s", body_bytes(&b2), fx.hash));
    let mut b2 = copy_body(&body);
    b2.state_hash[0] ^= 1;
    bodies.push(("step 7: state hash", body_bytes(&b2), fx.hash));
    let mut b2 = copy_body(&body);
    b2.membership.push(0);
    b2.state_hash = state_hash(1, &sha(&[&b2.board]), &b2.membership);
    bodies.push(("step 7: membership bytes that do not parse", body_bytes(&b2), b2.state_hash));
    let mut rec = body.history[1].to_bytes();
    let n = rec.len();
    rec[n - 1] ^= 1;
    let mut b2 = copy_body(&body);
    b2.history[1] = HistoryRecord::from_bytes(&rec).unwrap();
    bodies.push(("step 7: a history that fails the projection (a signature)", body_bytes(&b2), fx.hash));
    let mut b2 = copy_body(&body);
    b2.membership = older.clone();
    b2.state_hash = state_hash(1, &sha(&[&b2.board]), &b2.membership);
    bodies.push(("step 7: a history that disagrees with the membership", body_bytes(&b2), b2.state_hash));
    for (name, b, hash) in bodies {
        let (f, rf) = sealed_body(&mut s, &b);
        cases.push(fx.snap_expecting(name, &split_parts(&f), &rf, fx.s, (&hash, 1), &w, snap_key_json.clone(), Some("seal/blob_open")));
    }

    // open_blob, steps 1 to 5.
    let blob_key_json = key_json(&fx.root, 1, "blob");
    cases.push(fx.blob("open_blob", &split_parts(&blob), &br, &w, blob_key_json.clone(), None));
    cases.push(fx.blob("open_blob step 1: size one less", &split_parts(&blob), &BlobRef { size: br.size - 1, ..blob_ref(&blob, 0, 1) }, &w, blob_key_json.clone(), Some("seal/blob_size")));
    let mut f = blob.clone();
    f[60] ^= 1;
    cases.push(fx.blob("open_blob step 2: a byte flipped", &[&f], &br, &w, blob_key_json.clone(), Some("seal/blob_hash")));
    let mut f = blob.clone();
    f[6] = 1;
    cases.push(fx.blob("open_blob step 3: kind 1, the hash recomputed", &[&f], &blob_ref(&f, 0, 1), &w, blob_key_json.clone(), Some("seal/blob_header")));
    cases.push(fx.blob("open_blob step 4: another writer", &[&blob], &br, &other_w, blob_key_json.clone(), Some("seal/blob_open")));
    cases.push(fx.blob("open_blob step 4: K_snap in place of K_blob", &[&blob], &br, &w, snap_key_json.clone(), Some("seal/blob_open")));
    let mut dirty = pad(&jpeg);
    *dirty.last_mut().unwrap() = 7;
    let f = raw(&mut s, 2, 0, &dirty);
    cases.push(fx.blob("open_blob step 5: a padding byte not zero", &[&f], &blob_ref(&f, 0, 1), &w, blob_key_json.clone(), Some("seal/blob_open")));

    // The declared mode.
    let roots = json!({ "1": hex(&fx.root) });
    cases.push(fx.declared("declared", &[&snapshot], &r.sha256, r.size, fx.s, roots.clone(), None));
    cases.push(fx.declared("declared step 1: size one more", &[&snapshot], &r.sha256, r.size + 1, fx.s, roots.clone(), Some("seal/blob_size")));
    cases.push(fx.declared("declared step 2: another sha256", &[&snapshot], &br.sha256, r.size, fx.s, roots.clone(), Some("seal/blob_hash")));
    cases.push(fx.declared("declared step 3: a blob of kind 2", &[&blob], &br.sha256, br.size, fx.s, roots.clone(), Some("seal/blob_header")));
    cases.push(fx.declared("declared step 4: no root for the header's epoch", &[&snapshot], &r.sha256, r.size, fx.s, json!({ "2": hex(&fx.root) }), Some("seal/missing_root")));
    let wrong = s.rng.arr::<32>();
    cases.push(fx.declared("declared step 4: another root for the epoch", &[&snapshot], &r.sha256, r.size, fx.s, json!({ "1": hex(&wrong) }), Some("seal/blob_open")));
    let bomb = deflate(&[&inflated[..], &vec![0u8; 1 << 20][..]].concat());
    let f = raw(&mut s, 1, fx.s, &pad(&bomb));
    cases.push(fx.declared("declared step 7: a body followed by 1 MiB of zeros", &[&f], &sha(&[&f]), f.len() as u64, fx.s, roots.clone(), Some("seal/blob_open")));
    let mut b2 = copy_body(&body);
    b2.state_hash = [0x55; 32];
    let (f, rf) = sealed_body(&mut s, &body_bytes(&b2));
    cases.push(fx.declared("declared step 7: the state hash is not compared", &[&f], &rf.sha256, rf.size, fx.s, roots.clone(), None));
    let mut b2 = copy_body(&body);
    b2.seq += 64;
    let (f, rf) = sealed_body(&mut s, &body_bytes(&b2));
    cases.push(fx.declared("declared step 7: S of the body is not s", &[&f], &rf.sha256, rf.size, fx.s, roots.clone(), Some("seal/blob_open")));

    // Added by the fix pass of task S5 (lead's ruling), after every earlier
    // case so that none of their draws changes.
    let f = raw(&mut s, 1, fx.s, &pad(&compressed[..compressed.len() - 1]));
    cases.push(fx.declared("declared step 6: the stream cut short", &[&f], &sha(&[&f]), f.len() as u64, fx.s, roots.clone(), Some("seal/inflate")));
    let mut b2 = copy_body(&body);
    b2.seq = body.history.last().unwrap().seq() - 1;
    let bb = body_bytes(&b2);
    let f = raw(&mut s, 1, b2.seq, &pad(&deflate(&bb)));
    cases.push(fx.snap(
        "step 7: a history record with a seq above S",
        &[&f],
        &blob_ref(&f, bb.len() as u64, 1),
        b2.seq,
        &w,
        snap_key_json.clone(),
        Some("seal/blob_open"),
    ));

    // Added by task S6b (rulings Q57 to Q59): step 7 compares the body's
    // `state_hash` and `hash_version` with the expected values the case
    // gives (the signed entry's or the bundle's). The good snapshot, with an
    // expected value other than its own. No draw.
    let mut other = fx.hash;
    other[31] ^= 1;
    cases.push(fx.snap_expecting(
        "step 7: an expected state hash other than the body's",
        &split_parts(&snapshot),
        &r,
        fx.s,
        (&other, 1),
        &w,
        snap_key_json.clone(),
        Some("seal/blob_open"),
    ));
    cases.push(fx.snap_expecting(
        "step 7: an expected hash_version other than the body's",
        &split_parts(&snapshot),
        &r,
        fx.s,
        (&fx.hash, 2),
        &w,
        snap_key_json.clone(),
        Some("seal/blob_open"),
    ));

    // Replay every case as another implementation would.
    for c in &cases {
        for (k, v) in run(c) {
            assert_eq!(c[k.as_str()], v, "{}: {k}", c["name"]);
        }
    }
    check_or_write("seal-blob.json", json!({ "spec": "seal.md §20", "cases": cases }));
}

fn copy_body(b: &SnapshotBody) -> SnapshotBody {
    SnapshotBody {
        seq: b.seq,
        chain: b.chain,
        hash_version: b.hash_version,
        state_hash: b.state_hash,
        board: b.board.clone(),
        membership: b.membership.clone(),
        history: b.history.iter().map(|r| HistoryRecord::from_bytes(&r.to_bytes()).unwrap()).collect(),
    }
}
