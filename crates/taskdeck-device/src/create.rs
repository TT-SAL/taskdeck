//! Creating a board (device.md §18.1) and persisting a join (§18.3): the
//! keys and the genesis frame drawn at `begin_create`, the sheet and its
//! retyping with the test backup, the inactive identity directory, the
//! relay's `create`, the token, the activation and the reopen in synced
//! mode; and, for a joining device, its identity directory from the
//! pairing machine's `Joined`.
//!
//! Only `CreateSource::Empty` is built (finding f1-5: a genesis snapshot of
//! a non-empty board cannot be sealed yet).

use std::sync::Arc;
use std::time::Duration;

use subtle::ConstantTimeEq;
use taskdeck_core::board::BoardData;
use taskdeck_core::ids::IdMinter;
use taskdeck_seal::pair::Bundle;
use taskdeck_seal::{
    BackupContent, BackupInput, BoardId, DeviceKeys, Entropy, EpochRoot, GenesisAnchor, HistoryRecord, Membership,
    OsEntropy, ReceiptInput, RecoveryPublic, RecoverySeed, SnapshotBody,
};
use taskdeck_sync::{Bootstrap, RelayTarget, SnapshotMaterial, StateMachine};
use zeroize::Zeroizing;

use crate::adapt::{GenesisInput, SealSealer};
use crate::net::{Auth, Fail, Job, Method, Req, Resp, wire};
use crate::runtime::{Call, Runtime, ask, err};
use crate::store::{self, DeviceSettings, FolderState, NewIdentity, RelayToken, StoreDescriptor};
use crate::strict_json::{Obj, hex};
use crate::{CoreMachine, CreateRequest, CreateSource, CreatedBoard, DeviceError, Mode, SheetData};

/// What a create flow holds between its calls (device.md §18.1): every
/// secret in a type that erases itself on drop (DEV-SHEET-001).
pub(crate) struct Flow {
    pub(crate) inner: Arc<crate::Inner>,
    host: String,
    create_secret: Zeroizing<String>,
    seed: RecoverySeed,
    salt: [u8; 16],
    /// A copy of the device keys the sealer took: the test backup's writer.
    keys: DeviceKeys,
    root1: EpochRoot,
    l_d: Zeroizing<[u8; 32]>,
    board: [u8; 16],
    dev: [u8; 16],
    door: [u8; 5],
    genesis: Vec<u8>,
    frame_1: Vec<u8>,
    identity: Zeroizing<Vec<u8>>,
    name: String,
    at_ms: i64,
    confirmed: bool,
    recovery: RecoveryPublic,
}

/// The inactive identity directory of §18.1 step 7.
pub(crate) struct Persist {
    board: [u8; 16],
    dev: [u8; 16],
    created_ms: i64,
    identity: Zeroizing<Vec<u8>>,
    settings: DeviceSettings,
    bootstrap: Bootstrap,
    l_d: Zeroizing<[u8; 32]>,
}

fn se(e: taskdeck_seal::SealError) -> DeviceError {
    err(e.code())
}

fn copy_keys(k: &DeviceKeys) -> Result<DeviceKeys, DeviceError> {
    DeviceKeys::from_identity_bytes(&k.to_identity_bytes()).map_err(se)
}

/// A request through `taskdeck-https`, waited for on the caller's thread.
pub(crate) fn net_call(inner: &crate::Inner, req: Req) -> Result<Resp, Fail> {
    let (tx, rx) = std::sync::mpsc::channel();
    let reply = Box::new(move |r: Result<Resp, Fail>| {
        let _ = tx.send(r);
    });
    if !inner.net.submit(Job::Raw { req, reply }) {
        return Err(Fail::Unreachable);
    }
    rx.recv_timeout(Duration::from_secs(60)).unwrap_or(Err(Fail::Unreachable))
}

/// A refusal answer as the facade's error (its code and HTTP status).
fn relay_error(r: &Result<Resp, Fail>) -> DeviceError {
    match r {
        Err(Fail::Insecure) => err("device/insecure_url"),
        Err(Fail::Unreachable) => err("device/create_failed"),
        Ok(resp) => match wire::parse_refusal(resp.status, &resp.body) {
            Some(rf) => DeviceError { code: relay_code(&rf.code), status: Some(rf.status), message: None },
            None => DeviceError { code: "device/create_failed", status: Some(resp.status), message: None },
        },
    }
}

/// The codes `create` and `door` can answer (relay.md §10.2, §10.9, §19).
fn relay_code(c: &str) -> &'static str {
    const KNOWN: &[&str] = &[
        "relay/create_disabled",
        "relay/unauthorized",
        "relay/exists",
        "relay/bad_genesis",
        "relay/bad_body",
        "relay/not_found",
    ];
    KNOWN.iter().find(|k| **k == c).copied().unwrap_or("device/create_failed")
}

/// The relay host of a `CreateRequest` (§18.1 step 1): `https://` may lead;
/// with `test-trust`, `http://` may lead a loopback host (§11.1 item 1);
/// then relay.md §5.4 step 4 and seal.md §5.4.
pub(crate) fn relay_host(text: &str) -> Result<String, DeviceError> {
    let t = text.trim();
    let t = match t.get(..8) {
        Some(p) if p.eq_ignore_ascii_case("https://") => &t[8..],
        _ => {
            #[cfg(feature = "test-trust")]
            if let Some(rest) = t.get(..7).filter(|p| p.eq_ignore_ascii_case("http://")).map(|_| &t[7..]) {
                let h = wire::normalise_host(rest.trim_end_matches('/')).map_err(err)?;
                return if wire::is_loopback(&h) { Ok(h) } else { Err(err("device/insecure_url")) };
            }
            if t.contains("://") {
                return Err(err("device/insecure_url"));
            }
            t
        }
    };
    wire::normalise_host(t.trim_end_matches('/')).map_err(err)
}

/// `begin_create` (§18.1 steps 1 to 4).
pub(crate) fn begin(inner: &Arc<crate::Inner>, request: CreateRequest) -> Result<Flow, DeviceError> {
    // 1.
    if !inner.desktop {
        return Err(err("device/not_on_this_platform"));
    }
    if inner.shared.view().mode == Mode::Synced {
        return Err(err("device/wrong_mode"));
    }
    if !matches!(request.source, CreateSource::Empty) {
        // Finding f1-5: only an empty board can be created today.
        return Err(err("device/legacy_unreadable"));
    }
    let host = relay_host(&request.relay_host)?;
    let secret = request.create_secret;
    if secret.len() != 64 || !secret.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return Err(err("device/create_secret"));
    }
    // 2. The genesis board.
    let sm = CoreMachine::new();
    let board_data = BoardData::empty();
    let genesis = sm.canonical_bytes(&board_data);
    // 3. Keys first, then the door.
    let seed = RecoverySeed::generate(&mut OsEntropy).map_err(se)?;
    let recovery = seed.keys();
    let mut salt = [0u8; 16];
    OsEntropy.fill(&mut salt).map_err(se)?;
    let keys = DeviceKeys::generate(&mut OsEntropy).map_err(se)?;
    let mut l_d = Zeroizing::new([0u8; 32]);
    OsEntropy.fill(&mut l_d[..]).map_err(se)?;
    let root1 = EpochRoot::generate(&mut OsEntropy).map_err(se)?;
    let rp = recovery.public();
    let board = taskdeck_seal::board_id(&rp.sign_pk, &salt);
    let dev = keys.public().dev.0;
    let req = Req {
        method: Method::Get,
        host: host.clone(),
        path: format!("/v1/b/{}/door", hex(&board.0)),
        auth: Auth::Create(secret.clone()),
        proof: None,
        body: None,
        limit: wire::JSON_MAX,
    };
    let r = net_call(inner, req);
    let door = match &r {
        Ok(resp) if resp.status == 200 => wire::json_obj(&resp.body)
            .and_then(|mut o: Obj| {
                let d: [u8; 5] = o.hex("door")?;
                o.finish()?;
                Some(d)
            })
            .ok_or_else(|| err("device/bad_answer"))?,
        _ => return Err(relay_error(&r)),
    };
    // 4. Seq 1.
    let at_ms = inner.clock.wall_ms();
    let writer = copy_keys(&keys)?;
    let (sealer, frame_1) = SealSealer::create(GenesisInput {
        recovery: &recovery,
        salt,
        keys,
        root1: EpochRoot::from_bytes(*root1.to_bytes()),
        device_name: &request.device_name,
        at_ms,
        board_hash: sm.board_hash(&board_data),
    })
    .map_err(|e| err(e.code))?;
    let identity = taskdeck_sync::Sealer::export_identity(&sealer);
    drop(sealer);
    Ok(Flow {
        inner: inner.clone(),
        host,
        create_secret: secret,
        seed,
        salt,
        keys: writer,
        root1,
        l_d,
        board: board.0,
        dev,
        door,
        genesis,
        frame_1,
        identity,
        name: request.device_name,
        at_ms,
        confirmed: false,
        recovery: rp,
    })
}

impl Flow {
    /// §17.10 `CreateFlow::sheet`.
    pub(crate) fn sheet(&self) -> SheetData {
        let date = chrono::DateTime::from_timestamp_millis(self.at_ms).map_or_else(
            || chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap_or_default(),
            |d| d.date_naive(),
        );
        SheetData {
            seed_groups: self.seed.to_sheet(),
            relay_address: wire::address_text(&self.host, &self.board, &self.door),
            recovery_fp: taskdeck_seal::key_fingerprint(&self.recovery.sign_pk, &self.recovery.hpke_pk),
            // The release-signing key is stage `release`'s; none exists yet.
            release_fp: String::new(),
            date,
        }
    }

    /// §18.1 step 6: the typed sheet decodes to the held seed (constant
    /// time), then the test backup opens with the retyped seed's keys.
    pub(crate) fn confirm_sheet(&mut self, typed: &str) -> Result<(), DeviceError> {
        let retyped = RecoverySeed::from_sheet(typed).map_err(se)?;
        let a = self.seed.to_sheet();
        let b = retyped.to_sheet();
        if !bool::from(a.as_bytes().ct_eq(b.as_bytes())) {
            return Err(err("device/sheet_mismatch"));
        }
        self.test_backup(&retyped).map_err(|_| err("device/sheet_test_failed"))?;
        self.confirmed = true;
        Ok(())
    }

    /// A nightly backup of the genesis state written in memory to the
    /// recovery key with no read token, and read back (§18.1 step 6, V1-17).
    fn test_backup(&self, retyped: &RecoverySeed) -> Result<(), DeviceError> {
        let board = BoardId(self.board);
        let zero = crate::adapt::membership_zero().ok_or_else(|| err("device/sheet_test_failed"))?;
        let m0 = Membership::from_canonical_bytes(&zero).map_err(se)?;
        let root1 = EpochRoot::from_bytes(*self.root1.to_bytes());
        let anchor = GenesisAnchor { rsign_pk: self.recovery.sign_pk, salt: self.salt, root1: EpochRoot::from_bytes(*self.root1.to_bytes()) };
        let roots = |e: u32| (e == 1).then_some(&root1);
        let chain_at = |_: u64| None;
        let none = |_: u64| -> Option<&[u8]> { None };
        let input = ReceiptInput {
            seq: 1,
            frame: &self.frame_1,
            prev_chain: taskdeck_seal::chain_zero(&board),
            membership: &m0,
            roots: &roots,
            chain_at: &chain_at,
            retained_from: 1,
            own: None,
            supported_rules: &[1],
            genesis: Some(&anchor),
            skip_targets: &none,
        };
        let r = taskdeck_seal::classify(&input).map_err(se)?;
        let opened = match (&r.class, &r.opened) {
            (taskdeck_seal::Class::Accept, Some(o)) => o,
            _ => return Err(err("device/sheet_test_failed")),
        };
        let history = [HistoryRecord::from_accepted(1, &self.frame_1, opened).map_err(se)?];
        let m1 = r.membership.to_canonical_bytes();
        let sm = CoreMachine::new();
        let board_hash = {
            use sha2::Digest;
            let h: [u8; 32] = sha2::Sha256::digest(&self.genesis).into();
            h
        };
        let _ = sm;
        let content = BackupContent::Nightly {
            membership: m1.clone(),
            board: self.genesis.clone(),
            head: taskdeck_seal::Head { seq: 1, chain: r.chain },
            hash_version: 1,
            state_hash: taskdeck_sync::state_hash(1, &board_hash, &m1),
        };
        let file = taskdeck_seal::write_backup(
            &self.keys,
            BackupInput {
                board,
                seq: 1,
                written_ms: self.at_ms,
                history: &history,
                content,
                roots: vec![(1, EpochRoot::from_bytes(*self.root1.to_bytes()))],
                read_token: None,
                meta: b"{}".to_vec(),
                pinned_recovery: RecoveryPublic { sign_pk: self.recovery.sign_pk, hpke_pk: self.recovery.hpke_pk },
            },
            &mut OsEntropy,
        )
        .map_err(se)?;
        taskdeck_seal::read_backup(&file, &board, &retyped.keys()).map_err(se)?;
        Ok(())
    }

    /// §18.1 steps 7 to 10.
    pub(crate) fn finish(self) -> Result<CreatedBoard, DeviceError> {
        if !self.confirmed {
            // DEV-CREATE-002: nothing is sent before the sheet was retyped.
            return Err(err("device/sheet_mismatch"));
        }
        let inner = self.inner.clone();
        // 7.
        let mut settings = DeviceSettings::new(&self.name);
        settings.history_complete = true;
        settings.minter = Some(IdMinter::new(1));
        settings.other.insert(
            "backup".into(),
            serde_json::json!({
                "pinned_recovery_fp": taskdeck_seal::key_fingerprint(&self.recovery.sign_pk, &self.recovery.hpke_pk),
                "pinned_recovery_pk": { "hpke_pk": hex(&self.recovery.hpke_pk), "sign_pk": hex(&self.recovery.sign_pk) },
            }),
        );
        let relay = RelayTarget { host: self.host.clone(), door: self.door, home_gen: 1 };
        let persist = Persist {
            board: self.board,
            dev: self.dev,
            created_ms: self.at_ms,
            identity: self.identity.clone(),
            settings,
            bootstrap: Bootstrap::Genesis { board: self.genesis.clone(), frame_1: self.frame_1.clone(), relay },
            l_d: self.l_d.clone(),
        };
        ask(&inner.tx, |reply| Call::CreatePersist { new: Box::new(persist), reply })??;
        // 8. `create`, retried when no answer came (the relay's Repeat).
        let rp = &self.recovery;
        let pk = self.keys.public();
        let mut body = Vec::with_capacity(100 + self.frame_1.len());
        body.extend_from_slice(&rp.sign_pk);
        body.extend_from_slice(&self.salt);
        body.extend_from_slice(&pk.dev.0);
        body.extend_from_slice(&pk.sign_pk);
        body.extend_from_slice(&(self.frame_1.len() as u32).to_be_bytes());
        body.extend_from_slice(&self.frame_1);
        let mut answer = Err(Fail::Unreachable);
        for attempt in 0..3 {
            if attempt > 0 {
                std::thread::sleep(Duration::from_millis(1_000));
            }
            let req = Req {
                method: Method::Post,
                host: self.host.clone(),
                path: format!("/v1/b/{}/create", hex(&self.board)),
                auth: Auth::Create(self.create_secret.clone()),
                proof: None,
                body: Some(body.clone()),
                limit: wire::JSON_MAX,
            };
            answer = net_call(&inner, req);
            if !matches!(answer, Err(Fail::Unreachable)) {
                break;
            }
        }
        let token = match &answer {
            Ok(resp) if resp.status == 200 => wire::json_obj(&resp.body)
                .and_then(|mut o| {
                    let seq = o.u53("seq")?;
                    let _chain: [u8; 32] = o.hex("chain")?;
                    let token = Zeroizing::new(o.string("token")?);
                    let door: [u8; 5] = o.hex("door")?;
                    o.finish()?;
                    (seq == 1 && door == self.door && wire::is_token(&token)).then_some(token)
                })
                .ok_or_else(|| err("device/create_failed"))?,
            _ => return Err(relay_error(&answer)),
        };
        // 9, 10.
        let token = RelayToken { host: self.host.clone(), generation: 1, push: None, token };
        ask(&inner.tx, |reply| Call::CreateActivate { token, reply })??;
        Ok(CreatedBoard { address: wire::address_text(&self.host, &self.board, &self.door) })
    }
}

/// §18.1 step 7, on the sync thread.
pub(crate) fn persist(rt: &mut Runtime, p: Persist) -> Result<(), DeviceError> {
    if rt.synced.is_some() || rt.pending_create.is_some() {
        return Err(err("device/wrong_mode"));
    }
    rt.lock_ok()?;
    let descriptor = StoreDescriptor { board_id: p.board, dev: p.dev, key_store: rt.key_store.kind(), created_ms: p.created_ms };
    let new = NewIdentity {
        descriptor,
        identity: &p.identity,
        settings: p.settings,
        bootstrap: &p.bootstrap,
        genesis_blob: None,
        history: &[],
    };
    let store = store::create_identity(rt.files.clone(), &rt.data_dir, &*rt.key_store, &p.l_d, new)?;
    rt.pending_create = Some(store);
    Ok(())
}

/// §18.1 steps 9 and 10, on the sync thread: the token durable, then
/// `active.tds`, then the reopen in synced mode (DEV-CREATE-003).
pub(crate) fn activate(rt: &mut Runtime, token: RelayToken) -> Result<(), DeviceError> {
    let Some(mut store) = rt.pending_create.take() else { return Err(err("device/wrong_mode")) };
    let mut settings = store.settings().clone();
    settings.put_token(token);
    store.write_settings(settings)?;
    let d = store::read_store_descriptor(&*rt.files, store.dir())?;
    drop(store);
    store::activate(&*rt.files, &rt.data_dir, &d.board_id, &d.dev)?;
    rt.reopen()
}

/// Persisting a join (§18.3), on the sync thread, after the machine's
/// `Joined`: `L_d`, the identity directory with `history.tdk` before
/// `bootstrap.tdk` (DEV-PAIR-006), `active.tds`, and the reopen with
/// `Bootstrap::Join`. The plain files of a local-only board are not set
/// aside yet (§18.4 is not built).
pub(crate) fn persist_join(
    rt: &mut Runtime,
    device_no: u16,
    keys: DeviceKeys,
    bundle: Bundle,
    snapshot: SnapshotBody,
    name: &str,
) -> Result<(), DeviceError> {
    rt.lock_ok()?;
    let board = bundle.board_id.0;
    let dev = keys.public().dev.0;
    let identity = SealSealer::join_identity(board, &keys, &bundle.roots, &snapshot.membership).map_err(|e| err(e.code))?;
    drop(keys);
    let mut l_d = Zeroizing::new([0u8; 32]);
    OsEntropy.fill(&mut l_d[..]).map_err(se)?;
    let mut settings = DeviceSettings::new(name);
    settings.history_complete = true;
    settings.minter = Some(IdMinter::new(device_no));
    settings.put_token(RelayToken { host: bundle.host.clone(), generation: 1, push: None, token: Zeroizing::new(bundle.token.clone()) });
    let history: Vec<(u64, Vec<u8>)> = snapshot.history.iter().map(|h| (h.seq(), h.to_bytes())).collect();
    let home_gen = crate::adapt::home_gen_of(&snapshot.membership).ok_or_else(|| err("seal/bundle_invalid"))?;
    let bootstrap = Bootstrap::Join {
        material: SnapshotMaterial {
            seq: snapshot.seq,
            chain: snapshot.chain.0,
            hash_version: snapshot.hash_version,
            state_hash: snapshot.state_hash,
            board: snapshot.board.clone(),
            membership: snapshot.membership.clone(),
        },
        added_seq: bundle.added_seq,
        relay: RelayTarget { host: bundle.host.clone(), door: bundle.door, home_gen },
    };
    let descriptor = StoreDescriptor { board_id: board, dev, key_store: rt.key_store.kind(), created_ms: rt.wall_ms() };
    let new = NewIdentity { descriptor, identity: &identity, settings, bootstrap: &bootstrap, genesis_blob: None, history: &history };
    let store = store::create_identity(rt.files.clone(), &rt.data_dir, &*rt.key_store, &l_d, new)?;
    drop(store);
    store::activate(&*rt.files, &rt.data_dir, &board, &dev)?;
    rt.reopen()
}

impl Runtime {
    /// Reads the active identity again and starts synced mode (§5.4, §18.1
    /// step 10, §18.3), raising `status` and `view` (DEV-FACADE-009).
    pub(crate) fn reopen(&mut self) -> Result<(), DeviceError> {
        let now = self.wall_ms();
        let state = store::open_folder(
            self.files.clone(),
            &self.data_dir,
            &self.lock,
            self.cloud,
            &*self.key_store,
            self.build_id(),
            now,
        )?;
        match state {
            FolderState::Synced(o) => {
                self.start_synced(*o)?;
                self.mark_mode();
                Ok(())
            }
            _ => Err(err("device/store_corrupt")),
        }
    }

    fn lock_ok(&self) -> Result<(), DeviceError> {
        match self.lock.refusal() {
            Some(e) => Err(e),
            None if self.cloud => Err(err("device/cloud_folder")),
            None => Ok(()),
        }
    }
}
