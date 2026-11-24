//! The socket (`relay.md` §11.1 to §11.4): attachments, hello and welcome,
//! `second` and `displaced`, append and read messages, pushes and the state
//! notice. Messages are values, not bytes: the model parses nothing.

use super::{BoardState, Cred, ModelRelay};
use crate::{AppendResult, Dev, Page, RelayRefusal, Welcome};

/// A socket's id in one model relay, from [`ModelRelay::socket_open`].
pub type SocketId = u64;

/// A socket's attachment (§11.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Socket {
    pub dev: Dev,
    pub generation: u32,
    pub session: Option<[u8; 16]>,
    pub features: u32,
    pub ready: bool,
}

/// A message the relay sends on a socket (§11.3), or its close (§11.6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SockMsg {
    Welcome(Welcome),
    Result { req: u32, results: Vec<AppendResult> },
    Page { req: u32, page: Page },
    Push(Page),
    /// The session this socket displaced.
    Second([u8; 16]),
    /// The session that displaced this socket.
    Displaced([u8; 16]),
    Error { req: u32, refusal: RelayRefusal },
    /// The state notice after a `fence` (state 2 with the fence part).
    State { host: String, door: [u8; 5], home_gen: u32 },
    /// The relay closes the socket with this code; the socket is gone.
    Close(u16),
}

/// One message for one socket: which socket, its device and its session
/// (`None` before hello), and the message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Out {
    pub socket: SocketId,
    pub dev: Dev,
    pub session: Option<[u8; 16]>,
    pub msg: SockMsg,
}

impl ModelRelay {
    /// The upgrade (§11.1): a role-1 token valid at the object on an open or
    /// fenced board. The socket starts without a session and not ready.
    pub fn socket_open(&mut self, cred: &Cred) -> Result<SocketId, RelayRefusal> {
        let row = self.token_route(cred, &[Some(BoardState::Open), Some(BoardState::Fenced)])?;
        let id = self.next_socket;
        self.next_socket += 1;
        self.sockets.insert(id, Socket { dev: row.dev, generation: row.gen_of(cred), session: None, features: 0, ready: false });
        Ok(id)
    }

    /// The client closed the socket (or the network did).
    pub fn socket_close(&mut self, id: SocketId) {
        self.sockets.remove(&id);
    }

    /// The open sockets: id, device, session, ready.
    pub fn sockets(&self) -> Vec<(SocketId, Dev, Option<[u8; 16]>, bool)> {
        self.sockets.iter().map(|(id, s)| (*id, s.dev, s.session, s.ready)).collect()
    }

    fn out(&self, id: SocketId, msg: SockMsg) -> Out {
        let s = &self.sockets[&id];
        Out { socket: id, dev: s.dev, session: s.session, msg }
    }

    /// Closes socket `id` with `code`: the close message, then the socket is gone.
    pub(crate) fn close(&mut self, id: SocketId, code: u16) -> Out {
        let o = self.out(id, SockMsg::Close(code));
        self.sockets.remove(&id);
        o
    }

    /// Closes every socket of `dev` whose attachment `when` selects.
    pub(crate) fn close_dev(&mut self, dev: &Dev, when: impl Fn(&Socket) -> bool, code: u16) -> Vec<Out> {
        let ids: Vec<SocketId> = self.sockets.iter().filter(|(_, s)| s.dev == *dev && when(s)).map(|(id, _)| *id).collect();
        ids.into_iter().map(|id| self.close(id, code)).collect()
    }

    fn ready(&self) -> Vec<SocketId> {
        self.sockets.iter().filter(|(_, s)| s.ready).map(|(id, _)| *id).collect()
    }

    /// Hello (§11.3) with the client's `session` and `features`: `second`,
    /// `displaced` and closes for the device's other sockets, then the welcome.
    pub fn socket_hello(&mut self, now: i64, id: SocketId, session: [u8; 16], features: u32) -> Vec<Out> {
        let Some(s) = self.sockets.get(&id) else { return Vec::new() };
        if s.session.is_some() || self.board.is_none() {
            return vec![self.close(id, 4004)]; // RELAY-SOCK-002
        }
        let dev = s.dev;
        let mut out = Vec::new();
        let others: Vec<(SocketId, [u8; 16])> = self
            .sockets
            .iter()
            .filter(|(o, x)| **o != id && x.dev == dev)
            .filter_map(|(o, x)| x.session.map(|ss| (*o, ss)))
            .collect();
        for (o, other_session) in others {
            if other_session == session {
                out.push(self.close(o, 4002));
            } else {
                out.push(self.out(o, SockMsg::Displaced(session)));
                out.push(self.close(o, 4001));
                out.push(self.out(id, SockMsg::Second(other_session)));
            }
        }
        let Some(row) = self.devices.get(&dev).cloned() else {
            out.push(self.close(id, 4003));
            return out;
        };
        if let Some(s) = self.sockets.get_mut(&id) {
            s.session = Some(session);
            s.features = features;
            s.ready = true;
        }
        let w = self.welcome(&row, now);
        out.push(self.out(id, SockMsg::Welcome(w)));
        out
    }

    /// The socket's credential again (RELAY-SOCK-007): an `error` and a close
    /// with 4003 when the device is no longer current or its generation moved.
    fn sock_cred(&mut self, id: SocketId, req: u32) -> Result<Cred, Vec<Out>> {
        let s = &self.sockets[&id];
        let cred = Cred::Token { dev: s.dev, generation: s.generation };
        match self.cred_row(&cred, false) {
            Ok(_) => Ok(cred),
            Err(e) => {
                let err = self.out(id, SockMsg::Error { req, refusal: e });
                Err(vec![err, self.close(id, 4003)])
            }
        }
    }

    /// A message before hello, or on a closed socket: close 4004 (RELAY-SOCK-004).
    fn not_ready(&mut self, id: SocketId) -> Option<Vec<Out>> {
        match self.sockets.get(&id) {
            None => Some(Vec::new()),
            Some(s) if !s.ready => Some(vec![self.close(id, 4004)]),
            Some(_) => None,
        }
    }

    /// An append message (§11.4): processed as the append route; `result` on
    /// this socket, then the pushes to every ready socket (RELAY-SOCK-012).
    pub fn socket_append(&mut self, now: i64, id: SocketId, req: u32, frames: &[Vec<u8>]) -> Vec<Out> {
        if let Some(o) = self.not_ready(id) {
            return o;
        }
        let cred = match self.sock_cred(id, req) {
            Ok(c) => c,
            Err(o) => return o,
        };
        let r = self.append(now, &cred, frames);
        let mut out = Vec::new();
        match r.result {
            Ok(results) => out.push(self.out(id, SockMsg::Result { req, results })),
            Err(refusal) => out.push(self.out(id, SockMsg::Error { req, refusal })),
        }
        out.extend(r.out);
        out
    }

    /// A read message (§11.4), answered by `page`.
    pub fn socket_read(&mut self, now: i64, id: SocketId, req: u32, after: u64, limit: u16) -> Vec<Out> {
        if let Some(o) = self.not_ready(id) {
            return o;
        }
        let cred = match self.sock_cred(id, req) {
            Ok(c) => c,
            Err(o) => return o,
        };
        let msg = match self.read(now, &cred, after, limit).result {
            Ok(page) => SockMsg::Page { req, page },
            Err(refusal) => SockMsg::Error { req, refusal },
        };
        vec![self.out(id, msg)]
    }

    /// After a transaction stored `seqs`: exactly those entries, in seq order,
    /// to every ready socket, split as pages are (RELAY-SOCK-008).
    pub(crate) fn push(&self, now: i64, seqs: &[u64]) -> Vec<Out> {
        let socks = self.ready();
        if seqs.is_empty() || socks.is_empty() {
            return Vec::new();
        }
        let b = self.brd();
        let head = self.head_at(b);
        let rows: Vec<&super::LogRow> = seqs.iter().filter_map(|s| self.log.get(s)).collect();
        let mut pages = Vec::new();
        let mut i = 0;
        while i < rows.len() {
            let mut j = i;
            let mut bytes = 0;
            while j < rows.len() && j - i < 256 && (j == i || bytes + rows[j].frame.len() <= super::util::MIB) {
                bytes += rows[j].frame.len();
                j += 1;
            }
            pages.push(Page {
                head: crate::Head { seq: head.seq, chain: head.chain },
                relay_now: now,
                floor: b.compact_floor.max(b.purge_floor),
                purge_floor: b.purge_floor,
                more: j < rows.len(),
                truncated: false,
                fenced: b.state == BoardState::Fenced,
                entries: rows[i..j].iter().map(|r| crate::PageEntry { seq: r.seq, at: r.at, frame: r.frame.clone() }).collect(),
            });
            i = j;
        }
        let mut out = Vec::new();
        for s in socks {
            for p in &pages {
                out.push(self.out(s, SockMsg::Push(p.clone())));
            }
        }
        out
    }

    /// The state notice after a `fence` (§11.4), to every ready socket.
    pub(crate) fn state_notice(&self) -> Vec<Out> {
        let Some((host, door, home_gen)) = super::fence_of(self.brd()) else { return Vec::new() };
        self.ready().into_iter().map(|s| self.out(s, SockMsg::State { host: host.clone(), door, home_gen })).collect()
    }
}

impl super::DeviceRow {
    fn gen_of(&self, cred: &Cred) -> u32 {
        match cred {
            Cred::Token { generation, .. } => *generation,
            _ => self.generation,
        }
    }
}
