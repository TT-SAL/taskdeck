//! Tests of the rules that hold for every implementation (§3), of the model
//! (§4) and of "no relay answer deletes anything" (§11.8).

use super::kit_tests::{Hub, creator};
use super::script_tests::{OTHER, Plan, SNode, ScriptM};
use crate::toy::ToyCommand;
use crate::{
    AppendResult, Effect, Event, Head, Input, Notification, OutboxOp, Page, PageEntry, PersistOp, RelayRefusal,
    SessionState, StopKind,
};

/// Every source file of the engine and of the types it is built on (not the tests, not the
/// toy, the fake or the simulator).
const SOURCES: &[(&str, &str)] = &[
    ("lib.rs", include_str!("../lib.rs")),
    ("model.rs", include_str!("../model.rs")),
    ("events.rs", include_str!("../events.rs")),
    ("records.rs", include_str!("../records.rs")),
    ("session.rs", include_str!("../session.rs")),
    ("receive.rs", include_str!("../receive.rs")),
    ("snapshots.rs", include_str!("../snapshots.rs")),
    ("ops.rs", include_str!("../ops.rs")),
    ("ui.rs", include_str!("../ui.rs")),
    ("engine/mod.rs", include_str!("mod.rs")),
    ("engine/start.rs", include_str!("start.rs")),
    ("engine/outbox.rs", include_str!("outbox.rs")),
    ("engine/send.rs", include_str!("send.rs")),
    ("engine/sess.rs", include_str!("sess.rs")),
    ("engine/recv.rs", include_str!("recv.rs")),
    ("engine/board.rs", include_str!("board.rs")),
    ("engine/replay.rs", include_str!("replay.rs")),
    ("engine/repair.rs", include_str!("repair.rs")),
    ("engine/boot.rs", include_str!("boot.rs")),
    ("engine/duty.rs", include_str!("duty.rs")),
    ("engine/evid.rs", include_str!("evid.rs")),
    ("engine/epoch.rs", include_str!("epoch.rs")),
    ("engine/ops.rs", include_str!("ops.rs")),
    ("engine/movement.rs", include_str!("movement.rs")),
    ("engine/status.rs", include_str!("status.rs")),
];

// spec: ENG-GEN-001 ENG-GEN-003 ENG-GEN-006 ENG-GEN-007
#[test]
fn the_engine_has_no_clock_file_network_thread_hash_order_or_float() {
    assert!(SOURCES[0].1.contains("#![forbid(unsafe_code)]"));
    let banned = [
        "HashMap", "HashSet", "RandomState", "f32", "f64", "SystemTime", "Instant", "std::fs", "std::net",
        "std::thread", "thread_local", "std::env", "std::process", "unsafe ",
    ];
    for (name, src) in SOURCES {
        for b in banned {
            assert!(!src.contains(b), "{name} contains {b}");
        }
        // SHA-256 only through the `sha2` crate: no hash, MAC or cipher of the engine's own.
        if src.contains("Sha256") {
            assert!(src.contains("sha2::"), "{name} hashes without sha2");
        }
        for own in ["fn sha256", "fn hmac", "fn compress(", "rotate_right("] {
            assert!(!src.contains(own), "{name} contains {own}");
        }
    }
}

// spec: ENG-GEN-002 ENG-GEN-004 ENG-GEN-005 ENG-TOY-002
#[test]
fn the_manifest_the_lints_and_the_ci_hook_say_what_the_spec_requires() {
    let manifest = include_str!("../../Cargo.toml");
    let deps = manifest.split("[dependencies]").nth(1).unwrap().split("\n[").next().unwrap();
    let mut names: Vec<&str> = deps
        .lines()
        .filter(|l| !l.trim_start().starts_with('#') && l.contains('='))
        .map(|l| l.split('=').next().unwrap().trim())
        .collect();
    names.sort_unstable();
    assert_eq!(names, vec!["sha2", "zeroize"], "the non-development dependencies");
    // No default feature enables `sim`.
    let features = manifest.split("[features]").nth(1).unwrap().split("\n[").next().unwrap();
    assert!(features.lines().any(|l| l.trim() == "sim = []"));
    assert!(!features.lines().any(|l| l.trim_start().starts_with("default")));
    let clippy = include_str!("../../clippy.toml");
    for path in ["std::time::SystemTime::now", "std::time::Instant::now", "std::thread::spawn", "std::fs::", "std::net::"] {
        assert!(clippy.contains(path), "clippy.toml lists {path}");
    }
    // The wasm32 builds, with the default features and with `sim`, run in CI on every system.
    let ci = include_str!("../../ci.sh");
    assert!(ci.contains("cargo check --locked -p taskdeck-sync --target wasm32-unknown-unknown"));
    assert!(ci.contains("cargo check --locked -p taskdeck-sync --features sim --target wasm32-unknown-unknown"));
}

// spec: ENG-GEN-010 ENG-GEN-011 ENG-GEN-013
#[test]
fn secrets_stay_out_of_what_the_engine_shows() {
    let (mut n, f1) = creator(21);
    let mut hub = Hub::with_genesis(&n, &f1);
    hub.settle(&mut [&mut n]);
    let identity = n.identity0.clone();
    let hexed = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
    let secret = hexed(&identity[24..56]); // the device's secret key in the fake's identity layout
    let fx = Effect::Persist { id: 1, ops: vec![PersistOp::WriteIdentity(zeroize::Zeroizing::new(identity.clone()))] };
    let shown = format!("{fx:?}");
    assert!(shown.contains("WriteIdentity(<") && !shown.contains(&format!("{:?}", &identity[24..56])));
    let st = format!("{:?} {:?} {:?}", n.e.status(), n.e.pending(), n.e.notices());
    assert!(!st.contains(&secret));
    assert!(!st.contains(&format!("{:?}", &identity[24..56])));
    // The engine compares no secret: the move code goes to the sealer (the only caller of
    // `check_move_code`), and equality on effects exists only in test builds.
    let movement = include_str!("movement.rs");
    assert!(movement.contains("check_move_code"));
    for (name, src) in SOURCES {
        assert!(!src.contains("export_identity() =="), "{name} compares the identity");
    }
}

// spec: ENG-GEN-009 ENG-MODEL-004
#[test]
fn events_beyond_the_limits_or_out_of_time_are_ignored() {
    let mut n = SNode::new(10, ScriptM { my_ctr: 3, other_ctr: 1, ..Default::default() }, vec![]);
    let head = Head { seq: 10, chain: n.chain_b };
    n.live(head);
    let before = n.persists.len();
    // 257 entries in a page.
    let mut frames = Vec::new();
    for i in 0..257u64 {
        let f = n.s.frame(OTHER, 2 + i);
        n.s.plan(&f, Plan::accept_cmd(OTHER, 2 + i, vec![3, 77]));
        frames.push(f);
    }
    n.page(11, &frames);
    // A frame below 670 bytes.
    let small = Page {
        head: Head { seq: 11, chain: [1; 32] },
        relay_now: n.wall,
        floor: 1,
        purge_floor: 0,
        more: false,
        truncated: false,
        fenced: false,
        entries: vec![PageEntry { seq: 11, at: 0, frame: vec![0; 669] }],
    };
    n.feed(Event::Page { req: None, page: small });
    assert_eq!(n.persists.len(), before, "nothing of an oversized page is persisted");
    assert_eq!(n.e.status().head.seq, 10);
    // More results than a batch has frames.
    n.e.send.append = Some((77, vec![[1; 16]]));
    n.feed(Event::AppendResults { req: 77, results: vec![AppendResult::Skipped; 17] });
    // The third violation of one kind is a persistent local fault (§21.4).
    n.feed(Event::MissedRoots { first_epoch: 0, wraps: vec![] });
    n.feed(Event::MissedRoots { first_epoch: 1, wraps: vec![[0; 96]; 257] });
    n.feed(Event::MissedRoots { first_epoch: 1, wraps: vec![] });
    assert!(n.e.status().stops.iter().any(|s| matches!(&s.kind, StopKind::LocalFault { code } if code == "engine/runtime_protocol")));
    // An input whose monotonic clock goes back is ignored.
    let mut n = SNode::new(10, ScriptM { my_ctr: 3, other_ctr: 1, ..Default::default() }, vec![]);
    let notes = n.notes.len();
    let out = n.e.handle(Input {
        wall_ms: n.wall,
        mono_ms: n.mono - 1,
        event: Event::Enqueue {
            ticket: 9,
            command: ToyCommand::Set { key: 1, value: 1 },
            at_ms: n.wall,
            origin_state: vec![],
            hold: false,
        },
    });
    assert!(out.iter().all(|f| !matches!(f, Effect::Persist { .. })));
    assert!(!out.iter().any(|f| matches!(f, Effect::Notify(Notification::Enqueued { .. } | Notification::EnqueueRefused { .. }))));
    assert_eq!(n.notes.len(), notes);
    assert!(n.e.pending().is_empty());
}

// spec: ENG-MODEL-003 ENG-WINDOW-001 ENG-CLOCK-001
#[test]
fn no_clock_decides_what_is_accepted_or_applied() {
    let run = |wall: i64, mono: u64, relay_now: i64| {
        let mut n = SNode::new(10, ScriptM { my_ctr: 3, other_ctr: 1, ..Default::default() }, vec![]);
        n.wall = wall;
        n.mono = mono;
        let head = Head { seq: 10, chain: n.chain_b };
        n.live(head);
        let mut frames = Vec::new();
        for i in 0..5u64 {
            let f = n.s.frame(OTHER, 2 + i);
            let mut p = Plan::accept_cmd(OTHER, 2 + i, vec![1, 20 + i as u8, 0, 0, 0, 0, 0, 0, 0, i as u8]);
            p.at_ms = 1_600_000_000_000 + i as i64; // far from every clock of the receiver
            n.s.plan(&f, p);
            frames.push(f);
        }
        let mut chain = n.chain_b;
        let mut entries = Vec::new();
        for (i, f) in frames.iter().enumerate() {
            let seq = 11 + i as u64;
            chain = crate::Sealer::chain_step(&n.s, &chain, seq, f);
            entries.push(PageEntry { seq, at: relay_now, frame: f.clone() });
        }
        let page = Page {
            head: Head { seq: 15, chain },
            relay_now,
            floor: 1,
            purge_floor: 0,
            more: false,
            truncated: false,
            fenced: false,
            entries,
        };
        n.feed(Event::Page { req: None, page });
        (n.board_bytes(), n.e.status().board_seq, n.e.status().alarms.len())
    };
    let a = run(1_700_000_000_000, 20_000, 1_700_000_000_000);
    let b = run(1_900_000_000_000, 9_000_000_000, 1_000_000_000_000);
    assert_eq!(a, b);
    assert_eq!(a.1, 15);
    // Every at_ms the engine composes is the local clock, never corrected by the relay's.
    let mut n = SNode::new(10, ScriptM { my_ctr: 3, other_ctr: 1, ..Default::default() }, vec![]);
    let head = Head { seq: 10, chain: n.chain_b };
    n.feed(Event::Connected { push: true });
    let mut w = n.welcome(head);
    w.relay_now = n.wall - 1_000_000; // a relay clock far behind: only the guard looks at it
    n.feed(Event::Welcome(w));
    n.feed(Event::Enqueue {
        ticket: 1,
        command: ToyCommand::Set { key: 1, value: 1 },
        at_ms: n.wall,
        origin_state: vec![0; 8],
        hold: false,
    });
    assert_eq!(n.e.outbox[0].r.at_ms, n.wall);
    assert_eq!(n.e.status().clock.skew_ms, Some(1_000_000));
}

// spec: ENG-MODEL-005 ENG-EFFECT-002 ENG-EFFECT-003
#[test]
fn one_schedule_tick_before_the_notifications_and_one_request_of_a_kind() {
    let (mut n, f1) = creator(22);
    let mut hub = Hub::with_genesis(&n, &f1);
    hub.settle(&mut [&mut n]);
    let mut reads = 0i32;
    let mut appends = 0i32;
    for k in 0..40u8 {
        let out = n.e.handle(Input {
            wall_ms: n.wall,
            mono_ms: n.mono,
            event: Event::Enqueue {
                ticket: k as u64,
                command: ToyCommand::Set { key: k, value: 1 },
                at_ms: n.wall,
                origin_state: vec![0; 8],
                hold: false,
            },
        });
        let ticks = out.iter().filter(|f| matches!(f, Effect::ScheduleTick { .. })).count();
        assert!(ticks <= 1);
        if let Some(i) = out.iter().position(|f| matches!(f, Effect::ScheduleTick { .. })) {
            assert!(out[..i].iter().all(|f| !matches!(f, Effect::Notify(_))), "the tick comes before the notifications");
        }
        let first_note = out.iter().position(|f| matches!(f, Effect::Notify(_))).unwrap_or(out.len());
        assert!(out[first_note..].iter().all(|f| matches!(f, Effect::Notify(_))), "notifications come last");
        for f in &out {
            match f {
                Effect::Read { .. } => reads += 1,
                Effect::Append { .. } => appends += 1,
                _ => {}
            }
        }
        assert!(reads <= 1 && appends <= 1, "one read and one append outstanding at most");
        n.absorb(out);
    }
    // An answer with an id the engine did not issue is ignored.
    let before = n.e.status();
    n.feed(Event::AppendResults { req: 9_999, results: vec![AppendResult::Skipped] });
    n.feed(Event::Persisted { id: 9_999 });
    assert_eq!(n.e.status().queued, before.queued);
    hub.settle(&mut [&mut n]);
    assert_eq!(n.e.confirmed().0.keys.len(), 40);
}

// spec: ENG-PERSIST-003 ENG-EVID-006
#[test]
fn a_failed_write_stops_the_engine_until_the_next_start() {
    let (mut n, f1) = creator(23);
    let mut hub = Hub::with_genesis(&n, &f1);
    hub.settle(&mut [&mut n]);
    n.auto = false;
    n.set(1, 1, 1);
    let (id, _) = n.pq.pop_front().expect("the enqueue's write");
    n.pq.clear();
    n.fx.clear();
    n.feed(Event::PersistFailed { id });
    let st = n.e.status();
    assert!(st.stops.iter().any(|s| matches!(&s.kind, StopKind::LocalFault { code } if code == "engine/persist_failed")));
    assert!(st.alarms.is_empty(), "a local fault is not an integrity alarm");
    // No further Persist, Append, Admin or Blob until the next Start.
    n.set(2, 2, 2);
    n.tick(60_000);
    assert!(n.pq.is_empty());
    assert!(!n.fx.iter().any(|f| matches!(f, Effect::Append { .. } | Effect::Admin { .. } | Effect::Blob { .. })));
    // The unconfirmed write is not done: after a restart the record is gone, and the fault too.
    n.auto = true;
    n.restart(24);
    assert!(n.e.pending().is_empty());
    assert!(n.e.status().stops.is_empty());
}

// spec: ENG-SESS-005 ENG-SESS-006 ENG-PERSIST-005 ENG-STATE-004
#[test]
fn no_relay_answer_deletes_anything() {
    let (mut n, f1) = creator(25);
    let mut hub = Hub::with_genesis(&n, &f1);
    hub.settle(&mut [&mut n]);
    for k in 0..6u8 {
        n.set(k as u64, k, 1);
    }
    hub.settle(&mut [&mut n]);
    n.set(100, 100, 1); // an unapplied record
    let before = n.persists.len();
    let refusal = |status: u16, code: &str| RelayRefusal { status, code: code.into(), until: None, head: None, index: None };
    for (status, code) in [(404, "relay/no_board"), (403, "relay/revoked"), (403, "relay/fenced"), (409, "relay/seeding"), (500, "relay/whatever")] {
        n.feed(Event::RelayRefused { req: None, refusal: refusal(status, code) });
    }
    // A relay that claims the log is gone: the device asks its user, and replaces nothing.
    let head = n.e.status().head;
    let truncated = Page {
        head: Head { seq: head.seq + 500, chain: [3; 32] },
        relay_now: n.wall,
        floor: head.seq + 400,
        purge_floor: 0,
        more: true,
        truncated: true,
        fenced: false,
        entries: vec![],
    };
    n.feed(Event::Page { req: None, page: truncated });
    assert_eq!(n.e.status().session, SessionState::Rejoining);
    n.feed(Event::Disconnected { unreachable: true, paused_until_ms: None });
    n.tick(4_000_000);
    for ops in &n.persists[before..] {
        for op in ops {
            let deletes = match op {
                PersistOp::TruncateLogBelow(..) | PersistOp::DeleteBases(_) | PersistOp::RewriteOutbox(_) | PersistOp::WriteIdentity(_) => true,
                PersistOp::Outbox(v) => v.iter().any(|o| matches!(o, OutboxOp::Drop(_))),
                _ => false,
            };
            assert!(!deletes, "a relay answer led to {op:?}");
        }
    }
    assert_eq!(n.e.pending().len(), 1, "the unapplied record is kept");
    assert_eq!(n.e.status().head, head, "the log is kept");
    assert!(n.store.log.len() as u64 == head.seq);
}
