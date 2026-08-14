use std::{collections::HashMap, error::Error, fs, path::PathBuf, process::{Command, exit}, sync::{Arc, atomic::Ordering}, time::Instant};

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, Timelike, Weekday};
use egui::{self, Align, Button, Color32, ColorImage, ComboBox, Context, CornerRadius, Event, FontData, FontDefinitions, FontFamily, FontId, Grid, Key, Label, Layout, Margin, PointerButton, Pos2, Rect, RichText, Stroke, StrokeKind, TextureHandle, Ui, Vec2, ViewportCommand, pos2, vec2};
use image::{ImageBuffer, Rgba};
use toml_edit::{DocumentMut};

use crate::{calendarwidgets, color::{self, ColorScheme}, initialization::{DESIGN_WIDTH_POINTS, UI_SCALE_AUTO, UI_SCALE_MAX, UI_SCALE_MIN, clamp_ui_scale_percent}, paths::AppDirs, planner, utilities::{self, next_three_weekdays, resolve_colorscheme}, tasks::{self, Active, InActive}, weather::{self, WeatherService}};

const WEEK_DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

const URGENCY: [&str; 3] = ["Time-independence", "Normal urgency", "High urgency"];

const IMPORTANCE: [&str; 5] = ["Not important", "Mildly important", "Important", "Highly important", "Lethally important"];

/* ─────────────────────────── Day planner layout ─────────────────────────── */

/// Height of one hour on the planner timeline. Sized so a 15-minute block — the
/// snap step — is still a comfortable click target.
const PLANNER_HOUR_HEIGHT: f32 = 52.0;
/// Width of the hour-label gutter down the left of the timeline.
const PLANNER_GUTTER_WIDTH: f32 = 52.0;
/// Width of the backlog tray.
const PLANNER_TRAY_WIDTH: f32 = 250.0;
/// Grab area at the bottom of a block for resizing it.
const PLANNER_RESIZE_HANDLE: f32 = 8.0;
/// Hour brought into view when opening a day that isn't today.
const PLANNER_DEFAULT_SCROLL_HOUR: f32 = 7.0;
/// Vertical space the planner's header row and separator take, subtracted from
/// the window height to size the tray and timeline beneath them.
const PLANNER_HEADER_HEIGHT: f32 = 46.0;
/// Height of the selection inspector row. Reserved whether or not anything is
/// selected, so selecting doesn't shift the timeline under the pointer.
const PLANNER_INSPECTOR_HEIGHT: f32 = 26.0;
/// Step the automatic UI scale quantizes points-per-pixel to. See
/// `apply_ui_scale` — a quarter step keeps the layout close to the size that
/// fits while giving text a much better chance of a whole-pixel height.
const PPP_QUANTUM: f32 = 0.25;

/// Importance given to a task created straight on the timeline. Mid-scale: the
/// user is planning their day, not triaging, and can adjust it later.
const PLANNER_NEW_TASK_IMPORTANCE: u8 = 2;

/// The timeline gesture and the maths that interprets it live in `planner`;
/// this alias keeps the call sites here short.
use planner::Drag as PlannerDrag;

/// One row on the planner timeline: an item, and where it sits on the day being
/// shown. Rebuilt each frame from `active_things` — the planner has no cached
/// model of its own, so it can't drift out of sync with the calendar.
struct PlannerEntry {
    id: u64,
    name: String,
    color_id: usize,
    placement: planner::Placement,
}

/// Screen rectangle for a placement in its lane.
///
/// Blocks share the lane width with whatever overlaps them; due markers are
/// drawn full-width and short, because a deadline is a line in the day rather
/// than a claim on it.
fn planner_entry_rect(
    placement: planner::Placement,
    lane: planner::Lane,
    lane_area: Rect,
    geometry: &planner::TimelineGeometry,
) -> Rect {
    match placement {
        planner::Placement::Marker { at, .. } => {
            let top = geometry.y_for(at as f32);
            Rect::from_min_max(
                pos2(lane_area.left() + 4.0, top),
                pos2(lane_area.right() - 4.0, top + 18.0),
            )
        }
        planner::Placement::Block { start, minutes } => {
            let column_width = (lane_area.width() - 8.0) / lane.columns.max(1) as f32;
            let left = lane_area.left() + 4.0 + column_width * lane.column as f32;
            let top = geometry.y_for(start as f32);
            let bottom = geometry.y_for((start + minutes as i32) as f32);
            Rect::from_min_max(
                pos2(left + 1.0, top + 1.0),
                pos2(left + column_width - 2.0, (bottom - 1.0).max(top + 12.0)),
            )
        }
    }
}

struct FpsCounter {
    last_update: Instant,
    frame_count: u32,
    fps_text: String,
}

struct PressState {
    idx: usize,
    press_pos: Pos2,
    cancelled: bool,
}

/// One item in a day cell's compact preview (at most 3 are shown in the cell).
#[derive(Clone)]
struct PreviewItem {
    name: String,
    /// "HH:MM", or empty for an undated item.
    time: String,
    /// Palette index (see `Active::calendar_item_color`).
    color_id: usize,
}

/// One item in a day's full list, shown in the day popup. Carries the stable
/// `id` so the popup's complete/delete buttons act on the right item.
#[derive(Clone)]
struct DayItem {
    id: u64,
    name: String,
    time: String,
    is_event: bool,
}

/// One day cell of the calendar model, cached in `TaskApp::calendar_elements`
/// and consumed by `show_calendar` (the `preview`) and the day popup (`items`).
/// Named fields replace what used to be an opaque positional 6-tuple.
#[derive(Clone)]
struct DayCell {
    day_number: u8,
    preview: Vec<PreviewItem>,
    items: Vec<DayItem>,
    is_today: bool,
    date: NaiveDate,
    label: String,
}

impl FpsCounter {
    fn new() -> Self {
        Self {
            last_update: Instant::now(),
            frame_count: 0,
            fps_text: String::from("FPS: ..."),
        }
    }

    fn update(&mut self) {
        self.frame_count += 1;
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_update);
        if elapsed >= std::time::Duration::from_secs(1) {
            let fps = self.frame_count as f32 / elapsed.as_secs_f32();
            self.fps_text = format!("FPS: {:.1}", fps);
            self.frame_count = 0;
            self.last_update = now;
        }
    }
}

pub struct TaskAppConfig {
    pub colorschemes: HashMap<u32, ColorScheme>,
    pub selected_colorscheme_id: u32,
    pub active_items: Vec<Active>,
    pub dirs: AppDirs,
    pub background: String,
    pub background_options: Vec<String>,
    pub coordinates: [f32; 2],
    pub start_in_fullscreen: bool,
    pub enable_fps_counter: bool,
    pub calendar_weeks_to_show: usize,
    pub selected_monitor_name: String,
    pub textbox_text: String,
    pub three_day_weather: bool,
    pub background_image_tint_percent: u32,
    pub ui_scale_percent: u32,
    pub weather_service: WeatherService,
    /// Message describing any non-fatal startup recovery (e.g. a corrupt data
    /// file that was quarantined), to surface in the error window once the UI is
    /// up. `None` when startup loaded cleanly.
    pub startup_error: Option<String>,
}

pub struct TaskApp {
    /* ───────────────────────── Animation ───────────────────────── */
    row_anim: Vec<f32>,
    last_anim_time: f64,

    smoothed_scroll_velocity: f32,
    scroll_velocity_smoothing_tau: f32,
    animation_intensity: f32,
    smoothed_animation_intensity: f32,
    last_raw_scroll: f32,

    /* ───────────────────────── UI / Context ───────────────────────── */
    background_image_texture: Option<TextureHandle>,
    pending_initial_background: Option<String>,
    dirs: AppDirs,

    hovered_calendar_cell: Option<usize>,
    expanded_day: Option<usize>,
    offset: usize,
    press_origin: Option<PressState>,

    userconfig_path: PathBuf,

    /* ───────────────────────── Time & Date ───────────────────────── */
    date: DateTime<Local>,
    next_three_weekdays: (String, String, String),
    /// Timestamp of the most recent notepad edit, used to debounce autosave.
    /// `None` once there is nothing pending to save.
    last_textbox_edit_time: Option<Instant>,

    /* ───────────────────────── Tasks & Events ───────────────────────── */
    active_things: Vec<Active>,
    list_tasks: Vec<Active>,
    archive: Option<Vec<InActive>>,
    /// Next stable id to hand out to a newly created item. Seeded past the
    /// highest id present at startup (see `tasks::assign_missing_ids`).
    next_id: u64,

    calendar_elements: Vec<DayCell>,

    /* ───────────────────────── Weather ───────────────────────── */
    pub weather_service: WeatherService,
    weather_data_cache: Vec<Vec<(String, f64, i32, bool)>>,
    last_weather_version: u64,
    three_day_weather: bool,
    weather_is_broken_flag: bool,

    /* ───────────────────────── Inputs ───────────────────────── */
    week_number_input: String,
    task_name_input: String,
    task_importance_input: u8,
    time_importance_input: u8,
    event_name_input: String,

    year_input: i32,
    month_input: i32,
    day_input: i32,
    hour_input: i32,
    minute_input: i32,

    textbox_text: String,

    /* ───────────────────────── Flags ───────────────────────── */
    new_task_flag: bool,
    new_event_flag: bool,
    error_flag: bool,
    display_archive_flag: bool,
    expand_calendar_day_flag: bool,
    settings_flag: bool,
    should_save_textbox_text: bool,

    user_wants_to_complete_task_flag: bool,
    user_wants_to_delete_task_flag: bool,

    /* ───────────────────────── Day planner ───────────────────────── */
    /// Whether the planner window is open, and the day it is showing. The day is
    /// kept separately from `expanded_day` (a calendar cell index) so the
    /// planner can step forward past the end of the calendar's range.
    planner_flag: bool,
    planner_day: NaiveDate,
    /// Item the user last clicked on the timeline; its card shows the controls.
    planner_selection: Option<u64>,
    /// The pointer gesture in progress, if any. One field, because the gestures
    /// are mutually exclusive — you can't resize one block while moving another.
    planner_drag: Option<PlannerDrag>,
    /// Item whose title is being typed. Set right after a create so the block
    /// can be named without leaving the timeline.
    planner_naming: Option<u64>,
    planner_name_input: String,
    /// What a drag on empty timeline creates. Events are the common case, so
    /// they are the default.
    planner_creates_event: bool,
    /// Set for one frame after the planner opens or changes day, to scroll the
    /// timeline to a useful hour rather than to midnight.
    planner_scroll_to_hour: Option<f32>,

    /* ───────────────────────── Settings ───────────────────────── */
    start_in_fullscreen: bool,
    enable_fps_counter: bool,
    calendar_weeks_to_show: usize,

    selected_background_index: usize,
    background_options: Vec<String>,
    background_image_tint_percent: u32,
    background_tint_input: String,
    /// Percentage the whole UI is scaled by, or `UI_SCALE_AUTO` for the
    /// fit-to-window default. See `apply_ui_scale`.
    ui_scale_percent: u32,
    ui_scale_input: String,
    /// Points-per-pixel the text styles were last snapped for. See
    /// `apply_ui_scale` and `snap_font_points`.
    last_font_ppp: f32,
    /// Bumped once per `summarize_calendar`, and fed to the priority score's
    /// tie-break jitter. Keying the shuffle to rebuilds rather than to the
    /// clock is what keeps the planner's backlog — which re-sorts every frame —
    /// from reordering under the pointer. See `Active::tie_break_jitter`.
    shuffle_seed: u64,

    /* ───────────────────────── Errors & Confirmations ───────────────────────── */
    /// Id of the item awaiting a complete/delete confirmation. The dialog looks
    /// up the (cosmetic) name from this id for display.
    confirm_complete_task: Option<u64>,
    confirm_delete_task: Option<u64>,
    error_text: String,

    /* ───────────────────────── FPS / Monitor ───────────────────────── */
    fps_counter: FpsCounter,
    selected_monitor_name: String,
    pub monitor_options: Vec<String>, //this needs to be set as public because it is edited by the owner of the taskapp struct at runtime
    selected_monitor_index: usize,

    /* ───────────────────────── Map ───────────────────────── */
    coordinates_map_flag: bool,
    map_zoom: f32,
    map_offset: Vec2,
    /// Live, editable coordinates `[lat, lon]` — the single source of truth for
    /// the map picker. Pushed to the weather service (its own thread-local copy)
    /// and persisted only on confirm, via `set_weather_coordinates`.
    coordinates: [f32; 2],
    map_texture: Option<TextureHandle>,

    /* ───────────────────────── Color Schemes ───────────────────────── */
    color_picker_flag: bool,
    colorschemes: HashMap<u32, ColorScheme>,
    active_colorscheme: [Color32; 6],
    selected_colorscheme_id: u32,

    rename_colorscheme_flag: bool,
    colorscheme_rename_input: String,
    user_wants_to_delete_colorscheme_flag: bool,
    edit_colorscheme_flag: bool,
    colorscheme_being_edited: Option<ColorScheme>,
    dragged_color_index: Option<usize>,

    /* ───────────────────────── Calendar ───────────────────────── */
    row_contains_month_switch: Vec<Option<(String, String)>>,

    /* ───────────────────────── Misc ───────────────────────── */
    use_date_for_addable: bool,
}

impl TaskApp {
    pub fn new(config: TaskAppConfig) -> Self {
        let now = Local::now();

        let active_colorscheme =
            resolve_colorscheme(&config.colorschemes, config.selected_colorscheme_id);

        let selected_background_index = config
            .background_options
            .iter()
            .position(|b| b == &config.background)
            .unwrap_or(0);

        // Backfill stable ids onto any items from a pre-id / hand-edited save and
        // seed the id counter past the highest one in use.
        let mut active_items = config.active_items;
        let next_id = tasks::assign_missing_ids(&mut active_items);

        let userconfig_path = config.dirs.config_file();

        Self {
            /* Animation */
            row_anim: Vec::new(),
            last_anim_time: 0.0,
            smoothed_scroll_velocity: 0.0,
            scroll_velocity_smoothing_tau: 0.12,
            animation_intensity: 1.0,
            smoothed_animation_intensity: 0.0,
            last_raw_scroll: 0.0,

            /* UI */
            background_image_texture: None,
            pending_initial_background: Some(config.background),
            dirs: config.dirs,
            hovered_calendar_cell: None,
            expanded_day: None,
            offset: 0,
            press_origin: None,
            userconfig_path,

            /* Time */
            date: now,
            next_three_weekdays: next_three_weekdays(now),
            last_textbox_edit_time: None,

            /* Tasks */
            list_tasks: active_items
                .iter()
                .filter(|t| !t.is_event)
                .cloned()
                .collect(),
            active_things: active_items,
            archive: None,
            next_id,
            calendar_elements: Vec::new(),

            /* Weather */
            weather_service: config.weather_service,
            weather_data_cache: Vec::new(),
            last_weather_version: 0,
            three_day_weather: config.three_day_weather,
            weather_is_broken_flag: false,

            /* Inputs */
            week_number_input: config.calendar_weeks_to_show.to_string(),
            task_name_input: String::new(),
            task_importance_input: 2,
            time_importance_input: 1,
            event_name_input: String::new(),

            year_input: now.year(),
            month_input: now.month() as i32,
            day_input: now.day() as i32,
            hour_input: now.hour() as i32,
            minute_input: now.minute() as i32,

            textbox_text: config.textbox_text,

            /* Flags */
            new_task_flag: false,
            new_event_flag: false,
            error_flag: config.startup_error.is_some(),
            display_archive_flag: false,
            expand_calendar_day_flag: false,
            settings_flag: false,
            user_wants_to_complete_task_flag: false,
            user_wants_to_delete_task_flag: false,

            /* Day planner */
            planner_flag: false,
            planner_day: now.date_naive(),
            planner_selection: None,
            planner_drag: None,
            planner_naming: None,
            planner_name_input: String::new(),
            planner_creates_event: true,
            planner_scroll_to_hour: None,
            should_save_textbox_text: false,

            /* Settings */
            start_in_fullscreen: config.start_in_fullscreen,
            enable_fps_counter: config.enable_fps_counter,
            calendar_weeks_to_show: config.calendar_weeks_to_show,

            selected_background_index,
            background_options: config.background_options,
            background_image_tint_percent: config.background_image_tint_percent,
            background_tint_input: config.background_image_tint_percent.to_string(),
            ui_scale_percent: config.ui_scale_percent,
            ui_scale_input: config.ui_scale_percent.to_string(),
            last_font_ppp: 0.0,
            shuffle_seed: 0,

            /* Errors */
            confirm_complete_task: None,
            confirm_delete_task: None,
            error_text: config.startup_error.unwrap_or_default(),

            /* FPS / Monitor */
            fps_counter: FpsCounter::new(),
            selected_monitor_name: config.selected_monitor_name,
            monitor_options: Vec::new(),
            selected_monitor_index: 0,

            /* Map */
            coordinates_map_flag: false,
            map_zoom: 1.0,
            map_offset: Vec2::ZERO,
            coordinates: config.coordinates,
            map_texture: None,

            /* Colors */
            color_picker_flag: false,
            colorschemes: config.colorschemes,
            active_colorscheme,
            selected_colorscheme_id: config.selected_colorscheme_id,

            rename_colorscheme_flag: false,
            colorscheme_rename_input: String::new(),
            user_wants_to_delete_colorscheme_flag: false,
            edit_colorscheme_flag: false,
            colorscheme_being_edited: None,
            dragged_color_index: None,

            /* Calendar */
            row_contains_month_switch: Vec::new(),

            /* Misc */
            use_date_for_addable: true,
        }
    }
    
    fn sync_calendar_caches(&mut self) {
        if self.row_anim.len() != self.calendar_weeks_to_show {
            self.row_anim.resize(self.calendar_weeks_to_show, 0.0);
        }
    }

    pub fn init_with_context(&mut self, ctx: &Context) {
        load_fonts(ctx);
        set_styles(ctx);

        if self.start_in_fullscreen {
            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(true));
        }

        self.sync_calendar_caches();

        self.fix_and_cache_weather_data();

        egui_extras::install_image_loaders(ctx);

        self.map_texture = Some(set_world_map(ctx));
    }

    fn refilter_tasks(&mut self) {
        self.list_tasks = self.active_things.iter().filter(|task| task.is_event == false).cloned().collect();
    }

    fn show_tasks(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
        // egui 0.34+ changed the ScrollArea drag default to `DragScroll::OnTouch` (mouse drag no
        // longer scrolls). `ScrollSource::ALL` restores the 0.33 default (scrollbar + wheel +
        // mouse-drag) so click-drag scrolling works as before.
        .scroll_source(egui::scroll_area::ScrollSource::ALL)
        .wheel_scroll_multiplier(vec2(1.0, 1.5))
        .show(ui, |ui| {
            ui.set_min_size(egui::Vec2 { x: 290.0, y: 50.0 });
            ui.set_width(300f32);
            ui.vertical(|ui| {
                for task in self.list_tasks.iter() {
                    egui::Frame::new()
                        .fill(Color32::from_black_alpha(60))
                        .stroke(egui::Stroke::new(1.5, Color32::from_white_alpha(55)))
                        .corner_radius(egui::CornerRadius::same(14))
                        .inner_margin(Margin::symmetric(12, 12))
                        .show(ui, |ui| {
                            ui.set_width(245.0);
                            ui.set_min_size(egui::Vec2 { x: 258.0, y: 40.0 });
                            ui.set_max_size(egui::Vec2 { x: 245.0, y: 40.0 });
                            ui.horizontal(|ui| {
                                let task_font = FontId::new(17.0, FontFamily::Name("bungee".into()));
                                ui.set_width(245.0);
                                ui.set_min_size(egui::Vec2 { x: 245.0, y: 40.0 });
                                ui.set_max_size(egui::Vec2 { x: 245.0, y: 40.0 });
                                ui.add(Label::new(RichText::new(&task.name).color(Color32::from_white_alpha(120)).font(task_font)).wrap().selectable(false));
                                
                                if ui.ui_contains_pointer() {
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let min_button_size = Vec2::new(28.0, 28.0);

                                        let complete_button = egui::Button::new("✓").min_size(min_button_size).corner_radius(CornerRadius::same(8));
                                        let delete_button = egui::Button::new("x").min_size(min_button_size).corner_radius(CornerRadius::same(8));
                                        if ui.add(complete_button).clicked() {
                                            self.user_wants_to_complete_task_flag = true;
                                            self.confirm_complete_task = Some(task.id);
                                        }

                                        if ui.add(delete_button).clicked() {
                                            self.user_wants_to_delete_task_flag = true;
                                            self.confirm_delete_task = Some(task.id);
                                        }
                                    });
                                };
                            });
                        });
                }
            });
        });
    }

    fn display_stuff(&self, thing: &Vec<(String, f64, i32, bool)>, ui: &mut Ui, grid_id: String, upper_day: bool) {
        egui::Grid::new(grid_id)
            .spacing(Vec2::new(10.0, 10.0))
            .min_col_width(80.0)
            .max_col_width(80.0)
            .show(ui, |ui| {
                let nth_cell_to_highlight = match self.date.hour() {
                    0..2 => 0,
                    2..4 => 1,
                    4..6 => 2,
                    6..8 => 3,
                    8..10 => 4,
                    10..12 => 5,
                    12..14 => 6,
                    14..16 => 7,
                    16..18 => 8,
                    18..20 => 9,
                    20..22 => 10,
                    _ => 11,
                };
                for (i, (time, temp, wmo_code, is_day)) in thing.iter().enumerate() {
                    egui::Frame::default()
                        .stroke(
                            if i == nth_cell_to_highlight && upper_day {
                                Stroke::new(0.6, Color32::WHITE)
                            } else {
                                Stroke::new(0.5, Color32::from_white_alpha(150))
                            }                            
                        )
                        .corner_radius(CornerRadius::same(15))
                        .inner_margin(egui::Margin {
                            left: 10,
                            right: 10,
                            top: 8,
                            bottom: 6,
                        })
                        .show(ui, |ui| {
                            ui.with_layout(egui::Layout::bottom_up(Align::Center), |ui| {
                                let weather_icon_ref = weather::icon_for_wmo(*wmo_code, *is_day);
                                ui.add(egui::Image::new(weather_icon_ref.clone())
                                    .fit_to_exact_size(Vec2::new(48.0, 48.0)));

                                ui.add_space(-15.0);

                                ui.horizontal(|ui| {
                                    ui.add_space(37.0);

                                    ui.label(RichText::new(format!("{temp:.0}")).color(
                                        if i == nth_cell_to_highlight && upper_day {
                                            Color32::WHITE
                                        } else {
                                            Color32::from_white_alpha(120)
                                        }   ));
                                });

                                ui.add_space(-5.0);

                                let time_text = RichText::new(time)
                                    .color(
                                        if i == nth_cell_to_highlight && upper_day {
                                            Color32::WHITE
                                        } else {
                                            Color32::from_white_alpha(120)
                                        }   
                                )
                                    .size(14.0)
                                    .font(FontId { size:13.5, family: FontFamily::Name("space".into()) });

                                ui.label(time_text);
                            });
                        });
                    if (i + 1) % 4 == 0 {
                        ui.end_row();
                    }
                }
            });
    }

    fn show_weather_forecast(&mut self, ui: &mut Ui) {
        ui.vertical(|ui| {
            if self.weather_is_broken_flag {
                // No usable forecast yet — either the first fetch hasn't completed
                // or the data arrived in an unexpected shape. Show a small notice in
                // place of the grids; the notepad below stays reachable regardless.
                ui.add_space(75.0);
                ui.horizontal(|ui| {
                    ui.add_space(120.0);
                    ui.label(RichText::new("WEATHER IS BROKEN").size(14.0).color(Color32::from_white_alpha(165)));
                });
            } else {
                ui.horizontal(|ui| {
                    ui.add_space(147.0);
                    ui.label(RichText::new(&self.next_three_weekdays.0).size(14.0).color(Color32::from_white_alpha(165)));
                });
                ui.add_space(75.0);

                let day_1 = &self.weather_data_cache[0];
                self.display_stuff(day_1, ui, "firstweathergrid".to_string(), true);

                ui.add_space(5.0);

                ui.horizontal(|ui| {
                    ui.add_space(150.0);
                    ui.label(RichText::new(&self.next_three_weekdays.1).size(14.0).color(Color32::from_white_alpha(165)));
                });
                ui.add_space(75.0);

                let day_2 = &self.weather_data_cache[1];
                self.display_stuff(day_2, ui, "secondweathergrid".to_string(), false);

                if self.three_day_weather {
                    ui.add_space(5.0);

                    ui.horizontal(|ui| {
                        ui.add_space(150.0);
                        ui.label(RichText::new(&self.next_three_weekdays.2).size(14.0).color(Color32::from_white_alpha(165)));
                    });
                    ui.add_space(75.0);

                    let day_3 = &self.weather_data_cache[2];
                    self.display_stuff(day_3, ui, "thirdweathergrid".to_string(), false);
                }
            }

            // The notepad occupies the right panel whenever 3-day weather is off.
            // It is deliberately decoupled from `weather_is_broken_flag` so a failed
            // or still-pending weather fetch can never hide the user's notes
            // (CODE_REVIEW A6).
            if !self.three_day_weather {
                ui.add_space(15.0);
                ui.horizontal(|ui| {
                    ui.add_space(7.0);

                    egui::ScrollArea::vertical()
                        .min_scrolled_height(390.0)
                        .max_height(390.0)
                        .show(ui, |ui| {
                            if ui.add(egui::TextEdit::multiline(&mut self.textbox_text)
                                .desired_width(340.0)
                                .code_editor()
                                .font(FontId { size: 19.0, family: FontFamily::Monospace })
                                .background_color(Color32::from_black_alpha(40))
                            ).changed() {
                                self.should_save_textbox_text = true;
                                self.last_textbox_edit_time = Some(Instant::now());
                            }
                        });
                });
            }
        });
    }

    fn show_calendar(&mut self, ui: &mut egui::Ui) {
        #[inline]
        fn ease_out_quintic(x: f32) -> f32 {
            let t = x.clamp(0.0, 1.0);
            1.0 - (1.0 - t).powi(5)
        }

        #[inline]
        fn ease_out_square(x: f32) -> f32 {
            let t = x.clamp(0.0, 1.0);
            1.0 - (1.0 - t).powi(2)
        }

        #[inline]
        fn exponential_smooth(prev: f32, sample: f32, tau: f32, dt: f32) -> f32 {
            if dt <= 0.0 { return prev; }
            let alpha = 1.0 - (-dt / tau).exp();
            prev + (sample - prev) * alpha
        }

        /// Map smoothed velocity (px/s) -> intensity factor.
        /// tune constants to taste: v_scale controls how quickly intensity falls off,
        /// min_i and max_i control range.
        #[inline]
        fn map_velocity_to_intensity(v_px_per_s: f32) -> f32 {
            // typical: small velocities < ~200 px/s => boosted intensity,
            // large velocities > ~1500 px/s => reduced intensity.
            let v = v_px_per_s.abs();
            // normalize
            let v_scale = 100.0; // px/s where intensity is near midpoint
            let t = (v / v_scale).clamp(0.0, 4.0); // normalized (0..4)
            // non-linear mapping: high intensity at small t, low intensity at big t
            let min_i = 0.1_f32;
            let max_i = 2.5_f32;
            // Use smoothstep-like curve inverted: intensity = max_i - (smoothstep) * (max_i - min_i)
            // Smoothstep approx: s = t*t*(3 - 2*t) but we scale t to [0,1] first.
            let t1 = (t / 1.5).min(1.0); // compress into 0..1 for smoothstep
            let s = t1 * t1 * (3.0 - 2.0 * t1);
            let intensity = max_i - s * (max_i - min_i);
            intensity.clamp(min_i, max_i)
        }
        
        #[inline]
        fn exp_decay(prev: f32, target: f32, rate_per_s: f32, dt: f32) -> f32 {
            if dt <= 0.0 { return prev; }
            if rate_per_s <= 0.0 { return target; }
            let factor = (-rate_per_s * dt).exp();
            target + (prev - target) * factor
        }

        let mut visible_cells: Vec<(usize, Rect)> = Vec::new();

        let pointer_pos = ui.input(|i| i.pointer.latest_pos());

        // CONFIG
        let cell_size = Vec2::new(160.0, 215.0);
        let spacing_x = 14.0_f32;
        let spacing_y = 10.0_f32;
        let frame_corner = CornerRadius::same(14);
        let base_inner_margin = 12.0_f32;
        let max_inner_margin = 22.0_f32;
        let main_animation_decay_speed = 4.0_f32; //3.0
        let rows_total: usize = self.calendar_weeks_to_show;
        let cols_per_row: usize = 7;

        self.sync_calendar_caches();

        // Time delta - this contributes to monitor refresh rate related lag if handled improperly
        let now = ui.ctx().input(|i| i.time);
        let dt = (now - self.last_anim_time).clamp(1e-8, 0.1) as f32;
        self.last_anim_time = now;

        let total_width = (cols_per_row as f32 * cell_size.x) + ((cols_per_row - 1) as f32 * spacing_x as f32);
        let daybox_width = ((total_width) / 7.5).round();

        ui.vertical(|ui| {
            egui::Grid::new("calendarday grid")
                .min_col_width(daybox_width)
                .max_col_width(daybox_width)
                .show(ui, |ui| {
                    let day_current = self.calendar_elements.iter().position(|c| c.is_today);

                    for (i, day) in WEEK_DAYS.iter().enumerate() {
                        ui.vertical_centered(|ui| {
                            if day_current.iter().any(|x| x == &i) {
                                ui.label(RichText::new(*day).strong());
                            } else {
                                ui.label(RichText::new(*day));
                            }
                        });
                    }
                });
            ui.add_space(5.0);

            // Scroll area
            egui::ScrollArea::vertical()
                .id_salt("calendar_scrollage_row_scale")
                // Restore mouse click-drag scrolling, lost when egui 0.34+ changed the ScrollArea
                // drag default to `DragScroll::OnTouch`. `ScrollSource::ALL` == the old 0.33 default.
                .scroll_source(egui::scroll_area::ScrollSource::ALL)
                .wheel_scroll_multiplier(Vec2::new(1.0, 2.0))
                .show(ui, |ui| {
                    ui.set_min_width(total_width + 20.0);
                    ui.vertical_centered(|ui| {
                        // Geometry
                        let row_height = cell_size.y + spacing_y as f32;
                        let top_y = ui.cursor().min.y;

                        // --- measure scroll delta and update smoothed velocity ---
                        let clip = ui.clip_rect();

                        // keep a smoothed_scroll_pos in self
                        let raw_scroll = clip.min.y;

                        let scroll_delta = ui.input(|i| i.smooth_scroll_delta.y);

                        let raw_velocity = if dt > 0.0 {
                            (scroll_delta) / dt
                        } else {
                            0.0
                        }.abs();
                        self.last_raw_scroll = raw_scroll;

                        // then apply exponential smoothing to speed if desired
                        self.smoothed_scroll_velocity =
                            exponential_smooth(self.smoothed_scroll_velocity, raw_velocity, self.scroll_velocity_smoothing_tau, dt);

                        // Compute animation intensity factor
                        self.animation_intensity = map_velocity_to_intensity(self.smoothed_scroll_velocity);

                        let intensity_rate = 8.0;
                        self.smoothed_animation_intensity = exp_decay(self.smoothed_animation_intensity, self.animation_intensity, intensity_rate, dt);
                        let smoothed_intensity = self.smoothed_animation_intensity;

                        let start_row_f = ((raw_scroll - top_y) / row_height).floor() as isize;
                        let end_row_f = ((clip.max.y - top_y) / row_height).ceil() as isize - 1;
                        let start_row = start_row_f.clamp(0, (rows_total as isize) - 1) as usize;
                        let end_row = end_row_f.clamp(0, (rows_total as isize) - 1) as usize;
                       
                        let visible_lo = start_row.saturating_sub(1);
                        let visible_hi = (end_row + 1).min(rows_total - 1);
                        let base_rate = main_animation_decay_speed;
                        
                        let eps = 1e-10;
                        for row in 0..rows_total {
                            if row >= visible_lo && row <= visible_hi {
                                if (self.row_anim[row] - 1.0).abs() < eps {
                                    self.row_anim[row] = 1.0;
                                } else {
                                    let delay = (row as f32 * 0.05).min(1.0);
                                    let speed_mult = 0.85 + 0.3 * (smoothed_intensity - 1.0).clamp(-1.0, 1.0);
                                    let show_rate = base_rate * (1.0 - 0.25 * delay) * speed_mult;
                                    self.row_anim[row] = exp_decay(self.row_anim[row], 1.0, show_rate, dt).clamp(0.0, 1.0);
                                }
                            } else {
                                if self.row_anim[row] < eps {
                                    self.row_anim[row] = 0.0;
                                } else {
                                    let decay_rate = base_rate * 0.6;
                                    self.row_anim[row] = exp_decay(self.row_anim[row], 0.0, decay_rate, dt).clamp(0.0, 1.0);
                                }
                            }
                        }

                        // Skip before
                        if start_row > 0 {
                            ui.add_space(row_height * start_row as f32);
                        }

                        // Visible rows
                        for row in start_row..=end_row {
                            // anim value
                            let t_avg: f32 = ease_out_quintic(self.row_anim[row].clamp(0.0, 1.0));
                            let scale_min = 0.8;

                            let eps = 1e-10;
                            
                            let scale = if t_avg < eps {
                                scale_min
                            } else if (1.0 - t_avg) < eps {
                                1.0
                            } else {
                                scale_min + (1.0 - scale_min) * t_avg
                            };

                            // Reserve layout height
                            let row_top = ui.cursor().min.y;
                            let row_rect = Rect::from_min_size(
                                Pos2::new(ui.available_rect_before_wrap().min.x, row_top),
                                Vec2::new(ui.available_width(), row_height),
                            );

                            ui.add_space(row_height);

                            let scaled_size = row_rect.size() * scale;

                            let scaled_row_rect = row_rect.shrink2(
                                (row_rect.size() - scaled_size).max(Vec2::ZERO) * 0.5
                            );

                            // Build child UI inside scaled rect
                            let mut row_ui = ui.new_child(
                                egui::UiBuilder::new()
                                    .id_salt(row)
                                    .max_rect(scaled_row_rect)
                                    .layout(Layout::left_to_right(Align::Center)),
                            );

                            // Render all cells
                            for col in 0..cols_per_row {
                                let idx = row * cols_per_row + col;
                                if idx >= self.calendar_elements.len() {
                                    row_ui.allocate_space(cell_size);
                                    row_ui.add_space(spacing_x as f32);
                                    continue;
                                }

                                let (_, rect) = row_ui.allocate_space(cell_size);

                                let animation_level = self.row_anim[row].clamp(0.0, 1.0);
                                let t = ease_out_quintic(animation_level);
                                let color_factor = ease_out_square(animation_level);

                                let minimum_fill = 5f32;
                                let normal_fill = 60f32;

                                let fill = (minimum_fill - (-normal_fill + minimum_fill) * color_factor) as u8;

                                let fill_color = Color32::from_black_alpha(fill);
                                let stroke_color = Color32::from_white_alpha(fill - 5);

                                let hovered_fill_color = Color32::from_white_alpha(fill - 5);
                                let hovered_stroke_color = Color32::from_white_alpha(fill + 40);

                                if let Some(i) = self.hovered_calendar_cell && i == idx {
                                    row_ui.painter().rect_filled(rect, frame_corner, hovered_fill_color);
                                    row_ui.painter().rect_stroke(rect, frame_corner, Stroke::new(1.5, hovered_stroke_color), StrokeKind::Outside);
                                } else {
                                    row_ui.painter().rect_filled(rect, frame_corner, fill_color);
                                    row_ui.painter().rect_stroke(rect, frame_corner, Stroke::new(1.5, stroke_color), StrokeKind::Outside);
                                }

                                let eps = 1e-11;
                                let inner_margin_f = if (t - 1.0).abs() < eps {
                                    base_inner_margin
                                } else if t < eps {
                                    max_inner_margin
                                } else {
                                    base_inner_margin + (1.0 - t) * (max_inner_margin - base_inner_margin)
                                };

                                let inner_rect = rect.shrink(inner_margin_f);

                                visible_cells.push((idx, rect));

                                let hovered = pointer_pos.map_or(false, |p| rect.contains(p));
                                if hovered {
                                    self.hovered_calendar_cell = Some(idx);
                                }

                                row_ui.scope_builder(egui::UiBuilder::new().max_rect(inner_rect), |ui| {
                                    ui.set_min_size(inner_rect.size());
                                    let cell = &self.calendar_elements[idx];
                                    let preview = &cell.preview;
                                    let is_strong = cell.is_today;
                                    let day_label = &cell.label;
                                    ui.vertical(|ui| {
                                        let num = cell.items.len();
                                        if num == 0 {
                                            ui.add(calendarwidgets::DayNumber::new(day_label, is_strong));
                                            ui.with_layout(Layout::bottom_up(Align::RIGHT), |ui| {
                                                ui.add(calendarwidgets::RotatedNumberOnly::new(day_label, is_strong));
                                            });
                                        } else if num == 1 {
                                            let first = &preview[0];
                                            ui.add(calendarwidgets::DayHeader::new(day_label, &first.name, is_strong, &first.time, self.active_colorscheme[first.color_id]));
                                            ui.with_layout(Layout::bottom_up(Align::RIGHT), |ui| {
                                                ui.add(calendarwidgets::RotatedNumberOnly::new(day_label, is_strong));
                                            });
                                        } else if num == 2 {
                                            let first = &preview[0];
                                            ui.add(calendarwidgets::DayHeader::new(day_label, &first.name, is_strong, &first.time, self.active_colorscheme[first.color_id]));
                                            let second = &preview[1];
                                            ui.add(calendarwidgets::MiddleHeader::new(&second.name, Some(&second.time), self.active_colorscheme[second.color_id]));
                                            ui.with_layout(Layout::bottom_up(Align::RIGHT), |ui| {
                                                ui.add(calendarwidgets::RotatedNumberOnly::new(day_label, is_strong));
                                            });
                                        } else if num == 3 {
                                            let first = &preview[0];
                                            ui.add(calendarwidgets::DayHeader::new(day_label, &first.name, is_strong, &first.time, self.active_colorscheme[first.color_id]));
                                            let second = &preview[1];
                                            ui.add(calendarwidgets::MiddleHeader::new(&second.name, None, self.active_colorscheme[second.color_id]));
                                            let third = &preview[2];
                                            ui.add(calendarwidgets::BottomHeaderRotated::new(day_label, &third.name, is_strong, &third.time, Some(&second.time), self.active_colorscheme[third.color_id]));
                                        } else {
                                            let first = &preview[0];
                                            ui.add(calendarwidgets::DayHeader::new(day_label, &first.name, is_strong, &first.time, self.active_colorscheme[first.color_id]));
                                            let second = &preview[1];
                                            ui.add(calendarwidgets::MiddleHeader::new(&second.name, None, self.active_colorscheme[second.color_id]));
                                            let third = &preview[2];
                                            ui.add(calendarwidgets::ButtonHeaderRotated::new(day_label, &third.name, is_strong, &third.time, Some(&second.time), self.active_colorscheme[third.color_id]));
                                        }
                                    });
                                });

                                if !self.expand_calendar_day_flag {
                                    if hovered {
                                        self.hovered_calendar_cell = Some(idx);
                                    } else if self.hovered_calendar_cell == Some(idx) && !hovered {
                                        self.hovered_calendar_cell = None;
                                    }
                                }

                                row_ui.add_space(spacing_x as f32);
                            } // cols

                            if let Some(Some((this, next))) = self.row_contains_month_switch.get(row) {
                                row_ui.vertical(|ui| {
                                    let font_id = FontId {
                                        size: 12.0,
                                        family: FontFamily::Name("space".into()),
                                    };

                                    let font_color = Color32::from_rgba_unmultiplied(211, 215, 211, 210);

                                    ui.label(RichText::new(this).font(font_id.clone()).color(font_color));
                                    ui.label(RichText::new("↓").font(font_id.clone()).color(font_color));
                                    ui.label(RichText::new(next).font(font_id).color(font_color));
                                });
                            }
                        } // rows

                        // After visible rows
                        if end_row + 1 < rows_total {
                            let rows_after = rows_total - (end_row + 1);
                            ui.add_space(row_height * rows_after as f32);
                        }

                        const DRAG_THRESHOLD_POINTS: f32 = 6.0;

                        if !self.any_modal_open() {
                            // Inspect input events in place rather than cloning the
                            // whole event vector every frame (B5). The closure only
                            // mutates `self` fields and reads `visible_cells`; it must
                            // not call back into `ctx`/`ui` input (would re-lock).
                            ui.ctx().input(|i| {
                                for ev in &i.events {
                                    match ev {
                                        Event::PointerButton { pos, button, pressed, .. } => {
                                            if *button == PointerButton::Primary {
                                                if *pressed {
                                                    if let Some((idx, _)) = visible_cells.iter().find(|(_, r)| r.contains(*pos)) {
                                                        self.press_origin = Some(PressState {
                                                            idx: *idx,
                                                            press_pos: *pos,
                                                            cancelled: false,
                                                        });
                                                        #[cfg(debug_assertions)] {
                                                            println!("press_origin at {}", idx);
                                                        }
                                                    } else {
                                                        self.press_origin = None;
                                                    }
                                                } else {
                                                    if let Some(press) = self.press_origin.take() {
                                                        let release_idx_opt = visible_cells.iter().find(|(_, r)| r.contains(*pos)).map(|(i, _)| *i);

                                                        let dist = press.press_pos.distance(*pos);
                                                        let moved = dist > DRAG_THRESHOLD_POINTS;

                                                        if !press.cancelled && !moved {
                                                            if let Some(release_idx) = release_idx_opt {
                                                                if release_idx == press.idx {
                                                                    self.expanded_day = Some(release_idx);
                                                                    self.expand_calendar_day_flag = true;
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        },
                                        Event::PointerMoved(pos) => if let Some(press) = &mut self.press_origin {
                                            let dist = press.press_pos.distance(*pos);
                                            if dist > DRAG_THRESHOLD_POINTS {
                                                press.cancelled = true;
                                            }
                                        },
                                        Event::MouseWheel { .. } => {
                                            if let Some(press) = &mut self.press_origin {
                                                press.cancelled = true;
                                            }
                                        },
                                        _ => (),
                                    }
                                }
                            });
                        }
                        ui.add_space(2.0);
                    });
                });
        });
    }

    /// Hand out the next stable item id. See `tasks::assign_missing_ids`.
    fn next_item_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Add a fully-formed item, then rebuild the calendar and persist. Every
    /// creation path — the New Task/New Event modals and the planner — funnels
    /// through here, so "add" means the same thing (and saves once) everywhere.
    fn push_active_thing(&mut self, item: Active) {
        self.active_things.push(item);
        self.summarize_calendar();
        self.save_active_things();
    }

    /// Persist the active set, routing a failure to the error window. Shared by
    /// every mutation so none of them can quietly skip the save.
    fn save_active_things(&mut self) {
        if let Err(text) = tasks::oversafe_activesave(&self.active_things, &self.dirs.data) {
            self.show_error(format!("Saving error:\n{}", text.to_string()));
        }
    }

    fn add_active_thing(&mut self, name: String, deadline: Option<DateTime<Local>>, importance: Option<u8>, is_event: bool, time_importance: Option<u8>) {
        let id = self.next_item_id();
        self.push_active_thing(Active {
            id,
            name,
            deadline,
            importance,
            time_importance,
            is_event,
            created: chrono::Local::now(),
            // Nothing created through the modals is placed on the planner yet;
            // the planner sets these when the user gives the item a slot.
            planned_start: None,
            duration_minutes: None,
        });
    }

    fn delete_active_thing(&mut self, id: u64) {
        self.user_wants_to_delete_task_flag = false;
        self.active_things.retain(|task| task.id != id);
        self.confirm_delete_task = None;
        // A deleted item must not stay selected on the planner.
        if self.planner_selection == Some(id) {
            self.planner_selection = None;
        }
        self.summarize_calendar();
        self.save_active_things();
    }

    pub fn summarize_calendar(&mut self) {
        // Each rebuild gets a new shuffle seed, which is the whole extent of the
        // intentional gentle reshuffle (DOCUMENTATION §14.4).
        self.shuffle_seed = self.shuffle_seed.wrapping_add(1);

        // 1) Sort and separate active things
        let (mut events, tasks): (Vec<_>, Vec<_>) = self.active_things
            .drain(..)
            .partition(|a| a.is_event);

        // Sort events by deadline. Events are expected to always carry a deadline,
        // but a hand-edited / corrupted save could violate that. Sorting on the
        // `Option` (which orders `None` first) keeps this panic-free; the per-day
        // filtering below never places a deadline-less event on the grid, and such
        // items are still retained in `active_things` rather than dropped.
        events.sort_by_key(|e| e.deadline);

        // Sort tasks by importance score, highest first. The score is evaluated
        // exactly once per task here — capturing the intended per-rebuild random
        // shuffle a single time, which also keeps the comparator consistent — and
        // the resulting `f32` is compared directly. The old code cast the score to
        // `u16`, which saturated everything above 65535 (the high-importance
        // exponential curves and the 1e9 event/broken scores) to the same value
        // and flattened their ordering.
        let now = self.date;
        let mut scored_tasks: Vec<(f32, Active)> = tasks
            .into_iter()
            .map(|t| (t.importance_score(now, self.shuffle_seed), t))
            .collect();
        scored_tasks.sort_by(|(a, _), (b, _)| {
            b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal)
        });
        let tasks: Vec<Active> = scored_tasks.into_iter().map(|(_, t)| t).collect();

        let deadline_tasks: Vec<Active> = tasks.iter().filter(|task| task.deadline.is_some()).cloned().collect();

        // 2) Bucket dated items by day once, so each calendar cell is an O(1) map
        // lookup instead of a linear scan over every event/task (the old
        // O(days × items) rebuild). Build the buckets before moving the vecs into
        // active_things; iterating the already-sorted vecs keeps each bucket in
        // order — events by deadline, tasks by importance score (which the "take
        // 3" preview selection below relies on).
        let events_by_date = tasks::bucket_by_deadline_day(&events);
        let tasks_by_date = tasks::bucket_by_deadline_day(&deadline_tasks);

        // 3) Rebuild active_things sorted (if you need to keep the order)
        self.active_things.clear();
        self.active_things.extend(events.iter().cloned());
        self.active_things.extend(tasks);

        // 4) Determine the starting Monday
        let today = self.date;
        let monday = today
            .date_naive()
            .week(Weekday::Mon)
            .first_day();

        let mut calendar = Vec::new();

        let mut last_days_vec: Vec<Option<(String, String)>> = vec![];

        let empty: Vec<&Active> = Vec::new();

        // 5) Iterate n weeks x 7 days
        for week in 0..self.calendar_weeks_to_show {
            let mut contains_first_day_of_month = None;
            for day in 0..7 {
                let current = monday + Duration::days((week * 7 + day) as i64);

                if current.day() == 1 {
                    let prev_month = current - Duration::days(2);
                    contains_first_day_of_month = Some((prev_month.month().to_string(), current.month().to_string()));
                }

                let is_current_day: bool = current == self.date.date_naive();

                // O(1) lookups for this date's items (already ordered per bucket).
                let day_events = events_by_date.get(&current).unwrap_or(&empty);
                let day_tasks = tasks_by_date.get(&current).unwrap_or(&empty);

                // 6) Pick up to 3: events first, then tasks
                let mut chosen: Vec<&Active> = Vec::new();
                for e in day_events.iter().take(3) {
                    chosen.push(e);
                }
                if chosen.len() < 3 {
                    for t in day_tasks.iter().take(3 - chosen.len()) {
                        chosen.push(t);
                    }
                }

                // 7) Sort chosen by exact deadline time. Every item here came from
                // `day_events`/`day_tasks`, which only retain dated items, so the
                // deadline is present; format defensively regardless.
                chosen.sort_by_key(|a| a.deadline);

                let preview: Vec<PreviewItem> = chosen
                    .into_iter()
                    .map(|a| PreviewItem {
                        name: a.name.clone(),
                        time: a.deadline
                            .map(|d| d.format("%H:%M").to_string())
                            .unwrap_or_default(),
                        color_id: a.calendar_item_color(),
                    })
                    .collect();

                // 8) Build complete list for the day, sorted by deadline
                let mut all_for_day: Vec<&Active> = Vec::new();
                all_for_day.extend(day_events.iter().copied());
                all_for_day.extend(day_tasks.iter().copied());
                all_for_day.sort_by_key(|a| a.deadline);

                let items: Vec<DayItem> = all_for_day
                    .into_iter()
                    .map(|a| DayItem {
                        id: a.id,
                        name: a.name.clone(),
                        time: a.deadline
                            .map(|d| d.format("%H:%M").to_string())
                            .unwrap_or_default(),
                        is_event: a.is_event,
                    })
                    .collect();

                calendar.push(DayCell {
                    day_number: current.day() as u8,
                    preview,
                    items,
                    is_today: is_current_day,
                    date: current,
                    label: current.day().to_string(),
                });
            }
            last_days_vec.push(contains_first_day_of_month);
        }

        self.row_contains_month_switch = last_days_vec;

        self.calendar_elements = calendar;
        self.refilter_tasks();
    }

    /* ─────────────────────────── Day planner ─────────────────────────── */

    /// Open the planner on `day`, scrolled to something useful: the current hour
    /// when planning today, the start of the working day otherwise.
    fn open_planner(&mut self, day: NaiveDate) {
        self.planner_flag = true;
        self.planner_day = day;
        self.planner_selection = None;
        self.planner_drag = None;
        self.cancel_planner_naming();
        self.planner_scroll_to_hour = Some(self.planner_default_scroll_hour());
    }

    /// Hour to bring into view when the day changes. Today opens an hour before
    /// now so what's next is on screen with a little context above it.
    fn planner_default_scroll_hour(&self) -> f32 {
        if self.planner_day == self.date.date_naive() {
            (self.date.hour() as f32 - 1.0).max(0.0)
        } else {
            PLANNER_DEFAULT_SCROLL_HOUR
        }
    }

    fn planner_go_to_day(&mut self, day: NaiveDate) {
        self.planner_day = day;
        self.planner_selection = None;
        self.planner_drag = None;
        self.cancel_planner_naming();
        self.planner_scroll_to_hour = Some(self.planner_default_scroll_hour());
    }

    fn close_planner(&mut self) {
        // Committing first means a half-typed title isn't silently thrown away
        // by closing the window.
        self.commit_planner_naming();
        self.planner_flag = false;
        self.planner_drag = None;
        self.planner_selection = None;
    }

    /// The items that appear on `planner_day`'s timeline, in a stable order.
    ///
    /// One item can produce two entries across different days (a work block and
    /// a due marker), but never two on the same day — `placement_for` picks the
    /// block when both fall on the day being shown.
    fn planner_entries(&self) -> Vec<PlannerEntry> {
        let mut entries: Vec<PlannerEntry> = self
            .active_things
            .iter()
            .filter_map(|item| {
                planner::placement_for(item, self.planner_day).map(|placement| PlannerEntry {
                    id: item.id,
                    name: item.name.clone(),
                    color_id: item.calendar_item_color(),
                    placement,
                })
            })
            .collect();
        // Stable, time-ordered: the lane packer sorts its own copy, but a stable
        // input order keeps egui widget ids from shuffling between frames.
        entries.sort_by_key(|e| (e.placement.start(), e.id));
        entries
    }

    /// Tasks with no time set aside for them yet — the planner's backlog tray.
    /// Ordered by the same urgency score as the main task list, so the most
    /// pressing thing to schedule is at the top.
    fn planner_backlog_items(&self) -> Vec<(u64, String, usize, Option<DateTime<Local>>)> {
        let now = Local::now();
        let mut items: Vec<(f32, &Active)> = self
            .active_things
            .iter()
            .filter(|item| !item.is_event && item.planned_start.is_none())
            .map(|item| (item.importance_score(now, self.shuffle_seed), item))
            .collect();
        items.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        items
            .into_iter()
            .map(|(_, item)| (item.id, item.name.clone(), item.calendar_item_color(), item.deadline))
            .collect()
    }

    /// Give an item a slot on the planner's current day.
    ///
    /// An event *is* its time, so this moves its deadline. A task's deadline is
    /// when it is **due**, which planning must not touch — it gets a
    /// `planned_start` instead, and keeps showing a due marker on its own day.
    fn plan_item(&mut self, id: u64, start_minutes: i32, minutes: u32) {
        let Some(when) = planner::resolve_on_day(self.planner_day, start_minutes) else {
            self.show_error("That time doesn't exist on this day (daylight saving).".to_string());
            return;
        };

        let Some(item) = self.active_things.iter_mut().find(|item| item.id == id) else {
            return;
        };

        if item.is_event {
            item.deadline = Some(when);
        } else {
            item.planned_start = Some(when);
        }
        item.duration_minutes = Some(minutes.max(planner::MIN_BLOCK_MINUTES));

        self.summarize_calendar();
        self.save_active_things();
    }

    /// Return a task to the backlog, keeping the task itself (and its deadline)
    /// intact. Only tasks can be unplanned — an event with no time isn't an
    /// event, so its block offers delete instead.
    fn unplan_item(&mut self, id: u64) {
        if let Some(item) = self.active_things.iter_mut().find(|item| item.id == id) {
            if item.is_event {
                return;
            }
            item.planned_start = None;
            item.duration_minutes = None;
        }
        if self.planner_selection == Some(id) {
            self.planner_selection = None;
        }
        self.summarize_calendar();
        self.save_active_things();
    }

    /// Create an item directly on the timeline and put its title into edit mode,
    /// so blocking out time is one drag and a few keystrokes rather than a trip
    /// through the New Event dialog's date fields.
    fn create_planned_item(&mut self, start_minutes: i32, minutes: u32, is_event: bool) {
        let Some(when) = planner::resolve_on_day(self.planner_day, start_minutes) else {
            self.show_error("That time doesn't exist on this day (daylight saving).".to_string());
            return;
        };

        let id = self.next_item_id();
        self.push_active_thing(Active {
            id,
            name: String::new(),
            // An event lives at its deadline; a planned task is placed by
            // `planned_start` and starts life with no due date of its own.
            deadline: if is_event { Some(when) } else { None },
            planned_start: if is_event { None } else { Some(when) },
            duration_minutes: Some(minutes),
            importance: if is_event { None } else { Some(PLANNER_NEW_TASK_IMPORTANCE) },
            time_importance: None,
            is_event,
            created: Local::now(),
        });

        self.planner_selection = Some(id);
        self.begin_planner_naming(id);
    }

    fn begin_planner_naming(&mut self, id: u64) {
        self.planner_naming = Some(id);
        self.planner_name_input = self
            .active_things
            .iter()
            .find(|item| item.id == id)
            .map(|item| item.name.clone())
            .unwrap_or_default();
    }

    /// Commit the in-place title edit. An untitled *new* block would be a
    /// mystery rectangle, so an empty name falls back to a placeholder rather
    /// than being stored blank.
    fn commit_planner_naming(&mut self) {
        let Some(id) = self.planner_naming.take() else { return };
        let typed = self.planner_name_input.trim().to_string();
        self.planner_name_input.clear();

        let Some(item) = self.active_things.iter_mut().find(|item| item.id == id) else {
            return;
        };
        let placeholder = if item.is_event { "New event" } else { "New task" };
        item.name = if typed.is_empty() { placeholder.to_string() } else { typed };

        self.summarize_calendar();
        self.save_active_things();
    }

    /// Abandon a title edit without saving. Used when the planner closes or the
    /// day changes underneath the editor.
    fn cancel_planner_naming(&mut self) {
        self.planner_naming = None;
        self.planner_name_input.clear();
    }

    /// The planner window: a backlog tray on the left, a scrollable day timeline
    /// on the right, and a header that navigates days and reports the load.
    fn show_planner(&mut self, ctx: &Context) {
        if !self.planner_flag {
            return;
        }

        // Sized from the viewport rather than fixed: the planner wants as much of
        // the day on screen at once as it can get, and the viewport is a
        // different number of points on every machine (see `apply_ui_scale`).
        let viewport = ctx.viewport_rect();
        let width = (viewport.width() - 140.0).clamp(640.0, 1600.0);
        let height = (viewport.height() - 110.0).clamp(380.0, 1180.0);
        let body_height = height - PLANNER_HEADER_HEIGHT - PLANNER_INSPECTOR_HEIGHT;

        let mut open = true;
        egui::Window::new(RichText::new("Day planner").size(18.0))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .default_size(vec2(width, height))
            .show(ctx, |ui| {
                ui.set_width(width);
                ui.set_height(height);

                self.planner_header(ui);
                self.planner_inspector(ui);
                ui.separator();

                ui.horizontal_top(|ui| {
                    self.planner_backlog(ui, body_height);
                    ui.add_space(8.0);
                    self.planner_timeline(ui, body_height);
                });
            });

        // The window's own ✕ has to go through `close_planner` so a pending
        // title edit is committed and any in-flight drag is dropped.
        if !open {
            self.close_planner();
        }

        // Escape backs out one level at a time: first out of a title edit, then
        // out of the planner. Closing straight from the editor would be a
        // surprise mid-sentence.
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            if self.planner_naming.is_some() {
                self.commit_planner_naming();
            } else {
                self.close_planner();
            }
        }

        // A drag released outside the timeline (or outside the window entirely)
        // must not leave a gesture stuck to the pointer.
        if self.planner_drag.is_some() && !ctx.input(|i| i.pointer.any_down()) {
            self.planner_drag = None;
        }
    }

    fn planner_header(&mut self, ui: &mut Ui) {
        let today = self.date.date_naive();

        ui.horizontal(|ui| {
            if ui.button(RichText::new("◀").size(16.0)).on_hover_text("Previous day").clicked() {
                let day = self.planner_day.pred_opt().unwrap_or(self.planner_day);
                self.planner_go_to_day(day);
            }
            if ui.button("Today").clicked() {
                self.planner_go_to_day(today);
            }
            if ui.button(RichText::new("▶").size(16.0)).on_hover_text("Next day").clicked() {
                let day = self.planner_day.succ_opt().unwrap_or(self.planner_day);
                self.planner_go_to_day(day);
            }

            ui.add_space(12.0);

            let (weekday, full_date) = utilities::format_date(self.planner_day);
            ui.label(RichText::new(weekday).size(18.0).strong());
            ui.label(RichText::new(full_date).size(15.0));
            if self.planner_day == today {
                ui.label(RichText::new("· today").size(13.0).color(Color32::from_white_alpha(140)));
            }

            // Summary and the create-type toggle sit at the far right, so the
            // eye lands on the date first.
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.selectable_value(&mut self.planner_creates_event, false, "Task");
                ui.selectable_value(&mut self.planner_creates_event, true, "Event");
                ui.label(RichText::new("Drag creates:").size(13.0).color(Color32::from_white_alpha(150)));

                ui.add_space(16.0);

                let entries = self.planner_entries();
                let placements: Vec<_> = entries.iter().map(|e| e.placement).collect();
                let summary = planner::summarize(&placements);
                let mut parts = vec![format!(
                    "{} planned",
                    planner::format_duration(summary.planned_minutes.max(0) as u32)
                )];
                if summary.blocks > 0 {
                    parts.push(format!("{} block{}", summary.blocks, if summary.blocks == 1 { "" } else { "s" }));
                }
                if summary.due > 0 {
                    parts.push(format!("{} due", summary.due));
                }
                ui.label(RichText::new(parts.join(" · ")).size(13.0).color(Color32::from_white_alpha(190)));
            });
        });
    }

    /// Controls for whatever is selected on the timeline.
    ///
    /// These used to be a button strip drawn inside the block itself, which was
    /// wrong twice over: a 15-minute block has no room for three buttons, and
    /// the block's own drag target is registered over the same pixels and ate
    /// the clicks. A row of its own also gives importance somewhere to live —
    /// a task created by dragging on the timeline was previously stuck at the
    /// default with no way to change it without going through the task list.
    fn planner_inspector(&mut self, ui: &mut Ui) {
        let Some(id) = self.planner_selection else {
            // Reserve the row so selecting something doesn't shove the timeline
            // down and move what the user is aiming at.
            ui.add_space(PLANNER_INSPECTOR_HEIGHT);
            return;
        };

        let Some(item) = self.active_things.iter().find(|item| item.id == id) else {
            self.planner_selection = None;
            ui.add_space(PLANNER_INSPECTOR_HEIGHT);
            return;
        };

        // Copy out what the row needs; the closure below takes `&mut self`.
        let name = item.name.clone();
        let is_event = item.is_event;
        let is_planned = item.is_planned();
        let anchor = item.planner_anchor();
        let duration = item.duration_minutes;
        let deadline = item.deadline;
        let mut importance = item.importance;
        let mut time_importance = item.time_importance;

        let mut complete = false;
        let mut delete = false;
        let mut unplan = false;
        let mut rename = false;
        let mut changed = false;

        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.label(RichText::new(if is_event { "Event" } else { "Task" }).size(12.0).color(Color32::from_white_alpha(140)));
            ui.label(RichText::new(&name).size(14.0).strong());

            if let Some(anchor) = anchor {
                let when = match duration {
                    Some(minutes) => format!(
                        "{}–{} · {}",
                        anchor.format("%H:%M"),
                        (anchor + Duration::minutes(minutes as i64)).format("%H:%M"),
                        planner::format_duration(minutes)
                    ),
                    None => anchor.format("%H:%M").to_string(),
                };
                ui.label(RichText::new(when).size(12.0).color(Color32::from_white_alpha(170)));
            }

            // Spell out the deadline for tasks. Blocking out time on the planner
            // sets *when you will do it*, never a due date, and there is no way
            // to tell that from the block alone — so say it here rather than
            // leaving people to guess what a dragged-out task is due.
            if !is_event {
                let due = match deadline {
                    Some(deadline) => format!("due {}", deadline.format("%a %d %b %H:%M")),
                    None => "no deadline".to_string(),
                };
                ui.label(
                    RichText::new(due)
                        .size(12.0)
                        .color(if deadline.is_some() {
                            Color32::from_white_alpha(190)
                        } else {
                            Color32::from_white_alpha(120)
                        }),
                )
                .on_hover_text(
                    "A planned slot is when you will work on this; a deadline is when it is due. \
                     Dragging on the timeline sets the slot and leaves the deadline alone.",
                );
            }

            if ui.small_button("✎").on_hover_text("Rename").clicked() {
                rename = true;
            }

            // Events take their colour from being events (palette index 5) and
            // are ordered by time, so importance would mean nothing for them.
            if !is_event {
                ui.add_space(10.0);
                if let Some(level) = time_importance.as_mut() {
                    ui.label(RichText::new("Urgency:").size(12.0));
                    ComboBox::from_id_salt("planner_urgency")
                        .selected_text(URGENCY[(*level as usize).min(URGENCY.len() - 1)])
                        .show_ui(ui, |ui| {
                            for (index, label) in URGENCY.iter().enumerate() {
                                if ui.selectable_value(level, index as u8, *label).clicked() {
                                    changed = true;
                                }
                            }
                        });
                } else {
                    let level = importance.get_or_insert(PLANNER_NEW_TASK_IMPORTANCE);
                    ui.label(RichText::new("Importance:").size(12.0));
                    ComboBox::from_id_salt("planner_importance")
                        .selected_text(IMPORTANCE[(*level as usize).min(IMPORTANCE.len() - 1)])
                        .show_ui(ui, |ui| {
                            for (index, label) in IMPORTANCE.iter().enumerate() {
                                if ui.selectable_value(level, index as u8, *label).clicked() {
                                    changed = true;
                                }
                            }
                        });
                }
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(4.0);
                if ui.button("✗ Delete").clicked() {
                    delete = true;
                }
                if ui.button("✓ Complete").clicked() {
                    complete = true;
                }
                if !is_event && is_planned && ui.button("↩ Unplan").on_hover_text("Back to the unplanned tray").clicked() {
                    unplan = true;
                }
            });
        });

        if changed {
            if let Some(item) = self.active_things.iter_mut().find(|item| item.id == id) {
                item.importance = importance;
                item.time_importance = time_importance;
            }
            // Importance drives both the task order and the palette colour, so
            // the calendar and task list have to be rebuilt, not just saved.
            self.summarize_calendar();
            self.save_active_things();
        }
        if rename {
            self.begin_planner_naming(id);
        }
        if unplan {
            self.unplan_item(id);
        }
        if complete {
            self.confirm_complete_task = Some(id);
            self.user_wants_to_complete_task_flag = true;
        }
        if delete {
            self.confirm_delete_task = Some(id);
            self.user_wants_to_delete_task_flag = true;
        }
    }

    /// The backlog: every task with no time set aside for it. Cards are draggable
    /// onto the timeline, which is the whole point — planning a day should be
    /// moving things into it, not retyping them.
    fn planner_backlog(&mut self, ui: &mut Ui, body_height: f32) {
        ui.vertical(|ui| {
            ui.set_width(PLANNER_TRAY_WIDTH);

            ui.label(RichText::new("Unplanned tasks").size(14.0).strong());
            ui.label(
                RichText::new("drag onto the timeline")
                    .size(12.0)
                    .color(Color32::from_white_alpha(130)),
            );
            ui.add_space(6.0);

            let backlog = self.planner_backlog_items();
            if backlog.is_empty() {
                ui.add_space(12.0);
                ui.label(
                    RichText::new("Nothing waiting.\nEverything with a task card has a slot.")
                        .size(13.0)
                        .color(Color32::from_white_alpha(120)),
                );
                return;
            }

            let now = Local::now();
            egui::ScrollArea::vertical()
                .id_salt("planner_backlog")
                .max_height((body_height - 52.0).max(120.0))
                .show(ui, |ui| {
                    for (id, name, color_id, deadline) in backlog {
                        let being_dragged = matches!(
                            self.planner_drag,
                            Some(PlannerDrag::FromBacklog { id: dragged }) if dragged == id
                        );

                        let accent = self.planner_accent(color_id);
                        let frame = egui::Frame::new()
                            .fill(if being_dragged {
                                Color32::from_white_alpha(30)
                            } else {
                                Color32::from_black_alpha(60)
                            })
                            .stroke(Stroke::new(1.5, accent))
                            .corner_radius(CornerRadius::same(10))
                            .inner_margin(Margin::symmetric(10, 8));

                        let response = frame
                            .show(ui, |ui| {
                                ui.set_width(PLANNER_TRAY_WIDTH - 40.0);
                                ui.label(RichText::new(&name).size(14.0));
                                if let Some(deadline) = deadline {
                                    ui.label(
                                        RichText::new(format!(
                                            "due {} · {}",
                                            deadline.format("%a %H:%M"),
                                            planner::relative_due(deadline, now)
                                        ))
                                        .size(11.0)
                                        .color(Color32::from_white_alpha(150)),
                                    );
                                }
                            })
                            .response
                            .interact(egui::Sense::click_and_drag());

                        if response.drag_started() {
                            self.planner_drag = Some(PlannerDrag::FromBacklog { id });
                        }
                        if response.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                        }

                        ui.add_space(6.0);
                    }
                });
        });
    }

    /// The timeline: hour grid, blocks, and every pointer gesture that edits the
    /// day. Rebuilt from `active_things` each frame; the only persistent state is
    /// the in-flight drag.
    fn planner_timeline(&mut self, ui: &mut Ui, body_height: f32) {
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("planner_timeline")
            .max_height(body_height);
        if let Some(hour) = self.planner_scroll_to_hour.take() {
            scroll = scroll.vertical_scroll_offset(hour * PLANNER_HOUR_HEIGHT);
        }

        scroll.show(ui, |ui| {
            let width = ui.available_width().max(320.0);
            let geometry = planner::TimelineGeometry::new(0.0, PLANNER_HOUR_HEIGHT);

            // One allocation for the whole day. Sensing drags here (rather than
            // per-hour) is what makes "press on empty space and pull" work.
            let (rect, background) =
                ui.allocate_exact_size(vec2(width, geometry.full_height()), egui::Sense::click_and_drag());
            let geometry = planner::TimelineGeometry::new(rect.top(), PLANNER_HOUR_HEIGHT);
            let lane_area = Rect::from_min_max(
                pos2(rect.left() + PLANNER_GUTTER_WIDTH, rect.top()),
                rect.max,
            );

            self.paint_planner_grid(ui, rect, lane_area, &geometry);

            // Apply the in-flight gesture to the model *before* laying out, so
            // neighbours re-flow around the block being dragged as it moves.
            let pointer = ui.input(|i| i.pointer.interact_pos());
            let mut entries = self.planner_entries();
            let preview = self
                .planner_drag
                .clone()
                .and_then(|drag| self.planner_drag_preview_for(&drag, &entries, pointer, &geometry));
            if let Some((id, start, minutes)) = preview {
                match entries.iter_mut().find(|e| e.id == id) {
                    Some(entry) => entry.placement = planner::Placement::Block { start, minutes },
                    // A brand-new block (create drag) has no entry yet.
                    None => entries.push(PlannerEntry {
                        id,
                        name: String::new(),
                        color_id: if self.planner_creates_event { 5 } else { PLANNER_NEW_TASK_IMPORTANCE as usize },
                        placement: planner::Placement::Block { start, minutes },
                    }),
                }
            }

            let placements: Vec<_> = entries.iter().map(|e| e.placement).collect();
            let lanes = planner::lay_out(&placements);

            let block_rects: Vec<(u64, Rect)> = entries
                .iter()
                .zip(lanes.iter())
                .map(|(entry, lane)| (entry.id, planner_entry_rect(entry.placement, *lane, lane_area, &geometry)))
                .collect();

            // Interactions are registered *before* anything is drawn on top of
            // them. egui hit-tests the most recently added widget first, so a
            // control painted afterwards — the in-place title editor — wins the
            // click, instead of being swallowed by the block-sized drag target
            // covering it.
            self.handle_planner_gestures(ui, &background, &block_rects, &entries, pointer, &geometry, lane_area);

            for (entry, (_, block_rect)) in entries.iter().zip(block_rects.iter()) {
                self.paint_planner_entry(ui, entry, *block_rect);
            }
        });
    }

    /// Hour lines, hour labels, the shaded night hours, and the now-line.
    fn paint_planner_grid(
        &self,
        ui: &Ui,
        rect: Rect,
        lane_area: Rect,
        geometry: &planner::TimelineGeometry,
    ) {
        let painter = ui.painter_at(rect);

        painter.rect_filled(rect, CornerRadius::same(8), Color32::from_black_alpha(70));

        // Shade the hours most people aren't planning into, so the working day
        // reads as the foreground without hiding anything.
        for (from_hour, to_hour) in [(0.0, PLANNER_DEFAULT_SCROLL_HOUR), (22.0, 24.0)] {
            let shade = Rect::from_min_max(
                pos2(lane_area.left(), geometry.y_for(from_hour * 60.0)),
                pos2(lane_area.right(), geometry.y_for(to_hour * 60.0)),
            );
            painter.rect_filled(shade, CornerRadius::ZERO, Color32::from_black_alpha(45));
        }

        for hour in 0..=24 {
            let y = geometry.y_for(hour as f32 * 60.0);
            let on_the_hour = hour % 6 == 0;
            painter.line_segment(
                [pos2(lane_area.left(), y), pos2(lane_area.right(), y)],
                Stroke::new(
                    if on_the_hour { 1.2 } else { 0.6 },
                    Color32::from_white_alpha(if on_the_hour { 60 } else { 26 }),
                ),
            );

            if hour < 24 {
                painter.text(
                    pos2(rect.left() + PLANNER_GUTTER_WIDTH - 10.0, y + 2.0),
                    egui::Align2::RIGHT_TOP,
                    format!("{hour:02}"),
                    FontId::new(12.0, FontFamily::Monospace),
                    Color32::from_white_alpha(if on_the_hour { 190 } else { 110 }),
                );
                // Half-hour tick, to make a 30-minute block easy to read off.
                let half = geometry.y_for(hour as f32 * 60.0 + 30.0);
                painter.line_segment(
                    [pos2(lane_area.left(), half), pos2(lane_area.left() + 12.0, half)],
                    Stroke::new(0.6, Color32::from_white_alpha(40)),
                );
            }
        }

        if let Some(minutes) = planner::now_marker(self.planner_day, self.date) {
            let y = geometry.y_for(minutes as f32);
            let now_color = Color32::from_rgb(255, 120, 90);
            painter.line_segment(
                [pos2(lane_area.left(), y), pos2(lane_area.right(), y)],
                Stroke::new(1.6, now_color),
            );
            painter.circle_filled(pos2(lane_area.left(), y), 4.0, now_color);
        }
    }

    /// Accent colour for a planner item.
    ///
    /// The palette drives it, exactly as on the calendar — but the default
    /// scheme is six fully transparent entries (see `ColorScheme::default_scheme`),
    /// because on the calendar the background photo is meant to show through. A
    /// planner block has to read as a solid object you can grab, so a
    /// transparent palette entry falls back to a neutral highlight instead of
    /// disappearing.
    fn planner_accent(&self, color_id: usize) -> Color32 {
        let color = self.active_colorscheme[color_id.min(5)];
        if color.a() < 24 {
            Color32::from_white_alpha(85)
        } else {
            color
        }
    }

    /// Draw one block or due marker, plus the controls it reveals on hover.
    fn paint_planner_entry(&mut self, ui: &mut Ui, entry: &PlannerEntry, rect: Rect) {
        let palette = self.active_colorscheme[entry.color_id.min(5)];
        let accent = self.planner_accent(entry.color_id);
        let selected = self.planner_selection == Some(entry.id);
        let naming = self.planner_naming == Some(entry.id);

        match entry.placement {
            planner::Placement::Marker { at, due } => {
                let painter = ui.painter();
                // A due marker is deliberately flatter than a block: it marks a
                // line in the day rather than claiming time in it.
                painter.rect_filled(rect, CornerRadius::same(6), Color32::from_black_alpha(150));
                if !due {
                    painter.rect_filled(rect, CornerRadius::same(6), palette);
                }
                painter.rect_stroke(
                    rect,
                    CornerRadius::same(6),
                    Stroke::new(if selected { 2.0 } else { 1.0 }, accent),
                    StrokeKind::Inside,
                );
                let label = format!(
                    "{}  {}{}",
                    planner::format_minutes(at),
                    if due { "due · " } else { "" },
                    entry.name
                );
                painter.text(
                    pos2(rect.left() + 8.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    label,
                    FontId::new(12.0, FontFamily::Monospace),
                    Color32::from_white_alpha(220),
                );
            }
            planner::Placement::Block { start, minutes } => {
                let painter = ui.painter();
                // Base first, palette over it: the base guarantees the block is
                // legible under any scheme (including the transparent default),
                // and the palette still tints it the same colour the calendar
                // uses for this item.
                painter.rect_filled(rect, CornerRadius::same(8), Color32::from_black_alpha(170));
                painter.rect_filled(rect, CornerRadius::same(8), palette);
                painter.rect_stroke(
                    rect,
                    CornerRadius::same(8),
                    Stroke::new(
                        if selected { 2.2 } else { 1.2 },
                        if selected { Color32::WHITE } else { accent },
                    ),
                    StrokeKind::Inside,
                );

                let text_rect = rect.shrink2(vec2(8.0, 5.0));
                painter.text(
                    text_rect.left_top(),
                    egui::Align2::LEFT_TOP,
                    format!(
                        "{}–{}  ·  {}",
                        planner::format_minutes(start),
                        planner::format_minutes(start + minutes as i32),
                        planner::format_duration(minutes)
                    ),
                    FontId::new(11.0, FontFamily::Monospace),
                    Color32::from_white_alpha(200),
                );

                if naming {
                    // Type the title straight into the block.
                    let field = Rect::from_min_max(
                        pos2(text_rect.left(), text_rect.top() + 15.0),
                        pos2(text_rect.right(), (text_rect.top() + 41.0).min(text_rect.bottom())),
                    );
                    let mut committed = false;
                    ui.scope_builder(egui::UiBuilder::new().max_rect(field), |ui| {
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut self.planner_name_input)
                                .hint_text("name it")
                                .desired_width(field.width()),
                        );
                        response.request_focus();
                        if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                            committed = true;
                        }
                    });
                    if committed {
                        self.commit_planner_naming();
                    }
                } else if rect.height() > 26.0 {
                    painter.text(
                        pos2(text_rect.left(), text_rect.top() + 14.0),
                        egui::Align2::LEFT_TOP,
                        &entry.name,
                        FontId::new(14.0, FontFamily::Monospace),
                        Color32::WHITE,
                    );
                } else {
                    // Too short for two lines: title only, on the same row.
                    painter.text(
                        pos2(text_rect.right(), text_rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        &entry.name,
                        FontId::new(12.0, FontFamily::Monospace),
                        Color32::WHITE,
                    );
                }

                // The resize grip, hinted with a pair of lines along the bottom.
                if rect.height() >= 24.0 {
                    let grip_y = rect.bottom() - 4.0;
                    ui.painter().line_segment(
                        [pos2(rect.center().x - 12.0, grip_y), pos2(rect.center().x + 12.0, grip_y)],
                        Stroke::new(1.5, Color32::from_white_alpha(150)),
                    );
                }
            }
        }

    }

    /// Start, track, and commit the timeline gestures.
    fn handle_planner_gestures(
        &mut self,
        ui: &mut Ui,
        background: &egui::Response,
        block_rects: &[(u64, Rect)],
        entries: &[PlannerEntry],
        pointer: Option<Pos2>,
        geometry: &planner::TimelineGeometry,
        lane_area: Rect,
    ) {
        // --- start a gesture -------------------------------------------------
        if self.planner_drag.is_none() {
            for (entry, (id, rect)) in entries.iter().zip(block_rects.iter()) {
                // The block being named has a text field inside it. These
                // interaction rects are registered after the field is drawn, so
                // they sit on top of it and would swallow every click meant for
                // the cursor — leave the block alone until the title is done.
                if self.planner_naming == Some(*id) {
                    continue;
                }

                let planner::Placement::Block { start, minutes } = entry.placement else {
                    // Due markers are not draggable: a deadline is a fact about
                    // the task, not a plan, and moving it here would silently
                    // rewrite it. Click still selects.
                    let response = ui.interact(*rect, egui::Id::new(("planner_marker", *id)), egui::Sense::click());
                    if response.clicked() {
                        self.planner_selection = Some(*id);
                    }
                    continue;
                };

                let handle = Rect::from_min_max(
                    pos2(rect.left(), rect.bottom() - PLANNER_RESIZE_HANDLE),
                    rect.max,
                );
                let handle_response =
                    ui.interact(handle, egui::Id::new(("planner_resize", *id)), egui::Sense::click_and_drag());
                if handle_response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
                }
                if handle_response.drag_started() {
                    self.planner_drag = Some(PlannerDrag::Resize { id: *id, start });
                    self.planner_selection = Some(*id);
                    continue;
                }

                let body_response =
                    ui.interact(*rect, egui::Id::new(("planner_block", *id)), egui::Sense::click_and_drag());
                if body_response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                }
                if body_response.clicked() {
                    self.planner_selection = Some(*id);
                }
                if body_response.double_clicked() {
                    self.planner_selection = Some(*id);
                    self.begin_planner_naming(*id);
                }
                if body_response.drag_started() {
                    let grab = pointer
                        .map(|p| (geometry.minutes_at(p.y) - start as f32).round() as i32)
                        .unwrap_or(0)
                        .clamp(0, minutes as i32);
                    self.planner_drag = Some(PlannerDrag::Move { id: *id, grab_offset: grab, minutes });
                    self.planner_selection = Some(*id);
                }
            }

            // Empty timeline: pressing and pulling blocks out new time.
            if self.planner_drag.is_none() && background.drag_started() {
                if let Some(pos) = pointer {
                    let on_a_block = block_rects.iter().any(|(_, rect)| rect.contains(pos));
                    if !on_a_block && pos.x >= lane_area.left() {
                        self.commit_planner_naming();
                        self.planner_drag = Some(PlannerDrag::Create {
                            anchor: planner::snap(geometry.minutes_at(pos.y)),
                        });
                    }
                }
            }

            // A click on bare timeline clears the selection and any title edit.
            if background.clicked() {
                if let Some(pos) = pointer {
                    if !block_rects.iter().any(|(_, rect)| rect.contains(pos)) {
                        self.commit_planner_naming();
                        self.planner_selection = None;
                    }
                }
            }
        }

        // --- commit on release ----------------------------------------------
        let released = !ui.input(|i| i.pointer.any_down());
        if !released {
            return;
        }
        let Some(drag) = self.planner_drag.take() else { return };
        let Some(preview) = self.planner_drag_preview_for(&drag, entries, pointer, geometry) else {
            return;
        };
        let (_, start, minutes) = preview;

        // Dropping outside the lanes (on the hour gutter, or off the window) is
        // a cancel, not a plan at 00:00.
        let dropped_in_lanes = pointer.is_some_and(|p| {
            p.x >= lane_area.left() && p.x <= lane_area.right() && lane_area.y_range().contains(p.y)
        });

        match drag {
            PlannerDrag::Create { .. } => {
                if dropped_in_lanes {
                    let is_event = self.planner_creates_event;
                    self.create_planned_item(start, minutes, is_event);
                }
            }
            PlannerDrag::Move { id, .. } | PlannerDrag::Resize { id, .. } => {
                self.plan_item(id, start, minutes);
            }
            PlannerDrag::FromBacklog { id } => {
                if dropped_in_lanes {
                    self.plan_item(id, start, minutes);
                    self.planner_selection = Some(id);
                }
            }
        }
    }

    /// Where the in-flight gesture is currently placing a block, as
    /// `(id, start, minutes)`.
    ///
    /// The arithmetic lives in `planner::preview`, which is unit-tested; this
    /// only supplies the two things it can't know: the pointer's position in
    /// minutes, and the length to give a backlog card being dropped (its own, if
    /// it has been planned before, else the default).
    ///
    /// Taking the gesture as an argument rather than reading `self.planner_drag`
    /// lets the commit path call this *after* `take()`ing the gesture, so the
    /// live preview and the committed value come from one place and cannot
    /// disagree about where the block landed.
    fn planner_drag_preview_for(
        &self,
        drag: &PlannerDrag,
        entries: &[PlannerEntry],
        pointer: Option<Pos2>,
        geometry: &planner::TimelineGeometry,
    ) -> Option<(u64, i32, u32)> {
        let minutes_at_pointer = geometry.minutes_at(pointer?.y);

        let default_minutes = match drag {
            PlannerDrag::FromBacklog { id } => entries
                .iter()
                .find(|entry| entry.id == *id)
                .and_then(|entry| match entry.placement {
                    planner::Placement::Block { minutes, .. } => Some(minutes),
                    planner::Placement::Marker { .. } => None,
                })
                .unwrap_or(planner::DEFAULT_BLOCK_MINUTES),
            _ => planner::DEFAULT_BLOCK_MINUTES,
        };

        Some(planner::preview(drag, minutes_at_pointer, default_minutes))
    }

    fn show_error(&mut self, errortext: String) {
        self.error_flag = true;
        self.error_text = errortext;
    }

    /// True when any modal/overlay window is open. This is the single source of
    /// truth for "is the calendar covered" — used both to suppress the calendar
    /// tap/drag machine and to clear the hovered cell. It replaces two
    /// hand-maintained flag disjunctions that had drifted apart and *both* missed
    /// the map picker and colour-scheme editor (so calendar input leaked behind
    /// them). Adding a future modal means adding one line here.
    fn any_modal_open(&self) -> bool {
        self.new_task_flag
            || self.new_event_flag
            || self.settings_flag
            || self.display_archive_flag
            || self.expand_calendar_day_flag
            || self.planner_flag
            || self.error_flag
            || self.user_wants_to_complete_task_flag
            || self.user_wants_to_delete_task_flag
            || self.coordinates_map_flag
            || self.color_picker_flag
            || self.edit_colorscheme_flag
            || self.rename_colorscheme_flag
            || self.user_wants_to_delete_colorscheme_flag
    }

    fn complete_active_thing(&mut self, id: u64) {
        if let Some(thing) = self.active_things.iter().find(|x| x.id == id) {
            let found_inactive: InActive = thing.clone().to_inactive();

            if let Err(text) = tasks::save_inactive(&found_inactive, &self.dirs.data) {
                self.show_error(format!("Error archiving:\n{}", text.to_string()));
            };

            self.delete_active_thing(id);

            self.confirm_complete_task = None;
            self.user_wants_to_complete_task_flag = false;
        }
    }

    fn toggle_archive(&mut self) {
        self.display_archive_flag = !self.display_archive_flag;

        if !self.display_archive_flag {
            self.archive = None;
            self.offset = 0;
        } else {
            self.load_more_archives();
        }
    }

    fn load_more_archives(&mut self) {
        let new_items = tasks::read_lines_range(self.offset, 15, &self.dirs.data).unwrap_or_else(|_| Vec::new());
        self.offset += 15;

        if let Some(archive) = self.archive.as_mut() {
            archive.extend(new_items);
        } else {
            self.archive = Some(new_items);
        }
    }

    fn display_date_entering(&mut self, ui: &mut Ui) {
        let space_font = FontId::new(14.0, FontFamily::Name("space".into()));

        egui::Frame::default()
            .stroke(Stroke::new(0.9, Color32::from_white_alpha(80)))
            .corner_radius(CornerRadius::same(5))
            .inner_margin(Margin { left: 3, right: 3, top: 0, bottom: 2 })
            .show(ui, |ui| {
                ui.set_max_height(35.0);

                ui.horizontal_centered(|ui| {
                    // Constrain the day picker to the days that actually exist in
                    // the selected month/year (e.g. Feb has 28/29), and snap an
                    // already-selected day back into range when the month changes,
                    // so invalid dates like "Feb 31" can't be entered.
                    let max_day = utilities::days_in_month(self.year_input, self.month_input as u32) as i32;
                    self.day_input = self.day_input.clamp(1, max_day);

                    ComboBox::from_id_salt("day")
                        .width(40.0)
                        .selected_text(RichText::from(format!("{:02}", self.day_input)).font(space_font.clone()))
                        .show_ui(ui, |ui| {
                            for day in 1..=max_day {
                                ui.selectable_value(
                                    &mut self.day_input,
                                    day,
                                    RichText::from(format!("{:02}", day)).font(space_font.clone()),
                                );
                            }
                        });
                    ui.label(RichText::new(".").font(space_font.clone()));

                    ComboBox::from_id_salt("month")
                        .width(40.0)
                        .selected_text(RichText::from(format!("{:02}", self.month_input)).font(space_font.clone()))
                        .show_ui(ui, |ui| {
                            for month in 1..=12 {
                                ui.selectable_value(
                                    &mut self.month_input,
                                    month,
                                    RichText::from(format!("{:02}", month)).font(space_font.clone()),
                                );
                            }
                        });
                    ui.label(RichText::new(".").font(space_font.clone()));

                    ComboBox::from_id_salt("year")
                        .width(60.0)
                        .selected_text(RichText::from(format!("{}", self.year_input)).font(space_font.clone()))
                        .show_ui(ui, |ui| {
                            for year in 2000..=2100 {
                                ui.selectable_value(
                                    &mut self.year_input,
                                    year,
                                    RichText::from(year.to_string()).font(space_font.clone()),
                                );
                            }
                        });

                    ui.add_space(10.0);

                    ComboBox::from_id_salt("hour")
                        .width(35.0)
                        .selected_text(RichText::from(format!("{:02}", self.hour_input)).font(space_font.clone()))
                        .show_ui(ui, |ui| {
                            for hour in 0..=23 {
                                ui.selectable_value(
                                    &mut self.hour_input,
                                    hour,
                                    RichText::from(format!("{:02}", hour)).font(space_font.clone()),
                                );
                            }
                        });
                    ui.label(RichText::new(":").font(space_font.clone()));

                    ComboBox::from_id_salt("minute")
                        .width(35.0)
                        .selected_text(RichText::from(format!("{:02}", self.minute_input)).font(space_font.clone()))
                        .show_ui(ui, |ui| {
                            for minute in 0..=59 {
                                ui.selectable_value(
                                    &mut self.minute_input,
                                    minute,
                                    RichText::from(format!("{:02}", minute)).font(space_font.clone()),
                                );
                            }
                        });
                });
            });
    }

    /// Read the user config, set `key` to a typed value, and write it back.
    /// Single source of truth for the read-parse-set-write boilerplate every
    /// runtime setter used to duplicate. Values go in with their real TOML type
    /// (bool / integer / float-array / string) — never stringified numbers — so
    /// this agrees with the startup writer (`write_normalized_config`).
    fn write_config_value(
        &self,
        key: &str,
        value: impl Into<toml_edit::Value>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let toml_content = fs::read_to_string(&self.userconfig_path)?;
        let mut doc = toml_content.parse::<DocumentMut>()?;
        doc[key] = toml_edit::value(value);
        fs::write(&self.userconfig_path, doc.to_string())?;
        Ok(())
    }

    /// As `write_config_value`, but routes any failure to the error window
    /// instead of silently dropping it (disk full, permissions, locked file).
    fn persist_config_value(&mut self, key: &str, value: impl Into<toml_edit::Value>) {
        if let Err(e) = self.write_config_value(key, value) {
            self.show_error(format!("Could not save setting \"{}\":\n{}", key, e));
        }
    }

    fn set_calendar_weeks(&mut self) {
        let truncated: String = self.week_number_input.chars().take(5).collect();
        match truncated.parse::<usize>() {
            Ok(weeks) => {
                let clamped = weeks.clamp(
                    crate::initialization::CALENDAR_WEEKS_MIN,
                    crate::initialization::CALENDAR_WEEKS_MAX,
                );
                self.calendar_weeks_to_show = clamped;
                // Reflect the applied (post-clamp) value back into the field.
                self.week_number_input = clamped.to_string();
                self.persist_config_value("calendar_weeks_to_show", clamped as i64);
                // Apply immediately rather than only after a restart: rebuild the
                // calendar model and resize the per-row animation cache.
                self.summarize_calendar();
                self.sync_calendar_caches();
            }
            // Unparseable / empty input: restore the field to the active value.
            Err(_) => self.week_number_input = self.calendar_weeks_to_show.to_string(),
        }
    }
    /// Keep the whole layout inside the window by scaling the UI.
    ///
    /// The three columns are laid out at fixed point sizes adding up to
    /// [`DESIGN_WIDTH_POINTS`], which fits a 1920×1080 monitor at 100% display
    /// scaling — the setup this was built on — and nothing narrower. A HiDPI
    /// display reports *half* as many points as pixels, so a 3024px Retina screen
    /// is 1512 points and silently pushed the weather/notepad column off the right
    /// edge; Windows at 125% or 150% scaling does the same thing.
    ///
    /// Rather than re-tuning the hand-fitted widget geometry, this adjusts egui's
    /// zoom factor, which changes how many points the window is worth. The
    /// automatic value is the largest scale (never above 100%) at which the design
    /// width still fits.
    ///
    /// The computation is a fixed point, not a feedback loop: the window's
    /// physical width and native scale factor are both independent of the zoom, so
    /// `points × zoom` is a constant and re-running this on the next frame yields
    /// the same answer.
    fn apply_ui_scale(&mut self, ctx: &Context) {
        let target = if self.ui_scale_percent == UI_SCALE_AUTO {
            let current_zoom = ctx.zoom_factor();
            let width_in_points = ctx.viewport_rect().width();
            if width_in_points <= 0.0 {
                return;
            }
            // width_in_points * current_zoom is the window's width in
            // native-scale points, which is what the design width is expressed in.
            let fits_at = width_in_points * current_zoom / DESIGN_WIDTH_POINTS;

            // Quantize so the resulting points-per-pixel lands on a quarter
            // step. The app is set in Fixedsys, a pixel font (see
            // `snap_font_points`): an arbitrary scale like 0.78125 gives a
            // points-per-pixel of 1.5625, at which almost no text size is a
            // whole number of pixels and everything looks slightly melted.
            // Rounding *down* to a step keeps the layout fitting — it only ever
            // makes the UI smaller than strictly required.
            let native = ctx
                .input(|i| i.viewport().native_pixels_per_point)
                .unwrap_or(1.0)
                .max(0.1);
            let step = PPP_QUANTUM / native;
            let quantized = (fits_at / step).floor() * step;

            quantized.clamp(UI_SCALE_MIN as f32 / 100.0, UI_SCALE_MAX as f32 / 100.0)
        } else {
            self.ui_scale_percent as f32 / 100.0
        };

        // egui rebuilds every cached galley when the zoom changes, so only write
        // when it actually moved. The epsilon also stops sub-pixel jitter in the
        // automatic value from re-laying out the UI every frame.
        if (ctx.zoom_factor() - target).abs() > 0.001 {
            ctx.set_zoom_factor(target);
        }

        // The text sizes are snapped to whole pixels *at the current scale*, so
        // they have to be recomputed whenever the scale actually changes —
        // otherwise they stay snapped to the scale the app started at.
        let pixels_per_point = ctx.pixels_per_point();
        if (pixels_per_point - self.last_font_ppp).abs() > 0.001 {
            self.last_font_ppp = pixels_per_point;
            set_styles(ctx);
        }
    }

    fn set_ui_scale(&mut self) {
        let filtered: String = self.ui_scale_input.chars().take(3).collect();
        // An empty field, or anything unparseable, is read as "automatic" — the
        // same thing `0` means in the config file.
        let requested = filtered.trim().parse::<u32>().unwrap_or(UI_SCALE_AUTO);
        let clamped = clamp_ui_scale_percent(requested);
        self.ui_scale_percent = clamped;
        self.ui_scale_input = clamped.to_string();
        self.persist_config_value("ui_scale_percent", clamped as i64);
    }

    fn set_background_tint(&mut self) {
        let filtered: String = self.background_tint_input.chars().take(3).collect();
        if let Ok(number) = filtered.parse::<u32>() {
            let clamped = number.clamp(0, 100);
            self.background_image_tint_percent = clamped;
            self.persist_config_value("background_image_tint_percent", clamped as i64);
        }
    }
    fn set_weather_coordinates(&mut self) {
        let coords = self.coordinates;
        self.weather_service.set_coordinates(coords);
        self.persist_config_value("coordinates", utilities::float_pair_array(coords));
    }
    fn set_selected_monitor_name(&mut self) {
        self.selected_monitor_name = self.monitor_options.get(self.selected_monitor_index).unwrap_or(&"".to_string()).to_string();
        let name: String = self.selected_monitor_name.chars().take(1000).collect();
        self.persist_config_value("selected_monitor_name", name);
    }
    fn fix_and_cache_weather_data(&mut self) {
        self.weather_is_broken_flag = false;
        let static_weather_data = self.weather_service.data.read().map(|w| w.clone())
            .unwrap_or_else(|_| vec![]);

        // The reshape below indexes `static_weather_data[hour][day]` for all 24
        // hours and days 0..=2, so both the outer length (24 hours) and every
        // inner length (>= 3 days) must hold. A partial/short Open-Meteo response
        // (DST edge, API change, truncated payload) would otherwise panic here.
        if static_weather_data.len() != 24
            || static_weather_data.iter().any(|hour| hour.len() < 3)
        {
            self.weather_is_broken_flag = true;
            return ();
        }

        let mut weather_datas = vec![vec![], vec![], vec![]];

        for day in 0..=2 {
            for i in 0..12 {
                let index_first_hour = 2 * i;
                let index_second_hour = index_first_hour + 1;

                let data1 = static_weather_data[index_first_hour][day].clone();
                let data2 = static_weather_data[index_second_hour][day].clone();

                let mut temp_avg = (data1.temp + data2.temp) / 2_f64;

                temp_avg = temp_avg.round();

                let eps = 0.1;
                
                if (temp_avg.abs()) < eps {
                    temp_avg = 0.0;
                }

                //we set the weather code to be the greater one of the two
                let weather_code = data1.weather_code.max(data2.weather_code);

                //we maintain that the icon should contain the sun if the first or second hour is classified as being during the day
                weather_datas[day].push((data1.time, temp_avg, weather_code, data1.is_day == 1 || data2.is_day == 1));
            }
        }

        self.weather_data_cache = weather_datas;
    }
    fn restart_self(&mut self) {
        // Restart by spawning a fresh copy and exiting. Any failure is reported
        // and leaves the current process running rather than panicking — and
        // because we only `exit` on a successful spawn, a failed restart can't
        // tear down the running app or spin a respawn loop.
        let exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(e) => {
                self.show_error(format!("Could not restart (couldn't find the executable):\n{}", e));
                return;
            }
        };

        match Command::new(exe).args(std::env::args().skip(1)).spawn() {
            Ok(_) => exit(0),
            Err(e) => self.show_error(format!("Could not restart:\n{}", e)),
        }
    }
    fn save_textbox_text(&mut self) {
        if self.should_save_textbox_text {
            // A silent failure here loses the user's notes; surface it instead.
            if let Err(e) = utilities::save_notepad_text(self.textbox_text.clone(), &self.dirs.data) {
                self.show_error(format!("Could not save notepad text:\n{}", e));
            }
            self.should_save_textbox_text = false;
            self.last_textbox_edit_time = None;
        }
    }

    /// Force-persist any unsaved state before the application exits.
    /// Currently only the notepad text is buffered; this is a no-op when clean.
    pub fn flush_pending_saves(&mut self) {
        self.save_textbox_text();
    }
    fn set_colorscheme(&mut self) {
        let selected_scheme = if let Some(scheme) = self.colorschemes.get(&self.selected_colorscheme_id) {
            scheme.colors.map(|c| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]))
        } else {
            ColorScheme::default_scheme().colors.map(|c| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]))
        };

        self.active_colorscheme = selected_scheme;

        self.persist_config_value("selected_colorscheme_id", self.selected_colorscheme_id as i64);
    }
    fn rename_current_colorscheme(&mut self) {
        self.colorschemes.entry(self.selected_colorscheme_id).or_insert(ColorScheme::default_scheme()).rename(self.colorscheme_rename_input.clone());

        self.add_schemes_2_doc();
    }
    fn duplicate_current_colorscheme(&mut self) {
        let duplicate = self.colorschemes.get(&self.selected_colorscheme_id).unwrap_or(&ColorScheme::default_scheme()).duplicate();

        let new_id = self.colorschemes.keys().max().unwrap_or(&0) + 1;

        self.colorschemes.insert(new_id, duplicate);

        self.add_schemes_2_doc();
    }
    fn currently_selected_colorscheme_is_user_configurable(&self) -> bool {
        self.colorschemes.get(&self.selected_colorscheme_id).unwrap_or(&ColorScheme::default_scheme()).is_user_configurable
    }
    fn delete_current_colorscheme(&mut self) {
        if self.user_wants_to_delete_colorscheme_flag {
            let _ = self.colorschemes.remove_entry(&self.selected_colorscheme_id);

            let new_id = self.colorschemes.keys().max().unwrap_or(&0);
            self.selected_colorscheme_id = *new_id;

            self.set_colorscheme();

            self.add_schemes_2_doc();

            self.user_wants_to_delete_colorscheme_flag = false;
        }
    }
    fn add_schemes_2_doc(&self) {
        let _ = color::save_colorschemes(&self.colorschemes, &self.dirs.data);
    }
    fn save_colorscheme_edits(&mut self) {
        if let Some(scheme) = self.colorscheme_being_edited.take() {
            self.colorschemes.insert(self.selected_colorscheme_id, scheme);
        }
    }
    fn try_to_generate_colorscheme(&mut self) {
        let name = self.background_options[self.selected_background_index].clone();

        if let Some(scheme) = color::generate_colorscheme(&self.dirs, name) {
            let new_id = self.colorschemes.keys().max().unwrap_or(&0) + 1;

            self.colorschemes.insert(new_id, scheme);

            self.add_schemes_2_doc();
        }
    }
}

impl TaskApp {
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        // egui 0.35 is Ui-centric: the frame hands us a root `&mut Ui` (via `Context::run_ui`)
        // instead of a `&Context`. Bind an owned clone of the context to a local and reference it,
        // so every existing `ctx.*` / `Window::show(ctx, …)` call below stays unchanged while the
        // top-level panels can still borrow the root `ui` mutably (the clone doesn't borrow `ui`).
        let ctx_owned = ui.ctx().clone();
        let ctx = &ctx_owned;

        // Fit the fixed-width layout to whatever window we were given. Runs before
        // anything is drawn so the whole frame uses one consistent scale.
        self.apply_ui_scale(ctx);

        if self.background_image_texture.is_none() {
            if let Some(name) = self.pending_initial_background.take() {
                self.background_image_texture = Some(set_background(ctx, &self.dirs, name.clone()));
            }
        }

        if self.enable_fps_counter {
            self.fps_counter.update();
        }
        // F11 is the fullscreen key on Windows and Linux. macOS reserves it for
        // Mission Control's "show desktop" and never delivers it to the app, so
        // accept the platform's own Ctrl+Cmd+F there too.
        let toggle_fullscreen = ctx.input(|i| {
            i.key_pressed(Key::F11)
                || (cfg!(target_os = "macos")
                    && i.modifiers.mac_cmd
                    && i.modifiers.ctrl
                    && i.key_pressed(Key::F))
        });
        if toggle_fullscreen {
            let old_fullscreen = ctx.input(|i| i.viewport().fullscreen).unwrap_or(false);
            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(!old_fullscreen));
        }

        let current_weather = self.weather_service.version.load(Ordering::Relaxed);
        if current_weather != self.last_weather_version {
            self.fix_and_cache_weather_data();
            self.last_weather_version = current_weather;
        }

        let old_date = self.date;
        self.date = chrono::Local::now();
        if self.date.day() != old_date.day() {
            self.summarize_calendar();
            self.next_three_weekdays = next_three_weekdays(self.date);
        }

        // Debounced notepad autosave: persist ~2s after the last edit. This uses
        // wall-clock time so the cadence does not depend on the (uncapped) frame
        // rate. A final flush also runs on exit (App::exiting), so edits made just
        // before quitting are never lost.
        if self.should_save_textbox_text {
            let due = self
                .last_textbox_edit_time
                .map_or(true, |t| t.elapsed() >= std::time::Duration::from_secs(2));
            if due {
                self.save_textbox_text();
            }
        }

        egui::Panel::top("menu_bar").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                if ui.button("New Task").clicked() {
                    self.new_task_flag = true;
                }
                ui.add_space(12.0);
                if ui.button("New Event").clicked() {
                    self.new_event_flag = true;
                }
                ui.add_space(12.0);

                // Opens on today; the day popup's own button opens it on the day
                // you clicked.
                if self.planner_flag {
                    if ui.button("Planner").highlight().clicked() {
                        self.close_planner();
                    }
                } else {
                    if ui.button("Planner").clicked() {
                        let today = self.date.date_naive();
                        self.open_planner(today);
                    }
                }
                ui.add_space(12.0);

                if self.display_archive_flag {
                    if ui.button("Archived").highlight().clicked() {
                        self.toggle_archive();
                    }
                } else {
                    if ui.button("Archived").clicked() {
                        self.toggle_archive();
                    }
                }

                ui.add_space(12.0);

                if self.settings_flag {
                    if ui.button("Settings").highlight().clicked() {
                        self.settings_flag = false;
                    }
                } else {
                    if ui.button("Settings").clicked() {
                        self.settings_flag = true;
                    }
                }

                // Push a right-aligned layout for the Quit button
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Quit").clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }

                    ui.add_space(45.0);

                    if self.enable_fps_counter {
                        ui.label(self.fps_counter.fps_text.clone());
                    }
                });
            });
        });

        //PUT THE BACKGROUND COLOR INSIDE THE DEFAULT IN A FRAME
        //THIS IS WHERE YOU CONTROL THE OVERALL PANEL VISUALS
        // `available_rect_before_wrap()` on the root ui — after the menu bar panel above has
        // reserved its space — is the central region: the exact value the removed
        // `ctx.available_rect()` returned (before the CentralPanel's own inner margin).
        let screen_rect = ui.available_rect_before_wrap();
        egui::CentralPanel::default()
        .show(ui, |ui| {

            // Skip the background draw if the texture hasn't been initialized yet
            // rather than unwrapping. In practice it's set from
            // `pending_initial_background` at the top of `ui()`, but guarding here
            // removes the latent crash if that ever fails to run.
            if let Some(background_texture) = self.background_image_texture.as_ref() {
                ctx.layer_painter(egui::LayerId::background()).image(
                    background_texture.id(),
                    screen_rect,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0)),
                    egui::Color32::WHITE.gamma_multiply((self.background_image_tint_percent as f32 / 100.0).clamp(0.0, 1.0)),
                );
            }

            ui.horizontal_top(|ui| {

                ui.add_space(5.0);

                self.show_tasks(ui);

                self.show_calendar(ui);              

                ui.add_space(-20.0);

                self.show_weather_forecast(ui);
            });
        });

        if self.user_wants_to_complete_task_flag {
            if let Some(id) = self.confirm_complete_task {
                // Resolve the cosmetic name for display; if the item is gone
                // (e.g. removed underneath the dialog), dismiss instead of acting.
                if let Some(name) = self.active_things.iter().find(|x| x.id == id).map(|x| x.name.clone()) {
                    egui::Window::new("Confirm Complete")
                        .collapsible(false)
                        .resizable(false)
                        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                        .show(ctx, |ui| {
                            ui.label(format!("Are you sure you want to mark \"{}\" as complete?", name));
                            ui.horizontal(|ui| {
                                if ui.button("Yes").clicked() {
                                    self.complete_active_thing(id);
                                }
                                if ui.button("No").clicked() {
                                    self.confirm_complete_task = None;
                                    self.user_wants_to_complete_task_flag = false;
                                }
                            });
                        });
                } else {
                    self.confirm_complete_task = None;
                    self.user_wants_to_complete_task_flag = false;
                }
            }
        }

        if self.user_wants_to_delete_task_flag {
            if let Some(id) = self.confirm_delete_task {
                if let Some(name) = self.active_things.iter().find(|x| x.id == id).map(|x| x.name.clone()) {
                    egui::Window::new("Confirm Delete")
                        .collapsible(false)
                        .resizable(false)
                        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                        .show(ctx, |ui| {
                            ui.label(format!("Are you sure you want to delete \"{}\"?", name));
                            ui.horizontal(|ui| {
                                if ui.button("Yes").clicked() {
                                    self.delete_active_thing(id);
                                }
                                if ui.button("No").clicked() {
                                    self.confirm_delete_task = None;
                                    self.user_wants_to_delete_task_flag = false;
                                }
                            });
                        });
                } else {
                    self.confirm_delete_task = None;
                    self.user_wants_to_delete_task_flag = false;
                }
            }
        }

        if self.new_event_flag {
            egui::Window::new("Create new event")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.set_max_size(Vec2::new(300.0, 300.0));
                    ui.vertical(|ui| {
                        ui.add_space(5.0);
                        ui.label("Event Name:");
                        ui.add(egui::TextEdit::singleline(&mut self.event_name_input).hint_text("Attend meeting"));

                        ui.add_space(10.0);

                        ui.label("Date:");
                        self.display_date_entering(ui);

                        ui.add_space(15.0);
                        
                        ui.horizontal(|ui| {
                            if ui.button("Ok").clicked() {
                                // Names are cosmetic now (items are keyed by id),
                                // so duplicates are allowed.
                                match utilities::parse_time_input(self.day_input, self.month_input, self.year_input, self.hour_input, self.minute_input) {
                                    Ok(date) => {
                                        self.add_active_thing(self.event_name_input.clone(), Some(date), None, true, None);
                                        self.new_event_flag = false;
                                    },
                                    _ => {
                                        self.show_error("Problem with date".to_string());
                                    },
                                }
                            }

                            if ui.button("Cancel").clicked() {
                                self.new_event_flag = false;
                            }
                        });

                    });

                });
        }

        if self.new_task_flag {
            egui::Window::new("Create new task")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.vertical(|ui| {
                        //insert fields
                        ui.label("Task Name:");
                        ui.add(egui::TextEdit::singleline(&mut self.task_name_input).hint_text("Complete assignment"));

                        ui.checkbox(&mut self.use_date_for_addable, "Has deadline");

                        if self.use_date_for_addable {
                            ui.label("Importance:");
                            ComboBox::from_id_salt("importance combo")
                                .selected_text(IMPORTANCE[self.task_importance_input as usize])
                                .show_ui(ui, |ui| {
                                    for (i, importance) in IMPORTANCE.iter().enumerate() {
                                        ui.selectable_value(
                                            &mut self.task_importance_input,
                                            i as u8,
                                            importance.to_string(),
                                        );
                                    }
                                });

                            ui.label("Date:");
                            self.display_date_entering(ui);
                        } else {
                            ui.label("Urgency:");
                            ComboBox::from_id_salt("urgency combo")
                                .selected_text(URGENCY[self.time_importance_input as usize])
                                .show_ui(ui, |ui| {
                                    for (i, urgency) in URGENCY.iter().enumerate() {
                                        ui.selectable_value(
                                            &mut self.time_importance_input,
                                            i as u8,
                                            urgency.to_string(),
                                        );
                                    }
                                });
                        }
                        
                        ui.add_space(7.0);
                        
                        ui.horizontal(|ui| {
                            if ui.button("Ok").clicked() {
                                let importance = self.task_importance_input;
                                let date = utilities::parse_time_input(self.day_input, self.month_input, self.year_input, self.hour_input, self.minute_input);

                                // Names are cosmetic now (items are keyed by id),
                                // so duplicates are allowed.
                                if !self.use_date_for_addable {
                                    self.add_active_thing(self.task_name_input.clone(), None, None, false, Some(self.time_importance_input));
                                    self.new_task_flag = false;
                                } else {
                                    match date {
                                        Ok(date) => {
                                            self.add_active_thing(self.task_name_input.clone(), Some(date), Some(importance), false, None);
                                            self.new_task_flag = false;
                                        },
                                        _ => {self.show_error("Problem with date".to_string())},
                                    }
                                }
                            }

                            if ui.button("Cancel").clicked() {
                                self.new_task_flag = false;
                            }
                        });
                    });

                });
        }
        
        // Drawn before the day popup so the confirmation dialogs the planner
        // raises (complete / delete) land on top of it, matching how the popup's
        // own confirmations stack.
        self.show_planner(ctx);

        if self.expand_calendar_day_flag {
            if let Some(index) = self.expanded_day {
                if let Some(day) = self.calendar_elements.get(index) {
                let selected_date = day.date;

                let (weekday_str, formatted_date) = utilities::format_date(selected_date);
                let mut plan_day_requested = false;

                egui::Window::new("calendar_day_popup")
                    .title_bar(false)
                    .collapsible(false)
                    .resizable(false)
                    .default_pos([800.0, 300.0])
                    .show(ctx, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.label(formatted_date);
                            ui.add_space(-9.0);
                            ui.add(Label::new(RichText::new(weekday_str).font(FontId::new(50.0, FontFamily::Name("anton".into())))).selectable(false));
                            ui.add_space(10.0);
                            ui.separator();
                            ui.add_space(2.0);
                            egui::ScrollArea::vertical()
                            .scroll_source(egui::scroll_area::ScrollSource::ALL)
                            .auto_shrink([true, true])
                            .max_height(280.0)
                            .show(ui, |ui| {
                                for item in &day.items {
                                    egui::Frame::new()
                                        .fill(Color32::from_white_alpha(15))
                                        .stroke(egui::Stroke::new(1.5, ui.visuals().text_color()))
                                        .corner_radius(egui::CornerRadius::same(60))
                                        .inner_margin(Margin::symmetric(12, 12))
                                        .show(ui, |ui| {
                                            ui.set_min_size(egui::Vec2 { x: 320.0, y: 25.0 });
                                            ui.set_max_size(egui::Vec2 { x: 320.0, y: 25.0 });
                                            ui.horizontal(|ui| {
                                                let time_font = FontId::new(13.0, FontFamily::Name("space".into()));
                                                let text_font = FontId::new(12.0, FontFamily::Name("spaceb".into()));

                                                ui.label(RichText::new(&item.time).font(time_font));

                                                ui.add(Label::new(RichText::new(item.name.clone()).color(Color32::from_white_alpha(120)).font(text_font)).wrap().selectable(false));

                                                if ui.rect_contains_pointer(ui.max_rect()) {
                                                    if item.is_event {
                                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                            let min_button_size = Vec2::new(24.0, 24.0);

                                                            let delete_button = egui::Button::new("x").min_size(min_button_size).corner_radius(CornerRadius::same(8));
                                                            
                                                            if ui.add(delete_button).clicked() {
                                                                self.user_wants_to_delete_task_flag = true;
                                                                self.confirm_delete_task = Some(item.id);
                                                            }
                                                        });
                                                    } else {
                                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                            let min_button_size = Vec2::new(24.0, 24.0);

                                                            let complete_button = egui::Button::new("✓").min_size(min_button_size).corner_radius(CornerRadius::same(8));
                                                            let delete_button = egui::Button::new("x").min_size(min_button_size).corner_radius(CornerRadius::same(8));
                                                            if ui.add(complete_button).clicked() {
                                                                self.user_wants_to_complete_task_flag = true;
                                                                self.confirm_complete_task = Some(item.id);
                                                            }

                                                            if ui.add(delete_button).clicked() {
                                                                self.user_wants_to_delete_task_flag = true;
                                                                self.confirm_delete_task = Some(item.id);
                                                            }
                                                        });
                                                    }
                                                };
                                            });
                                        });
                                }
                            });
                        });

                            egui::Panel::bottom("bottompanel").show(ui, |ui| {
                                ui.add_space(8.0);
                                        ui.horizontal_centered(|ui| {
                                            ui.add_space(65.0);

                                            if ui.button("Close").clicked() {
                                                self.expand_calendar_day_flag = false;
                                                self.expanded_day = None;
                                            }
                                            ui.add_space(5.0);
                                            // The popup answers "what's on this
                                            // day"; the planner answers "how does
                                            // this day fit together". Hand off
                                            // rather than duplicating the second
                                            // question here.
                                            if ui.button("Plan day").on_hover_text("Lay this day out on a timeline").clicked() {
                                                self.expand_calendar_day_flag = false;
                                                self.expanded_day = None;
                                                // Deferred: `day` borrows
                                                // `self.calendar_elements` for the
                                                // length of this closure, so the
                                                // `&mut self` call happens once
                                                // that borrow has ended.
                                                plan_day_requested = true;
                                            }
                                            ui.add_space(5.0);
                                            if ui.button("Event+").clicked() {
                                                self.day_input = day.day_number as i32;
                                                self.month_input = (day.date.month0() + 1) as i32;
                                                self.year_input = day.date.year_ce().1 as i32;
                                                self.new_event_flag = true;
                                            }
                                            if ui.button("Task+").clicked() {
                                                self.day_input = day.day_number as i32;
                                                self.month_input = (day.date.month0() + 1) as i32;
                                                self.year_input = day.date.year_ce().1 as i32;
                                                self.new_task_flag = true;
                                            }
                                        });
                            });
                    });

                if plan_day_requested {
                    self.open_planner(selected_date);
                }
                } else {
                    // The calendar was rebuilt smaller (e.g. a midnight date
                    // rollover or a reduced week count) while this popup was open,
                    // so the cached `expanded_day` index no longer points at a day.
                    // Close the popup instead of indexing out of bounds.
                    self.expand_calendar_day_flag = false;
                    self.expanded_day = None;
                }
            }
        }

        if self.display_archive_flag {
            egui::Window::new("Archive")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.set_min_size(Vec2::new(500.0, 800.0));
                    
                    egui::Frame::default()
                        .fill(Color32::from_rgba_unmultiplied(40, 44, 52, 240)) // New background color
                        .outer_margin(5)
                        .corner_radius(egui::CornerRadius::same(14))
                        .show(ui, |ui| {
                                egui::ScrollArea::vertical().scroll_source(egui::scroll_area::ScrollSource::ALL).show(ui, |ui| {
                                    egui::Grid::new("archive_grid")
                                        .spacing([0.0, 30.0])
                                        .striped(true)
                                        .show(ui, |ui| {
                                            let header_font = FontId::new(20.0, FontFamily::Monospace);
                                            let label_color = Color32::LIGHT_GRAY;

                                            ui.label("");
                                            ui.label(RichText::new("Created").font(header_font.clone()).color(label_color));
                                            ui.label("");
                                            ui.label(RichText::new("Name").font(header_font.clone()).color(label_color));
                                            ui.label("");
                                            ui.label(RichText::new("Completed").font(header_font).color(label_color));
                                            ui.label("");
                                            ui.end_row();

                                            let date_color = Color32::from_rgb(98, 114, 164); // Soft blue
                                            let name_color = Color32::from_rgba_unmultiplied(255, 255, 255, 180);
                                            let font = FontId::new(18.0, FontFamily::Monospace);
                                            let font_space = FontId::new(15.0, FontFamily::Name("space".into()));

                                            if let Some(ref vec) = self.archive {
                                                for archive in vec {
                                                    ui.label("");
                                                    ui.label(RichText::new(archive.created.format("%d.%m.%Y %H.%M").to_string())
                                                        .font(font_space.clone()).color(date_color));
                                                    ui.label("");
                                                    ui.label(RichText::new(&archive.name)
                                                        .font(font.clone()).color(name_color));
                                                    ui.label("");
                                                    ui.label(RichText::new(archive.inactivated.format("%d.%m.%Y %H.%M").to_string())
                                                        .font(font_space.clone()).color(date_color));
                                                    ui.label("");
                                                    ui.end_row();
                                                }
                                            }
                                        });
                                    ui.vertical_centered_justified(|ui| {
                                        if ui.button("Show more").clicked() {
                                            self.load_more_archives();
                                        }
                                    });

                                });
                        });
                });
        }

        if self.settings_flag && !self.color_picker_flag {
            egui::Window::new("Settings")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .fixed_size(Vec2::new(400.0, 300.0))
                .show(ctx, |ui| {
                    Grid::new("fgd").show(ui, |ui| {
                        ui.horizontal_centered(|ui| {
                            ui.label("Background:");

                            // Keep track of the previously selected index
                            let previous_index = self.selected_background_index;

                            ComboBox::from_id_salt("background_combo")
                                .selected_text(&self.background_options[self.selected_background_index])
                                .show_ui(ui, |ui| {
                                    for (i, background_name) in self.background_options.iter().enumerate() {
                                        ui.selectable_value(
                                            &mut self.selected_background_index,
                                            i,
                                            background_name,
                                        );
                                    }
                                });

                            // Check if the selection changed
                            if previous_index != self.selected_background_index {
                                // Own the name so no borrow of `self` is held across
                                // the `&mut self` persist call.
                                let new_background = self.background_options[self.selected_background_index].clone();

                                self.background_image_texture = Some(set_background(ctx, &self.dirs, new_background.clone()));

                                self.persist_config_value("background", new_background);
                            }

                            if ui.button("♲").clicked() {
                                let available_background_name_to_refresh_into = self.background_options[self.selected_background_index].to_string();
                                self.persist_config_value("background", available_background_name_to_refresh_into.clone());
                                self.background_image_texture = Some(set_background(ctx, &self.dirs, available_background_name_to_refresh_into));
                            }
                        });
                        ui.end_row();
                        ui.end_row();
                        ui.horizontal_centered(|ui| {
                            ui.label("Selected startup monitor:");

                            if self.monitor_options.is_empty() {
                                // winit can report unnamed monitors (name() == None) and
                                // some headless/remote setups report none at all; either
                                // way the list is empty. Show a placeholder instead of
                                // indexing into it (which would panic).
                                ui.add_enabled(false, egui::Label::new("No monitors detected"));
                            } else {
                                // Sync the selected index to the saved name (falling back
                                // to the first monitor if the saved name isn't present).
                                let previous_index = self
                                    .monitor_options
                                    .iter()
                                    .position(|name| name == &self.selected_monitor_name)
                                    .unwrap_or(0);
                                self.selected_monitor_index = previous_index;

                                let selected_text = self
                                    .monitor_options
                                    .get(self.selected_monitor_index)
                                    .cloned()
                                    .unwrap_or_default();

                                ComboBox::from_id_salt("monitor_combo")
                                    .selected_text(selected_text)
                                    .show_ui(ui, |ui| {
                                        for (i, monitor_name) in self.monitor_options.iter().enumerate() {
                                            ui.selectable_value(
                                                &mut self.selected_monitor_index,
                                                i,
                                                monitor_name,
                                            );
                                        }
                                    });

                                if previous_index != self.selected_monitor_index {
                                    self.set_selected_monitor_name();
                                }
                            }

                            // The window is bound to its monitor at startup, so the
                            // monitor choice only takes effect after a restart. Make
                            // that explicit instead of silently saving the setting.
                            if ui.button("♲").on_hover_text("Restart now to move the window to the selected monitor").clicked() {
                                self.restart_self();
                            }
                            ui.label(RichText::new("(applies after restart)").weak());

                        });
                        ui.end_row();
                        ui.end_row();
                        ui.horizontal_centered(|ui| {
                            let previous_selection = self.start_in_fullscreen;

                            ui.checkbox(&mut self.start_in_fullscreen, "Start in fullscreen");

                            if previous_selection != self.start_in_fullscreen {
                                self.persist_config_value("start_in_fullscreen", self.start_in_fullscreen);
                            }
                        });
                        ui.end_row();
                        ui.end_row();
                        ui.horizontal_centered(|ui| {
                            let previous_selection = self.enable_fps_counter;

                            ui.checkbox(&mut self.enable_fps_counter, "Enable fps counter");

                            if previous_selection != self.enable_fps_counter {
                                self.persist_config_value("enable_fps_counter", self.enable_fps_counter);
                            }
                        });
                        ui.end_row();
                        ui.end_row();
                        ui.horizontal_centered(|ui| {
                            let previous_selection = self.three_day_weather;

                            ui.checkbox(&mut self.three_day_weather, "Show weather for three days");

                            if previous_selection != self.three_day_weather {
                                self.persist_config_value("three_day_weather", self.three_day_weather);
                            }
                        });
                        ui.end_row();
                        ui.end_row();
                        ui.horizontal_centered(|ui| {
                            ui.set_max_width(300.0);
                            ui.label("Number of displayed weeks: ");
                            // Commit on Enter / focus loss rather than every keystroke:
                            // applying re-builds the (potentially large) calendar model,
                            // so we don't want to do it per character.
                            if ui.text_edit_singleline(&mut self.week_number_input).lost_focus() {
                                self.set_calendar_weeks();
                            }
                        });
                        ui.end_row();
                        ui.end_row();
                        ui.horizontal_centered(|ui| {
                            ui.set_max_width(300.0);
                            ui.label("Background tint percent: ");
                            if ui.text_edit_singleline(&mut self.background_tint_input).changed() {
                                self.set_background_tint();
                            }
                        });
                        ui.end_row();
                        ui.end_row();
                        ui.horizontal_centered(|ui| {
                            ui.label("UI scale percent: ");
                            ui.scope(|ui| {
                                ui.set_max_width(60.0);
                                // Committed on Enter / focus-loss rather than per
                                // keystroke: every apply re-lays out the whole UI,
                                // and a half-typed "5" would briefly clamp to the
                                // minimum.
                                if ui.text_edit_singleline(&mut self.ui_scale_input).lost_focus() {
                                    self.set_ui_scale();
                                }
                            });
                            if self.ui_scale_percent == UI_SCALE_AUTO {
                                ui.label(
                                    RichText::new(format!(
                                        "(0 = fit to window, now {}%)",
                                        (ctx.zoom_factor() * 100.0).round() as u32
                                    ))
                                    .weak(),
                                );
                            } else {
                                ui.label(
                                    RichText::new(format!(
                                        "({UI_SCALE_MIN}–{UI_SCALE_MAX}, or 0 to fit to window)"
                                    ))
                                    .weak(),
                                );
                            }
                        });
                        ui.end_row();
                        ui.end_row();
                        ui.horizontal_centered(|ui| {
                            ui.label("Weather Coordinates: ");

                            let y_slider = egui::DragValue::new(&mut self.coordinates[0])
                                .prefix("Latitude (Y): ")
                                .range(-90..=90)
                                .fixed_decimals(2)
                                .speed(0.0025);
                            ui.add(y_slider);

                            let x_slider = egui::DragValue::new(&mut self.coordinates[1])
                                .prefix("Longitude (X): ")
                                .range(-180..=180)
                                .fixed_decimals(2)
                                .speed(0.005);
                            ui.add(x_slider);
                        });
                        ui.end_row();
                        ui.end_row();
                        ui.horizontal_centered(|ui| {
                            if ui.button("Pick coordinates with map").clicked() {
                                self.coordinates_map_flag = true;
                            }
                            if ui.button("Apply coordinates").clicked() {
                                self.set_weather_coordinates();
                            }
                        });
                        ui.end_row();
                        ui.end_row();
                        ui.horizontal_centered(|ui| {
                            let button = ui.add(Button::new("Manage colorschemes").min_size(Vec2::new(50.0, 30.0)));

                            if button.clicked() {
                                self.color_picker_flag = true;
                            }
                        });
                        ui.end_row();
                        ui.end_row();
                        ui.vertical_centered(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Timi Salonen").weak());

                                ui.add_space(190.0);

                                let button = ui.add(Button::new("Ok").min_size(Vec2::new(50.0, 30.0)));

                                if button.clicked() {
                                    self.settings_flag = false;
                                }
                            });
                        });
                    });
                });
        }

        if self.coordinates_map_flag {
            egui::Window::new("Weather Coordinates Picker")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .fixed_size(egui::Vec2::new(1500.0, 770.0))
                .show(ctx, |ui| {
                    // -------------------------------------------------
                    // CONSTANTS
                    // -------------------------------------------------
                    let map_size = egui::Vec2::new(1440.0, 712.5);
                    let footer_height = 40.0;

                    // -------------------------------------------------
                    // SPLIT WINDOW INTO MAP + FOOTER RECTS
                    // -------------------------------------------------
                    let available = ui.available_rect_before_wrap();

                    let map_rect = egui::Rect::from_min_size(
                        egui::pos2(
                            available.center().x - map_size.x * 0.5,
                            available.min.y + 20.0,
                        ),
                        map_size,
                    );

                    let footer_rect = egui::Rect::from_min_max(
                        egui::pos2(available.min.x, available.max.y - footer_height),
                        available.max,
                    );

                    // -------------------------------------------------
                    // MAP AREA
                    // -------------------------------------------------
                    let _map_response = ui.scope_builder(egui::UiBuilder::new().max_rect(map_rect), |ui| {
                        let (rect, response) = ui.allocate_exact_size(
                            map_size,
                            egui::Sense::click_and_drag(),
                        );

                        let painter = ui.painter_at(rect);

                        // ---------------- ZOOM ----------------
                        if response.hovered() {
                            let scroll = ui.input(|i| i.smooth_scroll_delta.y) * 2.0;

                            if scroll != 0.0 {
                                let old_zoom = self.map_zoom;
                                let zoom_factor = (scroll * 0.001).exp();
                                self.map_zoom = (self.map_zoom * zoom_factor).clamp(1.0, 15.0);

                                if let Some(cursor) = response.hover_pos() {
                                    let cursor_uv = egui::pos2(
                                        (cursor.x - rect.min.x) / rect.width(),
                                        (cursor.y - rect.min.y) / rect.height(),
                                    );

                                    let old_uv_size = egui::vec2(1.0 / old_zoom, 1.0 / old_zoom);
                                    let new_uv_size = egui::vec2(1.0 / self.map_zoom, 1.0 / self.map_zoom);

                                    self.map_offset += cursor_uv.to_vec2() * (old_uv_size - new_uv_size);
                                }
                            }
                        }

                        // ---------------- PAN ----------------
                        if response.dragged_by(egui::PointerButton::Secondary) {
                            let delta = response.drag_delta();
                            self.map_offset.x -= delta.x / rect.width() / self.map_zoom;
                            self.map_offset.y -= delta.y / rect.height() / self.map_zoom;
                        }

                        // ---------------- UV CLAMP ----------------
                        let uv_size = egui::vec2(1.0 / self.map_zoom, 1.0 / self.map_zoom);

                        self.map_offset.x = self.map_offset.x.clamp(0.0, 1.0 - uv_size.x);
                        self.map_offset.y = self.map_offset.y.clamp(0.0, 1.0 - uv_size.y);

                        let uv_min = egui::pos2(self.map_offset.x, self.map_offset.y);
                        let uv_max = egui::pos2(
                            uv_min.x + uv_size.x,
                            uv_min.y + uv_size.y,
                        );

                        // ---------------- DRAW MAP ----------------
                        if let Some(texture) = &self.map_texture {
                            painter.image(
                                texture.id(),
                                rect,
                                egui::Rect::from_min_max(uv_min, uv_max),
                                egui::Color32::WHITE,
                            );
                        }

                        // ---------------- CLICK TO SET COORDINATES ----------------
                        if response.clicked_by(egui::PointerButton::Primary) {
                            if let Some(pos) = response.interact_pointer_pos() {
                                let local_uv = egui::pos2(
                                    (pos.x - rect.min.x) / rect.width(),
                                    (pos.y - rect.min.y) / rect.height(),
                                );

                                let world_uv = egui::pos2(
                                    uv_min.x + local_uv.x * uv_size.x,
                                    uv_min.y + local_uv.y * uv_size.y,
                                );

                                self.coordinates[1] = world_uv.x * 360.0 - 180.0;
                                self.coordinates[0] = (1.0 - world_uv.y) * 180.0 - 90.0;
                            }
                        }

                        // ---------------- SELECTED MARKER ----------------
                        let world_uv = egui::pos2(
                            (self.coordinates[1] + 180.0) / 360.0,
                            1.0 - ((self.coordinates[0] + 90.0) / 180.0),
                        );

                        let local_uv = egui::pos2(
                            (world_uv.x - uv_min.x) / uv_size.x,
                            (world_uv.y - uv_min.y) / uv_size.y,
                        );

                        if (0.0..=1.0).contains(&local_uv.x) && (0.0..=1.0).contains(&local_uv.y) {
                            let marker_pos = egui::pos2(
                                rect.min.x + local_uv.x * rect.width(),
                                rect.min.y + local_uv.y * rect.height(),
                            );

                            painter.circle_filled(marker_pos, 5.0, egui::Color32::RED);
                            painter.circle_stroke(
                                marker_pos,
                                8.0,
                                egui::Stroke::new(1.5, egui::Color32::WHITE),
                            );
                        }

                        // ---------------- CITY MARKERS ----------------
                        for city in weather::CITIES {
                            let world_uv = egui::pos2(
                                (city.longitude + 180.0) / 360.0,
                                1.0 - ((city.latitude + 90.0) / 180.0),
                            );

                            let local_uv = egui::pos2(
                                (world_uv.x - uv_min.x) / uv_size.x,
                                (world_uv.y - uv_min.y) / uv_size.y,
                            );

                            if (0.0..=1.0).contains(&local_uv.x) && (0.0..=1.0).contains(&local_uv.y) {
                                let pos = egui::pos2(
                                    rect.min.x + local_uv.x * rect.width(),
                                    rect.min.y + local_uv.y * rect.height(),
                                );

                                let city_response = ui.allocate_rect(
                                    egui::Rect::from_center_size(pos, egui::Vec2::splat(10.0)),
                                    egui::Sense::hover(),
                                );

                                painter.circle_filled(pos, 4.0, egui::Color32::DARK_RED);

                                if city_response.hovered() {
                                    painter.text(
                                        pos + egui::vec2(6.0, -6.0),
                                        egui::Align2::LEFT_TOP,
                                        &city.name,
                                        egui::TextStyle::Body.resolve(&ui.style()),
                                        egui::Color32::WHITE,
                                    );
                                }
                            }
                        }
                    });

                    // -------------------------------------------------
                    // FOOTER
                    // -------------------------------------------------
                    ui.scope_builder(egui::UiBuilder::new().max_rect(footer_rect), |ui| {
                        ui.horizontal_centered(|ui| {
                            ui.add_space(20.0);
                            ui.label(format!("Lat: {:.2}", self.coordinates[0]));
                            ui.separator();
                            ui.label(format!("Lon: {:.2}", self.coordinates[1]));
                            ui.separator();
                            ui.label(format!("Zoom: {:.2}x", self.map_zoom));

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.add_space(20.0);
                                    if ui.button("OK").clicked() {
                                        self.coordinates_map_flag = false;
                                    }
                                },
                            );
                        });
                    });
                });
        }

        if self.color_picker_flag && !self.edit_colorscheme_flag {
            egui::Window::new("Colorscheme manager")
                .resizable(false)
                .default_pos(pos2(580.0, 250.0))
                .fixed_size(vec2(800.0, 800.0))
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {

                        ui.group(|ui| {
                            ui.set_max_size(vec2(310.0, 620.0));
                            ui.vertical(|ui| {
                                ui.heading("Schemes");

                                let previous_id = self.selected_colorscheme_id;

                                egui::ScrollArea::vertical()
                                    .scroll_source(egui::scroll_area::ScrollSource::ALL)
                                    .max_height(300.0)
                                    .max_width(300.0)
                                    .id_salt("scroll_area_1")
                                    .show(ui, |ui| {
                                        for (id, scheme) in &self.colorschemes {
                                            if scheme.is_user_configurable {
                                                ui.selectable_value(
                                                    &mut self.selected_colorscheme_id,
                                                    *id,
                                                    &scheme.name,
                                                );
                                            }
                                        }
                                    });

                                ui.add_space(5.0);
                                ui.separator();
                                ui.add_space(5.0);

                                egui::ScrollArea::vertical()
                                    .scroll_source(egui::scroll_area::ScrollSource::ALL)
                                    .max_height(300.0)
                                    .max_width(300.0)
                                    .id_salt("scroll_area_2")
                                    .show(ui, |ui| {
                                        for (id, scheme) in &self.colorschemes {
                                            if !scheme.is_user_configurable {
                                                ui.selectable_value(
                                                    &mut self.selected_colorscheme_id,
                                                    *id,
                                                    &scheme.name,
                                                );
                                            }
                                        }
                                    });

                                if previous_id != self.selected_colorscheme_id {
                                    self.set_colorscheme();
                                }
                            });
                        });

                        ui.vertical(|ui| {
                            let duplicate_button = ui.add(Button::new("Duplicate colorscheme").min_size(Vec2::new(50.0, 30.0)));
                            if self.currently_selected_colorscheme_is_user_configurable() {
                                let edit_button = ui.add(Button::new("Edit colorscheme").min_size(Vec2::new(50.0, 30.0)));
                                let rename_button = ui.add(Button::new("Rename colorscheme").min_size(Vec2::new(50.0, 30.0)));
                                let delete_button = ui.add(Button::new("Delete colorscheme").min_size(Vec2::new(50.0, 30.0)));

                                if rename_button.clicked() {
                                    self.rename_colorscheme_flag = true;
                                }

                                if delete_button.clicked() {
                                    self.user_wants_to_delete_colorscheme_flag = true;
                                }

                                if edit_button.clicked() {
                                    self.edit_colorscheme_flag = true;
                                    self.colorscheme_being_edited = Some(self.colorschemes.get(&self.selected_colorscheme_id).unwrap_or(&ColorScheme::default_scheme()).clone());
                                }
                            }

                            ui.add_space(5.0);
                            ui.separator();
                            ui.add_space(5.0);

                            let generate_button = ui.add(Button::new("Generate new colorscheme from current background").min_size(Vec2::new(50.0, 30.0)));
                            if generate_button.clicked() {
                                self.try_to_generate_colorscheme();
                            }

                            ui.add_space(5.0);
                            ui.separator();
                            ui.add_space(5.0);

                            let ok_button = ui.add(Button::new("OK").min_size(Vec2::new(50.0, 30.0)));


                            if duplicate_button.clicked() {
                                self.duplicate_current_colorscheme();
                            }

                            if ok_button.clicked() {
                                self.color_picker_flag = false;
                            }
                        });
                    });
                });
        }

        if self.edit_colorscheme_flag && !self.rename_colorscheme_flag && !self.user_wants_to_delete_colorscheme_flag {
            let mut should_save = false;
            let mut should_cancel = false;
            
            if let Some(scheme) = &mut self.colorscheme_being_edited {
                egui::Window::new("Editing colorscheme:")
                    .collapsible(true)
                    .resizable(false)
                    .fixed_size(vec2(270.0, 800.0))
                    .default_pos(pos2(580.0, 250.0))
                    .show(ctx, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.add(Label::new(RichText::new(scheme.name.clone()).color(Color32::from_white_alpha(120))).wrap().selectable(false));

                            ui.add_space(5.0);

                            ui.horizontal(|ui| {
                                ui.add_space(5.0);

                                let color_size = Vec2::new(36.0, 36.0);
                                let spacing = 8.0;

                                let mut swap_with: Option<usize> = None;

                                for i in 0..scheme.colors.len() {
                                    let (rect, response) = ui.allocate_exact_size(
                                        color_size,
                                        egui::Sense::click_and_drag(),
                                    );

                                    // Start dragging
                                    if response.drag_started() {
                                        self.dragged_color_index = Some(i);
                                    }

                                    // Handle hover-based swapping
                                    if let Some(dragged) = self.dragged_color_index {
                                        if dragged != i && response.hovered() {
                                            swap_with = Some(i);
                                        }
                                    }

                                    // Draw background frame
                                    let visuals = ui.style().interact(&response);
                                    ui.painter().rect_filled(
                                        rect.expand(2.0),
                                        4.0,
                                        visuals.bg_fill,
                                    );

                                    // Draw color button
                                    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                                        ui.color_edit_button_srgba_unmultiplied(&mut scheme.colors[i]);
                                    });

                                    ui.add_space(spacing);
                                }

                                // Perform swap AFTER rendering
                                if let (Some(from), Some(to)) = (self.dragged_color_index, swap_with) {
                                    scheme.colors.swap(from, to);
                                    self.dragged_color_index = Some(to);
                                }

                                // Clear drag state
                                if ui.input(|i| i.pointer.any_released()) {
                                    self.dragged_color_index = None;
                                }

                                ui.add_space(5.0);
                            });

                            ui.add_space(30.0);

                            ui.horizontal(|ui| {
                                ui.add_space(10.0);
                                let button = ui.add(Button::new("Save").min_size(Vec2::new(50.0, 30.0)));
                                if button.clicked() {
                                    should_save = true;
                                }

                                ui.add_space(40.0);
                                ui.label(RichText::new("drag to reorder").weak().small());

                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.add_space(10.0);
                                    let button = ui.add(Button::new("Cancel").min_size(Vec2::new(50.0, 30.0)));
                                    if button.clicked() {
                                        should_cancel = true;
                                    }
                                });
                            });
                        });
                    });

                    self.active_colorscheme = scheme.colors.map(|c| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]));
            }
            
            if should_save {
                self.save_colorscheme_edits();
                self.set_colorscheme();
                self.edit_colorscheme_flag = false;
            }
            if should_cancel {
                self.set_colorscheme();
                self.edit_colorscheme_flag = false;
            }
        }

        if self.rename_colorscheme_flag && !self.user_wants_to_delete_colorscheme_flag && self.currently_selected_colorscheme_is_user_configurable() {
            egui::Window::new("Rename selected colorscheme")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .min_size(Vec2::new(400.0, 400.0))
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {

                        ui.text_edit_singleline(&mut self.colorscheme_rename_input);

                        ui.add_space(5.0);

                        ui.horizontal(|ui| {
                            ui.add_space(10.0);
                            let button = ui.add(Button::new("Cancel").min_size(Vec2::new(50.0, 30.0)));
                            if button.clicked() {
                                self.rename_colorscheme_flag = false;
                            }

                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.add_space(10.0);
                                let button = ui.add(Button::new("Save").min_size(Vec2::new(50.0, 30.0)));
                                if button.clicked() {
                                    self.rename_current_colorscheme();
                                    self.rename_colorscheme_flag = false;
                                }
                            });
                        });
                    });
                });
        }

        if self.user_wants_to_delete_colorscheme_flag && self.currently_selected_colorscheme_is_user_configurable() && !self.rename_colorscheme_flag {
            egui::Window::new("Confirm Delete Scheme")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(format!("Are you sure you want to delete \"{}\"?", self.colorschemes.get(&self.selected_colorscheme_id).unwrap_or(&ColorScheme::default_scheme()).name));
                    ui.horizontal(|ui| {
                        if ui.button("Yes").clicked() {
                            self.delete_current_colorscheme();
                        }
                        if ui.button("No").clicked() {
                            self.user_wants_to_delete_colorscheme_flag = false;
                        }
                    });
                });
        }

        //this should be displayed last such that the error window is always on top
        if self.error_flag {
            egui::Window::new("error window")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .min_size(Vec2::new(400.0, 400.0))
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(5.0);
                        ui.colored_label(Color32::from_white_alpha(180), &self.error_text);

                        ui.add_space(15.0);

                        let button = ui.add(Button::new("Ok").min_size(Vec2::new(50.0, 30.0)));

                        if button.clicked() {
                            self.error_flag = false;
                        }
                    });
                });
        }

    if self.any_modal_open() {
        self.hovered_calendar_cell = None;
    }
    }
}

/// The pixel grid Fixedsys Excelsior is drawn on. Its outlines trace a bitmap
/// font's pixels, so a glyph is sharp when its em box lands on a whole number of
/// pixels — and sharpest at a multiple of this — and smeared across two pixels
/// when it doesn't. Rendering the same string at 16.00px and 17.19px side by
/// side makes the difference obvious.
const FIXEDSYS_GRID_PX: f32 = 16.0;
/// How far a requested size may be nudged to land on the grid. Beyond this the
/// size change would be more noticeable than the blur it fixes, so the size
/// settles for the nearest whole pixel instead.
const FIXEDSYS_GRID_TOLERANCE: f32 = 0.09;

/// Adjust a point size so Fixedsys renders on pixel boundaries at the current
/// scale.
///
/// Returns a *point* size, because that is what egui takes; the value is chosen
/// so `size × pixels_per_point` is a whole number of pixels, and a multiple of
/// [`FIXEDSYS_GRID_PX`] when one is close enough to be worth taking.
fn snap_font_points(points: f32, pixels_per_point: f32) -> f32 {
    if pixels_per_point <= 0.0 || points <= 0.0 {
        return points;
    }

    let requested_px = points * pixels_per_point;
    let nearest_grid_px = (requested_px / FIXEDSYS_GRID_PX).round().max(1.0) * FIXEDSYS_GRID_PX;

    let snapped_px = if (nearest_grid_px - requested_px).abs() / requested_px <= FIXEDSYS_GRID_TOLERANCE {
        nearest_grid_px
    } else {
        requested_px.round().max(1.0)
    };

    snapped_px / pixels_per_point
}

pub fn set_styles(ctx: &egui::Context) {
    let pixels_per_point = ctx.pixels_per_point();
    let font = |points: f32| {
        egui::FontId::new(snap_font_points(points, pixels_per_point), egui::FontFamily::Monospace)
    };

    let mut style = (*ctx.global_style()).clone();
    style.text_styles = [
        (egui::TextStyle::Heading, font(30.0)),
        (egui::TextStyle::Body, font(18.0)),
        (egui::TextStyle::Button, font(22.0)),
        (egui::TextStyle::Small, font(11.0)),
        (egui::TextStyle::Monospace, font(11.0)),
    ]
    .into();
    ctx.set_global_style(style);
}

pub fn load_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert(
        "fixedsys".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(r#"../fonts/FSEX300.ttf"#))),
    );
    fonts.font_data.insert(
        "dejavu".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(r#"../fonts/DejaVuSans.ttf"#))),
    );
        fonts.font_data.insert(
        "anton".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(r#"../fonts/Anton-Regular.ttf"#))),
    );
        fonts.font_data.insert(
        "space".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(r#"../fonts/SpaceMono-Regular.ttf"#))),
    );
        fonts.font_data.insert(
        "spaceb".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(r#"../fonts/LexendGiga-Light.ttf"#))),
    );
        fonts.font_data.insert(
        "bungee".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(r#"../fonts/FacultyGlyphic-Regular.ttf"#))),
    );    

    fonts.families.get_mut(&egui::FontFamily::Monospace).unwrap().clear();

    fonts
        .families
        .get_mut(&egui::FontFamily::Monospace)
        .unwrap()
        .push("fixedsys".to_owned());
    fonts
        .families
        .get_mut(&egui::FontFamily::Monospace)
        .unwrap()
        .push("dejavu".to_owned());
    fonts
        .families
        .get_mut(&egui::FontFamily::Monospace)
        .unwrap()
        .push("space".to_owned());
    fonts
        .families
        .get_mut(&egui::FontFamily::Proportional)
        .unwrap()
        .push("spaceb".to_owned());

    fonts.families.insert(FontFamily::Name("anton".into()), vec!["anton".to_owned()]);

    fonts.families.insert(FontFamily::Name("dejavu".into()), vec!["dejavu".to_owned()]);

    fonts.families.insert(FontFamily::Name("space".into()), vec!["space".to_owned()]);

    fonts.families.insert(FontFamily::Name("spaceb".into()), vec!["spaceb".to_owned()]);

    fonts.families.insert(FontFamily::Name("bungee".into()), vec!["bungee".to_owned()]);

    ctx.set_fonts(fonts);
}

fn attempt_background(path: PathBuf) -> Result<ImageBuffer<Rgba<u8>, Vec<u8>>, Box<dyn Error>> {
    let image_bytes = fs::read(&path)?;
    let image = image::load_from_memory(&image_bytes)?
        .to_rgba8();

    Ok(image)
}

fn set_background(ctx: &Context, dirs: &AppDirs, name: String) -> TextureHandle {
    // Fall back to the bundled placeholder if the name is unusable or the file
    // can't be loaded. `image_path` keeps this confined to `images/`.
    let image = dirs.image_path(&name)
        .and_then(|path| attempt_background(path).ok())
        .unwrap_or_else(|| {
            image::load_from_memory(include_bytes!("../noback.png"))
                .expect("Did not get access to fallback background")
                .to_rgba8()
        });

    // `Context::load_texture` *panics* on an image wider or taller than the GPU's
    // limit, so an oversized picture dropped into `images/` would take the whole
    // app down. Shrink it to fit instead — this is a full-window backdrop, so
    // anything past the limit is detail nobody can see anyway. The limit comes
    // from the adapter (commonly 8192 or 16384), so this only bites on genuinely
    // huge photographs.
    let max_side = ctx.input(|i| i.max_texture_side) as u32;
    let longest_side = image.width().max(image.height());
    let image = if longest_side > max_side {
        // Scale both axes by the same factor so the picture isn't stretched.
        let scale = max_side as f32 / longest_side as f32;
        image::imageops::resize(
            &image,
            ((image.width() as f32 * scale) as u32).max(1),
            ((image.height() as f32 * scale) as u32).max(1),
            image::imageops::FilterType::Triangle,
        )
    } else {
        image
    };

    let size = [image.width() as usize, image.height() as usize];

    let texture = ColorImage::from_rgba_unmultiplied(size, image.as_flat_samples().as_slice());

    ctx.load_texture("background", texture, Default::default())
}

fn set_world_map(ctx: &Context) -> TextureHandle {
    let bytes = image::load_from_memory(include_bytes!("../1920px-Blue_Marble_2002.png")).expect("Did not get access to fallback background").to_rgba8();

    let size = [bytes.width() as usize, bytes.height() as usize];

    let texture = ColorImage::from_rgba_unmultiplied(size, &bytes.as_flat_samples().as_slice());

    ctx.load_texture("world_map", texture, Default::default())
}

#[cfg(test)]
mod tests {
    use super::{snap_font_points, FIXEDSYS_GRID_PX};

    /// The whole point of the snap: whatever comes back must land on a whole
    /// number of pixels at the scale it was snapped for.
    fn assert_whole_pixels(points: f32, ppp: f32) {
        let px = snap_font_points(points, ppp) * ppp;
        assert!(
            (px - px.round()).abs() < 0.01,
            "{points}pt at ppp {ppp} gave {px}px, which is not a whole pixel"
        );
    }

    #[test]
    fn snaps_to_whole_pixels_at_any_scale() {
        for ppp in [1.0, 1.25, 1.5, 1.5625, 1.75, 2.0] {
            for points in [8.0, 11.0, 12.0, 14.0, 18.0, 22.0, 30.0] {
                assert_whole_pixels(points, ppp);
            }
        }
    }

    #[test]
    fn prefers_the_16px_grid_when_it_is_close() {
        // 11pt at 1.5625 is 17.19px — within tolerance of 16, the size Fixedsys
        // is actually drawn at, so it takes it.
        let px = snap_font_points(11.0, 1.5625) * 1.5625;
        assert!((px - FIXEDSYS_GRID_PX).abs() < 0.01, "got {px}px");

        // 30pt at 1x is 30px, near enough to 32 to be worth taking.
        assert!((snap_font_points(30.0, 1.0) - 32.0).abs() < 0.01);
    }

    #[test]
    fn leaves_sizes_alone_when_the_grid_is_far_away() {
        // 18pt at 1.5625 is 28.12px; the nearest grid multiple is 32, a 14%
        // jump — too big a change to make for sharpness, so it settles for the
        // nearest whole pixel instead.
        let px = snap_font_points(18.0, 1.5625) * 1.5625;
        assert!((px - 28.0).abs() < 0.01, "got {px}px");

        // At 1x the common sizes are already whole pixels and stay put.
        assert_eq!(snap_font_points(18.0, 1.0), 18.0);
        assert_eq!(snap_font_points(11.0, 1.0), 11.0);
    }

    #[test]
    fn degenerate_inputs_are_returned_unchanged() {
        assert_eq!(snap_font_points(12.0, 0.0), 12.0);
        assert_eq!(snap_font_points(0.0, 2.0), 0.0);
    }
}
