//! `taskdeck-devicectl`'s protocol (device.md §19), in the library so that
//! it shares the strict JSON parser and is tested here; the binary only
//! calls `main`. Built only with `test-trust` (DEV-CTL-001).
//!
//! Commands built: `status`, `board`, `submit`, `create`, `sheet`,
//! `confirm_sheet`, `finish_create`, `pair_start`, `pair_status`,
//! `pair_confirm`, `pair_cancel`, `join`, `join_sas`, `wait`, `clock`,
//! `shutdown`, `kill`. Every other command of the table is answered with its
//! id and `device/ctl_request` until its facade call exists (a shortcut,
//! `fit.md`).

use std::io::{BufRead, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{Map, Value, json};
use zeroize::Zeroizing;

use crate::strict_json::{self, hex};
use crate::{
    CreateFlow, CreateRequest, CreateSource, Device, DeviceError, DeviceStatus, FileKeyStore, Mode, OpenOptions,
    PairState, Platform, TestClock, TrustRoots, ZoneSource,
};

/// Lines are at most 16,777,216 bytes (§19.2).
const LINE_MAX: usize = 16_777_216;
const CTL_REQUEST: &str = "device/ctl_request";
/// The manual clock starts at 2026-01-05T12:00:00Z (§19.1).
pub const MANUAL_START_MS: i64 = 1_767_614_400_000;
const MAX_INT: u64 = (1 << 53) - 1;

/// The manual clock of `--clock manual` (§19.1): moved only by `clock`.
/// `advance_ms` moves `wall_ms` and `mono_ms` together; `set_ms` sets
/// `wall_ms` alone, so that a test can put the clock beside the relay's
/// without the device seeing months of monotonic time pass (finding f3-*).
pub struct ManualClock {
    state: Mutex<(i64, u64)>,
}

impl ManualClock {
    pub fn new(wall_ms: i64) -> ManualClock {
        ManualClock { state: Mutex::new((wall_ms, 1)) }
    }

    fn set(&self, wall_ms: i64) -> i64 {
        let mut g = match self.state.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        g.0 = wall_ms;
        g.0
    }

    fn advance(&self, ms: u64) -> i64 {
        let mut g = match self.state.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        g.0 = g.0.saturating_add(i64::try_from(ms).unwrap_or(i64::MAX));
        g.1 = g.1.saturating_add(ms);
        g.0
    }
}

impl TestClock for ManualClock {
    fn wall_ms(&self) -> i64 {
        self.state.lock().map_or(0, |g| g.0)
    }
    fn mono_ms(&self) -> u64 {
        self.state.lock().map_or(0, |g| g.1)
    }
}

/// The options of `run` (§19.1).
pub struct RunOptions {
    pub data: PathBuf,
    pub keys: PathBuf,
    pub platform: Platform,
    pub name: String,
    pub zone: ZoneSource,
    pub manual: bool,
    pub build_id: String,
}

/// Parses the arguments after `run`.
pub fn parse_run(args: &[String]) -> Result<RunOptions, String> {
    let mut o = RunOptions {
        data: PathBuf::new(),
        keys: PathBuf::new(),
        platform: Platform::Linux,
        name: "devicectl".into(),
        zone: ZoneSource::Table(taskdeck_core::time::TableZone { initial: 0, transitions: Vec::new() }),
        manual: true,
        build_id: "devicectl".into(),
    };
    let (mut data, mut keys) = (None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = || it.next().cloned().ok_or_else(|| format!("{a} needs a value"));
        match a.as_str() {
            "--data" => data = Some(PathBuf::from(val()?)),
            "--keys" => keys = Some(PathBuf::from(val()?)),
            "--name" => o.name = val()?,
            "--build-id" => o.build_id = val()?,
            "--platform" => {
                let v = val()?;
                o.platform = match v.as_str() {
                    "windows" => Platform::Windows,
                    "macos" => Platform::MacOs,
                    "linux" => Platform::Linux,
                    _ => match v.strip_prefix("android:").and_then(|n| n.parse().ok()) {
                        Some(api) => Platform::Android { api },
                        None => return Err(format!("unknown platform {v}")),
                    },
                }
            }
            "--zone" => {
                let v = val()?;
                o.zone = if v == "utc" {
                    ZoneSource::Table(taskdeck_core::time::TableZone { initial: 0, transitions: Vec::new() })
                } else if let Some(s) = v.strip_prefix("fixed:").and_then(|n| n.parse::<i32>().ok()) {
                    let z = taskdeck_core::time::TableZone::new(s, Vec::new()).map_err(|_| format!("bad zone {v}"))?;
                    ZoneSource::Table(z)
                } else {
                    return Err(format!("zone {v} is not built"));
                }
            }
            "--clock" => {
                o.manual = match val()?.as_str() {
                    "manual" => true,
                    "system" => false,
                    v => return Err(format!("unknown clock {v}")),
                }
            }
            "--trust-pem" | "--crash-at" => return Err(format!("{a} is not built yet")),
            _ => return Err(format!("unknown option {a}")),
        }
    }
    o.data = data.ok_or("--data is required")?;
    o.keys = keys.ok_or("--keys is required")?;
    Ok(o)
}

/// The output: one JSON object per line, never interleaved.
#[derive(Clone)]
struct Out(Arc<Mutex<std::io::Stdout>>);

impl Out {
    fn line(&self, v: &Value) {
        let mut s = v.to_string();
        s.push('\n');
        if let Ok(mut o) = self.0.lock() {
            let _ = o.write_all(s.as_bytes());
            let _ = o.flush();
        }
    }
}

/// `run` (§19.1): opens the facade, then answers requests until `shutdown`,
/// `kill` or the end of the input. Returns the exit code.
pub fn run(o: RunOptions) -> i32 {
    let clock = Arc::new(ManualClock::new(MANUAL_START_MS));
    let options = OpenOptions {
        data_dir: o.data,
        platform: o.platform,
        key_store: Arc::new(FileKeyStore::new(o.keys)),
        zone: o.zone,
        trust: TrustRoots::Platform,
        build_id: o.build_id,
        default_device_name: o.name,
        clock: o.manual.then(|| clock.clone() as Arc<dyn TestClock>),
        files: None,
    };
    let out = Out(Arc::new(Mutex::new(std::io::stdout())));
    let device = match Device::open(options) {
        Ok((d, _)) => d,
        Err(e) => {
            eprintln!("open failed: {}", e.code);
            out.line(&json!({"event": "exit", "code": 1}));
            return 1;
        }
    };
    // `changed` events after each publish (§19.2).
    let stop = Arc::new(AtomicBool::new(false));
    let events = {
        let (d, out, stop) = (device.clone(), out.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut last = d.changes().any;
            while !stop.load(Ordering::Relaxed) {
                let c = d.wait_for_change(last, 100);
                if c.any > last && !stop.load(Ordering::Relaxed) {
                    last = c.any;
                    out.line(&json!({"event": "changed", "changes": changes_json(&c)}));
                }
            }
        })
    };
    let mut ctl = Ctl { device, clock: o.manual.then_some(clock), flow: None };
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut buf = Vec::new();
    let code = loop {
        buf.clear();
        match (&mut input).take(LINE_MAX as u64 + 1).read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => {
                // The input ended: shut down as `shutdown` would, unanswered.
                let r = ctl.device.shutdown();
                break i32::from(r.is_err());
            }
            Ok(_) => {}
        }
        if buf.last() == Some(&b'\n') {
            buf.pop();
        } else if buf.len() > LINE_MAX {
            out.line(&json!({"id": 0, "ok": false, "error": CTL_REQUEST}));
            // Skip the rest of the overlong line.
            let mut rest = Vec::new();
            let _ = input.read_until(b'\n', &mut rest);
            continue;
        }
        match ctl.line(&buf) {
            Step::Answer(v) => out.line(&v),
            Step::Kill => std::process::abort(),
            Step::Exit(v, code) => {
                out.line(&v);
                break code;
            }
        }
    };
    stop.store(true, Ordering::Relaxed);
    let _ = events.join();
    out.line(&json!({"event": "exit", "code": code}));
    code
}

/// What one request line comes to.
pub enum Step {
    Answer(Value),
    Exit(Value, i32),
    Kill,
}

pub struct Ctl {
    pub device: Device,
    clock: Option<Arc<ManualClock>>,
    flow: Option<CreateFlow>,
}

fn refused(id: u64, code: &str) -> Value {
    json!({"id": id, "ok": false, "error": code})
}

fn device_error(id: u64, e: &DeviceError) -> Value {
    let mut m = Map::new();
    m.insert("id".into(), json!(id));
    m.insert("ok".into(), json!(false));
    m.insert("error".into(), json!(e.code));
    if let Some(s) = e.status {
        m.insert("status".into(), json!(s));
    }
    if let Some(msg) = &e.message {
        m.insert("message".into(), json!(msg));
    }
    Value::Object(m)
}

/// The answer of a result (DEV-CTL-003): its members beside `id` and `ok`.
fn ok(id: u64, result: Map<String, Value>) -> Value {
    let mut m = Map::new();
    m.insert("id".into(), json!(id));
    m.insert("ok".into(), json!(true));
    for (k, v) in result {
        m.insert(k, v);
    }
    Value::Object(m)
}

fn obj(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

/// The arguments of one request: each read once, the rest refused.
struct Args(Map<String, Value>);

impl Args {
    fn string(&mut self, k: &str) -> Option<String> {
        match self.0.remove(k)? {
            Value::String(s) => Some(s),
            _ => None,
        }
    }
    fn u53(&mut self, k: &str) -> Option<u64> {
        self.0.remove(k)?.as_u64().filter(|n| *n <= MAX_INT)
    }
    fn i53(&mut self, k: &str) -> Option<i64> {
        self.0.remove(k)?.as_i64().filter(|n| n.unsigned_abs() <= MAX_INT)
    }
    fn value(&mut self, k: &str) -> Option<Value> {
        self.0.remove(k)
    }
    fn done(&self) -> bool {
        self.0.is_empty()
    }
}

/// The commands of §19.2's table this build does not answer yet.
const NOT_BUILT: &[&str] = &[
    "snapshot",
    "submit_named",
    "release_hold",
    "notices",
    "dismiss_notice",
    "ack_banner",
    "dismiss_alarm",
    "confirm_clock",
    "confirm_rejoin",
    "use_this_copy",
    "ack_continuity",
    "ack_restored_copy",
    "ack_based_on_unchecked",
    "adopt",
    "skip",
    "restore_to",
    "restore_targets",
    "purge_preview",
    "purge",
    "revoke",
    "rules_override",
    "move_board",
    "move_status",
    "enter_move_code",
    "follow_address",
    "wait_op",
    "resume_create",
    "abandon_create",
    "re_pair",
    "begin_recovery_change",
    "finish_recovery_change",
    "confirm_current_sheet",
    "reset_trust",
    "backup_now",
    "backup_status",
    "pin_fp",
    "set_destinations",
    "delete_backups_before",
    "set_picture",
    "set_foreground",
    "resume",
    "background_sync",
    "fetch_now",
    "export_legacy",
];

impl Ctl {
    pub fn new(device: Device, clock: Option<Arc<ManualClock>>) -> Ctl {
        Ctl { device, clock, flow: None }
    }

    /// One request line (§19.2, DEV-CTL-004): a line that is not a strict
    /// JSON object with a valid `id` is answered with id 0; a valid `id`
    /// with an unknown `cmd` or bad arguments with that id; neither calls
    /// anything.
    pub fn line(&mut self, bytes: &[u8]) -> Step {
        let Some(Value::Object(mut m)) = strict_json::parse_bytes(bytes, 64) else {
            return Step::Answer(refused(0, CTL_REQUEST));
        };
        let Some(id) = m.remove("id").and_then(|v| v.as_u64()).filter(|n| *n <= MAX_INT) else {
            return Step::Answer(refused(0, CTL_REQUEST));
        };
        let Some(Value::String(cmd)) = m.remove("cmd") else {
            return Step::Answer(refused(id, CTL_REQUEST));
        };
        self.command(id, &cmd, Args(m))
    }

    fn command(&mut self, id: u64, cmd: &str, mut a: Args) -> Step {
        let bad = || Step::Answer(refused(id, CTL_REQUEST));
        let empty = |a: &Args| a.done();
        let answer = |r: Result<Map<String, Value>, DeviceError>| match r {
            Ok(m) => Step::Answer(ok(id, m)),
            Err(e) => Step::Answer(device_error(id, &e)),
        };
        match cmd {
            "status" if empty(&a) => answer(Ok(obj(status_json(&self.device.status())))),
            "board" if empty(&a) => {
                let v = self.device.view();
                match v.confirmed_state() {
                    Some((bytes, hash)) => {
                        answer(Ok(obj(json!({"canonical": hex(bytes), "state_hash": hex(&hash)}))))
                    }
                    None => answer(Err(crate::derr("device/needs_sync"))),
                }
            }
            "submit" => {
                let Some(intent) = a.value("intent").filter(Value::is_object) else { return bad() };
                if !a.done() {
                    return bad();
                }
                answer(self.device.submit_json(&intent.to_string()).map(|s| obj(submitted_json(&s))))
            }
            "create" => {
                let (Some(relay), Some(secret), Some(source), Some(name)) =
                    (a.string("relay"), a.string("create_secret"), a.value("source"), a.string("name"))
                else {
                    return bad();
                };
                let source = match &source {
                    Value::String(s) if s == "empty" => CreateSource::Empty,
                    Value::String(s) if s == "this" => CreateSource::ThisFolder,
                    Value::Object(o) if o.len() == 1 => match o.get("folder") {
                        Some(Value::String(p)) => CreateSource::Folder(PathBuf::from(p)),
                        _ => return bad(),
                    },
                    _ => return bad(),
                };
                if !a.done() {
                    return bad();
                }
                let request = CreateRequest { relay_host: relay, create_secret: Zeroizing::new(secret), source, device_name: name };
                match self.device.begin_create(request) {
                    Ok(f) => {
                        self.flow = Some(f);
                        answer(Ok(Map::new()))
                    }
                    Err(e) => answer(Err(e)),
                }
            }
            "sheet" if empty(&a) => match &self.flow {
                // Test only: the one place the seed leaves a flow (DEV-CTL-002).
                Some(f) => {
                    let s = f.sheet();
                    answer(Ok(obj(json!({"sheet": s.seed_groups.as_str(), "address": s.relay_address, "recovery_fp": s.recovery_fp}))))
                }
                None => answer(Err(crate::derr("device/pair_state"))),
            },
            "confirm_sheet" => {
                let Some(text) = a.string("text").map(Zeroizing::new) else { return bad() };
                if !a.done() {
                    return bad();
                }
                match self.flow.as_mut() {
                    Some(f) => answer(f.confirm_sheet(&text).map(|()| Map::new())),
                    None => answer(Err(crate::derr("device/pair_state"))),
                }
            }
            "finish_create" if empty(&a) => match self.flow.take() {
                Some(f) => answer(f.finish().map(|c| obj(json!({"address": c.address})))),
                None => answer(Err(crate::derr("device/pair_state"))),
            },
            "pair_start" if empty(&a) => answer(self.device.pair_start().map(|s| {
                // Test only: the code leaves the device here (DEV-CTL-002).
                obj(json!({"code": s.code_display.as_str(), "address": s.address_display}))
            })),
            "pair_status" if empty(&a) => answer(Ok(obj(pair_json(&self.device.status().pairing)))),
            "pair_confirm" if empty(&a) => {
                answer(self.device.pair_confirm().map(|()| obj(pair_json(&self.device.status().pairing))))
            }
            "pair_cancel" if empty(&a) => {
                answer(self.device.pair_cancel().map(|()| obj(pair_json(&self.device.status().pairing))))
            }
            "join" => {
                let (Some(address), Some(code), Some(name)) = (a.string("address"), a.string("code"), a.string("name"))
                else {
                    return bad();
                };
                if !a.done() {
                    return bad();
                }
                answer(self.device.join(&address, Zeroizing::new(code), &name).map(|()| Map::new()))
            }
            "join_sas" => {
                let Some(digits) = a.string("digits").map(Zeroizing::new) else { return bad() };
                if !a.done() {
                    return bad();
                }
                answer(self.device.join_sas(&digits).map(|()| Map::new()))
            }
            "wait" => {
                let (Some(since), Some(t)) = (a.u53("since"), a.u53("timeout_ms")) else { return bad() };
                if !a.done() || t > 60_000 {
                    return bad();
                }
                let c = self.device.wait_for_change(since, t as u32);
                answer(Ok(obj(changes_json(&c))))
            }
            "clock" => {
                let Some(clock) = self.clock.clone() else { return bad() };
                let set = a.i53("set_ms");
                let advance = if set.is_none() { a.u53("advance_ms") } else { None };
                if !a.done() {
                    return bad();
                }
                let wall = match (set, advance) {
                    (Some(s), None) => clock.set(s),
                    (None, Some(d)) => clock.advance(d),
                    _ => return bad(),
                };
                answer(Ok(obj(json!({"wall_ms": wall}))))
            }
            "shutdown" if empty(&a) => match self.device.shutdown() {
                Ok(()) => Step::Exit(ok(id, Map::new()), 0),
                Err(e) => Step::Exit(device_error(id, &e), 1),
            },
            "kill" => Step::Kill,
            c if NOT_BUILT.contains(&c) => bad(),
            _ => bad(),
        }
    }
}

fn changes_json(c: &crate::Changes) -> Value {
    json!({
        "any": c.any, "view": c.view, "status": c.status, "notices": c.notices, "banners": c.banners,
        "overlay": c.overlay, "weather": c.weather, "picture": c.picture, "backup": c.backup,
        "pairing": c.pairing, "ops": c.ops,
    })
}

fn submitted_json(s: &crate::Submitted) -> Value {
    let mut reply = Map::new();
    if let Some(i) = s.reply.id {
        reply.insert("id".into(), json!(i));
    }
    if let Some(i) = s.reply.session {
        reply.insert("session".into(), json!(i));
    }
    if let Some(i) = s.reply.moved {
        reply.insert("moved".into(), json!(i));
    }
    let mut m = Map::new();
    m.insert("reply".into(), Value::Object(reply));
    if let Some(e) = s.entry_id {
        m.insert("entry_id".into(), json!(hex(&e)));
    }
    if let Some(p) = s.provisional_seq {
        m.insert("provisional_seq".into(), json!(p));
    }
    m.insert("view_version".into(), json!(s.view_version));
    Value::Object(m)
}

fn state_name(p: &PairState) -> &'static str {
    match p {
        PairState::Idle => "idle",
        PairState::ShowingCode => "showing_code",
        PairState::ShowSas { .. } => "show_sas",
        PairState::Registering => "registering",
        PairState::AskSas => "ask_sas",
        PairState::Accepted => "accepted",
        PairState::Joining => "joining",
        PairState::Done => "done",
        PairState::Aborted { .. } => "aborted",
    }
}

/// `{"state": …, "sas"?: digits}`, and `reason` for an aborted pairing.
fn pair_json(p: &PairState) -> Value {
    let mut m = Map::new();
    m.insert("state".into(), json!(state_name(p)));
    match p {
        PairState::ShowSas { digits } => {
            m.insert("sas".into(), json!(digits));
        }
        PairState::Aborted { code } => {
            m.insert("reason".into(), json!(code));
        }
        _ => {}
    }
    Value::Object(m)
}

fn mode_name(m: Mode) -> &'static str {
    match m {
        Mode::LocalOnly => "local_only",
        Mode::Synced => "synced",
        Mode::KeyLost => "key_lost",
    }
}

/// `DeviceStatus` (§17.8) by §6.5's mapping; of the engine's `Status`, the
/// fields the end-to-end tests read (the full mapping is a shortcut, `fit.md`).
fn status_json(s: &DeviceStatus) -> Value {
    let mut m = Map::new();
    if let Some(e) = &s.engine {
        let mut o = Map::new();
        o.insert("session".into(), json!(format!("{:?}", e.session).to_lowercase()));
        o.insert("relay_host".into(), json!(e.relay_host));
        o.insert("push".into(), json!(e.push));
        if let Some(t) = e.last_synced_ms {
            o.insert("last_synced_ms".into(), json!(t));
        }
        if let Some(t) = e.unreachable_since_ms {
            o.insert("unreachable_since_ms".into(), json!(t));
        }
        if let Some(t) = e.paused_until_ms {
            o.insert("paused_until_ms".into(), json!(t));
        }
        o.insert("queued".into(), json!(e.queued));
        o.insert("doomed".into(), json!(e.doomed));
        o.insert("head".into(), json!({"seq": e.head.seq, "chain": hex(&e.head.chain)}));
        o.insert("board_seq".into(), json!(e.board_seq));
        o.insert("fingerprint".into(), json!(e.fingerprint));
        if let Some(h) = &e.halt {
            o.insert("halt".into(), json!({"kind": format!("{:?}", h.kind), "seq": h.seq, "code": h.code}));
        }
        o.insert("stops".into(), json!(e.stops.iter().map(|x| format!("{:?}", x.kind)).collect::<Vec<_>>()));
        o.insert(
            "alarms".into(),
            Value::Array(
                e.alarms
                    .iter()
                    .map(|a| json!({"id": a.id, "kind": format!("{:?}", a.kind), "code": a.code.clone().unwrap_or_default()}))
                    .collect(),
            ),
        );
        o.insert(
            "devices".into(),
            Value::Array(
                e.devices
                    .iter()
                    .map(|d| json!({"dev": hex(&d.dev), "name": d.name, "device_no": d.device_no, "is_self": d.is_self, "last_beat_ms": d.last_beat_ms}))
                    .collect(),
            ),
        );
        o.insert("notices_unread".into(), json!(e.notices_unread));
        o.insert("refusals_unread".into(), json!(e.refusals_unread));
        m.insert("engine".into(), Value::Object(o));
    }
    m.insert("mode".into(), json!(mode_name(s.mode)));
    if let Some(a) = &s.relay_address {
        m.insert("relay_address".into(), json!(a));
    }
    m.insert("socket_open".into(), json!(s.socket_open));
    m.insert("requests_today".into(), json!(s.requests_today));
    m.insert("pairing".into(), pair_json(&s.pairing));
    m.insert("device_name".into(), json!(s.device_name));
    Value::Object(m)
}

/// The binary's `main` (device.md §19.1).
pub fn main() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("run") => match parse_run(&args[1..]) {
            Ok(o) => run(o),
            Err(e) => {
                eprintln!("taskdeck-devicectl: {e}");
                2
            }
        },
        Some("import-check") => {
            eprintln!("taskdeck-devicectl: import-check is not built yet");
            2
        }
        _ => {
            eprintln!("usage: taskdeck-devicectl run --data <dir> --keys <dir> [options] | import-check <folder>");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(dir: &std::path::Path) -> Ctl {
        let clock = Arc::new(ManualClock::new(MANUAL_START_MS));
        let (d, _) = Device::open(OpenOptions {
            data_dir: dir.join("data"),
            platform: Platform::Linux,
            key_store: Arc::new(FileKeyStore::new(dir.join("keys"))),
            zone: ZoneSource::Table(taskdeck_core::time::TableZone { initial: 0, transitions: Vec::new() }),
            trust: TrustRoots::Platform,
            build_id: "test".into(),
            default_device_name: "desk".into(),
            clock: Some(clock.clone()),
            files: None,
        })
        .expect("opens");
        Ctl::new(d, Some(clock))
    }

    fn answer(c: &mut Ctl, line: &str) -> Value {
        match c.line(line.as_bytes()) {
            Step::Answer(v) | Step::Exit(v, _) => v,
            Step::Kill => panic!("kill"),
        }
    }

    /// DEV-CTL-004: an invalid line is answered with id 0; a valid id with
    /// an unknown `cmd`, missing or mistyped arguments, or an extra member,
    /// with that id; nothing is called.
    #[test]
    fn invalid_requests_are_refused_dev_ctl_004() {
        let t = tempfile::tempdir().expect("tmp");
        let mut c = device(t.path());
        let e = json!({"id": 0, "ok": false, "error": "device/ctl_request"});
        assert_eq!(answer(&mut c, "not json"), e);
        assert_eq!(answer(&mut c, r#"{"id":1,"cmd":"status","cmd":"board"}"#), e, "a duplicate key");
        assert_eq!(answer(&mut c, r#"{"id":1.5,"cmd":"status"}"#), e, "a float id");
        assert_eq!(answer(&mut c, r#"{"id":-1,"cmd":"status"}"#), e);
        assert_eq!(answer(&mut c, r#"{"id":9007199254740992,"cmd":"status"}"#), e, "above 2^53 - 1");
        assert_eq!(answer(&mut c, r#"[1]"#), e);
        assert_eq!(answer(&mut c, r#"{"cmd":"status"}"#), e, "no id");
        let e7 = json!({"id": 7, "ok": false, "error": "device/ctl_request"});
        assert_eq!(answer(&mut c, r#"{"id":7}"#), e7, "no cmd");
        assert_eq!(answer(&mut c, r#"{"id":7,"cmd":3}"#), e7);
        assert_eq!(answer(&mut c, r#"{"id":7,"cmd":"frobnicate"}"#), e7);
        assert_eq!(answer(&mut c, r#"{"id":7,"cmd":"status","x":1}"#), e7, "an extra member");
        assert_eq!(answer(&mut c, r#"{"id":7,"cmd":"wait","since":0}"#), e7, "a missing argument");
        assert_eq!(answer(&mut c, r#"{"id":7,"cmd":"wait","since":"0","timeout_ms":1}"#), e7, "a mistyped one");
        assert_eq!(answer(&mut c, r#"{"id":7,"cmd":"wait","since":0,"timeout_ms":60001}"#), e7);
        assert_eq!(answer(&mut c, r#"{"id":7,"cmd":"clock"}"#), e7);
        assert!(c.device.shutdown().is_ok());
    }

    /// DEV-CTL-003: a result's members sit beside `id` and `ok`; an empty
    /// result is `{"id", "ok"}` alone; a facade error carries its code.
    #[test]
    fn results_sit_beside_id_and_ok_dev_ctl_003() {
        let t = tempfile::tempdir().expect("tmp");
        let mut c = device(t.path());
        let w = answer(&mut c, r#"{"id":3,"cmd":"wait","since":0,"timeout_ms":0}"#);
        assert_eq!(w["id"], json!(3));
        assert_eq!(w["ok"], json!(true));
        for k in ["any", "view", "status", "notices", "banners", "overlay", "weather", "picture", "backup", "pairing", "ops"] {
            assert!(w[k].is_u64(), "{k} in {w}");
        }
        let c1 = answer(&mut c, r#"{"id":4,"cmd":"clock","advance_ms":1000}"#);
        assert_eq!(c1, json!({"id": 4, "ok": true, "wall_ms": MANUAL_START_MS + 1000}));
        let s = answer(&mut c, r#"{"id":5,"cmd":"status"}"#);
        assert_eq!((s["mode"].as_str(), s["pairing"]["state"].as_str()), (Some("local_only"), Some("idle")));
        let b = answer(&mut c, r#"{"id":6,"cmd":"board"}"#);
        assert_eq!(b, json!({"id": 6, "ok": false, "error": "device/needs_sync"}));
        let j = answer(&mut c, r#"{"id":8,"cmd":"join","address":"relay.example/x","code":"x","name":"n"}"#);
        assert_eq!(j["error"], json!("relay/address"));
        let d = answer(&mut c, r#"{"id":9,"cmd":"shutdown"}"#);
        assert_eq!(d, json!({"id": 9, "ok": true}));
    }
}
