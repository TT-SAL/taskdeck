//! The standby relay (`relay/standby.js`, relay.md §15.2) on loopback, started
//! for one test and stopped when the `Standby` is dropped.
//!
//! A token key and a create secret are made up for the run from the
//! operating system's randomness. The standby serves plain HTTP on
//! `127.0.0.1` (the loopback URLs of device.md §11.1 item 1). Its stderr goes
//! to a log file that is printed when the test that owns it fails.

use super::{find_node, random_hex};
use std::io::{BufRead, BufReader, Read};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Printed by the child once `startStandby` has returned, on one line: its URL.
/// The child stays up until its standard input is closed.
const LAUNCHER: &str = r#"
import { pathToFileURL } from 'node:url';
const { startStandby } = await import(pathToFileURL(process.env.TD_STANDBY_JS).href);
const e = process.env;
const s = await startStandby({ tokenKey: e.TD_TOKEN_KEY, createSecret: e.TD_CREATE_SECRET, origin: e.TD_RELAY_ORIGIN, dataDir: e.TD_DATA_DIR, listen: e.TD_LISTEN });
process.stdout.write(`${s.url}\n`);
process.stdin.on('end', () => s.close().then(() => process.exit(0)));
process.stdin.resume();
"#;

const START_TIMEOUT: Duration = Duration::from_secs(20);
const STOP_TIMEOUT: Duration = Duration::from_secs(10);
/// Reserving a port and starting on it can lose the race to another process.
const START_ATTEMPTS: usize = 5;

pub struct Standby {
    child: Option<Child>,
    host: String,
    url: String,
    token_key: String,
    create_secret: String,
    data_dir: PathBuf,
    log: PathBuf,
    _root: tempfile::TempDir,
}

impl std::fmt::Debug for Standby {
    /// Neither the token key nor the create secret is ever printed.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Standby").field("host", &self.host).finish_non_exhaustive()
    }
}

impl Standby {
    /// Starts a standby with a data folder, a token key and a create secret of
    /// its own, and returns once it answers on its port.
    pub fn start() -> Standby {
        let node = find_node();
        let root = tempfile::tempdir().expect("a temporary folder");
        let data_dir = root.path().join("relay-data");
        std::fs::create_dir(&data_dir).unwrap();
        let log = root.path().join("standby.stderr.log");
        let token_key = random_hex(32);
        let create_secret = random_hex(32);
        let standby_js = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../relay/standby.js");
        let standby_js = standby_js.canonicalize().unwrap_or_else(|e| panic!("{}: {e}", standby_js.display()));
        let mut last = String::new();
        for _ in 0..START_ATTEMPTS {
            // The relay origin is what devices sign in their login (relay.md
            // §5.4: `https://` and the host of the address), so it names the
            // port the standby listens on, which port 0 cannot tell the
            // standby before it starts: the port is reserved first (fit.md h-1).
            let port = {
                let probe = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
                probe.local_addr().unwrap().port()
            };
            let host = format!("127.0.0.1:{port}");
            let stderr = std::fs::OpenOptions::new().create(true).append(true).open(&log).unwrap();
            let mut child = Command::new(&node)
                .args(["--input-type=module", "-e", LAUNCHER])
                .env("TD_STANDBY_JS", &standby_js)
                .env("TD_TOKEN_KEY", &token_key)
                .env("TD_CREATE_SECRET", &create_secret)
                .env("TD_RELAY_ORIGIN", format!("https://{host}"))
                .env("TD_DATA_DIR", &data_dir)
                .env("TD_LISTEN", &host)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(stderr)
                .spawn()
                .unwrap_or_else(|e| panic!("start {}: {e}", node.display()));
            match read_url(&mut child) {
                Ok(url) => {
                    assert_eq!(url, format!("http://{host}"), "the standby listens elsewhere than asked");
                    return Standby { child: Some(child), url, host, token_key, create_secret, data_dir, log, _root: root };
                }
                Err(why) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    last = why;
                }
            }
        }
        panic!("the standby did not start in {START_ATTEMPTS} attempts: {last}\n--- standby stderr ---\n{}", std::fs::read_to_string(&log).unwrap_or_default());
    }

    /// `http://127.0.0.1:<port>`: where the standby listens, the relay target of a device.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// `127.0.0.1:<port>`: the host of a relay address (relay.md §5.4).
    pub fn host(&self) -> &str {
        &self.host
    }

    /// `https://127.0.0.1:<port>`: the relay origin the standby was configured with.
    pub fn origin(&self) -> String {
        format!("https://{}", self.host)
    }

    /// The token key, 64 lowercase hex characters. Never print it.
    pub fn token_key(&self) -> &str {
        &self.token_key
    }

    /// The create secret, 64 lowercase hex characters: what `create` takes.
    /// Never print it.
    pub fn create_secret(&self) -> &str {
        &self.create_secret
    }

    /// The folder of the relay's board files (`<board hex>.sqlite`).
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// The file the standby's stderr is written to.
    pub fn log_path(&self) -> &Path {
        &self.log
    }

    /// Stops the standby (its standard input closes; it closes its server and
    /// exits) and waits for it, killing it after a timeout.
    pub fn stop(&mut self) {
        let Some(mut child) = self.child.take() else { return };
        drop(child.stdin.take());
        let end = Instant::now() + STOP_TIMEOUT;
        while Instant::now() < end {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = child.kill();
        let _ = child.wait();
    }
}

impl Drop for Standby {
    fn drop(&mut self) {
        self.stop();
        if std::thread::panicking() {
            let log = std::fs::read_to_string(&self.log).unwrap_or_default();
            eprintln!("--- standby ({}) stderr ---\n{log}--- end of standby stderr ---", self.host);
        }
    }
}

/// Reads the one line the launcher prints, under a timeout, or says why not.
fn read_url(child: &mut Child) -> Result<String, String> {
    let stdout = child.stdout.take().expect("piped stdout");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let n = BufReader::new(stdout.take(4096)).read_line(&mut line);
        let _ = tx.send(n.map(|_| line));
    });
    match rx.recv_timeout(START_TIMEOUT) {
        Ok(Ok(line)) if line.ends_with('\n') => Ok(line.trim_end().to_string()),
        Ok(Ok(_)) => Err(format!("the standby exited before it listened ({:?})", child.try_wait())),
        Ok(Err(e)) => Err(format!("reading the standby's URL: {e}")),
        Err(_) => Err("the standby did not report its URL in time".into()),
    }
}
