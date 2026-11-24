//! The test that loads `engine-poison.json` (§15, §16.1): markers, attempts,
//! `last_failed`, caught panics, OS kills and the crash halt, with a state
//! machine whose apply outcomes are scripted (ok, panic, kill).

use std::cell::RefCell;
use std::collections::{BTreeSet, VecDeque};
use std::rc::Rc;

use serde_json::{Value, json};

use super::kit_tests::Store;
use super::script_tests::{OTHER, Plan, Script, ScriptM, ScriptedSealer, board};
use super::vectors_tests::cases;
use super::{Config, Engine};
use crate::toy::{ToyCommand, ToyMachine, ToyNotice, ToyState};
use crate::{
    Applied, Checkpoint, Coalesce, Ctx, Effect, Event, Head, Input, LogRecord, Marker, Meta, Page, PageEntry,
    PersistOp, RelayTarget, Sealer, SmError, SmRefusal, StateMachine, Welcome,
};

#[derive(Default)]
struct Shared {
    outcomes: VecDeque<String>,
    /// Seqs applied cleanly by this process: applying one again is the engine rebuilding its state.
    done: BTreeSet<u64>,
    events: Vec<Value>,
    killed: bool,
}

/// The toy, with the outcome of every confirmed apply taken from a script.
struct ScriptedMachine {
    toy: ToyMachine,
    shared: Rc<RefCell<Shared>>,
}

impl StateMachine for ScriptedMachine {
    type State = ToyState;
    type Command = ToyCommand;
    type Notice = ToyNotice;

    fn supported_rules(&self) -> &[u32] {
        self.toy.supported_rules()
    }
    fn hash_versions(&self) -> &[u32] {
        self.toy.hash_versions()
    }
    fn load(&self, bytes: &[u8]) -> Result<ToyState, SmError> {
        self.toy.load(bytes)
    }
    fn canonical_bytes(&self, state: &ToyState) -> Vec<u8> {
        self.toy.canonical_bytes(state)
    }
    fn board_hash(&self, state: &ToyState) -> [u8; 32] {
        self.toy.board_hash(state)
    }
    fn rules(&self, state: &ToyState) -> u32 {
        self.toy.rules(state)
    }
    fn upgrade(&self, state: &mut ToyState, v: u32) -> Result<(), SmError> {
        self.toy.upgrade(state, v)
    }
    fn parse(&self, payload: &[u8]) -> Result<ToyCommand, SmError> {
        self.toy.parse(payload)
    }
    fn encode(&self, command: &ToyCommand) -> Vec<u8> {
        self.toy.encode(command)
    }
    fn op_name(&self, command: &ToyCommand) -> &'static str {
        self.toy.op_name(command)
    }
    fn target(&self, command: &ToyCommand) -> Option<u64> {
        self.toy.target(command)
    }
    fn apply(&self, state: &mut ToyState, command: &ToyCommand, ctx: &Ctx) -> Result<Applied<ToyNotice>, SmRefusal> {
        let outcome = {
            let mut s = self.shared.borrow_mut();
            if s.done.contains(&ctx.seq) {
                None
            } else {
                let o = s.outcomes.pop_front().expect("an outcome for every apply");
                s.events.push(json!({"apply": ctx.seq, "outcome": o}));
                match o.as_str() {
                    "ok" => {
                        s.done.insert(ctx.seq);
                    }
                    "kill" => s.killed = true,
                    _ => {}
                }
                Some(o)
            }
        };
        match outcome.as_deref() {
            Some("panic") | Some("kill") => panic!("scripted: the apply of {} does not return", ctx.seq),
            _ => self.toy.apply(state, command, ctx),
        }
    }
    fn trial_apply(&self, state: &ToyState, command: &ToyCommand, ctx: &Ctx) -> Result<Applied<ToyNotice>, SmRefusal> {
        self.toy.trial_apply(state, command, ctx)
    }
    fn reset(&self, target: &ToyState, current: &ToyState) -> Result<ToyState, SmError> {
        self.toy.reset(target, current)
    }
    fn hold(&self, command: &ToyCommand) -> Option<u64> {
        self.toy.hold(command)
    }
    fn coalesce(&self, held: &ToyCommand, next: &ToyCommand) -> Coalesce<ToyCommand> {
        self.toy.coalesce(held, next)
    }
    fn rebind(
        &self,
        commands: &[ToyCommand],
        from: u16,
        to: u16,
        view: &ToyState,
        origin: &[u8],
    ) -> Result<(Vec<ToyCommand>, Vec<u8>), SmError> {
        self.toy.rebind(commands, from, to, view, origin)
    }
    fn notice_concerns(&self, notice: &ToyNotice, device_no: u16) -> bool {
        self.toy.notice_concerns(notice, device_no)
    }
    fn notice_bytes(&self, notice: &ToyNotice) -> Vec<u8> {
        self.toy.notice_bytes(notice)
    }
    fn refusal_bytes(&self, seq: u64, author: u16, refusal: &SmRefusal) -> Vec<u8> {
        self.toy.refusal_bytes(seq, author, refusal)
    }
}

struct PNode {
    e: Option<Engine<ScriptedMachine, ScriptedSealer>>,
    sealer: ScriptedSealer,
    shared: Rc<RefCell<Shared>>,
    store: Store,
    chains: Vec<(u64, [u8; 32])>,
    session: u8,
    connected: bool,
    wall: i64,
    mono: u64,
    /// The `Meta` as last written, to tell which fields a `WriteMeta` changed.
    seen_meta: (u64, Option<(u64, u64)>, bool),
}

impl PNode {
    fn feed(&mut self, event: Event<ToyCommand>) {
        let mut queue: VecDeque<Event<ToyCommand>> = VecDeque::from([event]);
        while let Some(ev) = queue.pop_front() {
            let Some(e) = self.e.as_mut() else { return };
            let out = e.handle(Input { wall_ms: self.wall, mono_ms: self.mono, event: ev });
            if self.shared.borrow().killed {
                // An OS kill: nothing this call emitted happened, and the process is gone.
                self.e = None;
                return;
            }
            for fx in out {
                if let Effect::Persist { id, ops } = fx {
                    for op in &ops {
                        self.record(op);
                        self.store.apply(op);
                    }
                    queue.push_back(Event::Persisted { id });
                }
            }
        }
    }

    /// The engine events of §15 that a persist operation stands for.
    fn record(&mut self, op: &PersistOp) {
        let mut ev = Vec::new();
        match op {
            PersistOp::WriteMeta(m) => {
                if m.last_failed != self.seen_meta.1
                    && let Some((x, a)) = m.last_failed {
                        ev.push(json!({"last_failed": [x, a]}));
                    }
                if m.attempt != self.seen_meta.0 {
                    ev.push(json!({"attempt": m.attempt}));
                }
                if let (Some(h), false) = (&m.halt, self.seen_meta.2) {
                    ev.push(json!({"halt": h.seq, "code": h.code}));
                }
                self.seen_meta = (m.attempt, m.last_failed, m.halt.is_some());
            }
            PersistOp::SetMarker(Marker { first, last, attempt }) => {
                ev.push(json!({"set_marker": [first, last, attempt]}))
            }
            PersistOp::ClearMarker => ev.push(json!({"clear_marker": true})),
            _ => {}
        }
        self.shared.borrow_mut().events.extend(ev);
    }

    fn start(&mut self) {
        {
            let mut s = self.shared.borrow_mut();
            s.killed = false;
            s.done.clear();
        }
        self.session += 1;
        self.connected = false;
        let machine = ScriptedMachine { toy: ToyMachine::new(false), shared: self.shared.clone() };
        self.e = Some(Engine::new(machine, self.sealer.clone(), Config {}));
        let loaded = self.store.loaded("b", [self.session; 16], None);
        self.feed(Event::Start(loaded));
    }

    fn head(&self) -> Head {
        let l = self.store.log.keys().next_back().copied().unwrap_or(0).max(self.chains[0].0);
        let chain = self.chains.iter().find(|c| c.0 == l).map(|c| c.1).unwrap();
        Head { seq: l, chain }
    }

    fn page(&mut self, seqs: &[u64]) {
        if !self.connected {
            self.connected = true;
            let head = self.head();
            self.feed(Event::Connected { push: true });
            self.feed(Event::Welcome(Welcome {
                head,
                last_ctr: Some(0),
                last_seq: Some(0),
                relay_now: self.wall,
                floor: 1,
                purge_floor: 0,
                fenced: false,
                fence: None,
                appends_left: None,
                push: true,
            }));
        }
        let mut chain = self.head().chain;
        let mut entries = Vec::new();
        for &seq in seqs {
            let frame = self.sealer.frame(OTHER, seq);
            self.sealer.plan(&frame, Plan::accept_cmd(OTHER, seq, vec![1, 6, 0, 0, 0, 0, 0, 0, 0, seq as u8]));
            chain = self.sealer.chain_step(&chain, seq, &frame);
            self.chains.push((seq, chain));
            entries.push(PageEntry { seq, at: 0, frame });
        }
        let head = Head { seq: *seqs.last().unwrap(), chain };
        let page = Page {
            head,
            relay_now: self.wall,
            floor: 1,
            purge_floor: 0,
            more: false,
            truncated: false,
            fenced: false,
            entries,
        };
        self.feed(Event::Page { req: None, page });
    }
}

// spec: ENG-POISON-001 ENG-POISON-002 ENG-POISON-003 ENG-POISON-004 ENG-POISON-005 ENG-HALT-002 ENG-PERSIST-006
#[test]
fn engine_poison_vectors() {
    let all = cases(include_str!("../../tests/vectors/engine-poison.json"));
    assert!(all.len() >= 12);
    for c in &all {
        let name = c["name"].as_str().unwrap();
        let st = &c["start"];
        let b = st["checkpoint_seq"].as_u64().unwrap();
        let log_head = st["log_head"].as_u64().unwrap();
        let sealer = ScriptedSealer(Rc::new(RefCell::new(Script::default())));
        let shared = Rc::new(RefCell::new(Shared::default()));
        shared.borrow_mut().outcomes =
            c["outcomes"].as_array().unwrap().iter().map(|o| o.as_str().unwrap().to_owned()).collect();
        let m = ScriptM { my_ctr: 0, other_ctr: b, ..Default::default() };
        let chain_b = [9u8; 32];
        let mut store = Store {
            ckpt: Some(Checkpoint {
                board_seq: b,
                board_chain: chain_b,
                board: board(),
                membership: sealer.membership_bytes(&m),
                log_floor: b + 1,
                registry: Default::default(),
            }),
            log_floor: b + 1,
            floor_chain: Some(chain_b),
            ..Default::default()
        };
        let mut chains = vec![(b, chain_b)];
        let mut chain = chain_b;
        for seq in b + 1..=log_head {
            let frame = sealer.frame(OTHER, seq);
            sealer.plan(&frame, Plan::accept_cmd(OTHER, seq, vec![1, 6, 0, 0, 0, 0, 0, 0, 0, seq as u8]));
            chain = sealer.chain_step(&chain, seq, &frame);
            chains.push((seq, chain));
            store.log.insert(seq, LogRecord { seq, chain, tau: 0, frame });
        }
        let pair = |v: &Value| v.as_array().map(|a| (a[0].as_u64().unwrap(), a[1].as_u64().unwrap()));
        let attempt = st["attempt"].as_u64().unwrap();
        let last_failed = pair(&st["last_failed"]);
        store.meta = Some(Meta {
            attempt,
            last_failed,
            build_id: "b".into(),
            relay: RelayTarget { host: "relay.test".into(), door: [1; 5], home_gen: 1 },
            last_beat_ms: 1_700_000_100_000,
            ..Default::default()
        });
        store.marker = st["marker"].as_array().map(|a| Marker {
            first: a[0].as_u64().unwrap(),
            last: a[1].as_u64().unwrap(),
            attempt: a[2].as_u64().unwrap(),
        });
        let mut n = PNode {
            e: None,
            sealer,
            shared: shared.clone(),
            store,
            chains,
            session: 0,
            connected: false,
            wall: 1_700_000_100_000,
            mono: 10_000,
            seen_meta: (attempt, last_failed, false),
        };
        for step in c["script"].as_array().unwrap() {
            match step {
                Value::String(s) if s == "start" => n.start(),
                other => {
                    let seqs: Vec<u64> = other["page"].as_array().unwrap().iter().map(|x| x.as_u64().unwrap()).collect();
                    n.page(&seqs);
                }
            }
        }
        // The engine's events against the expected ones; the echoes of the script, the mode of
        // an apply and `clears_last_failed` are not compared (the engine does not name them).
        let strip = |v: &Value| -> Option<Value> {
            if v.get("step").is_some() {
                return None;
            }
            if let Some(s) = v.get("apply") {
                return Some(json!({"apply": s, "outcome": v["outcome"]}));
            }
            Some(v.clone())
        };
        // One `WriteMeta` can carry a new attempt and a new `last_failed`: their order within
        // it is not an engine event, so neighbours of these two kinds are put in one order.
        let norm = |mut v: Vec<Value>| -> Vec<Value> {
            let is_meta = |x: &Value| x.get("attempt").is_some() || x.get("last_failed").is_some();
            for i in 1..v.len() {
                if is_meta(&v[i - 1]) && is_meta(&v[i]) && v[i - 1].get("last_failed").is_some() {
                    v.swap(i - 1, i);
                }
            }
            v
        };
        let want: Vec<Value> = norm(c["events"].as_array().unwrap().iter().filter_map(strip).collect());
        let got: Vec<Value> = norm(shared.borrow().events.iter().filter_map(strip).collect());
        assert_eq!(got, want, "{name}");
        let halted = n.e.as_ref().and_then(|e| e.status().halt.map(|h| h.seq));
        assert_eq!(halted, c["halt"].as_u64(), "{name}: halt");
    }
}
