//! Login (`relay.md` §10.3, without nonce and signature) and the admin
//! operations of §12.3 to §12.10 and §12.12, with the common checks of §12.1
//! that the model keeps (A1's counts, A2, A3, A5).

use super::seq::{new_device, safe};
use super::util::{self, DAY, DELETE_BUDGET, Fail, MAX_GEN, MIB, refuse, valid_host};
use super::{BoardRow, BoardState, Cred, ModelRelay, Out, Reply};
use crate::{AdminOk, AdminRequest, Dev, Hash32, Head, SeedDevice};

/// A successful login (§10.3): the new generation, and the wraps the
/// device's row stores (`first_epoch`, wraps) while it stores them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Login {
    pub generation: u32,
    pub wraps: Option<(u32, Vec<[u8; 96]>)>,
}

/// An admin operation with every argument the relay reads (`relay.md` §12.1
/// "Per operation", the `seal.md` §16.2 layouts). [`ModelRelay::admin`]
/// builds these from the engine's [`AdminRequest`]; a test that needs a
/// mismatching `frame_hash` or hash list states it here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdminOp {
    /// §12.3; `frame` is the frame mode, `seq` the seq mode.
    Register { dev: Dev, sign_pk: Hash32, frame_hash: Hash32, frame: Option<Vec<u8>>, seq: u64 },
    /// §12.4.
    Revoke { dev: Dev },
    /// §12.5.
    Reinstate { dev: Dev, first_epoch: u32, wraps: Vec<[u8; 96]> },
    /// §12.6.
    Fence { host: String, door: [u8; 5], home_gen: u32, frame: Option<Vec<u8>> },
    /// §12.7.
    Seed {
        home_gen: u32,
        first_seq: u64,
        chain_before: Hash32,
        last_seq: u64,
        chain_last: Hash32,
        blob_id: [u8; 16],
        devices: Vec<SeedDevice>,
    },
    /// §12.8.
    Compact { below_seq: u64, snap_entry_seq: u64, blob_id: [u8; 16], keep: Vec<u64> },
    /// §12.9; `hashes` are the signed `frame_hash` list.
    RestoreTail { first_seq: u64, hashes: Vec<Hash32>, chain_before: Hash32, frames: Vec<Vec<u8>> },
    /// §12.10.
    Purge { before_seq: u64, epoch: u32 },
    /// §12.12.
    RecoveryRegister { dev_r: Dev, sign_pk: Hash32, frame_hash: Hash32, seq: u64 },
}

/// What an operation's own part (A7) gives back.
struct Done {
    ok: AdminOk,
    token_gen: Option<u32>,
    out: Vec<Out>,
}


fn fail<T>(code: &str) -> Result<T, Fail> {
    Err(Box::new((refuse(code), None)))
}

/// An `AdminOk` with nothing set.
pub(crate) fn ok0() -> AdminOk {
    AdminOk {
        seq: None,
        chain: None,
        token: false,
        deleted: None,
        floor: None,
        more: None,
        purge_floor: None,
        done: None,
        head: None,
        door: None,
    }
}

fn done(ok: AdminOk) -> Result<Done, Fail> {
    Ok(Done { ok, token_gen: None, out: Vec::new() })
}

fn at(seq: u64, chain: Hash32) -> AdminOk {
    AdminOk { seq: Some(seq), chain: Some(chain), ..ok0() }
}

impl AdminOp {
    /// The "Credentials" column of §12.1: (token, door).
    fn creds(&self) -> (bool, bool) {
        match self {
            AdminOp::Register { .. } | AdminOp::Compact { .. } | AdminOp::Purge { .. } => (true, false),
            AdminOp::Seed { .. } => (false, false),
            _ => (true, true),
        }
    }
    /// The "Signer must be" column of §12.1: the role, 0 for either.
    fn signer_role(&self) -> u8 {
        match self {
            AdminOp::Register { .. } | AdminOp::Seed { .. } | AdminOp::Compact { .. } | AdminOp::Purge { .. } => 1,
            AdminOp::Reinstate { .. } => 2,
            _ => 0,
        }
    }
    /// The counts and layouts A1 checks that a typed request can still break.
    fn a1(&self) -> Result<(), &'static str> {
        let frame_ok = |f: &Vec<u8>| (670..=262_144).contains(&f.len());
        match self {
            AdminOp::Register { frame: Some(f), .. } if !frame_ok(f) => Err("relay/bad_body"),
            AdminOp::Fence { frame: Some(f), .. } if !frame_ok(f) => Err("relay/bad_body"),
            AdminOp::Fence { host, .. } if host.len() > 255 => Err("relay/bad_args"),
            AdminOp::Reinstate { wraps, .. } if wraps.is_empty() || wraps.len() > 256 => Err("relay/bad_args"),
            AdminOp::Seed { devices, .. } if devices.is_empty() || devices.len() > 320 => Err("relay/bad_args"),
            AdminOp::Compact { keep, .. } if keep.len() > 131_072 => Err("relay/bad_args"),
            AdminOp::RestoreTail { hashes, .. } if hashes.is_empty() || hashes.len() > 64 => Err("relay/bad_args"),
            AdminOp::RestoreTail { hashes, frames, .. } => {
                if frames.len() != hashes.len() || frames.iter().map(Vec::len).sum::<usize>() > MIB {
                    Err("relay/bad_body")
                } else {
                    Ok(())
                }
            }
            _ => Ok(()),
        }
    }
}

impl ModelRelay {
    /// An admin request of the engine (engine.md §11.7) by `signer`, with the
    /// credential the runtime sends it with: `Door` and `Seed` with
    /// [`Cred::Create`], `SeedTail` with the seeding device's token, the rest
    /// with a token or the door. `Door` answers the door; `SeedTail` is
    /// [`ModelRelay::seed_tail`]; every other request is [`ModelRelay::op`].
    pub fn admin(&mut self, now: i64, cred: &Cred, signer: &Dev, req: &AdminRequest) -> Reply<AdminOk> {
        let op = match req.clone() {
            AdminRequest::Door { .. } => {
                if *cred != Cred::Create {
                    return Reply::err(refuse("relay/forbidden"));
                }
                return Reply::ok(AdminOk { door: Some(self.door()), ..ok0() });
            }
            AdminRequest::SeedTail { first, frames, .. } => return self.seed_tail(now, cred, first, &frames),
            AdminRequest::Register { frame, dev, sign_pk } => {
                AdminOp::Register { dev, sign_pk, frame_hash: util::h(&[&frame]), frame: Some(frame), seq: 0 }
            }
            AdminRequest::RegisterSeq { seq, dev, sign_pk, frame_hash } => AdminOp::Register { dev, sign_pk, frame_hash, frame: None, seq },
            AdminRequest::RecoveryRegisterSeq { seq, dev_r, sign_pk, frame_hash } => {
                AdminOp::RecoveryRegister { dev_r, sign_pk, frame_hash, seq }
            }
            AdminRequest::Revoke { dev } => AdminOp::Revoke { dev },
            AdminRequest::Fence { host, door, home_gen, frame } => AdminOp::Fence { host, door, home_gen, frame },
            AdminRequest::Seed { home_gen, first_seq, chain_before, last_seq, chain_last, blob_id, devices, .. } => {
                AdminOp::Seed { home_gen, first_seq, chain_before, last_seq, chain_last, blob_id, devices }
            }
            AdminRequest::Compact { below_seq, snap_entry_seq, blob_id, keep } => AdminOp::Compact { below_seq, snap_entry_seq, blob_id, keep },
            AdminRequest::RestoreTail { first_seq, chain_before, frames } => {
                let hashes = frames.iter().map(|f| util::h(&[f])).collect();
                AdminOp::RestoreTail { first_seq, hashes, chain_before, frames }
            }
            AdminRequest::Purge { before_seq, epoch } => AdminOp::Purge { before_seq, epoch },
        };
        self.op(now, cred, signer, &op)
    }

    /// An admin operation (§12.1): the token's row, A1's counts, A2, A3, A5,
    /// then the operation's own checks and effects (A7). Nonces (A4) and the
    /// signature (A6) are not modelled.
    pub fn op(&mut self, now: i64, cred: &Cred, signer: &Dev, op: &AdminOp) -> Reply<AdminOk> {
        match self.op_inner(now, cred, signer, op) {
            Ok(d) => Reply { result: Ok(d.ok), last: None, token_gen: d.token_gen, out: d.out },
            Err(b) => {
                let (r, last) = *b;
                Reply { result: Err(r), last, token_gen: None, out: Vec::new() }
            }
        }
    }

    fn op_inner(&mut self, now: i64, cred: &Cred, signer: &Dev, op: &AdminOp) -> Result<Done, Fail> {
        let is_seed = matches!(op, AdminOp::Seed { .. });
        // The credential form the gate admits for the operation (§6.3, §12.1 A3).
        let (token, door) = op.creds();
        let form_ok = match cred {
            Cred::Token { .. } => token,
            Cred::Door => door,
            Cred::Create => is_seed,
        };
        if !form_ok {
            return fail("relay/forbidden");
        }
        self.cred_row(cred, false).map_err(|e| Box::new((e, None)))?; // RELAY-OBJ-003
        if let Err(code) = op.a1() {
            return fail(code); // A1
        }
        // A2
        let allowed: &[Option<BoardState>] = if is_seed { &[None, Some(BoardState::Seeding)] } else { &[Some(BoardState::Open)] };
        self.need(cred, allowed).map_err(|e| Box::new((e, None)))?;
        // A3
        let signer_row = self.devices.get(signer).cloned();
        if !is_seed {
            let own = matches!(cred, Cred::Token { dev, .. } if dev == signer);
            if !own && !signer_row.as_ref().is_some_and(|r| r.role == 2) {
                return fail("relay/forbidden");
            }
        }
        // A5
        let who = match op {
            AdminOp::Seed { devices, .. } => devices.iter().find(|e| e.dev == *signer).map(|e| (e.role, e.status)),
            _ => signer_row.as_ref().map(|r| (r.role, r.state)),
        };
        let role = op.signer_role();
        match who {
            Some((r, 1)) if role == 0 || r == role => {}
            _ => return fail("relay/not_signer"),
        }
        // A7
        match op {
            AdminOp::Register { dev, sign_pk, frame_hash, frame, seq } => self.register(now, cred, signer, dev, sign_pk, frame_hash, frame.as_deref(), *seq),
            AdminOp::Revoke { dev } => self.revoke(signer_row.as_ref().map(|r| r.role), signer, dev),
            AdminOp::Reinstate { dev, first_epoch, wraps } => self.reinstate(dev, *first_epoch, wraps),
            AdminOp::Fence { host, door, home_gen, frame } => self.fence(now, cred, signer, host, door, *home_gen, frame.as_deref()),
            AdminOp::Seed { .. } => self.seed(now, signer, op),
            AdminOp::Compact { below_seq, snap_entry_seq, blob_id, keep } => self.compact(now, signer, *below_seq, *snap_entry_seq, blob_id, keep),
            AdminOp::RestoreTail { first_seq, hashes, chain_before, frames } => self.restore_tail(now, *first_seq, hashes, chain_before, frames),
            AdminOp::Purge { before_seq, epoch } => self.purge(now, *before_seq, *epoch),
            AdminOp::RecoveryRegister { dev_r, sign_pk, frame_hash, seq } => self.recovery_register(now, dev_r, sign_pk, frame_hash, *seq),
        }
    }

    // ---------- §12.3 `register`

    #[allow(clippy::too_many_arguments)]
    fn register(
        &mut self,
        now: i64,
        cred: &Cred,
        signer: &Dev,
        dev: &Dev,
        pk: &Hash32,
        hash: &Hash32,
        frame: Option<&[u8]>,
        seq: u64,
    ) -> Result<Done, Fail> {
        let same = self.devices.get(dev).cloned();
        let repeat = same.as_ref().is_some_and(|d| d.role == 1 && d.state == 1 && d.sign_pk == *pk);
        if !repeat {
            if same.is_some() || self.devices.values().any(|d| d.sign_pk == *pk) {
                return fail("relay/duplicate_key");
            }
            let current = self.devices.values().filter(|d| d.role == 1 && d.state == 1).count();
            let all = self.devices.values().filter(|d| d.role == 1).count();
            if current >= 16 || all >= 256 {
                return fail("relay/device_cap");
            }
        }
        let mut carry = None;
        let (mut at_seq, mut at_chain) = (0, [0u8; 32]);
        if let Some(frame) = frame {
            let mine = if util::h(&[frame]) != *hash {
                Some("relay/frame_mismatch")
            } else if frame[20..36] != signer[..] {
                Some("relay/not_author")
            } else {
                None
            };
            if repeat {
                // relay-core.js: the repeat holds only for the stored frame (`dup`).
                if mine.is_some() {
                    return fail("relay/duplicate_key");
                }
                match self.carried(now, cred, frame) {
                    Ok((_, Some(h))) => (at_seq, at_chain) = (h.seq, h.chain),
                    _ => return fail("relay/duplicate_key"),
                }
            } else {
                if let Some(code) = mine {
                    return fail(code);
                }
                match self.carried(now, cred, frame)? {
                    (f, None) => carry = Some(f),
                    (f, Some(_)) => {
                        // Stored already by a plain append: a conflict with the author's counters.
                        let a = &self.devices[&f.dev];
                        return Err(Box::new((refuse("relay/conflict"), Some((a.last_ctr, a.last_seq)))));
                    }
                }
            }
        } else {
            let b = self.brd();
            match safe(seq).and_then(|s| self.log.get(&s)) {
                Some(r) if r.seq >= b.purge_floor && r.hash == *hash => (at_seq, at_chain) = (r.seq, r.chain),
                _ => return fail(if repeat { "relay/duplicate_key" } else { "relay/frame_mismatch" }),
            }
        }
        if repeat {
            let d = same.expect("a repeat has its row");
            if d.generation == MAX_GEN {
                return fail("relay/gen_exhausted");
            }
            if let Some(r) = self.devices.get_mut(dev) {
                r.generation = d.generation + 1;
            }
            return Ok(Done { ok: AdminOk { token: true, ..at(at_seq, at_chain) }, token_gen: Some(d.generation + 1), out: Vec::new() });
        }
        let mut stored = Vec::new();
        if let Some(f) = carry {
            let h = self.store_carried(now, f);
            (at_seq, at_chain) = (h.seq, h.chain);
            stored.push(h.seq);
        }
        self.devices.insert(*dev, new_device(*dev, *pk, 1, 1, 1, 0, 0, now));
        let out = self.push(now, &stored);
        Ok(Done { ok: AdminOk { token: true, ..at(at_seq, at_chain) }, token_gen: Some(1), out })
    }

    // ---------- §12.4 `revoke`, §12.5 `reinstate`

    fn revoke(&mut self, signer_role: Option<u8>, signer: &Dev, target: &Dev) -> Result<Done, Fail> {
        match self.devices.get(target) {
            Some(t) if t.role == 1 && t.state == 1 => {}
            _ => return fail("relay/target"),
        }
        if signer_role == Some(1) && signer == target {
            return fail("relay/not_signer");
        }
        if self.devices.values().filter(|d| d.role == 1 && d.state == 1).count() < 2 {
            return fail("relay/last_device");
        }
        if let Some(t) = self.devices.get_mut(target) {
            t.state = 2;
            t.generation = t.generation.saturating_add(1);
            t.wraps = None;
        }
        let out = self.close_dev(target, |_| true, 4003);
        Ok(Done { ok: ok0(), token_gen: None, out })
    }

    fn reinstate(&mut self, dev: &Dev, first_epoch: u32, wraps: &[[u8; 96]]) -> Result<Done, Fail> {
        if first_epoch < 2 {
            return fail("relay/bad_args");
        }
        match self.devices.get(dev) {
            Some(t) if t.role == 1 && t.state == 2 => {}
            _ => return fail("relay/target"),
        }
        if self.devices.values().filter(|d| d.role == 1 && d.state == 1).count() >= 16 {
            return fail("relay/device_cap");
        }
        if let Some(t) = self.devices.get_mut(dev) {
            t.state = 1;
            t.wraps = Some((first_epoch, wraps.to_vec()));
        }
        done(ok0())
    }

    // ---------- §12.6 `fence`

    #[allow(clippy::too_many_arguments)]
    fn fence(&mut self, now: i64, cred: &Cred, signer: &Dev, host: &str, door: &[u8; 5], home_gen: u32, frame: Option<&[u8]>) -> Result<Done, Fail> {
        if !valid_host(host) {
            return fail("relay/bad_args");
        }
        if home_gen <= self.brd().home_gen {
            return fail("relay/home_gen");
        }
        let mut ok = ok0();
        let mut stored = Vec::new();
        if let Some(frame) = frame {
            if frame[20..36] != signer[..] {
                return fail("relay/not_author");
            }
            match self.carried(now, cred, frame)? {
                (_, Some(h)) => ok = at(h.seq, h.chain),
                (f, None) => {
                    let h = self.store_carried(now, f);
                    ok = at(h.seq, h.chain);
                    stored.push(h.seq);
                }
            }
        }
        if let Some(b) = self.board.as_mut() {
            b.state = BoardState::Fenced;
            b.home_gen = home_gen;
            b.fence_host = Some(host.to_string());
            b.fence_door = Some(*door);
        }
        let mut out = self.push(now, &stored);
        out.extend(self.state_notice());
        Ok(Done { ok, token_gen: None, out })
    }

    // ---------- §12.7 `seed` and `seed-tail`

    fn seed(&mut self, now: i64, signer: &Dev, op: &AdminOp) -> Result<Done, Fail> {
        let AdminOp::Seed { home_gen, first_seq, chain_before, last_seq, chain_last, devices: e, .. } = op else {
            return fail("relay/bad_args");
        };
        // 1 (a u64 above 2^53 − 1 fails the check that names it, RELAY-GEN-008)
        let (Some(first), Some(last)) = (safe(*first_seq), safe(*last_seq)) else { return fail("relay/bad_args") };
        if *home_gen < 2 || first < 1 || last + 1 < first || (last + 1 == first && chain_last != chain_before) {
            return fail("relay/bad_args");
        }
        // 2
        for (i, x) in e.iter().enumerate() {
            if (i > 0 && e[i - 1].dev >= x.dev) || !(1..=2).contains(&x.role) || !(1..=2).contains(&x.status) || safe(x.last_ctr).is_none() {
                return fail("relay/bad_args");
            }
        }
        let keys: std::collections::BTreeSet<Hash32> = e.iter().map(|x| x.sign_pk).collect();
        if keys.len() != e.len() {
            return fail("relay/bad_args");
        }
        // 3
        let rec: Vec<&SeedDevice> = e.iter().filter(|x| x.role == 2).collect();
        if rec.iter().any(|x| x.dev != util::fake_dev_r(&self.board_id, &x.sign_pk)) || rec.iter().filter(|x| x.status == 1).count() != 1 || rec.len() > 64 {
            return fail("relay/bad_args");
        }
        // 4
        let current = e.iter().filter(|x| x.role == 1 && x.status == 1).count();
        if !(1..=16).contains(&current) || e.iter().filter(|x| x.role == 1).count() > 256 {
            return fail("relay/bad_args");
        }
        // A new seed replaces an unfinished one.
        self.devices.clear();
        self.log.clear();
        self.dev_ctr.clear();
        self.blobs.clear();
        self.parts.clear();
        let open = last + 1 == first;
        self.board = Some(BoardRow {
            state: if open { BoardState::Open } else { BoardState::Seeding },
            created_at: now,
            base_seq: first - 1,
            base_chain: *chain_before,
            compact_floor: first,
            purge_floor: first,
            purge_done: first,
            compact_snap: None,
            del_day: 0,
            del_count: 0,
            read_epoch: 1,
            home_gen: *home_gen,
            fence_host: None,
            fence_door: None,
            seed_signer: (!open).then_some(*signer),
            seed_last_seq: (!open).then_some(last),
            seed_chain: (!open).then_some(*chain_last),
        });
        for x in e {
            let generation = u32::from(x.dev == *signer);
            self.devices.insert(x.dev, new_device(x.dev, x.sign_pk, x.role, x.status, generation, x.last_ctr, 0, now));
        }
        Ok(Done { ok: AdminOk { token: true, door: Some(self.door()), ..ok0() }, token_gen: Some(1), out: Vec::new() })
    }

    /// `seed-tail` (§12.7) with the seeding device's token: the frames at
    /// `first` onward. The answer's `head` is the head after it; the board is
    /// open again once the tail reached `seed_last_seq` ([`ModelRelay::state`]).
    pub fn seed_tail(&mut self, now: i64, cred: &Cred, first: u64, frames: &[Vec<u8>]) -> Reply<AdminOk> {
        let row = match self.token_route(cred, &[Some(BoardState::Seeding)]) {
            Ok(r) => r,
            Err(e) => return Reply::err(e),
        };
        let b = self.brd().clone();
        if b.seed_signer != Some(row.dev) {
            return Reply::err(refuse("relay/forbidden"));
        }
        if frames.is_empty() || frames.len() > 256 || frames.iter().map(Vec::len).sum::<usize>() > MIB {
            return Reply::err(refuse("relay/bad_body"));
        }
        let head = self.head_at(&b);
        if safe(first) != Some(head.seq + 1) {
            let mut r = refuse("relay/not_next");
            r.head = Some(Head { seq: head.seq, chain: head.chain });
            return Reply::err(r);
        }
        let last = b.seed_last_seq.unwrap_or(0);
        if first + frames.len() as u64 - 1 > last {
            return Reply::err(refuse("relay/target"));
        }
        let mut open = false;
        let res = self.tail(now, frames, |_, _| None, |end| {
            if end.seq == last {
                if Some(end.chain) != b.seed_chain {
                    return Err(refuse("relay/chain_mismatch"));
                }
                open = true;
            }
            Ok(())
        });
        match res {
            Ok((h, _)) => {
                if open && let Some(b) = self.board.as_mut() {
                    b.state = BoardState::Open;
                    b.seed_signer = None;
                    b.seed_last_seq = None;
                    b.seed_chain = None;
                }
                Reply::ok(AdminOk { head: Some(Head { seq: h.seq, chain: h.chain }), ..ok0() })
            }
            Err(e) => Reply::err(e),
        }
    }

    // ---------- §12.8 `compact`

    fn compact(&mut self, now: i64, signer: &Dev, below: u64, snap_seq: u64, blob_id: &[u8; 16], keep: &[u64]) -> Result<Done, Fail> {
        let below_v = safe(below);
        // 1
        for (i, k) in keep.iter().enumerate() {
            if safe(*k).is_none() || below_v.is_some_and(|b| *k >= b) || (i > 0 && *k <= keep[i - 1]) {
                return fail("relay/bad_args");
            }
        }
        let b = self.brd().clone();
        let lo = b.compact_floor.max(b.purge_floor);
        let head = self.head_at(&b);
        // 2
        let Some(below) = below_v.filter(|x| lo < *x && *x + 16_384 <= head.seq) else { return fail("relay/target") };
        // 3
        let Some(snap) = safe(snap_seq).and_then(|s| self.log.get(&s)).filter(|r| r.seq >= b.purge_floor) else {
            return fail("relay/no_snapshot");
        };
        let author = snap.dev;
        // 4
        match self.blobs.get(blob_id) {
            Some(x) if x.complete && x.purpose == 1 && x.author == author && x.seq >= below && x.seq < snap_seq => {}
            _ => return fail("relay/no_snapshot"),
        }
        // 5
        if author == *signer {
            return fail("relay/not_signer");
        }
        // 6
        let day = now.div_euclid(DAY);
        let used = if b.del_day == day { b.del_count } else { 0 };
        if used >= DELETE_BUDGET {
            let mut r = refuse("relay/delete_budget");
            r.until = Some((day + 1) * DAY);
            return Err(Box::new((r, None)));
        }
        // 7
        let hi = below.min(lo + (DELETE_BUDGET - used));
        match self.log.get(&(hi - 1)) {
            Some(edge) if edge.at <= now - 31_536_000_000 => {}
            _ => return fail("relay/too_recent"),
        }
        let mut d = 0;
        for s in lo..hi {
            if keep.binary_search(&s).is_err() && self.delete_log(s) {
                d += 1;
            }
        }
        if let Some(b) = self.board.as_mut() {
            b.compact_floor = hi;
            b.compact_snap = Some(*blob_id);
            b.del_day = day;
            b.del_count = used + d;
        }
        self.delete_blobs(|x| x.purpose == 3); // §13.3
        done(AdminOk { deleted: Some(d), floor: Some(hi), more: Some(hi < below), ..ok0() })
    }

    // ---------- §12.9 `restore_tail`

    fn restore_tail(&mut self, now: i64, first: u64, hashes: &[Hash32], chain_before: &Hash32, frames: &[Vec<u8>]) -> Result<Done, Fail> {
        let b = self.brd().clone();
        let head = self.head_at(&b);
        if safe(first) != Some(head.seq + 1) {
            let mut r = refuse("relay/not_next");
            r.head = Some(Head { seq: head.seq, chain: head.chain });
            return Err(Box::new((r, None)));
        }
        if *chain_before != head.chain {
            return fail("relay/chain_mismatch");
        }
        let check = |i: usize, f: &[u8]| (util::h(&[f]) != hashes[i]).then_some("relay/frame_mismatch");
        let (h, stored) = self.tail(now, frames, check, |_| Ok(())).map_err(|e| Box::new((e, None)))?;
        let out = self.push(now, &stored);
        Ok(Done { ok: AdminOk { head: Some(Head { seq: h.seq, chain: h.chain }), ..ok0() }, token_gen: None, out })
    }

    // ---------- §12.10 `purge`

    fn purge(&mut self, now: i64, before: u64, epoch: u32) -> Result<Done, Fail> {
        if epoch < 1 {
            return fail("relay/bad_args");
        }
        let b = self.brd().clone();
        let head = self.head_at(&b);
        let Some(before) = safe(before).filter(|x| *x >= b.purge_floor && *x <= head.seq) else { return fail("relay/target") };
        let mut floor = b.purge_floor;
        let mut snap = b.compact_snap;
        let mut read_epoch = b.read_epoch;
        if before > floor {
            floor = before;
            let gone = self.delete_blobs(|x| x.purpose <= 3 && x.seq < before);
            if snap.is_some_and(|s| gone.contains(&s)) {
                snap = None;
            }
            read_epoch = read_epoch.max(epoch);
        }
        let day = now.div_euclid(DAY);
        let used = if b.del_day == day { b.del_count } else { 0 };
        let done_to = floor.min(b.purge_done + DELETE_BUDGET.saturating_sub(used));
        let mut d = 0;
        for s in b.purge_done..done_to {
            if self.delete_log(s) {
                d += 1;
            }
        }
        if let Some(x) = self.board.as_mut() {
            x.purge_floor = floor;
            x.purge_done = done_to.max(b.purge_done);
            x.compact_snap = snap;
            x.del_day = day;
            x.del_count = used + d;
            x.read_epoch = read_epoch;
        }
        let pd = done_to.max(b.purge_done);
        done(AdminOk { purge_floor: Some(floor), done: Some(pd), ..ok0() })
    }

    // ---------- §12.12 `recovery_register`

    fn recovery_register(&mut self, now: i64, dev: &Dev, pk: &Hash32, hash: &Hash32, seq: u64) -> Result<Done, Fail> {
        if *dev != util::fake_dev_r(&self.board_id, pk) {
            return fail("relay/bad_args");
        }
        let b = self.brd();
        let named = safe(seq).and_then(|s| self.log.get(&s)).filter(|r| r.seq >= b.purge_floor && r.hash == *hash).map(|r| (r.seq, r.chain));
        let same = self.devices.get(dev);
        if let (Some(s), Some((rs, rc))) = (same, named)
            && s.role == 2
            && s.state == 1
            && s.sign_pk == *pk
        {
            return done(at(rs, rc));
        }
        if same.is_some() || self.devices.values().any(|d| d.sign_pk == *pk) {
            return fail("relay/duplicate_key");
        }
        if self.devices.values().filter(|d| d.role == 2).count() >= 64 {
            return fail("relay/device_cap");
        }
        let Some((rs, rc)) = named else { return fail("relay/frame_mismatch") };
        for d in self.devices.values_mut() {
            if d.role == 2 && d.state == 1 {
                d.state = 2;
            }
        }
        self.devices.insert(*dev, new_device(*dev, *pk, 2, 1, 0, 0, 0, now));
        done(at(rs, rc))
    }

    // ---------- §10.3 login, without nonce or signature

    /// A login of `dev` (§10.3, step 1's checks and step 2's effects; nonces
    /// and the login signature are not modelled): with a token of `dev` of any
    /// generation, or the door. The stored generation becomes `gen + 1`, and
    /// the device's sockets with a lower generation are closed with 4003.
    pub fn login(&mut self, _now: i64, cred: &Cred, dev: &Dev) -> Reply<Login> {
        if *cred == Cred::Create {
            return Reply::err(refuse("relay/forbidden"));
        }
        let own = match self.cred_row(cred, true) {
            Ok(o) => o,
            Err(e) => return Reply::err(e),
        };
        if let Err(e) = self.need(cred, &[Some(BoardState::Open), Some(BoardState::Fenced)]) {
            return Reply::err(e);
        }
        if own.is_some_and(|o| o.dev != *dev) {
            return Reply::err(refuse("relay/forbidden"));
        }
        let d = match self.devices.get(dev) {
            Some(d) if d.role == 1 => d.clone(),
            _ => return Reply::err(refuse("relay/unauthorized")),
        };
        if d.state != 1 {
            return Reply::err(refuse("relay/revoked"));
        }
        if d.generation == MAX_GEN {
            return Reply::err(refuse("relay/gen_exhausted"));
        }
        let generation = d.generation + 1;
        if let Some(r) = self.devices.get_mut(dev) {
            r.generation = generation;
        }
        let out = self.close_dev(dev, |s| s.generation < generation, 4003);
        Reply { result: Ok(Login { generation, wraps: d.wraps }), last: None, token_gen: Some(generation), out }
    }
}
