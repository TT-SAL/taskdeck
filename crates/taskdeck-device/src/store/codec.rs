//! The record plaintexts of device.md §6.4 and the descriptors of §6.2.
//! Integers are big-endian; `vbN` is an N-bit big-endian length and the
//! bytes (seal.md §5.1). Every length is checked against its limit and the
//! bytes left before anything is copied (plan §3.4). A plaintext of another
//! layout is `device/record_format` (DEV-STORE-005).

use sha2::{Digest as _, Sha256};
use taskdeck_sync::{
    Bootstrap, Checkpoint, HashRecord, LogRecord, Marker, NoticeKind, NoticeOp, OutboxOp, RelayTarget,
    SnapshotMaterial,
};

use super::rjson;
use crate::DeviceError;

/// `device/record_format` (device.md §21).
pub(crate) const RECORD_FORMAT: &str = "device/record_format";
/// `device/store_descriptor` (device.md §21).
pub(crate) const STORE_DESCRIPTOR: &str = "device/store_descriptor";

/// Any record plaintext (device.md §6.4, §22).
pub const RECORD_MAX: usize = 67_108_864;
/// Entries of one log page record (device.md §6.4.1).
pub const LOG_PAGE_MAX: usize = 256;
/// A frame in a log entry (device.md §6.4.1).
pub const FRAME_MIN: usize = 670;
pub const FRAME_MAX: usize = 262_144;
/// Ops of one hash, outbox or notice record (device.md §6.4.3 to §6.4.5).
pub const OPS_MAX: usize = 1_024;
/// The checkpoint's parts (device.md §6.4.2).
pub const BOARD_MAX: usize = 62_914_560;
pub const MEMBERSHIP_MAX: usize = 2_097_152;
/// A notice or refusal record (device.md §6.4.5).
pub const NOTICE_MAX: usize = 4_096;

pub(crate) fn err(code: &'static str) -> DeviceError {
    DeviceError { code, status: None, message: None }
}

pub(crate) fn fmt() -> DeviceError {
    err(RECORD_FORMAT)
}

/// A bounds-checked reader.
pub(crate) struct Cur<'a>(pub(crate) &'a [u8]);

impl<'a> Cur<'a> {
    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8], DeviceError> {
        if self.0.len() < n {
            return Err(fmt());
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
    pub(crate) fn arr<const N: usize>(&mut self) -> Result<[u8; N], DeviceError> {
        self.take(N)?.try_into().map_err(|_| fmt())
    }
    pub(crate) fn u8(&mut self) -> Result<u8, DeviceError> {
        Ok(self.arr::<1>()?[0])
    }
    pub(crate) fn u16(&mut self) -> Result<u16, DeviceError> {
        Ok(u16::from_be_bytes(self.arr()?))
    }
    pub(crate) fn u32(&mut self) -> Result<u32, DeviceError> {
        Ok(u32::from_be_bytes(self.arr()?))
    }
    pub(crate) fn u64(&mut self) -> Result<u64, DeviceError> {
        Ok(u64::from_be_bytes(self.arr()?))
    }
    pub(crate) fn i64(&mut self) -> Result<i64, DeviceError> {
        Ok(i64::from_be_bytes(self.arr()?))
    }
    /// `vb32` of at most `max` bytes.
    pub(crate) fn vb32(&mut self, max: usize) -> Result<&'a [u8], DeviceError> {
        let n = usize::try_from(self.u32()?).map_err(|_| fmt())?;
        if n > max {
            return Err(fmt());
        }
        self.take(n)
    }
    pub(crate) fn vb8(&mut self) -> Result<&'a [u8], DeviceError> {
        let n = usize::from(self.u8()?);
        self.take(n)
    }
    pub(crate) fn end(&self) -> Result<(), DeviceError> {
        if self.0.is_empty() { Ok(()) } else { Err(fmt()) }
    }
}

fn put_vb32(out: &mut Vec<u8>, b: &[u8], max: usize) -> Result<(), DeviceError> {
    if b.len() > max {
        return Err(fmt());
    }
    out.extend_from_slice(&(b.len() as u32).to_be_bytes());
    out.extend_from_slice(b);
    Ok(())
}

// ---------------------------------------------------------------- kind 1

/// The plaintext size of one log entry (device.md §6.4.1).
pub(crate) fn log_entry_len(r: &LogRecord) -> usize {
    52 + r.frame.len()
}

/// A log page (device.md §6.4.1): `n u16 ‖ n × (seq ‖ chain ‖ tau ‖ flen ‖
/// frame)`. The caller keeps `n` within 1 to 256 and the plaintext within
/// the record limit (`split_log`).
pub fn encode_log_page(records: &[LogRecord]) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + records.iter().map(log_entry_len).sum::<usize>());
    out.extend_from_slice(&(records.len() as u16).to_be_bytes());
    for r in records {
        out.extend_from_slice(&r.seq.to_be_bytes());
        out.extend_from_slice(&r.chain);
        out.extend_from_slice(&r.tau.to_be_bytes());
        out.extend_from_slice(&(r.frame.len() as u32).to_be_bytes());
        out.extend_from_slice(&r.frame);
    }
    out
}

/// Reads a log page (device.md §6.4.1): `n` from 1 to 256, `flen` from 670
/// to 262,144, consecutive seqs, nothing after the last entry.
pub fn decode_log_page(bytes: &[u8]) -> Result<Vec<LogRecord>, DeviceError> {
    let mut c = Cur(bytes);
    let n = usize::from(c.u16()?);
    if !(1..=LOG_PAGE_MAX).contains(&n) {
        return Err(fmt());
    }
    let mut out: Vec<LogRecord> = Vec::with_capacity(n);
    for _ in 0..n {
        let seq = c.u64()?;
        let chain = c.arr::<32>()?;
        let tau = c.i64()?;
        let flen = usize::try_from(c.u32()?).map_err(|_| fmt())?;
        if !(FRAME_MIN..=FRAME_MAX).contains(&flen) {
            return Err(fmt());
        }
        let frame = c.take(flen)?.to_vec();
        if let Some(prev) = out.last()
            && prev.seq.checked_add(1) != Some(seq)
        {
            return Err(fmt());
        }
        out.push(LogRecord { seq, chain, tau, frame });
    }
    c.end()?;
    Ok(out)
}

/// The records of an `AppendLog` cut into pages: each of at most 256
/// entries and at most 67,108,864 plaintext bytes (device.md §6.4.1). The
/// store cuts the records at a segment's boundary first (DEV-STORE-003).
pub(crate) fn split_log(records: &[LogRecord]) -> Vec<&[LogRecord]> {
    let mut out = Vec::new();
    let mut start = 0;
    while start < records.len() {
        let mut end = start;
        let mut size = 2;
        while end < records.len() && end - start < LOG_PAGE_MAX && size + log_entry_len(&records[end]) <= RECORD_MAX {
            size += log_entry_len(&records[end]);
            end += 1;
        }
        // A single frame is far below the limit; progress is guaranteed.
        let end = end.max(start + 1);
        out.push(&records[start..end]);
        start = end;
    }
    out
}

// ---------------------------------------------------------------- kinds 2 and 3

/// `"TDC1"` (device.md §6.4.2).
const CHECKPOINT_MAGIC: &[u8; 4] = b"TDC1";

/// A checkpoint or base (device.md §6.4.2): `"TDC1" ‖ board_seq ‖
/// board_chain ‖ log_floor ‖ vb32(board) ‖ vb32(membership) ‖
/// vb32(registry JSON)`.
pub fn encode_checkpoint(c: &Checkpoint) -> Result<Vec<u8>, DeviceError> {
    let registry = rjson::registry_bytes(&c.registry)?;
    let mut out = Vec::with_capacity(64 + c.board.len() + c.membership.len() + registry.len());
    out.extend_from_slice(CHECKPOINT_MAGIC);
    out.extend_from_slice(&c.board_seq.to_be_bytes());
    out.extend_from_slice(&c.board_chain);
    out.extend_from_slice(&c.log_floor.to_be_bytes());
    put_vb32(&mut out, &c.board, BOARD_MAX)?;
    put_vb32(&mut out, &c.membership, MEMBERSHIP_MAX)?;
    put_vb32(&mut out, &registry, rjson::REGISTRY_MAX)?;
    Ok(out)
}

/// Reads a checkpoint or base (device.md §6.4.2, §6.5).
pub fn decode_checkpoint(bytes: &[u8]) -> Result<Checkpoint, DeviceError> {
    let mut c = Cur(bytes);
    if c.take(4)? != CHECKPOINT_MAGIC {
        return Err(fmt());
    }
    let board_seq = c.u64()?;
    let board_chain = c.arr::<32>()?;
    let log_floor = c.u64()?;
    let board = c.vb32(BOARD_MAX)?.to_vec();
    let membership = c.vb32(MEMBERSHIP_MAX)?.to_vec();
    let registry = rjson::registry_from(c.vb32(rjson::REGISTRY_MAX)?)?;
    c.end()?;
    Ok(Checkpoint { board_seq, board_chain, board, membership, log_floor, registry })
}

// ---------------------------------------------------------------- kind 4

/// A hash record (device.md §6.4.3): `n u16 ‖ n × (seq ‖ hash_version ‖
/// hash)`, `n` from 1 to 1,024.
pub(crate) fn encode_hashes(v: &[HashRecord]) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + 44 * v.len());
    out.extend_from_slice(&(v.len() as u16).to_be_bytes());
    for h in v {
        out.extend_from_slice(&h.seq.to_be_bytes());
        out.extend_from_slice(&h.hash_version.to_be_bytes());
        out.extend_from_slice(&h.hash);
    }
    out
}

pub(crate) fn decode_hashes(bytes: &[u8]) -> Result<Vec<HashRecord>, DeviceError> {
    let mut c = Cur(bytes);
    let n = usize::from(c.u16()?);
    if !(1..=OPS_MAX).contains(&n) {
        return Err(fmt());
    }
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(HashRecord { seq: c.u64()?, hash_version: c.u32()?, hash: c.arr()? });
    }
    c.end()?;
    Ok(out)
}

// ---------------------------------------------------------------- kind 5

/// One outbox op (device.md §6.4.4): `0x01 ‖ vb32(record JSON)` for `Put`,
/// `0x02 ‖ entry_id` for `Drop`.
pub(crate) fn encode_outbox_op(op: &OutboxOp) -> Result<Vec<u8>, DeviceError> {
    match op {
        OutboxOp::Put(r) => {
            let json = rjson::outbox_record_bytes(r)?;
            let mut out = Vec::with_capacity(5 + json.len());
            out.push(1);
            put_vb32(&mut out, &json, rjson::OUTBOX_RECORD_MAX)?;
            Ok(out)
        }
        OutboxOp::Drop(id) => {
            let mut out = Vec::with_capacity(17);
            out.push(2);
            out.extend_from_slice(id);
            Ok(out)
        }
    }
}

pub(crate) fn decode_outbox_ops(bytes: &[u8]) -> Result<Vec<OutboxOp>, DeviceError> {
    let mut c = Cur(bytes);
    let n = usize::from(c.u16()?);
    if !(1..=OPS_MAX).contains(&n) {
        return Err(fmt());
    }
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        match c.u8()? {
            1 => out.push(OutboxOp::Put(rjson::outbox_record_from(c.vb32(rjson::OUTBOX_RECORD_MAX)?)?)),
            2 => out.push(OutboxOp::Drop(c.arr()?)),
            _ => return Err(fmt()),
        }
    }
    c.end()?;
    Ok(out)
}

// ---------------------------------------------------------------- kind 8

/// One notice op (device.md §6.4.5): `Add` is `0x01 ‖ kind (1 notice, 2
/// refusal) ‖ seq ‖ vb32(record)`, the record at most 4,096 bytes;
/// `Dismiss` is `0x02 ‖ seq ‖ index u32`.
pub(crate) fn encode_notice_op(op: &NoticeOp) -> Result<Vec<u8>, DeviceError> {
    let mut out = Vec::new();
    match op {
        NoticeOp::Add { kind, seq, record } => {
            out.push(1);
            out.push(match kind {
                NoticeKind::Notice => 1,
                NoticeKind::Refusal => 2,
            });
            out.extend_from_slice(&seq.to_be_bytes());
            put_vb32(&mut out, record, NOTICE_MAX)?;
        }
        NoticeOp::Dismiss { seq, index } => {
            out.push(2);
            out.extend_from_slice(&seq.to_be_bytes());
            out.extend_from_slice(&index.to_be_bytes());
        }
    }
    Ok(out)
}

pub(crate) fn decode_notice_ops(bytes: &[u8]) -> Result<Vec<NoticeOp>, DeviceError> {
    let mut c = Cur(bytes);
    let n = usize::from(c.u16()?);
    if !(1..=OPS_MAX).contains(&n) {
        return Err(fmt());
    }
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        match c.u8()? {
            1 => {
                let kind = match c.u8()? {
                    1 => NoticeKind::Notice,
                    2 => NoticeKind::Refusal,
                    _ => return Err(fmt()),
                };
                let seq = c.u64()?;
                let record = c.vb32(NOTICE_MAX)?.to_vec();
                out.push(NoticeOp::Add { kind, seq, record });
            }
            2 => out.push(NoticeOp::Dismiss { seq: c.u64()?, index: c.u32()? }),
            _ => return Err(fmt()),
        }
    }
    c.end()?;
    Ok(out)
}

/// Encoded ops as record plaintexts `n u16 ‖ ops`: at most 1,024 ops and at
/// most 67,108,864 bytes each (device.md §6.4, §6.4.4, §6.4.5).
pub(crate) fn pack_ops(ops: &[Vec<u8>]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut cur: Vec<u8> = Vec::new();
    let mut n: usize = 0;
    let flush = |cur: &mut Vec<u8>, n: &mut usize, out: &mut Vec<Vec<u8>>| {
        if *n > 0 {
            let mut rec = Vec::with_capacity(2 + cur.len());
            rec.extend_from_slice(&(*n as u16).to_be_bytes());
            rec.append(cur);
            out.push(rec);
            *n = 0;
        }
    };
    for op in ops {
        if n == OPS_MAX || (n > 0 && 2 + cur.len() + op.len() > RECORD_MAX) {
            flush(&mut cur, &mut n, &mut out);
        }
        cur.extend_from_slice(op);
        n += 1;
    }
    flush(&mut cur, &mut n, &mut out);
    out
}

// ---------------------------------------------------------------- kind 10

/// The marker (device.md §6.4.6): `first ‖ last ‖ attempt`, 24 bytes.
pub(crate) fn encode_marker(m: &Marker) -> Vec<u8> {
    let mut out = Vec::with_capacity(24);
    out.extend_from_slice(&m.first.to_be_bytes());
    out.extend_from_slice(&m.last.to_be_bytes());
    out.extend_from_slice(&m.attempt.to_be_bytes());
    out
}

pub(crate) fn decode_marker(bytes: &[u8]) -> Result<Marker, DeviceError> {
    let mut c = Cur(bytes);
    let m = Marker { first: c.u64()?, last: c.u64()?, attempt: c.u64()? };
    c.end()?;
    Ok(m)
}

// ---------------------------------------------------------------- kind 17

fn put_target(out: &mut Vec<u8>, t: &RelayTarget) -> Result<(), DeviceError> {
    let h = t.host.as_bytes();
    if h.is_empty() || h.len() > 255 {
        return Err(fmt());
    }
    out.push(h.len() as u8);
    out.extend_from_slice(h);
    out.extend_from_slice(&t.door);
    out.extend_from_slice(&t.home_gen.to_be_bytes());
    Ok(())
}

fn get_target(c: &mut Cur<'_>) -> Result<RelayTarget, DeviceError> {
    let h = c.vb8()?;
    if h.is_empty() {
        return Err(fmt());
    }
    // A host of seal.md §5.4: printable ASCII; the full grammar is the
    // relay address parser's (SEAL-ENC-006).
    let host = std::str::from_utf8(h).map_err(|_| fmt())?;
    if !host.bytes().all(|b| b.is_ascii_graphic()) || host.ends_with(":443") {
        return Err(fmt());
    }
    Ok(RelayTarget { host: host.to_string(), door: c.arr()?, home_gen: c.u32()? })
}

/// The bootstrap (device.md §6.4.9): `0x01 ‖ vb32(board) ‖ vb32(frame_1) ‖
/// target` for `Genesis`; `0x02 ‖ seq ‖ chain ‖ hash_version ‖ state_hash ‖
/// vb32(board) ‖ vb32(membership) ‖ added_seq ‖ target` for `Join`; the
/// target `vb8(host) ‖ door[5] ‖ home_gen u32`.
pub(crate) fn encode_bootstrap(b: &Bootstrap) -> Result<Vec<u8>, DeviceError> {
    let mut out = Vec::new();
    match b {
        Bootstrap::Genesis { board, frame_1, relay } => {
            out.push(1);
            put_vb32(&mut out, board, BOARD_MAX)?;
            put_vb32(&mut out, frame_1, FRAME_MAX)?;
            put_target(&mut out, relay)?;
        }
        Bootstrap::Join { material, added_seq, relay } => {
            out.push(2);
            out.extend_from_slice(&material.seq.to_be_bytes());
            out.extend_from_slice(&material.chain);
            out.extend_from_slice(&material.hash_version.to_be_bytes());
            out.extend_from_slice(&material.state_hash);
            put_vb32(&mut out, &material.board, BOARD_MAX)?;
            put_vb32(&mut out, &material.membership, MEMBERSHIP_MAX)?;
            out.extend_from_slice(&added_seq.to_be_bytes());
            put_target(&mut out, relay)?;
        }
    }
    Ok(out)
}

pub(crate) fn decode_bootstrap(bytes: &[u8]) -> Result<Bootstrap, DeviceError> {
    let mut c = Cur(bytes);
    let b = match c.u8()? {
        1 => {
            let board = c.vb32(BOARD_MAX)?.to_vec();
            let frame_1 = c.vb32(FRAME_MAX)?.to_vec();
            Bootstrap::Genesis { board, frame_1, relay: get_target(&mut c)? }
        }
        2 => {
            let seq = c.u64()?;
            let chain = c.arr()?;
            let hash_version = c.u32()?;
            let state_hash = c.arr()?;
            let board = c.vb32(BOARD_MAX)?.to_vec();
            let membership = c.vb32(MEMBERSHIP_MAX)?.to_vec();
            let added_seq = c.u64()?;
            let material = SnapshotMaterial { seq, chain, hash_version, state_hash, board, membership };
            Bootstrap::Join { material, added_seq, relay: get_target(&mut c)? }
        }
        _ => return Err(fmt()),
    };
    c.end()?;
    Ok(b)
}

// ---------------------------------------------------------------- §6.2

/// `"TDA1"`, the magic of `active.tds` (device.md §6.2).
pub const ACTIVE_MAGIC: &[u8; 4] = b"TDA1";
/// `"TDP1"`, the magic of `pending.tds` (device.md §6.2).
pub const PENDING_MAGIC: &[u8; 4] = b"TDP1";
/// `"TDS1"`, the magic of `store.tds` (device.md §6.2).
const STORE_MAGIC: &[u8; 4] = b"TDS1";

fn check8(b: &[u8]) -> [u8; 8] {
    let d = Sha256::digest(b);
    let mut out = [0u8; 8];
    out.copy_from_slice(&d[..8]);
    out
}

/// `active.tds` or `pending.tds` (device.md §6.2): 45 bytes.
pub fn encode_active(magic: &[u8; 4], board: &[u8; 16], dev: &[u8; 16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(45);
    out.extend_from_slice(magic);
    out.push(1);
    out.extend_from_slice(board);
    out.extend_from_slice(dev);
    let c = check8(&out);
    out.extend_from_slice(&c);
    out
}

/// `(board_id, dev)` of an `active.tds` or `pending.tds`; any other bytes
/// are `device/store_descriptor` (DEV-STORE-004).
pub fn decode_active(magic: &[u8; 4], bytes: &[u8]) -> Result<([u8; 16], [u8; 16]), DeviceError> {
    let bad = || err(STORE_DESCRIPTOR);
    if bytes.len() != 45 || &bytes[..4] != magic || bytes[4] != 1 || bytes[37..45] != check8(&bytes[..37]) {
        return Err(bad());
    }
    let mut c = Cur(&bytes[5..37]);
    Ok((c.arr().map_err(|_| bad())?, c.arr().map_err(|_| bad())?))
}

/// The fields of `store.tds` (device.md §6.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StoreDescriptor {
    pub board_id: [u8; 16],
    pub dev: [u8; 16],
    pub key_store: u8,
    pub created_ms: i64,
}

impl StoreDescriptor {
    /// `store.tds` (device.md §6.2): 54 bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(54);
        out.extend_from_slice(STORE_MAGIC);
        out.push(1);
        out.extend_from_slice(&self.board_id);
        out.extend_from_slice(&self.dev);
        out.push(self.key_store);
        out.extend_from_slice(&self.created_ms.to_be_bytes());
        let c = check8(&out);
        out.extend_from_slice(&c);
        out
    }

    /// Any bytes other than the layout are `device/store_descriptor`
    /// (DEV-STORE-004); so is a key store kind outside 1 to 5.
    pub fn decode(bytes: &[u8]) -> Result<StoreDescriptor, DeviceError> {
        let bad = || err(STORE_DESCRIPTOR);
        if bytes.len() != 54 || &bytes[..4] != STORE_MAGIC || bytes[4] != 1 || bytes[46..54] != check8(&bytes[..46]) {
            return Err(bad());
        }
        let mut c = Cur(&bytes[5..46]);
        let d = StoreDescriptor {
            board_id: c.arr().map_err(|_| bad())?,
            dev: c.arr().map_err(|_| bad())?,
            key_store: c.u8().map_err(|_| bad())?,
            created_ms: c.i64().map_err(|_| bad())?,
        };
        if !(1..=5).contains(&d.key_store) {
            return Err(bad());
        }
        Ok(d)
    }
}
