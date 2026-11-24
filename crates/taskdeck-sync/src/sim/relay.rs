//! The model relay (engine.md §31.3): `relay.md` implemented over in-memory
//! tables, with no I/O, no clock and no randomness of its own.
//!
//! One `ModelRelay` is the object of one board on one relay (`relay.md` §3.1):
//! its tables (§7.1: board, devices, log, blobs, parts), its sockets (§11) and
//! nothing else. It models §8.1, §9, §10.2, §10.4, §10.5, §10.7, §11.3, §11.4,
//! §12.3 to §12.10, §12.12, §13 and §14.1 to §14.2. It does not model the gate,
//! the door's MAC, token MACs, nonces or admin signatures: a request's
//! credential is a [`Cred`] the caller states, an admin request passes §12.1
//! A4 and A6 by construction, and A5 is checked against the rows. Frames carry
//! the fake signature of engine.md §6.3, verified here from its formula.
//!
//! Every call takes `now`, the relay clock of that request (§8.3). Every map
//! is a `BTreeMap`, so no answer depends on hashing. The whole relay is
//! `Clone`: a rollback restores an earlier clone (`rollback_to`), a fork
//! serves some devices from a clone. `docs/sync/status/engine/api-model-relay.md`
//! is the API for the simulator's author.
//!
//! Where `relay.md` leaves the model in doubt it follows `relay/relay-core.js`;
//! each such place is marked "relay-core.js" below.

mod admin;
mod blobs;
mod seq;
mod socket;
mod util;

use std::collections::BTreeMap;

use crate::{Dev, Hash32, Head, Page, PageEntry, RelayRefusal, Welcome};

pub use admin::{AdminOp, Login};
pub use blobs::meta;
pub use socket::{Out, SockMsg, SocketId};
pub use util::{fake_board_id, fake_dev_r, fake_sign, fake_sign_pk, fake_signed_frame, status_of};

use util::{DAY, MAX_SAFE, refuse};

/// The credential a request presents, as the gate has already checked it
/// (`relay.md` §5, §8.2). The model checks only its row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cred {
    /// A role-1 token of `dev` with generation `gen` (§5.1).
    Token { dev: Dev, generation: u32 },
    /// The board's door (§5.3).
    Door,
    /// The relay's create secret (§5.5).
    Create,
}

/// A board's state (`relay.md` §8.1); an empty board has no [`BoardRow`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BoardState {
    Open = 1,
    Fenced = 2,
    Seeding = 3,
}

/// The `board` row (`relay.md` §7.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoardRow {
    pub state: BoardState,
    pub created_at: i64,
    pub base_seq: u64,
    pub base_chain: Hash32,
    pub compact_floor: u64,
    pub purge_floor: u64,
    pub purge_done: u64,
    pub compact_snap: Option<[u8; 16]>,
    pub del_day: i64,
    pub del_count: u64,
    pub read_epoch: u32,
    pub home_gen: u32,
    pub fence_host: Option<String>,
    pub fence_door: Option<[u8; 5]>,
    pub seed_signer: Option<Dev>,
    pub seed_last_seq: Option<u64>,
    pub seed_chain: Option<Hash32>,
}

/// A `devices` row (`relay.md` §7.1). `role` 1 device, 2 recovery; `state`
/// 1 current, 2 revoked or retired.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceRow {
    pub dev: Dev,
    pub sign_pk: Hash32,
    pub role: u8,
    pub state: u8,
    pub generation: u32,
    pub last_ctr: u64,
    pub last_seq: u64,
    pub app_day: i64,
    pub app_count: u32,
    pub added_at: i64,
    /// `first_epoch` and the wraps of the latest `reinstate` (§12.5).
    pub wraps: Option<(u32, Vec<[u8; 96]>)>,
}

/// A `log` row (`relay.md` §7.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogRow {
    pub seq: u64,
    pub dev: Dev,
    pub ctr: u64,
    pub hash: Hash32,
    pub chain: Hash32,
    pub at: i64,
    pub frame: Vec<u8>,
}

/// A `blobs` row (`relay.md` §7.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlobRow {
    pub id: [u8; 16],
    pub slot: u64,
    pub purpose: u8,
    pub seq: u64,
    pub author: Dev,
    pub size: u64,
    pub parts: u32,
    pub sha256: Hash32,
    pub received: u32,
    pub complete: bool,
    pub at: i64,
}

/// The answer to one request: its result, what a refusal's error body adds
/// that [`RelayRefusal`] does not hold, and the socket messages the request
/// makes the object send (§11.4), in the order it sends them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply<T> {
    pub result: Result<T, RelayRefusal>,
    /// `last_ctr` and `last_seq` of a `409 relay/gap` or `relay/conflict`
    /// refusing the frame of a `register` or `fence` (§4.2, RELAY-ADMIN-005).
    pub last: Option<(u64, u64)>,
    /// The generation of the token a success issues (create, login, register, seed).
    pub token_gen: Option<u32>,
    pub out: Vec<Out>,
}

impl<T> Reply<T> {
    pub(crate) fn ok(v: T) -> Self {
        Reply { result: Ok(v), last: None, token_gen: None, out: Vec::new() }
    }
    pub(crate) fn err(r: RelayRefusal) -> Self {
        Reply { result: Err(r), last: None, token_gen: None, out: Vec::new() }
    }
}

/// The relay's head (`relay.md` §7.3) with the `at` of its row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HeadAt {
    pub seq: u64,
    pub chain: Hash32,
    pub at: i64,
}

/// The object of one board on one relay (`relay.md` §3.1), over in-memory tables.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelRelay {
    board_id: [u8; 16],
    relay_key: Hash32,
    board: Option<BoardRow>,
    devices: BTreeMap<Dev, DeviceRow>,
    log: BTreeMap<u64, LogRow>,
    /// The unique index `log_dev_ctr`; frames stored by `inject_frame` are not in it.
    dev_ctr: BTreeMap<(Dev, u64), u64>,
    blobs: BTreeMap<[u8; 16], BlobRow>,
    parts: BTreeMap<u64, Vec<u8>>,
    sockets: BTreeMap<SocketId, socket::Socket>,
    next_socket: SocketId,
}

impl ModelRelay {
    /// The empty object of `board_id` on a relay whose `TOKEN_KEY` stands in as
    /// `relay_key` (it only derives the door, [`ModelRelay::door`]).
    pub fn new(board_id: [u8; 16], relay_key: Hash32) -> Self {
        ModelRelay {
            board_id,
            relay_key,
            board: None,
            devices: BTreeMap::new(),
            log: BTreeMap::new(),
            dev_ctr: BTreeMap::new(),
            blobs: BTreeMap::new(),
            parts: BTreeMap::new(),
            sockets: BTreeMap::new(),
            next_socket: 1,
        }
    }

    /// The board id this object serves.
    pub fn board_id(&self) -> [u8; 16] {
        self.board_id
    }

    /// The board's door (`relay.md` §5.3), with the fake formula
    /// `H("FAKE-DOOR" ‖ relay_key ‖ board_id)[0..5]` in place of the HMAC.
    pub fn door(&self) -> [u8; 5] {
        let h = util::h(&[b"FAKE-DOOR", &self.relay_key, &self.board_id]);
        let mut d = [0u8; 5];
        d.copy_from_slice(&h[..5]);
        d
    }

    // ---------- inspection (engine.md §31.7)

    /// The board row, `None` while the board is empty.
    pub fn board_row(&self) -> Option<&BoardRow> {
        self.board.as_ref()
    }
    /// The board state, `None` while the board is empty.
    pub fn state(&self) -> Option<BoardState> {
        self.board.as_ref().map(|b| b.state)
    }
    /// Every `devices` row, by `dev`.
    pub fn devices(&self) -> &BTreeMap<Dev, DeviceRow> {
        &self.devices
    }
    /// Every stored `log` row, by seq.
    pub fn log(&self) -> &BTreeMap<u64, LogRow> {
        &self.log
    }
    /// Every `blobs` row, by id.
    pub fn blobs(&self) -> &BTreeMap<[u8; 16], BlobRow> {
        &self.blobs
    }
    /// The bytes of part `index` of blob `id`, complete or not.
    pub fn part(&self, id: &[u8; 16], index: u32) -> Option<&Vec<u8>> {
        let b = self.blobs.get(id)?;
        self.parts.get(&(b.slot * 64 + u64::from(index)))
    }
    /// The number of stored `parts` rows.
    pub fn part_count(&self) -> usize {
        self.parts.len()
    }
    /// The relay head (§7.3); `None` while the board is empty.
    pub fn head(&self) -> Option<Head> {
        self.board.as_ref().map(|b| {
            let h = self.head_at(b);
            Head { seq: h.seq, chain: h.chain }
        })
    }

    // ---------- faults (engine.md §31.6)

    /// Stores `frame` as it is at the next seq, without any check, counter or
    /// budget, and pushes it like any stored entry. The frame stays out of the
    /// `(dev, ctr)` index, so the real author's counters are untouched. `None`
    /// while the board is empty.
    pub fn inject_frame(&mut self, now: i64, frame: Vec<u8>) -> Option<(Head, Vec<Out>)> {
        let board = self.board.as_ref()?;
        let head = self.head_at(board);
        let mut dev = [0u8; 16];
        let mut ctr = 0;
        if frame.len() >= 44 {
            dev.copy_from_slice(&frame[20..36]);
            ctr = u64::from_be_bytes(frame[36..44].try_into().unwrap_or([0; 8]));
        }
        let hash = util::h(&[&frame]);
        let seq = head.seq + 1;
        let chain = util::chain_step(&head.chain, seq, &hash);
        let at = now.max(head.at);
        self.log.insert(seq, LogRow { seq, dev, ctr, hash, chain, at, frame });
        let out = self.push(now, &[seq]);
        Some((Head { seq, chain }, out))
    }

    /// Restores the tables of `earlier` (a clone taken before) and keeps this
    /// relay's sockets: a relay rollback (§31.6).
    pub fn rollback_to(&mut self, earlier: &ModelRelay) {
        let sockets = std::mem::take(&mut self.sockets);
        let next = self.next_socket;
        *self = earlier.clone();
        self.sockets = sockets;
        self.next_socket = next.max(self.next_socket);
    }

    /// Whether the tables (not the sockets) equal `other`'s: nothing was written.
    pub fn same_tables(&self, other: &ModelRelay) -> bool {
        self.board == other.board
            && self.devices == other.devices
            && self.log == other.log
            && self.blobs == other.blobs
            && self.parts == other.parts
    }

    // ---------- shared checks

    pub(crate) fn head_at(&self, board: &BoardRow) -> HeadAt {
        match self.log.iter().next_back() {
            Some((_, r)) => HeadAt { seq: r.seq, chain: r.chain, at: r.at },
            None => HeadAt { seq: board.base_seq, chain: board.base_chain, at: board.created_at },
        }
    }

    /// §8.2: the token's row, its conditions in the order the table lists
    /// them. `any_gen` for `login` (§10.3). Door and create secret: `None`.
    pub(crate) fn cred_row(&self, cred: &Cred, any_gen: bool) -> Result<Option<DeviceRow>, RelayRefusal> {
        match cred {
            Cred::Token { dev, generation } => {
                let d = match self.devices.get(dev) {
                    Some(d) if d.role == 1 => d,
                    _ => return Err(refuse("relay/unauthorized")),
                };
                if d.state != 1 {
                    return Err(refuse("relay/revoked"));
                }
                if !any_gen && d.generation != *generation {
                    return Err(refuse("relay/token_stale"));
                }
                Ok(Some(d.clone()))
            }
            _ => Ok(None),
        }
    }

    /// §8.1: the board must be in one of `allowed` (`None` is empty).
    /// relay-core.js `need`: an `open` board where only `seeding` is allowed
    /// answers `409 relay/target` (the `seed-tail` of §12.7).
    pub(crate) fn need(&self, cred: &Cred, allowed: &[Option<BoardState>]) -> Result<(), RelayRefusal> {
        let st = self.state();
        if allowed.contains(&st) {
            return Ok(());
        }
        Err(refuse(match st {
            None => "relay/no_board",
            Some(BoardState::Seeding) => "relay/seeding",
            _ if *cred == Cred::Create => "relay/exists",
            Some(BoardState::Fenced) => "relay/fenced",
            Some(BoardState::Open) => "relay/target",
        }))
    }

    /// The board row, for a caller that has passed `need` with a non-empty state.
    pub(crate) fn brd(&self) -> &BoardRow {
        self.board.as_ref().expect("need() admitted only a non-empty board")
    }

    /// The token's row and the board state for a route that takes a role-1
    /// token only (the gate refuses any other credential, `relay.md` §6.3).
    pub(crate) fn token_route(&self, cred: &Cred, allowed: &[Option<BoardState>]) -> Result<DeviceRow, RelayRefusal> {
        let row = self.cred_row(cred, false)?.ok_or_else(|| refuse("relay/forbidden"))?;
        self.need(cred, allowed)?;
        Ok(row)
    }

    // ---------- §10.4 `hello`

    /// `hello` (§10.4) for a role-1 token: the fields of the welcome (§11.3).
    /// The door is [`ModelRelay::door`]; `push` is always true.
    pub fn hello(&self, now: i64, cred: &Cred) -> Reply<Welcome> {
        match self.token_route(cred, &[Some(BoardState::Open), Some(BoardState::Fenced)]) {
            Ok(row) => Reply::ok(self.welcome(&row, now)),
            Err(e) => Reply::err(e),
        }
    }

    pub(crate) fn welcome(&self, d: &DeviceRow, now: i64) -> Welcome {
        let b = self.brd();
        let head = self.head_at(b);
        let fenced = b.state == BoardState::Fenced;
        Welcome {
            head: Head { seq: head.seq, chain: head.chain },
            last_ctr: Some(d.last_ctr),
            last_seq: Some(d.last_seq),
            relay_now: now,
            floor: b.compact_floor.max(b.purge_floor),
            purge_floor: b.purge_floor,
            fenced,
            fence: if fenced { fence_of(b) } else { None },
            appends_left: Some(appends_left(d, now)),
            push: true,
        }
    }

    // ---------- §10.5 `read`

    /// `read` (§10.5) with `after` and `limit` (1 to 256, else `400 relay/bad_query`).
    pub fn read(&self, now: i64, cred: &Cred, after: u64, limit: u16) -> Reply<Page> {
        if let Err(e) = self.token_route(cred, &[Some(BoardState::Open), Some(BoardState::Fenced)]) {
            return Reply::err(e);
        }
        if !(1..=256).contains(&limit) || after > MAX_SAFE {
            return Reply::err(refuse("relay/bad_query"));
        }
        Reply::ok(self.page(now, after, limit))
    }

    /// The selection and page of §10.5 (RELAY-READ-001, RELAY-READ-002).
    pub(crate) fn page(&self, now: i64, after: u64, limit: u16) -> Page {
        let b = self.brd();
        let head = self.head_at(b);
        let from = after.saturating_add(1).max(b.purge_floor);
        let mut entries = Vec::new();
        let mut bytes = 0usize;
        for r in self.log.range(from..).map(|(_, r)| r) {
            if entries.len() >= usize::from(limit) || (!entries.is_empty() && bytes + r.frame.len() > util::MIB) {
                break;
            }
            bytes += r.frame.len();
            entries.push(PageEntry { seq: r.seq, at: r.at, frame: r.frame.clone() });
        }
        let floor = b.compact_floor.max(b.purge_floor);
        Page {
            head: Head { seq: head.seq, chain: head.chain },
            relay_now: now,
            floor,
            purge_floor: b.purge_floor,
            more: entries.last().is_some_and(|e| e.seq < head.seq),
            truncated: after.saturating_add(1) < floor,
            fenced: b.state == BoardState::Fenced,
            entries,
        }
    }
}

/// The fence part of a fenced board (§11.3): host, door, home generation.
pub(crate) fn fence_of(b: &BoardRow) -> Option<(String, [u8; 5], u32)> {
    Some((b.fence_host.clone()?, b.fence_door?, b.home_gen))
}

/// `appends_left` of §10.4: `3,000 − app_count` today, 3,000 on another day.
pub(crate) fn appends_left(d: &DeviceRow, now: i64) -> u32 {
    let used = if d.app_day == now.div_euclid(DAY) { d.app_count } else { 0 };
    util::APPEND_BUDGET.saturating_sub(used)
}
