//! Tests of the harness itself: the standby alone, and the device driver
//! against a stand-in process (the real `taskdeck-devicectl` is F3's).

use crate::harness::{Ctl, CtlOptions, CtlError, Standby, http_get, hex_encode};
use serde_json::json;
use std::time::Duration;

/// The relay's refusal of a request without credentials (relay.md §6.2 step 6,
/// §4.2): `401`, `X-TD-Relay: gate`, a JSON body with the code
/// `relay/unauthorized`.
#[test]
fn standby_answers_a_hello_without_credentials() {
    let standby = Standby::start();
    let board = hex_encode(&[0x5a; 16]);
    let r = http_get(standby.url(), &format!("/v1/b/{board}/hello"), &[]);
    assert_eq!(r.status, 401, "{r:?}");
    assert_eq!(r.header("x-td-relay"), Some("gate"), "{r:?}");
    assert!(r.header("content-type").is_some_and(|t| t.starts_with("application/json")), "{r:?}");
    let body: serde_json::Value = serde_json::from_slice(&r.body).expect("a JSON body");
    assert_eq!(body, json!({"error": "relay/unauthorized"}));
}

/// A route the standby does not have is a gate refusal too, and the harness's
/// standby serves more than one request.
#[test]
fn standby_serves_several_requests_and_refuses_an_unknown_route() {
    let standby = Standby::start();
    let r = http_get(standby.url(), "/nothing", &[]);
    assert_eq!(r.status, 404, "{r:?}");
    assert_eq!(r.header("x-td-relay"), Some("gate"), "{r:?}");
    let board = hex_encode(&[1; 16]);
    let r = http_get(standby.url(), &format!("/v1/b/{board}/hello"), &[("Authorization", "Door 0000000000")]);
    assert_eq!(r.status, 401, "{r:?}");
}

/// The origin the standby signs into logins is the address its devices use.
#[test]
fn standby_origin_names_its_own_host() {
    let standby = Standby::start();
    assert_eq!(standby.origin(), format!("https://{}", standby.host()));
    assert_eq!(standby.url(), format!("http://{}", standby.host()));
    assert_eq!(standby.token_key().len(), 64);
    assert_eq!(standby.create_secret().len(), 64);
}

const STAND_IN: &str = include_str!("stand_in_device.mjs");

fn stand_in(dir: &std::path::Path, name: &str) -> Ctl {
    let script = dir.join("stand_in_device.mjs");
    std::fs::write(&script, STAND_IN).unwrap();
    let node = crate::harness::find_node();
    let mut o = CtlOptions::in_dir(dir, name);
    o.program = node;
    o.leading_args = vec![script.into_os_string()];
    o.timeout = Duration::from_secs(10);
    Ctl::start(o)
}

/// Requests carry increasing ids, answers are matched by id, events are
/// collected on the side, `shutdown` returns the exit code, and a device is
/// started again on the same folders.
#[test]
fn ctl_drives_a_device_by_the_protocol() {
    let dir = tempfile::tempdir().unwrap();
    let mut ctl = stand_in(dir.path(), "desk");
    let a = ctl.status();
    assert_eq!(a["id"], 1);
    assert_eq!(a["ok"], true);
    assert_eq!(a["argv_has_name"], true, "{a}");
    let c = ctl.clock_advance(1500);
    assert_eq!(c, 1_767_614_401_500);
    let ev = ctl.take_events();
    assert!(ev.iter().any(|e| e["event"] == "changed"), "{ev:?}");
    let err = ctl.call_err("nonsense", json!({}));
    assert_eq!(err, "device/ctl_request");
    ctl.confirm_sheet("words go here");
    assert_eq!(ctl.board().state_hash.len(), 64);
    let code = ctl.shutdown();
    assert_eq!(code, 0);
    assert!(ctl.take_events().iter().any(|e| e["event"] == "exit" && e["code"] == 0));
    // Again, on the same folders: the stand-in keeps a counter in its data folder.
    ctl.restart();
    assert_eq!(ctl.status()["id"], 1);
    assert_eq!(ctl.status()["starts"], 2);
    assert_eq!(ctl.shutdown(), 0);
}

/// `kill` ends the process at once and without an answer; the harness then
/// starts it again.
#[test]
fn ctl_kills_and_restarts_a_device() {
    let dir = tempfile::tempdir().unwrap();
    let mut ctl = stand_in(dir.path(), "laptop");
    assert_eq!(ctl.status()["ok"], true);
    let status = ctl.kill();
    assert!(!status.success());
    ctl.restart();
    assert_eq!(ctl.status()["starts"], 2);
    ctl.shutdown();
}

/// Every wait has a timeout, and a timeout is an error that names the last
/// lines of the device's output and stderr.
#[test]
fn ctl_times_out_with_the_last_lines() {
    let dir = tempfile::tempdir().unwrap();
    let mut ctl = stand_in(dir.path(), "slow");
    let e = ctl
        .try_call("stall", json!({}), Duration::from_millis(300))
        .expect_err("no answer");
    match &e {
        CtlError::Timeout { context, .. } => {
            assert!(context.contains("stand-in: stalling"), "{context}");
        }
        other => panic!("{other:?}"),
    }
    ctl.kill();
}

/// The device's `sheet` and `pair_start` answers carry secrets: the harness
/// never prints them (DEV-CTL-002), neither in `Debug` nor in the context of a
/// failure.
#[test]
fn ctl_never_prints_a_seed_or_a_code() {
    let dir = tempfile::tempdir().unwrap();
    let mut ctl = stand_in(dir.path(), "secret");
    let sheet = ctl.sheet();
    assert_eq!(sheet.sheet, "seed words seed words");
    let pair = ctl.pair_start();
    assert_eq!(pair.code, "123456789");
    assert!(!format!("{sheet:?}{pair:?}").contains("seed words"));
    assert!(!format!("{pair:?}").contains("123456789"));
    let e = ctl.try_call("stall", json!({"text": "typed sheet words", "code": "987654321"}), Duration::from_millis(300)).unwrap_err();
    let shown = format!("{e}");
    for secret in ["seed words", "123456789", "typed sheet words", "987654321"] {
        assert!(!shown.contains(secret), "{shown}");
    }
    ctl.kill();
}

/// `wait_until` polls `wait` with the change counters.
#[test]
fn ctl_wait_until_polls_the_counters() {
    let dir = tempfile::tempdir().unwrap();
    let mut ctl = stand_in(dir.path(), "poll");
    let c = ctl.wait_until(|c| c["any"].as_u64() >= Some(3), Duration::from_secs(5));
    assert!(c["any"].as_u64() >= Some(3));
    ctl.shutdown();
}
