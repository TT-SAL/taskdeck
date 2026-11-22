//! Driving today's board: a builder that sends wire commands at a pinned
//! clock, opening a copy of a board so reading never touches the fixtures, and
//! the settings files (palette, home place) every board carries.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;

use chrono::{DateTime, Duration, Local};
use serde_json::Value;
use task_deck::board::{Board, Command, Reply};
use task_deck::color::{self, ColorScheme};
use task_deck::{initialization, utilities};
use tempfile::TempDir;

use crate::out;

/// The made-up home place: open ocean, nowhere anybody lives.
pub const HOME_PLACE: [f32; 2] = [-41.25, -132.5];

/// The palette the boards select: a scheme of the fixtures' own.
pub const FIXTURE_SCHEME_ID: u32 = 100;
pub const FIXTURE_PALETTE: [[u8; 4]; 6] = [
    [10, 20, 30, 40],
    [200, 60, 60, 120],
    [60, 200, 60, 104],
    [60, 60, 200, 88],
    [180, 180, 40, 74],
    [120, 40, 160, 136],
];

/// Builds a board by sending it wire commands, the JSON the phone and the
/// desktop send, at a clock that moves only when told to.
pub struct Builder {
    pub board: Board,
    pub now: DateTime<Local>,
}

impl Builder {
    pub fn new(dir: &Path, now: DateTime<Local>) -> Result<Self, String> {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let (board, problems) = Board::open(dir.to_path_buf());
        if !problems.is_empty() {
            return Err(format!("{}: {problems:?}", dir.display()));
        }
        Ok(Self { board, now })
    }

    /// Carry out one wire command; any refusal is a bug in the fixture script.
    pub fn send(&mut self, wire: Value) -> Result<Reply, String> {
        let text = wire.to_string();
        let command: Command = serde_json::from_str(&text).map_err(|e| format!("{text}: {e}"))?;
        self.board.apply(command, self.now).map_err(|e| format!("{text}: {} {}", e.status, e.message))
    }

    /// The same, for a command that makes something: its id.
    pub fn id(&mut self, wire: Value) -> Result<u64, String> {
        let text = wire.to_string();
        self.send(wire)?.id.ok_or_else(|| format!("{text}: no id in the reply"))
    }

    pub fn tick(&mut self, minutes: i64) {
        self.now += Duration::minutes(minutes);
    }
}

/// A board read from a copy of `dir`, so whatever opening writes (today's
/// code seeds a missing file) lands in the copy. The problems are what
/// `Board::open` reported.
pub fn open_copy(dir: &Path) -> Result<(Board, Vec<String>, TempDir), String> {
    let copy = tempfile::tempdir().map_err(|e| e.to_string())?;
    for entry in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        fs::copy(entry.path(), copy.path().join(entry.file_name())).map_err(|e| e.to_string())?;
    }
    let (board, problems) = Board::open(copy.path().to_path_buf());
    Ok((board, problems, copy))
}

/// The palette a host of this board paints the phone in: the scheme
/// `userconfig.toml` selects from `colorschemes.json`, all-transparent when
/// there is none — the same lookup `taskdeck-server` makes at start.
pub fn palette_of(dir: &Path) -> [[u8; 4]; 6] {
    let config = initialization::read_config_only(&dir.join("userconfig.toml"));
    color::read_colorschemes(dir)
        .ok()
        .and_then(|schemes| schemes.get(&config.selected_colorscheme_id).map(|scheme| scheme.colors))
        .unwrap_or([[0; 4]; 6])
}

/// `colorschemes.json` and `userconfig.toml` as today's app leaves them after
/// a first start and two settings changes (home place, palette).
///
/// Normalised: `colorschemes.json` is a `HashMap` that today's code writes in
/// the map's own (random) order; it is written here in key order, in the same
/// pretty-printed shape `color::save_colorschemes` uses.
pub fn write_settings(dir: &Path) -> Result<(), String> {
    let mut schemes: HashMap<u32, ColorScheme> = HashMap::new();
    let mut selected = 0;
    color::install_builtins(&mut schemes, &mut selected);
    schemes.insert(
        FIXTURE_SCHEME_ID,
        ColorScheme { name: "Fixture palette (made up)".into(), colors: FIXTURE_PALETTE, is_user_configurable: true },
    );
    let sorted: BTreeMap<u32, ColorScheme> = schemes.into_iter().collect();
    out::write(&dir.join("colorschemes.json"), serde_json::to_string_pretty(&sorted).map_err(|e| e.to_string())?)?;

    let config = dir.join("userconfig.toml");
    initialization::get_check_and_set_config(&config);
    initialization::write_config_value(&config, "coordinates", utilities::float_pair_array(HOME_PLACE))
        .map_err(|e| e.to_string())?;
    initialization::write_config_value(&config, "selected_colorscheme_id", FIXTURE_SCHEME_ID as i64)
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// The whole state of a board, in today's serialisation (`Command::Board`'s
/// answer), compact and with today's field order.
pub fn state(board: &mut Board) -> Result<String, String> {
    serde_json::to_string(&board.state()?).map_err(|e| e.to_string())
}
