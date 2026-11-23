//! Receipt checks (seal.md §14, §26).
//!
//! `classify` runs the seventeen steps of §14.2 in order, each failure with
//! the class and code of its row, on task S2's receive-side functions (the
//! table at the top of `entries.rs`), and the transitions of §13.6 on a
//! working copy (`membership::transitions`). It is a function of its inputs
//! alone (SEAL-RCPT-006): it reads no global state, iterates no hash map and
//! draws no entropy, and it never changes its input state.
//!
//! What it does when its own caller breaks the contract of §14.1:
//! - seq 0: no author exists at seq 0 (§13.4 starts at seq 1), so a frame
//!   that passes steps 1 and 2 is `Skip` with `seal/unknown_author`;
//! - seq 1 without a genesis anchor: the receiver has no board, so step 2
//!   fails (`Skip`, `seal/wrong_board`), or step 3 (`seal/unknown_author`)
//!   for a frame whose `board_id` is all zero; the state returned is `M_0`;
//! - seq 1 with any `membership`: it is ignored; seq 1 is classified
//!   against the empty state `M_0` (§13.3);
//! - an anchor at another seq: ignored;
//! - no root for an epoch step 9 accepted, or a root that does not match
//!   the commitment the state records for that epoch (SEAL-KEY-013): the
//!   local error `seal/missing_root` as `Err`, the only `Err` it returns;
//! - `chain_at` returning nothing for a `b` at or above `retained_from`:
//!   the comparison is skipped with `based_on_unchecked` set.

use subtle::ConstantTimeEq;

use crate::entries::{envelope_header, open_envelope, unpad, verify_frame_signature};
use crate::membership::transitions::{self, Auth, CtlOutcome, EntryCx, envelope_allowed};
use crate::payloads::{self, BeatContext, CtlContext, Shape, SnapContext, wrap_recipient};
use crate::{
    Beat, BoardId, Chain, CtlItem, DevId, DeviceKeys, EntryId, EpochRoot, FrameHash, GenesisAnchor, Head, KeyKind,
    Kind, Membership, Recipient, RecoveryPublic, SealError, SnapAnnounce, board_id, chain_step, chain_zero,
    derive_epoch_key, frame_hash, frame_header, unwrap_root,
};

/// The stale-epoch window of §13.9 (§23).
pub(crate) const STALE_WINDOW_MS: i64 = 2_592_000_000;

/// The inputs of one classification (seal.md §14.1, §26).
pub struct ReceiptInput<'a> {
    pub seq: u64,
    pub frame: &'a [u8],
    pub prev_chain: Chain,
    /// `M_{s−1}`.
    pub membership: &'a Membership,
    pub roots: &'a dyn Fn(u32) -> Option<&'a EpochRoot>,
    /// `C_b` for `b ≥ retained_from`.
    pub chain_at: &'a dyn Fn(u64) -> Option<Chain>,
    pub retained_from: u64,
    /// `None` for read-only readers.
    pub own: Option<OwnIdentity<'a>>,
    pub supported_rules: &'a [u32],
    /// `Some` only for seq 1.
    pub genesis: Option<&'a GenesisAnchor>,
    pub skip_targets: &'a dyn Fn(u64) -> Option<&'a [u8]>,
}

/// This device's keys and its own frames' hashes (seal.md §14.1, §14.7, §26).
pub struct OwnIdentity<'a> {
    pub keys: &'a DeviceKeys,
    pub sealed: &'a dyn Fn(u64) -> Option<FrameHash>,
}

/// A receipt class (seal.md §14.4, §26).
pub enum Class {
    Skip,
    Tamper,
    Refuse,
    Accept,
    NeedFrames(Vec<(u64, FrameHash)>),
}

/// A halt (seal.md §14.5, §26).
pub enum Halt {
    Commitment,
    UnknownRules(u32),
}

/// The result of a classification (seal.md §14, §26).
///
/// `code` is the code of the failing step for `Skip`, `Tamper` and
/// `Refuse`; for `Accept` it is the halt's code (`seal/commitment`,
/// `seal/unknown_rules`) when `halt` is set and `None` otherwise; for
/// `NeedFrames` it is `None`. `opened` is set from step 11 on, whatever the
/// class (§14.4: `Refuse` returns `entry_id` "if the entry opened"; `Tamper`
/// its `based_on`).
pub struct Receipt {
    pub class: Class,
    pub code: Option<&'static str>,
    /// `C_s`.
    pub chain: Chain,
    pub own_dev_foreign: bool,
    pub based_on_unchecked: bool,
    pub opened: Option<Opened>,
    /// `M_s` (equal to `M_{s−1}` when unchanged).
    pub membership: Membership,
    pub learned_root: Option<(u32, EpochRoot)>,
    pub halt: Option<Halt>,
}

/// An opened entry (seal.md §14.2, §26).
pub struct Opened {
    pub entry_id: EntryId,
    pub kind: Kind,
    pub at_ms: i64,
    pub based_on: Head,
    pub payload: Vec<u8>,
    pub items: Option<Vec<CtlItem>>,
    pub beat: Option<Beat>,
    pub snap: Option<SnapAnnounce>,
}

/// An author's class at a position (seal.md §13.4).
#[derive(Clone, Copy, PartialEq, Eq)]
enum AuthorClass {
    CurrentDevice,
    CurrentRecovery,
    /// A revoked device, a retired recovery key, or one not yet in force.
    NonAuthority,
}

/// The author of a frame as `M_{s−1}` (or the anchor) records it.
#[derive(Clone, Copy)]
struct Author {
    dev: DevId,
    sign_pk: [u8; 32],
    last_ctr: u64,
    class: AuthorClass,
}

/// What the steps found before their verdict, for the bookkeeping.
struct Progress {
    /// The author and its counter, once step 5 passed.
    counted: Option<(DevId, u64)>,
    own_dev_foreign: bool,
    based_on_unchecked: bool,
    opened: Option<Opened>,
}

enum Verdict {
    Skip(SealError),
    Tamper(SealError),
    Refuse(SealError),
    NeedFrames(Vec<(u64, FrameHash)>),
    Accept { next: Box<Membership>, learned: Option<(u32, EpochRoot)>, halt: Option<Halt> },
}

/// Classifies one sequenced frame (seal.md §14.2, §26).
pub fn classify(input: &ReceiptInput) -> Result<Receipt, SealError> {
    let s = input.seq;
    let chain = chain_step(&input.prev_chain, s, input.frame);
    let empty;
    let (prev, anchor) = if s == 1 {
        empty = Membership::empty();
        (&empty, input.genesis)
    } else {
        (input.membership, None)
    };
    let mut p = Progress { counted: None, own_dev_foreign: false, based_on_unchecked: false, opened: None };
    let verdict = steps(input, prev, anchor, &mut p)?;
    let (class, code, membership, learned_root, halt) = match verdict {
        Verdict::Skip(e) => (Class::Skip, Some(e.code()), prev.dup(), None, None),
        Verdict::Tamper(e) => (Class::Tamper, Some(e.code()), prev.dup(), None, None),
        Verdict::NeedFrames(v) => (Class::NeedFrames(v), None, prev.dup(), None, None),
        Verdict::Refuse(e) => {
            // §13.5: a refused entry consumes its author's counter (SEAL-MEM-009).
            // SPEC-QUESTION: at seq 1 the author is the anchor's recovery
            // identity, which `M_0` does not record; there is no counter to
            // set, so a refused seq 1 leaves `M_1 = M_0`.
            let mut m = prev.dup();
            if let Some((dev, ctr)) = p.counted {
                m.set_last_ctr(&dev, ctr);
            }
            (Class::Refuse, Some(e.code()), m, None, None)
        }
        Verdict::Accept { next, learned, halt } => {
            let code = halt.as_ref().map(|h| match h {
                Halt::Commitment => SealError::Commitment.code(),
                Halt::UnknownRules(_) => SealError::UnknownRules.code(),
            });
            (Class::Accept, code, *next, learned, halt)
        }
    };
    Ok(Receipt {
        class,
        code,
        chain,
        own_dev_foreign: p.own_dev_foreign,
        based_on_unchecked: p.based_on_unchecked,
        opened: p.opened,
        membership,
        learned_root,
        halt,
    })
}

/// Step 3: the author's record at seq `s` (§13.4, SEAL-MEM-007).
fn find_author(s: u64, prev: &Membership, anchor: Option<&GenesisAnchor>, board: &BoardId, dev: &DevId) -> Option<Author> {
    if s == 0 {
        return None;
    }
    if s == 1 {
        let a = anchor?;
        let dev_r = RecoveryPublic { sign_pk: a.rsign_pk, hpke_pk: [0; 32] }.dev(board);
        return (dev_r == *dev).then_some(Author {
            dev: dev_r,
            sign_pk: a.rsign_pk,
            last_ctr: 0,
            class: AuthorClass::CurrentRecovery,
        });
    }
    if let Some(d) = prev.device(dev) {
        let class = if d.is_current() { AuthorClass::CurrentDevice } else { AuthorClass::NonAuthority };
        return Some(Author { dev: d.dev, sign_pk: d.sign_pk, last_ctr: d.last_ctr, class });
    }
    let r = prev.recovery_record(dev)?;
    let class =
        if r.until_seq == 0 && r.since_seq < s { AuthorClass::CurrentRecovery } else { AuthorClass::NonAuthority };
    Some(Author { dev: r.dev, sign_pk: r.sign_pk, last_ctr: r.last_ctr, class })
}

/// The steps of §14.2 in order (SEAL-RCPT-001).
fn steps(
    input: &ReceiptInput<'_>,
    prev: &Membership,
    anchor: Option<&GenesisAnchor>,
    p: &mut Progress,
) -> Result<Verdict, SealError> {
    let s = input.seq;
    let frame = input.frame;

    // 1. Layout (SEAL-FRAME-003).
    let hdr = match frame_header(frame) {
        Ok(h) => h,
        Err(e) => return Ok(Verdict::Skip(e)),
    };
    // 2. The receiver's board: the anchor's at seq 1, else `M_{s−1}`'s.
    let board = match anchor {
        Some(a) => board_id(&a.rsign_pk, &a.salt),
        None => prev.board_id,
    };
    if hdr.board != board {
        return Ok(Verdict::Skip(SealError::WrongBoard));
    }
    // 3. A key of this board.
    let Some(author) = find_author(s, prev, anchor, &board, &hdr.dev) else {
        return Ok(Verdict::Skip(SealError::UnknownAuthor));
    };
    // 4. Strict Ed25519 (SEAL-GEN-010).
    if let Err(e) = verify_frame_signature(frame, &author.sign_pk) {
        return Ok(Verdict::Skip(e));
    }
    // 5. The counter.
    if author.last_ctr.checked_add(1) != Some(hdr.ctr) {
        let e = if hdr.ctr <= author.last_ctr { SealError::CounterReplay } else { SealError::CounterGap };
        return Ok(Verdict::Tamper(e));
    }
    p.counted = Some((author.dev, hdr.ctr));
    // 6. Own-dev check (§14.7, SEAL-RCPT-010).
    if let Some(own) = &input.own
        && own.keys.public().dev == hdr.dev
    {
        let fh = frame_hash(frame);
        let known = (own.sealed)(hdr.ctr).is_some_and(|h| bool::from(h.0.ct_eq(&fh.0)));
        p.own_dev_foreign = !known;
    }
    // 7. A current authority.
    if author.class == AuthorClass::NonAuthority {
        return Ok(Verdict::Refuse(SealError::RevokedAuthor));
    }
    // 8. (`ver`, `suite`) under the active rules (§14.3); rules 1 at seq 1.
    let env = match envelope_header(frame) {
        Ok(e) => e,
        Err(e) => return Ok(Verdict::Skip(e)),
    };
    let rules = if s == 1 { 1 } else { prev.rules };
    if !envelope_allowed(rules, env.ver, env.suite) {
        return Ok(Verdict::Refuse(SealError::BadVersion));
    }
    // 9. The epochs this position accepts (§9.3, SEAL-ENV-005).
    let stale = if s == 1 {
        if env.epoch != 1 {
            return Ok(Verdict::Refuse(SealError::BadEpoch));
        }
        false
    } else {
        let e = prev.epoch;
        if env.epoch == e {
            false
        } else if e >= 2 && Some(env.epoch) == e.checked_sub(1) {
            // Condition 4 of §13.9 (30 days of log clock since the rotation)
            // is checked here, before the root of `E − 1` is asked for and
            // before decryption (spec change E3); conditions 1 to 3 follow
            // at steps 13 and 14.
            let window = prev.clock_ms.checked_sub(prev.epoch_clock_ms).is_some_and(|d| d <= STALE_WINDOW_MS);
            if !window {
                return Ok(Verdict::Refuse(SealError::StaleEpoch));
            }
            true
        } else if env.epoch > e {
            return Ok(Verdict::Refuse(SealError::BadEpoch));
        } else {
            return Ok(Verdict::Refuse(SealError::OldEpoch));
        }
    };
    // The root: the anchor's `R_1` at seq 1 (checked against Genesis's
    // `commit` at step 16, SEAL-MEM-005); else the caller's, checked against
    // the commitment the state records before any key is derived from it
    // (SEAL-KEY-013). A missing or mismatching root is the receiver's fault.
    // SPEC-QUESTION: §14.2 names `seal/missing_root` for a root the receiver
    // does not hold; a held root that does not match `c_e` is a fault of the
    // same store (opening with it would refuse a good frame with
    // `seal/aead` on this device alone), so it gets the same local error.
    let root: &EpochRoot = match anchor {
        Some(a) => &a.root1,
        None => {
            let root = (input.roots)(env.epoch).ok_or(SealError::MissingRoot)?;
            let recorded = prev.commitment(env.epoch).ok_or(SealError::MissingRoot)?;
            if !bool::from(root.commitment(&board, env.epoch).ct_eq(recorded)) {
                return Err(SealError::MissingRoot);
            }
            root
        }
    };
    // 10. AEAD.
    let key = derive_epoch_key(root, &board, env.epoch, KeyKind::Entry);
    let pt = match open_envelope(frame, &key) {
        Ok(pt) => pt,
        Err(_) => return Ok(Verdict::Refuse(SealError::Aead)),
    };
    // 11. Unpadding (SEAL-PAD-003).
    let plain = match unpad(&pt) {
        Ok(pl) => pl,
        Err(_) => return Ok(Verdict::Refuse(SealError::BadPlaintext)),
    };
    let (kind, at_ms, based_on) = (plain.kind, plain.at_ms, plain.based_on);
    p.opened = Some(Opened {
        entry_id: plain.entry_id,
        kind,
        at_ms,
        based_on,
        payload: plain.payload,
        items: None,
        beat: None,
        snap: None,
    });
    // 12. `based_on` (§14.8).
    let b = based_on.seq;
    if b >= s {
        return Ok(Verdict::Tamper(SealError::BasedOnFuture));
    } else if b == 0 {
        if based_on.chain != chain_zero(&board) {
            return Ok(Verdict::Tamper(SealError::BasedOnFork));
        }
    } else if b < input.retained_from {
        p.based_on_unchecked = true;
    } else {
        // SPEC-QUESTION: §14.1 has the caller supply `C_b` for every `b` from
        // `retained_from` to `s − 1`; when it supplies none, the receiver does
        // not hold `C_b`, and this takes §14.8 case 3 (skip, flag) rather
        // than treat its own store's gap as the author's fork.
        match (input.chain_at)(b) {
            Some(c) if c != based_on.chain => return Ok(Verdict::Tamper(SealError::BasedOnFork)),
            Some(_) => {}
            None => p.based_on_unchecked = true,
        }
    }
    // 13. Author and epoch for the kind (§11.1, SEAL-KIND-001, SEAL-MEM-026).
    let by_recovery = author.class == AuthorClass::CurrentRecovery;
    match kind {
        Kind::Ctl => {
            if stale {
                return Ok(Verdict::Refuse(SealError::StaleEpoch));
            }
        }
        Kind::Snap => {
            if by_recovery {
                return Ok(Verdict::Refuse(SealError::NotAuthorized));
            }
            if stale {
                return Ok(Verdict::Refuse(SealError::StaleEpoch));
            }
        }
        Kind::Cmd | Kind::Beat => {
            if by_recovery {
                return Ok(Verdict::Refuse(SealError::NotAuthorized));
            }
        }
    }
    // 14. Conditions 1 to 3 of the stale-epoch rule (§13.9, SEAL-MEM-018;
    // condition 4 was step 9's); the author is a current device here (step
    // 13 refused the recovery identity's cmd and beat).
    if stale && (author.class != AuthorClass::CurrentDevice || b >= prev.epoch_seq) {
        return Ok(Verdict::Refuse(SealError::StaleEpoch));
    }
    let tau = prev.clock_ms.max(at_ms);
    let payload: &[u8] = p.opened.as_ref().map(|o| o.payload.as_slice()).unwrap_or(&[]);

    // 15 and 16, per kind; then step 17 with the bookkeeping of §13.5.
    match kind {
        Kind::Cmd => Ok(accept_plain(prev, &author, hdr.ctr, tau, None)),
        Kind::Snap => match payloads::parse_snap(payload, SnapContext { based_on_seq: b, epoch: env.epoch }) {
            Err(e) => Ok(Verdict::Refuse(e)),
            Ok(snap) => {
                if let Some(o) = p.opened.as_mut() {
                    o.snap = Some(snap);
                }
                Ok(accept_plain(prev, &author, hdr.ctr, tau, None))
            }
        },
        Kind::Beat => match payloads::parse_beat(payload, BeatContext { seq: s, based_on_seq: b }) {
            Err(e) => Ok(Verdict::Refuse(e)),
            Ok(beat) => {
                let v = accept_plain(prev, &author, hdr.ctr, tau, Some((&beat, at_ms)));
                if let Some(o) = p.opened.as_mut() {
                    o.beat = Some(beat);
                }
                Ok(v)
            }
        },
        Kind::Ctl => {
            let cx = CtlContext {
                board: &board,
                dev: &hdr.dev,
                ctr: hdr.ctr,
                entry_id: &plain.entry_id,
                at_ms,
                sign_pk: &author.sign_pk,
            };
            let items = match payloads::parse_ctl(payload, &cx) {
                Ok(items) => items,
                Err(e) => return Ok(Verdict::Refuse(e)),
            };
            let v = control(input, prev, anchor, &board, &author, hdr.ctr, at_ms, tau, &items);
            if let Some(o) = p.opened.as_mut() {
                o.items = Some(items);
            }
            Ok(v)
        }
    }
}

/// `Accept` of a `cmd`, `beat` or `snap` (§13.5): the counter, the clock,
/// and for a beat its fields (§13.10).
fn accept_plain(prev: &Membership, author: &Author, ctr: u64, tau: i64, beat: Option<(&Beat, i64)>) -> Verdict {
    let mut next = prev.dup();
    next.set_last_ctr(&author.dev, ctr);
    next.clock_ms = tau;
    if let Some((beat, at_ms)) = beat
        && let Some(d) = next.device_mut(&author.dev)
    {
        d.beat_ms = d.beat_ms.max(at_ms);
        d.rules = beat.rules.clone();
        d.envelopes = beat.envelopes.clone();
        d.hash_versions = beat.hash_versions.clone();
    }
    Verdict::Accept { next: Box::new(next), learned: None, halt: None }
}

/// Step 16 for a control entry, then step 17 (§12.1, §13.3, §13.6, §14.5,
/// §14.6).
#[allow(clippy::too_many_arguments)]
fn control(
    input: &ReceiptInput<'_>,
    prev: &Membership,
    anchor: Option<&GenesisAnchor>,
    board: &BoardId,
    author: &Author,
    ctr: u64,
    at_ms: i64,
    tau: i64,
    items: &[CtlItem],
) -> Verdict {
    let s = input.seq;
    let shape = match payloads::ctl_shape(items) {
        Ok(sh) => sh,
        Err(e) => return Verdict::Refuse(e),
    };
    // SEAL-CTL-002 and §12.1's note on G: Genesis only at seq 1, and seq 1
    // holds only Genesis.
    // SPEC-QUESTION: §12.1 refuses "any other sequence" with
    // `seal/ctl_shape` and states the seq rule in its G row; §22 describes
    // `seal/ctl_genesis` as "a Genesis item or anchor is wrong". A Genesis
    // at another seq, or another shape at seq 1, is a shape that cannot
    // stand at that seq: this takes `seal/ctl_shape`.
    if (shape == Shape::G) != (s == 1) {
        return Verdict::Refuse(SealError::CtlShape);
    }
    if let (Some(a), [CtlItem::Genesis(g)]) = (anchor, items) {
        return genesis(a, board, author, at_ms, g);
    }
    if s == 1 {
        // Seq 1 without an anchor cannot reach this step (step 3).
        return Verdict::Refuse(SealError::CtlGenesis);
    }

    let auth = match author.class {
        AuthorClass::CurrentDevice => Auth::Device(author.dev),
        AuthorClass::CurrentRecovery => Auth::Recovery(author.dev),
        AuthorClass::NonAuthority => return Verdict::Refuse(SealError::RevokedAuthor),
    };
    let cx = EntryCx { s, at_ms, tau, author: auth, board: *board, skip_targets: input.skip_targets };
    let mut w = prev.dup();
    let activated = match transitions::apply(&mut w, shape, items, &cx) {
        CtlOutcome::Refused(e) => return Verdict::Refuse(e),
        CtlOutcome::NeedFrames(v) => return Verdict::NeedFrames(v),
        CtlOutcome::Applied { activated } => activated,
    };
    // §13.5: `s` is appended to `ctl_seqs`, within its limit of §23.
    // SPEC-QUESTION: §13.6 gives `seal/ctl_cap` to an item whose effect
    // would overfill a list; `ctl_seqs` grows by the entry's bookkeeping,
    // not by an item. A full `ctl_seqs` refuses the entry with the same code.
    if w.ctl_seqs.len() >= crate::membership::CTL_SEQS_MAX {
        return Verdict::Refuse(SealError::CtlCap);
    }
    w.set_last_ctr(&author.dev, ctr);
    w.clock_ms = tau;
    w.ctl_seqs.push(s);

    // 17. The halts (§14.5).
    let mut halt = None;
    let mut learned = None;
    if let Some(CtlItem::EpochRotated(r)) = items.iter().find(|i| matches!(i, CtlItem::EpochRotated(_)))
        && let Some(own) = &input.own
    {
        let me = own.keys.public().dev;
        if let Some(wrap) = r.wraps.iter().find(|w| wrap_recipient(w) == me.0) {
            // SEAL-RCPT-007, SEAL-WRAP-003: open the own wrap and compare
            // with `commit`; `unwrap_root` does both.
            match unwrap_root(wrap, board, r.epoch, &r.commit, &Recipient::Device(own.keys)) {
                Ok(root) => learned = Some((r.epoch, root)),
                Err(_) => halt = Some(Halt::Commitment),
            }
        }
    }
    if let Some(v) = activated
        && !input.supported_rules.contains(&v)
    {
        halt = Some(Halt::UnknownRules(v));
    }
    Verdict::Accept { next: Box::new(w), learned, halt }
}

/// The entry at seq 1 (§13.3; SEAL-MEM-004, SEAL-MEM-005, SEAL-MEM-006,
/// SEAL-KEY-017). Steps 1 to 15 already checked `dev = dev_R`, `ctr = 1`,
/// the signature under `rsign_pk`, `board_id` against the anchor, epoch 1,
/// the AEAD under `R_1` and `based_on = (0, C_0)`.
fn genesis(a: &GenesisAnchor, board: &BoardId, author: &Author, at_ms: i64, g: &crate::Genesis) -> Verdict {
    let dev_r = author.dev;
    let ok = g.salt == a.salt
        && g.recovery.sign_pk == a.rsign_pk
        && board_id(&g.recovery.sign_pk, &g.salt) == *board
        && g.device.dev != dev_r
        && bool::from(a.root1.commitment(board, 1).ct_eq(&g.commit));
    let mut want = [g.device.dev.0, dev_r.0];
    want.sort_unstable();
    let got: Vec<[u8; 16]> = g.wraps.iter().map(wrap_recipient).collect();
    if !ok || got != want {
        return Verdict::Refuse(SealError::CtlGenesis);
    }
    Verdict::Accept { next: Box::new(Membership::genesis(*board, g, dev_r, at_ms)), learned: None, halt: None }
}

#[cfg(test)]
pub(crate) mod tests;
