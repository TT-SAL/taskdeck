//! The lead's driver over the model relay (task C, `crate::sim::relay`): the
//! engine's relay effects become the model's calls, so that operations with
//! admin and blob requests run against what `relay.md` says. Poll sessions only.

// A kit: not every helper is used by every test.
#![allow(dead_code)]

use std::collections::BTreeMap;

use super::kit_tests::{Node, creator, joined, relay};
use crate::fake::FakeSealer;
use crate::sim::relay::{Cred, ModelRelay};
use crate::toy::ToyMachine;
use crate::{
    AdminRequest, BlobRequest, Dev, Effect, Event, NewDevice, Notification, RelayRefusal, RelayTarget, Sealer,
    SnapInfo, SnapshotMaterial, StateMachine,
};

pub(crate) struct Net {
    /// One model relay per host.
    pub relays: BTreeMap<String, ModelRelay>,
    /// The token generation each device holds on each host.
    pub gens: BTreeMap<(String, Dev), u32>,
    /// Where each device last connected.
    pub at: BTreeMap<Dev, String>,
    /// Every relay call in order: the device and a short name (`append`, `admin:revoke`, `blob:upload:2`).
    pub calls: Vec<(Dev, String)>,
    /// Every admin request sent, in order.
    pub admins: Vec<(Dev, AdminRequest)>,
    /// For each of them: the relay's head and compaction floor and the sender's clock when it
    /// was sent, and the answer's code.
    pub admin_ctx: Vec<(u64, u64, i64, Result<(), String>)>,
}

pub(crate) fn admin_name(op: &AdminRequest) -> &'static str {
    match op {
        AdminRequest::Register { .. } => "admin:register",
        AdminRequest::RegisterSeq { .. } => "admin:register_seq",
        AdminRequest::RecoveryRegisterSeq { .. } => "admin:recovery_register_seq",
        AdminRequest::Revoke { .. } => "admin:revoke",
        AdminRequest::Fence { .. } => "admin:fence",
        AdminRequest::Door { .. } => "admin:door",
        AdminRequest::Seed { .. } => "admin:seed",
        AdminRequest::SeedTail { .. } => "admin:seed_tail",
        AdminRequest::Compact { .. } => "admin:compact",
        AdminRequest::RestoreTail { .. } => "admin:restore_tail",
        AdminRequest::Purge { .. } => "admin:purge",
    }
}

pub(crate) fn blob_name(op: &BlobRequest) -> String {
    match op {
        BlobRequest::Upload { purpose, .. } => format!("blob:upload:{purpose}"),
        BlobRequest::Fetch { .. } => "blob:fetch".into(),
        BlobRequest::List => "blob:list".into(),
        BlobRequest::Meta { .. } => "blob:meta".into(),
    }
}

impl Net {
    pub(crate) fn cred(&self, host: &str, dev: &Dev) -> Cred {
        match self.gens.get(&(host.to_owned(), *dev)) {
            Some(&generation) => Cred::Token { dev: *dev, generation },
            None => Cred::Door,
        }
    }

    pub(crate) fn relay(&mut self) -> &mut ModelRelay {
        self.relays.get_mut(&relay().host).expect("the first relay")
    }

    /// Answers one effect of `n`. False when it was not a relay effect.
    pub(crate) fn answer(&mut self, n: &mut Node, fx: Effect) -> bool {
        let dev = n.dev();
        let now = n.wall;
        let host = self.at.get(&dev).cloned().unwrap_or_default();
        match fx {
            Effect::Connect { relay } => {
                self.at.insert(dev, relay.host.clone());
                self.calls.push((dev, "connect".into()));
                let cred = self.cred(&relay.host, &dev);
                let Some(r) = self.relays.get_mut(&relay.host) else {
                    n.feed(Event::Disconnected { unreachable: true, paused_until_ms: None });
                    return true;
                };
                match r.login(now, &cred, &dev).result {
                    Ok(login) => {
                        self.gens.insert((relay.host.clone(), dev), login.generation);
                        n.feed(Event::Connected { push: false });
                        if let Some((first_epoch, wraps)) = login.wraps {
                            n.feed(Event::MissedRoots { first_epoch, wraps });
                        }
                    }
                    Err(refusal) => n.feed(Event::RelayRefused { req: None, refusal }),
                }
            }
            Effect::Hello => {
                self.calls.push((dev, "hello".into()));
                let cred = self.cred(&host, &dev);
                match self.relays[&host].hello(now, &cred).result {
                    // These sessions poll: no socket is open, so nothing is pushed.
                    Ok(mut w) => {
                        w.push = false;
                        n.feed(Event::Welcome(w))
                    }
                    Err(refusal) => n.feed(Event::RelayRefused { req: None, refusal }),
                }
            }
            Effect::Read { req, after, limit } => {
                let cred = self.cred(&host, &dev);
                match self.relays[&host].read(now, &cred, after, limit).result {
                    Ok(page) => n.feed(Event::Page { req: Some(req), page }),
                    Err(refusal) => n.feed(Event::RelayRefused { req: Some(req), refusal }),
                }
            }
            Effect::Append { req, frames } => {
                self.calls.push((dev, "append".into()));
                let cred = self.cred(&host, &dev);
                match self.relays.get_mut(&host).expect("a relay").append(now, &cred, &frames).result {
                    Ok(results) => n.feed(Event::AppendResults { req, results }),
                    Err(refusal) => n.feed(Event::RelayRefused { req: Some(req), refusal }),
                }
            }
            Effect::Admin { req, op } => {
                self.calls.push((dev, admin_name(&op).into()));
                self.admins.push((dev, op.clone()));
                // A request that names a host goes to that relay (the new relay of a move).
                let to = match &op {
                    AdminRequest::Door { host } | AdminRequest::Seed { host, .. } | AdminRequest::SeedTail { host, .. } => {
                        host.clone()
                    }
                    _ => host.clone(),
                };
                let cred = match &op {
                    // The runtime holds the new relay's create secret for the door and the seed.
                    AdminRequest::Door { .. } | AdminRequest::Seed { .. } => Cred::Create,
                    _ => self.cred(&to, &dev),
                };
                let Some(r) = self.relays.get_mut(&to) else {
                    let refusal = RelayRefusal { status: 503, code: "relay/unavailable".into(), until: None, head: None, index: None };
                    self.admin_ctx.push((0, 0, now, Err(refusal.code.clone())));
                    n.feed(Event::AdminResult { req, result: Err(refusal) });
                    return true;
                };
                let head_then = r.head().map_or(0, |h| h.seq);
                let floor_then = r.board_row().map_or(0, |b| b.compact_floor.max(b.purge_floor));
                let reply = r.admin(now, &cred, &dev, &op);
                self.admin_ctx.push((head_then, floor_then, now, reply.result.as_ref().map(|_| ()).map_err(|e| e.code.clone())));
                if let (Ok(_), Some(g)) = (&reply.result, reply.token_gen) {
                    match &op {
                        AdminRequest::Register { dev: new, .. } | AdminRequest::RegisterSeq { dev: new, .. } => {
                            self.gens.insert((to.clone(), *new), g);
                        }
                        AdminRequest::Seed { .. } => {
                            self.gens.insert((to.clone(), dev), g);
                        }
                        _ => {}
                    }
                }
                n.feed(Event::AdminResult { req, result: reply.result });
            }
            Effect::Blob { req, op } => {
                self.calls.push((dev, blob_name(&op)));
                let cred = self.cred(&host, &dev);
                let result = self.relays.get_mut(&host).expect("a relay").blob(now, &cred, &op).result;
                n.feed(Event::BlobResult { req, result });
            }
            _ => return false,
        }
        true
    }

    pub(crate) fn serve(&mut self, n: &mut Node) {
        for _ in 0..20_000 {
            let Some(fx) = n.fx.pop_front() else { return };
            self.answer(n, fx);
        }
        let tail: Vec<_> = self.calls.iter().rev().take(8).map(|c| c.1.clone()).collect();
        panic!("the node did not settle: last calls {tail:?}, waiting {:?}", n.fx.iter().take(4).map(super::kit_tests::fx_name).collect::<Vec<_>>());
    }

    /// The head of the relay a device is at.
    pub(crate) fn head_seq(&self, n: &Node) -> u64 {
        self.at.get(&n.dev()).and_then(|h| self.relays.get(h)).and_then(|r| r.head()).map_or(0, |h| h.seq)
    }

    /// A second relay for the same board, empty (the target of a move).
    pub(crate) fn add_relay(&mut self, host: &str) {
        let board = self.relay().board_id();
        self.relays.insert(host.to_owned(), ModelRelay::new(board, [8; 32]));
    }

    pub(crate) fn log_len(&self) -> u64 {
        self.relays[&relay().host].head().map_or(0, |h| h.seq)
    }

    /// Serves every node, with a poll thirty seconds later, until nothing moves.
    pub(crate) fn settle(&mut self, nodes: &mut [&mut Node]) {
        for _ in 0..80 {
            let before: Vec<u64> = self.relays.values().map(|r| r.head().map_or(0, |h| h.seq)).collect();
            let calls = self.calls.iter().filter(|c| c.1 != "hello" && c.1 != "connect").count();
            for n in nodes.iter_mut() {
                self.serve(n);
            }
            for n in nodes.iter_mut() {
                n.tick(30_000);
                self.serve(n);
            }
            let after: Vec<u64> = self.relays.values().map(|r| r.head().map_or(0, |h| h.seq)).collect();
            let quiet = calls == self.calls.iter().filter(|c| c.1 != "hello" && c.1 != "connect").count();
            if before == after && quiet && nodes.iter().all(|n| n.fx.is_empty()) {
                return;
            }
        }
        let d: Vec<String> = nodes
            .iter()
            .map(|n| {
                let st = n.e.status();
                format!(
                    "head {} board {} session {:?} queued {} stops {:?} halt {:?} ops {:?} no {:?}",
                    st.head.seq,
                    st.board_seq,
                    st.session,
                    st.queued,
                    st.stops,
                    st.halt.map(|h| h.code),
                    st.ops,
                    n.e.device_no()
                )
            })
            .collect();
        let tail: Vec<_> = self.calls.iter().rev().take(12).map(|c| c.1.clone()).collect();
        panic!("the nodes did not settle: {d:#?} last calls {tail:?}");
    }
}

/// A board on a model relay with its creator, settled.
pub(crate) fn board(seed: u64) -> (Net, Node) {
    let (mut a, f1) = creator(seed);
    let id = a.e.sealer().export_identity().to_vec();
    let take = |r: std::ops::Range<usize>| id[r].to_vec();
    let dev: Dev = take(8..24).try_into().expect("dev");
    let sign_pk: [u8; 32] = take(56..88).try_into().expect("sign_pk");
    let board_id: [u8; 16] = take(120..136).try_into().expect("board id");
    let rsign_pk: [u8; 32] = take(137..169).try_into().expect("rsign_pk");
    let salt: [u8; 16] = take(169..185).try_into().expect("salt");
    assert_eq!(dev, a.dev());
    let mut model = ModelRelay::new(board_id, [7; 32]);
    let reply = model.create(a.wall, &rsign_pk, &salt, &dev, &sign_pk, &f1);
    assert!(reply.result.is_ok(), "{:?}", reply.result);
    let host = relay().host;
    let mut net = Net { relays: BTreeMap::new(), gens: BTreeMap::new(), at: BTreeMap::new(), calls: Vec::new(), admins: Vec::new(), admin_ctx: Vec::new() };
    net.gens.insert((host.clone(), dev), reply.token_gen.expect("a token"));
    net.relays.insert(host, model);
    net.settle(&mut [&mut a]);
    (net, a)
}

/// What a pairing answered: the new device's keys, the snapshot and where it was added.
pub(crate) struct Paired {
    pub material: SnapshotMaterial,
    pub snapshot: SnapInfo,
    pub added_seq: u64,
}

pub(crate) fn new_device(seed: u64, name: &str) -> NewDevice {
    let (dev, sign_pk, hpke_pk) = FakeSealer::join_keys(seed);
    NewDevice { dev, sign_pk, hpke_pk, name: name.to_owned(), transcript: [7; 32] }
}

/// The material of `a`'s confirmed state now (what its pairing snapshot holds when taken at `B`).
pub(crate) fn material_of(a: &Node) -> SnapshotMaterial {
    let (state, head) = a.e.confirmed();
    let sm = ToyMachine::new(false);
    let membership = a.e.membership_bytes();
    SnapshotMaterial {
        seq: head.seq,
        chain: head.chain,
        hash_version: 1,
        state_hash: crate::state_hash(1, &sm.board_hash(state), &membership),
        board: sm.canonical_bytes(state),
        membership,
    }
}

pub(crate) fn pair_result(a: &Node, t: u64) -> Option<Result<(SnapInfo, u64), &'static str>> {
    a.notes.iter().find_map(|x| match x {
        Notification::PairRegistered { ticket, snapshot, added_seq, .. } if *ticket == t => Some(Ok((snapshot.clone(), *added_seq))),
        Notification::PairFailed { ticket, code } if *ticket == t => Some(Err(*code)),
        _ => None,
    })
}

/// Pairs a new device through `a`'s engine (§28.4) and starts it.
pub(crate) fn pair(net: &mut Net, a: &mut Node, seed: u64, name: &str, ticket: u64) -> Node {
    net.settle(&mut [a]);
    let material = material_of(a);
    a.feed(Event::PairRegister { ticket, device: new_device(seed, name) });
    net.settle(&mut [a]);
    let (snapshot, added_seq) = pair_result(a, ticket).expect("an answer").expect("paired");
    assert_eq!(snapshot.seq, material.seq);
    let bundle = a.e.sealer().bundle(material.clone(), added_seq);
    let sealer = FakeSealer::join(seed, name, bundle);
    let at = RelayTarget { host: relay().host, door: relay().door, home_gen: relay().home_gen };
    let mut n = joined(sealer, seed, a.wall, material, added_seq, at);
    net.settle(&mut [a, &mut n]);
    n
}
