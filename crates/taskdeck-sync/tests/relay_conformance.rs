//! The model relay (`taskdeck_sync::sim::relay`, engine.md §31.3) against the
//! relay's published conformance cases (`relay.md` §21; the files under
//! `relay/test/vectors/`, format in their `README.md`). spec: ENG-SIM-001.
//!
//! The cases are symbolic. This test expands them as the relay's harness
//! (`relay/test/lib/harness.mjs`) does, with the fake of engine.md §6.3 in
//! place of Ed25519: a key's `sign_pk` is `H("FAKE-PK" ‖ seed)`, a signature
//! the fake one, a board id `H("FAKE-BOARD" ‖ rsign_pk ‖ salt)[0..16]`, a
//! recovery key's `dev` the fake `dev_R`, any other key's `dev`
//! `H("taskdeck-test-dev" ‖ sign_pk)[0..16]` (the harness's rule). It then
//! parses each request body by its layout into a call of the model, and
//! checks the case's expectations against the model's answer and tables.
//!
//! **Families that apply** (every case of each): `relay-append.json` (§9),
//! `relay-read.json` (§10.4, §10.5), `relay-blobs.json` (§10.7, §13),
//! `relay-budget.json` (§14.1), `relay-compact.json` (§12.8, §13.4),
//! `relay-create.json` (§10.2), `relay-admin.json` (§12.3 to §12.6, §12.12),
//! `relay-seed.json` (§12.7), `relay-restore.json` (§12.9),
//! `relay-purge.json` (§12.10), `relay-socket.json` (§11.3, §11.4).
//!
//! **Families left out**, because the model has no part they test:
//! `relay-gate.json` (the gate, §6), `relay-token.json` (token bytes and MACs,
//! §5.1), `relay-address.json` (address text, §5.4, pure functions),
//! `relay-login.json` (login nonces and signatures, §10.3),
//! `relay-pairing.json` (tickets and slots, §10.8, not in §31.3),
//! `relay-rows.json` and `relay-day.json` (rows written per request, §16:
//! a property of SQLite storage), `relay-standby.json` (the standby, §15.2).
//!
//! **Steps left out inside the families that apply.** A step is left out on
//! what it sends and what it expects, never on what the model answers:
//! - routes the model does not have: `admin-nonce`, `login-nonce`, `caps`,
//!   `door`, the pairing routes, `mint_read_token`, `revoke_read_token`
//!   (nonces, tickets and read tokens are not modelled, §31.3);
//! - credentials the gate decides: a backup-read token, a token or door of
//!   another board, a wrong create secret, no or a raw credential;
//! - a step whose expectation says the gate answered (`x_td_relay: "gate"`
//!   with a refusal, or `object_called: false`);
//! - an admin request whose signature or nonce is what the step tests (a
//!   flipped signature, another signing key, context, op or args; codes
//!   `relay/no_nonce`, `relay/bad_admin_sig`), and a login whose nonce,
//!   generation or signature it tests (`relay/no_nonce`, `relay/bad_gen`,
//!   `relay/bad_login_sig`); a login that succeeds is run (its effect on the
//!   generation and on sockets is modelled);
//! - a body or socket message whose bytes do not parse by their layout (the
//!   model takes values, not bytes: `relay/bad_body`, `relay/bad_batch`,
//!   `relay/bad_args` of A1, `relay/bad_message`, closes 4004 and 1009 for
//!   malformed messages), a query that is not two integers, and the text
//!   `ping` (answered by the runtime, §11.5);
//! - storage and entropy faults (`relay/storage_refused`,
//!   `relay/object_unavailable`): the model's storage cannot throw.
//!
//! Expectations not checked: `x_td_relay`, `rows_read_max`, `elapsed_ms` and
//! `rows_written` above 0 (storage costs); `rows_written: 0` is checked as
//! "the model's tables did not change" (RELAY-SEQ-005); `storage` is checked
//! for the tables the model has (board, devices, log, blobs, parts), not for
//! `nonces`, `tickets` or `read_tokens`; the `nonce`, `expires` and `token`
//! fields are not compared byte for byte (a token is a placeholder naming its
//! device and generation).

use std::collections::{BTreeMap, VecDeque};

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use taskdeck_sync::sim::relay::{
    AdminOp, BlobRow, BoardState, Cred, ModelRelay, Reply, SockMsg, SocketId, fake_board_id, fake_dev_r, fake_sign,
    fake_sign_pk,
};
use taskdeck_sync::{AdminOk, AppendResult, Dev, Hash32, Page, RelayRefusal, SeedDevice};

const FAMILIES: &[(&str, &str)] = &[
    ("relay-append.json", include_str!("../../../relay/test/vectors/relay-append.json")),
    ("relay-read.json", include_str!("../../../relay/test/vectors/relay-read.json")),
    ("relay-blobs.json", include_str!("../../../relay/test/vectors/relay-blobs.json")),
    ("relay-budget.json", include_str!("../../../relay/test/vectors/relay-budget.json")),
    ("relay-compact.json", include_str!("../../../relay/test/vectors/relay-compact.json")),
    ("relay-create.json", include_str!("../../../relay/test/vectors/relay-create.json")),
    ("relay-admin.json", include_str!("../../../relay/test/vectors/relay-admin.json")),
    ("relay-seed.json", include_str!("../../../relay/test/vectors/relay-seed.json")),
    ("relay-restore.json", include_str!("../../../relay/test/vectors/relay-restore.json")),
    ("relay-purge.json", include_str!("../../../relay/test/vectors/relay-purge.json")),
    ("relay-socket.json", include_str!("../../../relay/test/vectors/relay-socket.json")),
];

/// The harness's standard clock (`harness.mjs` `STD.clock_ms`).
const CLOCK: i64 = 1_795_000_000_000;
/// The model relay's stand-in for `TOKEN_KEY`.
const RELAY_KEY: Hash32 = [0x5a; 32];

// ---------- bytes

fn sha(parts: &[&[u8]]) -> Hash32 {
    let mut d = Sha256::new();
    for p in parts {
        d.update(p);
    }
    d.finalize().into()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn is_hex(s: &str) -> bool {
    s.len().is_multiple_of(2) && s.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn unhex(s: &str) -> Vec<u8> {
    assert!(is_hex(s), "not hex: {s}");
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex")).collect()
}

fn ctx(name: &str) -> Vec<u8> {
    let mut v = format!("taskdeck/v1/{name}").into_bytes();
    v.push(0);
    v
}

/// The harness's filler: `H(seed ‖ u32 0) ‖ H(seed ‖ u32 1) ‖ …` cut to `len`.
fn fill(len: usize, seed: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(len + 32);
    let mut i = 0u32;
    while out.len() < len {
        out.extend_from_slice(&sha(&[seed.as_bytes(), &i.to_be_bytes()]));
        i += 1;
    }
    out.truncate(len);
    out
}

fn chain_next(prev: &Hash32, seq: u64, fh: &Hash32) -> Hash32 {
    sha(&[&ctx("chain"), prev, &seq.to_be_bytes(), fh])
}

fn arr<const N: usize>(b: &[u8]) -> [u8; N] {
    b.try_into().unwrap_or_else(|_| panic!("{} bytes where {N} are needed", b.len()))
}

/// A cursor over a body, for the layouts of `relay.md`.
struct Rd<'a> {
    b: &'a [u8],
    o: usize,
}

impl<'a> Rd<'a> {
    fn new(b: &'a [u8]) -> Self {
        Rd { b, o: 0 }
    }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.o..self.o.checked_add(n)?)?;
        self.o += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|s| s[0])
    }
    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|s| u16::from_be_bytes(arr(s)))
    }
    fn u32(&mut self) -> Option<u32> {
        self.take(4).map(|s| u32::from_be_bytes(arr(s)))
    }
    fn u64(&mut self) -> Option<u64> {
        self.take(8).map(|s| u64::from_be_bytes(arr(s)))
    }
    fn arr<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N).map(arr)
    }
    fn left(&self) -> usize {
        self.b.len() - self.o
    }
    fn end(&self) -> Option<()> {
        (self.left() == 0).then_some(())
    }
}

// ---------- the world of one case

/// Why a step is left out (the header lists the rules).
#[derive(Debug)]
struct Skip(&'static str);

type Step<T> = Result<T, Skip>;

/// A model answer in the shape the expectations name.
enum Resp {
    Json(u16, Value),
    Results(u16, Vec<AppendResult>),
    Page(Page),
    Bytes(Vec<u8>),
}

struct Board {
    id: [u8; 16],
    salt: [u8; 16],
    recovery: String,
    first: String,
    relay: ModelRelay,
    hash_at: BTreeMap<u64, Hash32>,
    chain_at: BTreeMap<u64, Hash32>,
}

struct Sock {
    board: String,
    id: SocketId,
    queue: VecDeque<SockMsg>,
    /// A message the model could not take (malformed bytes) was sent: the
    /// real socket is closed, the rest of its expectations are left out.
    lost: bool,
}

#[derive(Default)]
struct Tally {
    run: usize,
    skipped: BTreeMap<&'static str, usize>,
}

struct World {
    clock: i64,
    keys: BTreeMap<String, ([u8; 32], Hash32)>,
    boards: BTreeMap<String, Board>,
    sockets: BTreeMap<String, Sock>,
    last: Value,
    saved: BTreeMap<String, Value>,
    skipped_mint: bool,
}

/// The token text this test puts where a relay answers a token.
fn token_text(role: u64, board: &[u8; 16], dev: &Dev, generation: u64) -> String {
    format!("TD1.fake.{role}.{}.{}.{generation}", hex(board), hex(dev))
}

fn status_name(r: &AppendResult) -> &'static str {
    match r {
        AppendResult::New { .. } => "new",
        AppendResult::Dup { .. } => "dup",
        AppendResult::Conflict { .. } => "conflict",
        AppendResult::Gap { .. } => "gap",
        AppendResult::Refused { .. } => "refused",
        AppendResult::Skipped => "skipped",
    }
}

/// The HTTP status of an append's results body (§9.6).
fn results_status(results: &[AppendResult]) -> u16 {
    for r in results {
        match r {
            AppendResult::Conflict { .. } | AppendResult::Gap { .. } => return 409,
            AppendResult::Refused { status, .. } => return *status,
            _ => {}
        }
    }
    200
}

/// The error body of §4.2 for a refusal.
fn error_json(r: &RelayRefusal, last: Option<(u64, u64)>) -> Value {
    let mut m = Map::new();
    m.insert("error".into(), json!(r.code));
    if let Some(u) = r.until {
        m.insert("until".into(), json!(u));
    }
    if let Some(i) = r.index {
        m.insert("index".into(), json!(i));
    }
    if let Some(h) = r.head {
        m.insert("head".into(), json!({"seq": h.seq, "chain": hex(&h.chain)}));
    }
    if let Some((c, s)) = last {
        m.insert("last_ctr".into(), json!(c));
        m.insert("last_seq".into(), json!(s));
    }
    Value::Object(m)
}

fn blob_json(b: &BlobRow) -> Value {
    json!({"id": hex(&b.id), "purpose": b.purpose, "seq": b.seq, "author": hex(&b.author), "size": b.size,
        "parts": b.parts, "sha256": hex(&b.sha256), "received": b.received, "complete": b.complete})
}

impl World {
    fn new(setup: &Value) -> Self {
        let mut w = World {
            clock: setup["clock_ms"].as_i64().unwrap_or(CLOCK),
            keys: BTreeMap::new(),
            boards: BTreeMap::new(),
            sockets: BTreeMap::new(),
            last: Value::Null,
            saved: BTreeMap::new(),
            skipped_mint: false,
        };
        for (name, k) in setup["keys"].as_object().into_iter().flatten() {
            let seed: [u8; 32] = arr(&unhex(k["seed"].as_str().expect("a seed")));
            w.keys.insert(name.clone(), (seed, fake_sign_pk(&seed)));
        }
        for (name, b) in setup["boards"].as_object().into_iter().flatten() {
            let salt: [u8; 16] = arr(&unhex(b["salt"].as_str().expect("a salt")));
            let recovery = b["recovery"].as_str().expect("a recovery key").to_string();
            let id = match b["id"].as_str() {
                Some(h) => arr(&unhex(h)),
                None => fake_board_id(&w.pk(&recovery), &salt),
            };
            let mut chain_at = BTreeMap::new();
            chain_at.insert(0, sha(&[&ctx("chain"), &id]));
            w.boards.insert(
                name.clone(),
                Board {
                    id,
                    salt,
                    recovery,
                    first: b["first"].as_str().expect("a first device").to_string(),
                    relay: ModelRelay::new(id, RELAY_KEY),
                    hash_at: BTreeMap::new(),
                    chain_at,
                },
            );
        }
        w
    }

    fn pk(&self, key: &str) -> Hash32 {
        self.keys.get(key).unwrap_or_else(|| panic!("key {key}")).1
    }

    fn board(&self, name: &str) -> &Board {
        self.boards.get(name).unwrap_or_else(|| panic!("board {name}"))
    }

    fn board_mut(&mut self, name: &str) -> &mut Board {
        self.boards.get_mut(name).unwrap_or_else(|| panic!("board {name}"))
    }

    /// The dev of a key on a board: `dev_R` for a recovery key.
    fn dev(&self, bn: &str, key: &str) -> Dev {
        if key.starts_with("rec") {
            fake_dev_r(&self.board(bn).id, &self.pk(key))
        } else {
            arr(&sha(&[b"taskdeck-test-dev", &self.pk(key)])[..16])
        }
    }

    /// `C_seq` from the frames this test saw sequenced (SEAL-CHAIN-001).
    fn chain(&mut self, bn: &str, seq: u64) -> Hash32 {
        let b = self.board_mut(bn);
        let (&base, &c0) = b.chain_at.range(..=seq).next_back().unwrap_or_else(|| panic!("no chain base below seq {seq}"));
        let mut c = c0;
        for i in base + 1..=seq {
            let fh = *b.hash_at.get(&i).unwrap_or_else(|| panic!("the test saw no frame at seq {i}"));
            c = chain_next(&c, i, &fh);
            b.chain_at.insert(i, c);
        }
        c
    }

    fn note(&mut self, bn: &str, seq: u64, frame: &[u8]) {
        self.board_mut(bn).hash_at.insert(seq, sha(&[frame]));
    }

    fn note_results(&mut self, bn: &str, results: &[AppendResult], frames: &[Vec<u8>]) {
        for (r, f) in results.iter().zip(frames) {
            if let AppendResult::New { seq, .. } | AppendResult::Dup { seq, .. } = r {
                self.note(bn, *seq, f);
            }
        }
    }

    // ----- builders (relay/test/vectors/README.md)

    /// A builder's value: bytes, a string or a number, as JSON (bytes as hex).
    fn val(&mut self, v: &Value, bn: &str) -> Value {
        match v {
            Value::String(s) if s.starts_with("$last.") => self.last[&s[6..]].clone(),
            Value::String(s) if s.starts_with("$saved.") => {
                // Saved values are nonces in these families: any 32 bytes do.
                self.saved.get(&s[7..]).cloned().unwrap_or_else(|| json!(hex(&[0; 32])))
            }
            Value::Object(o) if o.len() == 1 && o.keys().next().is_some_and(|k| k.starts_with('$')) => {
                let (k, a) = o.iter().next().expect("one key");
                match k.as_str() {
                    "$text" => a.clone(),
                    "$token" => {
                        let board = a["board"].as_str().unwrap_or(bn).to_string();
                        let dev = self.dev(&board, a["key"].as_str().expect("a key"));
                        let id = self.board(&board).id;
                        json!(token_text(a["role"].as_u64().unwrap_or(1), &id, &dev, a["gen"].as_u64().unwrap_or(1)))
                    }
                    "$door" => {
                        let b = if a == &Value::Bool(true) { bn.to_string() } else { a.as_str().expect("a board").to_string() };
                        json!(hex(&self.board(&b).relay.door()))
                    }
                    _ => json!(hex(&self.bytes(v, bn))),
                }
            }
            _ => v.clone(),
        }
    }

    fn num(&mut self, v: &Value, bn: &str) -> u64 {
        match self.val(v, bn) {
            Value::Number(n) => n.as_u64().expect("an unsigned integer"),
            Value::String(s) => s.parse().unwrap_or_else(|_| panic!("not a number: {s}")),
            x => panic!("not a number: {x}"),
        }
    }

    fn bytes(&mut self, v: &Value, bn: &str) -> Vec<u8> {
        let Value::Object(o) = v else {
            return match self.val(v, bn) {
                Value::String(s) if is_hex(&s) => unhex(&s),
                Value::String(s) => s.into_bytes(),
                x => panic!("not bytes: {x}"),
            };
        };
        let (k, a) = o.iter().next().expect("a builder");
        match k.as_str() {
            "$hex" => unhex(a.as_str().expect("hex")),
            "$text" => a.as_str().expect("text").as_bytes().to_vec(),
            "$cat" => a.as_array().expect("a list").iter().flat_map(|x| self.bytes(x, bn)).collect(),
            "$u8" => vec![self.num(a, bn) as u8],
            "$u16" => (self.num(a, bn) as u16).to_be_bytes().to_vec(),
            "$u32" => (self.num(a, bn) as u32).to_be_bytes().to_vec(),
            "$u64" => match a.as_str() {
                Some(s) if s.len() == 16 && is_hex(s) => unhex(s),
                _ => self.num(a, bn).to_be_bytes().to_vec(),
            },
            "$fill" => fill(a["len"].as_u64().expect("len") as usize, &seed_text(&a["seed"])),
            "$hash" => sha(&[&self.bytes(a, bn)]).to_vec(),
            "$key" => self.pk(a.as_str().expect("a key")).to_vec(),
            "$dev" => match a {
                Value::String(key) => self.dev(bn, key).to_vec(),
                _ => self.dev(a["board"].as_str().unwrap_or(bn), a["key"].as_str().expect("a key")).to_vec(),
            },
            "$dev_r" => fake_dev_r(&self.board(bn).id, &self.pk(a.as_str().expect("a key"))).to_vec(),
            "$slice" => {
                let b = self.bytes(&a["of"], bn);
                let from = a["from"].as_u64().unwrap_or(0) as usize;
                let to = a["to"].as_u64().map_or(b.len(), |t| t as usize);
                b[from..to].to_vec()
            }
            "$chain_from" => {
                let mut c: Hash32 = arr(&self.bytes(&a["before"], bn));
                let first = a["first"].as_u64().expect("first");
                for (seq, f) in (first..).zip(a["frames"].as_array().expect("frames")) {
                    let fb = self.bytes(f, bn);
                    c = chain_next(&c, seq, &sha(&[&fb]));
                }
                c.to_vec()
            }
            "$board" => self.board(a.as_str().expect("a board")).id.to_vec(),
            "$door" | "$token" => match self.val(v, bn) {
                Value::String(s) => unhex(&s),
                x => panic!("{x}"),
            },
            "$chain" => {
                let (board, seq) = match a {
                    Value::Object(_) => (a["board"].as_str().unwrap_or(bn).to_string(), a["seq"].as_u64().expect("seq")),
                    _ => (bn.to_string(), a.as_u64().expect("a seq")),
                };
                self.chain(&board, seq).to_vec()
            }
            "$frame" => self.frame(a, bn),
            "$batch" | "$frames" => {
                let frames: Vec<Vec<u8>> = a.as_array().expect("frames").iter().map(|x| self.bytes(x, bn)).collect();
                let mut out = Vec::new();
                if k == "$batch" {
                    out.extend_from_slice(b"TDA1");
                    out.push(frames.len() as u8);
                }
                for f in frames {
                    out.extend_from_slice(&(f.len() as u32).to_be_bytes());
                    out.extend_from_slice(&f);
                }
                out
            }
            "$create" => self.create_body(a),
            "$blob_begin" => {
                let id = self.bytes(&a["id"], bn);
                let mut out = id.clone();
                out.push(a["purpose"].as_u64().expect("purpose") as u8);
                out.extend(self.bytes(&json!({"$u64": a["seq"]}), bn));
                out.extend(self.bytes(&json!({"$u64": a["size"]}), bn));
                match a.get("sha256") {
                    Some(s) => out.extend(self.bytes(s, bn)),
                    None => out.extend(sha(&[&id])),
                }
                out
            }
            "$part" => {
                let size = a["size"].as_u64().expect("size");
                let index = a["index"].as_u64().expect("index");
                let parts = size.div_ceil(1_048_576);
                let n = if index + 1 < parts { 1_048_576 } else { size - (parts - 1) * 1_048_576 };
                let len = a["len"].as_u64().unwrap_or(n) as usize;
                vec![((a["fill"].as_u64().unwrap_or(7) + index) & 255) as u8; len]
            }
            _ => panic!("no builder {k}"),
        }
    }

    /// A frame (`seal.md` §8.1) with the fake signature, damaged as the builder says.
    fn frame(&mut self, a: &Value, bn: &str) -> Vec<u8> {
        let author = a["author"].as_str().expect("an author").to_string();
        let ctr = a["ctr"].as_u64().unwrap_or(0);
        let body_len = a["body_len"].as_u64().unwrap_or(558) as usize;
        let fill_seed = a.get("fill").map_or_else(|| format!("{author}/{ctr}"), seed_text);
        let dev = match a.get("dev") {
            Some(d) => self.bytes(d, bn),
            None => self.dev(bn, &author).to_vec(),
        };
        let mut head = a["magic"].as_str().unwrap_or("TDF1").as_bytes().to_vec();
        match a["board_id"].as_str() {
            Some(h) => head.extend(unhex(h)),
            None => head.extend(self.board(bn).id),
        }
        head.extend(dev);
        match a["ctr_hex"].as_str() {
            Some(h) => head.extend(unhex(h)),
            None => head.extend(ctr.to_be_bytes()),
        }
        let len_field = body_len as i64 + a["len_delta"].as_i64().unwrap_or(0);
        head.extend((len_field as u32).to_be_bytes());
        head.extend([1, 1]);
        head.extend(1u32.to_be_bytes());
        head.extend([0; 24]);
        head.extend(fill(body_len - 30, &fill_seed));
        let signer = a["signer"].as_str().unwrap_or(&author).to_string();
        let mut m = ctx("frame");
        m.extend_from_slice(&head);
        let mut sig = fake_sign(&self.pk(&signer), &m);
        match a["sig"].as_str().unwrap_or("good") {
            "good" => {}
            "flip" => sig[10] ^= 1,
            "zero-s" => sig[32..].fill(0),
            "high-s" => {
                // S + L, little-endian, as the harness does to a real signature.
                let l = unhex("edd3f55c1a631258d69cf7a2def9de1400000000000000000000000000000010");
                let mut carry = 0u16;
                for i in 0..32 {
                    let s = u16::from(sig[32 + i]) + u16::from(l[i]) + carry;
                    sig[32 + i] = s as u8;
                    carry = s >> 8;
                }
            }
            x => panic!("sig {x}"),
        }
        head.extend(sig);
        if let Some(t) = a["tail"].as_str() {
            head.extend(unhex(t));
        }
        head
    }

    fn create_body(&mut self, a: &Value) -> Vec<u8> {
        let bn = a["board"].as_str().expect("a board").to_string();
        let (recovery, first, salt) = {
            let b = self.board(&bn);
            (b.recovery.clone(), b.first.clone(), b.salt)
        };
        let rsign = a["rsign"].as_str().unwrap_or(&recovery).to_string();
        let first = a["dev"].as_str().unwrap_or(&first).to_string();
        let frame = match a.get("frame") {
            Some(f) => self.bytes(f, &bn),
            None => {
                let author = a["author"].as_str().unwrap_or(&recovery).to_string();
                self.frame(&json!({"author": author, "ctr": 1, "fill": "genesis"}), &bn)
            }
        };
        let mut out = self.pk(&rsign).to_vec();
        match a["salt"].as_str() {
            Some(s) => out.extend(unhex(s)),
            None => out.extend(salt),
        }
        match a["dev_id"].as_str() {
            Some(d) => out.extend(unhex(d)),
            None => out.extend(self.dev(&bn, &first)),
        }
        out.extend(self.pk(a["sign_pk"].as_str().unwrap_or(&first)));
        out.extend((a["flen"].as_u64().unwrap_or(frame.len() as u64) as u32).to_be_bytes());
        out.extend(frame);
        out
    }

    fn cred(&self, c: &Value, bn: &str) -> Step<Cred> {
        if let Some(key) = c["token"].as_str() {
            if c.get("board").is_some_and(|b| b.as_str() != Some(bn)) {
                return Err(Skip("a token of another board (gate)"));
            }
            return Ok(Cred::Token { dev: self.dev(bn, key), generation: c["gen"].as_u64().unwrap_or(1) as u32 });
        }
        if let Some(d) = c.get("door") {
            if d == &Value::Bool(true) || d.as_str() == Some(bn) {
                return Ok(Cred::Door);
            }
            return Err(Skip("the door of another board (gate)"));
        }
        if c.get("create") == Some(&Value::Bool(true)) {
            return Ok(Cred::Create);
        }
        if c.get("read_token").is_some() {
            return Err(Skip("a backup-read token (not modelled)"));
        }
        Err(Skip("no credential, a raw one or a wrong create secret (gate)"))
    }

    // ----- one step

    fn step(&mut self, s: &Value, tally: &mut Tally) {
        if let Some(c) = s["clock_ms"].as_i64() {
            self.clock = c;
        }
        let r = if s.get("socket").is_some() {
            self.socket_step(s)
        } else if let Some(f) = s.get("fill_log") {
            self.fill_log(f);
            Ok(())
        } else if s.get("storage_fault").is_some() || s.get("entropy_fault").is_some() {
            Err(Skip("a storage or entropy fault (not modelled)"))
        } else {
            self.request_step(s)
        };
        match r {
            Ok(()) => tally.run += 1,
            Err(Skip(why)) => *tally.skipped.entry(why).or_default() += 1,
        }
    }

    fn fill_log(&mut self, f: &Value) {
        let bn = f["board"].as_str().expect("a board").to_string();
        let cred = self.cred(&f["cred"], &bn).expect("fill_log's credential");
        let author = f["author"].as_str().expect("an author");
        let from = f["from_ctr"].as_u64().expect("from_ctr");
        let count = f["count"].as_u64().expect("count");
        let mut done = 0;
        while done < count {
            let n = 16.min(count - done);
            let frames: Vec<Vec<u8>> = (0..n)
                .map(|i| {
                    let mut spec = json!({"author": author, "ctr": from + done + i});
                    if let Some(extra) = f["frame"].as_object() {
                        for (k, v) in extra {
                            spec[k] = v.clone();
                        }
                    }
                    self.frame(&spec, &bn)
                })
                .collect();
            let now = self.clock;
            let r = self.board_mut(&bn).relay.append(now, &cred, &frames);
            let results = r.result.expect("fill_log: the batch is taken");
            assert!(results.iter().all(|x| matches!(x, AppendResult::New { .. })), "fill_log: every frame is new");
            self.note_results(&bn, &results, &frames);
            self.deliver(&bn, r.out);
            done += n;
        }
    }

    /// Hands the socket messages of an answer to the sockets they are for.
    fn deliver(&mut self, bn: &str, out: Vec<taskdeck_sync::sim::relay::Out>) {
        for o in out {
            if let Some(s) = self.sockets.values_mut().find(|s| s.board == bn && s.id == o.socket) {
                s.queue.push_back(o.msg);
            }
        }
    }

    fn request_step(&mut self, s: &Value) -> Step<()> {
        let rq = &s["request"];
        let e = &s["expect"];
        let bn = rq["board"].as_str().expect("a board").to_string();
        let route = rq["route"].as_str().unwrap_or("raw").to_string();
        let gate_refusal = e["x_td_relay"] == "gate" && e["status"].as_u64().is_some_and(|x| x >= 400);
        if gate_refusal || e["object_called"] == Value::Bool(false) {
            return Err(Skip("the gate answers (x_td_relay gate)"));
        }
        let unmodelled_code = ["relay/storage_refused", "relay/object_unavailable"];
        if e["json"]["error"].as_str().is_some_and(|c| unmodelled_code.contains(&c)) {
            return Err(Skip("a storage or entropy fault (not modelled)"));
        }
        match route.as_str() {
            "admin-nonce" | "login-nonce" | "caps" | "door" | "pair-open" | "slot-post" | "slot-get" | "pair-close" | "raw" => {
                self.last = json!({"nonce": hex(&[0; 32])});
                return Err(Skip("a route the model does not have"));
            }
            "admin" if matches!(rq["params"]["op"].as_str(), Some("mint_read_token" | "revoke_read_token")) => {
                self.skipped_mint = true;
                return Err(Skip("a route the model does not have"));
            }
            _ => {}
        }
        let cred = self.cred(&rq["cred"], &bn)?;
        let before = self.board(&bn).relay.clone();
        let resp = self.call(&route, rq, &bn, &cred, e)?;
        self.check(e, &resp, &bn, &before);
        Ok(())
    }

    /// Runs one HTTPS request against the model.
    fn call(&mut self, route: &str, rq: &Value, bn: &str, cred: &Cred, e: &Value) -> Step<Resp> {
        let now = self.clock;
        let body = match rq.get("body") {
            Some(b) if route != "admin" && route != "login" => self.bytes(b, bn),
            _ => Vec::new(),
        };
        let resp = match route {
            "create" => {
                let mut r = Rd::new(&body);
                let parsed = (|| {
                    let rsign: Hash32 = r.arr()?;
                    let salt: [u8; 16] = r.arr()?;
                    let dev: Dev = r.arr()?;
                    let pk: Hash32 = r.arr()?;
                    let flen = r.u32()? as usize;
                    (flen == r.left()).then_some(())?;
                    Some((rsign, salt, dev, pk, r.take(flen)?.to_vec()))
                })();
                let Some((rsign, salt, dev, pk, frame)) = parsed else { return Err(Skip("a body that does not parse by its layout")) };
                let rep = self.board_mut(bn).relay.create(now, &rsign, &salt, &dev, &pk, &frame);
                if rep.result.is_ok() {
                    self.note(bn, 1, &frame);
                }
                let id = self.board(bn).id;
                self.admin_json(bn, rep, |ok, _, g| {
                    json!({"seq": ok.seq, "chain": hex(&ok.chain.expect("chain")), "token": token_text(1, &id, &dev, g),
                        "door": hex(&ok.door.expect("door"))})
                })
            }
            "hello" => {
                let rep = self.board(bn).relay.hello(now, cred);
                let door = hex(&self.board(bn).relay.door());
                match rep.result {
                    Ok(w) => {
                        let mut o = json!({"head": {"seq": w.head.seq, "chain": hex(&w.head.chain)}, "last_ctr": w.last_ctr,
                            "last_seq": w.last_seq, "appends_left": w.appends_left, "relay_now": w.relay_now, "floor": w.floor,
                            "purge_floor": w.purge_floor, "state": if w.fenced { "fenced" } else { "open" }, "door": door, "push": w.push});
                        if let Some((h, d, g)) = w.fence {
                            o["fence"] = json!({"host": h, "door": hex(&d), "home_gen": g});
                        }
                        Resp::Json(200, o)
                    }
                    Err(r) => Resp::Json(r.status, error_json(&r, None)),
                }
            }
            "read" => {
                let (after, limit) = read_query(&rq["query"]).ok_or(Skip("a query that is not two integers (gate)"))?;
                match self.board(bn).relay.read(now, cred, after, limit).result {
                    Ok(p) => Resp::Page(p),
                    Err(r) => Resp::Json(r.status, error_json(&r, None)),
                }
            }
            "append" => {
                let frames = parse_batch(&body).ok_or(Skip("a body that does not parse by its layout"))?;
                let rep = self.board_mut(bn).relay.append(now, cred, &frames);
                self.deliver(bn, rep.out);
                match rep.result {
                    Ok(results) => {
                        self.note_results(bn, &results, &frames);
                        Resp::Results(results_status(&results), results)
                    }
                    Err(r) => Resp::Json(r.status, error_json(&r, None)),
                }
            }
            "blob-begin" => {
                let mut r = Rd::new(&body);
                let parsed = (|| {
                    let x = (r.arr::<16>()?, r.u8()?, r.u64()?, r.u64()?, r.arr::<32>()?);
                    r.end()?;
                    Some(x)
                })();
                let Some((id, purpose, seq, size, sha256)) = parsed else { return Err(Skip("a body that does not parse by its layout")) };
                match self.board_mut(bn).relay.blob_begin(now, cred, &id, purpose, seq, size, &sha256).result {
                    Ok(parts) => Resp::Json(200, json!({"id": hex(&id), "parts": parts})),
                    Err(r) => Resp::Json(r.status, error_json(&r, None)),
                }
            }
            "blob-put" | "blob-get" | "blob-meta" => {
                let id: [u8; 16] = match self.val(&rq["params"]["id"], bn) {
                    Value::String(s) if s.len() == 32 && is_hex(&s) => arr(&unhex(&s)),
                    _ => return Err(Skip("a path that does not match a route (gate)")),
                };
                let part = match &rq["params"]["part"] {
                    Value::Null => 0,
                    v => match self.val(v, bn) {
                        Value::Number(n) if n.as_u64().is_some_and(|p| p <= 63) => n.as_u64().unwrap_or(0) as u32,
                        Value::String(t) if t.parse::<u32>().is_ok_and(|p| p <= 63) => t.parse().unwrap_or(0),
                        _ => return Err(Skip("a path that does not match a route (gate)")),
                    },
                };
                let relay = &mut self.board_mut(bn).relay;
                match route {
                    "blob-put" => match relay.blob_put(now, cred, &id, part, &body).result {
                        Ok((received, complete)) => Resp::Json(200, json!({"received": received, "complete": complete})),
                        Err(r) => Resp::Json(r.status, error_json(&r, None)),
                    },
                    "blob-get" => match relay.blob_get(cred, &id, part).result {
                        Ok(b) => Resp::Bytes(b),
                        Err(r) => Resp::Json(r.status, error_json(&r, None)),
                    },
                    _ => match relay.blob_meta(cred, &id).result {
                        Ok(b) => Resp::Json(200, blob_json(&b)),
                        Err(r) => Resp::Json(r.status, error_json(&r, None)),
                    },
                }
            }
            "blob-list" => match self.board(bn).relay.blob_list(cred).result {
                Ok(v) => Resp::Json(200, json!({"blobs": v.iter().map(blob_json).collect::<Vec<_>>()})),
                Err(r) => Resp::Json(r.status, error_json(&r, None)),
            },
            "seed-tail" => {
                let mut r = Rd::new(&body);
                let parsed = (|| {
                    let first = r.u64()?;
                    let n = r.u16()?;
                    let mut frames = Vec::new();
                    for _ in 0..n {
                        let len = r.u32()? as usize;
                        frames.push(r.take(len)?.to_vec());
                    }
                    r.end()?;
                    Some((first, frames))
                })();
                let Some((first, frames)) = parsed else { return Err(Skip("a body that does not parse by its layout")) };
                let rep = self.board_mut(bn).relay.seed_tail(now, cred, first, &frames);
                if let Ok(ok) = &rep.result {
                    let h = ok.head.expect("head");
                    for (i, f) in frames.iter().enumerate() {
                        self.note(bn, h.seq + 1 - frames.len() as u64 + i as u64, f);
                    }
                }
                let state = self.board(bn).relay.state();
                self.admin_json(bn, rep, |ok, _, _| {
                    let h = ok.head.expect("head");
                    json!({"head": {"seq": h.seq, "chain": hex(&h.chain)}, "state": state_text(state)})
                })
            }
            "login" => {
                let code = e["json"]["error"].as_str();
                if e["status"].as_u64() != Some(200) || code.is_some() {
                    return Err(Skip("a login whose nonce, generation or signature is tested (not modelled)"));
                }
                let key = rq["body"]["$login"]["dev"].as_str().expect("a login dev");
                let dev = self.dev(bn, key);
                let id = self.board(bn).id;
                let rep = self.board_mut(bn).relay.login(now, cred, &dev);
                self.deliver(bn, rep.out);
                match rep.result {
                    Ok(l) => {
                        let mut o = json!({"token": token_text(1, &id, &dev, u64::from(l.generation)), "gen": l.generation, "push": true});
                        if let Some((first, wraps)) = l.wraps {
                            o["wraps"] = json!({"first_epoch": first, "wraps": wraps.iter().map(|w| hex(w)).collect::<Vec<_>>()});
                        }
                        Resp::Json(200, o)
                    }
                    Err(r) => Resp::Json(r.status, error_json(&r, None)),
                }
            }
            "admin" => self.admin_step(rq, bn, cred, e)?,
            x => panic!("route {x}"),
        };
        Ok(resp)
    }

    /// The JSON of an admin-like answer: `f` builds the success body.
    fn admin_json(&mut self, bn: &str, rep: Reply<AdminOk>, f: impl FnOnce(&AdminOk, &mut World, u64) -> Value) -> Resp {
        self.deliver(bn, rep.out);
        match rep.result {
            Ok(ok) => {
                let g = u64::from(rep.token_gen.unwrap_or(0));
                Resp::Json(200, f(&ok, self, g))
            }
            Err(r) => Resp::Json(r.status, error_json(&r, rep.last)),
        }
    }

    fn admin_step(&mut self, rq: &Value, bn: &str, cred: &Cred, e: &Value) -> Step<Resp> {
        let op = rq["params"]["op"].as_str().expect("an op").to_string();
        let Some(a) = rq["body"].get("$admin") else { return Err(Skip("a body that does not parse by its layout")) };
        if ["sig", "sign_key", "context", "sign_op", "sign_args"].iter().any(|k| a.get(*k).is_some()) {
            return Err(Skip("an admin signature that is tested (not modelled)"));
        }
        if ["args_len", "extra_len", "tail"].iter().any(|k| a.get(*k).is_some()) || a["op"].as_str() != Some(&op) {
            return Err(Skip("a body that does not parse by its layout"));
        }
        if e["json"]["error"].as_str().is_some_and(|c| c == "relay/no_nonce" || c == "relay/bad_admin_sig") {
            return Err(Skip("an admin nonce or signature that is tested (not modelled)"));
        }
        let signer: Dev = match a.get("signer_dev") {
            Some(d) => arr(&self.bytes(d, bn)),
            None => self.dev(bn, a["signer"].as_str().expect("a signer")),
        };
        let args = self.admin_args(&op, &a["args"], bn);
        let extra = match a.get("extra") {
            Some(x) => self.bytes(x, bn),
            None => Vec::new(),
        };
        let parsed = parse_op(&op, &args, &extra).ok_or(Skip("a body that does not parse by its layout"))?;
        let now = self.clock;
        let rep = self.board_mut(bn).relay.op(now, cred, &signer, &parsed);
        let id = self.board(bn).id;
        if rep.result.is_ok() {
            match &parsed {
                AdminOp::Register { frame: Some(f), .. } | AdminOp::Fence { frame: Some(f), .. } => {
                    if let Ok(AdminOk { seq: Some(s), .. }) = &rep.result {
                        self.note(bn, *s, f);
                    }
                }
                AdminOp::RestoreTail { frames, .. } => {
                    let h = rep.result.as_ref().map(|o| o.head.expect("head")).expect("ok");
                    for (i, f) in frames.iter().enumerate() {
                        self.note(bn, h.seq + 1 - frames.len() as u64 + i as u64, f);
                    }
                }
                AdminOp::Seed { first_seq, chain_before, .. } => {
                    let b = self.board_mut(bn);
                    b.hash_at.clear();
                    b.chain_at.clear();
                    b.chain_at.insert(first_seq - 1, *chain_before);
                }
                _ => {}
            }
        }
        let state = self.board(bn).relay.state();
        let p = parsed.clone();
        Ok(self.admin_json(bn, rep, move |ok, _, g| match &p {
            AdminOp::Register { dev, .. } => {
                json!({"seq": ok.seq, "chain": hex(&ok.chain.expect("chain")), "token": token_text(1, &id, dev, g)})
            }
            AdminOp::Fence { .. } | AdminOp::RecoveryRegister { .. } => match ok.seq {
                Some(s) => json!({"seq": s, "chain": hex(&ok.chain.expect("chain"))}),
                None => json!({}),
            },
            AdminOp::Seed { first_seq, .. } => json!({"door": hex(&ok.door.expect("door")), "token": token_text(1, &id, &signer, g),
                "next_seq": first_seq, "state": state_text(state)}),
            AdminOp::Compact { .. } => json!({"deleted": ok.deleted, "floor": ok.floor, "more": ok.more}),
            AdminOp::RestoreTail { .. } => {
                let h = ok.head.expect("head");
                json!({"head": {"seq": h.seq, "chain": hex(&h.chain)}})
            }
            AdminOp::Purge { .. } => json!({"purge_floor": ok.purge_floor, "done": ok.done}),
            AdminOp::Revoke { .. } | AdminOp::Reinstate { .. } => json!({}),
        }))
    }

    /// The `args` bytes of an operation (the harness's `adminArgs`).
    fn admin_args(&mut self, op: &str, f: &Value, bn: &str) -> Vec<u8> {
        if f.get("$hex").is_some() || f.get("$cat").is_some() {
            return self.bytes(f, bn);
        }
        let dev = |w: &mut World, x: &Value| match x.as_str() {
            Some(k) if w.keys.contains_key(k) => w.dev(bn, k).to_vec(),
            _ => w.bytes(x, bn),
        };
        let pk = |w: &mut World, x: &Value| match x.as_str() {
            Some(k) if w.keys.contains_key(k) => w.pk(k).to_vec(),
            _ => w.bytes(x, bn),
        };
        let mut out = Vec::new();
        match op {
            "register" | "recovery_register" => {
                out.extend(dev(self, &f["dev"]));
                out.extend(pk(self, &f["sign_pk"]));
                out.extend(self.bytes(&f["frame_hash"], bn));
            }
            "revoke" | "revoke_read_token" => out.extend(dev(self, &f["dev"])),
            "reinstate" => {
                out.extend(dev(self, &f["dev"]));
                out.extend((f["first_epoch"].as_u64().expect("first_epoch") as u32).to_be_bytes());
                let wraps = f["wraps"].as_u64().expect("wraps");
                out.extend((f["n"].as_u64().unwrap_or(wraps) as u16).to_be_bytes());
                out.extend(fill(96 * wraps as usize, f["fill"].as_str().unwrap_or("wraps")));
            }
            "fence" => {
                let host = f["host"].as_str().expect("host");
                out.push(host.len() as u8);
                out.extend(host.as_bytes());
                out.extend(self.bytes(&f["door"], bn));
                out.extend((f["home_gen"].as_u64().expect("home_gen") as u32).to_be_bytes());
            }
            "seed" => {
                let mut entries = Vec::new();
                for x in f["entries"].as_array().expect("entries") {
                    let mut en = dev(self, &x["dev"]);
                    en.extend(pk(self, &x["sign_pk"]));
                    en.push(x["role"].as_u64().expect("role") as u8);
                    en.push(x["status"].as_u64().expect("status") as u8);
                    en.extend(self.bytes(&json!({"$u64": x["last_ctr"]}), bn));
                    entries.push(en);
                }
                if f["sort"] != Value::Bool(false) {
                    entries.sort();
                }
                if f["sort"] == "reverse" {
                    entries.reverse();
                }
                out.extend((f["home_gen"].as_u64().expect("home_gen") as u32).to_be_bytes());
                out.extend(self.bytes(&json!({"$u64": f["first_seq"]}), bn));
                out.extend(self.bytes(&f["chain_before"], bn));
                out.extend(self.bytes(&json!({"$u64": f["last_seq"]}), bn));
                out.extend(self.bytes(&f["chain_last"], bn));
                match f.get("blob_id") {
                    Some(b) => out.extend(self.bytes(b, bn)),
                    None => out.extend([0; 16]),
                }
                out.extend((f["n"].as_u64().unwrap_or(entries.len() as u64) as u16).to_be_bytes());
                for en in entries {
                    out.extend(en);
                }
            }
            "compact" => {
                out.extend(self.bytes(&json!({"$u64": f["below_seq"]}), bn));
                out.extend(self.bytes(&json!({"$u64": f["snap_entry_seq"]}), bn));
                out.extend(self.bytes(&f["blob_id"], bn));
                let keep = f["keep"].as_array().expect("keep");
                out.extend((f["n"].as_u64().unwrap_or(keep.len() as u64) as u32).to_be_bytes());
                for k in keep {
                    out.extend(k.as_u64().expect("a keep seq").to_be_bytes());
                }
            }
            "restore_tail" => {
                let hashes: Vec<Vec<u8>> = f["hashes"].as_array().expect("hashes").iter().map(|h| self.bytes(h, bn)).collect();
                out.extend(self.bytes(&json!({"$u64": f["first_seq"]}), bn));
                out.extend((f["n"].as_u64().unwrap_or(hashes.len() as u64) as u32).to_be_bytes());
                for h in hashes {
                    out.extend(h);
                }
            }
            "purge" => {
                out.extend(self.bytes(&json!({"$u64": f["before_seq"]}), bn));
                out.extend((f["epoch"].as_u64().expect("epoch") as u32).to_be_bytes());
            }
            x => panic!("no args layout for {x}"),
        }
        out
    }

    // ----- expectations

    fn check(&mut self, e: &Value, resp: &Resp, bn: &str, before: &ModelRelay) {
        let (status, j) = match resp {
            Resp::Json(s, j) => (*s, Some(j.clone())),
            Resp::Results(s, _) => (*s, None),
            Resp::Page(_) | Resp::Bytes(_) => (200, None),
        };
        let text = j.as_ref().map(Value::to_string).unwrap_or_default();
        if let Some(s) = e["status"].as_u64() {
            assert_eq!(u64::from(status), s, "status ({text})");
        }
        if let Some(j) = &j {
            self.last = j.clone();
        }
        if let Some(x) = e.get("json") {
            let got = j.clone().unwrap_or_else(|| panic!("a JSON body expected, {}", resp_kind(resp)));
            self.expect_value(x, &got, bn, "json");
        }
        if let Some(code) = e["error"].as_str() {
            assert_eq!(j.as_ref().and_then(|j| j["error"].as_str()), Some(code), "error ({text})");
        }
        if let Some(b) = e.get("body") {
            let want = self.bytes(b, bn);
            match resp {
                Resp::Bytes(got) => assert_eq!(hex(got), hex(&want), "body"),
                _ => panic!("a raw body expected ({text})"),
            }
        }
        if let Some(r) = e.get("results") {
            let Resp::Results(_, got) = resp else { panic!("a results body expected ({text})") };
            self.expect_results(r, got, bn);
        }
        if let Some(p) = e.get("page") {
            let Resp::Page(got) = resp else { panic!("a page expected ({text})") };
            self.expect_page(p, got, bn);
        }
        if e["rows_written"].as_u64() == Some(0) {
            assert!(self.board(bn).relay.same_tables(before), "rows_written 0: the model wrote ({text})");
        }
        if let Some(st) = e.get("storage") {
            self.expect_storage(st, bn);
        }
    }

    fn expect_value(&mut self, exp: &Value, got: &Value, bn: &str, at: &str) {
        match exp {
            Value::String(s) if s == "$any" => assert!(!got.is_null(), "{at}: missing"),
            Value::String(s) if s == "$push" => assert_eq!(got, &Value::Bool(true), "{at}"),
            Value::Object(o) if o.len() == 1 && o.keys().next().is_some_and(|k| k.starts_with('$')) => {
                if let Some(off) = o.get("$time") {
                    assert_eq!(got.as_i64(), Some(self.clock + off.as_i64().expect("an offset")), "{at}: time");
                    return;
                }
                if at.ends_with(".token") || at.ends_with(".nonce") {
                    assert!(got.is_string(), "{at}: a string");
                    if at.ends_with(".nonce") {
                        return;
                    }
                }
                let want = self.val(exp, bn);
                assert_eq!(got, &want, "{at}");
            }
            Value::Object(o) => {
                let g = got.as_object().unwrap_or_else(|| panic!("{at}: not an object: {got}"));
                let mut gk: Vec<&String> = g.keys().collect();
                let mut ek: Vec<&String> = o.keys().collect();
                gk.sort();
                ek.sort();
                assert_eq!(gk, ek, "{at}: the fields ({got})");
                for (k, v) in o {
                    self.expect_value(v, &g[k], bn, &format!("{at}.{k}"));
                }
            }
            Value::Array(a) => {
                let g = got.as_array().unwrap_or_else(|| panic!("{at}: not an array: {got}"));
                assert_eq!(g.len(), a.len(), "{at}: {got}");
                for (i, v) in a.iter().enumerate() {
                    self.expect_value(v, &g[i], bn, &format!("{at}[{i}]"));
                }
            }
            Value::String(s) if s.starts_with('$') => {
                let want = self.val(exp, bn);
                assert_eq!(got, &want, "{at}");
            }
            _ => assert_eq!(got, exp, "{at}"),
        }
    }

    fn expect_results(&mut self, exp: &Value, got: &[AppendResult], bn: &str) {
        let exp = exp.as_array().expect("results");
        let names: Vec<&str> = got.iter().map(status_name).collect();
        assert_eq!(got.len(), exp.len(), "results: {names:?}");
        for (i, (x, g)) in exp.iter().zip(got).enumerate() {
            let x = if x.is_string() { json!({"status": x}) } else { x.clone() };
            assert_eq!(status_name(g), x["status"], "result {i}: {g:?}");
            let fields = match g {
                AppendResult::New { seq, .. } | AppendResult::Dup { seq, .. } | AppendResult::Conflict { seq, .. } => json!({"seq": seq}),
                AppendResult::Gap { last_ctr, last_seq } => json!({"last_ctr": last_ctr, "last_seq": last_seq}),
                AppendResult::Refused { status, code } => json!({"http_status": status, "code": code}),
                AppendResult::Skipped => json!({}),
            };
            for k in ["seq", "last_ctr", "last_seq", "http_status", "code"] {
                if !x[k].is_null() {
                    assert_eq!(fields[k], x[k], "result {i} {k}");
                }
            }
            if let AppendResult::New { seq, chain } | AppendResult::Dup { seq, chain } = g {
                assert_eq!(hex(chain), hex(&self.chain(bn, *seq)), "result {i} chain (RELAY-STORE-003)");
            }
            if let Some(f) = x.get("frame") {
                let AppendResult::Conflict { frame, .. } = g else { panic!("result {i}: a frame on a non-conflict") };
                let want = if f.is_null() { Vec::new() } else { self.bytes(f, bn) };
                assert_eq!(hex(frame), hex(&want), "result {i} frame");
            }
        }
    }

    fn expect_page(&mut self, exp: &Value, got: &Page, bn: &str) {
        let have = json!({"head_seq": got.head.seq, "floor": got.floor, "purge_floor": got.purge_floor, "more": got.more,
            "truncated": got.truncated, "fenced": got.fenced});
        for k in ["head_seq", "floor", "purge_floor", "more", "truncated", "fenced"] {
            if !exp[k].is_null() {
                assert_eq!(have[k], exp[k], "page {k}");
            }
        }
        if let Some(h) = exp["head_seq"].as_u64() {
            assert_eq!(hex(&got.head.chain), hex(&self.chain(bn, h)), "page head_chain");
        }
        if let Some(n) = exp.get("relay_now") {
            self.expect_value(n, &json!(got.relay_now), bn, "page relay_now");
        }
        if let Some(s) = exp.get("seqs") {
            let seqs: Vec<u64> = got.entries.iter().map(|x| x.seq).collect();
            assert_eq!(json!(seqs), *s, "page seqs");
        }
        if let Some(fs) = exp["frames"].as_array() {
            for (i, f) in fs.iter().enumerate() {
                let want = self.bytes(f, bn);
                assert_eq!(hex(&got.entries[i].frame), hex(&want), "page frame {i}");
            }
        }
        assert!(got.entries.windows(2).all(|w| w[0].at <= w[1].at), "at never decreases (RELAY-STORE-004)");
    }

    fn expect_storage(&mut self, exp: &Value, bn: &str) {
        let relay = self.board(bn).relay.clone();
        if let Some(b) = exp.get("board") {
            match (b, relay.board_row()) {
                (Value::Null, r) => assert!(r.is_none(), "no board row"),
                (_, None) => panic!("one board row"),
                (want, Some(row)) => {
                    let row = board_json(&relay, row);
                    for (k, v) in want.as_object().expect("board") {
                        if k == "read_epoch" && self.skipped_mint {
                            continue; // raised by a `mint_read_token` the model does not have
                        }
                        self.expect_value(v, &row[k], bn, &format!("board.{k}"));
                    }
                }
            }
        }
        if let Some(devs) = exp["devices"].as_object() {
            for (name, want) in devs {
                let d = self.dev(bn, name);
                match (want, relay.devices().get(&d)) {
                    (Value::Null, r) => assert!(r.is_none(), "no devices row for {name}"),
                    (_, None) => panic!("devices row of {name}"),
                    (want, Some(row)) => {
                        let row = json!({"dev": hex(&row.dev), "sign_pk": hex(&row.sign_pk), "role": row.role, "state": row.state,
                            "gen": row.generation, "last_ctr": row.last_ctr, "last_seq": row.last_seq, "app_day": row.app_day,
                            "app_count": row.app_count, "added_at": row.added_at,
                            "wraps": row.wraps.as_ref().map(|(f, w)| {
                                let mut b = f.to_be_bytes().to_vec();
                                b.extend((w.len() as u16).to_be_bytes());
                                for x in w { b.extend(x); }
                                hex(&b)
                            })});
                        for (k, v) in want.as_object().expect("a devices row") {
                            self.expect_value(v, &row[k], bn, &format!("devices.{name}.{k}"));
                        }
                    }
                }
            }
        }
        if let Some(l) = exp.get("log") {
            let seqs: Vec<u64> = relay.log().keys().copied().collect();
            assert_eq!(json!(seqs), *l, "log seqs");
        }
        if let Some(c) = exp["count"].as_object() {
            for (t, n) in c {
                let have = match t.as_str() {
                    "log" => relay.log().len(),
                    "devices" => relay.devices().len(),
                    "blobs" => relay.blobs().len(),
                    "parts" => relay.part_count(),
                    "board" => usize::from(relay.board_row().is_some()),
                    _ => continue, // nonces, tickets, read_tokens: not modelled
                };
                assert_eq!(json!(have), *n, "rows in {t}");
            }
        }
        if let Some(bl) = exp["blobs"].as_array() {
            let rows: Vec<&BlobRow> = relay.blobs().values().collect();
            assert_eq!(rows.len(), bl.len(), "rows in blobs: {:?}", rows.iter().map(|b| hex(&b.id)).collect::<Vec<_>>());
            for (i, want) in bl.iter().enumerate() {
                let b = rows[i];
                let row = json!({"id": hex(&b.id), "slot": b.slot, "purpose": b.purpose, "seq": b.seq, "author": hex(&b.author),
                    "size": b.size, "parts": b.parts, "sha256": hex(&b.sha256), "received": b.received,
                    "complete": u8::from(b.complete), "at": b.at});
                for (k, v) in want.as_object().expect("a blobs row") {
                    self.expect_value(v, &row[k], bn, &format!("blobs[{i}].{k}"));
                }
            }
        }
    }

    // ----- sockets

    fn socket_step(&mut self, s: &Value) -> Step<()> {
        let name = s["socket"].as_str().expect("a socket name").to_string();
        let e = &s["expect"];
        if let Some(o) = s.get("open") {
            let bn = o["board"].as_str().expect("a board").to_string();
            if e["x_td_relay"] == "gate" {
                return Err(Skip("the gate answers (x_td_relay gate)"));
            }
            let cred = self.cred(&o["cred"], &bn)?;
            match self.board_mut(&bn).relay.socket_open(&cred) {
                Ok(id) => {
                    assert_eq!(e["status"].as_u64(), Some(101), "upgrade of {name}");
                    self.sockets.insert(name, Sock { board: bn, id, queue: VecDeque::new(), lost: false });
                }
                Err(r) => {
                    assert_eq!(e["status"].as_u64(), Some(u64::from(r.status)), "upgrade status of {name}: {}", r.code);
                    if let Some(c) = e["error"].as_str() {
                        assert_eq!(r.code, c, "upgrade of {name}");
                    }
                }
            }
            return Ok(());
        }
        let (bn, id, lost) = {
            let k = self.sockets.get(&name).unwrap_or_else(|| panic!("socket {name}"));
            (k.board.clone(), k.id, k.lost)
        };
        if lost {
            return Err(Skip("a malformed socket message (the model takes values)"));
        }
        if e["messages"].as_array().is_some_and(|m| m.iter().any(|x| x["type"] == "slot")) {
            return Err(Skip("a slot notice (pairing is not modelled)"));
        }
        let now = self.clock;
        if let Some(m) = s.get("send") {
            let out = match m.as_object().and_then(|o| o.iter().next()) {
                Some((k, a)) if k == "$hello" => {
                    let session: [u8; 16] = arr(&self.bytes(&a["session"], &bn));
                    let features = a["features"].as_u64().unwrap_or(0) as u32;
                    self.board_mut(&bn).relay.socket_hello(now, id, session, features)
                }
                Some((k, a)) if k == "$sock_append" && a["batch"].is_array() => {
                    let frames: Vec<Vec<u8>> = a["batch"].as_array().expect("a batch").iter().map(|f| self.bytes(f, &bn)).collect();
                    let req = a["req"].as_u64().expect("req") as u32;
                    let out = self.board_mut(&bn).relay.socket_append(now, id, req, &frames);
                    for o in &out {
                        if let SockMsg::Result { results, .. } = &o.msg {
                            self.note_results(&bn, &results.clone(), &frames);
                        }
                    }
                    out
                }
                Some((k, a)) if k == "$sock_read" => {
                    let after = match a["after"].as_str() {
                        Some(h) => u64::from_be_bytes(arr(&unhex(h))),
                        None => a["after"].as_u64().expect("after"),
                    };
                    let limit = a["limit"].as_u64().expect("limit");
                    let Ok(limit) = u16::try_from(limit) else { return Err(Skip("a malformed socket message (the model takes values)")) };
                    let req = a["req"].as_u64().expect("req") as u32;
                    self.board_mut(&bn).relay.socket_read(now, id, req, after, limit)
                }
                Some((k, a)) if k == "$text" && a == "ping" => {
                    return Err(Skip("the text ping (answered by the runtime)"));
                }
                _ => {
                    // Malformed bytes or text: the model has no byte parser.
                    self.board_mut(&bn).relay.socket_close(id);
                    if let Some(k) = self.sockets.get_mut(&name) {
                        k.lost = true;
                    }
                    return Err(Skip("a malformed socket message (the model takes values)"));
                }
            };
            self.deliver(&bn, out);
        }
        if s["close"].as_bool() == Some(true) || s["close"].is_number() {
            self.board_mut(&bn).relay.socket_close(id);
        }
        let msgs: Vec<Value> = match (e.get("messages"), e.get("message")) {
            (Some(m), _) => m.as_array().expect("messages").clone(),
            (None, Some(m)) => vec![m.clone()],
            _ => Vec::new(),
        };
        for m in msgs {
            let got = self.sockets.get_mut(&name).and_then(|k| k.queue.pop_front());
            let got = got.unwrap_or_else(|| panic!("socket {name}: expected {}, got nothing", m["type"]));
            self.expect_message(&m, &got, &bn, &name);
        }
        if let Some(code) = e["closed"].as_u64() {
            let got = self.sockets.get_mut(&name).and_then(|k| k.queue.pop_front());
            assert_eq!(got, Some(SockMsg::Close(code as u16)), "socket {name} close");
        }
        if e["silent"] == Value::Bool(true) {
            let q = &self.sockets[&name].queue;
            assert!(q.is_empty(), "socket {name}: unexpected {:?}", q.front());
        }
        Ok(())
    }

    fn expect_message(&mut self, exp: &Value, got: &SockMsg, bn: &str, name: &str) {
        let door = hex(&self.board(bn).relay.door());
        let (kind, fields) = match got {
            SockMsg::Welcome(w) => {
                let mut f = json!({"head_seq": w.head.seq, "last_ctr": w.last_ctr, "last_seq": w.last_seq, "relay_now": w.relay_now,
                    "floor": w.floor, "purge_floor": w.purge_floor, "state": if w.fenced { 2 } else { 1 }, "caps": u8::from(w.push),
                    "appends_left": w.appends_left, "door": door, "has_fence": u8::from(w.fence.is_some())});
                if let Some((h, d, g)) = &w.fence {
                    f["fence"] = json!({"host": h, "door": hex(d), "home_gen": g});
                }
                ("welcome", f)
            }
            SockMsg::Result { req, .. } => ("result", json!({"req": req})),
            SockMsg::Page { req, .. } => ("page", json!({"req": req})),
            SockMsg::Push(_) => ("push", json!({})),
            SockMsg::Second(s) => ("second", json!({"session": hex(s)})),
            SockMsg::Displaced(s) => ("displaced", json!({"session": hex(s)})),
            SockMsg::Error { req, refusal } => ("error", json!({"req": req, "status": refusal.status, "code": refusal.code})),
            SockMsg::State { host, door, home_gen } => {
                ("state", json!({"state": 2, "has_fence": 1, "fence": {"host": host, "door": hex(door), "home_gen": home_gen}}))
            }
            SockMsg::Close(c) => panic!("socket {name}: expected {}, got close {c}", exp["type"]),
        };
        assert_eq!(exp["type"], kind, "socket {name}: message {got:?}");
        for (k, v) in exp.as_object().expect("a message") {
            match k.as_str() {
                "type" => {}
                "results" => {
                    let SockMsg::Result { results, .. } = got else { unreachable!() };
                    self.expect_results(v, results, bn);
                }
                "page" => match got {
                    SockMsg::Page { page, .. } | SockMsg::Push(page) => self.expect_page(v, page, bn),
                    _ => panic!("socket {name}: a page"),
                },
                "head_seq" => {
                    let SockMsg::Welcome(w) = got else { unreachable!() };
                    assert_eq!(json!(w.head.seq), *v, "welcome head_seq");
                    let h = v.as_u64().expect("a seq");
                    assert_eq!(hex(&w.head.chain), hex(&self.chain(bn, h)), "welcome head_chain");
                }
                _ => self.expect_value(v, &fields[k], bn, &format!("message.{k}")),
            }
        }
    }
}

fn seed_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        x => x.to_string(),
    }
}

fn state_text(s: Option<BoardState>) -> &'static str {
    match s {
        Some(BoardState::Seeding) => "seeding",
        Some(BoardState::Fenced) => "fenced",
        _ => "open",
    }
}

fn resp_kind(r: &Resp) -> &'static str {
    match r {
        Resp::Json(..) => "json",
        Resp::Results(..) => "results",
        Resp::Page(_) => "page",
        Resp::Bytes(_) => "bytes",
    }
}

fn board_json(relay: &ModelRelay, b: &taskdeck_sync::sim::relay::BoardRow) -> Value {
    json!({"one": 1, "schema": 1, "board_id": hex(&relay.board_id()), "state": b.state as u8, "created_at": b.created_at,
        "base_seq": b.base_seq, "base_chain": hex(&b.base_chain), "compact_floor": b.compact_floor,
        "purge_floor": b.purge_floor, "purge_done": b.purge_done, "compact_snap": b.compact_snap.map(|x| hex(&x)),
        "del_day": b.del_day, "del_count": b.del_count, "read_epoch": b.read_epoch, "home_gen": b.home_gen,
        "fence_host": b.fence_host, "fence_door": b.fence_door.map(|x| hex(&x)), "seed_signer": b.seed_signer.map(|x| hex(&x)),
        "seed_last_seq": b.seed_last_seq, "seed_chain": b.seed_chain.map(|x| hex(&x))})
}

/// `after` and `limit` of a read query; `None` for anything the gate refuses.
fn read_query(q: &Value) -> Option<(u64, u16)> {
    let pairs: Vec<(String, String)> = match q {
        Value::String(s) => s
            .split('&')
            .map(|p| {
                let (k, v) = p.split_once('=').unwrap_or((p, ""));
                (k.to_string(), v.to_string())
            })
            .collect(),
        Value::Object(o) => o.iter().map(|(k, v)| (k.clone(), seed_text(v))).collect(),
        _ => return None,
    };
    let mut after = None;
    let mut limit = Some(256);
    for (k, v) in pairs {
        let ok = !v.is_empty() && v.bytes().all(|c| c.is_ascii_digit()) && (v == "0" || !v.starts_with('0'));
        match k.as_str() {
            "after" if ok && after.is_none() => after = Some(v.parse::<u64>().ok()?),
            "limit" if ok => limit = Some(v.parse::<u16>().ok()?),
            _ => return None,
        }
    }
    Some((after?, limit?))
}

/// The frames of a batch (§9.2); `None` when its layout fails.
fn parse_batch(b: &[u8]) -> Option<Vec<Vec<u8>>> {
    let mut r = Rd::new(b);
    (r.take(4)? == b"TDA1").then_some(())?;
    let n = r.u8()?;
    let mut frames = Vec::new();
    for _ in 0..n {
        let len = r.u32()? as usize;
        frames.push(r.take(len)?.to_vec());
    }
    r.end()?;
    Some(frames)
}

/// An admin operation from its `args` and `extra` bytes (relay-core.js
/// `parseAdmin`); `None` when a layout fails (A1, left out).
fn parse_op(op: &str, args: &[u8], extra: &[u8]) -> Option<AdminOp> {
    let mut r = Rd::new(args);
    let x = Rd::new(extra);
    let out = match op {
        "register" => {
            let (dev, sign_pk, frame_hash) = (r.arr()?, r.arr()?, r.arr()?);
            r.end()?;
            if extra.len() == 8 {
                AdminOp::Register { dev, sign_pk, frame_hash, frame: None, seq: u64::from_be_bytes(arr(extra)) }
            } else {
                AdminOp::Register { dev, sign_pk, frame_hash, frame: Some(extra.to_vec()), seq: 0 }
            }
        }
        "recovery_register" => {
            let (dev_r, sign_pk, frame_hash) = (r.arr()?, r.arr()?, r.arr()?);
            r.end()?;
            (extra.len() == 8).then_some(())?;
            AdminOp::RecoveryRegister { dev_r, sign_pk, frame_hash, seq: u64::from_be_bytes(arr(extra)) }
        }
        "revoke" => {
            let dev = r.arr()?;
            r.end()?;
            x.end()?;
            AdminOp::Revoke { dev }
        }
        "reinstate" => {
            let dev = r.arr()?;
            let first_epoch = r.u32()?;
            let n = r.u16()? as usize;
            (r.left() == 96 * n).then_some(())?;
            let wraps = (0..n).map(|_| r.arr::<96>()).collect::<Option<Vec<_>>>()?;
            x.end()?;
            AdminOp::Reinstate { dev, first_epoch, wraps }
        }
        "fence" => {
            let hl = r.u8()? as usize;
            let host = String::from_utf8(r.take(hl)?.to_vec()).ok()?;
            let (door, home_gen) = (r.arr()?, r.u32()?);
            r.end()?;
            AdminOp::Fence { host, door, home_gen, frame: (!extra.is_empty()).then(|| extra.to_vec()) }
        }
        "seed" => {
            let home_gen = r.u32()?;
            let first_seq = r.u64()?;
            let chain_before = r.arr()?;
            let last_seq = r.u64()?;
            let chain_last = r.arr()?;
            let blob_id = r.arr()?;
            let n = r.u16()? as usize;
            (r.left() == 58 * n).then_some(())?;
            let mut devices = Vec::new();
            for _ in 0..n {
                devices.push(SeedDevice { dev: r.arr()?, sign_pk: r.arr()?, role: r.u8()?, status: r.u8()?, last_ctr: r.u64()? });
            }
            x.end()?;
            AdminOp::Seed { home_gen, first_seq, chain_before, last_seq, chain_last, blob_id, devices }
        }
        "compact" => {
            let below_seq = r.u64()?;
            let snap_entry_seq = r.u64()?;
            let blob_id = r.arr()?;
            let n = r.u32()? as usize;
            (r.left() == 8 * n).then_some(())?;
            let keep = (0..n).map(|_| r.u64()).collect::<Option<Vec<_>>>()?;
            x.end()?;
            AdminOp::Compact { below_seq, snap_entry_seq, blob_id, keep }
        }
        "restore_tail" => {
            let first_seq = r.u64()?;
            let n = r.u32()? as usize;
            (r.left() == 32 * n).then_some(())?;
            let hashes = (0..n).map(|_| r.arr::<32>()).collect::<Option<Vec<_>>>()?;
            let mut x = Rd::new(extra);
            let chain_before = x.arr()?;
            let mut frames = Vec::new();
            for _ in 0..n {
                let len = x.u32()? as usize;
                frames.push(x.take(len)?.to_vec());
            }
            x.end()?;
            AdminOp::RestoreTail { first_seq, hashes, chain_before, frames }
        }
        "purge" => {
            let (before_seq, epoch) = (r.u64()?, r.u32()?);
            r.end()?;
            x.end()?;
            AdminOp::Purge { before_seq, epoch }
        }
        _ => return None,
    };
    Some(out)
}

// ---------- the families

fn run_family(file: &str, text: &str) -> Tally {
    let v: Value = serde_json::from_str(text).expect("a vector file is JSON");
    let mut tally = Tally::default();
    for c in v["cases"].as_array().expect("cases") {
        if c.get("call").is_some() || c.get("script").is_some() {
            *tally.skipped.entry("a pure-function or script case").or_default() += 1;
            continue;
        }
        let name = c["name"].as_str().expect("a name");
        let setup = &c["setup"];
        let mut w = World::new(setup);
        let boards: Vec<(String, bool)> =
            setup["boards"].as_object().into_iter().flatten().map(|(n, b)| (n.clone(), b["create"] != Value::Bool(false))).collect();
        for (bn, create) in boards {
            if create {
                let step = json!({"request": {"route": "create", "board": bn, "cred": {"create": true}, "body": {"$create": {"board": bn}}},
                    "expect": {"status": 200}});
                run_step(&mut w, &step, &mut Tally::default(), file, name, "create");
            }
        }
        for s in setup["state"].as_array().into_iter().flatten() {
            run_step(&mut w, s, &mut Tally::default(), file, name, "setup");
        }
        for (i, s) in c["steps"].as_array().expect("steps").iter().enumerate() {
            run_step(&mut w, s, &mut tally, file, name, &format!("step {i}"));
        }
    }
    tally
}

fn run_step(w: &mut World, s: &Value, tally: &mut Tally, file: &str, case: &str, at: &str) {
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| w.step(s, tally)));
    if let Err(p) = r {
        let msg = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|x| x.to_string())).unwrap_or_default();
        let st = s.to_string();
        panic!("{file} / {case} / {at}: {}\n{msg}", &st[..st.len().min(400)]);
    }
}

/// Runs one family and requires that some of its steps apply.
fn family(idx: usize) {
    let (file, text) = FAMILIES[idx];
    let t = run_family(file, text);
    println!("{file}: {} steps run; left out: {:?}", t.run, t.skipped);
    assert!(t.run > 0, "{file}: no step applies");
}

// spec: ENG-SIM-001
// spec: RELAY-SEQ-001
// spec: RELAY-SEQ-002
// spec: RELAY-SEQ-003
// spec: RELAY-SEQ-004
// spec: RELAY-SEQ-010
#[test]
fn append_family() {
    family(0);
}

// spec: ENG-SIM-001
// spec: RELAY-READ-001
// spec: RELAY-READ-002
#[test]
fn read_family() {
    family(1);
}

// spec: ENG-SIM-001
#[test]
fn blobs_family() {
    family(2);
}

// spec: ENG-SIM-001
#[test]
fn budget_family() {
    family(3);
}

// spec: ENG-SIM-001
// spec: RELAY-READ-002
#[test]
fn compact_family() {
    family(4);
}

// spec: ENG-SIM-001
#[test]
fn create_family() {
    family(5);
}

// spec: ENG-SIM-001
// spec: RELAY-SEQ-002
// spec: RELAY-SEQ-003
#[test]
fn admin_family() {
    family(6);
}

// spec: ENG-SIM-001
#[test]
fn seed_family() {
    family(7);
}

// spec: ENG-SIM-001
// spec: RELAY-SOCK-008
#[test]
fn restore_family() {
    family(8);
}

// spec: ENG-SIM-001
// spec: RELAY-READ-001
#[test]
fn purge_family() {
    family(9);
}

// spec: ENG-SIM-001
// spec: RELAY-SOCK-008
#[test]
fn socket_family() {
    family(10);
}

// ---------- the clauses `engine` co-owns, one by one, and the simulator's API

struct Fixture {
    relay: ModelRelay,
    alice: Dev,
    apk: Hash32,
    bob: Dev,
    bpk: Hash32,
}

const ALICE: Cred = Cred::Token { dev: [0xa1; 16], generation: 1 };
const BOB: Cred = Cred::Token { dev: [0xb0; 16], generation: 1 };

/// A frame of `dev` signed by `pk` with a body of `len` bytes of `fill`.
fn mk_frame(board: &[u8; 16], dev: &Dev, pk: &Hash32, ctr: u64, fill_byte: u8, len: usize) -> Vec<u8> {
    let mut hb = b"TDF1".to_vec();
    hb.extend(board);
    hb.extend(dev);
    hb.extend(ctr.to_be_bytes());
    hb.extend((len as u32).to_be_bytes());
    hb.extend(vec![fill_byte; len]);
    taskdeck_sync::sim::relay::fake_signed_frame(pk, &hb)
}

/// A board with the recovery identity at seq 1, Alice, and Bob registered
/// by Alice's `DeviceAdded` at seq 2 through the engine's request type.
fn fixture() -> Fixture {
    let rpk = fake_sign_pk(&[1; 32]);
    let salt = [2; 16];
    let id = fake_board_id(&rpk, &salt);
    let dev_r = fake_dev_r(&id, &rpk);
    let (alice, apk, bob, bpk) = ([0xa1; 16], fake_sign_pk(&[3; 32]), [0xb0; 16], fake_sign_pk(&[4; 32]));
    let mut relay = ModelRelay::new(id, RELAY_KEY);
    let g = mk_frame(&id, &dev_r, &rpk, 1, 0, 558);
    let r = relay.create(CLOCK, &rpk, &salt, &alice, &apk, &g);
    assert_eq!(r.token_gen, Some(1));
    let added = mk_frame(&id, &alice, &apk, 1, 1, 558);
    let req = taskdeck_sync::AdminRequest::Register { frame: added.clone(), dev: bob, sign_pk: bpk };
    let r = relay.admin(CLOCK, &ALICE, &alice, &req);
    let ok = r.result.expect("register");
    assert_eq!((ok.seq, ok.token, r.token_gen), (Some(2), true, Some(1)));
    assert_eq!(relay.log()[&2].frame, added);
    Fixture { relay, alice, apk, bob, bpk }
}

impl Fixture {
    fn id(&self) -> [u8; 16] {
        self.relay.board_id()
    }
    fn a(&self, ctr: u64, fill_byte: u8) -> Vec<u8> {
        mk_frame(&self.id(), &self.alice, &self.apk, ctr, fill_byte, 558)
    }
}

/// `new` at `head_seq + 1` for the next counter, its row committed with that
/// hash and chain; `dup`, `conflict` (with the stored frame, or seq 0 below
/// the purge floor), `gap`, and the stop with `skipped` after it.
// spec: RELAY-SEQ-001
// spec: RELAY-SEQ-002
// spec: RELAY-SEQ-003
// spec: RELAY-SEQ-004
// spec: RELAY-SEQ-010
#[test]
fn sequencing_rules() {
    let mut f = fixture();
    let (f2, f3) = (f.a(2, 2), f.a(3, 3));
    let r = f.relay.append(CLOCK, &ALICE, &[f2.clone(), f3.clone()]).result.expect("taken");
    let c2 = f.relay.log()[&2].chain;
    let c3 = chain_next(&c2, 3, &sha(&[&f2]));
    let c4 = chain_next(&c3, 4, &sha(&[&f3]));
    assert_eq!(r, vec![AppendResult::New { seq: 3, chain: c3 }, AppendResult::New { seq: 4, chain: c4 }]);
    for (seq, fr) in [(3, &f2), (4, &f3)] {
        let row = &f.relay.log()[&seq];
        assert_eq!((row.hash, &row.frame, row.ctr), (sha(&[fr]), fr, seq - 1), "the row behind a `new` (RELAY-SEQ-010)");
    }
    // A replay, another frame under a stored counter, a gap: nothing written.
    let before = f.relay.clone();
    let r = f.relay.append(CLOCK, &ALICE, &[f2.clone(), f.a(3, 9)]).result.expect("taken");
    assert_eq!(r, vec![AppendResult::Dup { seq: 3, chain: c3 }, AppendResult::Conflict { seq: 4, frame: f3.clone() }]);
    let r = f.relay.append(CLOCK, &ALICE, &[f.a(6, 6), f.a(4, 4)]).result.expect("taken");
    assert_eq!(r, vec![AppendResult::Gap { last_ctr: 3, last_seq: 4 }, AppendResult::Skipped]);
    assert!(f.relay.same_tables(&before), "dup, conflict and gap write nothing");
    // A stop keeps what was stored before it.
    let mut bad = f.a(5, 5);
    let n = bad.len();
    bad[n - 1] ^= 1;
    let r = f.relay.append(CLOCK, &ALICE, &[f.a(4, 4), bad, f.a(6, 6)]).result.expect("taken");
    assert!(matches!(r[0], AppendResult::New { seq: 5, .. }));
    assert_eq!(r[1], AppendResult::Refused { status: 400, code: "relay/bad_signature".into() });
    assert_eq!(r[2], AppendResult::Skipped);
    assert_eq!(f.relay.head().map(|h| h.seq), Some(5));
    // Below the purge floor, a stored counter has no frame any more: conflict with seq 0.
    let p = taskdeck_sync::AdminRequest::Purge { before_seq: 4, epoch: 2 };
    let alice = f.alice;
    f.relay.admin(CLOCK, &ALICE, &alice, &p).result.expect("purge");
    let r = f.relay.append(CLOCK, &ALICE, &[f2]).result.expect("taken");
    assert_eq!(r, vec![AppendResult::Conflict { seq: 0, frame: Vec::new() }]);
}

/// The selection of a read (at most `limit`, at most 1 MiB of frames but
/// always the first row, only at or above the purge floor) and the page fields.
// spec: RELAY-READ-001
// spec: RELAY-READ-002
#[test]
fn read_selection_and_page() {
    let mut f = fixture();
    let big = 262_144 - 112;
    let frames: Vec<Vec<u8>> = (2..7).map(|c| mk_frame(&f.id(), &f.alice, &f.apk, c, c as u8, big)).collect();
    f.relay.append(CLOCK, &ALICE, &frames).result.expect("taken");
    let p = f.relay.read(CLOCK + 5, &ALICE, 2, 256).result.expect("a page");
    assert_eq!(p.entries.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![3, 4, 5, 6], "1 MiB of frames");
    assert!(p.more && !p.truncated && !p.fenced);
    assert_eq!((p.head.seq, p.relay_now, p.floor, p.purge_floor), (7, CLOCK + 5, 1, 1));
    let p = f.relay.read(CLOCK, &ALICE, 0, 2).result.expect("a page");
    assert_eq!(p.entries.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![1, 2], "at most `limit`");
    let alice = f.alice;
    f.relay.admin(CLOCK, &ALICE, &alice, &taskdeck_sync::AdminRequest::Purge { before_seq: 6, epoch: 2 }).result.expect("purge");
    let p = f.relay.read(CLOCK, &ALICE, 0, 256).result.expect("a page");
    assert_eq!(p.entries.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![6, 7], "nothing below purge_floor");
    assert!(p.truncated && !p.more && p.floor == 6 && p.purge_floor == 6, "`truncated` exactly when after + 1 < floor");
    let p = f.relay.read(CLOCK, &ALICE, 5, 256).result.expect("a page");
    assert!(!p.truncated);
    assert_eq!(f.relay.read(CLOCK, &ALICE, 0, 0).result.map_err(|e| e.code), Err("relay/bad_query".to_string()));
}

/// After a socket append: the result first on the author's socket, then the
/// new entries pushed to every ready socket, the author's included, and to no
/// socket that has not said hello.
// spec: RELAY-SOCK-008
#[test]
fn pushes_go_to_every_ready_socket() {
    let mut f = fixture();
    let a = f.relay.socket_open(&ALICE).expect("upgrade");
    let b = f.relay.socket_open(&BOB).expect("upgrade");
    let quiet = f.relay.socket_open(&BOB).expect("upgrade");
    assert!(matches!(f.relay.socket_hello(CLOCK, a, [1; 16], 0).as_slice(), [o] if matches!(o.msg, SockMsg::Welcome(_))));
    let out = f.relay.socket_hello(CLOCK, b, [2; 16], 0);
    assert!(matches!(out.last().map(|o| &o.msg), Some(SockMsg::Welcome(w)) if w.last_ctr == Some(0) && w.head.seq == 2));
    let frames = vec![f.a(2, 2), f.a(3, 3)];
    let out = f.relay.socket_append(CLOCK, a, 7, &frames);
    assert!(matches!(&out[0].msg, SockMsg::Result { req: 7, results } if results.len() == 2), "the result first");
    let pushed: Vec<(SocketId, Vec<u64>)> = out[1..]
        .iter()
        .map(|o| match &o.msg {
            SockMsg::Push(p) => (o.socket, p.entries.iter().map(|e| e.seq).collect()),
            m => panic!("a push, not {m:?}"),
        })
        .collect();
    assert_eq!(pushed, vec![(a, vec![3, 4]), (b, vec![3, 4])]);
    assert!(out.iter().all(|o| o.socket != quiet));
    // A second process of Bob: `second` to it, `displaced` and close 4001 to the first.
    let out = f.relay.socket_hello(CLOCK, quiet, [3; 16], 0);
    let msgs: Vec<&SockMsg> = out.iter().map(|o| &o.msg).collect();
    assert!(matches!(msgs.as_slice(), [SockMsg::Displaced(s), SockMsg::Close(4001), SockMsg::Second(t), SockMsg::Welcome(_)]
        if *s == [3; 16] && *t == [2; 16]));
    assert_eq!(out[1].socket, b);
}

/// What the simulator's faults use: a clone to roll back to (sockets kept),
/// an injected frame stored as it is and pushed, without touching counters.
// spec: ENG-SIM-001
#[test]
fn rollback_and_injected_frames() {
    let mut f = fixture();
    let a = f.relay.socket_open(&ALICE).expect("upgrade");
    f.relay.socket_hello(CLOCK, a, [1; 16], 0);
    let earlier = f.relay.clone();
    f.relay.append(CLOCK, &ALICE, &[f.a(2, 2)]).result.expect("taken");
    assert_eq!(f.relay.head().map(|h| h.seq), Some(3));
    f.relay.rollback_to(&earlier);
    assert_eq!(f.relay.head(), earlier.head());
    assert_eq!(f.relay.sockets().len(), 1, "the sockets stay");
    let mut forged = f.a(2, 9);
    let n = forged.len();
    forged[n - 1] ^= 1;
    let (h, out) = f.relay.inject_frame(CLOCK, forged.clone()).expect("a board");
    assert_eq!(h.seq, 3);
    assert!(matches!(&out[0].msg, SockMsg::Push(p) if p.entries[0].frame == forged));
    let r = f.relay.append(CLOCK, &ALICE, &[f.a(2, 2)]).result.expect("taken");
    assert!(matches!(r[0], AppendResult::New { seq: 4, .. }), "the author's counter is untouched");
    let other = mk_frame(&[9; 16], &f.alice, &f.apk, 3, 3, 558);
    assert_eq!(f.relay.inject_frame(CLOCK, other).map(|x| x.0.seq), Some(5));
}

/// The engine's blob requests: an upload in parts, then fetch, list and meta.
// spec: ENG-SIM-001
#[test]
fn engine_blob_requests() {
    use taskdeck_sync::{BlobOk, BlobRequest, SealedBlob};
    let mut f = fixture();
    let bytes: Vec<u8> = (0..1_048_576 + 100).map(|i| (i % 251) as u8).collect();
    let blob = SealedBlob { id: [7; 16], sha256: sha(&[&bytes]), bytes: bytes.clone(), parts: 2, inflated: 0, epoch: 1 };
    let up = f.relay.blob(CLOCK, &ALICE, &BlobRequest::Upload { purpose: 4, seq: 0, blob });
    assert_eq!(up.result, Ok(BlobOk::Uploaded));
    let got = f.relay.blob(CLOCK, &BOB, &BlobRequest::Fetch { id: [7; 16], parts: 2 }).result.expect("parts");
    assert_eq!(got, BlobOk::Parts(vec![bytes[..1_048_576].to_vec(), bytes[1_048_576..].to_vec()]));
    let Ok(BlobOk::List(l)) = f.relay.blob(CLOCK, &BOB, &BlobRequest::List).result else { panic!("a list") };
    assert_eq!((l.len(), l[0].author, l[0].complete, l[0].size), (1, f.alice, true, bytes.len() as u64));
    let Ok(BlobOk::Meta(m)) = f.relay.blob(CLOCK, &BOB, &BlobRequest::Meta { id: [7; 16] }).result else { panic!("meta") };
    assert_eq!(m.parts, 2);
    let missing = f.relay.blob(CLOCK, &BOB, &BlobRequest::Meta { id: [8; 16] }).result;
    assert_eq!(missing.map_err(|e| (e.status, e.code)), Err((404, "relay/no_blob".to_string())));
    let _ = f.bpk;
    let _ = f.bob;
}
