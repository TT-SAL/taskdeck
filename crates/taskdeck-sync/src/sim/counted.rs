//! The toy state machine the simulated devices run (`ToyMachine`, §5.3), with
//! one addition the engine cannot see: each state value carries the seqs of the
//! entries its lineage applied since it was last loaded from bytes. It is what
//! the second half of §31.7 invariant 2 reads ("every device whose `B` is past
//! that seq has applied it exactly once").
//!
//! Every method delegates to `ToyMachine`; the trail takes no part in the
//! canonical bytes, the board hash or any decision, so the engine runs exactly
//! as it would on the toy. A clone shares the trail (a persistent list), so a
//! state cloned to an older point and replayed forward counts each entry of the
//! replay once, and a state loaded from bytes starts an empty trail.

use std::ops::Deref;
use std::rc::Rc;

use crate::toy::{ToyCommand, ToyMachine, ToyNotice, ToyState};
use crate::{Applied, Coalesce, Ctx, SmError, SmRefusal, StateMachine};

/// One application of the lineage, newest first.
struct Node {
    seq: u64,
    prev: Option<Rc<Node>>,
}

/// A toy state and the applications that made it (since its last load).
#[derive(Clone)]
pub(crate) struct Counted {
    pub(crate) toy: ToyState,
    trail: Option<Rc<Node>>,
}

impl Deref for Counted {
    type Target = ToyState;
    fn deref(&self) -> &ToyState {
        &self.toy
    }
}

impl Counted {
    /// The seqs `apply` ran for on this state's lineage, oldest first, with
    /// repeats (a seq applied twice appears twice).
    pub(crate) fn applied(&self) -> Vec<u64> {
        let mut out = Vec::new();
        let mut n = self.trail.as_deref();
        while let Some(x) = n {
            out.push(x.seq);
            n = x.prev.as_deref();
        }
        out.reverse();
        out
    }

    fn fresh(toy: ToyState) -> Counted {
        Counted { toy, trail: None }
    }
}

/// `ToyMachine` over `Counted` states.
pub(crate) struct CountedToy(pub(crate) ToyMachine);

impl StateMachine for CountedToy {
    type State = Counted;
    type Command = ToyCommand;
    type Notice = ToyNotice;

    fn supported_rules(&self) -> &[u32] {
        self.0.supported_rules()
    }
    fn hash_versions(&self) -> &[u32] {
        self.0.hash_versions()
    }
    fn load(&self, bytes: &[u8]) -> Result<Counted, SmError> {
        self.0.load(bytes).map(Counted::fresh)
    }
    fn canonical_bytes(&self, state: &Counted) -> Vec<u8> {
        self.0.canonical_bytes(&state.toy)
    }
    fn board_hash(&self, state: &Counted) -> [u8; 32] {
        self.0.board_hash(&state.toy)
    }
    fn rules(&self, state: &Counted) -> u32 {
        self.0.rules(&state.toy)
    }
    fn upgrade(&self, state: &mut Counted, v: u32) -> Result<(), SmError> {
        self.0.upgrade(&mut state.toy, v)
    }
    fn parse(&self, payload: &[u8]) -> Result<ToyCommand, SmError> {
        self.0.parse(payload)
    }
    fn encode(&self, command: &ToyCommand) -> Vec<u8> {
        self.0.encode(command)
    }
    fn op_name(&self, command: &ToyCommand) -> &'static str {
        self.0.op_name(command)
    }
    fn target(&self, command: &ToyCommand) -> Option<u64> {
        self.0.target(command)
    }
    fn apply(&self, state: &mut Counted, command: &ToyCommand, ctx: &Ctx) -> Result<Applied<ToyNotice>, SmRefusal> {
        // Counted once apply returns, applied or refused (a refusal leaves the
        // toy state unchanged, §5.1); a panic is not an application.
        let r = self.0.apply(&mut state.toy, command, ctx);
        state.trail = Some(Rc::new(Node { seq: ctx.seq, prev: state.trail.take() }));
        r
    }
    fn trial_apply(&self, state: &Counted, command: &ToyCommand, ctx: &Ctx) -> Result<Applied<ToyNotice>, SmRefusal> {
        self.0.trial_apply(&state.toy, command, ctx)
    }
    fn reset(&self, target: &Counted, current: &Counted) -> Result<Counted, SmError> {
        // The reset state continues the current lineage: what it applied
        // before the reset it applied once.
        self.0.reset(&target.toy, &current.toy).map(|toy| Counted { toy, trail: current.trail.clone() })
    }
    fn hold(&self, command: &ToyCommand) -> Option<u64> {
        self.0.hold(command)
    }
    fn coalesce(&self, held: &ToyCommand, next: &ToyCommand) -> Coalesce<ToyCommand> {
        self.0.coalesce(held, next)
    }
    fn rebind(
        &self,
        commands: &[ToyCommand],
        from_device_no: u16,
        to_device_no: u16,
        view: &Counted,
        origin_state: &[u8],
    ) -> Result<(Vec<ToyCommand>, Vec<u8>), SmError> {
        self.0.rebind(commands, from_device_no, to_device_no, &view.toy, origin_state)
    }
    fn notice_concerns(&self, notice: &ToyNotice, device_no: u16) -> bool {
        self.0.notice_concerns(notice, device_no)
    }
    fn notice_bytes(&self, notice: &ToyNotice) -> Vec<u8> {
        self.0.notice_bytes(notice)
    }
    fn refusal_bytes(&self, seq: u64, author: u16, refusal: &SmRefusal) -> Vec<u8> {
        self.0.refusal_bytes(seq, author, refusal)
    }
}
