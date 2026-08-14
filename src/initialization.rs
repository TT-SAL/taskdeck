use egui::Context;
use egui_wgpu::wgpu::{StoreOp};
use egui_wgpu::{wgpu, Renderer, RendererOptions, ScreenDescriptor};
use egui_winit::{ActionRequested, State};
use serde::{Deserialize, Serialize};
use crate::ui::TaskApp;
use wgpu::{Color, ExperimentalFeatures, LoadOp};
use winit::event::WindowEvent;
// The taskbar-icon extension trait only exists on Windows; see `window_attributes`.
#[cfg(windows)]
use winit::platform::windows::WindowAttributesExtWindows;
use winit::window::{Window, WindowId};
use egui_wgpu::wgpu::CurrentSurfaceTexture;
use std::collections::HashMap;
use std::{fs, time};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event_loop::ActiveEventLoop;
use toml::Value;

/// Reads a TOML config file and extracts all valid values, including arrays.
/// If parsing fails, falls back to line-by-line extraction.
fn read_config(path: &Path) -> HashMap<String, String> {
    let contents = match fs::read_to_string(path) {
        Ok(thing) => thing,
        Err(_) => {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::File::create(path);
            " ".to_string()
        },
    };
    let mut config = HashMap::new();

    // Try parsing with the TOML crate first
    match contents.parse::<Value>() {
        Ok(toml_value) => {
            fn extract_values(value: &Value, prefix: &str, config: &mut HashMap<String, String>) {
                match value {
                    Value::Table(table) => {
                        for (k, v) in table {
                            let new_prefix = if prefix.is_empty() { k.clone() } else { format!("{}.{}", prefix, k) };
                            extract_values(v, &new_prefix, config);
                        }
                    }
                    Value::Array(arr) => {
                        let mut items = vec![];
                        for item in arr {
                            match item {
                                Value::String(s) => items.push(format!("\"{}\"", s)),
                                Value::Integer(i) => items.push(i.to_string()),
                                Value::Float(f) => items.push(f.to_string()),
                                Value::Boolean(b) => items.push(b.to_string()),
                                _ => {}
                            }
                        }
                        config.insert(prefix.to_string(), format!("[{}]", items.join(", ")));
                    }
                    Value::String(s) => { config.insert(prefix.to_string(), s.clone()); }
                    Value::Integer(i) => { config.insert(prefix.to_string(), i.to_string()); }
                    Value::Float(f) => { config.insert(prefix.to_string(), f.to_string()); }
                    Value::Boolean(b) => { config.insert(prefix.to_string(), b.to_string()); }
                    _ => {} // ignore datetime for simplicity
                }
            }

            extract_values(&toml_value, "", &mut config);
        }
        Err(_) => {
            // Fallback: line-by-line extraction
            for line in contents.lines() {
                let line = line.split('#').next().unwrap().trim(); // remove comments
                if let Some(pos) = line.find('=') {
                    let key = line[..pos].trim();
                    let value = line[pos + 1..].trim();

                    let parsed_value = if let Ok(i) = value.parse::<i64>() {
                        i.to_string()
                    } else if let Ok(f) = value.parse::<f64>() {
                        f.to_string()
                    } else if let Ok(b) = value.parse::<bool>() {
                        b.to_string()
                    } else if value.starts_with('"') && value.ends_with('"') && value.len() > 1 {
                        value[1..value.len() - 1].to_string()
                    } else if value.starts_with('[') && value.ends_with(']') {
                        value.to_string() // crude fallback: keep array as-is
                    } else if
                        (value.starts_with('"') && value.ends_with('"')) ||
                        (value.starts_with('\'') && value.ends_with('\''))
                    {
                        value[1..value.len() - 1].to_string()
                    } else {
                        continue; // skip invalid
                    };

                    config.insert(key.to_string(), parsed_value);
                }
            }
        }
    }

    config
}

/// Parse a boolean config value. Accepts the canonical `true`/`false` written by
/// the config writer plus a few common variants (case-insensitive, surrounding
/// whitespace ignored); anything unrecognised defaults to `false`. The old
/// `contains("t")` heuristic treated e.g. `"east"` or `"set"` as `true`.
fn parse_config_bool(text: &str) -> bool {
    matches!(text.trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes" | "on")
}

/// Allowed range for `calendar_weeks_to_show`. Shared by the startup loader and
/// the live setter (`TaskApp::set_calendar_weeks`) so both clamp identically.
/// The upper bound is ~10 years: enough for any realistic planning horizon while
/// keeping the calendar model rebuild (and `row_anim`) bounded.
pub const CALENDAR_WEEKS_MIN: usize = 6;
pub const CALENDAR_WEEKS_MAX: usize = 520;

/// Width, in egui points, that the three-column layout needs to show all of
/// itself: the 300pt task list, the 7×160pt calendar grid with its 14pt gutters,
/// and the weather/notepad column. `TaskApp::apply_ui_scale` keeps the window at
/// least this wide in points by scaling the UI down when it isn't.
pub const DESIGN_WIDTH_POINTS: f32 = 1920.0;

/// `ui_scale_percent = 0` means "fit the layout to the window automatically".
/// Any other value is an explicit scale the user picked.
pub const UI_SCALE_AUTO: u32 = 0;
/// Bounds for an explicit `ui_scale_percent`, and for the automatic fit. The
/// lower bound keeps text legible in a small window; the upper bound is 100%
/// because the layout is tuned at that size and nothing is gained by magnifying
/// it past the design width.
pub const UI_SCALE_MIN: u32 = 40;
pub const UI_SCALE_MAX: u32 = 100;

/// Clamp a configured UI scale, preserving the `0` = automatic sentinel.
pub fn clamp_ui_scale_percent(percent: u32) -> u32 {
    if percent == UI_SCALE_AUTO {
        UI_SCALE_AUTO
    } else {
        percent.clamp(UI_SCALE_MIN, UI_SCALE_MAX)
    }
}

pub fn get_check_and_set_config(config_path: &Path) -> Config {
    let extracted = read_config(config_path);

    let config = Config {
        window_size_startup: extracted
            .get("window_size_startup")
            .and_then(|v| {
                v.trim_matches(|c| c == '[' || c == ']') // remove brackets if present
                    .split(',')
                    .map(|s| s.trim().parse::<f32>().ok())
                    .collect::<Option<Vec<_>>>() // only succeeds if both parse correctly
                    .and_then(|nums| {
                        if nums.len() == 2 {
                            Some([nums[0], nums[1]])
                        } else {
                            None
                        }
                    })
            })
            .filter(|v| !v.iter().any(|x| x < &200.0))
            .unwrap_or([1280.0, 720.0]),
        start_in_fullscreen: extracted
            .get("start_in_fullscreen")
            .map(|s| parse_config_bool(s))
            .unwrap_or(false),
        enable_fps_counter: extracted
            .get("enable_fps_counter")
            .map(|s| parse_config_bool(s))
            .unwrap_or(false),
        three_day_weather: extracted
            .get("three_day_weather")
            .map(|s| parse_config_bool(s))
            .unwrap_or(false),
        background: extracted
            .get("background")
            .unwrap_or(&"".to_string()).to_string(),
        coordinates: extracted
            .get("coordinates")
            .and_then(|v| {
                v.trim_matches(|c| c == '[' || c == ']') // remove brackets if present
                    .split(',')
                    .map(|s| s.trim().parse::<f32>().ok())
                    .collect::<Option<Vec<_>>>() // only succeeds if both parse correctly
                    .and_then(|nums| {
                        if nums.len() == 2 {
                            Some([nums[0], nums[1]])
                        } else {
                            None
                        }
                    })
            })
            .unwrap_or([0.0, 0.0]),
        calendar_weeks_to_show: extracted
            .get("calendar_weeks_to_show")
            .and_then(|n| n.parse::<usize>().ok().map(|x| x.clamp(CALENDAR_WEEKS_MIN, CALENDAR_WEEKS_MAX)))
            .unwrap_or(100),
        background_image_tint_percent: extracted
            .get("background_image_tint_percent")
            .and_then(|n| n.parse::<u32>().ok().and_then(|x| Some(x.clamp(1, 100))))
            .unwrap_or(30),
        ui_scale_percent: extracted
            .get("ui_scale_percent")
            .and_then(|n| n.parse::<u32>().ok())
            .map(clamp_ui_scale_percent)
            .unwrap_or(UI_SCALE_AUTO),
        selected_monitor_name: extracted
            .get("selected_monitor_name")
            .unwrap_or(&"".to_string()).to_string(),
        selected_colorscheme_id: extracted
            .get("selected_colorscheme_id")
            .and_then(|n| n.parse::<u32>().ok().and_then(|x| Some(x.clamp(0, 200000))))
            .unwrap_or(0),
    };

    write_normalized_config(config_path, &config);

    config
}

/// Persist the normalized/clamped config back to disk using `toml_edit`, so the
/// runtime setters (also `toml_edit`) and this startup writer share one
/// mechanism and one set of value types. Unlike the old `toml::to_string`
/// rewrite, this preserves any comments, key ordering, and unknown keys already
/// in the file; it only updates the keys we own, and writes numbers as real
/// integers/float-arrays rather than strings. A missing or unparseable file
/// falls back to a fresh document (the same self-healing the old code did).
fn write_normalized_config(path: &Path, config: &Config) {
    use toml_edit::{value, DocumentMut};

    let mut doc = fs::read_to_string(path)
        .ok()
        .and_then(|c| c.parse::<DocumentMut>().ok())
        .unwrap_or_default();

    doc["start_in_fullscreen"] = value(config.start_in_fullscreen);
    doc["coordinates"] = value(crate::utilities::float_pair_array(config.coordinates));
    doc["background"] = value(config.background.clone());
    doc["enable_fps_counter"] = value(config.enable_fps_counter);
    doc["window_size_startup"] = value(crate::utilities::float_pair_array(config.window_size_startup));
    doc["calendar_weeks_to_show"] = value(config.calendar_weeks_to_show as i64);
    doc["selected_monitor_name"] = value(config.selected_monitor_name.clone());
    doc["selected_colorscheme_id"] = value(config.selected_colorscheme_id as i64);
    doc["three_day_weather"] = value(config.three_day_weather);
    doc["background_image_tint_percent"] = value(config.background_image_tint_percent as i64);
    doc["ui_scale_percent"] = value(config.ui_scale_percent as i64);

    let _ = fs::write(path, doc.to_string());
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    pub start_in_fullscreen: bool,
    pub coordinates: [f32; 2],
    pub background: String,
    pub enable_fps_counter: bool,
    pub window_size_startup: [f32; 2],
    pub calendar_weeks_to_show: usize,
    pub selected_monitor_name: String,
    pub selected_colorscheme_id: u32,
    pub three_day_weather: bool,
    pub background_image_tint_percent: u32,
    /// Percentage the whole UI is scaled by, or `UI_SCALE_AUTO` (0) to fit the
    /// layout to the window automatically.
    pub ui_scale_percent: u32,
}

pub struct AppState<'a> {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface_config: wgpu::SurfaceConfiguration,
    pub surface: wgpu::Surface<'a>,
    pub egui_winit_state: State,
    pub egui_wgpu_renderer: Renderer,
    /// Kept alive for as long as the surface it created. Declared last so it is
    /// dropped after the surface.
    _instance: wgpu::Instance,
}

impl AppState<'_> {
    async fn new(
        instance: wgpu::Instance,
        surface: wgpu::Surface<'static>,
        window: &Window,
        width: u32,
        height: u32,
    ) -> Self {
        let power_pref = wgpu::PowerPreference::HighPerformance; //Used to be on default
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: power_pref,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await
            .expect("Failed to find an appropriate adapter");

        let features = wgpu::Features::empty();
        
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: None,
                    required_features: features,
                    required_limits: Default::default(),
                    memory_hints: Default::default(),
                    trace: Default::default(),
                    experimental_features: ExperimentalFeatures::disabled(),
                }
            )
            .await
            .expect("Failed to create device");

        let swapchain_capabilities = surface.get_capabilities(&adapter);

        // `Bgra8Unorm` is the format this app was tuned against and is what
        // Windows/DX12, macOS/Metal and most Vulkan drivers offer, so it stays the
        // first choice. It is not, however, guaranteed anywhere — some Linux GL
        // and software adapters expose only RGBA or only the sRGB variants — and
        // the old `expect` turned that into a startup panic. Fall back through the
        // near-equivalents and finally to whatever the surface does support;
        // `egui-wgpu` picks its sRGB-aware shader from the format we pass it, so
        // the colours stay right either way.
        let swapchain_format = [
            wgpu::TextureFormat::Bgra8Unorm,
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureFormat::Bgra8UnormSrgb,
            wgpu::TextureFormat::Rgba8UnormSrgb,
        ]
        .into_iter()
        .find(|preferred| swapchain_capabilities.formats.contains(preferred))
        .or_else(|| swapchain_capabilities.formats.first().copied())
        .expect("the surface reported no supported texture formats");

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: swapchain_format,
            width,
            height,
            present_mode: wgpu::PresentMode::AutoNoVsync,       //Should work on different devices
            desired_maximum_frame_latency: 2,                   //This may need adjusting
            alpha_mode: swapchain_capabilities.alpha_modes[0],
            view_formats: vec![],
        };

        surface.configure(&device, &surface_config);

        let egui_context = Context::default();

        // Pin egui to a single pass per frame. 0.35's `run_ui` defaults to `max_passes = 2`
        // (a second pass on `request_discard`, e.g. first-frame Grid/window sizing), whereas
        // 0.33's begin_pass/end_pass loop was always single-pass. Staying single-pass preserves
        // behaviour and avoids re-running this app's non-idempotent click handlers (add task,
        // toggle archive, "show more") twice within one frame.
        egui_context.options_mut(|o| o.max_passes = std::num::NonZeroUsize::new(1).unwrap());

        let max_texture_side = device.limits().max_texture_dimension_2d as usize;

        #[cfg(debug_assertions)] {
            println!(
                "Adapter max texture size: {}",
                adapter.limits().max_texture_dimension_2d
            );

            println!(
                "Device max texture size: {}",
                device.limits().max_texture_dimension_2d
            );
        }

        let egui_winit_state = egui_winit::State::new(
            egui_context,
            egui::viewport::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            Some(max_texture_side), // default dimension is 2048
        );

        let renderer_options = RendererOptions {
            msaa_samples: 1,
            depth_stencil_format: None,
            dithering: false,
            predictable_texture_filtering: true,
        };

        let egui_wgpu_renderer = Renderer::new(
            &device,
            surface_config.format,
            renderer_options,
        );

        Self {
            device,
            queue,
            surface,
            surface_config,
            egui_wgpu_renderer,
            egui_winit_state,
            _instance: instance,
        }
    }

    fn resize_surface(&mut self, width: u32, height: u32) {
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(&self.device, &self.surface_config);
    }

    pub fn context(&self) -> &Context {
        self.egui_winit_state.egui_ctx()
    }
}

pub struct App<'a> {
    cursor_inside_window: bool,
    window_is_focused: bool,
    state: Option<AppState<'a>>,
    window: Option<Arc<Window>>,
    task_app: TaskApp,
    #[cfg(debug_assertions)]
    repaint_debugger_count: u32,
    last_active: Option<Instant>,
    in_sleep: bool,
    window_size_startup: [f32; 2],
    selected_monitor_name: String,
}

impl<'a> App<'a> {
    pub fn new(task_app: TaskApp, window_size_startup: [f32; 2], selected_monitor_name: String) -> Self {
        Self {
            cursor_inside_window: false,
            window_is_focused: false,
            state: None,
            window: None,
            task_app,
            #[cfg(debug_assertions)]
            repaint_debugger_count: 0,
            last_active: Some(std::time::Instant::now()),
            in_sleep: false,
            window_size_startup,
            selected_monitor_name,
        }
    }

    async fn set_window(&mut self, window: Window) {
        let window = Arc::new(window);

        // The surface must be sized in *physical* pixels. Take the size the
        // platform actually gave the window rather than re-deriving it from the
        // (logical) configured size — on a HiDPI display those differ by the
        // scale factor, and a mismatched surface renders blurry/stretched until
        // the first resize event happens to correct it.
        let inner_size = window.inner_size();
        let initial_width = inner_size.width.max(1);
        let initial_height = inner_size.height.max(1);

        // wgpu 29 takes `InstanceDescriptor` by value and dropped its `Default` impl.
        // Building it from the window's *display* handle (rather than
        // `new_without_display_handle()`) is what lets the GL/EGL backend come up on
        // Linux, where enumerating adapters needs the X11 or Wayland display — that
        // is the fallback path on machines with no working Vulkan driver. On Windows
        // and macOS the backends work from the window handle alone, so this is
        // simply the same instance as before.
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(
            Box::new(window.clone()),
        ));

        let surface = instance
            .create_surface(window.clone())
            .expect("Failed to create surface!");

        let state = AppState::new(
            instance,
            surface,
            &window,
            initial_width,
            initial_height,
        )
        .await;

        self.window.get_or_insert(window);

        let ctx = state.context();
        self.task_app.init_with_context(ctx);

        self.state.get_or_insert(state);
    }

    fn handle_resized(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.state.as_mut().unwrap().resize_surface(width, height);
        }
    }

    fn handle_redraw(&mut self, event_loop: &ActiveEventLoop) {
        let window = match &self.window {
            Some(w) => w,
            None => return,
        };

        // Skip if minimized
        if window.is_minimized().unwrap_or(false) {
            return;
        }

        let state = match &mut self.state {
            Some(s) => s,
            None => return,
        };

        // --- Acquire next surface texture ---
        // This happens *before* the input is taken, and deliberately so: every arm
        // below abandons the frame, and `take_egui_input` is destructive — it hands
        // over the accumulated input and clears it. Taking input first meant an
        // abandoned frame silently swallowed whatever had accumulated: the user's
        // clicks and keystrokes, and — because it is delivered exactly once, on the
        // first take — the `max_texture_side` the GPU actually supports. egui then
        // kept its conservative 2048 default forever, and loading any background
        // image wider than that panicked. macOS reports `Outdated` on the first
        // acquire almost every launch, which is why this bit here and not on
        // Windows.
        let surface_texture = match state.surface.get_current_texture() {
            // wgpu 29 returns a `CurrentSurfaceTexture` enum instead of `Result`. A suboptimal
            // texture is still rendered, matching the old code which used `Ok(tex)` without
            // checking the `suboptimal` flag.
            CurrentSurfaceTexture::Success(tex) | CurrentSurfaceTexture::Suboptimal(tex) => tex,
            CurrentSurfaceTexture::Outdated | CurrentSurfaceTexture::Lost => {
                state.surface.configure(&state.device, &state.surface_config);
                self.window.as_ref().unwrap().request_redraw();
                return;
            }
            CurrentSurfaceTexture::Timeout => {
                eprintln!("Surface timed out!");
                return;
            }
            // `Occluded` (window hidden) and `Validation` are new variants; skip the frame.
            // The old `OutOfMemory => exit(1)` arm is gone — OOM is no longer a surface-acquire
            // variant in wgpu 29.
            CurrentSurfaceTexture::Occluded | CurrentSurfaceTexture::Validation => return,
        };

        let raw_input = state.egui_winit_state.take_egui_input(window);
        //When the window is both not active and not being interacted with for 10 seconds put the app into sleep
        if raw_input.events.is_empty() && !state.context().has_requested_repaint() &&!self.window_is_focused && !self.cursor_inside_window {
            match self.last_active {
                Some(time) => {
                    let elapsed = time.elapsed();
                    if elapsed > time::Duration::from_secs(10) {
                        self.in_sleep = true;
                    }
                }
                None => self.last_active = Some(Instant::now()),
            }
        } else {
            self.last_active = Some(Instant::now());
        }

        let surface_view = surface_texture.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = state.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        // --- Run one egui frame ---
        // Set the viewport's fullscreen flag before the pass runs (unchanged behaviour: this
        // mutates the persisted winit input, taking effect on the next input take).
        let is_fullscreen = self
            .window
            .as_ref()
            .and_then(|w| w.fullscreen().map(|_| true))
            .unwrap_or(false);

        let root_id = egui::viewport::ViewportId::ROOT;
        let info = state.egui_winit_state.egui_input_mut().viewports.entry(root_id).or_default();
        info.fullscreen = Some(is_fullscreen);

        // egui 0.35 replaces the `begin_pass` + `end_pass` pair with `run_ui`, which builds the
        // root `&mut Ui` the panels now attach to (and does the panel/hit-test bookkeeping the old
        // path did). `max_passes` is pinned to 1 in `AppState::new`, so this stays single-pass like
        // the old loop. The context is an `Arc` handle; cloning it frees the borrow of `state` so
        // the closure can take `&mut self.task_app`.
        let egui_ctx = state.context().clone();
        let task_app = &mut self.task_app;
        let full_output = egui_ctx.run_ui(raw_input, |ui| {
            task_app.ui(ui);
        });

        // egui reports the points-per-pixel it actually laid the frame out with
        // (window scale factor × egui's zoom factor). Feeding that same value to
        // the renderer is what keeps the drawn size matched to the surface on a
        // HiDPI display; deriving it separately lets the two drift apart.
        let screen_descriptor = ScreenDescriptor {
            size_in_pixels: [state.surface_config.width, state.surface_config.height],
            pixels_per_point: full_output.pixels_per_point,
        };

        let mut actions_requested: Vec<ActionRequested> = vec![];
        let egui_ctx = state.context().clone();
        let window = &self.window.as_ref().unwrap();

        for (id, output) in full_output.viewport_output.into_iter() {
            // First, let egui_winit process most commands (it mutates ViewportInfo and calls Window APIs).
            if let Some(viewport_info) = state.egui_winit_state.egui_input_mut().viewports.get_mut(&id) {
                egui_winit::process_viewport_commands(
                    &egui_ctx,
                    viewport_info,
                    output.commands,
                    &window,
                    &mut actions_requested,
                );
                if viewport_info.events.iter().any(|e| matches!(e, egui::ViewportEvent::Close)) {
                    event_loop.exit();
                }
            }
        }

        // Handle platform output first (mutable borrow)
        state.egui_winit_state.handle_platform_output(window, full_output.platform_output);

        // Tessellate shapes (immutable borrow). Tessellating at the same
        // points-per-pixel the frame was laid out with keeps glyph rasterization
        // and anti-aliasing sharp at any DPI.
        let ctx = state.context();
        let paint_jobs = ctx.tessellate(full_output.shapes, full_output.pixels_per_point);

        #[cfg(debug_assertions)]
        let repaint_reasons = {
            let causes = ctx.repaint_causes();
            let reasons = causes.clone();
            reasons
        };

        // Upload GPU textures
        for (id, delta) in &full_output.textures_delta.set {
            state.egui_wgpu_renderer.update_texture(&state.device, &state.queue, *id, delta);
        }

        // Update vertex/index buffers
        state.egui_wgpu_renderer.update_buffers(&state.device, &state.queue, &mut encoder, &paint_jobs, &screen_descriptor);

        // --- Scoped render pass ---
        {
            let rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui main render pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &surface_view,
                    resolve_target: None,
                    ops: egui_wgpu::wgpu::Operations {
                        load: LoadOp::Clear(Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }),
                        store: StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            state.egui_wgpu_renderer.render(&mut rpass.forget_lifetime(), &paint_jobs, &screen_descriptor);
        } // rpass dropped here

        // --- Submit encoder and present ---
        state.queue.submit(Some(encoder.finish()));
        surface_texture.present();

        // Free old textures after submission
        for tex_id in full_output.textures_delta.free {
            state.egui_wgpu_renderer.free_texture(&tex_id);
        }

        #[cfg(debug_assertions)] {
            self.repaint_debugger_count += 1;
            if self.repaint_debugger_count >= 50 {
                self.repaint_debugger_count = 0;
                for reason in repaint_reasons {
                    println!("reason for repainting: {}", reason);
                }
            }
        }
    }

    /// Build the window attributes: title, icon, size, and the position that
    /// centres the window on the configured monitor.
    ///
    /// All the geometry here is done in **physical** pixels. `window_size_startup`
    /// is a logical size (that's what the Settings UI shows), so it is scaled by
    /// the target monitor's scale factor before being compared against the
    /// monitor's physical rect. Mixing the two — as the earlier version did —
    /// pushed the window off-screen on any HiDPI display (a 2× Retina monitor
    /// made the window twice as wide as the centring maths assumed).
    fn window_attributes(&mut self, event_loop: &ActiveEventLoop) -> winit::window::WindowAttributes {
        self.task_app.monitor_options = event_loop
            .available_monitors()
            .flat_map(|m| m.name())
            .collect();

        // Prefer the configured monitor, else the primary, else the first one
        // winit reports. All three can be absent (headless / Wayland without the
        // right protocol), in which case we simply don't set a position and let
        // the compositor place the window.
        let target_monitor = event_loop
            .available_monitors()
            .find(|m| m.name().as_deref() == Some(self.selected_monitor_name.as_str()))
            .or_else(|| event_loop.primary_monitor())
            .or_else(|| event_loop.available_monitors().next());

        let window_title = format!("TaskDeck    -   Ver.{}", env!("BUILD_DATE"));

        let icon_data = window_icon();

        let mut attributes = Window::default_attributes()
            .with_title(window_title)
            .with_window_icon(icon_data.clone())
            .with_inner_size(LogicalSize::new(
                self.window_size_startup[0],
                self.window_size_startup[1],
            ))
            .with_min_inner_size(LogicalSize::new(200.0, 200.0))
            .with_active(false);

        // Windows keeps a separate taskbar icon; every other platform takes the
        // window icon (or, on macOS, the bundle icon) and has no such setter.
        #[cfg(windows)]
        {
            attributes = attributes.with_taskbar_icon(icon_data);
        }

        if let Some(monitor) = target_monitor {
            let scale = monitor.scale_factor();
            let monitor_position = monitor.position();
            let monitor_size = monitor.size();

            let window_width = (self.window_size_startup[0] as f64 * scale) as i32;
            let window_height = (self.window_size_startup[1] as f64 * scale) as i32;

            attributes = attributes.with_position(PhysicalPosition::new(
                monitor_position.x + (monitor_size.width as i32 - window_width) / 2,
                monitor_position.y + (monitor_size.height as i32 - window_height) / 2,
            ));
        }

        attributes
    }
}

/// Decode the embedded PNG into a winit icon. A failure here is cosmetic — the
/// window just gets the platform default — so it must not abort startup the way
/// the old `unwrap()` pair did.
fn window_icon() -> Option<winit::window::Icon> {
    let image = image::load_from_memory_with_format(
        include_bytes!("../icon.png"),
        image::ImageFormat::Png,
    )
    .ok()?
    .into_rgba8();
    let (width, height) = image.dimensions();
    winit::window::Icon::from_rgba(image.into_raw(), width, height).ok()
}

impl ApplicationHandler for App<'_> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attributes = self.window_attributes(event_loop);
        let window = event_loop.create_window(attributes).unwrap();
        pollster::block_on(self.set_window(window));
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        if let Some(state) = self.state.as_mut() {
            // let egui render to process the event first
            let resp = state
                .egui_winit_state
                .on_window_event(self.window.as_ref().unwrap(), &event);

            if resp.consumed {
                return;
            }
        }

        match event {
            WindowEvent::CloseRequested => {
                #[cfg(debug_assertions)] {
                    eprintln!("The close button was pressed; stopping");
                }
                event_loop.exit();
            }
            WindowEvent::Resized(new_size) => {
                self.handle_resized(new_size.width, new_size.height);
            }
            // Moving the window to a monitor with a different DPI (or changing the
            // display scale) only requires reconfiguring the surface to the new
            // physical size. egui's own points-per-pixel is re-read from the window
            // by `egui-winit` every frame, so it must *not* be pushed back into the
            // context here: `set_pixels_per_point` sets egui's zoom factor, which is
            // then multiplied by the native scale factor again — on a 2× display
            // that squared to 4×, and the UI was laid out for a quarter of the
            // window. (Invisible on a 1× Windows display, where the factor is 1 and
            // this event never fires.)
            WindowEvent::ScaleFactorChanged { mut inner_size_writer, .. } => {
                let physical_size = self.window.as_ref().unwrap().inner_size();

                if let Some(state) = self.state.as_mut() {
                    state.resize_surface(physical_size.width, physical_size.height);
                }

                // Optionally, request the inner size (to affirm this size)
                let _ = inner_size_writer.request_inner_size(physical_size);
            }
            WindowEvent::Focused(bool) => {
                self.window_is_focused = bool;
                self.cursor_inside_window = bool;

                self.last_active = None;
                self.in_sleep = false;
                self.handle_redraw(event_loop);
                self.window.as_ref().unwrap().request_redraw();
            }
            WindowEvent::RedrawRequested => {
                self.handle_redraw(event_loop);

                if !self.in_sleep {
                    self.window.as_ref().unwrap().request_redraw();
                }
            }
            WindowEvent::CursorEntered { .. } => {
                self.cursor_inside_window = true;
                self.last_active = None;
                self.in_sleep = false;
                self.handle_redraw(event_loop);
                self.window.as_ref().unwrap().request_redraw();
            }
            WindowEvent::CursorMoved { .. } => {
                self.cursor_inside_window = true;
                self.last_active = None;
                self.in_sleep = false;
                self.handle_redraw(event_loop);
                self.window.as_ref().unwrap().request_redraw();
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor_inside_window = false;
                self.last_active = None;
                self.in_sleep = false;
                self.handle_redraw(event_loop);
                self.window.as_ref().unwrap().request_redraw();
            }
            _ => (),
        }
    }

    //This function is implemented so that the weather thread can make the UI refresh
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: ()) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
            window.request_redraw();
        }
    }

    // Called once when the event loop is shutting down (Quit button, window close,
    // etc.). Flush any buffered state so notepad edits made right before exit are
    // not lost. Note: this does not run on a hard kill or on panic (panic = abort).
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.task_app.flush_pending_saves();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_config() -> Config {
        Config {
            start_in_fullscreen: true,
            coordinates: [60.17, 24.94],
            background: "pic.png".to_string(),
            enable_fps_counter: false,
            window_size_startup: [1280.0, 720.0],
            calendar_weeks_to_show: 100,
            selected_monitor_name: "Main".to_string(),
            selected_colorscheme_id: 3,
            three_day_weather: true,
            background_image_tint_percent: 30,
            ui_scale_percent: UI_SCALE_AUTO,
        }
    }

    #[test]
    fn parse_config_bool_only_true_for_real_truthy_values() {
        for t in ["true", "TRUE", " True ", "1", "yes", "on"] {
            assert!(parse_config_bool(t), "{t:?} should parse true");
        }
        // The old contains("t") heuristic wrongly returned true for these.
        for f in ["false", "east", "set", "", "0", "no", "off"] {
            assert!(!parse_config_bool(f), "{f:?} should parse false");
        }
    }

    #[test]
    fn write_normalized_config_preserves_comments_and_writes_typed_values() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("userconfig.toml");

        // A file with a user comment and a *stringified* number, as the old buggy
        // runtime setters used to write.
        fs::write(&path, "# keep me\ncalendar_weeks_to_show = \"100\"\n").unwrap();

        write_normalized_config(&path, &sample_config());

        let written = fs::read_to_string(&path).unwrap();
        // The comment survives (the old `toml::to_string` rewrite dropped it).
        assert!(written.contains("# keep me"), "comment lost:\n{written}");

        // Values come back with their real TOML types, not as quoted strings.
        let doc = written.parse::<toml_edit::DocumentMut>().unwrap();
        assert_eq!(doc["calendar_weeks_to_show"].as_integer(), Some(100));
        assert_eq!(doc["selected_colorscheme_id"].as_integer(), Some(3));
        assert_eq!(doc["background_image_tint_percent"].as_integer(), Some(30));
        assert_eq!(doc["start_in_fullscreen"].as_bool(), Some(true));
        assert!(doc["coordinates"].is_array(), "coordinates should be an array");
        assert!(doc["window_size_startup"].is_array(), "window size should be an array");
    }
}