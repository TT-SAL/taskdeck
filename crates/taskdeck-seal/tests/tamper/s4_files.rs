//! Sealed files (seal.md §19): flipping any byte of a file never returns
//! the tampered record's plaintext and never leaves the read `Clean` with
//! every record.

use crate::common::BOARD;
use taskdeck_seal::*;

const DEV: DevId = DevId([0x4D; 16]);

fn kind() -> FileKind {
    FileKind::Log
}

// Every byte of a three-record file, flipped two ways: the read fails, or
// it returns only records before the tampered one (none for a header byte)
// with a tail that is not `Clean`.
// spec: SEAL-FILE-003
// spec: SEAL-FILE-001
#[test]
fn flipping_any_byte_of_a_file() {
    let key = StorageKey::generate(&mut OsEntropy).unwrap();
    let (mut s, mut file) = FileSealer::create(&key, kind(), &BOARD, &DEV, &mut OsEntropy).unwrap();
    let plaintexts: [&[u8]; 3] = [b"first record", b"", b"the third and last record"];
    let mut starts = Vec::new();
    for p in plaintexts {
        starts.push(file.len());
        file.extend(s.seal_record(p, &mut OsEntropy).unwrap());
    }
    let ok = open_file(&key, kind(), &BOARD, &DEV, &file).unwrap();
    assert!(matches!(ok.tail, FileTail::Clean) && ok.records.len() == 3);
    for i in 0..file.len() {
        // The record the byte belongs to; header bytes belong to none.
        let k = starts.iter().rposition(|&st| i >= st);
        for mask in [0x01u8, 0x80] {
            let mut f = file.clone();
            f[i] ^= mask;
            match open_file(&key, kind(), &BOARD, &DEV, &f) {
                Err(_) => {}
                Ok(o) => {
                    assert!(!matches!(o.tail, FileTail::Clean), "byte {i}: a clean read");
                    let limit = k.unwrap_or(0);
                    assert!(o.records.len() <= limit, "byte {i}: the tampered record came back");
                    for (j, r) in o.records.iter().enumerate() {
                        assert_eq!(r.as_slice(), plaintexts[j]);
                    }
                }
            }
        }
    }
}
