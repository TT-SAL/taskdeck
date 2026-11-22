//! Transcripts: wire commands sent to today's board one by one, with the
//! answer and the whole state after each.
//!
//! A step is answered the way `taskdeck-server` answers `POST /api/command`:
//! the body is parsed as a `Command` (a failure is 400 "Could not read the
//! command: ..."), a query is refused (400), anything else goes to
//! `Board::apply` at the step's `now`, and a carried-out command answers
//! `phone::reply_json` with status 200, a refusal `{"error": message}` with
//! its status. The request-key replay cache and the HTTP framing are not part
//! of a transcript.

use std::collections::BTreeMap;
use std::path::Path;

use chrono::{DateTime, Duration, Local, SecondsFormat, Utc};
use serde_json::{Value, json};
use task_deck::board::{Board, Command, NAME_MAX_CHARS, NOTES_MAX_CHARS};
use task_deck::phone;
use tempfile::TempDir;

use crate::boards::WORDS;
use crate::clock::{Zone, text, utc};
use crate::common::{open_copy, state};
use crate::out;
use crate::rng::SplitMix64;

/// Every operation the wire knows. The last three are queries, which
/// `/api/command` refuses.
pub const OPS: [&str; 28] = [
    "add", "quick_add", "rename", "complete", "delete", "forget", "move_block", "add_block", "remove_block", "unplan",
    "set_estimate", "set_deadline", "set_severity", "set_horizon", "set_repeat", "create", "reflow", "set_notes",
    "restore", "forget_archived", "add_subscription", "remove_subscription", "rename_subscription",
    "set_subscription_color", "set_subscription_enabled", "snapshot", "feed", "board",
];

/// Every refusal today's code can give a well-formed request on a working
/// disk, by message. Not reachable here and so not listed: "That time doesn't
/// exist on this day (daylight saving)." (neither pinned zone has a gap of
/// more than the hour `resolve_on_day` steps over) and the 500s of a failed
/// save.
pub const REFUSALS: [&str; 30] = [
    "Could not read the command: ",
    "That is a query, not a command.",
    "It needs a name.",
    "An event needs a time.",
    "That date is out of range (1970\u{2013}9999).",
    "No such severity.",
    "No such horizon.",
    "A task needs a name.",
    "A name is at most 200 characters.",
    "A name cannot be empty.",
    "That item is no longer on the board.",
    "Only a task can be finished \u{2014} an event or a routine is deleted instead.",
    "That block is no longer there.",
    "Only a task can be given a block of time.",
    "Only a task can be unplanned.",
    "A length is between fifteen minutes and a day.",
    "Only a task has a due date; an event's time is its time.",
    "Only a task has a severity.",
    "Only a task has a horizon.",
    "Only a routine repeats.",
    "Those notes are too long for the notepad.",
    "That is no longer in the archive.",
    "That is already 20 calendars, which is as many as this keeps.",
    "An address is needed.",
    "That address is too long.",
    "Use https \u{2014} a calendar link is a password, and http sends it in clear.",
    "An address should start with https://",
    "That address has no host in it.",
    "That calendar is already subscribed to.",
    "That calendar is no longer subscribed to.",
];

/// Which ops were carried out and which refusals were seen, over a zone's
/// transcripts.
#[derive(Default)]
pub struct Coverage {
    pub accepted: BTreeMap<String, usize>,
    pub refused: BTreeMap<String, usize>,
    pub messages: BTreeMap<String, usize>,
}

pub struct Runner<'a> {
    pub board: Board,
    _copy: TempDir,
    pub now: DateTime<Local>,
    lines: String,
    step: usize,
    coverage: &'a mut Coverage,
}

impl<'a> Runner<'a> {
    fn start(
        out_dir: &Path,
        zone: Zone,
        name: &str,
        board_name: &str,
        seed: Option<u64>,
        now: DateTime<Local>,
        coverage: &'a mut Coverage,
    ) -> Result<Self, String> {
        let (mut board, problems, copy) = open_copy(&out_dir.join("boards").join(board_name))?;
        if !problems.is_empty() {
            return Err(format!("{board_name}: {problems:?}"));
        }
        let start = state(&mut board)?;
        let lines = format!(
            "{{\"kind\":\"start\",\"transcript\":{},\"zone\":{},\"board\":{},\"seed\":{},\"now\":{},\"state\":{start}}}\n",
            json!(name),
            json!(zone.tz()),
            json!(board_name),
            json!(seed),
            json!(text(now)),
        );
        Ok(Self { board, _copy: copy, now, lines, step: 0, coverage })
    }

    /// Send one request body exactly as written; answer `(status, body)`.
    pub fn send(&mut self, wire: &str) -> Result<(u16, Value), String> {
        let op = serde_json::from_str::<Value>(wire)
            .ok()
            .and_then(|value| value.get("op").and_then(Value::as_str).map(str::to_string));
        let (status, body) = match serde_json::from_str::<Command>(wire) {
            Err(error) => (400, json!({ "error": format!("Could not read the command: {error}") })),
            Ok(command) if command.is_query() => (400, json!({ "error": "That is a query, not a command." })),
            Ok(command) => match self.board.apply(command, self.now) {
                Ok(reply) => (200, phone::reply_json(&reply, self.board.version())),
                Err(error) => (error.status, json!({ "error": error.message })),
            },
        };
        let label = op.clone().unwrap_or_else(|| "(none)".to_string());
        if status == 200 {
            *self.coverage.accepted.entry(label).or_default() += 1;
        } else {
            *self.coverage.refused.entry(label).or_default() += 1;
            let message = body["error"].as_str().unwrap_or_default();
            let known = REFUSALS.iter().find(|known| message == **known || (known.ends_with(": ") && message.starts_with(**known)));
            *self.coverage.messages.entry(known.map_or(message, |k| k).to_string()).or_default() += 1;
        }
        self.step += 1;
        let after = state(&mut self.board)?;
        self.lines.push_str(&format!(
            "{{\"kind\":\"step\",\"step\":{},\"now\":{},\"wire\":{},\"op\":{},\"status\":{status},\"body\":{body},\"state\":{after}}}\n",
            self.step,
            json!(text(self.now)),
            json!(wire),
            json!(op),
        ));
        Ok((status, body))
    }

    pub fn wire(&mut self, value: Value) -> Result<(u16, Value), String> {
        self.send(&value.to_string())
    }

    /// Send a command that must make something; its id.
    pub fn id(&mut self, value: Value) -> Result<u64, String> {
        let text = value.to_string();
        match self.send(&text)? {
            (200, body) => body["id"].as_u64().ok_or_else(|| format!("{text}: no id")),
            (status, body) => Err(format!("{text}: {status} {body}")),
        }
    }

    pub fn tick(&mut self, minutes: i64) {
        self.now += Duration::minutes(minutes);
    }

    /// The live item whose name starts with `prefix`.
    pub fn find(&self, prefix: &str) -> Result<u64, String> {
        self.board.items.iter().find(|item| item.name.starts_with(prefix)).map(|item| item.id).ok_or_else(|| format!("no item `{prefix}`"))
    }

    /// The archive key of the newest row with this id, as the wire spells it.
    pub fn key(&mut self, id: u64) -> Result<String, String> {
        self.board.load_archive()?;
        self.board.archive.entries().iter().find(|row| row.id == id).map(|row| text(row.archived_at)).ok_or_else(|| format!("no row {id}"))
    }

    fn finish(self, path: &Path) -> Result<(), String> {
        out::write(path, self.lines)
    }
}

pub fn write(out_dir: &Path, zone: Zone) -> Result<(), String> {
    let mut coverage = Coverage::default();
    let dir = out_dir.join("transcripts").join(zone.label());
    type Script = fn(&mut Runner) -> Result<(), String>;
    let scripted: [(&str, &str, DateTime<Local>, Script); 4] = [
        ("every-command", "small", utc(2026, 10, 20, 8, 15, 0), every_command),
        ("refusals", "small", utc(2026, 10, 20, 8, 15, 0), refusals),
        ("dst-edges", "dst-edges", utc(2026, 10, 25, 0, 45, 0), dst),
        ("id-limits", "id-limits", utc(2026, 10, 20, 8, 15, 0), id_limits),
    ];
    for (name, board, now, script) in scripted {
        let mut runner = Runner::start(out_dir, zone, name, board, None, now, &mut coverage)?;
        script(&mut runner).map_err(|e| format!("transcript {name}: {e}"))?;
        runner.finish(&dir.join(format!("{name}.jsonl")))?;
    }
    for (seed, board, steps) in [(1u64, "empty", 30), (2, "empty", 30), (3, "empty", 30), (4, "empty", 30), (5, "small", 20), (6, "small", 20)] {
        let name = format!("seeded-{seed:02}");
        let mut runner = Runner::start(out_dir, zone, &name, board, Some(seed), utc(2026, 10, 20, 6, 0, 0), &mut coverage)?;
        seeded(&mut runner, seed, steps)?;
        runner.finish(&dir.join(format!("{name}.jsonl")))?;
    }
    if zone == Zone::Berlin {
        let missing_ops: Vec<&str> = OPS[..25].iter().copied().filter(|op| !coverage.accepted.contains_key(*op)).collect();
        let missing_refusals: Vec<&str> = REFUSALS.iter().copied().filter(|m| !coverage.messages.contains_key(*m)).collect();
        let unlisted: Vec<&String> = coverage.messages.keys().filter(|m| !REFUSALS.contains(&m.as_str())).collect();
        if !missing_ops.is_empty() || !missing_refusals.is_empty() || !unlisted.is_empty() {
            return Err(format!(
                "transcript coverage: ops never carried out {missing_ops:?}; refusals never seen {missing_refusals:?}; refusals not in the list {unlisted:?}"
            ));
        }
    }
    let report = json!({ "accepted": coverage.accepted, "refused": coverage.refused, "refusals": coverage.messages });
    out::write(&dir.join("COVERAGE.json"), out::pretty(&report))
}

/// Every command, carried out at least once, from `small`.
fn every_command(r: &mut Runner) -> Result<(), String> {
    r.send(r#"{"op":"snapshot","from":"2026-10-20","days":7}"#)?;
    r.send(r#"{"op":"feed"}"#)?;
    r.send(r#"{"op":"board"}"#)?;
    let a = r.id(json!({"op":"add","name":"Transcript task: label the invented jars","deadline":"2026-10-28T09:00","importance":2}))?;
    let e = r.id(json!({"op":"add","name":"Transcript event: made-up concert","deadline":"2026-10-29T19:00:00+01:00","is_event":true}))?;
    let u = r.id(json!({"op":"add","name":"Transcript task: undated, unranked"}))?;
    let w = r.id(json!({"op":"add","name":"Transcript task: whenever","time_importance":3,"importance":null}))?;
    let q = r.id(json!({"op":"quick_add","name":"Transcript quick add"}))?;
    let qd = r.id(json!({"op":"quick_add","name":"Transcript quick add, dated","deadline":"2026-10-30T12:00"}))?;
    r.tick(3);
    r.wire(json!({"op":"rename","id":a,"name":"  Transcript task, renamed: label the jars  "}))?;
    let ct = r.id(json!({"op":"create","kind":"task","name":"Transcript plan","day":"2026-10-21","start":600,"minutes":60}))?;
    let ce = r.id(json!({"op":"create","kind":"event","name":"Transcript timeline event","day":"2026-10-21","start":840,"minutes":30}))?;
    let cr = r.id(json!({"op":"create","kind":"routine","name":"","day":"2026-10-20","start":1260,"minutes":30}))?;
    r.wire(json!({"op":"add_block","id":ct,"day":"2026-10-22","start":660,"minutes":45}))?;
    r.wire(json!({"op":"add_block","id":ct,"day":"2026-10-23","start":541}))?;
    r.wire(json!({"op":"move_block","id":ct,"session":0,"day":"2026-10-21","start":630,"minutes":75}))?;
    r.wire(json!({"op":"move_block","id":ce,"day":"2026-10-21","start":870,"minutes":45}))?;
    r.wire(json!({"op":"move_block","id":cr,"day":"2026-10-20","start":1275,"minutes":30}))?;
    r.wire(json!({"op":"remove_block","id":ct,"session":1}))?;
    r.wire(json!({"op":"unplan","id":qd}))?;
    r.tick(5);
    r.wire(json!({"op":"set_estimate","id":ct,"minutes":120}))?;
    r.wire(json!({"op":"set_estimate","id":ce,"minutes":60}))?;
    r.wire(json!({"op":"set_estimate","id":cr,"minutes":45}))?;
    r.wire(json!({"op":"set_deadline","id":u,"deadline":"2026-11-02T10:00"}))?;
    r.wire(json!({"op":"set_deadline","id":a,"deadline":null}))?;
    r.wire(json!({"op":"set_severity","id":u,"level":3}))?;
    r.wire(json!({"op":"set_horizon","id":w,"level":0}))?;
    r.wire(json!({"op":"set_repeat","id":cr,"days":0b100_0001,"day":"2026-10-20"}))?;
    r.wire(json!({"op":"set_repeat","id":cr,"days":0,"day":"2026-10-24"}))?;
    r.wire(json!({"op":"reflow","day":"2026-10-20","from":600}))?;
    r.wire(json!({"op":"reflow","day":"2026-10-20"}))?;
    r.wire(json!({"op":"reflow","day":"2026-10-27"}))?;
    r.wire(json!({"op":"set_notes","text":"Transcript notes\twith a tab\nand a second line"}))?;
    r.tick(10);
    r.wire(json!({"op":"complete","id":a}))?;
    r.wire(json!({"op":"complete","id":qd,"at":"2026-10-25T09:00"}))?;
    r.wire(json!({"op":"complete","id":q,"at":"2026-10-19T18:00"}))?;
    r.wire(json!({"op":"delete","id":u,"at":"2026-10-20T10:00:00+02:00"}))?;
    r.wire(json!({"op":"delete","id":e}))?;
    r.wire(json!({"op":"delete","id":cr}))?;
    r.wire(json!({"op":"forget","id":w}))?;
    let key = r.key(a)?;
    r.wire(json!({"op":"restore","id":a,"archived_at":key}))?;
    let key = r.key(q)?;
    r.wire(json!({"op":"forget_archived","id":q,"archived_at":key}))?;
    r.tick(10);
    let s1 = r.id(json!({"op":"add_subscription","name":"Transcript calendar","url":"https://example.invalid/transcript/one.ics"}))?;
    let s2 = r.id(json!({"op":"add_subscription","name":"","url":"https://calendar.example.com/transcript/two.ics"}))?;
    r.id(json!({"op":"add_subscription","name":"Transcript webcal","url":"webcal://example.com/transcript/three.ics","color":[1,2,3,4]}))?;
    r.wire(json!({"op":"rename_subscription","id":s2,"name":"Transcript calendar two"}))?;
    r.wire(json!({"op":"rename_subscription","id":s2,"name":"   "}))?;
    r.wire(json!({"op":"set_subscription_color","id":s1,"color":[255,0,128,200]}))?;
    let team = r.board.subscriptions().first().map(|s| s.id).ok_or("small has calendars")?;
    r.wire(json!({"op":"set_subscription_enabled","id":team,"enabled":false}))?;
    r.wire(json!({"op":"set_subscription_enabled","id":team,"enabled":true}))?;
    r.wire(json!({"op":"remove_subscription","id":s1}))?;
    Ok(())
}

/// Every refusal, from `small`.
fn refusals(r: &mut Runner) -> Result<(), String> {
    let task = r.find("Fixture task, severity 2")?;
    let event = r.find("Fictional event: tea")?;
    let routine = r.find("Example routine: sleep")?;
    let sub = r.board.subscriptions().first().map(|s| s.id).ok_or("small has calendars")?;
    let sub_url = r.board.subscriptions().first().map(|s| s.url.clone()).unwrap_or_default();
    // Bodies that are not a command at all.
    for body in [
        "{not json",
        "",
        r#"{"op":"explode"}"#,
        r#"{"id":1}"#,
        r#"{"op":"rename","id":1}"#,
        r#"{"op":"rename","id":"one","name":"x"}"#,
        r#"{"op":"forget","id":-1}"#,
        r#"{"op":"forget","id":18446744073709551616}"#,
        r#"{"op":"add","name":"x","deadline":"tomorrow-ish"}"#,
        r#"{"op":"add","name":"x","deadline":"1969-12-31T12:00"}"#,
        r#"{"op":"create","kind":"chore","name":"x","day":"2026-10-20","start":0,"minutes":30}"#,
        r#"{"op":"create","kind":"task","name":"x","day":"2026-02-30","start":0,"minutes":30}"#,
        r#"{"op":"set_severity","id":1,"level":256}"#,
        r#"{"op":"snapshot","from":"2026-10-20"}"#,
    ] {
        r.send(body)?;
    }
    let long_name = "n".repeat(NAME_MAX_CHARS + 1);
    for wire in [
        json!({"op":"add","name":""}),
        json!({"op":"add","name":" \t "}),
        json!({"op":"add","name":long_name}),
        json!({"op":"add","name":"Refused event","is_event":true}),
        json!({"op":"add","name":"Refused: before 1970","deadline":"1969-12-31T22:00:00Z"}),
        json!({"op":"add","name":"Refused: after 9999","deadline":"9999-12-31T23:30:00-05:00"}),
        json!({"op":"add","name":"Refused severity","deadline":"2026-10-22T10:00","importance":5}),
        json!({"op":"add","name":"Refused horizon","time_importance":4}),
        json!({"op":"quick_add","name":""}),
        json!({"op":"quick_add","name":long_name}),
        json!({"op":"rename","id":task,"name":""}),
        json!({"op":"rename","id":424_242,"name":"Nobody"}),
        json!({"op":"complete","id":event}),
        json!({"op":"complete","id":routine}),
        json!({"op":"complete","id":424_242}),
        json!({"op":"complete","id":task,"at":"1969-06-01T12:00:00Z"}),
        json!({"op":"delete","id":424_242}),
        json!({"op":"forget","id":424_242}),
        json!({"op":"move_block","id":task,"session":9,"day":"2026-10-20","start":600,"minutes":30}),
        json!({"op":"move_block","id":task,"day":"2026-10-20","start":600,"minutes":30}),
        json!({"op":"move_block","id":task,"session":0,"day":"1969-12-31","start":600,"minutes":30}),
        json!({"op":"move_block","id":424_242,"day":"2026-10-20","start":600,"minutes":30}),
        json!({"op":"add_block","id":event,"day":"2026-10-20","start":600}),
        json!({"op":"add_block","id":routine,"day":"2026-10-20","start":600}),
        json!({"op":"add_block","id":task,"day":"10000-01-01","start":600}),
        json!({"op":"remove_block","id":task,"session":99}),
        json!({"op":"remove_block","id":event,"session":0}),
        json!({"op":"unplan","id":event}),
        json!({"op":"unplan","id":424_242}),
        json!({"op":"set_estimate","id":task,"minutes":14}),
        json!({"op":"set_estimate","id":task,"minutes":1441}),
        json!({"op":"set_estimate","id":424_242,"minutes":60}),
        json!({"op":"set_deadline","id":event,"deadline":"2026-10-22T10:00"}),
        json!({"op":"set_deadline","id":routine,"deadline":null}),
        json!({"op":"set_deadline","id":task,"deadline":"1969-12-31T00:00:00Z"}),
        json!({"op":"set_severity","id":task,"level":5}),
        json!({"op":"set_severity","id":event,"level":1}),
        json!({"op":"set_horizon","id":task,"level":4}),
        json!({"op":"set_horizon","id":routine,"level":1}),
        json!({"op":"set_repeat","id":task,"days":127,"day":"2026-10-20"}),
        json!({"op":"set_repeat","id":routine,"days":127,"day":"1969-12-31"}),
        json!({"op":"create","kind":"event","name":long_name,"day":"2026-10-20","start":600,"minutes":30}),
        json!({"op":"create","kind":"task","name":"x","day":"1969-12-31","start":600,"minutes":30}),
        json!({"op":"reflow","day":"1969-12-31","from":0}),
        json!({"op":"set_notes","text":"x".repeat(NOTES_MAX_CHARS + 1)}),
        json!({"op":"restore","id":424_242,"archived_at":"2026-10-01T10:00:00+02:00"}),
        json!({"op":"forget_archived","id":424_242,"archived_at":"2026-10-01T10:00:00+02:00"}),
        json!({"op":"add_subscription","name":"x","url":""}),
        json!({"op":"add_subscription","name":"x","url":format!("https://example.invalid/{}", "l".repeat(2000))}),
        json!({"op":"add_subscription","name":"x","url":"http://example.invalid/plain.ics"}),
        json!({"op":"add_subscription","name":"x","url":"ftp://example.invalid/file.ics"}),
        json!({"op":"add_subscription","name":"x","url":"https://nohost/feed.ics"}),
        json!({"op":"add_subscription","name":"x","url":sub_url}),
        json!({"op":"add_subscription","name":long_name,"url":"https://example.invalid/refused/long-name.ics"}),
        json!({"op":"remove_subscription","id":424_242}),
        json!({"op":"rename_subscription","id":424_242,"name":"x"}),
        json!({"op":"rename_subscription","id":sub,"name":long_name}),
        json!({"op":"set_subscription_color","id":424_242,"color":[0,0,0,0]}),
        json!({"op":"set_subscription_enabled","id":424_242,"enabled":true}),
    ] {
        r.wire(wire)?;
        r.tick(1);
    }
    // Accepted, though the stored text ends up past the cap: the length is
    // checked before tabs become four spaces (V2 audit item 18).
    let mut tabbed = "t".repeat(NOTES_MAX_CHARS - 10);
    tabbed.push_str(&"\t".repeat(10));
    r.wire(json!({"op":"set_notes","text":tabbed}))?;
    Ok(())
}

/// Naive and exact times around the transitions, from `dst-edges`, starting
/// inside Berlin's first 02:45 of the fold.
fn dst(r: &mut Runner) -> Result<(), String> {
    let gap_task = r.find("DST fixture: block in the spring-forward gap")?;
    let fold_event = r.find("DST fixture: event at 02:30 on the fall-back day")?;
    r.wire(json!({"op":"add","name":"Transcript DST: naive time in Berlin's gap","deadline":"2026-03-29T02:30","importance":1}))?;
    r.wire(json!({"op":"add","name":"Transcript DST: naive time in Berlin's fold","deadline":"2026-10-25T02:30","importance":1}))?;
    r.wire(json!({"op":"add","name":"Transcript DST: naive time in New York's gap","deadline":"2026-03-08T02:30","importance":1}))?;
    r.wire(json!({"op":"add","name":"Transcript DST: naive time in New York's fold","deadline":"2026-11-01T01:30","importance":1}))?;
    r.wire(json!({"op":"create","kind":"task","name":"Transcript DST: created in the fold","day":"2026-10-25","start":150,"minutes":60}))?;
    r.wire(json!({"op":"create","kind":"event","name":"Transcript DST: created in New York's gap","day":"2026-03-08","start":150,"minutes":30}))?;
    r.wire(json!({"op":"move_block","id":gap_task,"session":0,"day":"2026-03-29","start":130,"minutes":30}))?;
    r.wire(json!({"op":"set_estimate","id":fold_event,"minutes":90}))?;
    r.wire(json!({"op":"reflow","day":"2026-10-25"}))?;
    r.tick(60);
    // Now in the second 02:45.
    r.wire(json!({"op":"reflow","day":"2026-10-25"}))?;
    r.wire(json!({"op":"create","kind":"task","name":"Transcript DST: created in the second pass","day":"2026-10-25","start":165,"minutes":30}))?;
    let done = r.id(json!({"op":"add","name":"Transcript DST: finished in the fold","time_importance":0}))?;
    r.wire(json!({"op":"complete","id":done}))?;
    // Instants whose local date differs between the zones.
    r.wire(json!({"op":"add","name":"Transcript edge: 1970 in Berlin, 1969 in New York","deadline":"1970-01-01T00:30:00+01:00","importance":0}))?;
    r.wire(json!({"op":"add","name":"Transcript edge: 9999 in New York, 10000 in Berlin","deadline":"9999-12-31T23:30:00-05:00","importance":0}))?;
    r.wire(json!({"op":"create","kind":"event","name":"Transcript edge: the last quarter-hour of 9999","day":"9999-12-31","start":1439,"minutes":60}))?;
    r.wire(json!({"op":"set_deadline","id":done,"deadline":"2026-11-01T01:30"}))?;
    Ok(())
}

/// At the limits, from `id-limits`: the calendar cap, and the id counter
/// saturated at `u64::MAX`.
fn id_limits(r: &mut Runner) -> Result<(), String> {
    r.wire(json!({"op":"add_subscription","name":"One too many","url":"https://example.invalid/limits/21.ics"}))?;
    r.wire(json!({"op":"remove_subscription","id":1}))?;
    r.wire(json!({"op":"quick_add","name":"Transcript limits: the id after u64::MAX"}))?;
    r.wire(json!({"op":"rename","id":u64::MAX,"name":"Transcript limits: which u64::MAX is renamed"}))?;
    r.wire(json!({"op":"complete","id":(1u64 << 62) + 1}))?;
    r.wire(json!({"op":"forget","id":(1u64 << 53) + 1}))?;
    Ok(())
}

// The seeded wire texts are drawn from the UTC instant, never from the local
// rendering, so both zones' runs send the same commands (only archive keys,
// which the board echoes back in local time, differ).
fn some_day(rng: &mut SplitMix64, now: DateTime<Utc>) -> String {
    (now.date_naive() + Duration::days(rng.between(-2, 7))).to_string()
}

fn some_start(rng: &mut SplitMix64) -> i64 {
    if rng.chance(1, 10) { *rng.pick(&[-30, 1439, 2000]) } else { rng.between(0, 95) * 15 }
}

fn some_minutes(rng: &mut SplitMix64) -> u32 {
    *rng.pick(&[0, 15, 30, 45, 60, 90, 120, 240, 2000])
}

/// A naive wire time (read in the board's zone) or an exact UTC instant.
fn some_instant(rng: &mut SplitMix64, now: DateTime<Utc>) -> String {
    let at = now + Duration::minutes(rng.between(-3 * 1440, 10 * 1440) / 15 * 15);
    if rng.chance(1, 2) { at.format("%Y-%m-%dT%H:%M").to_string() } else { at.to_rfc3339_opts(SecondsFormat::Secs, true) }
}

fn some_name(rng: &mut SplitMix64) -> String {
    if rng.chance(1, 15) { String::new() } else { format!("Seeded {} {}", rng.pick(&WORDS), rng.below(1000)) }
}

/// Seeded commands: mostly well-aimed, sometimes at an id that is not there.
fn seeded(r: &mut Runner, seed: u64, steps: usize) -> Result<(), String> {
    let mut rng = SplitMix64::new(seed);
    for _ in 0..steps {
        r.tick(rng.between(1, 180));
        let ids: Vec<u64> = r.board.items.iter().map(|item| item.id).collect();
        let id = if ids.is_empty() || rng.chance(1, 8) { 900_000 + rng.below(100) } else { *rng.pick(&ids) };
        let subs: Vec<u64> = r.board.subscriptions().iter().map(|s| s.id).collect();
        let sub = if subs.is_empty() || rng.chance(1, 6) { 800_000 } else { *rng.pick(&subs) };
        r.board.load_archive()?;
        let keys: Vec<(u64, String)> = r.board.archive.entries().iter().map(|row| (row.id, text(row.archived_at))).collect();
        let (key_id, key_at) = if keys.is_empty() || rng.chance(1, 6) { (700_000, text(r.now)) } else { rng.pick(&keys).clone() };
        let now = r.now.with_timezone(&Utc);
        let wire = match rng.below(40) {
            0..=4 => {
                let mut w = json!({"op":"add","name":some_name(&mut rng)});
                if rng.chance(1, 2) {
                    w["deadline"] = json!(some_instant(&mut rng, now));
                    w["importance"] = json!(rng.below(6));
                } else {
                    w["time_importance"] = json!(rng.below(5));
                }
                if rng.chance(1, 6) {
                    w["is_event"] = json!(true);
                }
                w
            }
            5..=7 => json!({"op":"quick_add","name":some_name(&mut rng)}),
            8..=11 => json!({"op":"create","kind":*rng.pick(&["task","event","routine"]),"name":some_name(&mut rng),
                "day":some_day(&mut rng, now),"start":some_start(&mut rng),"minutes":some_minutes(&mut rng)}),
            12 => json!({"op":"rename","id":id,"name":some_name(&mut rng)}),
            13 => json!({"op":"complete","id":id}),
            14 => json!({"op":"delete","id":id,"at":some_instant(&mut rng, now)}),
            15 => json!({"op":"forget","id":id}),
            16 => json!({"op":"move_block","id":id,"session":rng.below(3),"day":some_day(&mut rng, now),"start":some_start(&mut rng),"minutes":some_minutes(&mut rng)}),
            17..=18 => json!({"op":"add_block","id":id,"day":some_day(&mut rng, now),"start":some_start(&mut rng)}),
            19 => json!({"op":"remove_block","id":id,"session":rng.below(3)}),
            20 => json!({"op":"unplan","id":id}),
            21 => json!({"op":"set_estimate","id":id,"minutes":some_minutes(&mut rng)}),
            22 => json!({"op":"set_deadline","id":id,"deadline":if rng.chance(1, 3) { Value::Null } else { json!(some_instant(&mut rng, now)) }}),
            23 => json!({"op":"set_severity","id":id,"level":rng.below(6)}),
            24 => json!({"op":"set_horizon","id":id,"level":rng.below(5)}),
            25 => json!({"op":"set_repeat","id":id,"days":rng.below(256),"day":some_day(&mut rng, now)}),
            26 => json!({"op":"reflow","day":some_day(&mut rng, now)}),
            27 => json!({"op":"set_notes","text":format!("Seeded notes {}\n\t{}", rng.below(1000), rng.pick(&WORDS))}),
            28 => json!({"op":"restore","id":key_id,"archived_at":key_at}),
            29 => json!({"op":"forget_archived","id":key_id,"archived_at":key_at}),
            30..=31 => json!({"op":"add_subscription","name":some_name(&mut rng),"url":format!("https://example.invalid/seeded/{}.ics", rng.below(40))}),
            32 => json!({"op":"remove_subscription","id":sub}),
            33 => json!({"op":"rename_subscription","id":sub,"name":some_name(&mut rng)}),
            34 => json!({"op":"set_subscription_color","id":sub,"color":[rng.below(256),rng.below(256),rng.below(256),255]}),
            35 => json!({"op":"set_subscription_enabled","id":sub,"enabled":rng.chance(1, 2)}),
            36 => json!({"op":*rng.pick(&["snapshot","feed","board"]),"from":some_day(&mut rng, now)}),
            37 => json!({"op":"reflow","day":some_day(&mut rng, now),"from":some_start(&mut rng)}),
            38 => {
                r.send(rng.pick(&["{", "{\"op\":\"complete\"}", "{\"op\":\"add\",\"name\":7}"]))?;
                continue;
            }
            _ => json!({"op":"complete","id":id,"at":some_instant(&mut rng, now)}),
        };
        r.wire(wire)?;
    }
    Ok(())
}
