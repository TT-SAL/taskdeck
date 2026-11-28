//! `seal-backup.json` (seal.md §21): a nightly backup and a segment written
//! from a small history (inputs: the seed, the writer's keys, the history
//! records, the payload fields, the 32 bytes of entropy), with the inflated
//! payload and the file; the reader's result for each step of §21.6 (a
//! successful read with the verified history, each record's bytes),
//! including forged, truncated and re-signed files; `verify_backup_file` on
//! a good file, a re-signed one, one whose header names another key and a
//! truncated one; history records built by `from_accepted` from accepted
//! `ctl` entries, their bytes, and `from_bytes` failures.
//!
//! Files the public writer refuses to make (a forged writer, a defect after
//! the HPKE layer) are sealed here with the pinned crates
//! (`common/backup.rs`), with a fixed ephemeral `ikm`.

use crate::backup::*;
use crate::board::*;
use crate::check_or_write;
use crate::common::{hex, unhex, unhex_n};
use serde_json::{Map, Value, json};
use taskdeck_seal::*;

/// `S + L` (a non-canonical `S`) over 32 little-endian bytes.
fn add_l(s: &[u8]) -> [u8; 32] {
    const L: [u8; 32] = [
        0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde, 0x14, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10,
    ];
    let mut out = [0u8; 32];
    let mut carry = 0u16;
    for i in 0..32 {
        let v = u16::from(s[i]) + u16::from(L[i]) + carry;
        out[i] = v as u8;
        carry = v >> 8;
    }
    out
}

/// The reader's result for `file` with the sheet of `seed` and `board`.
fn read_result(file: &[u8], board: &BoardId, seed: &[u8; 32]) -> Map<String, Value> {
    let mut m = Map::new();
    match read_backup(file, board, &recovery_from_seed(seed)) {
        Err(e) => {
            m.insert("error".into(), json!(e.code()));
        }
        Ok(v) => {
            m.insert("writer_dev".into(), json!(hex(&v.writer.dev.0)));
            m.insert("writer_sign_pk".into(), json!(hex(&v.writer.sign_pk)));
            m.insert("seq".into(), json!(v.seq));
            m.insert("written_ms".into(), json!(v.written_ms));
            m.insert("file_type".into(), json!(v.file_type));
            m.insert("read_token".into(), json!(v.read_token));
            m.insert("meta".into(), json!(hex(&v.meta)));
            m.insert(
                "roots".into(),
                json!(v.roots.iter().map(|(e, r)| json!({ "epoch": e, "root": hex(&r.to_bytes()[..]) })).collect::<Vec<_>>()),
            );
            m.insert("projection".into(), json!(hex(&v.projection.to_canonical_bytes())));
            m.insert("history".into(), json!(v.history.iter().map(|r| hex(&r.to_bytes())).collect::<Vec<_>>()));
        }
    }
    m
}

fn read_case(name: &str, file: &[u8], board: &BoardId, seed: &[u8; 32], want: Option<&str>) -> Value {
    let mut m = Map::new();
    m.insert("name".into(), json!(name));
    m.insert("op".into(), json!("read_backup"));
    m.insert("file".into(), json!(hex(file)));
    m.insert("sheet_board_id".into(), json!(hex(&board.0)));
    m.insert("seed".into(), json!(hex(seed)));
    let r = read_result(file, board, seed);
    assert_eq!(r.get("error").and_then(|e| e.as_str()), want, "{name}");
    m.extend(r);
    Value::Object(m)
}

fn verify_result(file: &[u8], pk: &[u8; 32]) -> Map<String, Value> {
    let mut m = Map::new();
    match verify_backup_file(file, pk) {
        Err(e) => {
            m.insert("error".into(), json!(e.code()));
        }
        Ok(h) => {
            m.insert(
                "header".into(),
                json!({
                    "file_type": h.file_type, "board_id": hex(&h.board.0), "writer_dev": hex(&h.writer_dev.0),
                    "writer_sign_pk": hex(&h.writer_sign_pk), "recipient": hex(&h.recipient.0), "seq": h.seq,
                    "written_ms": h.written_ms, "ct_len": h.ct_len,
                }),
            );
        }
    }
    m
}

fn verify_case(name: &str, file: &[u8], pk: &[u8; 32], want: Option<&str>) -> Value {
    let mut m = Map::new();
    m.insert("name".into(), json!(name));
    m.insert("op".into(), json!("verify_backup_file"));
    m.insert("file".into(), json!(hex(file)));
    m.insert("writer_sign_pk".into(), json!(hex(pk)));
    let r = verify_result(file, pk);
    assert_eq!(r.get("error").and_then(|e| e.as_str()), want, "{name}");
    m.extend(r);
    Value::Object(m)
}

/// The inputs of a write and its outputs.
fn write_case(name: &str, s: &Sim, w: usize, i: BackupInput, entropy: Vec<u8>) -> (Value, Vec<u8>) {
    let mut m = Map::new();
    m.insert("name".into(), json!(name));
    m.insert("op".into(), json!("write_backup"));
    m.insert("seed".into(), json!(hex(s.seeds.last().unwrap())));
    m.insert("board_id".into(), json!(hex(&i.board.0)));
    m.insert("writer_identity".into(), json!(hex(&s.devs[w].to_identity_bytes())));
    m.insert("seq".into(), json!(i.seq));
    m.insert("written_ms".into(), json!(i.written_ms));
    m.insert("history".into(), json!(i.history.iter().map(|r| hex(&r.to_bytes())).collect::<Vec<_>>()));
    match &i.content {
        BackupContent::Nightly { membership, board, head, hash_version, state_hash } => {
            m.insert("membership".into(), json!(hex(membership)));
            m.insert("board".into(), json!(hex(board)));
            m.insert("head_chain".into(), json!(hex(&head.chain.0)));
            m.insert("hash_version".into(), json!(hash_version));
            m.insert("state_hash".into(), json!(hex(state_hash)));
        }
        BackupContent::Segment { membership, first_seq, chain_before, frames } => {
            m.insert("membership".into(), json!(hex(membership)));
            m.insert("first_seq".into(), json!(first_seq));
            m.insert("chain_before".into(), json!(hex(&chain_before.0)));
            m.insert("frames".into(), json!(frames.iter().map(|f| hex(f)).collect::<Vec<_>>()));
        }
    }
    m.insert(
        "roots".into(),
        json!(i.roots.iter().map(|(e, r)| json!({ "epoch": e, "root": hex(&r.to_bytes()[..]) })).collect::<Vec<_>>()),
    );
    m.insert("read_token".into(), json!(i.read_token));
    m.insert("meta".into(), json!(hex(&i.meta)));
    m.insert("pinned_rsign_pk".into(), json!(hex(&i.pinned_recovery.sign_pk)));
    m.insert("pinned_rhpke_pk".into(), json!(hex(&i.pinned_recovery.hpke_pk)));
    m.insert("entropy".into(), json!(hex(&entropy)));
    let board = i.board;
    let file = write_backup(&s.devs[w], i, &mut *source(entropy)).unwrap();
    let compressed = open_ct(&file, &board, s.seeds.last().unwrap()).unwrap();
    m.insert("payload".into(), json!(hex(&inflate(&compressed))));
    m.insert("compressed".into(), json!(hex(&compressed)));
    m.insert("file".into(), json!(hex(&file)));
    let r = read_result(&file, &board, s.seeds.last().unwrap());
    assert!(r.get("error").is_none(), "{name}: {r:?}");
    m.insert("read".into(), Value::Object(r));
    (Value::Object(m), file)
}

/// Classifies `f` at the next seq, records the `from_accepted` case of its
/// opened entry, and commits it.
fn accepted_case(s: &mut Sim, name: &str, f: Vec<u8>) -> Value {
    let r = s.classify(&f, &Opts::default()).unwrap();
    assert!(matches!(r.class, Class::Accept));
    let seq = s.seq() + 1;
    let o = r.opened.as_ref().unwrap();
    let mut m = Map::new();
    m.insert("name".into(), json!(name));
    m.insert("op".into(), json!("from_accepted"));
    m.insert("seq".into(), json!(seq));
    m.insert("frame".into(), json!(hex(&f)));
    m.insert("entry_id".into(), json!(hex(&o.entry_id.0)));
    m.insert("kind".into(), json!(o.kind as u8));
    m.insert("at_ms".into(), json!(o.at_ms));
    m.insert("payload".into(), json!(hex(&o.payload)));
    match HistoryRecord::from_accepted(seq, &f, o) {
        Ok(h) => m.insert("record".into(), json!(hex(&h.to_bytes()))),
        Err(e) => m.insert("error".into(), json!(e.code())),
    };
    s.commit(f, r);
    Value::Object(m)
}

fn from_bytes_case(name: &str, b: &[u8]) -> Value {
    let e = HistoryRecord::from_bytes(b).err().map(|e| e.code());
    json!({ "name": name, "op": "from_bytes", "record": hex(b), "error": e })
}

// spec: SEAL-BKP-001
// spec: SEAL-BKP-002
// spec: SEAL-BKP-003
// spec: SEAL-BKP-009
// spec: SEAL-BKP-010
// spec: SEAL-SIG-006
#[test]
fn seal_backup() {
    let mut s = Sim::new(84, 1);
    let mut cases = Vec::new();
    // History records from accepted control entries; a cmd is not one.
    let k = s.new_device();
    let item = s.added_item(&k);
    s.devs.push(k);
    let f = s.ctl(Who::D(0), vec![item], &Spec::default());
    cases.push(accepted_case(&mut s, "from_accepted: DeviceAdded", f));
    let items = s.rotate_items(vec![]);
    let f = s.ctl(Who::D(1), items, &Spec::default());
    cases.push(accepted_case(&mut s, "from_accepted: EpochRotated", f));
    let f = s.cmd(Who::D(0), &Spec::default());
    cases.push(accepted_case(&mut s, "from_accepted: a cmd is not a control entry", f));
    let rec = s.history[1].to_bytes();
    cases.push(from_bytes_case("from_bytes: the record", &rec));
    cases.push(from_bytes_case("from_bytes: one byte short", &rec[..rec.len() - 1]));
    cases.push(from_bytes_case("from_bytes: a trailing byte", &[&rec[..], &[0]].concat()));
    let mut big = rec.clone();
    big[88..92].copy_from_slice(&262_145u32.to_be_bytes());
    cases.push(from_bytes_case("from_bytes: items over 262,144 bytes", &big));

    let seed = *s.seeds.last().unwrap();
    let board = s.board;

    // A nightly backup.
    let h = copy_history(&s.history);
    let entropy = s.rng.bytes(32);
    let (c, file) = write_case("write: nightly backup", &s, 0, input(&s, &h, nightly(&s)), entropy);
    cases.push(c);
    let n = file.len();
    let pk0 = s.devs[0].public().sign_pk;
    // The same payload field by field, for the files sealed here.
    let payload = Payload::nightly(&s);

    // A segment of the next two frames.
    let membership = s.m.to_canonical_bytes();
    let first_seq = s.seq() + 1;
    let chain_before = s.head().chain;
    let rs = roots(&s);
    for _ in 0..2 {
        let f = s.cmd(Who::D(1), &Spec::default());
        s.accept(f);
    }
    let frames = s.frames[first_seq as usize - 1..].to_vec();
    let mut i = input(&s, &h, BackupContent::Segment { membership, first_seq, chain_before, frames });
    i.roots = rs;
    i.read_token = None;
    let entropy = s.rng.bytes(32);
    let (c, _) = write_case("write: monthly segment", &s, 1, i, entropy);
    cases.push(c);

    // The steps of §21.6, each failing with every earlier one passing.
    cases.push(read_case("step 1: 222 bytes", &file[..222], &board, &seed, Some("seal/backup_size")));
    cases.push(read_case("step 2: truncated by one byte", &file[..n - 1], &board, &seed, Some("seal/backup_header")));
    let mut f = file.clone();
    f[0] = b'X';
    cases.push(read_case("step 2: magic", &f, &board, &seed, Some("seal/backup_header")));
    cases.push(read_case("step 3: another sheet's board", &file, &BoardId([0x99; 16]), &seed, Some("seal/backup_board")));
    cases.push(read_case("step 4: another seed", &file, &board, &[0x77; 32], Some("seal/backup_recipient")));
    let other = recovery_from_seed(&[0x77; 32]).public();
    let mut hd = Header::of(&s, &s.devs[0], 1, file_seq(&file));
    hd.recipient = other;
    let to_other = seal_compressed(&hd, &s.devs[0], &deflate(&payload.bytes()), [1; 32]);
    cases.push(read_case("step 4: sealed to another recovery key", &to_other, &board, &seed, Some("seal/backup_recipient")));
    let mut f = file.clone();
    resign(&mut f, &s.devs[1]);
    cases.push(read_case("step 5: re-signed by another member", &f, &board, &seed, Some("seal/backup_signature")));
    let mut f = file.clone();
    f[100] ^= 1;
    cases.push(read_case("step 5: written_ms changed", &f, &board, &seed, Some("seal/backup_signature")));
    let mut f = file.clone();
    let high = add_l(&file[n - 32..]);
    f[n - 32..].copy_from_slice(&high);
    cases.push(read_case("step 5: S not canonical", &f, &board, &seed, Some("seal/backup_signature")));
    let mut f = file.clone();
    f[n - 100] ^= 1;
    resign(&mut f, &s.devs[0]);
    cases.push(read_case("step 6: ct changed, re-signed by the writer", &f, &board, &seed, Some("seal/backup_open")));
    let base = Header::of(&s, &s.devs[0], 1, file_seq(&file));
    let good = deflate(&payload.bytes());
    let sealed = seal_compressed(&base, &s.devs[0], &good, [2; 32]);
    cases.push(read_case("step 6 passes: sealed here", &sealed, &board, &seed, None));
    let trailing = [&good[..], &[0]].concat();
    cases.push(read_case("step 7: a byte after the stream", &seal_compressed(&base, &s.devs[0], &trailing, [2; 32]), &board, &seed, Some("seal/inflate")));
    let mut p = {
        let mut p = payload.clone();
        p.magic = *b"TDBQ";
        p
    };
    cases.push(read_case("step 8: payload magic", &seal_compressed(&base, &s.devs[0], &deflate(&p.bytes()), [2; 32]), &board, &seed, Some("seal/backup_payload")));
    p = {
        let mut p = payload.clone();
        let last = p.history.len() - 1;
        let l = p.history[last].len();
        p.history[last][l - 1] ^= 1;
        p
    };
    cases.push(read_case("step 9: a history signature", &seal_compressed(&base, &s.devs[0], &deflate(&p.bytes()), [2; 32]), &board, &seed, Some("seal/backup_history")));
    let stranger = s.new_device();
    let forged_h = Header::of(&s, &stranger, 1, file_seq(&file));
    cases.push(read_case("step 10: forged by a key that is no member", &seal_compressed(&forged_h, &stranger, &good, [3; 32]), &board, &seed, Some("seal/backup_writer")));
    p = {
        let mut p = payload.clone();
        p.state_hash[0] ^= 1;
        p
    };
    cases.push(read_case("step 11: state hash", &seal_compressed(&base, &s.devs[0], &deflate(&p.bytes()), [2; 32]), &board, &seed, Some("seal/backup_payload")));
    p = {
        let mut p = payload.clone();
        p.roots[0].1[0] ^= 1;
        p
    };
    cases.push(read_case("step 11: a root that does not match its commitment", &seal_compressed(&base, &s.devs[0], &deflate(&p.bytes()), [2; 32]), &board, &seed, Some("seal/backup_payload")));
    p = {
        let mut p = payload.clone();
        p.meta.push(b' ');
        p
    };
    let mut q = p.bytes();
    q.push(0);
    cases.push(read_case("step 11: a trailing byte", &seal_compressed(&base, &s.devs[0], &deflate(&q), [2; 32]), &board, &seed, Some("seal/backup_payload")));

    // Step 10 compares the recovery keys, not only `dev_R` (SEAL-BKP-004,
    // ruling Q45): the current recovery identity installs a recovery key made
    // of a new sheet's signing key and a third party's HPKE key; device 0 seals a
    // file to the new sheet's real keys. Read with the new sheet's seed, it
    // passes steps 1 to 9 and fails step 10.
    {
        let mut t = Sim::new(86, 2);
        let (sheet_seed, sheet) = t.new_recovery_seed();
        let (_, third) = t.new_recovery_seed();
        let mixed = RecoveryPublic { sign_pk: sheet.public().sign_pk, hpke_pk: third.public().hpke_pk };
        let change = CtlItem::RecoveryKeyChange(RecoveryKeyChange { sign_pk: mixed.sign_pk, hpke_pk: mixed.hpke_pk });
        let items = t.rotate_items(vec![change]);
        let f = t.ctl(Who::R, items, &Spec::default());
        t.accept(f);
        let mut hd = Header::of(&t, &t.devs[0], 1, t.seq());
        hd.recipient = sheet.public();
        let sealed = seal_compressed(&hd, &t.devs[0], &deflate(&Payload::nightly(&t).bytes()), [6; 32]);
        cases.push(read_case(
            "step 10: the current recovery key pairs the sheet's signing key with another HPKE key",
            &sealed,
            &t.board,
            &sheet_seed,
            Some("seal/backup_writer"),
        ));
    }

    // verify_backup_file.
    cases.push(verify_case("verify: good file", &file, &pk0, None));
    let mut f = file.clone();
    resign(&mut f, &s.devs[1]);
    cases.push(verify_case("verify: re-signed by another key", &f, &pk0, Some("seal/backup_signature")));
    // The header names device 1's key, the file is signed by the given key
    // (device 0's): only the comparison of the header's key rejects it.
    let mut names_one = Header::of(&s, &s.devs[0], 1, file_seq(&file));
    names_one.writer.sign_pk = s.devs[1].public().sign_pk;
    let names_one = seal_compressed(&names_one, &s.devs[0], &good, [5; 32]);
    let m = names_one.len() - 64;
    let msg = [b"taskdeck/v1/backup\0".as_slice(), &sha(&[&names_one[..m]])].concat();
    let sig = ed25519_dalek::Signature::from_bytes(names_one[m..].try_into().unwrap());
    assert!(signing_key(&s.devs[0]).verifying_key().verify_strict(&msg, &sig).is_ok());
    cases.push(verify_case("verify: header names another key", &names_one, &pk0, Some("seal/backup_signature")));
    let by_one = write(&s, 1, vec![4; 32]);
    cases.push(verify_case("verify: written and signed by another key", &by_one, &pk0, Some("seal/backup_signature")));
    cases.push(verify_case("verify: truncated", &file[..n - 1], &pk0, Some("seal/backup_header")));

    // Replay the reads and verifications as another implementation would.
    for c in &cases {
        let file = c.get("file").and_then(|f| f.as_str()).map(unhex);
        match c["op"].as_str() {
            Some("read_backup") => {
                let r = read_result(&file.unwrap(), &BoardId(unhex_n(c["sheet_board_id"].as_str().unwrap())), &unhex_n(c["seed"].as_str().unwrap()));
                for (k, v) in r {
                    assert_eq!(c[k.as_str()], v, "{}", c["name"]);
                }
            }
            Some("verify_backup_file") => {
                let r = verify_result(&file.unwrap(), &unhex_n(c["writer_sign_pk"].as_str().unwrap()));
                for (k, v) in r {
                    assert_eq!(c[k.as_str()], v, "{}", c["name"]);
                }
            }
            Some("from_accepted") => {
                let kind = match c["kind"].as_u64() {
                    Some(1) => Kind::Cmd,
                    Some(2) => Kind::Ctl,
                    Some(3) => Kind::Beat,
                    _ => Kind::Snap,
                };
                let o = Opened {
                    entry_id: EntryId(unhex_n(c["entry_id"].as_str().unwrap())),
                    kind,
                    at_ms: c["at_ms"].as_i64().unwrap(),
                    based_on: Head { seq: 0, chain: Chain([0; 32]) },
                    payload: unhex(c["payload"].as_str().unwrap()),
                    items: None,
                    beat: None,
                    snap: None,
                };
                let got = HistoryRecord::from_accepted(c["seq"].as_u64().unwrap(), &unhex(c["frame"].as_str().unwrap()), &o);
                match got {
                    Ok(r) => assert_eq!(c["record"].as_str(), Some(hex(&r.to_bytes()).as_str())),
                    Err(e) => assert_eq!(c["error"].as_str(), Some(e.code())),
                }
            }
            _ => {}
        }
    }
    check_or_write("seal-backup.json", json!({ "spec": "seal.md §21", "cases": cases }));
}

fn file_seq(file: &[u8]) -> u64 {
    u64::from_be_bytes(file[87..95].try_into().unwrap())
}
