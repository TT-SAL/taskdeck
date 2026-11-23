// The fixture folders as `LegacyFolder` values (core.md §15.2, §18.2), for
// every test target that imports one. The bytes come in by `include_bytes!`;
// each folder's `LegacyConfig` is what its `userconfig.toml` holds, written
// out as values (today's parser, initialization.rs, reads it that way).

use taskdeck_core::legacy::{LegacyConfig, LegacyFolder};

/// The groups of folders under `fixtures/` and the folders in each.
pub const GROUPS: [(&str, &[&str]); 3] = [
    ("boards", &["archive-10k", "dst-edges", "empty", "id-limits", "small", "unreadable"]),
    ("legacy-main", &["archive-10k", "dst-edges", "empty", "id-limits", "small", "unreadable"]),
    ("legacy-other", &["empty", "small"]),
];

type Files = [Option<&'static [u8]>; 5];

fn folder(files: Files, selected_colorscheme_id: u32) -> LegacyFolder {
    let [read_at_startup, archived, notepad, subscriptions, colorschemes] = files.map(|f| f.map(<[u8]>::to_vec));
    LegacyFolder {
        read_at_startup,
        archived,
        notepad,
        subscriptions,
        colorschemes,
        config: LegacyConfig { coordinates: Some([-41.25, -132.5]), selected_colorscheme_id: Some(selected_colorscheme_id) },
    }
}

/// A folder by its path under `fixtures/`, such as `boards/small`.
pub fn by_path(path: &str) -> LegacyFolder {
    match path.split_once('/') {
        Some(("boards", name)) => board(name),
        Some(("legacy-main", name)) => legacy_main(name),
        Some(("legacy-other", name)) => legacy_other(name),
        _ => panic!("no fixture folder {path}"),
    }
}

/// `fixtures/boards/<name>`; its config, as every `fixtures/boards/*/userconfig.toml` holds: `coordinates = [-41.25, -132.5]`, `selected_colorscheme_id = 100`.
pub fn board(name: &str) -> LegacyFolder {
    let files: Files = match name {
        "archive-10k" => [
            Some(include_bytes!("../../../../fixtures/boards/archive-10k/read_at_startup.json")),
            Some(include_bytes!("../../../../fixtures/boards/archive-10k/archived.jsonl")),
            Some(include_bytes!("../../../../fixtures/boards/archive-10k/notepad_text.json")),
            None,
            Some(include_bytes!("../../../../fixtures/boards/archive-10k/colorschemes.json")),
        ],
        "dst-edges" => [
            Some(include_bytes!("../../../../fixtures/boards/dst-edges/read_at_startup.json")),
            Some(include_bytes!("../../../../fixtures/boards/dst-edges/archived.jsonl")),
            Some(include_bytes!("../../../../fixtures/boards/dst-edges/notepad_text.json")),
            None,
            Some(include_bytes!("../../../../fixtures/boards/dst-edges/colorschemes.json")),
        ],
        "empty" => [
            Some(include_bytes!("../../../../fixtures/boards/empty/read_at_startup.json")),
            None,
            Some(include_bytes!("../../../../fixtures/boards/empty/notepad_text.json")),
            None,
            Some(include_bytes!("../../../../fixtures/boards/empty/colorschemes.json")),
        ],
        "id-limits" => [
            Some(include_bytes!("../../../../fixtures/boards/id-limits/read_at_startup.json")),
            Some(include_bytes!("../../../../fixtures/boards/id-limits/archived.jsonl")),
            Some(include_bytes!("../../../../fixtures/boards/id-limits/notepad_text.json")),
            Some(include_bytes!("../../../../fixtures/boards/id-limits/subscriptions.json")),
            Some(include_bytes!("../../../../fixtures/boards/id-limits/colorschemes.json")),
        ],
        "small" => [
            Some(include_bytes!("../../../../fixtures/boards/small/read_at_startup.json")),
            Some(include_bytes!("../../../../fixtures/boards/small/archived.jsonl")),
            Some(include_bytes!("../../../../fixtures/boards/small/notepad_text.json")),
            Some(include_bytes!("../../../../fixtures/boards/small/subscriptions.json")),
            Some(include_bytes!("../../../../fixtures/boards/small/colorschemes.json")),
        ],
        "unreadable" => [
            Some(include_bytes!("../../../../fixtures/boards/unreadable/read_at_startup.json")),
            Some(include_bytes!("../../../../fixtures/boards/unreadable/archived.jsonl")),
            Some(include_bytes!("../../../../fixtures/boards/unreadable/notepad_text.json")),
            None,
            Some(include_bytes!("../../../../fixtures/boards/unreadable/colorschemes.json")),
        ],
        _ => panic!("no fixture folder boards/{name}"),
    };
    folder(files, 100)
}

/// `fixtures/legacy-main/<name>`; its config, as each folder's own `userconfig.toml` holds: `coordinates = [-41.25, -132.5]`, `selected_colorscheme_id = 1`.
pub fn legacy_main(name: &str) -> LegacyFolder {
    let files: Files = match name {
        "archive-10k" => [
            Some(include_bytes!("../../../../fixtures/legacy-main/archive-10k/read_at_startup.json")),
            Some(include_bytes!("../../../../fixtures/legacy-main/archive-10k/archived.jsonl")),
            Some(include_bytes!("../../../../fixtures/legacy-main/archive-10k/notepad_text.json")),
            None,
            Some(include_bytes!("../../../../fixtures/legacy-main/archive-10k/colorschemes.json")),
        ],
        "dst-edges" => [
            Some(include_bytes!("../../../../fixtures/legacy-main/dst-edges/read_at_startup.json")),
            Some(include_bytes!("../../../../fixtures/legacy-main/dst-edges/archived.jsonl")),
            Some(include_bytes!("../../../../fixtures/legacy-main/dst-edges/notepad_text.json")),
            None,
            Some(include_bytes!("../../../../fixtures/legacy-main/dst-edges/colorschemes.json")),
        ],
        "empty" => [
            Some(include_bytes!("../../../../fixtures/legacy-main/empty/read_at_startup.json")),
            None,
            Some(include_bytes!("../../../../fixtures/legacy-main/empty/notepad_text.json")),
            None,
            Some(include_bytes!("../../../../fixtures/legacy-main/empty/colorschemes.json")),
        ],
        "id-limits" => [
            Some(include_bytes!("../../../../fixtures/legacy-main/id-limits/read_at_startup.json")),
            Some(include_bytes!("../../../../fixtures/legacy-main/id-limits/archived.jsonl")),
            Some(include_bytes!("../../../../fixtures/legacy-main/id-limits/notepad_text.json")),
            None,
            Some(include_bytes!("../../../../fixtures/legacy-main/id-limits/colorschemes.json")),
        ],
        "small" => [
            Some(include_bytes!("../../../../fixtures/legacy-main/small/read_at_startup.json")),
            Some(include_bytes!("../../../../fixtures/legacy-main/small/archived.jsonl")),
            Some(include_bytes!("../../../../fixtures/legacy-main/small/notepad_text.json")),
            None,
            Some(include_bytes!("../../../../fixtures/legacy-main/small/colorschemes.json")),
        ],
        "unreadable" => [
            Some(include_bytes!("../../../../fixtures/legacy-main/unreadable/read_at_startup.json")),
            Some(include_bytes!("../../../../fixtures/legacy-main/unreadable/archived.jsonl")),
            Some(include_bytes!("../../../../fixtures/legacy-main/unreadable/notepad_text.json")),
            None,
            Some(include_bytes!("../../../../fixtures/legacy-main/unreadable/colorschemes.json")),
        ],
        _ => panic!("no fixture folder legacy-main/{name}"),
    };
    folder(files, 1)
}

/// `fixtures/legacy-other/<name>`; its config, as each folder's own `userconfig.toml` holds: `coordinates = [-41.25, -132.5]`, `selected_colorscheme_id = "1"` (today's parser reads the string as 1).
pub fn legacy_other(name: &str) -> LegacyFolder {
    let files: Files = match name {
        "empty" => [
            Some(include_bytes!("../../../../fixtures/legacy-other/empty/read_at_startup.json")),
            None,
            Some(include_bytes!("../../../../fixtures/legacy-other/empty/notepad_text.json")),
            None,
            Some(include_bytes!("../../../../fixtures/legacy-other/empty/colorschemes.json")),
        ],
        "small" => [
            Some(include_bytes!("../../../../fixtures/legacy-other/small/read_at_startup.json")),
            Some(include_bytes!("../../../../fixtures/legacy-other/small/archived.jsonl")),
            Some(include_bytes!("../../../../fixtures/legacy-other/small/notepad_text.json")),
            None,
            Some(include_bytes!("../../../../fixtures/legacy-other/small/colorschemes.json")),
        ],
        _ => panic!("no fixture folder legacy-other/{name}"),
    };
    folder(files, 1)
}
