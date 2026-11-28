//! The milestone "device: first sync" (device.md §20; the stage's status,
//! decision 2): devices with the real core and seal in the engine, driven
//! through `taskdeck-devicectl` against the standby on loopback.
//!
//! The clock is manual: whatever waits on a tick (the 30,000 ms poll of
//! `hello` while `Live`, the pairing machines' 2,000 ms polls and their
//! ticks) moves only when the test advances the clock, so `settle` advances
//! it in steps and waits on the change counters between them.

use crate::harness::{Ctl, Source, Standby};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

/// How long one condition may take, in real time.
const SETTLE: Duration = Duration::from_secs(60);

/// Advances the clock by `step_ms` and waits for a change, until `done`
/// accepts the device's status and its confirmed board's text.
fn settle(dev: &mut Ctl, step_ms: u64, what: &str, mut done: impl FnMut(&Value, &str) -> bool) -> Value {
    let end = Instant::now() + SETTLE;
    let mut since = 0;
    loop {
        let s = dev.status();
        let board = if s["mode"] == json!("synced") { text(&dev.board().canonical) } else { String::new() };
        if done(&s, &board) {
            return s;
        }
        assert!(Instant::now() < end, "{what}: not reached in {SETTLE:?}; last status {s}\n{}", dev.context());
        dev.clock_advance(step_ms);
        let c = dev.wait(since, 300);
        since = c["any"].as_u64().unwrap_or(since);
    }
}

/// The real time in ms: the standby's clock.
fn real_now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64
}

/// Starts a device and puts its manual clock beside the standby's: the
/// engine's clock guard blocks composing above an hour of skew
/// (`engine.md` §27.2), and the standby runs on the real clock, while
/// `--clock manual` starts at 2026-01-05 (device.md §19.1; finding f3-*).
fn start(dir: &std::path::Path, name: &str) -> Ctl {
    let mut c = Ctl::start_in(dir, name);
    c.clock_set(real_now_ms());
    c
}

/// Restarts a device on the same folders, its clock set again, never back.
fn restart(c: &mut Ctl, last_wall: i64) {
    c.restart();
    c.clock_set(real_now_ms().max(last_wall));
}

fn engine(s: &Value) -> &Value {
    &s["engine"]
}

/// Live, nothing queued, the board at the relay head `head` (or any head
/// when `None`), no halt and no stop.
fn synced(s: &Value, head: Option<u64>) -> bool {
    let e = engine(s);
    e["session"] == json!("live")
        && e["queued"] == json!(0)
        && e["halt"].is_null()
        && e["stops"] == json!([])
        && e["board_seq"] == e["head"]["seq"]
        && head.is_none_or(|h| e["head"]["seq"] == json!(h))
}

fn head(s: &Value) -> u64 {
    engine(s)["head"]["seq"].as_u64().expect("a head")
}

/// Creates a board on `standby` from an empty board and returns its address.
fn create(dev: &mut Ctl, standby: &Standby, name: &str) -> String {
    dev.create(standby.url(), standby.create_secret(), Source::Empty, name);
    let sheet = dev.sheet();
    dev.confirm_sheet(&sheet.sheet);
    let address = dev.finish_create();
    assert_eq!(address, sheet.address, "the sheet names the relay address");
    address
}

fn add(name: &str) -> Value {
    json!({"op": "add", "name": name})
}

/// Part A (plan §6.7 item 2): one device end to end through the standby.
/// It creates a board, submits an intent, and the entry is appended, read
/// back and applied (the status shows it confirmed: nothing queued, the
/// board at the relay head); after `shutdown` and a new start on the same
/// folders it holds the same board with the same state hash and takes
/// another edit.
#[test]
fn one_device_round_trip() {
    let standby = Standby::start();
    let dir = tempfile::tempdir().unwrap();
    let mut desk = start(dir.path(), "desk");
    let address = create(&mut desk, &standby, "desk");
    assert!(address.starts_with(&format!("{}/", standby.host())), "the address names the standby's host");

    // Genesis accepted and the session live at head 1.
    let s = settle(&mut desk, 30_000, "genesis", |s, _| synced(s, None));
    assert_eq!(s["mode"], json!("synced"));
    let genesis_head = head(&s);
    let before = desk.board();

    // An edit: enqueued, appended, read back, applied.
    let a = desk.submit(add("Water the plants"));
    assert!(a["provisional_seq"].as_u64().is_some(), "a provisional seq: {a}");
    let s = settle(&mut desk, 30_000, "the edit", |s, b| {
        synced(s, None) && head(s) > genesis_head && b.contains("Water the plants")
    });
    let after = desk.board();
    assert_ne!(after.canonical, before.canonical, "the edit changed the confirmed board");
    let head1 = head(&s);

    // Stop and start again on the same folders.
    let wall = desk.clock_advance(0);
    assert_eq!(desk.shutdown(), 0);
    restart(&mut desk, wall);
    let s = desk.status();
    assert_eq!(s["mode"], json!("synced"));
    let again = desk.board();
    assert_eq!(again.canonical, after.canonical, "the same board after the restart");
    assert_eq!(again.state_hash, after.state_hash, "the same state hash after the restart");
    let s = settle(&mut desk, 30_000, "after the restart", |s, _| synced(s, None));
    assert!(head(&s) >= head1);

    // Another edit after it.
    desk.submit(add("Feed the cat"));
    let h = head(&s);
    settle(&mut desk, 30_000, "the second edit", |s, b| synced(s, None) && head(s) > h && b.contains("Feed the cat"));
    let last = desk.board();
    assert!(text(&last.canonical).contains("Water the plants"), "the first edit is kept");
    assert_eq!(desk.shutdown(), 0);
}

/// The board's canonical bytes (hex) as text.
fn text(hex: &str) -> String {
    let b: Vec<u8> = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex")).collect();
    String::from_utf8(b).expect("canonical JSON")
}

/// Advances both devices' clocks by `step_ms` and waits a little, until
/// `done` accepts both statuses and confirmed boards.
fn settle_both(
    a: &mut Ctl,
    b: &mut Ctl,
    step_ms: u64,
    what: &str,
    mut done: impl FnMut(&Value, &str, &Value, &str) -> bool,
) -> (Value, Value) {
    let end = Instant::now() + SETTLE;
    let board = |c: &mut Ctl, s: &Value| if s["mode"] == json!("synced") { text(&c.board().canonical) } else { String::new() };
    loop {
        let (sa, sb) = (a.status(), b.status());
        let (ba, bb) = (board(a, &sa), board(b, &sb));
        if done(&sa, &ba, &sb, &bb) {
            return (sa, sb);
        }
        assert!(
            Instant::now() < end,
            "{what}: not reached in {SETTLE:?}\nlast statuses:\n{sa}\n{sb}\n{}\n{}",
            a.context(),
            b.context()
        );
        for c in [&mut *a, &mut *b] {
            c.clock_advance(step_ms);
            let since = c.wait(0, 0)["any"].as_u64().unwrap_or(0);
            c.wait(since, 100);
        }
    }
}

fn pairing(s: &Value) -> &str {
    s["pairing"]["state"].as_str().unwrap_or("")
}

/// Both devices synced at one head with the same confirmed state.
fn converged(sa: &Value, sb: &Value) -> bool {
    synced(sa, None) && synced(sb, None) && head(sa) == head(sb)
}

/// Part B, the first sync: device A creates the board, device B joins with
/// the address and code of A's `pair_start`, A shows the six digits, B
/// types them (`join_sas`), A confirms; B holds the board with A's state
/// hash; an edit on each reaches the other; both are stopped and started
/// again and still hold the board with that hash.
#[test]
fn first_sync() {
    let standby = Standby::start();
    let dir = tempfile::tempdir().unwrap();
    let mut desk = start(dir.path(), "desk");
    create(&mut desk, &standby, "desk");
    settle(&mut desk, 30_000, "genesis", |s, _| synced(s, None));

    // Pairing (device.md §18.3; seal.md §17.10).
    let mut laptop = start(dir.path(), "laptop");
    assert_eq!(laptop.status()["mode"], json!("local_only"));
    let sheet = desk.pair_start();
    assert_eq!(pairing(&desk.status()), "showing_code");
    laptop.join(&sheet.address, &sheet.code, "laptop");
    // E polls slot B, N polls slot C, every 2,000 ms of each one's clock.
    let (sd, sl) = settle_both(&mut desk, &mut laptop, 2_000, "the six digits", |a, _, b, _| {
        pairing(a) == "show_sas" && pairing(b) == "ask_sas"
    });
    let digits = sd["pairing"]["sas"].as_str().expect("E shows the digits").to_string();
    assert_eq!(digits.len(), 6);
    assert!(sl["pairing"]["sas"].is_null(), "N never shows digits");
    laptop.join_sas(&digits);
    assert_eq!(pairing(&laptop.status()), "accepted");
    desk.pair_confirm();
    // E registers N and posts the bundle; N reads it, the snapshot and the
    // log, and starts with `Bootstrap::Join`.
    settle_both(&mut desk, &mut laptop, 2_000, "the join", |a, _, b, _| pairing(a) == "done" && pairing(b) == "done");
    assert_eq!(laptop.status()["mode"], json!("synced"));

    // B holds the board with A's state hash.
    settle_both(&mut desk, &mut laptop, 30_000, "B holds the board", |a, _, b, _| converged(a, b));
    let (bd, bl) = (desk.board(), laptop.board());
    assert_eq!(bl.canonical, bd.canonical, "the same board");
    assert_eq!(bl.state_hash, bd.state_hash, "the same state hash");

    // An edit on A reaches B, and one on B reaches A.
    desk.submit(add("From the desk"));
    settle_both(&mut desk, &mut laptop, 30_000, "A's edit on B", |a, ba, b, bb| {
        converged(a, b) && ba.contains("From the desk") && bb.contains("From the desk")
    });
    laptop.submit(add("From the laptop"));
    settle_both(&mut desk, &mut laptop, 30_000, "B's edit on A", |a, ba, b, bb| {
        converged(a, b) && ba.contains("From the laptop") && bb.contains("From the laptop")
    });
    let (bd, bl) = (desk.board(), laptop.board());
    assert_eq!(bl.state_hash, bd.state_hash, "both state hashes equal again");

    // Both stopped and started again.
    let (wd, wl) = (desk.clock_advance(0), laptop.clock_advance(0));
    assert_eq!(desk.shutdown(), 0);
    assert_eq!(laptop.shutdown(), 0);
    restart(&mut desk, wd);
    restart(&mut laptop, wl);
    assert_eq!(desk.board().state_hash, bd.state_hash, "A holds the board after the restart");
    assert_eq!(laptop.board().state_hash, bd.state_hash, "B holds the board after the restart");
    settle_both(&mut desk, &mut laptop, 30_000, "after the restart", |a, _, b, _| converged(a, b));
    let (bd2, bl2) = (desk.board(), laptop.board());
    assert_eq!(bd2.state_hash, bl2.state_hash);
    assert!(text(&bd2.canonical).contains("From the desk") && text(&bd2.canonical).contains("From the laptop"));
    assert_eq!(desk.shutdown(), 0);
    assert_eq!(laptop.shutdown(), 0);
}
