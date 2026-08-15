#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use mimalloc::MiMalloc;
use task_deck::{color, initialization::{self, App, Config, get_check_and_set_config}, paths::AppDirs, utilities, tasks::{self, Active}, ui::{TaskApp, TaskAppConfig}, weather::get_weather};
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

    let Config { start_in_fullscreen, coordinates, background, enable_fps_counter, window_size_startup, calendar_weeks_to_show, selected_monitor_name, mut selected_colorscheme_id, three_day_weather, background_image_tint_percent, ui_scale_percent } = get_check_and_set_config(&dirs.config_file());

    // Collected non-fatal startup recovery messages (e.g. quarantined corrupt
    // files), surfaced in the error window once the UI is up.
    let mut startup_errors: Vec<String> = Vec::new();

    // A corrupt/unreadable active set must not abort the boot; quarantine the
    // bad file and start from an empty set instead.
    let active_items: Vec<Active> = match tasks::read_at_startup(&dirs.data) {
        Ok(items) => items,
        Err(e) => {
            startup_errors.push(tasks::quarantine_corrupt_file(
                &dirs.data,
                "read_at_startup.json",
                e.as_ref(),
            ));
            Vec::new()
        }
    };

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

    let textbox_text = utilities::read_notepad_text(&dirs.data).unwrap_or("There was something wrong with taskdeck_data/notepad_text.json!".to_string());

    let setup_config = TaskAppConfig {
        colorschemes,
        selected_colorscheme_id,
        active_items,
        dirs,
        background,
        background_options,
        coordinates,
        start_in_fullscreen,
        enable_fps_counter,
        calendar_weeks_to_show,
        selected_monitor_name: selected_monitor_name.clone(),
        textbox_text,
        three_day_weather,
        background_image_tint_percent,
        ui_scale_percent,
        weather_service: get_weather(coordinates, proxy),
        startup_error: if startup_errors.is_empty() {
            None
        } else {
            Some(startup_errors.join("\n\n"))
        },
    };

    let mut task_app = TaskApp::new(setup_config);

    //Perform sort before initializing app
    task_app.summarize_calendar();

    let mut app = App::new(task_app, window_size_startup, selected_monitor_name);

    event_loop.run_app(&mut app).expect("Failed to run app");
}