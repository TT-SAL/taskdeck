//! The toy state machine of the simulator (engine.md §5.3), compiled only with
//! the feature `sim` (ENG-TOY-002). Working code: stage `engine` starts from it.
//!
//! Every integer is big-endian, every map is a `BTreeMap` (ascending order is
//! the canonical order) and no `usize` is encoded, so the bytes do not depend
//! on the platform.

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use crate::{Applied, Coalesce, Ctx, SmError, SmRefusal, StateMachine};

const MAGIC: &[u8; 4] = b"TOY1";
/// `2^40`: an item id is `device_no × 2^40 + n` (engine.md §5.3).
const ID_SPLIT: u64 = 1 << 40;
const ID_LOW: u64 = ID_SPLIT - 1;
const MAX_DEVICE_NO: u16 = 8_191;
const MAX_KEYS: usize = 256;
const MAX_ITEMS: usize = 10_000;
const MAX_MARKS: usize = 8_191;

const RULES_1: &[u32] = &[1];
const RULES_1_2: &[u32] = &[1, 2];
const HASH_VERSIONS: &[u32] = &[1];

const TOY_FORMAT: &str = "engine/toy_format";
const TOY_PARSE: &str = "engine/toy_parse";
const TOY_BAD_CTX: &str = "engine/toy_bad_ctx";
const TOY_RULES: &str = "engine/toy_rules";
const TOY_GONE: &str = "engine/toy_gone";
const TOY_OVERFLOW: &str = "engine/toy_overflow";
const TOY_BAD_ID: &str = "engine/toy_bad_id";
const TOY_CAP: &str = "engine/toy_cap";
const TOY_FAIL: &str = "engine/toy_fail";
const TOY_UNKNOWN_RULES: &str = "engine/toy_unknown_rules";

/// The toy state machine (engine.md §5.3).
pub struct ToyMachine {
    rules2: bool,
}

impl ToyMachine {
    /// A toy that supports rules 1, and 2 when `rules2` (engine.md §5.3 "Other methods").
    pub fn new(rules2: bool) -> Self {
        ToyMachine { rules2 }
    }
}

/// A toy command (engine.md §5.3 "Commands").
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToyCommand {
    Set { key: u8, value: i64 },
    Add { key: u8, delta: i64 },
    Del { key: u8 },
    Create { id: u64, name: u32 },
    Name { id: u64, name: u32 },
    Forget { id: u64 },
    Fail { code: u8 },
    Panic,
    Mul { key: u8, factor: i64 },
}

/// The toy state (engine.md §5.3 "State").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToyState {
    /// The active rules version.
    pub rules: u32,
    /// `key → (value, stamp_seq, stamp_dev)`.
    pub keys: BTreeMap<u8, (i64, u64, u16)>,
    /// `id → name`.
    pub items: BTreeMap<u64, u32>,
    /// `device_no → the highest item number that device minted`.
    pub marks: BTreeMap<u16, u64>,
}

/// The notice of a `Set` over another device's newer value (engine.md §5.3 `apply`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToyNotice {
    pub seq: u64,
    pub author: u16,
    pub other: u16,
    pub other_seq: u64,
    pub key: u8,
}

/// A strict big-endian reader over a byte slice.
struct Reader<'a> {
    rest: &'a [u8],
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Reader { rest: bytes }
    }

    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let (head, tail) = self.rest.split_first_chunk::<N>()?;
        self.rest = tail;
        Some(*head)
    }

    fn u8(&mut self) -> Option<u8> {
        self.take::<1>().map(|b| b[0])
    }

    fn u16(&mut self) -> Option<u16> {
        self.take().map(u16::from_be_bytes)
    }

    fn u32(&mut self) -> Option<u32> {
        self.take().map(u32::from_be_bytes)
    }

    fn u64(&mut self) -> Option<u64> {
        self.take().map(u64::from_be_bytes)
    }

    fn i64(&mut self) -> Option<i64> {
        self.take().map(i64::from_be_bytes)
    }

    fn is_empty(&self) -> bool {
        self.rest.is_empty()
    }
}

fn mark_of(marks: &BTreeMap<u16, u64>, device_no: u16) -> u64 {
    marks.get(&device_no).copied().unwrap_or(0)
}

impl ToyMachine {
    fn decode_state(&self, bytes: &[u8]) -> Option<ToyState> {
        let mut r = Reader::new(bytes);
        if &r.take::<4>()? != MAGIC {
            return None;
        }
        let rules = r.u32()?;
        if !self.supported_rules().contains(&rules) {
            return None;
        }

        let n_keys = usize::from(r.u16()?);
        if n_keys > MAX_KEYS {
            return None;
        }
        let mut keys = BTreeMap::new();
        let mut last_key: Option<u8> = None;
        for _ in 0..n_keys {
            let key = r.u8()?;
            let value = r.i64()?;
            let stamp_seq = r.u64()?;
            let stamp_dev = r.u16()?;
            if last_key.is_some_and(|last| key <= last) {
                return None;
            }
            last_key = Some(key);
            keys.insert(key, (value, stamp_seq, stamp_dev));
        }

        let n_items = usize::try_from(r.u32()?).ok()?;
        if n_items > MAX_ITEMS {
            return None;
        }
        let mut items = BTreeMap::new();
        let mut last_id: Option<u64> = None;
        for _ in 0..n_items {
            let id = r.u64()?;
            let name = r.u32()?;
            if last_id.is_some_and(|last| id <= last) {
                return None;
            }
            last_id = Some(id);
            items.insert(id, name);
        }

        let n_marks = usize::from(r.u16()?);
        if n_marks > MAX_MARKS {
            return None;
        }
        let mut marks = BTreeMap::new();
        let mut last_dev: Option<u16> = None;
        for _ in 0..n_marks {
            let device_no = r.u16()?;
            let n = r.u64()?;
            // Strictly ascending, `device_no` from 1 to 8,191, `n` from 1 to 2^40 − 1 (engine.md §5.3).
            if last_dev.is_some_and(|last| device_no <= last)
                || !(1..=MAX_DEVICE_NO).contains(&device_no)
                || !(1..=ID_LOW).contains(&n)
            {
                return None;
            }
            last_dev = Some(device_no);
            marks.insert(device_no, n);
        }
        if !r.is_empty() {
            return None;
        }

        for &id in items.keys() {
            let h = id / ID_SPLIT;
            let n = id % ID_SPLIT;
            if h == 0 || h > u64::from(MAX_DEVICE_NO) {
                return None;
            }
            // `h` is at most 8,191 here, so it fits a u16.
            let h = u16::try_from(h).ok()?;
            if n == 0 || n > mark_of(&marks, h) {
                return None;
            }
        }
        Some(ToyState { rules, keys, items, marks })
    }

    fn decode_command(payload: &[u8]) -> Option<ToyCommand> {
        let (&op, rest) = payload.split_first()?;
        let mut r = Reader::new(rest);
        let command = match op {
            1 => ToyCommand::Set { key: r.u8()?, value: r.i64()? },
            2 => ToyCommand::Add { key: r.u8()?, delta: r.i64()? },
            3 => ToyCommand::Del { key: r.u8()? },
            4 => ToyCommand::Create { id: r.u64()?, name: r.u32()? },
            5 => ToyCommand::Name { id: r.u64()?, name: r.u32()? },
            6 => ToyCommand::Forget { id: r.u64()? },
            7 => ToyCommand::Fail { code: r.u8()? },
            8 => ToyCommand::Panic,
            9 => ToyCommand::Mul { key: r.u8()?, factor: r.i64()? },
            _ => return None,
        };
        r.is_empty().then_some(command)
    }

    fn refusal(&self, command: &ToyCommand, code: &'static str) -> SmRefusal {
        SmRefusal { code, op: Some(self.op_name(command)), target: self.target(command), base: None, current: None }
    }
}

fn applied(benign: bool, notices: Vec<ToyNotice>) -> Applied<ToyNotice> {
    Applied { benign, notices }
}

fn put_vb8(out: &mut Vec<u8>, text: &str) {
    // A code or op name is a short constant; a longer one is cut to 255 bytes.
    let bytes = &text.as_bytes()[..text.len().min(255)];
    out.push(bytes.len() as u8);
    out.extend_from_slice(bytes);
}

impl StateMachine for ToyMachine {
    type State = ToyState;
    type Command = ToyCommand;
    type Notice = ToyNotice;

    fn supported_rules(&self) -> &[u32] {
        if self.rules2 { RULES_1_2 } else { RULES_1 }
    }

    fn hash_versions(&self) -> &[u32] {
        HASH_VERSIONS
    }

    fn load(&self, bytes: &[u8]) -> Result<ToyState, SmError> {
        self.decode_state(bytes).ok_or(SmError { code: TOY_FORMAT })
    }

    fn canonical_bytes(&self, state: &ToyState) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            16 + 19 * state.keys.len() + 12 * state.items.len() + 10 * state.marks.len(),
        );
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&state.rules.to_be_bytes());
        out.extend_from_slice(&(state.keys.len() as u16).to_be_bytes());
        for (&key, &(value, stamp_seq, stamp_dev)) in &state.keys {
            out.push(key);
            out.extend_from_slice(&value.to_be_bytes());
            out.extend_from_slice(&stamp_seq.to_be_bytes());
            out.extend_from_slice(&stamp_dev.to_be_bytes());
        }
        out.extend_from_slice(&(state.items.len() as u32).to_be_bytes());
        for (&id, &name) in &state.items {
            out.extend_from_slice(&id.to_be_bytes());
            out.extend_from_slice(&name.to_be_bytes());
        }
        out.extend_from_slice(&(state.marks.len() as u16).to_be_bytes());
        for (&device_no, &n) in &state.marks {
            out.extend_from_slice(&device_no.to_be_bytes());
            out.extend_from_slice(&n.to_be_bytes());
        }
        out
    }

    fn board_hash(&self, state: &ToyState) -> [u8; 32] {
        Sha256::digest(self.canonical_bytes(state)).into()
    }

    fn rules(&self, state: &ToyState) -> u32 {
        state.rules
    }

    fn upgrade(&self, state: &mut ToyState, v: u32) -> Result<(), SmError> {
        if v <= state.rules || !self.supported_rules().contains(&v) {
            return Err(SmError { code: TOY_UNKNOWN_RULES });
        }
        state.rules = v;
        Ok(())
    }

    fn parse(&self, payload: &[u8]) -> Result<ToyCommand, SmError> {
        Self::decode_command(payload).ok_or(SmError { code: TOY_PARSE })
    }

    fn encode(&self, command: &ToyCommand) -> Vec<u8> {
        let mut out = Vec::with_capacity(13);
        match *command {
            ToyCommand::Set { key, value } => {
                out.push(1);
                out.push(key);
                out.extend_from_slice(&value.to_be_bytes());
            }
            ToyCommand::Add { key, delta } => {
                out.push(2);
                out.push(key);
                out.extend_from_slice(&delta.to_be_bytes());
            }
            ToyCommand::Del { key } => {
                out.push(3);
                out.push(key);
            }
            ToyCommand::Create { id, name } => {
                out.push(4);
                out.extend_from_slice(&id.to_be_bytes());
                out.extend_from_slice(&name.to_be_bytes());
            }
            ToyCommand::Name { id, name } => {
                out.push(5);
                out.extend_from_slice(&id.to_be_bytes());
                out.extend_from_slice(&name.to_be_bytes());
            }
            ToyCommand::Forget { id } => {
                out.push(6);
                out.extend_from_slice(&id.to_be_bytes());
            }
            ToyCommand::Fail { code } => {
                out.push(7);
                out.push(code);
            }
            ToyCommand::Panic => out.push(8),
            ToyCommand::Mul { key, factor } => {
                out.push(9);
                out.push(key);
                out.extend_from_slice(&factor.to_be_bytes());
            }
        }
        out
    }

    fn op_name(&self, command: &ToyCommand) -> &'static str {
        match command {
            ToyCommand::Set { .. } => "set",
            ToyCommand::Add { .. } => "add",
            ToyCommand::Del { .. } => "del",
            ToyCommand::Create { .. } => "create",
            ToyCommand::Name { .. } => "name",
            ToyCommand::Forget { .. } => "forget",
            ToyCommand::Fail { .. } => "fail",
            ToyCommand::Panic => "panic",
            ToyCommand::Mul { .. } => "mul",
        }
    }

    fn target(&self, command: &ToyCommand) -> Option<u64> {
        match *command {
            ToyCommand::Set { key, .. }
            | ToyCommand::Add { key, .. }
            | ToyCommand::Del { key }
            | ToyCommand::Mul { key, .. } => Some(u64::from(key)),
            ToyCommand::Create { id, .. } | ToyCommand::Name { id, .. } | ToyCommand::Forget { id } => Some(id),
            ToyCommand::Fail { .. } | ToyCommand::Panic => None,
        }
    }

    fn apply(&self, state: &mut ToyState, command: &ToyCommand, ctx: &Ctx) -> Result<Applied<ToyNotice>, SmRefusal> {
        // Step 1: the context.
        if !(1..=MAX_DEVICE_NO).contains(&ctx.device_no) || ctx.seq < 2 || ctx.based_on >= ctx.seq {
            return Err(self.refusal(command, TOY_BAD_CTX));
        }
        // Step 2: the rules versions column of the commands table.
        let allowed = match command {
            ToyCommand::Mul { .. } => state.rules == 2,
            _ => state.rules == 1 || state.rules == 2,
        };
        if !allowed {
            return Err(self.refusal(command, TOY_RULES));
        }
        // Step 3: per op. Every check comes before the first change, so a
        // refusal leaves the state unchanged.
        let stamp = (ctx.seq, ctx.device_no);
        match *command {
            ToyCommand::Set { key, value } => {
                let mut notices = Vec::new();
                if let Some(&(_, stamp_seq, stamp_dev)) = state.keys.get(&key)
                    && stamp_seq > ctx.based_on
                    && stamp_dev != ctx.device_no
                    && stamp_dev != 0
                {
                    notices.push(ToyNotice {
                        seq: ctx.seq,
                        author: ctx.device_no,
                        other: stamp_dev,
                        other_seq: stamp_seq,
                        key,
                    });
                }
                state.keys.insert(key, (value, stamp.0, stamp.1));
                Ok(applied(false, notices))
            }
            ToyCommand::Add { key, delta: operand } | ToyCommand::Mul { key, factor: operand } => {
                let Some(&(value, _, _)) = state.keys.get(&key) else {
                    return Err(self.refusal(command, TOY_GONE));
                };
                let result = if matches!(command, ToyCommand::Add { .. }) {
                    value.checked_add(operand)
                } else {
                    value.checked_mul(operand)
                };
                let Some(result) = result else {
                    return Err(self.refusal(command, TOY_OVERFLOW));
                };
                state.keys.insert(key, (result, stamp.0, stamp.1));
                Ok(applied(false, Vec::new()))
            }
            ToyCommand::Del { key } => Ok(applied(state.keys.remove(&key).is_none(), Vec::new())),
            ToyCommand::Create { id, name } => {
                let h = id / ID_SPLIT;
                let n = id % ID_SPLIT;
                if h != u64::from(ctx.device_no) || n == 0 || n <= mark_of(&state.marks, ctx.device_no) {
                    return Err(self.refusal(command, TOY_BAD_ID));
                }
                if state.items.len() >= MAX_ITEMS {
                    return Err(self.refusal(command, TOY_CAP));
                }
                state.items.insert(id, name);
                state.marks.insert(ctx.device_no, n);
                Ok(applied(false, Vec::new()))
            }
            ToyCommand::Name { id, name } => match state.items.get_mut(&id) {
                Some(slot) => {
                    *slot = name;
                    Ok(applied(false, Vec::new()))
                }
                None => Err(self.refusal(command, TOY_GONE)),
            },
            ToyCommand::Forget { id } => Ok(applied(state.items.remove(&id).is_none(), Vec::new())),
            ToyCommand::Fail { .. } => Err(self.refusal(command, TOY_FAIL)),
            ToyCommand::Panic => panic!("toy: the Panic command"),
        }
    }

    fn trial_apply(&self, state: &ToyState, command: &ToyCommand, ctx: &Ctx) -> Result<Applied<ToyNotice>, SmRefusal> {
        let mut copy = state.clone();
        self.apply(&mut copy, command, ctx)
    }

    fn reset(&self, target: &ToyState, current: &ToyState) -> Result<ToyState, SmError> {
        let mut out = target.clone();
        out.rules = current.rules;
        for (&device_no, &n) in &current.marks {
            let mark = out.marks.entry(device_no).or_insert(n);
            *mark = (*mark).max(n);
        }
        Ok(out)
    }

    fn hold(&self, command: &ToyCommand) -> Option<u64> {
        match *command {
            ToyCommand::Create { id, .. } => Some(id),
            _ => None,
        }
    }

    fn coalesce(&self, held: &ToyCommand, next: &ToyCommand) -> Coalesce<ToyCommand> {
        let ToyCommand::Create { id, .. } = *held else {
            return Coalesce::Keep;
        };
        match *next {
            ToyCommand::Name { id: named, name } if named == id => Coalesce::Merge(ToyCommand::Create { id, name }),
            ToyCommand::Forget { id: forgotten } if forgotten == id => Coalesce::CancelBoth,
            _ => Coalesce::Keep,
        }
    }

    fn rebind(
        &self,
        commands: &[ToyCommand],
        from_device_no: u16,
        to_device_no: u16,
        view: &ToyState,
        origin_state: &[u8],
    ) -> Result<(Vec<ToyCommand>, Vec<u8>), SmError> {
        let mut next_n = u64::from_be_bytes(
            <[u8; 8]>::try_from(origin_state).map_err(|_| SmError { code: TOY_FORMAT })?,
        );
        let m = mark_of(&view.marks, to_device_no);
        let mut mapping: BTreeMap<u64, u64> = BTreeMap::new();
        let mut out = Vec::with_capacity(commands.len());
        for command in commands {
            let rewritten = match *command {
                ToyCommand::Create { id, name } if id / ID_SPLIT == u64::from(from_device_no) => {
                    let n = next_n.max(m.saturating_add(1));
                    if n > ID_LOW {
                        return Err(SmError { code: TOY_BAD_ID });
                    }
                    let new_id = u64::from(to_device_no) * ID_SPLIT + n;
                    next_n = n + 1;
                    mapping.insert(id, new_id);
                    ToyCommand::Create { id: new_id, name }
                }
                ToyCommand::Name { id, name } => {
                    ToyCommand::Name { id: mapping.get(&id).copied().unwrap_or(id), name }
                }
                ToyCommand::Forget { id } => ToyCommand::Forget { id: mapping.get(&id).copied().unwrap_or(id) },
                ref other => other.clone(),
            };
            out.push(rewritten);
        }
        Ok((out, next_n.to_be_bytes().to_vec()))
    }

    fn notice_concerns(&self, notice: &ToyNotice, device_no: u16) -> bool {
        notice.author == device_no || notice.other == device_no
    }

    fn notice_bytes(&self, notice: &ToyNotice) -> Vec<u8> {
        let mut out = Vec::with_capacity(21);
        out.extend_from_slice(&notice.seq.to_be_bytes());
        out.extend_from_slice(&notice.author.to_be_bytes());
        out.extend_from_slice(&notice.other.to_be_bytes());
        out.extend_from_slice(&notice.other_seq.to_be_bytes());
        out.push(notice.key);
        out
    }

    fn refusal_bytes(&self, seq: u64, author: u16, refusal: &SmRefusal) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&seq.to_be_bytes());
        out.extend_from_slice(&author.to_be_bytes());
        put_vb8(&mut out, refusal.code);
        put_vb8(&mut out, refusal.op.unwrap_or(""));
        match refusal.target {
            Some(target) => {
                out.push(1);
                out.extend_from_slice(&target.to_be_bytes());
            }
            None => {
                out.push(0);
                out.extend_from_slice(&[0; 8]);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(text: &str) -> Vec<u8> {
        let digits: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
        assert!(digits.len().is_multiple_of(2), "odd hex");
        digits
            .chunks(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }

    fn ctx(seq: u64, device_no: u16, based_on: u64) -> Ctx {
        Ctx { seq, device_no, at_ms: 1_700_000_000_000, based_on }
    }

    fn id(device_no: u16, n: u64) -> u64 {
        u64::from(device_no) * ID_SPLIT + n
    }

    fn ok(result: Result<Applied<ToyNotice>, SmRefusal>) -> Applied<ToyNotice> {
        match result {
            Ok(applied) => applied,
            Err(refusal) => panic!("refused with {}", refusal.code),
        }
    }

    fn refused(result: Result<Applied<ToyNotice>, SmRefusal>) -> SmRefusal {
        match result {
            Ok(_) => panic!("expected a refusal"),
            Err(refusal) => refusal,
        }
    }

    fn loaded(toy: &ToyMachine, bytes: &[u8]) -> ToyState {
        match toy.load(bytes) {
            Ok(state) => state,
            Err(e) => panic!("load refused with {}", e.code),
        }
    }

    fn load_code(toy: &ToyMachine, bytes: &[u8]) -> &'static str {
        match toy.load(bytes) {
            Ok(_) => "ok",
            Err(e) => e.code,
        }
    }

    fn parse_code(toy: &ToyMachine, bytes: &[u8]) -> &'static str {
        match toy.parse(bytes) {
            Ok(_) => "ok",
            Err(e) => e.code,
        }
    }

    fn empty(toy: &ToyMachine) -> ToyState {
        loaded(toy, &hex(EMPTY))
    }

    /// The empty state (16 bytes).
    const EMPTY: &str = "544f5931 00000001 0000 00000000 0000";
    /// Key 7 = (-2, stamp 5 by device 3); item 2^40 + 1 named 9; device 1's mark 1 (57 bytes).
    const SAMPLE: &str = "544f5931 00000001 \
                          0001 07 fffffffffffffffe 0000000000000005 0003 \
                          00000001 0000010000000001 00000009 \
                          0001 0001 0000000000000001";

    /// Builds state bytes from parts, for the refusal cases of `load`.
    fn state_bytes(rules: u32, keys: &[(u8, i64, u64, u16)], items: &[(u64, u32)], marks: &[(u16, u64)]) -> Vec<u8> {
        let mut out = b"TOY1".to_vec();
        out.extend_from_slice(&rules.to_be_bytes());
        out.extend_from_slice(&(keys.len() as u16).to_be_bytes());
        for &(key, value, stamp_seq, stamp_dev) in keys {
            out.push(key);
            out.extend_from_slice(&value.to_be_bytes());
            out.extend_from_slice(&stamp_seq.to_be_bytes());
            out.extend_from_slice(&stamp_dev.to_be_bytes());
        }
        out.extend_from_slice(&(items.len() as u32).to_be_bytes());
        for &(item, name) in items {
            out.extend_from_slice(&item.to_be_bytes());
            out.extend_from_slice(&name.to_be_bytes());
        }
        out.extend_from_slice(&(marks.len() as u16).to_be_bytes());
        for &(device_no, n) in marks {
            out.extend_from_slice(&device_no.to_be_bytes());
            out.extend_from_slice(&n.to_be_bytes());
        }
        out
    }

    // ---------- state bytes

    #[test]
    fn the_empty_state_is_16_bytes() {
        let toy = ToyMachine::new(false);
        let state = empty(&toy);
        assert_eq!(state.rules, 1);
        assert!(state.keys.is_empty() && state.items.is_empty() && state.marks.is_empty());
        assert_eq!(toy.canonical_bytes(&state), hex(EMPTY));
        assert_eq!(toy.canonical_bytes(&state).len(), 16);
    }

    #[test]
    fn a_state_round_trips_through_its_canonical_bytes() {
        let toy = ToyMachine::new(false);
        let state = loaded(&toy, &hex(SAMPLE));
        assert_eq!(state.keys.get(&7), Some(&(-2, 5, 3)));
        assert_eq!(state.items.get(&id(1, 1)), Some(&9));
        assert_eq!(state.marks.get(&1), Some(&1));
        let bytes = toy.canonical_bytes(&state);
        assert_eq!(bytes, hex(SAMPLE));
        assert_eq!(bytes.len(), 16 + 19 + 12 + 10);
        assert_eq!(toy.canonical_bytes(&loaded(&toy, &bytes)), bytes);
    }

    #[test]
    fn the_board_hash_is_sha256_of_the_canonical_bytes() {
        let toy = ToyMachine::new(false);
        let state = loaded(&toy, &hex(SAMPLE));
        let expected: [u8; 32] = Sha256::digest(hex(SAMPLE)).into();
        assert_eq!(toy.board_hash(&state), expected);
        assert_eq!(toy.rules(&state), 1);
    }

    #[test]
    fn load_refuses_every_violation_of_the_table() {
        let toy = ToyMachine::new(false);
        let toy2 = ToyMachine::new(true);
        let format = "engine/toy_format";
        let mark1 = [(1u16, 1u64)];
        // A well-formed base for the mutations below.
        assert_eq!(load_code(&toy, &state_bytes(1, &[(1, 0, 0, 0)], &[(id(1, 1), 0)], &mark1)), "ok");

        let mut bad_magic = hex(EMPTY);
        bad_magic[3] = b'2';
        assert_eq!(load_code(&toy, &bad_magic), format);
        // Rules: a supported version only.
        assert_eq!(load_code(&toy, &state_bytes(0, &[], &[], &[])), format);
        assert_eq!(load_code(&toy, &state_bytes(2, &[], &[], &[])), format);
        assert_eq!(load_code(&toy2, &state_bytes(2, &[], &[], &[])), "ok");
        assert_eq!(load_code(&toy2, &state_bytes(3, &[], &[], &[])), format);
        // Length: no trailing bytes, nothing missing.
        let mut trailing = hex(EMPTY);
        trailing.push(0);
        assert_eq!(load_code(&toy, &trailing), format);
        assert_eq!(load_code(&toy, &hex(EMPTY)[..15]), format);
        assert_eq!(load_code(&toy, &hex(SAMPLE)[..56]), format);
        assert_eq!(load_code(&toy, &[]), format);
        // Keys strictly ascending.
        assert_eq!(load_code(&toy, &state_bytes(1, &[(2, 0, 0, 0), (2, 1, 0, 0)], &[], &[])), format);
        assert_eq!(load_code(&toy, &state_bytes(1, &[(3, 0, 0, 0), (2, 1, 0, 0)], &[], &[])), format);
        // n_keys above 256.
        let mut many_keys = state_bytes(1, &[], &[], &[]);
        many_keys[8..10].copy_from_slice(&257u16.to_be_bytes());
        assert_eq!(load_code(&toy, &many_keys), format);
        // Items strictly ascending.
        let two_marks = [(1u16, 2u64)];
        assert_eq!(load_code(&toy, &state_bytes(1, &[], &[(id(1, 2), 0), (id(1, 1), 0)], &two_marks)), format);
        assert_eq!(load_code(&toy, &state_bytes(1, &[], &[(id(1, 1), 0), (id(1, 1), 0)], &two_marks)), format);
        // Item ids: high part 0 or above 8,191, low part 0, low part above the mark, no mark.
        assert_eq!(load_code(&toy, &state_bytes(1, &[], &[(id(0, 1), 0)], &[(0, 1)])), format);
        assert_eq!(load_code(&toy, &state_bytes(1, &[], &[(id(8_192, 1), 0)], &[(8_192, 1)])), format);
        assert_eq!(load_code(&toy, &state_bytes(1, &[], &[(id(8_191, 1), 0)], &[(8_191, 1)])), "ok");
        assert_eq!(load_code(&toy, &state_bytes(1, &[], &[(id(1, 0), 0)], &mark1)), format);
        assert_eq!(load_code(&toy, &state_bytes(1, &[], &[(id(1, 2), 0)], &mark1)), format);
        assert_eq!(load_code(&toy, &state_bytes(1, &[], &[(id(1, 1), 0)], &[])), format);
        // Marks strictly ascending, n at least 1.
        assert_eq!(load_code(&toy, &state_bytes(1, &[], &[], &[(2, 1), (1, 1)])), format);
        assert_eq!(load_code(&toy, &state_bytes(1, &[], &[], &[(1, 1), (1, 2)])), format);
        assert_eq!(load_code(&toy, &state_bytes(1, &[], &[], &[(1, 0)])), format);
        // A mark's device_no from 1 to 8,191 and its n from 1 to 2^40 - 1, with no item.
        assert_eq!(load_code(&toy, &state_bytes(1, &[], &[], &[(0, 1)])), format);
        assert_eq!(load_code(&toy, &state_bytes(1, &[], &[], &[(8_192, 1)])), format);
        assert_eq!(load_code(&toy, &state_bytes(1, &[], &[], &[(u16::MAX, 1)])), format);
        assert_eq!(load_code(&toy, &state_bytes(1, &[], &[], &[(1, 1), (8_191, 1)])), "ok");
        assert_eq!(load_code(&toy, &state_bytes(1, &[], &[], &[(1, ID_LOW)])), "ok");
        assert_eq!(load_code(&toy, &state_bytes(1, &[], &[], &[(1, ID_SPLIT)])), format);
        assert_eq!(load_code(&toy, &state_bytes(1, &[], &[], &[(1, u64::MAX)])), format);
        assert_eq!(load_code(&toy, &state_bytes(1, &[], &[], &[(2, 1), (3, ID_SPLIT)])), format);
        // n_items above 10,000 and n_marks above 8,191, with well-formed entries
        // (8,191 marks is every device_no, so an 8,192nd cannot be well formed).
        let items: Vec<(u64, u32)> = (1..=10_001).map(|n| (id(1, n), 0)).collect();
        assert_eq!(load_code(&toy, &state_bytes(1, &[], &items, &[(1, 10_001)])), format);
        assert_eq!(load_code(&toy, &state_bytes(1, &[], &items[..10_000], &[(1, 10_001)])), "ok");
        let marks: Vec<(u16, u64)> = (1..=8_192).map(|d| (d, 1)).collect();
        assert_eq!(load_code(&toy, &state_bytes(1, &[], &[], &marks)), format);
        assert_eq!(load_code(&toy, &state_bytes(1, &[], &[], &marks[..8_191])), "ok");
    }

    // ---------- commands

    fn command_table() -> Vec<(ToyCommand, &'static str, &'static str, Option<u64>)> {
        let item = id(1, 1);
        vec![
            (ToyCommand::Set { key: 7, value: 5 }, "01 07 0000000000000005", "set", Some(7)),
            (ToyCommand::Add { key: 7, delta: -1 }, "02 07 ffffffffffffffff", "add", Some(7)),
            (ToyCommand::Del { key: 7 }, "03 07", "del", Some(7)),
            (ToyCommand::Create { id: item, name: 9 }, "04 0000010000000001 00000009", "create", Some(item)),
            (ToyCommand::Name { id: item, name: 10 }, "05 0000010000000001 0000000a", "name", Some(item)),
            (ToyCommand::Forget { id: item }, "06 0000010000000001", "forget", Some(item)),
            (ToyCommand::Fail { code: 3 }, "07 03", "fail", None),
            (ToyCommand::Panic, "08", "panic", None),
            (ToyCommand::Mul { key: 7, factor: 3 }, "09 07 0000000000000003", "mul", Some(7)),
        ]
    }

    #[test]
    fn every_command_encodes_parses_and_names_as_the_table_states() {
        let toy = ToyMachine::new(false);
        let widths = [10, 10, 2, 13, 13, 9, 2, 1, 10];
        for ((command, bytes, name, target), width) in command_table().into_iter().zip(widths) {
            let encoded = toy.encode(&command);
            assert_eq!(encoded, hex(bytes), "{name}");
            assert_eq!(encoded.len(), width, "{name}");
            // `parse` accepts op 9 under any rules: the check is apply's.
            assert_eq!(toy.parse(&encoded).ok(), Some(command.clone()), "{name}");
            assert_eq!(toy.op_name(&command), name);
            assert_eq!(toy.target(&command), target, "{name}");
        }
    }

    #[test]
    fn parse_refuses_unknown_ops_and_wrong_widths() {
        let toy = ToyMachine::new(false);
        let code = "engine/toy_parse";
        assert_eq!(parse_code(&toy, &[]), code);
        assert_eq!(parse_code(&toy, &[0]), code);
        assert_eq!(parse_code(&toy, &[10]), code);
        assert_eq!(parse_code(&toy, &[255]), code);
        for (command, _, name, _) in command_table() {
            let encoded = toy.encode(&command);
            let mut longer = encoded.clone();
            longer.push(0);
            assert_eq!(parse_code(&toy, &longer), code, "{name} with a trailing byte");
            if encoded.len() > 1 {
                assert_eq!(parse_code(&toy, &encoded[..encoded.len() - 1]), code, "{name} cut short");
            }
        }
    }

    // ---------- apply

    #[test]
    fn apply_refuses_a_context_out_of_range() {
        let toy = ToyMachine::new(false);
        let command = ToyCommand::Set { key: 1, value: 1 };
        for bad in [ctx(5, 0, 1), ctx(5, 8_192, 1), ctx(1, 1, 0), ctx(5, 1, 5), ctx(5, 1, 6)] {
            let mut state = empty(&toy);
            let refusal = refused(toy.apply(&mut state, &command, &bad));
            assert_eq!(refusal.code, "engine/toy_bad_ctx");
            assert_eq!(toy.canonical_bytes(&state), hex(EMPTY));
        }
        let mut state = empty(&toy);
        ok(toy.apply(&mut state, &command, &ctx(2, 8_191, 1)));
    }

    #[test]
    fn mul_is_allowed_only_under_rules_2() {
        let toy = ToyMachine::new(true);
        let mut state = empty(&toy);
        ok(toy.apply(&mut state, &ToyCommand::Set { key: 1, value: 6 }, &ctx(2, 1, 1)));
        let before = toy.canonical_bytes(&state);
        let refusal = refused(toy.apply(&mut state, &ToyCommand::Mul { key: 1, factor: 7 }, &ctx(3, 1, 2)));
        assert_eq!(refusal.code, "engine/toy_rules");
        assert_eq!(toy.canonical_bytes(&state), before);

        if toy.upgrade(&mut state, 2).is_err() {
            panic!("upgrade to 2 refused");
        }
        let applied = ok(toy.apply(&mut state, &ToyCommand::Mul { key: 1, factor: 7 }, &ctx(4, 2, 3)));
        assert!(!applied.benign && applied.notices.is_empty());
        assert_eq!(state.keys.get(&1), Some(&(42, 4, 2)));
    }

    #[test]
    fn set_stamps_and_notices_only_over_another_devices_newer_value() {
        let toy = ToyMachine::new(false);
        let mut state = empty(&toy);
        // A new key: no notice.
        let applied = ok(toy.apply(&mut state, &ToyCommand::Set { key: 7, value: 1 }, &ctx(2, 3, 1)));
        assert!(!applied.benign && applied.notices.is_empty());
        assert_eq!(state.keys.get(&7), Some(&(1, 2, 3)));
        // Device 2 composed at 1, so it had not seen device 3's value at 2: a notice.
        let applied = ok(toy.apply(&mut state, &ToyCommand::Set { key: 7, value: 5 }, &ctx(10, 2, 1)));
        let notice = ToyNotice { seq: 10, author: 2, other: 3, other_seq: 2, key: 7 };
        assert_eq!(applied.notices, vec![notice.clone()]);
        assert_eq!(state.keys.get(&7), Some(&(5, 10, 2)));
        assert_eq!(toy.notice_bytes(&notice), hex("000000000000000a 0002 0003 0000000000000002 07"));
        assert_eq!(toy.notice_bytes(&notice).len(), 21);
        // Seen (stamp_seq ≤ based_on): no notice.
        let applied = ok(toy.apply(&mut state, &ToyCommand::Set { key: 7, value: 6 }, &ctx(11, 3, 10)));
        assert!(applied.notices.is_empty());
        // The same device: no notice.
        let applied = ok(toy.apply(&mut state, &ToyCommand::Set { key: 7, value: 7 }, &ctx(12, 3, 1)));
        assert!(applied.notices.is_empty());
        // A stamp by device 0 (from loaded bytes): no notice.
        let mut zero = loaded(&toy, &state_bytes(1, &[(7, 0, 50, 0)], &[], &[]));
        let applied = ok(toy.apply(&mut zero, &ToyCommand::Set { key: 7, value: 1 }, &ctx(60, 2, 1)));
        assert!(applied.notices.is_empty());
    }

    #[test]
    fn add_and_mul_need_the_key_and_refuse_overflow() {
        let toy = ToyMachine::new(true);
        let mut state = loaded(&toy, &state_bytes(2, &[(1, i64::MAX - 1, 4, 3), (2, i64::MIN, 4, 3)], &[], &[]));
        let before = toy.canonical_bytes(&state);
        let cases = [
            (ToyCommand::Add { key: 9, delta: 1 }, "engine/toy_gone"),
            (ToyCommand::Mul { key: 9, factor: 1 }, "engine/toy_gone"),
            (ToyCommand::Add { key: 1, delta: 2 }, "engine/toy_overflow"),
            (ToyCommand::Add { key: 2, delta: -1 }, "engine/toy_overflow"),
            (ToyCommand::Mul { key: 1, factor: 2 }, "engine/toy_overflow"),
            (ToyCommand::Mul { key: 2, factor: -1 }, "engine/toy_overflow"),
        ];
        for (command, code) in cases {
            let refusal = refused(toy.apply(&mut state, &command, &ctx(10, 1, 9)));
            assert_eq!(refusal.code, code);
            assert_eq!(refusal.op, Some(toy.op_name(&command)));
            assert_eq!(refusal.target, toy.target(&command));
            assert_eq!(toy.canonical_bytes(&state), before);
        }
        // Device 1 composed at 1 over device 3's value at 4: Add stamps and gives no notice.
        let applied = ok(toy.apply(&mut state, &ToyCommand::Add { key: 1, delta: 1 }, &ctx(10, 1, 1)));
        assert!(!applied.benign && applied.notices.is_empty());
        assert_eq!(state.keys.get(&1), Some(&(i64::MAX, 10, 1)));
    }

    #[test]
    fn del_and_forget_of_an_absent_target_are_benign_repeats() {
        let toy = ToyMachine::new(false);
        let mut state = loaded(&toy, &hex(SAMPLE));
        assert!(!ok(toy.apply(&mut state, &ToyCommand::Del { key: 7 }, &ctx(10, 1, 9))).benign);
        assert!(state.keys.is_empty());
        assert!(ok(toy.apply(&mut state, &ToyCommand::Del { key: 7 }, &ctx(11, 1, 10))).benign);
        assert!(!ok(toy.apply(&mut state, &ToyCommand::Forget { id: id(1, 1) }, &ctx(12, 2, 11))).benign);
        assert!(state.items.is_empty());
        assert!(ok(toy.apply(&mut state, &ToyCommand::Forget { id: id(1, 1) }, &ctx(13, 2, 12))).benign);
        // The mark stays: forgetting does not free the number.
        assert_eq!(state.marks.get(&1), Some(&1));
        assert_eq!(toy.canonical_bytes(&state), hex("544f5931 00000001 0000 00000000 0001 0001 0000000000000001"));
    }

    #[test]
    fn create_mints_above_the_mark_of_its_own_device() {
        let toy = ToyMachine::new(false);
        let mut state = empty(&toy);
        let applied = ok(toy.apply(&mut state, &ToyCommand::Create { id: id(2, 5), name: 1 }, &ctx(2, 2, 1)));
        assert!(!applied.benign);
        assert_eq!(state.marks.get(&2), Some(&5));
        assert_eq!(
            toy.canonical_bytes(&state),
            hex("544f5931 00000001 0000 00000001 0000020000000005 00000001 0001 0002 0000000000000005")
        );
        let before = toy.canonical_bytes(&state);
        for command in [
            ToyCommand::Create { id: id(3, 6), name: 1 }, // another device's high part
            ToyCommand::Create { id: id(2, 0), name: 1 }, // n = 0
            ToyCommand::Create { id: id(2, 5), name: 1 }, // not above the mark
            ToyCommand::Create { id: id(2, 4), name: 1 }, // below the mark
        ] {
            let refusal = refused(toy.apply(&mut state, &command, &ctx(3, 2, 2)));
            assert_eq!(refusal.code, "engine/toy_bad_id");
            assert_eq!(toy.canonical_bytes(&state), before);
        }
        // The largest n, 2^40 - 1, is valid.
        ok(toy.apply(&mut state, &ToyCommand::Create { id: id(2, ID_LOW), name: 1 }, &ctx(3, 2, 2)));
        assert_eq!(state.marks.get(&2), Some(&ID_LOW));
    }

    #[test]
    fn create_refuses_the_10001st_item() {
        let toy = ToyMachine::new(false);
        let items: Vec<(u64, u32)> = (1..=10_000).map(|n| (id(1, n), 0)).collect();
        let mut state = loaded(&toy, &state_bytes(1, &[], &items, &[(1, 10_000)]));
        let before = toy.canonical_bytes(&state);
        let refusal = refused(toy.apply(&mut state, &ToyCommand::Create { id: id(1, 10_001), name: 0 }, &ctx(2, 1, 1)));
        assert_eq!(refusal.code, "engine/toy_cap");
        assert_eq!(toy.canonical_bytes(&state), before);
        // The id check comes first.
        let refusal = refused(toy.apply(&mut state, &ToyCommand::Create { id: id(1, 1), name: 0 }, &ctx(2, 1, 1)));
        assert_eq!(refusal.code, "engine/toy_bad_id");
    }

    #[test]
    fn name_needs_the_item() {
        let toy = ToyMachine::new(false);
        let mut state = loaded(&toy, &hex(SAMPLE));
        let refusal = refused(toy.apply(&mut state, &ToyCommand::Name { id: id(1, 2), name: 4 }, &ctx(10, 1, 9)));
        assert_eq!(refusal.code, "engine/toy_gone");
        assert_eq!(refusal.target, Some(id(1, 2)));
        ok(toy.apply(&mut state, &ToyCommand::Name { id: id(1, 1), name: 4 }, &ctx(10, 1, 9)));
        assert_eq!(state.items.get(&id(1, 1)), Some(&4));
    }

    #[test]
    fn fail_is_refused_and_its_record_has_the_fixed_layout() {
        let toy = ToyMachine::new(false);
        let mut state = empty(&toy);
        let refusal = refused(toy.apply(&mut state, &ToyCommand::Fail { code: 3 }, &ctx(10, 2, 9)));
        assert_eq!(refusal.code, "engine/toy_fail");
        assert_eq!(
            toy.refusal_bytes(10, 2, &refusal),
            hex("000000000000000a 0002 0f 656e67696e652f746f795f6661696c 04 6661696c 00 0000000000000000")
        );
        // With a target, and with no op.
        let gone = SmRefusal { code: "engine/toy_gone", op: Some("add"), target: Some(7), base: None, current: None };
        assert_eq!(
            toy.refusal_bytes(10, 2, &gone),
            hex("000000000000000a 0002 0f 656e67696e652f746f795f676f6e65 03 616464 01 0000000000000007")
        );
        let bare = SmRefusal { code: "engine/toy_fail", op: None, target: None, base: None, current: None };
        assert_eq!(
            toy.refusal_bytes(1, 1, &bare),
            hex("0000000000000001 0001 0f 656e67696e652f746f795f6661696c 00 00 0000000000000000")
        );
    }

    #[test]
    #[should_panic(expected = "toy: the Panic command")]
    fn panic_panics() {
        let toy = ToyMachine::new(false);
        let mut state = empty(&toy);
        let _ = toy.apply(&mut state, &ToyCommand::Panic, &ctx(2, 1, 1));
    }

    #[test]
    fn trial_apply_leaves_the_state_unchanged() {
        let toy = ToyMachine::new(false);
        let state = loaded(&toy, &hex(SAMPLE));
        let applied = ok(toy.trial_apply(&state, &ToyCommand::Set { key: 7, value: 1 }, &ctx(10, 2, 1)));
        assert_eq!(applied.notices.len(), 1);
        assert_eq!(toy.canonical_bytes(&state), hex(SAMPLE));
    }

    // ---------- other methods

    #[test]
    fn supported_rules_hash_versions_and_upgrade() {
        let toy = ToyMachine::new(false);
        let toy2 = ToyMachine::new(true);
        assert_eq!(toy.supported_rules(), &[1]);
        assert_eq!(toy2.supported_rules(), &[1, 2]);
        assert_eq!(toy.hash_versions(), &[1]);
        assert_eq!(toy2.hash_versions(), &[1]);

        let mut state = empty(&toy);
        for v in [0, 1, 2, 3] {
            assert!(matches!(toy.upgrade(&mut state, v), Err(SmError { code: "engine/toy_unknown_rules" })));
            assert_eq!(state.rules, 1);
        }
        let mut state = empty(&toy2);
        assert!(matches!(toy2.upgrade(&mut state, 3), Err(SmError { code: "engine/toy_unknown_rules" })));
        assert!(toy2.upgrade(&mut state, 2).is_ok());
        assert_eq!(state.rules, 2);
        assert_eq!(toy2.canonical_bytes(&state), hex("544f5931 00000002 0000 00000000 0000"));
        assert!(matches!(toy2.upgrade(&mut state, 2), Err(SmError { code: "engine/toy_unknown_rules" })));
    }

    #[test]
    fn reset_keeps_the_target_with_the_current_rules_and_the_larger_marks() {
        let toy = ToyMachine::new(true);
        let target = loaded(&toy, &state_bytes(1, &[(1, 10, 2, 1)], &[(id(1, 3), 5)], &[(1, 3), (2, 9)]));
        let current = loaded(&toy, &state_bytes(2, &[(4, 1, 8, 2)], &[(id(1, 7), 1)], &[(1, 7), (3, 2)]));
        let Ok(out) = toy.reset(&target, &current) else { panic!("reset failed") };
        assert_eq!(
            toy.canonical_bytes(&out),
            toy.canonical_bytes(&loaded(
                &toy,
                &state_bytes(2, &[(1, 10, 2, 1)], &[(id(1, 3), 5)], &[(1, 7), (2, 9), (3, 2)])
            ))
        );
    }

    #[test]
    fn hold_and_coalesce() {
        let toy = ToyMachine::new(false);
        let item = id(1, 1);
        let create = ToyCommand::Create { id: item, name: 0 };
        assert_eq!(toy.hold(&create), Some(item));
        for (command, _, _, _) in command_table().into_iter().filter(|c| !matches!(c.0, ToyCommand::Create { .. })) {
            assert_eq!(toy.hold(&command), None);
        }
        assert!(matches!(
            toy.coalesce(&create, &ToyCommand::Name { id: item, name: 4 }),
            Coalesce::Merge(ToyCommand::Create { id, name: 4 }) if id == item
        ));
        assert!(matches!(toy.coalesce(&create, &ToyCommand::Forget { id: item }), Coalesce::CancelBoth));
        assert!(matches!(toy.coalesce(&create, &ToyCommand::Name { id: id(1, 2), name: 4 }), Coalesce::Keep));
        assert!(matches!(toy.coalesce(&create, &ToyCommand::Forget { id: id(1, 2) }), Coalesce::Keep));
        assert!(matches!(toy.coalesce(&create, &ToyCommand::Set { key: 1, value: 1 }), Coalesce::Keep));
        let not_held = ToyCommand::Set { key: 1, value: 1 };
        assert!(matches!(toy.coalesce(&not_held, &ToyCommand::Forget { id: item }), Coalesce::Keep));
    }

    #[test]
    fn rebind_mints_anew_for_the_new_device_and_rewrites_later_references() {
        let toy = ToyMachine::new(false);
        // The new identity is device 5; its pending view holds device 5's mark 4.
        let view = loaded(&toy, &state_bytes(1, &[], &[(id(5, 4), 0)], &[(5, 4)]));
        let commands = [
            ToyCommand::Create { id: id(2, 1), name: 7 },
            ToyCommand::Name { id: id(2, 1), name: 8 },
            ToyCommand::Set { key: 1, value: 1 },
            ToyCommand::Create { id: id(2, 2), name: 9 },
            ToyCommand::Forget { id: id(2, 2) },
            ToyCommand::Create { id: id(3, 1), name: 1 },
            ToyCommand::Name { id: id(3, 1), name: 2 },
            ToyCommand::Forget { id: id(2, 9) },
        ];
        let Ok((out, origin)) = toy.rebind(&commands, 2, 5, &view, &hex("0000000000000002")) else {
            panic!("rebind failed")
        };
        assert_eq!(
            out,
            vec![
                ToyCommand::Create { id: id(5, 5), name: 7 },
                ToyCommand::Name { id: id(5, 5), name: 8 },
                ToyCommand::Set { key: 1, value: 1 },
                ToyCommand::Create { id: id(5, 6), name: 9 },
                ToyCommand::Forget { id: id(5, 6) },
                ToyCommand::Create { id: id(3, 1), name: 1 },
                ToyCommand::Name { id: id(3, 1), name: 2 },
                ToyCommand::Forget { id: id(2, 9) },
            ]
        );
        assert_eq!(origin, hex("0000000000000007"));
        assert_eq!(toy.encode(&out[0]), hex("04 0000050000000005 00000007"));

        // A minter ahead of the mark wins.
        let Ok((out, origin)) = toy.rebind(&commands[..1], 2, 5, &view, &hex("0000000000000010")) else {
            panic!("rebind failed")
        };
        assert_eq!(out, vec![ToyCommand::Create { id: id(5, 16), name: 7 }]);
        assert_eq!(origin, hex("0000000000000011"));

        // No create: the origin state comes back unchanged.
        let Ok((out, origin)) = toy.rebind(&commands[2..3], 2, 5, &view, &hex("0000000000000003")) else {
            panic!("rebind failed")
        };
        assert_eq!(out, vec![ToyCommand::Set { key: 1, value: 1 }]);
        assert_eq!(origin, hex("0000000000000003"));
    }

    #[test]
    fn rebind_refuses_a_bad_origin_state_and_an_id_out_of_range() {
        let toy = ToyMachine::new(false);
        let view = empty(&toy);
        let create = [ToyCommand::Create { id: id(2, 1), name: 7 }];
        assert!(matches!(toy.rebind(&create, 2, 5, &view, &[0; 7]), Err(SmError { code: "engine/toy_format" })));
        assert!(matches!(
            toy.rebind(&create, 2, 5, &view, &ID_SPLIT.to_be_bytes()),
            Err(SmError { code: "engine/toy_bad_id" })
        ));
    }

    #[test]
    fn a_notice_concerns_its_author_and_the_other_device() {
        let toy = ToyMachine::new(false);
        let notice = ToyNotice { seq: 10, author: 2, other: 3, other_seq: 5, key: 7 };
        assert!(toy.notice_concerns(&notice, 2));
        assert!(toy.notice_concerns(&notice, 3));
        assert!(!toy.notice_concerns(&notice, 1));
    }

    #[test]
    fn equal_inputs_give_equal_bytes() {
        let script = [
            ToyCommand::Set { key: 3, value: 10 },
            ToyCommand::Create { id: id(1, 1), name: 1 },
            ToyCommand::Add { key: 3, delta: -4 },
            ToyCommand::Set { key: 3, value: 2 },
            ToyCommand::Name { id: id(1, 1), name: 2 },
            ToyCommand::Fail { code: 1 },
            ToyCommand::Mul { key: 3, factor: 5 },
            ToyCommand::Create { id: id(2, 1), name: 3 },
            ToyCommand::Del { key: 3 },
            ToyCommand::Forget { id: id(1, 1) },
        ];
        let run = || {
            let toy = ToyMachine::new(true);
            let mut state = empty(&toy);
            let mut trace = Vec::new();
            for (i, command) in script.iter().enumerate() {
                let seq = 2 + i as u64;
                let device_no = 1 + (i % 2) as u16;
                if i == 6 && toy.upgrade(&mut state, 2).is_err() {
                    panic!("upgrade refused");
                }
                match toy.apply(&mut state, command, &ctx(seq, device_no, 1)) {
                    Ok(applied) => {
                        for notice in &applied.notices {
                            trace.extend(toy.notice_bytes(notice));
                        }
                    }
                    Err(refusal) => trace.extend(toy.refusal_bytes(seq, device_no, &refusal)),
                }
                trace.extend(toy.canonical_bytes(&state));
            }
            trace
        };
        let first = run();
        assert_eq!(first, run());
        assert!(!first.is_empty());
    }
}
