//! Sequencing (`relay.md` §9), `create` (§10.2) and the tails of §12.7 and
//! §12.9.

use std::collections::BTreeMap;

use super::util::{self, DAY, Fail, MAX_SAFE, refuse};
use super::{BoardRow, BoardState, Cred, DeviceRow, HeadAt, LogRow, ModelRelay, Reply};
use crate::{AdminOk, AppendResult, Dev, Hash32, Head, RelayRefusal};

/// A frame that passed P1 to P7 (or the tail form of them).
pub(crate) struct Pre {
    pub frame: Vec<u8>,
    pub dev: Dev,
    pub ctr: u64,
    pub hash: Hash32,
    pub sign_pk: Hash32,
}

/// An author row's running counters inside one transaction (§9.4: each author
/// row is written once per batch, after its last frame).
#[derive(Clone)]
pub(crate) struct Author {
    pub role: u8,
    pub state: u8,
    pub sign_pk: Hash32,
    pub last_ctr: u64,
    pub last_seq: u64,
    pub app_day: i64,
    pub app_count: u32,
}

/// The outcome of T1 to T4 for one frame, before any write.
pub(crate) enum Decision {
    Store,
    Dup(u64, Hash32),
    Conflict(u64, Vec<u8>),
    Gap(u64, u64),
    Refused(&'static str),
}

impl ModelRelay {
    /// P1 to P7 of §9.3 for one frame. `cred` `None` is the tail form of
    /// §12.7 and §12.9: any registered author, of any role and state.
    pub(crate) fn precheck(&self, frame: &[u8], cred: Option<&Cred>) -> Result<Pre, &'static str> {
        let f = util::frame_head(frame, &self.board_id)?;
        let a = self.devices.get(&f.dev).ok_or("relay/unknown_author")?;
        if let Some(c) = cred {
            let own = matches!(c, Cred::Token { dev, .. } if *dev == f.dev);
            if a.role != 2 && !own {
                return Err("relay/not_author");
            }
            if a.state != 1 {
                return Err("relay/revoked");
            }
        }
        if !util::fake_verify(&a.sign_pk, &util::frame_signed(frame, f.len), &frame[48 + f.len..]) {
            return Err("relay/bad_signature");
        }
        Ok(Pre { frame: frame.to_vec(), dev: f.dev, ctr: f.ctr, hash: util::h(&[frame]), sign_pk: a.sign_pk })
    }

    pub(crate) fn author<'a>(&self, authors: &'a mut BTreeMap<Dev, Author>, dev: &Dev) -> Option<&'a mut Author> {
        if !authors.contains_key(dev) {
            let r = self.devices.get(dev)?;
            authors.insert(
                *dev,
                Author {
                    role: r.role,
                    state: r.state,
                    sign_pk: r.sign_pk,
                    last_ctr: r.last_ctr,
                    last_seq: r.last_seq,
                    app_day: r.app_day,
                    app_count: r.app_count,
                },
            );
        }
        authors.get_mut(dev)
    }

    /// T1 to T4 of §9.4 for one frame, writing nothing.
    pub(crate) fn decide(&self, board: &BoardRow, f: &Pre, a: Option<&Author>, day: i64) -> Decision {
        let Some(a) = a.filter(|a| a.state == 1 && a.sign_pk == f.sign_pk) else {
            return Decision::Refused("relay/revoked");
        };
        if f.ctr <= a.last_ctr {
            return match self.dev_ctr.get(&(f.dev, f.ctr)).and_then(|s| self.log.get(s)) {
                Some(row) if row.seq >= board.purge_floor => {
                    if row.hash == f.hash {
                        Decision::Dup(row.seq, row.chain)
                    } else {
                        Decision::Conflict(row.seq, row.frame.clone())
                    }
                }
                _ => Decision::Conflict(0, Vec::new()),
            };
        }
        if f.ctr > a.last_ctr + 1 {
            return Decision::Gap(a.last_ctr, a.last_seq);
        }
        let count = if a.app_day == day { a.app_count } else { 0 };
        if a.role == 1 && count >= util::APPEND_BUDGET {
            return Decision::Refused("relay/budget");
        }
        Decision::Store
    }

    /// T5 of §9.4: the log row at head + 1 (RELAY-STORE-003, RELAY-STORE-004).
    pub(crate) fn store(&mut self, f: Pre, a: &mut Author, head: HeadAt, now: i64, budget: bool) -> HeadAt {
        let seq = head.seq + 1;
        let at = now.max(head.at);
        let chain = util::chain_step(&head.chain, seq, &f.hash);
        let day = now.div_euclid(DAY);
        self.dev_ctr.insert((f.dev, f.ctr), seq);
        self.log.insert(seq, LogRow { seq, dev: f.dev, ctr: f.ctr, hash: f.hash, chain, at, frame: f.frame });
        a.last_ctr = f.ctr;
        a.last_seq = seq;
        if budget && a.role == 1 {
            a.app_count = if a.app_day == day { a.app_count } else { 0 } + 1;
            a.app_day = day;
        }
        HeadAt { seq, chain, at }
    }

    /// Writes each author row once, with its final counters.
    pub(crate) fn flush(&mut self, authors: BTreeMap<Dev, Author>) {
        for (dev, a) in authors {
            if let Some(r) = self.devices.get_mut(&dev) {
                r.last_ctr = a.last_ctr;
                r.last_seq = a.last_seq;
                r.app_day = a.app_day;
                r.app_count = a.app_count;
            }
        }
    }

    // ---------- §10.6 `append` (and the socket's append, §11.4)

    /// `append` (§9, §10.6): a role-1 token, or the door for recovery frames.
    /// 1 to 16 frames, else `400 relay/bad_batch`. The results are in frame
    /// order; `out` pushes the stored entries to every ready socket.
    pub fn append(&mut self, now: i64, cred: &Cred, frames: &[Vec<u8>]) -> Reply<Vec<AppendResult>> {
        if *cred == Cred::Create {
            return Reply::err(refuse("relay/forbidden"));
        }
        if let Err(e) = self.cred_row(cred, false).and_then(|_| self.need(cred, &[Some(BoardState::Open)])) {
            return Reply::err(e);
        }
        if frames.is_empty() || frames.len() > 16 {
            return Reply::err(refuse("relay/bad_batch"));
        }
        let (results, stored) = self.sequence(now, cred, frames);
        let out = self.push(now, &stored);
        Reply { result: Ok(results), last: None, token_gen: None, out }
    }

    /// §9.3 and §9.4 for a batch: the results and the seqs stored.
    pub(crate) fn sequence(&mut self, now: i64, cred: &Cred, frames: &[Vec<u8>]) -> (Vec<AppendResult>, Vec<u64>) {
        let mut ok = Vec::new();
        let mut stop = None;
        for f in frames {
            match self.precheck(f, Some(cred)) {
                Ok(p) => ok.push(p),
                Err(code) => {
                    stop = Some(code);
                    break;
                }
            }
        }
        let board = self.brd().clone();
        let day = now.div_euclid(DAY);
        let mut head = self.head_at(&board);
        let mut authors = BTreeMap::new();
        let mut results = Vec::new();
        let mut stored = Vec::new();
        let mut stopped = false;
        for f in ok {
            let a = self.author(&mut authors, &f.dev).cloned();
            match self.decide(&board, &f, a.as_ref(), day) {
                Decision::Store => {
                    let mut a = a.expect("decide stores only with an author");
                    let dev = f.dev;
                    head = self.store(f, &mut a, head, now, true);
                    authors.insert(dev, a);
                    stored.push(head.seq);
                    results.push(AppendResult::New { seq: head.seq, chain: head.chain });
                }
                Decision::Dup(seq, chain) => results.push(AppendResult::Dup { seq, chain }),
                d => {
                    results.push(match d {
                        Decision::Conflict(seq, frame) => AppendResult::Conflict { seq, frame },
                        Decision::Gap(last_ctr, last_seq) => AppendResult::Gap { last_ctr, last_seq },
                        Decision::Refused(code) => refused(code),
                        _ => unreachable!(),
                    });
                    stopped = true;
                    break;
                }
            }
        }
        if !stopped && let Some(code) = stop {
            results.push(refused(code));
        }
        while results.len() < frames.len() {
            results.push(AppendResult::Skipped);
        }
        self.flush(authors);
        (results, stored)
    }

    /// The frame a `register` or `fence` carries (§12.3, §12.6): `Ok(None)`
    /// to store, `Ok(Some(head))` for a `dup`, else the refusal and, for a gap
    /// or conflict, the author row's `last_ctr` and `last_seq`.
    pub(crate) fn carried(&self, now: i64, cred: &Cred, frame: &[u8]) -> Result<(Pre, Option<Head>), Fail> {
        let f = self.precheck(frame, Some(cred)).map_err(|c| Box::new((refuse(c), None)))?;
        let board = self.brd();
        let mut authors = BTreeMap::new();
        let a = self.author(&mut authors, &f.dev).cloned();
        let day = now.div_euclid(DAY);
        match self.decide(board, &f, a.as_ref(), day) {
            Decision::Store => Ok((f, None)),
            Decision::Dup(seq, chain) => Ok((f, Some(Head { seq, chain }))),
            d @ (Decision::Conflict(..) | Decision::Gap(..)) => {
                let a = a.expect("an author row");
                let code = if matches!(d, Decision::Gap(..)) { "relay/gap" } else { "relay/conflict" };
                Err(Box::new((refuse(code), Some((a.last_ctr, a.last_seq)))))
            }
            Decision::Refused(code) => {
                let mut r = refuse(code);
                if code == "relay/budget" {
                    r.until = Some((day + 1) * DAY);
                }
                Err(Box::new((r, None)))
            }
        }
    }

    /// Stores a carried frame that `carried` admitted, with the budget.
    pub(crate) fn store_carried(&mut self, now: i64, f: Pre) -> HeadAt {
        let board = self.brd().clone();
        let head = self.head_at(&board);
        let mut authors = BTreeMap::new();
        let dev = f.dev;
        let mut a = self.author(&mut authors, &dev).cloned().expect("an author row");
        let h = self.store(f, &mut a, head, now, true);
        authors.insert(dev, a);
        self.flush(authors);
        h
    }

    /// The tails of §12.7 and §12.9: every frame has a registered author, a
    /// valid signature and its author's next counter, or nothing is written
    /// (the refusal carries the frame's `index`). `check` runs first for each
    /// frame (the hash check of `restore_tail`); `at_end` sees the last head
    /// before anything is written. Without the budget.
    pub(crate) fn tail(
        &mut self,
        now: i64,
        frames: &[Vec<u8>],
        check: impl Fn(usize, &[u8]) -> Option<&'static str>,
        at_end: impl FnOnce(&HeadAt) -> Result<(), RelayRefusal>,
    ) -> Result<(HeadAt, Vec<u64>), RelayRefusal> {
        let board = self.brd().clone();
        let mut cur = self.head_at(&board);
        let mut authors: BTreeMap<Dev, Author> = BTreeMap::new();
        let mut plan = Vec::new();
        let at_index = |code: &str, i: usize| {
            let mut r = refuse(code);
            r.index = Some(i as u32);
            r
        };
        for (i, frame) in frames.iter().enumerate() {
            if let Some(code) = check(i, frame) {
                return Err(at_index(code, i));
            }
            let f = self.precheck(frame, None).map_err(|c| at_index(c, i))?;
            let a = self.author(&mut authors, &f.dev).ok_or_else(|| at_index("relay/unknown_author", i))?;
            if a.sign_pk != f.sign_pk {
                return Err(at_index("relay/bad_signature", i));
            }
            if f.ctr != a.last_ctr + 1 {
                return Err(at_index(if f.ctr > a.last_ctr + 1 { "relay/gap" } else { "relay/conflict" }, i));
            }
            let seq = cur.seq + 1;
            cur = HeadAt { seq, chain: util::chain_step(&cur.chain, seq, &f.hash), at: now.max(cur.at) };
            a.last_ctr = f.ctr;
            a.last_seq = seq;
            plan.push((cur, f));
        }
        at_end(&cur)?;
        let mut stored = Vec::new();
        for (h, f) in plan {
            self.dev_ctr.insert((f.dev, f.ctr), h.seq);
            self.log.insert(h.seq, LogRow { seq: h.seq, dev: f.dev, ctr: f.ctr, hash: f.hash, chain: h.chain, at: h.at, frame: f.frame });
            stored.push(h.seq);
        }
        self.flush(authors);
        Ok((cur, stored))
    }

    /// Deletes the `log` row at `seq` and its index entry; whether it existed.
    pub(crate) fn delete_log(&mut self, seq: u64) -> bool {
        match self.log.remove(&seq) {
            Some(r) => {
                if self.dev_ctr.get(&(r.dev, r.ctr)) == Some(&seq) {
                    self.dev_ctr.remove(&(r.dev, r.ctr));
                }
                true
            }
            None => false,
        }
    }

    // ---------- §10.2 `create`

    /// `create` (§10.2) with the create secret: the Genesis frame as seq 1,
    /// the recovery identity `dev_R` and the first device. The fake
    /// derivations of engine.md §6.3 stand for `ctx("board")` and
    /// `ctx("recovery-dev")`, and the signature is the fake one. A success
    /// carries `seq`, `chain`, `token` and `door`, and the token's generation
    /// in `token_gen`.
    pub fn create(&mut self, now: i64, rsign_pk: &Hash32, salt: &[u8; 16], dev: &Dev, sign_pk: &Hash32, frame: &[u8]) -> Reply<AdminOk> {
        let dev_r = util::fake_dev_r(&self.board_id, rsign_pk);
        let fh = util::h(&[frame]);
        let good = util::fake_board_id(rsign_pk, salt) == self.board_id
            && match util::frame_head(frame, &self.board_id) {
                Ok(f) => {
                    f.dev == dev_r
                        && f.ctr == 1
                        && util::fake_verify(rsign_pk, &util::frame_signed(frame, f.len), &frame[48 + f.len..])
                }
                Err(_) => false,
            }
            && *dev != dev_r
            && sign_pk != rsign_pk;
        let door = self.door();
        let answer = |chain: Hash32| AdminOk {
            seq: Some(1),
            chain: Some(chain),
            token: true,
            deleted: None,
            floor: None,
            more: None,
            purge_floor: None,
            done: None,
            head: None,
            door: Some(door),
        };
        let Some(board) = &self.board else {
            if !good {
                return Reply::err(refuse("relay/bad_genesis"));
            }
            let c0 = util::chain_zero(&self.board_id);
            let c1 = util::chain_step(&c0, 1, &fh);
            self.board = Some(BoardRow {
                state: BoardState::Open,
                created_at: now,
                base_seq: 0,
                base_chain: c0,
                compact_floor: 1,
                purge_floor: 1,
                purge_done: 1,
                compact_snap: None,
                del_day: 0,
                del_count: 0,
                read_epoch: 1,
                home_gen: 1,
                fence_host: None,
                fence_door: None,
                seed_signer: None,
                seed_last_seq: None,
                seed_chain: None,
            });
            self.devices.insert(dev_r, new_device(dev_r, *rsign_pk, 2, 1, 0, 1, 1, now));
            self.devices.insert(*dev, new_device(*dev, *sign_pk, 1, 1, 1, 0, 0, now));
            self.dev_ctr.insert((dev_r, 1), 1);
            self.log.insert(1, LogRow { seq: 1, dev: dev_r, ctr: 1, hash: fh, chain: c1, at: now, frame: frame.to_vec() });
            let mut r = Reply::ok(answer(c1));
            r.token_gen = Some(1);
            return r;
        };
        if board.state == BoardState::Seeding {
            return Reply::err(refuse("relay/seeding"));
        }
        // Repeat (relay-core.js: only on an open board whose seq 1 is still served).
        let row1 = if board.state == BoardState::Open && board.purge_floor <= 1 { self.log.get(&1) } else { None };
        let d = self.devices.get(dev);
        match (row1, d) {
            (Some(r1), Some(d)) if r1.hash == fh && d.role == 1 && d.state == 1 && d.sign_pk == *sign_pk => {
                if d.generation == util::MAX_GEN {
                    return Reply::err(refuse("relay/gen_exhausted"));
                }
                let chain = r1.chain;
                let generation = d.generation + 1;
                if let Some(d) = self.devices.get_mut(dev) {
                    d.generation = generation;
                }
                let mut r = Reply::ok(answer(chain));
                r.token_gen = Some(generation);
                r
            }
            _ => Reply::err(refuse("relay/exists")),
        }
    }
}

/// A new `devices` row with zero day counters and no wraps.
#[allow(clippy::too_many_arguments)]
pub(crate) fn new_device(dev: Dev, sign_pk: Hash32, role: u8, state: u8, generation: u32, last_ctr: u64, last_seq: u64, now: i64) -> DeviceRow {
    DeviceRow { dev, sign_pk, role, state, generation, last_ctr, last_seq, app_day: 0, app_count: 0, added_at: now, wraps: None }
}

/// A refused result of §9.6 with its code's status.
fn refused(code: &str) -> AppendResult {
    AppendResult::Refused { status: util::status_of(code), code: code.to_string() }
}

/// A value of a request that must be at most 2^53 − 1 (RELAY-GEN-008).
pub(crate) fn safe(v: u64) -> Option<u64> {
    (v <= MAX_SAFE).then_some(v)
}
