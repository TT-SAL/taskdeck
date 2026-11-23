//! Backups (seal.md §21): flipping any byte of a nightly backup or of a
//! segment makes `read_backup` fail, and `verify_backup_file` under the
//! writer's key as well.

use crate::backup::*;
use crate::board::*;
use taskdeck_seal::*;

fn flip_all(file: &[u8], s: &Sim, writer: usize) {
    let rk = s.rec.last().unwrap();
    let pk = s.devs[writer].public().sign_pk;
    assert!(read_backup(file, &s.board, rk).is_ok());
    assert!(verify_backup_file(file, &pk).is_ok());
    for i in 0..file.len() {
        for mask in [0x01u8, 0x80] {
            let mut f = file.to_vec();
            f[i] ^= mask;
            assert!(read_backup(&f, &s.board, rk).is_err(), "byte {i} flipped with {mask:#04x} was read");
            assert!(verify_backup_file(&f, &pk).is_err(), "byte {i} flipped with {mask:#04x} verified");
        }
    }
}

// Every byte of a nightly backup and of a segment, flipped two ways: the
// file is rejected by the reader and by `verify_backup_file`.
// spec: SEAL-BKP-009
// spec: SEAL-BKP-003
// spec: SEAL-BKP-001
#[test]
fn flipping_any_byte_of_a_backup() {
    let mut s = Sim::new(71, 2);
    s.rotate(Who::D(1));
    let bytes = s.rng.bytes(32);
    flip_all(&write(&s, 0, bytes), &s, 0);
    // A segment of two frames.
    let history = copy_history(&s.history);
    let membership = s.m.to_canonical_bytes();
    let first_seq = s.seq() + 1;
    let chain_before = s.head().chain;
    let rs = roots(&s);
    for _ in 0..2 {
        let f = s.cmd(Who::D(0), &Spec::default());
        s.accept(f);
    }
    let frames = s.frames[first_seq as usize - 1..].to_vec();
    let mut i = input(&s, &history, BackupContent::Segment { membership, first_seq, chain_before, frames });
    i.roots = rs;
    let bytes = s.rng.bytes(32);
    let seg = write_backup(&s.devs[1], i, &mut *source(bytes)).unwrap();
    flip_all(&seg, &s, 1);
}
