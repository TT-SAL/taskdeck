//! Properties of the committed data that a byte comparison with a fresh run
//! cannot show: what later stages rely on the fixtures to exercise.

use std::fs;
use std::path::PathBuf;

use serde_json::Value;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn lines(path: PathBuf) -> Vec<Value> {
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    text.lines().map(|line| serde_json::from_str(line).unwrap()).collect()
}

/// The seeded transcripts send the same commands under both zones; only the
/// archive keys a command quotes back (`archived_at`, in the board's local
/// offset) may differ.
#[test]
fn seeded_transcripts_send_the_same_commands_in_both_zones() {
    for n in 1..=6 {
        let name = format!("seeded-{n:02}.jsonl");
        let berlin = lines(fixtures().join("transcripts/europe-berlin").join(&name));
        let new_york = lines(fixtures().join("transcripts/america-new-york").join(&name));
        assert_eq!(berlin.len(), new_york.len(), "{name}");
        for (b, y) in berlin.iter().zip(&new_york).skip(1) {
            let wire = |step: &Value| -> Value {
                let mut wire = serde_json::from_str(step["wire"].as_str().unwrap()).unwrap_or(step["wire"].clone());
                if let Some(object) = wire.as_object_mut() {
                    object.remove("archived_at");
                }
                wire
            };
            assert_eq!(wire(b), wire(y), "{name} step {}", b["step"]);
        }
    }
}

/// The budget board's 7- and 31-day snapshots place archived rows (ghosts).
#[test]
fn the_budget_snapshots_place_ghosts() {
    for request in ["2026-10-19_7d", "2026-10-05_31d"] {
        let path = fixtures().join(format!("snapshots/europe-berlin/archive-10k/{request}_at-20261020T0815Z.json"));
        let snapshot: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let ghosts: usize = snapshot["days"].as_array().unwrap().iter().map(|day| day["ghosts"].as_array().unwrap().len()).sum();
        assert!(ghosts > 0, "{request}: no ghosts");
    }
}

/// The order fixtures/README.md states for a transcript's state ("Order in
/// `state`") holds at every step, so a replayer can compare arrays in order:
/// `items` and `subscriptions` keep their order, and a new one goes last;
/// `overlay.events` only loses entries; an archive row is filed before the
/// first row with the same or an earlier instant, and a restore or forget
/// takes the first row in this order with its key, (id, instant).
#[test]
fn transcript_states_keep_the_stated_order() {
    let instant = |row: &Value| chrono::DateTime::parse_from_rfc3339(row["inactivated"].as_str().unwrap()).unwrap();
    let ids = |list: &Value| -> Vec<u64> { list.as_array().unwrap().iter().map(|x| x["id"].as_u64().unwrap()).collect() };
    for zone in ["europe-berlin", "america-new-york"] {
        for entry in fs::read_dir(fixtures().join("transcripts").join(zone)).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|extension| extension != "jsonl") {
                continue;
            }
            for pair in lines(path.clone()).windows(2) {
                let (before, after) = (&pair[0]["state"], &pair[1]["state"]);
                let at = format!("{} step {}", path.display(), pair[1]["step"]);
                for list in ["items", "subscriptions"] {
                    let (was, now) = (ids(&before[list]), ids(&after[list]));
                    let mut rest = was.iter();
                    let only_lost = now.iter().all(|id| rest.any(|old| old == id));
                    let one_added = now.len() == was.len() + 1 && now[..was.len()] == was[..];
                    assert!(only_lost || one_added, "{at}: {list} reordered: {was:?} -> {now:?}");
                }
                let (was, now) = (before["overlay"]["events"].as_array().unwrap(), after["overlay"]["events"].as_array().unwrap());
                let mut rest = was.iter();
                assert!(now.iter().all(|event| rest.any(|old| old == event)), "{at}: overlay.events reordered");

                let (was, now) = (before["archive"].as_array().unwrap(), after["archive"].as_array().unwrap());
                assert!(now.windows(2).all(|w| instant(&w[0]) >= instant(&w[1])), "{at}: archive not newest first");
                let differ = (0..was.len().max(now.len())).find(|&i| was.get(i) != now.get(i));
                let Some(i) = differ else { continue };
                if now.len() == was.len() + 1 {
                    let filed = &now[i];
                    assert_eq!(&now[..i], &was[..i], "{at}");
                    assert_eq!(&now[i + 1..], &was[i..], "{at}: more than one row changed");
                    assert_eq!(i, was.iter().position(|row| instant(row) <= instant(filed)).unwrap_or(was.len()), "{at}: row filed out of place");
                } else {
                    assert_eq!(now.len() + 1, was.len(), "{at}: more than one row changed");
                    let taken = &was[i];
                    assert_eq!(&now[i..], &was[i + 1..], "{at}: more than one row changed");
                    let first = was.iter().position(|row| row["id"] == taken["id"] && instant(row) == instant(taken));
                    assert_eq!(first, Some(i), "{at}: not the first row with its key was taken");
                }
            }
        }
    }
}
