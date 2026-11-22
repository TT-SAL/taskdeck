//! `taskdeck-fixtures --zone berlin|new-york --out <dir>`: write one zone's
//! fixture set. `scripts/regen-fixtures.sh` runs both, Berlin first, each under
//! the `TZ` it requires.

use std::path::PathBuf;
use std::process::ExitCode;

use taskdeck_fixtures::clock::Zone;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mut zone, mut out) = (None, None);
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match (arg.as_str(), rest.next()) {
            ("--zone", Some(value)) => zone = Zone::parse(value),
            ("--out", Some(value)) => out = Some(PathBuf::from(value)),
            _ => zone = None,
        }
    }
    let (Some(zone), Some(out)) = (zone, out) else {
        eprintln!("usage: taskdeck-fixtures --zone berlin|new-york --out <dir>");
        return ExitCode::from(2);
    };
    match taskdeck_fixtures::generate(zone, &out) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("taskdeck-fixtures: {error}");
            ExitCode::FAILURE
        }
    }
}
