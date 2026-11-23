//! Epoch-root wraps (seal.md §7): a changed byte, or the right wrap opened
//! under another board, epoch, commitment or recipient, does not unwrap.

use crate::common::BOARD;
use taskdeck_seal::{
    BoardId, DeviceKeys, EpochRoot, OsEntropy, Recipient, RecoverySeed, SealError, unwrap_root, wrap_root,
};

struct Fixture {
    device: DeviceKeys,
    root: EpochRoot,
    epoch: u32,
    commit: [u8; 32],
    wrap: [u8; 96],
}

fn fixture() -> Fixture {
    let device = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let root = EpochRoot::generate(&mut OsEntropy).unwrap();
    let epoch = 7;
    let commit = root.commitment(&BOARD, epoch);
    let p = device.public();
    let wrap = wrap_root(&root, &BOARD, epoch, &commit, p.dev, &p.hpke_pk, &mut OsEntropy).unwrap();
    Fixture { device, root, epoch, commit, wrap }
}

// Every byte of the 96, each changed three ways, with every other input
// held: the wrap does not open, and the failure is `seal/commitment`.
// spec: SEAL-WRAP-002
// spec: SEAL-WRAP-001
#[test]
fn every_flipped_byte_fails() {
    let f = fixture();
    let own = Recipient::Device(&f.device);
    let opened = unwrap_root(&f.wrap, &BOARD, f.epoch, &f.commit, &own).unwrap();
    assert_eq!(*opened.to_bytes(), *f.root.to_bytes());
    for i in 0..96 {
        for mask in [0x01u8, 0x80, 0xFF] {
            let mut w = f.wrap;
            w[i] ^= mask;
            assert_eq!(unwrap_root(&w, &BOARD, f.epoch, &f.commit, &own).err(), Some(SealError::Commitment), "byte {i} ^ {mask:#x}");
        }
    }
}

// The right wrap under another board, epoch, commitment or recipient.
// spec: SEAL-WRAP-002
// spec: SEAL-KEY-013
#[test]
fn the_right_wrap_in_the_wrong_place_fails() {
    let f = fixture();
    let own = Recipient::Device(&f.device);
    let fail = Some(SealError::Commitment);
    assert_eq!(unwrap_root(&f.wrap, &BoardId([0xB1; 16]), f.epoch, &f.commit, &own).err(), fail);
    for e in [f.epoch - 1, f.epoch + 1, 0, u32::MAX] {
        assert_eq!(unwrap_root(&f.wrap, &BOARD, e, &f.commit, &own).err(), fail, "epoch {e}");
    }
    let mut c = f.commit;
    c[31] ^= 1;
    assert_eq!(unwrap_root(&f.wrap, &BOARD, f.epoch, &c, &own).err(), fail);
    // Another device, another device with the same dev, the recovery key.
    let other = DeviceKeys::generate(&mut OsEntropy).unwrap();
    assert_eq!(unwrap_root(&f.wrap, &BOARD, f.epoch, &f.commit, &Recipient::Device(&other)).err(), fail);
    let mut same_dev = f.device.to_identity_bytes().to_vec();
    same_dev[16..].copy_from_slice(&other.to_identity_bytes()[16..]);
    let same_dev = DeviceKeys::from_identity_bytes(&same_dev).unwrap();
    assert_eq!(same_dev.public().dev, f.device.public().dev);
    assert_eq!(unwrap_root(&f.wrap, &BOARD, f.epoch, &f.commit, &Recipient::Device(&same_dev)).err(), fail);
    let rk = RecoverySeed::generate(&mut OsEntropy).unwrap().keys();
    assert_eq!(unwrap_root(&f.wrap, &BOARD, f.epoch, &f.commit, &Recipient::Recovery(&rk, BOARD)).err(), fail);
}

// A wrap to the recovery identity is bound to its dev_R and key the same way.
// spec: SEAL-WRAP-002
#[test]
fn a_recovery_wrap_flipped_fails() {
    let rk = RecoverySeed::generate(&mut OsEntropy).unwrap().keys();
    let rp = rk.public();
    let root = EpochRoot::generate(&mut OsEntropy).unwrap();
    let commit = root.commitment(&BOARD, 1);
    let wrap = wrap_root(&root, &BOARD, 1, &commit, rp.dev(&BOARD), &rp.hpke_pk, &mut OsEntropy).unwrap();
    let own = Recipient::Recovery(&rk, BOARD);
    assert!(unwrap_root(&wrap, &BOARD, 1, &commit, &own).is_ok());
    for i in 0..96 {
        let mut w = wrap;
        w[i] ^= 0x40;
        assert_eq!(unwrap_root(&w, &BOARD, 1, &commit, &own).err(), Some(SealError::Commitment), "byte {i}");
    }
    // Wrapped to dev_R of another board: the id is not this identity's.
    let other_dev = rp.dev(&BoardId([0xB1; 16]));
    let wrap = wrap_root(&root, &BOARD, 1, &commit, other_dev, &rp.hpke_pk, &mut OsEntropy).unwrap();
    assert_eq!(unwrap_root(&wrap, &BOARD, 1, &commit, &own).err(), Some(SealError::Commitment));
}
