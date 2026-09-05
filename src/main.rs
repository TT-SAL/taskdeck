#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use mimalloc::MiMalloc;
use std::sync::Arc;

use task_deck::{board::Board, color, initialization::{self, App, Config, get_check_and_set_config}, paths::{self, AppDirs}, phone, subscriptions, sync, tasks, ui::{TaskApp, TaskAppConfig}, weather::get_weather};
use winit::event_loop::{ControlFlow, EventLoop};

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        pollster::block_on(run());
    }
}

async fn run() {    
    let event_loop = EventLoop::new().unwrap();
    let proxy = event_loop.create_proxy();

    event_loop.set_control_flow(ControlFlow::Wait);

    // Every file the app touches hangs off these two folders, resolved once here
    // rather than from the working directory (which the launcher controls, and
    // which differs per platform — see `paths`).
    let dirs = AppDirs::resolve();

    // Held for the whole of `run`, which is the whole of the program: the
    // kernel drops it when this process does. See `paths::claim_data_dir` for
    // why a second instance is warned rather than turned away.
    let _data_claim = paths::claim_data_dir(&dirs.data);

    let Config { start_in_fullscreen, coordinates, background, enable_fps_counter, window_size_startup, calendar_weeks_to_show, selected_monitor_name, mut selected_colorscheme_id, three_day_weather, background_image_tint_percent, ui_scale_percent, phone_server_enabled, phone_server_port, mut phone_token, phone_bind_address, phone_public_url: _, frame_cap_fps, server_url, server_token } = get_check_and_set_config(&dirs.config_file());

    // Collected non-fatal startup recovery messages (e.g. quarantined corrupt
    // files), surfaced in the error window once the UI is up.
    let mut startup_errors: Vec<String> = Vec::new();

    if _data_claim.is_contended() {
        startup_errors.push(_data_claim.warning());
    }

    // The phone view's token is minted once and then kept: it is the secret in
    // the link on the phone, and a link that changed at every start would be
    // no link at all. Minted whether or not the view is on, so turning it on
    // later in Settings has a link to show straight away.
    if phone_token.is_empty() {
        phone_token = phone::generate_token();
        if let Err(e) = initialization::write_config_value(&dirs.config_file(), "phone_token", phone_token.clone()) {
            startup_errors.push(format!(
                "Could not save the phone view's key, so the phone link will change at the next start:\n{e}"
            ));
        }
    }

    // The phone view's request queue. Its thread sends; the UI thread serves
    // (`TaskApp::serve_phone_requests`), woken through the same proxy the
    // weather thread uses.
    let (phone_tx, phone_rx) = std::sync::mpsc::channel();

    // Wakes the event loop from another thread: the weather thread, the phone
    // view's server, and the sync engine all use it.
    let wake: phone::Wake = {
        let proxy = proxy.clone();
        Arc::new(move || {
            let _ = proxy.send_event(());
        })
    };

    // The board: the live set, the archive and the notepad. With a server
    // configured it is the server's board, fetched now and kept as a replica
    // (`sync.rs`); if the server cannot be reached the last replica saved
    // here is used and edits queue until it can. Without one, it is this
    // machine's own, as it always was. A corrupt active set is quarantined
    // rather than aborting the boot either way (`Board::open`).
    let mut sync_handle = None;
    let board = if server_url.is_empty() {
        // A folder that was a server's replica is its own board again: the
        // marker goes, and an outbox of edits meant for that server is set
        // aside — so a later return to the server starts with a first
        // contact's set-aside rather than writing over what was done here.
        match sync::forget_replica(&dirs.data) {
            Ok(Some(note)) => startup_errors.push(note),
            Ok(None) => {}
            Err(why) => startup_errors.push(format!("Could not close this folder's replica state:\n{why}")),
        }
        let (mut board, problems) = Board::open(dirs.data.clone());
        startup_errors.extend(problems);
        board.seed_version(clock_version());
        // Items created while this folder was a replica, never confirmed by
        // the server, become this board's own (`adopt_temporaries`).
        if let Err(error) = board.adopt_temporaries() {
            startup_errors.push(error.message);
        }
        board
    } else {
        match sync::Remote::new(&server_url, &server_token, sync::STARTUP_TIMEOUT) {
            Err(why) => {
                startup_errors.push(format!("The server setting is not usable, so this copy runs on its own:\n{why}"));
                let (mut board, problems) = Board::open(dirs.data.clone());
                startup_errors.extend(problems);
                // A board of its own, like every other branch here: its
                // version has to start past whatever a phone already
                // remembers, or the phone keeps its stale copy of the day.
                board.seed_version(clock_version());
                board
            }
            Ok(remote) => {
                // Is this folder already this server's replica, or a board
                // of its own that has never met the server?
                let replica_already = sync::is_replica_of(&dirs.data, remote.base());
                match remote.board() {
                    Ok(state) => {
                        let mut safe_to_replace = true;
                        if !replica_already {
                            // A board that lived here is set aside, dated — never
                            // written over by the server's. This is the moment a
                            // calendar would otherwise be lost.
                            match sync::set_aside_local_board(&dirs.data) {
                                Ok(Some(note)) => startup_errors.push(note),
                                Ok(None) => {}
                                Err(why) => {
                                    // Half a set-aside is the one state that loses
                                    // a board: nothing is marked and nothing is
                                    // written over; this copy runs on its own files.
                                    startup_errors.push(format!(
                                        "Could not set the local board aside, so this copy runs on its own board and the server was not used:\n{why}"
                                    ));
                                    safe_to_replace = false;
                                }
                            }
                            if safe_to_replace
                                && let Err(why) = sync::mark_replica_of(&dirs.data, remote.base())
                            {
                                startup_errors.push(format!("Could not mark this folder as the server's replica:\n{why}"));
                            }
                        }
                        if !safe_to_replace {
                            let (mut board, problems) = Board::open(dirs.data.clone());
                            startup_errors.extend(problems);
                            board.seed_version(clock_version());
                            board
                        } else {
                        let mut board = Board::from_parts(state.items, state.notes, dirs.data.clone());
                        board.archive.replace_with(state.archive);
                        // The calendars and what they said come down with the
                        // board: a client never fetches for itself (§23), and
                        // without this it would show none until the first
                        // board arrives, which can be minutes.
                        board.adopt_from_server(state.subscriptions, state.overlay);
                        // This copy's own phone page watches this board's version;
                        // it starts at the clock here too, so a page that saw the
                        // last run's numbers is not told the world went backwards.
                        board.seed_version(clock_version());
                        if let Err(why) = board.save_all() {
                            startup_errors.push(format!("Could not keep a local copy of the server's board:\n{why}"));
                        }
                        let (outbox, problem) = sync::Outbox::open(&dirs.data);
                        startup_errors.extend(problem);
                        // Anything created here before the server confirms it
                        // wears an id the server never issues — and never one
                        // the outbox still names from the last run.
                        board.number_from(outbox.highest_temporary_id().map_or(sync::CLIENT_ID_FLOOR, |id| id + 1));
                        sync_handle = Some(sync::start(remote, outbox, Arc::clone(&wake), state.version, true));
                        board
                        }
                    }
                    Err(why) if replica_already => {
                        // Out of touch, but this folder is the server's replica:
                        // use the last board seen and queue what happens.
                        startup_errors.push(format!(
                            "The server at {} could not be reached, so this is the last board seen from it; changes made now are sent when it is back.\n{why}",
                            remote.base()
                        ));
                        let (mut board, problems) = Board::open(dirs.data.clone());
                        startup_errors.extend(problems);
                        board.seed_version(clock_version());
                        let (outbox, problem) = sync::Outbox::open(&dirs.data);
                        startup_errors.extend(problem);
                        board.number_from(outbox.highest_temporary_id().map_or(sync::CLIENT_ID_FLOOR, |id| id + 1));
                        sync_handle = Some(sync::start(remote, outbox, Arc::clone(&wake), 0, false));
                        board
                    }
                    Err(why) => {
                        // A first contact that failed: this folder's board is
                        // still its own, and treating it as a replica now would
                        // have the server's board overwrite it at the next
                        // successful start. Run on it, and try the server again
                        // next time.
                        startup_errors.push(format!(
                            "The server at {} could not be reached for a first connection, so this copy runs on its own board for now. Start TaskDeck again when the server is up.\n{why}",
                            remote.base()
                        ));
                        let (mut board, problems) = Board::open(dirs.data.clone());
                        startup_errors.extend(problems);
                        board.seed_version(clock_version());
                        board
                    }
                }
            }
        }
    };

    // Whichever process owns the board fetches for it (§23). A client's
    // overlay arrives with the board it is a replica of, and a second fetcher
    // would be this machine asking a stranger's server for the same file on
    // the same timer as the server that already has it.
    let calendars = sync_handle.is_none().then(|| {
        subscriptions::start(board.subscriptions().to_vec(), board.overlay().clone(), Arc::clone(&wake))
    });

    let background_options = dirs.background_options();

    // Same treatment for the colour schemes: a corrupt file falls back to the
    // default scheme (inserted below) rather than panicking at boot.
    let mut colorschemes = match color::read_colorschemes(&dirs.data) {
        Ok(schemes) => schemes,
        Err(e) => {
            startup_errors.push(tasks::quarantine_corrupt_file(
                &dirs.data,
                "colorschemes.json",
                e.as_ref(),
            ));
            std::collections::HashMap::new()
        }
    };

    // The built-in palettes are (re)installed on every run, not seeded once into
    // an empty map: that way they are there after a wiped file, after an upgrade
    // from a version that didn't have them, and with the current colours rather
    // than whatever an old install happens to hold. Id 0 is COLORSCHEME ZERO, so
    // the untinted default look is unchanged.
    if color::install_builtins(&mut colorschemes, &mut selected_colorscheme_id) {
        if let Err(e) = color::save_colorschemes(&colorschemes, &dirs.data) {
            startup_errors.push(format!("Could not save the built-in colour schemes:\n{e}"));
        }
        // `install_builtins` may have moved a scheme off a reserved id; the
        // config has to follow, or the next run selects the built-in that took
        // its place.
        let _ = initialization::write_config_value(
            &dirs.config_file(),
            "selected_colorscheme_id",
            selected_colorscheme_id as i64,
        );
    }

    // A selection pointing at a scheme that is no longer there (hand-edited
    // file, deleted scheme) falls back to the untinted default rather than to
    // whatever `set_colorscheme` guesses later.
    if !colorschemes.contains_key(&selected_colorscheme_id) {
        selected_colorscheme_id = 0;
    }

    let setup_config = TaskAppConfig {
        colorschemes,
        selected_colorscheme_id,
        board,
        dirs,
        background,
        background_options,
        coordinates,
        start_in_fullscreen,
        enable_fps_counter,
        calendar_weeks_to_show,
        selected_monitor_name: selected_monitor_name.clone(),
        three_day_weather,
        background_image_tint_percent,
        ui_scale_percent,
        weather_service: get_weather(coordinates, proxy.clone()),
        calendars,
        startup_error: if startup_errors.is_empty() {
            None
        } else {
            Some(startup_errors.join("\n\n"))
        },
        phone_enabled: phone_server_enabled,
        phone_port: phone_server_port,
        phone_bind: phone_bind_address,
        phone_token,
        phone_tx,
        phone_rx,
        event_proxy: proxy,
        frame_cap_fps,
        sync: sync_handle,
        server_url,
        server_token,
    };

    let mut task_app = TaskApp::new(setup_config);

    //Perform sort before initializing app
    task_app.summarize_calendar();

    let mut app = App::new(task_app, window_size_startup, selected_monitor_name);

    event_loop.run_app(&mut app).expect("Failed to run app");
}
/// A version number no client has seen before: milliseconds on the clock.
/// A board's counter starts here rather than at zero, so a restart of the
/// process that serves it never hands out a number a client already holds.
fn clock_version() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}
