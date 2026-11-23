//! `seal-file.json` (seal.md §19): sealed local files from `L_d`, kind,
//! board, dev, the drawn `file_id` and nonces, and record plaintexts; reads
//! of whole files and of files with torn, bad and corrupt tails and wrong
//! headers.

use crate::common::{hex, unhex, unhex_n};
use crate::{check_or_write, count};
use serde_json::{Map, Value, json};
use taskdeck_seal::{BoardId, DevId, FileKind, FileSealer, FileTail, FixedEntropy, StorageKey, open_file};

const BOARD: BoardId = BoardId([0x42; 16]);
const DEV: DevId = DevId([0xD1; 16]);

fn kind(k: u8) -> FileKind {
    match k {
        1 => FileKind::Log,
        7 => FileKind::Identity,
        11 => FileKind::History,
        2 => FileKind::Checkpoint,
        _ => panic!("kind {k}"),
    }
}

fn k_file(l_d: &[u8; 32], k: u8, board: &BoardId, dev: &DevId) -> [u8; 32] {
    let mut out = [0u8; 32];
    let info = [b"taskdeck/v1/file-key\0".as_slice(), &[k], &dev.0].concat();
    hkdf::Hkdf::<sha2::Sha256>::new(Some(&board.0), l_d).expand(&info, &mut out).unwrap();
    out
}

/// A sealing case: the file of `plaintexts`, or the error.
fn seal_case(name: &str, l_d: [u8; 32], k: u8, plaintexts: &[Vec<u8>], entropy: Vec<u8>) -> (Value, Option<Vec<u8>>) {
    let mut m = Map::new();
    m.insert("name".into(), json!(name));
    m.insert("l_d".into(), json!(hex(&l_d)));
    m.insert("kind".into(), json!(k));
    m.insert("board_id".into(), json!(hex(&BOARD.0)));
    m.insert("dev".into(), json!(hex(&DEV.0)));
    m.insert("plaintexts".into(), json!(plaintexts.iter().map(|p| hex(p)).collect::<Vec<_>>()));
    m.insert("entropy".into(), json!(hex(&entropy)));
    m.insert("k_file".into(), json!(hex(&k_file(&l_d, k, &BOARD, &DEV))));
    let key = StorageKey::from_bytes(l_d);
    let mut rng = FixedEntropy::new(entropy);
    let built = (|| {
        let (mut s, mut file) = FileSealer::create(&key, kind(k), &BOARD, &DEV, &mut rng)?;
        for p in plaintexts {
            file.extend(s.seal_record(p, &mut rng)?);
        }
        Ok::<_, taskdeck_seal::SealError>(file)
    })();
    match built {
        Ok(f) => {
            m.insert("file".into(), json!(hex(&f)));
            (Value::Object(m), Some(f))
        }
        Err(e) => {
            m.insert("error".into(), json!(e.code()));
            (Value::Object(m), None)
        }
    }
}

/// A reading case: the records and tail of `file`, or the error.
fn read_case(name: &str, l_d: [u8; 32], k: u8, board: BoardId, dev: DevId, file: &[u8]) -> Value {
    let mut m = Map::new();
    m.insert("name".into(), json!(name));
    m.insert("l_d".into(), json!(hex(&l_d)));
    m.insert("kind".into(), json!(k));
    m.insert("board_id".into(), json!(hex(&board.0)));
    m.insert("dev".into(), json!(hex(&dev.0)));
    m.insert("file".into(), json!(hex(file)));
    read_into(&mut m, l_d, k, board, dev, file);
    Value::Object(m)
}

fn read_into(m: &mut Map<String, Value>, l_d: [u8; 32], k: u8, board: BoardId, dev: DevId, file: &[u8]) {
    match open_file(&StorageKey::from_bytes(l_d), kind(k), &board, &dev, file) {
        Ok(o) => {
            m.insert("records".into(), json!(o.records.iter().map(|r| hex(r)).collect::<Vec<_>>()));
            let (tail, valid) = match o.tail {
                FileTail::Clean => ("clean", None),
                FileTail::Torn { valid_bytes } => ("torn_tail", Some(valid_bytes)),
                FileTail::Bad { valid_bytes } => ("bad_tail", Some(valid_bytes)),
            };
            m.insert("tail".into(), json!(tail));
            m.insert("valid_bytes".into(), json!(valid));
        }
        Err(e) => {
            m.insert("error".into(), json!(e.code()));
        }
    }
}

// spec: SEAL-FILE-001
// spec: SEAL-FILE-002
// spec: SEAL-FILE-003
// spec: SEAL-KEY-014
#[test]
fn seal_file() {
    let l_d: [u8; 32] = count(0x60, 32).try_into().unwrap();
    let three = vec![b"frame at seq 1".to_vec(), Vec::new(), count(0, 100)];
    let mut ent = count(0x10, 16);
    ent.extend(count(0x80, 72));
    let (c1, log) = seal_case("kind 1, three records", l_d, 1, &three, ent.clone());
    let mut cases = vec![c1];
    let log = log.unwrap();
    let (c, _) = seal_case("kind 7, one record", l_d, 7, &[count(0xA0, 80)], count(0x30, 40));
    cases.push(c);
    let (c, _) = seal_case("kind 11, header only", l_d, 11, &[], count(0x50, 16));
    cases.push(c);
    let (c, _) = seal_case("entropy short for the file id", l_d, 1, &three, count(0x10, 15));
    cases.push(c);
    let (c, _) = seal_case("entropy short for the second nonce", l_d, 1, &three, ent[..16 + 24 + 23].to_vec());
    cases.push(c);

    // Reads of the three-record file: offsets 55, 55+58, 55+58+44.
    let r1 = 55 + 44 + three[0].len();
    let r2 = r1 + 44;
    let end = log.len();
    cases.push(read_case("read: clean", l_d, 1, BOARD, DEV, &log));
    cases.push(read_case("read: torn inside the last record", l_d, 1, BOARD, DEV, &log[..end - 1]));
    cases.push(read_case("read: torn, fewer than 28 bytes left", l_d, 1, BOARD, DEV, &log[..r2 + 27]));
    let mut over = log.clone();
    over[r2..r2 + 4].copy_from_slice(&67_108_881u32.to_be_bytes());
    cases.push(read_case("read: torn, ct_len over 67,108,880", l_d, 1, BOARD, DEV, &over));
    let mut under = log.clone();
    under[r1..r1 + 4].copy_from_slice(&15u32.to_be_bytes());
    cases.push(read_case("read: torn, ct_len under 16", l_d, 1, BOARD, DEV, &under));
    let mut bad = log.clone();
    bad[end - 1] ^= 1;
    cases.push(read_case("read: bad tail", l_d, 1, BOARD, DEV, &bad));
    let mut corrupt = log.clone();
    corrupt[r1 + 30] ^= 1;
    cases.push(read_case("read: corrupt record followed by another", l_d, 1, BOARD, DEV, &corrupt));
    let swapped = [&log[..r1], &log[r2..], &log[r1..r2]].concat();
    cases.push(read_case("read: records swapped", l_d, 1, BOARD, DEV, &swapped));
    cases.push(read_case("read: another kind expected", l_d, 2, BOARD, DEV, &log));
    cases.push(read_case("read: another board", l_d, 1, BoardId([0x43; 16]), DEV, &log));
    cases.push(read_case("read: another device", l_d, 1, BOARD, DevId([0xD2; 16]), &log));
    cases.push(read_case("read: another storage key", [0x61; 32], 1, BOARD, DEV, &log));
    cases.push(read_case("read: shorter than a header", l_d, 1, BOARD, DEV, &log[..54]));
    let mut ver = log.clone();
    ver[4] = 2;
    cases.push(read_case("read: ver 2", l_d, 1, BOARD, DEV, &ver));

    // Replay: every case gives what it records.
    for c in &cases {
        if let Some(f) = c.get("file").and_then(|f| f.as_str())
            && c.get("plaintexts").is_none()
        {
            let mut m = Map::new();
            let board = BoardId(unhex_n(c["board_id"].as_str().unwrap()));
            let dev = DevId(unhex_n(c["dev"].as_str().unwrap()));
            read_into(&mut m, unhex_n(c["l_d"].as_str().unwrap()), c["kind"].as_u64().unwrap() as u8, board, dev, &unhex(f));
            for (k, v) in m {
                assert_eq!(c[k.as_str()], v, "{}", c["name"]);
            }
        }
    }
    check_or_write("seal-file.json", json!({ "spec": "seal.md §19", "cases": cases }));
}
