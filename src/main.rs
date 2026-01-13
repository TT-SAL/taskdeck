#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::{fs, path::PathBuf};
use mimalloc::MiMalloc;
use task_deck::{color::{self, ColorScheme}, initialization::{App, Config, get_check_and_set_config}, utilities, tasks::{self, Active}, ui::{TaskApp, TaskAppConfig}, weather::get_weather};
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

    let Config { start_in_fullscreen, coordinates, background, enable_fps_counter, window_size_startup, calendar_weeks_to_show, selected_monitor_name, mut selected_colorscheme_id, three_day_weather, background_image_tint_percent } = get_check_and_set_config();

    //this allows us to use the debug exe as though it was located in the final folder structure
    let exe_file_path = std::env::current_exe().expect("error finding exe path");

    let active_items: Vec<Active> = tasks::read_at_startup(&exe_file_path).unwrap();

    let images_path = PathBuf::from("images");
    // Try reading the directory, if it fails, return an empty vector
    let background_options: Vec<String> = match fs::read_dir(&images_path) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok()) // ignore entries that caused errors
            .filter_map(|entry| entry.file_name().into_string().ok()) // convert OsString to String
            .collect(),
        Err(_) => vec![],
    };
    
    let mut colorschemes = color::read_colorschemes(&exe_file_path).unwrap();

    if colorschemes.is_empty() {
        colorschemes.insert(0, ColorScheme::default_scheme());
        selected_colorscheme_id = 0;
    }

    let textbox_text = utilities::read_notepad_text(&exe_file_path).unwrap_or("There was something wrong with data/notepad_text.json!".to_string());

    let setup_config = TaskAppConfig {
        colorschemes,
        selected_colorscheme_id,
        active_items,
        exe_file_path,
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
        weather_service: get_weather(coordinates, proxy),
    };

    let mut task_app = TaskApp::new(setup_config);

    //Perform sort before initializing app
    task_app.summarize_calendar();

    let mut app = App::new(task_app, window_size_startup, selected_monitor_name);

    event_loop.run_app(&mut app).expect("Failed to run app");
}