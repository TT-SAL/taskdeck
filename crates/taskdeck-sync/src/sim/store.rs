//! A device's durable storage in the model runtime (engine.md §31.4, §7):
//! exactly the writes whose `Persisted` was delivered, plus, after a crash, the
//! write in progress when the fault generator keeps it (ENG-PERSIST-004).

use std::collections::{BTreeMap, BTreeSet};

use sha2::Digest;

use crate::{
    Bootstrap, Checkpoint, HashRecord, Hash32, Head, LogRecord, Loaded, Marker, Meta, NoticeOp, OutboxOp, OutboxRecord,
    PersistOp,
};

/// The files of one replica, as records (engine.md §7 table).
#[derive(Clone, Debug, Default)]
pub(crate) struct Store {
    pub log: BTreeMap<u64, LogRecord>,
    pub log_floor: u64,
    pub floor_chain: Option<Hash32>,
    pub checkpoint: Option<Checkpoint>,
    pub bases: BTreeMap<u64, Checkpoint>,
    pub hashes: Vec<HashRecord>,
    pub outbox: Vec<OutboxOp>,
    pub notices: Vec<NoticeOp>,
    pub meta: Option<Meta>,
    pub marker: Option<Marker>,
    /// The sealer's export (`identity.tdk`).
    pub identity: Vec<u8>,
    /// The fake's control history (`history.tdk`, `device.md` §15.7), kept beside the identity.
    pub history: Vec<Vec<u8>>,
    /// Written before the first `Start`, removed after the first checkpoint (`device.md` §6.1).
    pub bootstrap: Option<Bootstrap>,
    /// The simulator's index of the log: frame hash → seq (never pruned).
    pub by_hash: BTreeMap<Hash32, u64>,
    /// The frame hash of each row `log` holds (pruned with it).
    pub log_hash: BTreeMap<u64, Hash32>,
}

impl Store {
    pub(crate) fn new(identity: Vec<u8>, history: Vec<Vec<u8>>, bootstrap: Bootstrap) -> Self {
        Store { log_floor: 1, identity, history, bootstrap: Some(bootstrap), ..Store::default() }
    }

    /// Performs one `Persist` effect's ops in order (engine.md §7.11).
    /// `history` is the sealer's control history when the effect was emitted.
    pub(crate) fn apply(&mut self, ops: &[PersistOp], history: Option<&Vec<Vec<u8>>>) {
        for op in ops {
            match op {
                PersistOp::AppendLog(rs) => {
                    for r in rs {
                        let fh: Hash32 = sha2::Sha256::digest(&r.frame).into();
                        self.by_hash.insert(fh, r.seq);
                        self.log_hash.insert(r.seq, fh);
                        self.log.insert(r.seq, r.clone());
                    }
                }
                PersistOp::SetMarker(m) => self.marker = Some(*m),
                PersistOp::ClearMarker => self.marker = None,
                PersistOp::WriteCheckpoint(c) => {
                    self.checkpoint = Some(c.clone());
                    self.bootstrap = None;
                }
                PersistOp::WriteBase(c) => {
                    self.bases.insert(c.board_seq, c.clone());
                }
                PersistOp::DeleteBases(seqs) => {
                    for s in seqs {
                        self.bases.remove(s);
                    }
                }
                PersistOp::TruncateLogBelow(seq, fc) => {
                    self.log = self.log.split_off(seq);
                    self.log_hash = self.log_hash.split_off(seq);
                    self.log_floor = *seq;
                    self.floor_chain = *fc;
                }
                PersistOp::AppendHashes(hs) => self.hashes.extend_from_slice(hs),
                PersistOp::Outbox(ops) => self.outbox.extend(ops.iter().cloned()),
                PersistOp::RewriteOutbox(recs) => {
                    self.outbox = recs.iter().cloned().map(OutboxOp::Put).collect();
                }
                PersistOp::Notices(ops) => self.notices.extend(ops.iter().cloned()),
                PersistOp::RewriteNotices(ops) => self.notices = ops.clone(),
                PersistOp::WriteMeta(m) => self.meta = Some(m.clone()),
                PersistOp::WriteIdentity(id) => self.identity = id.to_vec(),
            }
        }
        if let Some(h) = history {
            self.history = h.clone();
        }
    }

    /// The live outbox records, the newest `Put` of each `entry_id` winning
    /// and a `Drop` removing it (ENG-STATE-003), in file order of first `Put`.
    pub(crate) fn live_outbox(&self) -> Vec<OutboxRecord> {
        let mut order: Vec<[u8; 16]> = Vec::new();
        let mut live: BTreeMap<[u8; 16], OutboxRecord> = BTreeMap::new();
        for op in &self.outbox {
            match op {
                OutboxOp::Put(r) => {
                    if live.insert(r.entry_id, r.clone()).is_none() {
                        order.push(r.entry_id);
                    }
                }
                OutboxOp::Drop(id) => {
                    live.remove(id);
                    order.retain(|x| x != id);
                }
            }
        }
        order.iter().filter_map(|id| live.get(id).cloned()).collect()
    }

    /// The hashes of the frames this replica holds: its log's and its live
    /// outbox records' (what a device "holds" when it raises an alarm, §31.7).
    pub(crate) fn held_frames(&self) -> BTreeSet<Hash32> {
        let mut out: BTreeSet<Hash32> = self.log_hash.values().copied().collect();
        for r in self.live_outbox() {
            if let Some(f) = &r.frame {
                out.insert(sha2::Sha256::digest(f).into());
            }
        }
        out
    }

    /// The log head `(L, C_L)` the store holds.
    pub(crate) fn head(&self) -> Option<(u64, Hash32)> {
        self.log.values().next_back().map(|r| (r.seq, r.chain))
    }

    /// The log head `(L, C_L)` and the first seq of the unbroken run of rows
    /// that ends there: the seqs a restore tail can be read from (engine.md
    /// §21.3 step 1, `ReadLog`).
    pub(crate) fn run_head(&self) -> Option<(u64, Head)> {
        let (top, chain) = self.head()?;
        let mut lo = top;
        for s in self.log.keys().rev().skip(1) {
            if *s + 1 != lo {
                break;
            }
            lo = *s;
        }
        Some((lo, Head { seq: top, chain }))
    }

    /// The chain value this replica holds for `seq`, from its log, its
    /// checkpoint or its floor chain.
    pub(crate) fn chain_at(&self, seq: u64) -> Option<Hash32> {
        if let Some(r) = self.log.get(&seq) {
            return Some(r.chain);
        }
        if let Some(c) = &self.checkpoint
            && c.board_seq == seq {
                return Some(c.board_chain);
            }
        if seq + 1 == self.log_floor {
            return self.floor_chain;
        }
        None
    }

    /// What `Start` carries (engine.md §7.10).
    pub(crate) fn loaded(&self, build_id: &str, session: [u8; 16]) -> Loaded {
        let b = self.checkpoint.as_ref().map_or(0, |c| c.board_seq);
        Loaded {
            checkpoint: self.checkpoint.clone(),
            bases: self.bases.values().map(|c| (c.board_seq, c.board_chain)).collect(),
            chains: self.log.values().map(|r| (r.seq, r.chain, r.tau)).collect(),
            log_floor: self.log_floor,
            floor_chain: self.floor_chain,
            tail: self.log.range(b + 1..).map(|(_, r)| r.clone()).collect(),
            hashes: self.hashes.clone(),
            outbox: self.outbox.clone(),
            notices: self.notices.clone(),
            meta: self.meta.clone(),
            marker: self.marker,
            build_id: build_id.to_owned(),
            session,
            bootstrap: self.bootstrap.clone(),
        }
    }
}
