use std::{collections::HashMap, error::Error, fs, path::PathBuf, process::{Command, exit}, sync::{Arc, atomic::Ordering}, time::Instant};

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, Timelike, Weekday};
use egui::{self, Align, Button, Color32, ColorImage, ComboBox, Context, CornerRadius, Event, FontData, FontDefinitions, FontFamily, FontId, Grid, Key, Label, Layout, Margin, PointerButton, Pos2, Rect, RichText, Stroke, StrokeKind, TextureHandle, Ui, Vec2, ViewportCommand, pos2, vec2};
use image::{ImageBuffer, Rgba};
use toml_edit::{DocumentMut};

use crate::{calendarwidgets, color::{self, ColorScheme}, utilities::{self, next_three_weekdays, resolve_colorscheme}, tasks::{self, Active, InActive}, weather::{self, WeatherService}};

const WEEK_DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

const URGENCY: [&str; 3] = ["Time-independence", "Normal urgency", "High urgency"];

const IMPORTANCE: [&str; 5] = ["Not important", "Mildly important", "Important", "Highly important", "Lethally important"];

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
    pub exe_file_path: PathBuf,
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
    pub weather_service: WeatherService,
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
    exe_file_path: PathBuf,

    hovered_calendar_cell: Option<usize>,
    expanded_day: Option<usize>,
    offset: usize,
    press_origin: Option<PressState>,

    userconfig_path: PathBuf,

    /* ───────────────────────── Time & Date ───────────────────────── */
    date: DateTime<Local>,
    next_three_weekdays: (String, String, String),
    chrono_tick_counter: u16,

    /* ───────────────────────── Tasks & Events ───────────────────────── */
    active_things: Vec<Active>,
    list_tasks: Vec<Active>,
    archive: Option<Vec<InActive>>,

    calendar_elements: Vec<(
        u8,
        Vec<(String, String, usize)>,
        Vec<(String, String, bool)>,
        bool,
        NaiveDate,
        String,
    )>,

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

    /* ───────────────────────── Settings ───────────────────────── */
    start_in_fullscreen: bool,
    enable_fps_counter: bool,
    calendar_weeks_to_show: usize,

    selected_background_index: usize,
    background_options: Vec<String>,
    background_image_tint_percent: u32,
    background_tint_input: String,

    /* ───────────────────────── Errors & Confirmations ───────────────────────── */
    confirm_complete_task: Option<String>,
    confirm_delete_task: Option<String>,
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
    latitude: f32,
    longitude: f32,
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
            exe_file_path: config.exe_file_path,
            hovered_calendar_cell: None,
            expanded_day: None,
            offset: 0,
            press_origin: None,
            userconfig_path: PathBuf::from("taskdeck_data").join(PathBuf::from("userconfig.toml")),

            /* Time */
            date: now,
            next_three_weekdays: next_three_weekdays(now),
            chrono_tick_counter: 0,

            /* Tasks */
            list_tasks: config
                .active_items
                .iter()
                .filter(|t| !t.is_event)
                .cloned()
                .collect(),
            active_things: config.active_items,
            archive: None,
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
            error_flag: false,
            display_archive_flag: false,
            expand_calendar_day_flag: false,
            settings_flag: false,
            user_wants_to_complete_task_flag: false,
            user_wants_to_delete_task_flag: false,
            should_save_textbox_text: false,

            /* Settings */
            start_in_fullscreen: config.start_in_fullscreen,
            enable_fps_counter: config.enable_fps_counter,
            calendar_weeks_to_show: config.calendar_weeks_to_show,

            selected_background_index,
            background_options: config.background_options,
            background_image_tint_percent: config.background_image_tint_percent,
            background_tint_input: config.background_image_tint_percent.to_string(),

            /* Errors */
            confirm_complete_task: None,
            confirm_delete_task: None,
            error_text: String::new(),

            /* FPS / Monitor */
            fps_counter: FpsCounter::new(),
            selected_monitor_name: config.selected_monitor_name,
            monitor_options: Vec::new(),
            selected_monitor_index: 0,

            /* Map */
            coordinates_map_flag: false,
            map_zoom: 1.0,
            map_offset: Vec2::ZERO,
            latitude: config.coordinates[0],
            longitude: config.coordinates[1],
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
                                            self.confirm_complete_task = Some(task.name.clone());
                                        }

                                        if ui.add(delete_button).clicked() {
                                            self.user_wants_to_delete_task_flag = true;
                                            self.confirm_delete_task = Some(task.name.clone());
                                        }
                                    });
                                };
                            });
                        });
                }
            });
        });
    }

    fn display_stuff(&self, thing: &Vec<(String, f64, i32, bool)>, ui: &mut Ui, grid_id: String) {
        egui::Grid::new(grid_id)
            .spacing(Vec2::new(10.0, 10.0))
            .min_col_width(80.0)
            .max_col_width(80.0)
            .show(ui, |ui| {
                for (i, (time, temp, wmo_code, is_day)) in thing.iter().enumerate() {
                    egui::Frame::default()
                        .stroke(Stroke::new(0.5, Color32::from_white_alpha(150)))
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

                                    ui.label(RichText::new(format!("{temp:.0}")).color(Color32::from_white_alpha(155)));
                                });

                                ui.add_space(-5.0);

                                let time_text = RichText::new(time)
                                    .color(Color32::from_white_alpha(120))
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
        if self.weather_is_broken_flag {
            ui.label("WEATHER IS BROKEN");
        } else {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.add_space(147.0);
                    ui.label(RichText::new(&self.next_three_weekdays.0).size(14.0).color(Color32::from_white_alpha(165)));
                });
                ui.add_space(75.0);

                let day_1 = &self.weather_data_cache[0];
                self.display_stuff(day_1, ui, "firstweathergrid".to_string());

                ui.add_space(5.0);

                ui.horizontal(|ui| {
                    ui.add_space(150.0);
                    ui.label(RichText::new(&self.next_three_weekdays.1).size(14.0).color(Color32::from_white_alpha(165)));
                });
                ui.add_space(75.0);

                let day_2 = &self.weather_data_cache[1];
                self.display_stuff(day_2, ui, "secondweathergrid".to_string());

                if self.three_day_weather {
                    ui.add_space(5.0);

                    ui.horizontal(|ui| {
                        ui.add_space(150.0);
                        ui.label(RichText::new(&self.next_three_weekdays.2).size(14.0).color(Color32::from_white_alpha(165)));
                    });
                    ui.add_space(75.0);

                    let day_3 = &self.weather_data_cache[2];
                    self.display_stuff(day_3, ui, "thirdweathergrid".to_string());
                } else {
                    ui.add_space(15.0);
                    ui.horizontal(|ui| {
                        ui.add_space(5.0);

                        if ui.add(egui::TextEdit::multiline(&mut self.textbox_text)
                            .background_color(Color32::from_black_alpha(40))
                        ).changed() {
                            self.should_save_textbox_text = true;
                        }
                    });
                }
            });
        }
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
                    let day_current = self.calendar_elements.iter().position(|x| x.3);

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
                            let mut row_ui = ui.child_ui_with_id_source(
                                scaled_row_rect,
                                Layout::left_to_right(Align::Center),
                                row,
                                None,
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

                                row_ui.allocate_ui_at_rect(inner_rect, |ui| {
                                    ui.set_min_size(inner_rect.size());
                                    let (_, widget_items, full_list, is_strong, _, day_label) = &mut self.calendar_elements[idx];
                                    ui.vertical(|ui| {
                                        let num = full_list.len();
                                        if num == 0 {
                                            ui.add(calendarwidgets::DayNumber::new(day_label, *is_strong));
                                            ui.with_layout(Layout::bottom_up(Align::RIGHT), |ui| {
                                                ui.add(calendarwidgets::RotatedNumberOnly::new(day_label, *is_strong));
                                            });
                                        } else if num == 1 {
                                            let first = &widget_items[0];
                                            ui.add(calendarwidgets::DayHeader::new(day_label, &first.0, *is_strong, &first.1, self.active_colorscheme[first.2]));
                                            ui.with_layout(Layout::bottom_up(Align::RIGHT), |ui| {
                                                ui.add(calendarwidgets::RotatedNumberOnly::new(day_label, *is_strong));
                                            });
                                        } else if num == 2 {
                                            let first = &widget_items[0];
                                            ui.add(calendarwidgets::DayHeader::new(day_label, &first.0, *is_strong, &first.1, self.active_colorscheme[first.2]));
                                            let second = &widget_items[1];
                                            ui.add(calendarwidgets::MiddleHeader::new(&second.0, Some(&second.1), self.active_colorscheme[second.2]));
                                            ui.with_layout(Layout::bottom_up(Align::RIGHT), |ui| {
                                                ui.add(calendarwidgets::RotatedNumberOnly::new(day_label, *is_strong));
                                            });
                                        } else if num == 3 {
                                            let first = &widget_items[0];
                                            ui.add(calendarwidgets::DayHeader::new(day_label, &first.0, *is_strong, &first.1, self.active_colorscheme[first.2]));
                                            let second = &widget_items[1];
                                            ui.add(calendarwidgets::MiddleHeader::new(&second.0, None, self.active_colorscheme[second.2]));
                                            let third = &widget_items[2];
                                            ui.add(calendarwidgets::BottomHeaderRotated::new(day_label, &third.0, *is_strong, &third.1, Some(&second.1), self.active_colorscheme[third.2]));
                                        } else {
                                            let first = &widget_items[0];
                                            ui.add(calendarwidgets::DayHeader::new(day_label, &first.0, *is_strong, &first.1, self.active_colorscheme[first.2]));
                                            let second = &widget_items[1];
                                            ui.add(calendarwidgets::MiddleHeader::new(&second.0, None, self.active_colorscheme[second.2]));
                                            let third = &widget_items[2];
                                            ui.add(calendarwidgets::ButtonHeaderRotated::new(day_label, &third.0, *is_strong, &third.1, Some(&second.1), self.active_colorscheme[third.2]));
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

                        if !(self.expand_calendar_day_flag | self.display_archive_flag | self.error_flag | self.new_task_flag | self.user_wants_to_delete_task_flag | self.user_wants_to_complete_task_flag | self.new_event_flag | self.settings_flag) {
                            let events = ui.ctx().input(|i| i.events.clone());
                            for ev in events {
                                match ev {
                                    Event::PointerButton { pos, button, pressed, .. } => {
                                        if button == PointerButton::Primary {
                                            if pressed {
                                                if let Some((idx, _)) = visible_cells.iter().find(|(_, r)| r.contains(pos)) {
                                                    self.press_origin = Some(PressState {
                                                        idx: *idx,
                                                        press_pos: pos,
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
                                                    let release_idx_opt = visible_cells.iter().find(|(_, r)| r.contains(pos)).map(|(i, _)| *i);

                                                    let dist = press.press_pos.distance(pos);
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
                                        let dist = press.press_pos.distance(pos);
                                        if dist > DRAG_THRESHOLD_POINTS {
                                            press.cancelled = true;
                                        }
                                    },
                                    Event::MouseWheel { unit, delta, modifiers } => {
                                        if let Some(press) = &mut self.press_origin {
                                            press.cancelled = true;
                                        }
                                    },
                                    _ => (),
                                }
                            }
                        }
                        ui.add_space(2.0);
                    });
                });
        });
    }

    fn add_active_thing(&mut self, name: String, deadline: Option<DateTime<Local>>, importance: Option<u8>, is_event: bool, time_importance: Option<u8>) {
        self.active_things.push(Active {
            name,
            deadline,
            importance,
            time_importance,
            is_event,
            created: chrono::Local::now(),
        });
        self.summarize_calendar();
        if let Err(text) = tasks::oversafe_activesave(&self.active_things, &self.exe_file_path) {
            self.show_error(format!("Saving error:\n{}", text.to_string()));
        }
    }

    fn delete_active_thing(&mut self, name: &str) {
        self.user_wants_to_delete_task_flag = false;
        self.active_things = self.active_things.iter().filter(|task| task.name != name).cloned().collect();
        self.confirm_delete_task = None;
        self.summarize_calendar();
        
        if let Err(text) = tasks::oversafe_activesave(&self.active_things, &self.exe_file_path) {
            self.show_error(format!("Saving error:\n{}", text.to_string()));
        };
    }

    fn name_is_unique(&self, input_name: &str) -> bool {
        !self.active_things.iter().any(|x| x.name == input_name)
    }

    pub fn summarize_calendar(&mut self) {
        // 1) Sort and separate active things
        let (mut events, mut tasks): (Vec<_>, Vec<_>) = self.active_things
            .drain(..)
            .partition(|a| a.is_event);

        events.sort_by_key(|e| e.deadline.expect("Event without a deadline"));
        tasks.sort_by_key(|t| std::cmp::Reverse(t.importance_score(self.date) as u16));

        let deadline_tasks: Vec<Active> = tasks.iter().filter(|task| task.deadline.is_some()).cloned().collect();

        // 2) Rebuild active_things sorted (if you need to keep the order)
        self.active_things.clear();
        self.active_things.extend(events.clone());
        self.active_things.extend(tasks);

        // 3) Determine the starting Monday
        let today = self.date;
        let monday = today
            .date_naive()
            .week(Weekday::Mon)
            .first_day();

        let mut calendar = Vec::new();

        let mut last_days_vec: Vec<Option<(String, String)>> = vec![];

        // 4) Iterate n weeks x 7 days
        for week in 0..self.calendar_weeks_to_show {
            let mut contains_first_day_of_month = None;
            for day in 0..7 {
                let current = monday + Duration::days((week * 7 + day) as i64);
                
                if current.day() == 1 {
                    let prev_month = current - Duration::days(2);
                    contains_first_day_of_month = Some((prev_month.month().to_string(), current.month().to_string()));
                }

                let is_current_day: bool = current == self.date.date_naive();

                // Filter items for this date
                let day_events: Vec<_> = events
                    .iter()
                    .filter(|e| e.deadline.unwrap().date_naive() == current)
                    .cloned()
                    .collect();

                let day_tasks: Vec<_> = deadline_tasks
                    .iter()
                    .filter(|t| t.deadline.unwrap().date_naive() == current)
                    .cloned()
                    .collect();

                // 5) Pick up to 3: events first, then tasks
                let mut chosen = Vec::new();
                for e in day_events.iter().take(3) {
                    chosen.push(e.clone());
                }
                if chosen.len() < 3 {
                    for t in day_tasks.iter().take(3 - chosen.len()) {
                        chosen.push(t.clone());
                    }
                }

                // 6) Sort chosen by exact deadline time
                chosen.sort_by_key(|a| a.deadline.unwrap());

                let chosen_str: Vec<(String, String, usize)> = chosen
                    .into_iter()
                    .map(|a| {
                        let time = a.deadline
                            .unwrap()
                            .format("%H:%M")
                            .to_string();
                        let color_id = a.calendar_item_color();
                        
                        (a.name, time, color_id)
                    })
                    .collect();

                // 7) Build complete list for the day, sorted by deadline
                let mut all_for_day = Vec::new();
                all_for_day.extend(day_events);
                all_for_day.extend(day_tasks);
                all_for_day.sort_by_key(|a| a.deadline.unwrap());

                let all_str: Vec<(String, String, bool)> = all_for_day
                    .into_iter()
                    .map(|a| {
                        let time = a.deadline
                            .unwrap()
                            .format("%H:%M")
                            .to_string();
                        (a.name, time, a.is_event)
                    })
                    .collect();

                let day_label = current.day().to_string();
                calendar.push((current.day() as u8, chosen_str, all_str, is_current_day, current, day_label));
            }
            last_days_vec.push(contains_first_day_of_month);
        }

        self.row_contains_month_switch = last_days_vec;

        self.calendar_elements = calendar;
        self.refilter_tasks();
    }

    fn show_error(&mut self, errortext: String) {
        self.error_flag = true;
        self.error_text = errortext;
    }

    fn complete_active_thing(&mut self, name: &str) {
        if let Some(thing) = self.active_things.iter().find(|x| x.name == name) {
            let found_inactive: InActive = thing.clone().to_inactive();

            if let Err(text) = tasks::save_inactive(&found_inactive, &self.exe_file_path) {
                self.show_error(format!("Error archiving:\n{}", text.to_string()));
            };

            self.delete_active_thing(name);

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
        let new_items = tasks::read_lines_range(self.offset, 15, &self.exe_file_path).unwrap_or_else(|_| Vec::new());
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
                    ComboBox::from_id_source("day")
                        .width(40.0)
                        .selected_text(RichText::from(format!("{:02}", self.day_input)).font(space_font.clone()))
                        .show_ui(ui, |ui| {
                            for day in 1..=31 {
                                ui.selectable_value(
                                    &mut self.day_input,
                                    day,
                                    RichText::from(format!("{:02}", day)).font(space_font.clone()),
                                );
                            }
                        });
                    ui.label(RichText::new(".").font(space_font.clone()));

                    ComboBox::from_id_source("month")
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

                    ComboBox::from_id_source("year")
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

                    ComboBox::from_id_source("hour")
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

                    ComboBox::from_id_source("minute")
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

    fn update_background_config(&self, new_background: &str) -> Result<(), Box<dyn std::error::Error>> {
        // Read the existing file
        let toml_content = fs::read_to_string(&self.userconfig_path)?;

        // Parse the TOML content
        let mut doc = toml_content.parse::<DocumentMut>()?;

        // Insert or update the background key
        doc["background"] = toml_edit::value(new_background);

        // Write the updated content back to the file
        fs::write(&self.userconfig_path, doc.to_string())?;

        Ok(())
    }

    fn toggle_fullscreen_option(&self, yesorno: bool) -> Result<(), Box<dyn std::error::Error>> {
        // Read the existing file
        let toml_content = fs::read_to_string(&self.userconfig_path)?;

        // Parse the TOML content
        let mut doc = toml_content.parse::<DocumentMut>()?;

        // Update or insert the key within [window]
        doc["start_in_fullscreen"] = toml_edit::value(yesorno);

        // Write back to the file
        fs::write(&self.userconfig_path, doc.to_string())?;

        Ok(())
    }

    fn toggle_fps_option(&self, yesorno: bool) -> Result<(), Box<dyn std::error::Error>> {
        // Read the existing file
        let toml_content = fs::read_to_string(&self.userconfig_path)?;

        // Parse the TOML content
        let mut doc = toml_content.parse::<DocumentMut>()?;

        // Update or insert the key within [window]
        doc["enable_fps_counter"] = toml_edit::value(yesorno);

        // Write back to the file
        fs::write(&self.userconfig_path, doc.to_string())?;

        Ok(())
    }
    fn toggle_num_weather_days(&self, yesorno: bool) -> Result<(), Box<dyn std::error::Error>> {
        // Read the existing file
        let toml_content = fs::read_to_string(&self.userconfig_path)?;

        // Parse the TOML content
        let mut doc = toml_content.parse::<DocumentMut>()?;

        // Update or insert the key within [window]
        doc["three_day_weather"] = toml_edit::value(yesorno);

        // Write back to the file
        fs::write(&self.userconfig_path, doc.to_string())?;

        Ok(())
    }
    fn set_calendar_weeks(&self) {
        if let Ok(toml_content) = fs::read_to_string(&self.userconfig_path) {
            if let Ok(mut doc) = toml_content.parse::<DocumentMut>() {
                doc["calendar_weeks_to_show"] = toml_edit::value(self.week_number_input.clone().chars().take(5).collect::<String>());

                let _ = fs::write(&self.userconfig_path, doc.to_string());
            }
        }       
    }
    fn set_background_tint(&mut self) {
        if let Ok(toml_content) = fs::read_to_string(&self.userconfig_path) {
            if let Ok(mut doc) = toml_content.parse::<DocumentMut>() {
                let filtered_input = self.background_tint_input.clone().chars().take(3).collect::<String>();
                doc["background_image_tint_percent"] = toml_edit::value(filtered_input.clone());

                let _ = fs::write(&self.userconfig_path, doc.to_string());

                if let Ok(number) = filtered_input.parse::<u32>() {
                    self.background_image_tint_percent = number.clamp(0, 100);
                }
            }
        }       
    }
    fn set_weather_coordinates(&mut self) {
        let coords = [self.latitude, self.longitude];
        self.weather_service.set_coordinates(coords);

        if let Ok(toml_content) = fs::read_to_string(&self.userconfig_path) {
            if let Ok(mut doc) = toml_content.parse::<DocumentMut>() {
                let coordinates = format!("[{},{}]", self.latitude, self.longitude);

                doc["coordinates"] = toml_edit::value(coordinates);

                let _ = fs::write(&self.userconfig_path, doc.to_string());
            }
        }
    }
    fn set_selected_monitor_name(&mut self) {
        self.selected_monitor_name = self.monitor_options.get(self.selected_monitor_index).unwrap_or(&"".to_string()).to_string();

        if let Ok(toml_content) = fs::read_to_string(&self.userconfig_path) {
            if let Ok(mut doc) = toml_content.parse::<DocumentMut>() {
                doc["selected_monitor_name"] = toml_edit::value(self.selected_monitor_name.clone().chars().take(1000).collect::<String>());

                let _ = fs::write(&self.userconfig_path, doc.to_string());
            }
        }
    }
    fn fix_and_cache_weather_data(&mut self) {
        self.weather_is_broken_flag = false;
        let static_weather_data = self.weather_service.data.read().map(|w| w.clone())
            .unwrap_or_else(|_| vec![]);

        if static_weather_data.len() != 24 {
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
    fn restart_self(&self) {
        let exe = std::env::current_exe().expect("Failed to get exe path!");

        Command::new(exe)
            .args(std::env::args().skip(1))
            .spawn()
            .expect("Failed to restart!");

        exit(0);
    }
    fn save_textbox_text(&mut self) {
        if self.should_save_textbox_text {
            let _ = utilities::save_notepad_text(self.textbox_text.clone(), &self.exe_file_path);
            self.should_save_textbox_text = false;
        }
    }
    fn set_colorscheme(&mut self) {
        let selected_scheme = if let Some(scheme) = self.colorschemes.get(&self.selected_colorscheme_id) {
            scheme.colors.map(|c| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]))
        } else {
            ColorScheme::default_scheme().colors.map(|c| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]))
        };

        self.active_colorscheme = selected_scheme;

        if let Ok(toml_content) = fs::read_to_string(&self.userconfig_path) {
            if let Ok(mut doc) = toml_content.parse::<DocumentMut>() {
                doc["selected_colorscheme_id"] = toml_edit::value(self.selected_colorscheme_id.to_string());

                let _ = fs::write(&self.userconfig_path, doc.to_string());
            }
        }
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
        let _ = color::save_colorschemes(&self.colorschemes, &self.exe_file_path);
    }
    fn save_colorscheme_edits(&mut self) {
        if let Some(scheme) = self.colorscheme_being_edited.take() {
            self.colorschemes.insert(self.selected_colorscheme_id, scheme);
        }
    }
    fn try_to_generate_colorscheme(&mut self) {
        let name = self.background_options[self.selected_background_index].clone();

        if let Some(scheme) = color::generate_colorscheme(name) {
            let new_id = self.colorschemes.keys().max().unwrap_or(&0) + 1;

            self.colorschemes.insert(new_id, scheme);

            self.add_schemes_2_doc();
        }
    }
}

impl TaskApp {
    pub fn ui(&mut self, ctx: &egui::Context) {
        if self.background_image_texture.is_none() {
            if let Some(name) = self.pending_initial_background.take() {
                self.background_image_texture = Some(set_background(ctx, name.clone()));
            }
        }

        if self.enable_fps_counter {
            self.fps_counter.update();
        }
        if let Some(old_fullscreen) = ctx.input(|i| {
            if i.key_pressed(Key::F11) {
                i.viewport().fullscreen
            } else {
                None
            }
        }) {
            let new_fullscreen = !old_fullscreen;
            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(new_fullscreen));
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

        if self.chrono_tick_counter > 12000 {
            if self.should_save_textbox_text {
                self.save_textbox_text();
            }

            self.chrono_tick_counter = 0;
        } else {
            self.chrono_tick_counter += 1;
        }

        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                if ui.button("New Task").clicked() {
                    self.new_task_flag = true;
                }
                ui.add_space(12.0);
                if ui.button("New Event").clicked() {
                    self.new_event_flag = true;
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
        egui::CentralPanel::default()
        .show(ctx, |ui| {
            
            let screen_rect = ctx.available_rect();
            ctx.layer_painter(egui::LayerId::background()).image(
                self.background_image_texture.as_ref().unwrap().id(), //THIS WILL CRASH THE APP IF BACKGROUND HAS NOT BEEN INITALIZED
                screen_rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0)),
                egui::Color32::WHITE.gamma_multiply((self.background_image_tint_percent as f32 / 100.0).clamp(0.0, 1.0)),
            );

            ui.horizontal_top(|ui| {

                ui.add_space(5.0);

                self.show_tasks(ui);

                self.show_calendar(ui);              

                ui.add_space(-20.0);

                self.show_weather_forecast(ui);
            });
        });

        if self.user_wants_to_complete_task_flag {
            if let Some(name) = self.confirm_complete_task.clone() {
                egui::Window::new("Confirm Complete")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label(format!("Are you sure you want to mark \"{}\" as complete?", name));
                        ui.horizontal(|ui| {
                            if ui.button("Yes").clicked() {
                                self.complete_active_thing(&name);
                            }
                            if ui.button("No").clicked() {
                                self.confirm_complete_task = None;
                                self.user_wants_to_complete_task_flag = false;
                            }
                        });
                    });
            }
        }

        if self.user_wants_to_delete_task_flag {
            if let Some(name) = self.confirm_delete_task.clone() {
                egui::Window::new("Confirm Delete")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label(format!("Are you sure you want to delete \"{}\"?", name));
                        ui.horizontal(|ui| {
                            if ui.button("Yes").clicked() {
                                self.delete_active_thing(&name);
                            }
                            if ui.button("No").clicked() {
                                self.confirm_delete_task = None;
                                self.user_wants_to_delete_task_flag = false;
                            }
                        });
                    });
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
                                if self.name_is_unique(&self.event_name_input) {
                                    match utilities::parse_time_input(self.day_input, self.month_input, self.year_input, self.hour_input, self.minute_input) {
                                        Ok(date) => {
                                            self.add_active_thing(self.event_name_input.clone(), Some(date), None, true, None);
                                            self.new_event_flag = false;
                                        },
                                        _ => {
                                            self.show_error("Problem with date".to_string());
                                        },
                                    }
                                } else {
                                    self.show_error("An item with that name already exists".to_string());
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
                                
                                if self.name_is_unique(&self.task_name_input) {
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
                            } else {
                                self.show_error("An item with that name already exists".to_string());
                            }
                                }

                            if ui.button("Cancel").clicked() {
                                self.new_task_flag = false;
                            }
                        });
                    });

                });
        }
        
        if self.expand_calendar_day_flag {
            if let Some(index) = self.expanded_day {
                let day = &mut self.calendar_elements[index];
                let selected_date = day.4;

                let (weekday_str, formatted_date) = utilities::format_date(selected_date);

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
                            .auto_shrink([true, true])
                            .max_height(280.0)
                            .show(ui, |ui| {
                                for (event_name, event_time, is_event) in &day.2 {
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

                                                ui.label(RichText::new(event_time).font(time_font));

                                                ui.add(Label::new(RichText::new(event_name.clone()).color(Color32::from_white_alpha(120)).font(text_font)).wrap().selectable(false));
                                                
                                                if ui.rect_contains_pointer(ui.max_rect()) {
                                                    if *is_event {
                                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                            let min_button_size = Vec2::new(24.0, 24.0);

                                                            let delete_button = egui::Button::new("x").min_size(min_button_size).corner_radius(CornerRadius::same(8));
                                                            
                                                            if ui.add(delete_button).clicked() {
                                                                self.user_wants_to_delete_task_flag = true;
                                                                self.confirm_delete_task = Some(event_name.clone());
                                                            }
                                                        });
                                                    } else {
                                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                            let min_button_size = Vec2::new(24.0, 24.0);

                                                            let complete_button = egui::Button::new("✓").min_size(min_button_size).corner_radius(CornerRadius::same(8));
                                                            let delete_button = egui::Button::new("x").min_size(min_button_size).corner_radius(CornerRadius::same(8));
                                                            if ui.add(complete_button).clicked() {
                                                                self.user_wants_to_complete_task_flag = true;
                                                                self.confirm_complete_task = Some(event_name.clone());
                                                            }

                                                            if ui.add(delete_button).clicked() {
                                                                self.user_wants_to_delete_task_flag = true;
                                                                self.confirm_delete_task = Some(event_name.clone());
                                                            }
                                                        });
                                                    }
                                                };
                                            });
                                        });
                                }
                            });
                        });

                            egui::TopBottomPanel::bottom("bottompanel").show_inside(ui, |ui| {
                                ui.add_space(8.0);
                                        ui.horizontal_centered(|ui| {
                                            ui.add_space(65.0);

                                            if ui.button("Close").clicked() {
                                                self.expand_calendar_day_flag = false;
                                                self.expanded_day = None;
                                            }
                                            ui.add_space(5.0);
                                            if ui.button("Event+").clicked() {
                                                self.day_input = day.0 as i32;
                                                self.month_input = (day.4.month0() + 1) as i32;
                                                self.year_input = day.4.year_ce().1 as i32;
                                                self.new_event_flag = true;
                                            }
                                            if ui.button("Task+").clicked() {
                                                self.day_input = day.0 as i32;
                                                self.month_input = (day.4.month0() + 1) as i32;
                                                self.year_input = day.4.year_ce().1 as i32;
                                                self.new_task_flag = true;
                                            }
                                        });
                            });
                    });
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
                                egui::ScrollArea::vertical().show(ui, |ui| {
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
                                let new_background = &self.background_options[self.selected_background_index];

                                self.background_image_texture = Some(set_background(ctx, new_background.to_string()));

                                let _ = self.update_background_config(new_background);
                            }

                            if ui.button("♲").clicked() {
                                let available_background_name_to_refresh_into = self.background_options[self.selected_background_index].to_string();
                                let _ = self.update_background_config(&available_background_name_to_refresh_into);
                                self.background_image_texture = Some(set_background(ctx, available_background_name_to_refresh_into));
                            }
                        });
                        ui.end_row();
                        ui.end_row();
                        ui.horizontal_centered(|ui| {
                            ui.label("Selected startup monitor:");

                            // Keep track of the previously selected index
                            let previous_index = {
                                self.monitor_options.iter().enumerate().find(|(_, name)| name == &&self.selected_monitor_name ).unwrap_or((0, &&self.selected_monitor_name))
                            };
                            self.selected_monitor_index = previous_index.0;

                            ComboBox::from_id_salt("monitor_combo")
                                .selected_text(&self.monitor_options[self.selected_monitor_index])
                                .show_ui(ui, |ui| {
                                    for (i, monitor_name) in self.monitor_options.iter().enumerate() {
                                        ui.selectable_value(
                                            &mut self.selected_monitor_index,
                                            i,
                                            monitor_name,
                                        );
                                    }
                                });

                            if previous_index.0 != self.selected_monitor_index {
                                self.set_selected_monitor_name();
                            }

                            if ui.button("♲").clicked() {
                                self.restart_self();
                            }

                        });                        
                        ui.end_row();
                        ui.end_row();
                        ui.horizontal_centered(|ui| {
                            let previous_selection = self.start_in_fullscreen;

                            ui.checkbox(&mut self.start_in_fullscreen, "Start in fullscreen");

                            if previous_selection != self.start_in_fullscreen {
                                let _ = self.toggle_fullscreen_option(self.start_in_fullscreen);
                            }
                        });
                        ui.end_row();
                        ui.end_row();
                        ui.horizontal_centered(|ui| {
                            let previous_selection = self.enable_fps_counter;

                            ui.checkbox(&mut self.enable_fps_counter, "Enable fps counter");

                            if previous_selection != self.enable_fps_counter {
                                let _ = self.toggle_fps_option(self.enable_fps_counter);
                            }
                        });
                        ui.end_row();
                        ui.end_row();
                        ui.horizontal_centered(|ui| {
                            let previous_selection = self.three_day_weather;

                            ui.checkbox(&mut self.three_day_weather, "Show weather for three days");

                            if previous_selection != self.three_day_weather {
                                let _ = self.toggle_num_weather_days(self.three_day_weather);
                            }
                        });
                        ui.end_row();
                        ui.end_row();
                        ui.horizontal_centered(|ui| {
                            ui.set_max_width(300.0);
                            ui.label("Number of displayed weeks: ");
                            if ui.text_edit_singleline(&mut self.week_number_input).changed() {
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
                            ui.label("Weather Coordinates: ");

                            let y_slider = egui::DragValue::new(&mut self.latitude)
                                .prefix("Latitude (Y): ")
                                .range(-90..=90)
                                .fixed_decimals(2)
                                .speed(0.0025);
                            ui.add(y_slider);

                            let x_slider = egui::DragValue::new(&mut self.longitude)
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
                    let _map_response = ui.allocate_ui_at_rect(map_rect, |ui| {
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

                                self.longitude = world_uv.x * 360.0 - 180.0;
                                self.latitude = (1.0 - world_uv.y) * 180.0 - 90.0;
                            }
                        }

                        // ---------------- SELECTED MARKER ----------------
                        let world_uv = egui::pos2(
                            (self.longitude + 180.0) / 360.0,
                            1.0 - ((self.latitude + 90.0) / 180.0),
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
                    ui.allocate_ui_at_rect(footer_rect, |ui| {
                        ui.horizontal_centered(|ui| {
                            ui.add_space(20.0);
                            ui.label(format!("Lat: {:.2}", self.latitude));
                            ui.separator();
                            ui.label(format!("Lon: {:.2}", self.longitude));
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
                                    ui.allocate_ui_at_rect(rect, |ui| {
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

    if self.expand_calendar_day_flag || self.display_archive_flag || self.error_flag || self.new_task_flag || self.user_wants_to_delete_task_flag || self.user_wants_to_complete_task_flag || self.new_event_flag || self.settings_flag || self.rename_colorscheme_flag || self.user_wants_to_delete_colorscheme_flag || self.color_picker_flag {
        self.hovered_calendar_cell = None;
    }
    }
}

pub fn set_styles(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (egui::TextStyle::Heading, egui::FontId::new(30.0, egui::FontFamily::Monospace)),
        (egui::TextStyle::Body, egui::FontId::new(18.0, egui::FontFamily::Monospace)),
        (egui::TextStyle::Button, egui::FontId::new(22.0, egui::FontFamily::Monospace)),
        (egui::TextStyle::Small, egui::FontId::new(11.0, egui::FontFamily::Monospace)),
        (egui::TextStyle::Monospace, egui::FontId::new(11.0, egui::FontFamily::Monospace)),
    ]
    .into();
    ctx.set_style(style);
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

fn set_background(ctx: &Context, name: String) -> TextureHandle {
    let cleaned = name.replace("..", "");

    let mut path = PathBuf::from("images");
    path.push(cleaned);

    let image = match attempt_background(path) {
        Ok(background) => background,
        Err(_) => image::load_from_memory(include_bytes!("../noback.png")).expect("Did not get access to fallback background").to_rgba8()
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