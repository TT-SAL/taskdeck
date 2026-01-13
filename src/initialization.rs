use egui::Context;
use egui_wgpu::wgpu::{StoreOp};
use egui_wgpu::{wgpu, Renderer, RendererOptions, ScreenDescriptor};
use egui_winit::{ActionRequested, State};
use serde::{Deserialize, Serialize};
use crate::ui::TaskApp;
use wgpu::{Color, ExperimentalFeatures, LoadOp};
use winit::event::WindowEvent;
use winit::platform::windows::{WindowAttributesExtWindows};
use winit::window::{Window, WindowId};
use egui_wgpu::wgpu::SurfaceError;
use std::collections::HashMap;
use std::{fs, time};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalSize};
use winit::event_loop::ActiveEventLoop;
use toml::Value;

/// Reads a TOML config file and extracts all valid values, including arrays.
/// If parsing fails, falls back to line-by-line extraction.
fn read_config(path: &PathBuf) -> HashMap<String, String> {
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

fn text_2_bool_lazy(text: &String) -> bool {
    text.contains("t")
} 

pub fn get_check_and_set_config() -> Config {
    let config_path = PathBuf::from("taskdeck_data").join(PathBuf::from("userconfig.toml"));
    let extracted = read_config(&config_path);

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
            .map(text_2_bool_lazy)
            .unwrap_or(false),
        enable_fps_counter: extracted
            .get("enable_fps_counter")
            .map(text_2_bool_lazy)
            .unwrap_or(false),
        three_day_weather: extracted
            .get("three_day_weather")
            .map(text_2_bool_lazy)
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
            .and_then(|n| n.parse::<usize>().ok().and_then(|x| Some(x.clamp(6, 20000))))
            .unwrap_or(100),
        background_image_tint_percent: extracted
            .get("background_image_tint_percent")
            .and_then(|n| n.parse::<u32>().ok().and_then(|x| Some(x.clamp(1, 100))))
            .unwrap_or(30),
        selected_monitor_name: extracted
            .get("selected_monitor_name")
            .unwrap_or(&"".to_string()).to_string(),
        selected_colorscheme_id: extracted
            .get("selected_colorscheme_id")
            .and_then(|n| n.parse::<u32>().ok().and_then(|x| Some(x.clamp(0, 200000))))
            .unwrap_or(0),
    };

    if let Some(toml_string) = toml::to_string(&config).ok() {
        let _ = fs::write(config_path, toml_string);
    }

    config
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
}

pub struct AppState<'a> {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface_config: wgpu::SurfaceConfiguration,
    pub surface: wgpu::Surface<'a>,
    pub scale_factor: f32,
    pub egui_winit_state: State,
    pub egui_wgpu_renderer: Renderer,
}

impl AppState<'_> {
    async fn new(
        instance: &wgpu::Instance,
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

        let selected_format = wgpu::TextureFormat::Bgra8Unorm;

        let swapchain_format = swapchain_capabilities
            .formats
            .iter()
            .find(|d| **d == selected_format)
            .expect("failed to select proper surface texture format!");

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: *swapchain_format,
            width,
            height,
            present_mode: wgpu::PresentMode::AutoNoVsync,       //Should work on different devices
            desired_maximum_frame_latency: 2,                   //This may need adjusting
            alpha_mode: swapchain_capabilities.alpha_modes[0],
            view_formats: vec![],
        };

        surface.configure(&device, &surface_config);

        let egui_context = Context::default();

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

        let scale_factor = window.scale_factor() as f32;

        Self {
            device,
            queue,
            surface,
            surface_config,
            scale_factor,
            egui_wgpu_renderer,
            egui_winit_state,
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
    instance: wgpu::Instance,
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
        let instance = egui_wgpu::wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        Self {
            cursor_inside_window: false,
            window_is_focused: false,
            instance,
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
        let initial_width = self.window_size_startup[0] as u32;
        let initial_height = self.window_size_startup[1] as u32;

        let _ = window.request_inner_size(PhysicalSize::new(initial_width, initial_height));

        let surface = self
            .instance
            .create_surface(window.clone())
            .expect("Failed to create surface!");

        let state = AppState::new(
            &self.instance,
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

        let screen_descriptor = ScreenDescriptor {
            size_in_pixels: [state.surface_config.width, state.surface_config.height],
            pixels_per_point: state.scale_factor,
        };

        // --- Acquire next surface texture ---
        let surface_texture = match state.surface.get_current_texture() {
            Ok(tex) => tex,
            Err(SurfaceError::Outdated | SurfaceError::Lost) => {
                state.surface.configure(&state.device, &state.surface_config);
                self.window.as_ref().unwrap().request_redraw();
                return;
            }
            Err(SurfaceError::Timeout) => {
                eprintln!("Surface timed out!");
                return;
            }
            Err(SurfaceError::OutOfMemory) => {
                eprintln!("Out of memory!");
                std::process::exit(1);
            }
            Err(_) => return,
        };

        let surface_view = surface_texture.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = state.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        // --- Begin egui frame ---

        state.context().begin_pass(raw_input);

        let is_fullscreen = self
            .window
            .as_ref()
            .and_then(|w| w.fullscreen().map(|_| true))
            .unwrap_or(false);

        let root_id = egui::viewport::ViewportId::ROOT;
        let info = state.egui_winit_state.egui_input_mut().viewports.entry(root_id).or_default();
        info.fullscreen = Some(is_fullscreen);

        let ctx = state.context();
        self.task_app.ui(ctx);

        // --- End frame, get full output ---
        let full_output = state.context().end_pass();

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

        // Tessellate shapes (immutable borrow)
        let ctx = state.context();
        let paint_jobs = ctx.tessellate(full_output.shapes, ctx.pixels_per_point());

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
}

impl ApplicationHandler for App<'_> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = event_loop
            .create_window({
                let available_monitors: Vec<String> = event_loop
                    .available_monitors()
                    .flat_map(|m| m.name())
                    .collect();

                self.task_app.monitor_options = available_monitors;

                let target_monitor = match event_loop
                    .available_monitors()
                    .find(|m| {
                        if let Some(name) = m.name() {
                            name == self.selected_monitor_name
                        } else { false }
                    }) {
                        None => event_loop.available_monitors().nth(0).unwrap(),
                        Some(monitor) => monitor,
                    };

                let monitor_position = target_monitor.position();
                let monitor_size = target_monitor.size();
                let window_size = LogicalSize::new(self.window_size_startup[0], self.window_size_startup[1]);

                let window_position = LogicalPosition::new(
                    monitor_position.x + (monitor_size.width as i32 - window_size.width as i32)/2,
                    monitor_position.y + (monitor_size.height as i32 - window_size.height as i32)/2,
                );

                let window_title = format!("TaskDeck    -   Ver.{}", env!("BUILD_DATE"));

                let embedded_icon_png = image::load_from_memory_with_format(include_bytes!("../icon.png"), image::ImageFormat::Png);
                let (icon_rgba, width, height) = {
                    let image = embedded_icon_png.unwrap().into_rgba8();
                    let (w, h) = image.dimensions();
                    (image.into_raw(), w, h)
                };
                let icon_data = winit::window::Icon::from_rgba(icon_rgba, width, height).unwrap();

                let minimum_size = LogicalSize::new(200.0, 200.0);

                Window::default_attributes()
                    .with_title(window_title)
                    .with_window_icon(Some(icon_data.clone()))
                    .with_taskbar_icon(Some(icon_data))
                    .with_position(window_position)
                    .with_min_inner_size(minimum_size)
                    .with_active(false)
            })
            .unwrap();
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
            WindowEvent::ScaleFactorChanged { scale_factor, mut inner_size_writer } => {
                let physical_size = self.window.as_ref().unwrap().inner_size();

                if let Some(state) = self.state.as_mut() {
                    state.scale_factor = scale_factor as f32;
                    state.resize_surface(physical_size.width, physical_size.height);

                    let ctx = state.context();
                    ctx.set_pixels_per_point(state.scale_factor);
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
            WindowEvent::CursorEntered { device_id } => {
                self.cursor_inside_window = true;
                self.last_active = None;
                self.in_sleep = false;
                self.handle_redraw(event_loop);
                self.window.as_ref().unwrap().request_redraw();
            }
            WindowEvent::CursorMoved { device_id, position } => {
                self.cursor_inside_window = true;
                self.last_active = None;
                self.in_sleep = false;
                self.handle_redraw(event_loop);
                self.window.as_ref().unwrap().request_redraw();
            }
            WindowEvent::CursorLeft { device_id } => {
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
}