//! Blobs (`relay.md` §10.7) and their retention (§13).

use super::util::{MIB, refuse};
use super::{BlobRow, BoardState, Cred, ModelRelay, Reply};
use crate::{BlobMeta, BlobOk, BlobRequest, Hash32};

/// The total bytes of all blobs a board may hold (§10.7 step 5).
const BLOB_BYTES: u64 = 805_306_368;
/// How long incomplete blobs and pairing snapshots live (§13.2).
const BLOB_HOUR: i64 = 3_600_000;

impl ModelRelay {
    fn delete_blob(&mut self, id: &[u8; 16]) {
        if let Some(b) = self.blobs.remove(id) {
            for i in 0..64 {
                self.parts.remove(&(b.slot * 64 + i));
            }
        }
    }

    /// Deletes every blob `pred` selects, with its parts (§13.3); their ids.
    pub(crate) fn delete_blobs(&mut self, pred: impl Fn(&BlobRow) -> bool) -> Vec<[u8; 16]> {
        let ids: Vec<[u8; 16]> = self.blobs.values().filter(|b| pred(b)).map(|b| b.id).collect();
        for id in &ids {
            self.delete_blob(id);
        }
        ids
    }

    /// `blob-begin` (§10.7) with a role-1 token: the lazy deletions of §13.2
    /// first (kept whatever follows), then the checks in order. The answer is
    /// the blob's `parts`.
    #[allow(clippy::too_many_arguments)]
    pub fn blob_begin(&mut self, now: i64, cred: &Cred, id: &[u8; 16], purpose: u8, seq: u64, size: u64, sha256: &Hash32) -> Reply<u32> {
        let row = match self.token_route(cred, &[Some(BoardState::Open)]) {
            Ok(r) => r,
            Err(e) => return Reply::err(e),
        };
        // §13.2 (RELAY-BLOB-006).
        self.delete_blobs(|b| b.at <= now - BLOB_HOUR && (!b.complete || b.purpose == 2));
        if !(67..=67_108_864).contains(&size) || seq > super::util::MAX_SAFE {
            return Reply::err(refuse("relay/bad_blob"));
        }
        let head = self.head_at(self.brd()).seq;
        let ok = match purpose {
            1 => seq >= 64 && seq.is_multiple_of(64) && seq <= head,
            2 => (1..=head).contains(&seq),
            3 => seq == 1,
            4 => seq == 0,
            _ => false,
        };
        if !ok {
            return Reply::err(refuse("relay/bad_blob"));
        }
        if purpose == 3 && self.blobs.values().any(|b| b.purpose == 3 && b.id != *id) {
            return Reply::err(refuse("relay/blob_exists"));
        }
        let parts = size.div_ceil(MIB as u64) as u32;
        if let Some(same) = self.blobs.get(id) {
            let repeat = same.author == row.dev && same.purpose == purpose && same.seq == seq && same.size == size && same.sha256 == *sha256;
            return if repeat { Reply::ok(parts) } else { Reply::err(refuse("relay/blob_exists")) };
        }
        let incomplete = self.blobs.values().filter(|b| !b.complete).count();
        let bytes: u64 = self.blobs.values().map(|b| b.size).sum();
        if incomplete >= 3 || bytes + size > BLOB_BYTES {
            return Reply::err(refuse("relay/blob_quota"));
        }
        let slot = 1 + self.blobs.values().map(|b| b.slot).max().unwrap_or(0);
        self.blobs.insert(
            *id,
            BlobRow { id: *id, slot, purpose, seq, author: row.dev, size, parts, sha256: *sha256, received: 0, complete: false, at: now },
        );
        Reply::ok(parts)
    }

    /// `blob-put` of part `index` (§10.7): the answer is `(received, complete)`.
    pub fn blob_put(&mut self, _now: i64, cred: &Cred, id: &[u8; 16], index: u32, data: &[u8]) -> Reply<(u32, bool)> {
        let row = match self.token_route(cred, &[Some(BoardState::Open)]) {
            Ok(r) => r,
            Err(e) => return Reply::err(e),
        };
        let Some(b) = self.blobs.get(id).cloned() else { return Reply::err(refuse("relay/no_blob")) };
        if b.author != row.dev {
            return Reply::err(refuse("relay/forbidden"));
        }
        if index >= b.parts {
            return Reply::err(refuse("relay/bad_blob"));
        }
        let want = if index + 1 < b.parts { MIB as u64 } else { b.size - u64::from(b.parts - 1) * MIB as u64 };
        if data.len() as u64 != want {
            return Reply::err(refuse("relay/bad_blob"));
        }
        let pk = b.slot * 64 + u64::from(index);
        if let Some(old) = self.parts.get(&pk) {
            if old.as_slice() != data {
                return Reply::err(refuse("relay/part_conflict"));
            }
            return Reply::ok((b.received, b.complete));
        }
        self.parts.insert(pk, data.to_vec());
        let received = b.received + 1;
        let complete = received == b.parts;
        if let Some(x) = self.blobs.get_mut(id) {
            x.received = received;
            x.complete = complete;
        }
        if complete {
            self.retain(&b);
        }
        Reply::ok((received, complete))
    }

    /// §13.1, in the transaction of the `blob-put` that completed `done`.
    fn retain(&mut self, done: &BlobRow) {
        let mut all: Vec<BlobRow> = self.blobs.values().filter(|b| b.complete && b.purpose == done.purpose).cloned().collect();
        let mut gone = Vec::new();
        if done.purpose == 1 {
            all.sort_by(|x, y| y.seq.cmp(&x.seq).then(y.at.cmp(&x.at)).then(x.id.cmp(&y.id)));
            let snap = self.brd().compact_snap;
            let mut authors = std::collections::BTreeSet::new();
            for (i, b) in all.iter().enumerate() {
                let first = authors.insert(b.author);
                if !(i < 3 || first || snap == Some(b.id)) {
                    gone.push(b.id);
                }
            }
        } else if done.purpose == 4 {
            all.retain(|b| b.author == done.author);
            all.sort_by(|x, y| y.at.cmp(&x.at).then(x.id.cmp(&y.id)));
            gone.extend(all.iter().skip(2).map(|b| b.id));
        }
        for id in gone {
            self.delete_blob(&id);
        }
    }

    /// `blob-meta` (§10.7), for any blob, complete or not.
    pub fn blob_meta(&self, cred: &Cred, id: &[u8; 16]) -> Reply<BlobRow> {
        if let Err(e) = self.token_route(cred, &[Some(BoardState::Open), Some(BoardState::Fenced)]) {
            return Reply::err(e);
        }
        match self.blobs.get(id) {
            Some(b) => Reply::ok(b.clone()),
            None => Reply::err(refuse("relay/no_blob")),
        }
    }

    /// `blob-list` (§10.7): the first 64 complete blobs, by purpose ascending,
    /// seq descending, `at` descending, id ascending.
    pub fn blob_list(&self, cred: &Cred) -> Reply<Vec<BlobRow>> {
        if let Err(e) = self.token_route(cred, &[Some(BoardState::Open), Some(BoardState::Fenced)]) {
            return Reply::err(e);
        }
        let mut done: Vec<BlobRow> = self.blobs.values().filter(|b| b.complete).cloned().collect();
        done.sort_by(|x, y| x.purpose.cmp(&y.purpose).then(y.seq.cmp(&x.seq)).then(y.at.cmp(&x.at)).then(x.id.cmp(&y.id)));
        done.truncate(64);
        Reply::ok(done)
    }

    /// `blob-get` of part `index` of a complete blob (§10.7).
    pub fn blob_get(&self, cred: &Cred, id: &[u8; 16], index: u32) -> Reply<Vec<u8>> {
        if let Err(e) = self.token_route(cred, &[Some(BoardState::Open), Some(BoardState::Fenced)]) {
            return Reply::err(e);
        }
        let Some(b) = self.blobs.get(id) else { return Reply::err(refuse("relay/no_blob")) };
        if !b.complete {
            return Reply::err(refuse("relay/blob_incomplete"));
        }
        if index >= b.parts {
            return Reply::err(refuse("relay/bad_blob"));
        }
        match self.parts.get(&(b.slot * 64 + u64::from(index))) {
            Some(p) => Reply::ok(p.clone()),
            None => Reply::err(refuse("relay/no_blob")),
        }
    }

    /// The engine's blob request (engine.md §19.6): `Upload` is `blob-begin`
    /// then every part in order (a part already stored with the same bytes
    /// passes), `Fetch` every part, `List` and `Meta` as their routes. The
    /// first refusal ends the request.
    pub fn blob(&mut self, now: i64, cred: &Cred, req: &BlobRequest) -> Reply<BlobOk> {
        match req {
            BlobRequest::Upload { purpose, seq, blob } => {
                let size = blob.bytes.len() as u64;
                let r = self.blob_begin(now, cred, &blob.id, *purpose, *seq, size, &blob.sha256);
                if let Err(e) = r.result {
                    return Reply::err(e);
                }
                for (i, chunk) in blob.bytes.chunks(MIB).enumerate() {
                    if let Err(e) = self.blob_put(now, cred, &blob.id, i as u32, chunk).result {
                        return Reply::err(e);
                    }
                }
                Reply::ok(BlobOk::Uploaded)
            }
            BlobRequest::Fetch { id, parts } => {
                let mut out = Vec::new();
                for i in 0..*parts {
                    match self.blob_get(cred, id, i).result {
                        Ok(p) => out.push(p),
                        Err(e) => return Reply::err(e),
                    }
                }
                Reply::ok(BlobOk::Parts(out))
            }
            BlobRequest::List => match self.blob_list(cred).result {
                Ok(v) => Reply::ok(BlobOk::List(v.iter().map(meta).collect())),
                Err(e) => Reply::err(e),
            },
            BlobRequest::Meta { id } => match self.blob_meta(cred, id).result {
                Ok(b) => Reply::ok(BlobOk::Meta(meta(&b))),
                Err(e) => Reply::err(e),
            },
        }
    }
}

/// The engine's view of a blob row.
pub fn meta(b: &BlobRow) -> BlobMeta {
    BlobMeta {
        id: b.id,
        purpose: b.purpose,
        seq: b.seq,
        author: b.author,
        size: b.size,
        parts: b.parts,
        sha256: b.sha256,
        complete: b.complete,
    }
}
