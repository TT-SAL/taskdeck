//! `CoreMachine`: `taskdeck_sync::StateMachine` over `taskdeck-core`
//! (engine.md §5.1, decision D6; device.md §4.1, §25).
//!
//! Stateless: every method is a call into `taskdeck-core`. The state is
//! `BoardData`, the command `LogCommand`, the notice `apply::Notice`. The
//! origin state of an `Enqueue`, `Resubmit` and `rebind` is the `IdMinter`
//! laid out as device.md §8.3 states: `device_no u16 ‖ next_item u64 ‖
//! next_block u64`, 18 bytes.

use sha2::{Digest, Sha256};
use taskdeck_core::apply::{self as capply, HASH_VERSIONS, Notice, RefusalRecord, SUPPORTED_RULES};
use taskdeck_core::board::BoardData;
use taskdeck_core::command::{LogCommand, parse_command};
use taskdeck_core::ids::{self, IdMinter};
use taskdeck_core::time::Instant;
use taskdeck_sync::{Applied, Coalesce, Ctx, SmError, SmRefusal, StateMachine};

/// The code of an origin state that is not the 18 bytes of device.md §8.3,
/// or whose `device_no` is not the identity's (decision f1-6).
pub(crate) const ORIGIN_FORMAT: &str = "device/record_format";

/// The state machine over `taskdeck-core` (device.md §4.1, decision D6).
#[derive(Clone, Copy, Debug, Default)]
pub struct CoreMachine {
    _private: (),
}

impl CoreMachine {
    /// The adapter; it holds nothing.
    pub fn new() -> Self {
        CoreMachine { _private: () }
    }

    /// The origin state of device.md §8.3 for a minter.
    pub fn origin_state(minter: &IdMinter) -> Vec<u8> {
        let mut o = Vec::with_capacity(18);
        o.extend_from_slice(&minter.device_no.to_be_bytes());
        o.extend_from_slice(&minter.next_item.to_be_bytes());
        o.extend_from_slice(&minter.next_block.to_be_bytes());
        o
    }

    /// The minter of an origin state (device.md §8.3); `None` for any other
    /// length.
    pub fn minter(origin_state: &[u8]) -> Option<IdMinter> {
        let b: &[u8; 18] = origin_state.try_into().ok()?;
        let (no, rest) = b.split_at(2);
        let (item, block) = rest.split_at(8);
        Some(IdMinter {
            device_no: u16::from_be_bytes(no.try_into().ok()?),
            next_item: u64::from_be_bytes(item.try_into().ok()?),
            next_block: u64::from_be_bytes(block.try_into().ok()?),
        })
    }
}

fn err(code: &'static str) -> SmError {
    SmError { code }
}

fn refusal(r: capply::Refusal) -> SmRefusal {
    SmRefusal {
        code: r.code,
        op: Some(r.op),
        target: r.target,
        base: r.notes.map(|n| n.base),
        current: r.notes.map(|n| n.current),
    }
}

/// `core.md` §8.1 from the engine's context: `Ctx.at` is
/// `Instant::from_at_ms(at_ms)` (engine.md §36).
fn core_ctx(ctx: &Ctx) -> Option<capply::Ctx> {
    Some(capply::Ctx {
        seq: ctx.seq,
        device_no: ctx.device_no,
        at: Instant::from_at_ms(ctx.at_ms)?,
        based_on: ctx.based_on,
    })
}

impl StateMachine for CoreMachine {
    type State = BoardData;
    type Command = LogCommand;
    type Notice = Notice;

    fn supported_rules(&self) -> &[u32] {
        SUPPORTED_RULES
    }

    fn hash_versions(&self) -> &[u32] {
        HASH_VERSIONS
    }

    fn load(&self, bytes: &[u8]) -> Result<BoardData, SmError> {
        BoardData::from_canonical_bytes(bytes).map_err(|e| err(e.code))
    }

    fn canonical_bytes(&self, state: &BoardData) -> Vec<u8> {
        // Every state the engine holds came from `load`, `apply`, `reset` or
        // `upgrade`, so it passes `check` and encodes (core.md §20).
        state.to_canonical_bytes()
    }

    fn board_hash(&self, state: &BoardData) -> [u8; 32] {
        Sha256::digest(state.to_canonical_bytes()).into()
    }

    fn rules(&self, state: &BoardData) -> u32 {
        state.rules
    }

    fn upgrade(&self, state: &mut BoardData, v: u32) -> Result<(), SmError> {
        capply::upgrade_rules(state, v).map_err(|e| err(e.code))
    }

    fn parse(&self, payload: &[u8]) -> Result<LogCommand, SmError> {
        parse_command(payload).map_err(|e| err(e.code))
    }

    fn encode(&self, command: &LogCommand) -> Vec<u8> {
        // Commands come from `parse` or the compile step (core.md §20).
        command.to_canonical()
    }

    fn op_name(&self, command: &LogCommand) -> &'static str {
        command.op()
    }

    fn target(&self, command: &LogCommand) -> Option<u64> {
        command.target()
    }

    fn apply(&self, state: &mut BoardData, command: &LogCommand, ctx: &Ctx) -> Result<Applied<Notice>, SmRefusal> {
        let Some(cx) = core_ctx(ctx) else {
            return Err(SmRefusal {
                code: "core/bad_ctx",
                op: Some(command.op()),
                target: command.target(),
                base: None,
                current: None,
            });
        };
        // core.md CORE-APPLY-002: a refusal leaves the board as it was.
        match capply::apply(state, command, &cx) {
            Ok(a) => Ok(Applied { benign: a.benign, notices: a.notices }),
            Err(r) => Err(refusal(r)),
        }
    }

    fn trial_apply(&self, state: &BoardData, command: &LogCommand, ctx: &Ctx) -> Result<Applied<Notice>, SmRefusal> {
        let mut copy = state.clone();
        self.apply(&mut copy, command, ctx)
    }

    fn reset(&self, target: &BoardData, current: &BoardData) -> Result<BoardData, SmError> {
        capply::reset(target, current).map_err(|e| err(e.code))
    }

    fn hold(&self, command: &LogCommand) -> Option<u64> {
        match command {
            LogCommand::CreateTask(_) | LogCommand::CreateEvent(_) | LogCommand::CreateRoutine(_) => command.target(),
            _ => None,
        }
    }

    fn coalesce(&self, held: &LogCommand, next: &LogCommand) -> Coalesce<LogCommand> {
        let Some(id) = self.hold(held) else { return Coalesce::Keep };
        match next {
            LogCommand::Rename { id: n, name } if *n == id => match held.clone().with_name(name.clone()) {
                Ok(c) => Coalesce::Merge(c),
                Err(_) => Coalesce::Keep,
            },
            LogCommand::Forget { id: n } if *n == id => Coalesce::CancelBoth,
            _ => Coalesce::Keep,
        }
    }

    fn rebind(
        &self,
        commands: &[LogCommand],
        from_device_no: u16,
        to_device_no: u16,
        view: &BoardData,
        origin_state: &[u8],
    ) -> Result<(Vec<LogCommand>, Vec<u8>), SmError> {
        let mut minter = CoreMachine::minter(origin_state).ok_or(err(ORIGIN_FORMAT))?;
        if minter.device_no != to_device_no {
            return Err(err(ORIGIN_FORMAT));
        }
        let out = ids::rebind(commands, from_device_no, view, &mut minter).map_err(|e| err(e.code))?;
        Ok((out, CoreMachine::origin_state(&minter)))
    }

    fn notice_concerns(&self, notice: &Notice, device_no: u16) -> bool {
        notice.concerns(device_no)
    }

    fn notice_bytes(&self, notice: &Notice) -> Vec<u8> {
        notice.to_canonical()
    }

    fn refusal_bytes(&self, seq: u64, author: u16, refusal: &SmRefusal) -> Vec<u8> {
        RefusalRecord {
            seq,
            author,
            code: refusal.code.to_owned(),
            op: refusal.op.map(str::to_owned),
            target: refusal.target,
            base: refusal.base,
            current: refusal.current,
        }
        .to_canonical()
    }
}

#[cfg(test)]
mod tests;
