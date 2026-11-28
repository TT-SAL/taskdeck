//! A `taskdeck-devicectl` process driven by the line protocol of device.md
//! §19.2: one JSON request per line on its standard input, one answer per
//! request (matched by `id`) and event lines on its standard output.
//!
//! Every wait has a timeout, and a timeout fails the test with the last lines
//! of the device's output and stderr. Nothing secret is printed: the answers
//! of `sheet` and `pair_start` carry a seed and a pairing code (DEV-CTL-002),
//! and the requests that carry a create secret, a sheet text, a code, SAS
//! digits or an address are shown with those fields redacted.

use super::DEVICECTL;
use serde_json::{Map, Value, json};
use std::collections::VecDeque;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long one request may take before the test fails.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a process may take to end after `shutdown`, `kill` or on drop.
const EXIT_TIMEOUT: Duration = Duration::from_secs(10);
/// Lines of output and stderr kept for a failure message.
const TAIL_LINES: usize = 40;
/// Fields whose values never appear in a message of the harness: they carry a
/// seed, a pairing code, a create secret or an address with its door.
const SECRET_FIELDS: &[&str] = &["create_secret", "text", "code", "digits", "sheet", "seed", "token", "secret", "address"];

/// How a device is started (device.md §19.1).
#[derive(Clone, Debug)]
pub struct CtlOptions {
    /// The program: `taskdeck-devicectl`. A test of the harness itself puts a
    /// stand-in here.
    pub program: PathBuf,
    /// Arguments before `run` (a stand-in's script).
    pub leading_args: Vec<OsString>,
    pub data: PathBuf,
    pub keys: PathBuf,
    pub name: String,
    pub platform: String,
    pub zone: String,
    pub clock: String,
    /// More options of `run` (`--crash-at <k>`, `--trust-pem <file>`, ...).
    pub extra_args: Vec<String>,
    /// Where the device's stderr is written.
    pub stderr_log: PathBuf,
    /// The timeout of a request that does not set its own.
    pub timeout: Duration,
}

impl CtlOptions {
    /// A device named `name` whose folders are `<dir>/<name>/data` and
    /// `<dir>/<name>/keys`, with the options the milestone needs:
    /// `--clock manual --zone utc --platform linux`.
    pub fn in_dir(dir: &Path, name: &str) -> CtlOptions {
        let root = dir.join(name);
        let (data, keys) = (root.join("data"), root.join("keys"));
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&keys).unwrap();
        CtlOptions {
            program: PathBuf::from(DEVICECTL),
            leading_args: Vec::new(),
            data,
            keys,
            name: name.to_string(),
            platform: "linux".into(),
            zone: "utc".into(),
            clock: "manual".into(),
            extra_args: Vec::new(),
            stderr_log: root.join("stderr.log"),
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

/// What a request can come to.
#[derive(Debug)]
pub enum CtlError {
    /// No answer within the timeout.
    Timeout { cmd: String, context: String },
    /// The device's output ended, or its standard input closed.
    Closed { cmd: String, context: String },
    /// A line that is not the protocol, or an answer to another request.
    Protocol { why: String, context: String },
}

impl std::fmt::Display for CtlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CtlError::Timeout { cmd, context } => write!(f, "no answer to `{cmd}` in time\n{context}"),
            CtlError::Closed { cmd, context } => write!(f, "the device ended during `{cmd}`\n{context}"),
            CtlError::Protocol { why, context } => write!(f, "protocol: {why}\n{context}"),
        }
    }
}

impl std::error::Error for CtlError {}

enum Msg {
    Answer(Value),
    Event(Value),
    /// A line that is not a JSON object with `id` or `event`: only its length is kept.
    Junk(usize),
    Eof,
}

type Tail = Arc<Mutex<VecDeque<String>>>;

fn push_tail(tail: &Tail, line: String) {
    let mut t = tail.lock().unwrap();
    if t.len() == TAIL_LINES {
        t.pop_front();
    }
    t.push_back(line);
}

/// `v` with every secret field replaced.
pub fn redact(v: &Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(
            m.iter()
                .map(|(k, x)| (k.clone(), if SECRET_FIELDS.contains(&k.as_str()) { json!("<redacted>") } else { redact(x) }))
                .collect::<Map<_, _>>(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(redact).collect()),
        other => other.clone(),
    }
}

pub struct Ctl {
    opts: CtlOptions,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    rx: Receiver<Msg>,
    next_id: u64,
    events: Vec<Value>,
    out_tail: Tail,
    err_tail: Tail,
}

impl Ctl {
    /// Starts the device (`run --data --keys --name --clock --zone --platform`).
    pub fn start(opts: CtlOptions) -> Ctl {
        let (rx, child, stdin, out_tail, err_tail) = spawn(&opts);
        Ctl { opts, child: Some(child), stdin: Some(stdin), rx, next_id: 1, events: Vec::new(), out_tail, err_tail }
    }

    /// Starts a device named `name` in `<dir>/<name>` with the default options.
    pub fn start_in(dir: &Path, name: &str) -> Ctl {
        Ctl::start(CtlOptions::in_dir(dir, name))
    }

    pub fn options(&self) -> &CtlOptions {
        &self.opts
    }

    /// The same device again on the same folders, after `kill` or `shutdown`
    /// (or a crash): a new process, with request ids starting again at 1.
    pub fn restart(&mut self) {
        self.restart_with(|_| {});
    }

    /// As [`Ctl::restart`], with other options for the new process
    /// (`--crash-at`, another name, ...).
    pub fn restart_with(&mut self, change: impl FnOnce(&mut CtlOptions)) {
        if let Some(child) = self.child.as_mut() {
            assert!(matches!(child.try_wait(), Ok(Some(_))), "restart of a device that is still running: kill or shut it down first");
        }
        change(&mut self.opts);
        let (rx, child, stdin, out_tail, err_tail) = spawn(&self.opts);
        self.rx = rx;
        self.child = Some(child);
        self.stdin = Some(stdin);
        self.out_tail = out_tail;
        self.err_tail = err_tail;
        self.next_id = 1;
    }

    /// Whether the process is still running.
    pub fn is_running(&mut self) -> bool {
        self.child.as_mut().is_some_and(|c| matches!(c.try_wait(), Ok(None)))
    }

    /// The `event` lines received since the last call, oldest first
    /// (`changed` after each publish, `exit` before an exit).
    pub fn take_events(&mut self) -> Vec<Value> {
        self.pump();
        std::mem::take(&mut self.events)
    }

    /// The last lines of the device's output and stderr, secrets redacted.
    pub fn context(&self) -> String {
        let join = |t: &Tail| t.lock().unwrap().iter().cloned().collect::<Vec<_>>().join("\n");
        format!(
            "--- device `{}`: last lines of its protocol ---\n{}\n--- its stderr (all of it: {}) ---\n{}\n--- end ---",
            self.opts.name,
            join(&self.out_tail),
            self.opts.stderr_log.display(),
            join(&self.err_tail)
        )
    }

    /// Moves what has arrived into `events` without waiting.
    fn pump(&mut self) {
        while let Ok(m) = self.rx.try_recv() {
            if let Msg::Event(e) = m {
                self.events.push(e);
            }
        }
    }

    /// Sends one request and waits for its answer. `args` is a JSON object
    /// (or `null`) of the arguments of §19.2's table. Any answer is returned,
    /// `ok` or not.
    pub fn try_call(&mut self, cmd: &str, args: Value, timeout: Duration) -> Result<Value, CtlError> {
        let id = self.next_id;
        self.next_id += 1;
        let mut req = match args {
            Value::Null => Map::new(),
            Value::Object(m) => m,
            other => panic!("the arguments of `{cmd}` must be an object, not {other}"),
        };
        req.insert("id".into(), json!(id));
        req.insert("cmd".into(), json!(cmd));
        let req = Value::Object(req);
        push_tail(&self.out_tail, format!("-> {}", redact(&req)));
        let mut line = serde_json::to_vec(&req).unwrap();
        line.push(b'\n');
        let sent = self.stdin.as_mut().ok_or("no standard input").and_then(|s| s.write_all(&line).and_then(|()| s.flush()).map_err(|_| "write failed"));
        if sent.is_err() {
            return Err(CtlError::Closed { cmd: cmd.into(), context: self.context() });
        }
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(Msg::Event(e)) => self.events.push(e),
                Ok(Msg::Answer(a)) => {
                    if a["id"] == json!(id) {
                        return Ok(a);
                    }
                    return Err(CtlError::Protocol { why: format!("an answer with id {} while waiting for {id}", a["id"]), context: self.context() });
                }
                Ok(Msg::Junk(n)) => {
                    return Err(CtlError::Protocol { why: format!("a line of {n} bytes that is not an answer or an event"), context: self.context() });
                }
                Ok(Msg::Eof) | Err(RecvTimeoutError::Disconnected) => {
                    return Err(CtlError::Closed { cmd: cmd.into(), context: self.context() });
                }
                Err(RecvTimeoutError::Timeout) => {
                    return Err(CtlError::Timeout { cmd: cmd.into(), context: self.context() });
                }
            }
        }
    }

    /// [`Ctl::try_call`] with the options' timeout; a failure fails the test.
    pub fn call(&mut self, cmd: &str, args: Value) -> Value {
        let t = self.opts.timeout;
        self.try_call(cmd, args, t).unwrap_or_else(|e| panic!("{e}"))
    }

    /// A request that must be answered `ok`; returns the whole answer.
    pub fn call_ok(&mut self, cmd: &str, args: Value) -> Value {
        let a = self.call(cmd, args);
        assert_eq!(a["ok"], json!(true), "`{cmd}` was refused: {}\n{}", redact(&a), self.context());
        a
    }

    /// A request that must be refused; returns the error code.
    pub fn call_err(&mut self, cmd: &str, args: Value) -> String {
        let a = self.call(cmd, args);
        assert_eq!(a["ok"], json!(false), "`{cmd}` was accepted: {}\n{}", redact(&a), self.context());
        a["error"].as_str().unwrap_or_else(|| panic!("an answer without an error code: {a}")).to_string()
    }

    // ---- the commands of §19.2 the milestone uses ----

    /// `status`: the answer carries the status as record JSON (§6.5).
    pub fn status(&mut self) -> Value {
        self.call_ok("status", json!({}))
    }

    /// `board`: the board bytes and the state hash with the current membership.
    pub fn board(&mut self) -> Board {
        let a = self.call_ok("board", json!({}));
        Board { canonical: str_field(&a, "canonical"), state_hash: str_field(&a, "state_hash") }
    }

    /// `submit`: `intent` is the intent JSON of `core.md` §12.2; the answer is `Submitted`.
    pub fn submit(&mut self, intent: Value) -> Value {
        self.call_ok("submit", json!({"intent": intent}))
    }

    /// `create` (`begin_create`): `relay` is the relay target (the standby's URL).
    pub fn create(&mut self, relay: &str, create_secret: &str, source: Source, name: &str) -> Value {
        self.call_ok("create", json!({"relay": relay, "create_secret": create_secret, "source": source.json(), "name": name}))
    }

    /// `sheet`: the pairing sheet of the create flow; test only, the one place the seed leaves a flow.
    pub fn sheet(&mut self) -> Sheet {
        let a = self.call_ok("sheet", json!({}));
        Sheet { sheet: str_field(&a, "sheet"), address: str_field(&a, "address"), recovery_fp: str_field(&a, "recovery_fp") }
    }

    pub fn confirm_sheet(&mut self, text: &str) -> Value {
        self.call_ok("confirm_sheet", json!({"text": text}))
    }

    /// `finish_create`: returns the relay address of the new board.
    pub fn finish_create(&mut self) -> String {
        let a = self.call_ok("finish_create", json!({}));
        str_field(&a, "address")
    }

    /// `pair_start`: a pairing code and the address; test only.
    pub fn pair_start(&mut self) -> PairStart {
        let a = self.call_ok("pair_start", json!({}));
        PairStart { code: str_field(&a, "code"), address: str_field(&a, "address") }
    }

    /// `pair_status`: `{"state": …, "sas"?: digits}`.
    pub fn pair_status(&mut self) -> Value {
        self.call_ok("pair_status", json!({}))
    }

    pub fn pair_confirm(&mut self) -> Value {
        self.call_ok("pair_confirm", json!({}))
    }

    pub fn pair_cancel(&mut self) -> Value {
        self.call_ok("pair_cancel", json!({}))
    }

    /// `join`: the joining device's side of a pairing.
    pub fn join(&mut self, address: &str, code: &str, name: &str) -> Value {
        self.call_ok("join", json!({"address": address, "code": code, "name": name}))
    }

    /// `join_sas`: the digits the joining device's person typed.
    pub fn join_sas(&mut self, digits: &str) -> Value {
        self.call_ok("join_sas", json!({"digits": digits}))
    }

    /// `wait` (`wait_for_change`): the change counters (§17.3) as soon as
    /// `any > since`, or at `timeout_ms` (at most 60,000). The request's own
    /// timeout is `timeout_ms` plus the options' timeout.
    pub fn wait(&mut self, since: u64, timeout_ms: u32) -> Value {
        let t = self.opts.timeout + Duration::from_millis(u64::from(timeout_ms));
        let a = self.try_call("wait", json!({"since": since, "timeout_ms": timeout_ms}), t).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(a["ok"], json!(true), "`wait` was refused: {}\n{}", redact(&a), self.context());
        a
    }

    /// Polls `wait` until `done` accepts the counters, or fails the test after
    /// `timeout`. Returns the counters that satisfied it. `done` sees the
    /// counters of §17.3 as the answer's fields (`any`, `view`, `status`, ...).
    pub fn wait_until(&mut self, mut done: impl FnMut(&Value) -> bool, timeout: Duration) -> Value {
        let deadline = Instant::now() + timeout;
        let mut since = 0u64;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "wait_until: the condition did not hold in {timeout:?}\n{}", self.context());
            let slice = u32::try_from(left.as_millis().min(1000)).unwrap();
            let counters = self.wait(since, slice);
            if done(&counters) {
                return counters;
            }
            since = since.max(counters["any"].as_u64().unwrap_or(since));
        }
    }

    /// `clock` with `advance_ms`: returns the new `wall_ms`.
    pub fn clock_advance(&mut self, ms: u64) -> i64 {
        let a = self.call_ok("clock", json!({"advance_ms": ms}));
        a["wall_ms"].as_i64().unwrap_or_else(|| panic!("clock answered without wall_ms: {a}"))
    }

    /// `clock` with `set_ms`: returns the new `wall_ms`.
    pub fn clock_set(&mut self, ms: i64) -> i64 {
        let a = self.call_ok("clock", json!({"set_ms": ms}));
        a["wall_ms"].as_i64().unwrap_or_else(|| panic!("clock answered without wall_ms: {a}"))
    }

    /// `shutdown`: the device answers, writes the `exit` event and exits;
    /// returns its exit code (0 when it shut down as asked).
    pub fn shutdown(&mut self) -> i32 {
        self.call_ok("shutdown", json!({}));
        let status = self.wait_exit();
        self.drain_to_eof();
        status.code().unwrap_or_else(|| panic!("the device was killed by a signal after shutdown: {status}\n{}", self.context()))
    }

    /// `kill`: `std::process::abort()` at once, without an answer. Returns how
    /// the process ended.
    pub fn kill(&mut self) -> ExitStatus {
        let id = self.next_id;
        self.next_id += 1;
        let line = format!("{{\"id\":{id},\"cmd\":\"kill\"}}\n");
        push_tail(&self.out_tail, format!("-> {}", line.trim_end()));
        if let Some(s) = self.stdin.as_mut() {
            // The device may already be gone: the write is allowed to fail.
            let _ = s.write_all(line.as_bytes()).and_then(|()| s.flush());
        }
        let status = self.wait_exit();
        self.drain_to_eof();
        status
    }

    /// Waits for the process to end (the standard input is closed first).
    fn wait_exit(&mut self) -> ExitStatus {
        drop(self.stdin.take());
        let child = self.child.as_mut().expect("a started device");
        let end = Instant::now() + EXIT_TIMEOUT;
        loop {
            if let Ok(Some(status)) = child.try_wait() {
                return status;
            }
            if Instant::now() >= end {
                let _ = child.kill();
                let _ = child.wait();
                panic!("the device did not exit within {EXIT_TIMEOUT:?}\n{}", self.context());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Reads what the ended process wrote, collecting its events.
    fn drain_to_eof(&mut self) {
        let end = Instant::now() + EXIT_TIMEOUT;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(Msg::Event(e)) => self.events.push(e),
                Ok(Msg::Answer(_) | Msg::Junk(_)) => {}
                Ok(Msg::Eof) | Err(_) => return,
            }
        }
    }
}

impl Drop for Ctl {
    fn drop(&mut self) {
        drop(self.stdin.take());
        if let Some(child) = self.child.as_mut() {
            if matches!(child.try_wait(), Ok(None)) {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
        if std::thread::panicking() {
            eprintln!("{}", self.context());
        }
    }
}

fn str_field(a: &Value, field: &str) -> String {
    a[field].as_str().unwrap_or_else(|| panic!("the answer has no string `{field}`: {}", redact(a))).to_string()
}

fn spawn(opts: &CtlOptions) -> (Receiver<Msg>, Child, ChildStdin, Tail, Tail) {
    let mut cmd = Command::new(&opts.program);
    cmd.args(&opts.leading_args)
        .arg("run")
        .arg("--data")
        .arg(&opts.data)
        .arg("--keys")
        .arg(&opts.keys)
        .args(["--name", &opts.name, "--clock", &opts.clock, "--zone", &opts.zone, "--platform", &opts.platform])
        .args(&opts.extra_args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().unwrap_or_else(|e| panic!("start {}: {e}", opts.program.display()));
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let out_tail: Tail = Arc::default();
    let err_tail: Tail = Arc::default();
    let (tx, rx) = mpsc::channel();
    {
        let tail = Arc::clone(&out_tail);
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let msg = match serde_json::from_str::<Value>(&line) {
                    Ok(v) if v.get("event").is_some() => Msg::Event(v),
                    Ok(v) if v.get("id").is_some() => Msg::Answer(v),
                    _ => Msg::Junk(line.len()),
                };
                match &msg {
                    Msg::Answer(v) => push_tail(&tail, format!("<- {}", redact(v))),
                    Msg::Event(v) => push_tail(&tail, format!("<- {v}")),
                    Msg::Junk(n) => push_tail(&tail, format!("<- (a line of {n} bytes, not shown)")),
                    Msg::Eof => {}
                }
                if tx.send(msg).is_err() {
                    return;
                }
            }
            let _ = tx.send(Msg::Eof);
        });
    }
    {
        let tail = Arc::clone(&err_tail);
        let log = opts.stderr_log.clone();
        std::thread::spawn(move || {
            let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&log).ok();
            if let Some(f) = file.as_mut() {
                let _ = writeln!(f, "--- start ---");
            }
            for line in BufReader::new(stderr).lines() {
                let Ok(line) = line else { break };
                if let Some(f) = file.as_mut() {
                    let _ = writeln!(f, "{line}");
                }
                push_tail(&tail, line);
            }
        });
    }
    (rx, child, stdin, out_tail, err_tail)
}

/// The `source` of `create` (§19.2): what the new board starts from.
#[derive(Clone, Debug)]
pub enum Source {
    Empty,
    /// This device's own local board.
    This,
    /// A legacy folder to import.
    Folder(PathBuf),
}

impl Source {
    fn json(&self) -> Value {
        match self {
            Source::Empty => json!("empty"),
            Source::This => json!("this"),
            Source::Folder(p) => json!({"folder": p}),
        }
    }
}

/// The answer of `board`.
#[derive(Clone, Debug)]
pub struct Board {
    /// The board bytes, lowercase hex.
    pub canonical: String,
    /// The state hash with the current membership, 64 hex characters.
    pub state_hash: String,
}

/// The answer of `sheet`. `Debug` shows no seed.
#[derive(Clone)]
pub struct Sheet {
    pub sheet: String,
    pub address: String,
    pub recovery_fp: String,
}

impl std::fmt::Debug for Sheet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sheet").field("recovery_fp", &self.recovery_fp).finish_non_exhaustive()
    }
}

/// The answer of `pair_start`. `Debug` shows no code.
#[derive(Clone)]
pub struct PairStart {
    pub code: String,
    pub address: String,
}

impl std::fmt::Debug for PairStart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PairStart").finish_non_exhaustive()
    }
}
