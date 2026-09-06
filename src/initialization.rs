use egui::Context;
use egui_wgpu::wgpu::{StoreOp};
use egui_wgpu::{wgpu, Renderer, RendererOptions, ScreenDescriptor};
use egui_winit::{ActionRequested, State};
use serde::{Deserialize, Serialize};
use crate::ui::TaskApp;
use wgpu::{Color, ExperimentalFeatures, LoadOp};
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::ControlFlow;
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
    parse_config_text(&contents)
}

/// The settings file's text as key → value, whether or not it is valid TOML.
fn parse_config_text(contents: &str) -> HashMap<String, String> {
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
                    } else if value.len() > 1
                        && ((value.starts_with('"') && value.ends_with('"'))
                            || (value.starts_with('\'') && value.ends_with('\'')))
                    {
                        // A lone quote — the unclosed string that broke the
                        // TOML and brought the file here — is one character
                        // that both starts and ends the value; it is skipped
                        // below rather than sliced as `1..0`.
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
/// Default blur: a fraction of the picture's width, gentle.
pub const BACKGROUND_BLUR_DEFAULT: u32 = 11;
/// Default lightness ceiling for the phone, where text sits on the picture.
pub const BACKGROUND_LIGHT_DEFAULT: u32 = 39;
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

/// A bind address from the file: an IP address as written (trimmed), or
/// `phone::DEFAULT_BIND` for anything that is not one — the setting narrows
/// where the phone view listens; it never turns the view off.
pub fn clean_bind_address(text: &str) -> String {
    match text.trim().parse::<std::net::IpAddr>() {
        Ok(ip) => ip.to_string(),
        Err(_) => crate::phone::DEFAULT_BIND.to_string(),
    }
}

/// The address to hand out, from the file: an `http://` or `https://` origin
/// with the trailing slash taken off, or empty for anything else.
///
/// Empty is the ordinary case and means "use this machine's own addresses".
/// Anything that is not a URL is treated as empty rather than refused: the
/// consequence of a typo here should be the links this app already knew how to
/// build, not no links at all.
pub fn clean_public_url(text: &str) -> String {
    let text = text.trim().trim_end_matches('/');
    let looks_right = (text.starts_with("https://") || text.starts_with("http://"))
        && text.split("://").nth(1).is_some_and(|rest| {
            let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
            !host.is_empty() && host.contains('.')
        });
    if looks_right { text.to_string() } else { String::new() }
}

/// `frame_cap_fps = 0` means uncapped — the render loop of `DOCUMENTATION.md`
/// §14.1, which is the default and the intended way to run on a desktop.
pub const FRAME_CAP_UNCAPPED: u32 = 0;
/// Bounds for a cap that is set. The floor is where the row animations stop
/// reading as motion; the ceiling is past any display this runs on.
pub const FRAME_CAP_MIN: u32 = 15;
pub const FRAME_CAP_MAX: u32 = 360;
/// What the settings slider offers when the cap is first switched on.
pub const FRAME_CAP_DEFAULT: u32 = 60;

/// Clamp a configured frame cap, preserving the `0` = uncapped sentinel.
pub fn clamp_frame_cap(fps: u32) -> u32 {
    if fps == FRAME_CAP_UNCAPPED {
        FRAME_CAP_UNCAPPED
    } else {
        fps.clamp(FRAME_CAP_MIN, FRAME_CAP_MAX)
    }
}

pub fn get_check_and_set_config(config_path: &Path) -> Config {
    let extracted = read_config(config_path);
    let config = config_from(&extracted);
    write_normalized_config(config_path, &config);
    config
}

/// The settings as they are in `config_path`, creating, normalising and
/// writing nothing — for a look at a setup that is not ours to touch
/// (`taskdeck-server --print-link`, which may run as another user).
pub fn read_config_only(config_path: &Path) -> Config {
    let extracted = fs::read_to_string(config_path).map(|text| parse_config_text(&text)).unwrap_or_default();
    config_from(&extracted)
}

/// Every setting, checked and defaulted, from the file's raw key → value.
fn config_from(extracted: &HashMap<String, String>) -> Config {
    Config {
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
            // `nan` is a float to TOML and to `parse`, and compares false
            // with everything — so the check is for what a size must be, not
            // for what it must not.
            .filter(|v| v.iter().all(|x| x.is_finite() && *x >= 200.0))
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
            .filter(|v| v.iter().all(|x| x.is_finite()))
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
        phone_server_enabled: extracted
            .get("phone_server_enabled")
            .map(|s| parse_config_bool(s))
            .unwrap_or(false),
        phone_server_port: extracted
            .get("phone_server_port")
            .and_then(|n| n.parse::<u16>().ok())
            .filter(|port| *port >= crate::phone::PORT_MIN)
            .unwrap_or(crate::phone::DEFAULT_PORT),
        phone_token: extracted
            .get("phone_token")
            .map(|s| s.trim().to_string())
            .unwrap_or_default(),
        background_blur_percent: extracted
            .get("background_blur_percent")
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(BACKGROUND_BLUR_DEFAULT)
            .min(100),
        background_light_percent: extracted
            .get("background_light_percent")
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(BACKGROUND_LIGHT_DEFAULT)
            .min(100),
        phone_bind_address: extracted
            .get("phone_bind_address")
            .map(|s| clean_bind_address(s))
            .unwrap_or_else(|| crate::phone::DEFAULT_BIND.to_string()),
        phone_public_url: extracted
            .get("phone_public_url")
            .map(|s| clean_public_url(s))
            .unwrap_or_default(),
        frame_cap_fps: extracted
            .get("frame_cap_fps")
            .and_then(|n| n.parse::<u32>().ok())
            .map(clamp_frame_cap)
            .unwrap_or(FRAME_CAP_UNCAPPED),
        server_url: extracted
            .get("server_url")
            .map(|s| s.trim().to_string())
            .unwrap_or_default(),
        server_token: extracted
            .get("server_token")
            .map(|s| s.trim().to_string())
            .unwrap_or_default(),
    }
}

/// Write the settings file the way every other file here is written: whole,
/// through a temporary file renamed into place. A power cut mid-write used to
/// leave an empty file — and an empty file means a fresh token at the next
/// start, which locks every phone and client out.
fn write_atomically(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(dir)?;
    temp.write_all(text.as_bytes())?;
    temp.as_file_mut().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    // And the directory entry, so the rename survives a power cut too (§4.1).
    crate::tasks::sync_directory(dir);
    Ok(())
}

/// Write one key into the settings file, leaving everything else in it —
/// comments, key order, keys we don't own — exactly as it was.
///
/// Public because startup needs it too: `main` can have to correct a setting
/// (a selected colour scheme that had to move id) before there is any `TaskApp`
/// to route it through, and one writer means the two paths cannot disagree
/// about how a value is spelled in TOML.
pub fn write_config_value(
    path: &Path,
    key: &str,
    value: impl Into<toml_edit::Value>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut doc = fs::read_to_string(path)?.parse::<toml_edit::DocumentMut>()?;
    doc[key] = toml_edit::value(value);
    write_atomically(path, &doc.to_string())?;
    Ok(())
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
    doc["background_blur_percent"] = value(config.background_blur_percent as i64);
    doc["background_light_percent"] = value(config.background_light_percent as i64);
    doc["background_image_tint_percent"] = value(config.background_image_tint_percent as i64);
    doc["ui_scale_percent"] = value(config.ui_scale_percent as i64);
    doc["phone_server_enabled"] = value(config.phone_server_enabled);
    doc["phone_server_port"] = value(config.phone_server_port as i64);
    doc["phone_token"] = value(config.phone_token.clone());
    doc["phone_bind_address"] = value(config.phone_bind_address.clone());
    doc["phone_public_url"] = value(config.phone_public_url.clone());
    doc["frame_cap_fps"] = value(config.frame_cap_fps as i64);
    doc["server_url"] = value(config.server_url.clone());
    doc["server_token"] = value(config.server_token.clone());

    let _ = write_atomically(path, &doc.to_string());
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
    /// How blurred the background is, on a 0–100 dial, on both surfaces.
    ///
    /// Not a pixel radius: a radius that reads well on a 720-pixel phone crop
    /// is invisible on a 3000-pixel desktop picture. This is a fraction of the
    /// picture's own width, so one number means the same *look* at any size.
    pub background_blur_percent: u32,
    /// How bright the phone is allowed to let the picture get, on a 0–100 dial.
    ///
    /// The phone solves its darkening against a ceiling on relative luminance,
    /// because text sits directly on the picture there; this is that ceiling.
    /// The desktop's own `background_image_tint_percent` is a different control
    /// for a different problem — a wall calendar's type is a heading.
    pub background_light_percent: u32,
    /// Percentage the whole UI is scaled by, or `UI_SCALE_AUTO` (0) to fit the
    /// layout to the window automatically.
    pub ui_scale_percent: u32,
    /// Whether the phone view (`phone.rs`) is served while the app runs.
    pub phone_server_enabled: bool,
    /// Port it listens on. `phone::PORT_MIN` or above.
    pub phone_server_port: u16,
    /// The secret in the phone's link. Empty until `main` mints one, which
    /// happens once and is then kept, so the link on the phone keeps working.
    pub phone_token: String,
    /// The address the phone view listens on: `phone::DEFAULT_BIND` (this
    /// machine only) unless the file names another, which then serves alone.
    /// Not on the settings sheet — a posture decided once, in the file.
    pub phone_bind_address: String,
    /// The address to *hand out*, when it is not the one to listen on.
    ///
    /// Empty by default, and then the links are built from this machine's own
    /// addresses. Set it when something in front of the server owns the name a
    /// phone should use — `tailscale serve`, a reverse proxy, a real domain —
    /// and the printed links and the QR code use it verbatim. Without it, a
    /// Tailscale setup hands out a bare `http://100.x.y.z:7373`, which works
    /// but arrives with a browser warning and no offline shell, because it is
    /// not a secure context.
    pub phone_public_url: String,
    /// Frames per second the render loop is held to while awake, or
    /// `FRAME_CAP_UNCAPPED` (0) for the flat-out loop of §14.1. For laptops on
    /// battery; see `App::schedule_next_frame`.
    pub frame_cap_fps: u32,
    /// A `taskdeck-server` to keep the board on — `http://host:port` — and
    /// its key. Empty means the board lives here. See `sync.rs`, `SERVER.md`.
    pub server_url: String,
    pub server_token: String,
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
    /// When the next frame is due under a frame cap, if one is set and the
    /// loop is waiting for it. See `schedule_next_frame`.
    redraw_at: Option<Instant>,
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
            redraw_at: None,
            window_size_startup,
            selected_monitor_name,
        }
    }

    /// Ask for the frame after the one that began at `frame_start`.
    ///
    /// Uncapped — the default, and §14.1's deliberate choice — that is at
    /// once, and the loop renders as fast as the GPU allows while awake. With a
    /// cap set, the request is deferred until the frame's share of a second has
    /// passed: the event loop is told to wake at that instant
    /// (`ControlFlow::WaitUntil`) and `new_events` asks for the redraw when it
    /// does. Frames still arrive continuously, only slower, so the hand-rolled
    /// animations — which advance by measured `dt`, not by frame count — are
    /// untouched; what changes is that a laptop on battery is not asked to draw
    /// a calendar two hundred times a second.
    ///
    /// Input still draws immediately: the `CursorMoved` and friends arms call
    /// `handle_redraw` themselves, which the cap does not gate. It only paces
    /// the loop that would otherwise chase its own tail.
    fn schedule_next_frame(&mut self, event_loop: &ActiveEventLoop, frame_start: Instant) {
        let Some(window) = self.window.as_ref() else { return };
        let cap = self.task_app.frame_cap_fps();
        if cap == FRAME_CAP_UNCAPPED {
            window.request_redraw();
            return;
        }
        let due = frame_start + time::Duration::from_secs_f64(1.0 / cap as f64);
        if Instant::now() >= due {
            window.request_redraw();
        } else {
            self.redraw_at = Some(due);
            event_loop.set_control_flow(ControlFlow::WaitUntil(due));
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
                let frame_start = Instant::now();
                self.handle_redraw(event_loop);

                if !self.in_sleep {
                    self.schedule_next_frame(event_loop, frame_start);
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

    // A frame cap's timer going off: the frame it deferred is due. Back to
    // plain waiting first, so a wake that turns out to be the last one (the
    // app fell asleep meanwhile) does not leave a stale deadline armed.
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        if !matches!(cause, StartCause::ResumeTimeReached { .. }) {
            return;
        }
        if self.redraw_at.take().is_some() {
            event_loop.set_control_flow(ControlFlow::Wait);
            if !self.in_sleep {
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
        }
    }

    // The weather thread and the phone view's thread both wake the UI this way.
    // Phone commands are served *here* rather than only inside the frame, so a
    // phone edit lands while the window is minimized or asleep — `handle_redraw`
    // returns before running the frame in both of those states. Nothing the
    // commands do needs egui: they go through the same setters a gesture uses,
    // which end in a calendar rebuild and a save.
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: ()) {
        self.task_app.serve_phone_requests();
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
        self.task_app.shutdown_sync();
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
            background_blur_percent: BACKGROUND_BLUR_DEFAULT,
            background_light_percent: BACKGROUND_LIGHT_DEFAULT,
            ui_scale_percent: UI_SCALE_AUTO,
            phone_server_enabled: false,
            phone_server_port: crate::phone::DEFAULT_PORT,
            phone_token: "abc".to_string(),
            phone_bind_address: crate::phone::DEFAULT_BIND.to_string(),
            phone_public_url: String::new(),
            frame_cap_fps: FRAME_CAP_UNCAPPED,
            server_url: String::new(),
            server_token: String::new(),
        }
    }

    #[test]
    fn frame_cap_keeps_the_uncapped_sentinel_and_clamps_the_rest() {
        assert_eq!(clamp_frame_cap(0), FRAME_CAP_UNCAPPED);
        assert_eq!(clamp_frame_cap(1), FRAME_CAP_MIN);
        assert_eq!(clamp_frame_cap(60), 60);
        assert_eq!(clamp_frame_cap(10_000), FRAME_CAP_MAX);
    }

    #[test]
    fn a_bind_address_is_kept_when_it_is_one_and_every_interface_otherwise() {
        assert_eq!(clean_bind_address(" 127.0.0.1 "), "127.0.0.1");
        assert_eq!(clean_bind_address("100.64.0.7"), "100.64.0.7");
        assert_eq!(clean_bind_address("::1"), "::1");
        // Not an address: the phone view still comes up, on this machine,
        // rather than not at all. The default is loopback now — `tailscale
        // serve` reaches it there, so the ordinary setup needs nothing wider,
        // and a fresh install is not on every network it ever joins.
        for bad in ["", "kitchen", "192.168.1", "0.0.0.0:7373"] {
            assert_eq!(clean_bind_address(bad), crate::phone::DEFAULT_BIND, "{bad:?}");
        }

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("userconfig.toml");
        fs::write(&path, "phone_bind_address = \" 100.64.0.7 \"\n").unwrap();
        assert_eq!(get_check_and_set_config(&path).phone_bind_address, "100.64.0.7");
        fs::write(&path, "phone_bind_address = \"kitchen\"\n").unwrap();
        assert_eq!(get_check_and_set_config(&path).phone_bind_address, crate::phone::DEFAULT_BIND);
        fs::write(&path, "phone_server_port = 7373\n").unwrap();
        assert_eq!(get_check_and_set_config(&path).phone_bind_address, crate::phone::DEFAULT_BIND);
        // And the normalised file carries the key from then on.
        assert!(fs::read_to_string(&path).unwrap().contains("phone_bind_address = \"127.0.0.1\""));
    }

    #[test]
    fn an_address_to_hand_out_is_taken_as_given_or_left_empty() {
        // What goes on the QR when something in front of the server owns the
        // name — `tailscale serve`, a proxy, a domain. Taken verbatim: the
        // thing in front chose the scheme, host and port, and guessing at any
        // of them is how a link that looks right stops working.
        assert_eq!(clean_public_url(" https://a-laptop.tailnet.ts.net/ "), "https://a-laptop.tailnet.ts.net");
        assert_eq!(clean_public_url("https://cal.example.com:8443"), "https://cal.example.com:8443");
        // http is allowed here, unlike a calendar subscription: this one is
        // the user's own server on their own network, and they may not have a
        // certificate yet.
        assert_eq!(clean_public_url("http://box.local.example.com"), "http://box.local.example.com");
        // Anything that is not a URL falls back to the links the app already
        // knew how to build, rather than to no links at all.
        for bad in ["", "  ", "a-laptop.tailnet.ts.net", "https://", "ftp://x.example.com", "https://nodots"] {
            assert_eq!(clean_public_url(bad), "", "{bad:?}");
        }

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("userconfig.toml");
        fs::write(&path, "phone_public_url = \"https://a-laptop.tailnet.ts.net/\"\n").unwrap();
        assert_eq!(get_check_and_set_config(&path).phone_public_url, "https://a-laptop.tailnet.ts.net");
        // And it is written back, so the key is discoverable in the file.
        assert!(fs::read_to_string(&path).unwrap().contains("phone_public_url"));
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
    fn a_broken_file_with_a_lone_quote_is_read_not_a_crash() {
        // An unclosed string is the typo that breaks the TOML and sends the
        // file down the line-by-line fallback — where a value that is one
        // quote character used to be sliced as `1..0`. The rest of the file
        // is still read; the broken line is skipped.
        let broken = "phone_token = \"\nselected_colorscheme_id = 4\nbackground = '\n";
        let config = config_from(&parse_config_text(broken));
        assert_eq!(config.selected_colorscheme_id, 4);
        assert_eq!(config.phone_token, "");
        assert_eq!(config.background, "");
    }

    #[test]
    fn a_float_pair_that_is_not_a_number_falls_back() {
        // `nan` and `inf` are floats to TOML and to `parse::<f32>`, and a
        // window of no size or a place at no latitude is nothing to start
        // from.
        let text = "window_size_startup = [nan, 720.0]\ncoordinates = [inf, 24.94]\n";
        let config = config_from(&parse_config_text(text));
        assert_eq!(config.window_size_startup, [1280.0, 720.0]);
        assert_eq!(config.coordinates, [0.0, 0.0]);
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