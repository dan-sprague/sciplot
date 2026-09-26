# ezviz dependency research (Rust ecosystem as of 2026-09-26)

## 0. Summary

- **GPU and windowing:** use wgpu **30.0.1** with winit **0.30.13**. That is the pair wgpu's own v30 examples use. winit 0.31 is still `0.31.0-beta.3` and changes the API a lot, so avoid it for now.
- **Text:** use **ab_glyph 0.2.32**. It rasterises coverage bitmaps at any size, gives glyph outlines for SVG, reads CFF `.otf` files, and has a small dependency tree.
  - Its kerning only reads the old `kern` table. That does not matter here: the font Makie actually ships (TeXGyreHerosMakie) has no `kern`/GPOS/GSUB tables, and Makie's layout never kerns (`x += hadvance`).
- **Existing crates:** none already does what ezviz aims to do (Makie object model + GPU + interactive + SVG/PNG).
  - Closest in spirit: **ruviz** (CPU tiny-skia, one builder chain, no Figure/Axis model) and **pluot** (wgpu layers that render to both pixels and SVG, experimental).
  - Best interaction ideas: **egui_plot** and **rerun**.
- **Font:** embed Makie's **TeXGyreHerosMakie-{Regular,Bold}.otf** (~171 KB each). The GUST Font License is legally LPPL‑1.3c, which is OSI-approved. Embedding and redistributing unmodified files is fine; renaming is only *requested* if you modify them.
- **Offline:** wgpu, winit, pollster, ab_glyph/ttf-parser 0.25, ndarray and nalgebra are **not** in the local cargo cache. The first build needs network access. png 0.18.1 plus all its dependencies, bytemuck 1.25.2 and base64 0.23.1 are cached.

---

## 1. Dependency table (pin these)

| crate | version (release date) | features | in local cache? | notes |
|---|---|---|---|---|
| wgpu | **30.0.1** (2026-08-22) | `default-features=false, features=["std","parking_lot","wgsl","metal","vulkan","dx12"]` (drops gles/webgpu) | no | MSRV 1.87 for the `wgpu` crate itself (repo/examples need 1.93). |
| winit | **0.30.13** (2026-03-02) | default (`rwh_06`, x11, wayland…). On macOS-only builds, `default-features=false, features=["rwh_06"]` works. | no | 0.31.0-beta.3 (2026-09-04) exists; don't use it yet. |
| raw-window-handle | 0.6.2 (transitive) | – | **yes** | Shared by wgpu 30 and winit 0.30. |
| pollster | **1.0.1** (2026-07-10) | – | no | `pollster::block_on` is unchanged. wgpu's examples still use 0.4, which is also fine. |
| bytemuck | **1.25.2** | `derive` | **yes** (bytemuck_derive is **not** cached) | |
| png | **0.18.1** (2026-02-14) | default | **yes**, with all its deps (bitflags 2.13.2, crc32fast, fdeflate 0.3.7, flate2 1.1.10, miniz_oxide 0.8.9) | Use this instead of `image`. |
| ab_glyph | **0.2.32** (2025-09-28) | `default-features=false, features=["std"]` (drops variable-font/gvar code) | no (cache has ttf-parser 0.20, but 0.25.1 is needed) | Pulls ab_glyph_rasterizer 0.1.10, owned_ttf_parser 0.25.1, ttf-parser 0.25.1. |
| base64 (optional) | **0.23.1** (2026-08-04) | – | **yes** | For `data:image/png;base64` heatmaps in SVG; easy to hand-roll instead. |
| ndarray (optional) | **0.17.2** (2026-01-10) | default | no | 0.17 adds `ArrayRef`: accept `&ndarray::ArrayRef<f64, Ix2>`. |
| nalgebra (optional) | **0.35.0** (2026-05-24) | default | no | MSRV 1.89. `DMatrix` is column-major like Julia. |
| log | 0.4.34 | – | **yes** | wgpu logs quietly since v28; use `RUST_LOG=wgpu=warn`. |
| image | 0.25.10 | – | yes | **Not needed.** |

Other versions checked: glam 0.33.10, crossbeam-channel 0.5.17 (std `mpsc` is enough), flume 0.12.0, tiny-skia 0.12.0, resvg 0.48.1, kurbo 0.13.1, lyon_tessellation 1.0.22.

---

## 2. Code sketches (checked against wgpu v30.0.1 source and winit v0.30.13 source)

### 2.1 Cargo.toml
```toml
[package]
name = "ezviz"
version = "0.1.0"
edition = "2024"
rust-version = "1.88"          # bump to 1.89 if the nalgebra feature is on
license = "(MIT OR Apache-2.0) AND LPPL-1.3c"   # LPPL-1.3c covers the embedded GUST fonts (or put fonts in a separate crate)

[features]
default = ["window"]
window = ["dep:winit"]
ndarray = ["dep:ndarray"]
nalgebra = ["dep:nalgebra"]

[dependencies]
wgpu = { version = "30.0.1", default-features = false, features = ["std", "parking_lot", "wgsl", "metal", "vulkan", "dx12"] }
winit = { version = "0.30.13", optional = true }
pollster = "1.0.1"
bytemuck = { version = "1.25.2", features = ["derive"] }
png = "0.18.1"
ab_glyph = { version = "0.2.32", default-features = false, features = ["std"] }
base64 = "0.23.1"
log = "0.4"
ndarray = { version = "0.17.2", optional = true }
nalgebra = { version = "0.35.0", optional = true }
```

### 2.2 Shared GPU context (works headless or with windows)
```rust
pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl Gpu {
    pub fn new() -> Self {
        // v29+: InstanceDescriptor no longer has Default. Metal/Vulkan/DX12 ignore the display
        // handle, so one windowless Instance can serve both headless export and windows.
        // (Wayland+GLES would need new_with_display_handle(Box::new(event_loop.owned_display_handle())).)
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .expect("no GPU adapter"); // returns Result<Adapter, RequestAdapterError>
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("ezviz"),
            required_limits: adapter.limits(), // default Limits cap 2D textures at 8192; M3 supports 16384
            ..Default::default()               // required_features, experimental_features, memory_hints, trace
        }))
        .expect("request_device");
        Self { instance, adapter, device, queue }
    }
}
```

### 2.3 Window, resize and render loop (`ApplicationHandler`)
```rust
use std::{cell::RefCell, sync::Arc, time::Duration};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalSize},
    event::{MouseScrollDelta, StartCause, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    platform::{pump_events::{EventLoopExtPumpEvents, PumpStatus}, run_on_demand::EventLoopExtRunOnDemand},
    window::{Window, WindowId},
};

#[derive(Debug, Clone, Copy)]
pub enum UserEvent { Redraw }

pub struct Screen {
    surface: wgpu::Surface<'static>, // declared first so it drops before the window
    window: Arc<Window>,
    config: wgpu::SurfaceConfiguration,
}

pub struct App {
    pub gpu: Arc<Gpu>,
    pub want_window: bool,
    pub exit_on_close: bool,               // true for a blocking show(); false when pumping
    pub screen: Option<Screen>,
    pub cursor_units: Option<(f64, f64)>,  // figure units (CSS px), bottom-left origin like Makie
}

impl App {
    fn ensure_window(&mut self, el: &ActiveEventLoop) {
        if !self.want_window || self.screen.is_some() { return; }
        let attrs = Window::default_attributes()
            .with_title("ezviz")
            .with_inner_size(LogicalSize::new(600.0, 450.0)); // Makie `size` is logical (CSS) px
        let window = Arc::new(el.create_window(attrs).expect("create_window"));
        // On macOS, create_surface panics if not called on the main thread.
        let surface = self.gpu.instance.create_surface(window.clone()).expect("create_surface");
        let caps = surface.get_capabilities(&self.gpu.adapter);
        // A non-sRGB format blends in gamma space, like GLMakie (RGBA8) and Cairo.
        let format = caps.formats.iter().copied().find(|f| !f.is_srgb()).unwrap_or(caps.formats[0]);
        let PhysicalSize { width, height } = window.inner_size(); // physical px = logical * scale_factor
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto, // new, required field in v30
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        };
        surface.configure(&self.gpu.device, &config);
        window.request_redraw();
        self.screen = Some(Screen { surface, window, config });
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn new_events(&mut self, el: &ActiveEventLoop, _c: StartCause) { self.ensure_window(el); } // later windows in pump mode
    fn resumed(&mut self, el: &ActiveEventLoop) { self.ensure_window(el); }                    // once per run
    fn user_event(&mut self, _el: &ActiveEventLoop, ev: UserEvent) {
        match ev { UserEvent::Redraw => if let Some(s) = &self.screen { s.window.request_redraw() } }
    }
    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if matches!(event, WindowEvent::CloseRequested) {
            self.screen = None; // drop surface + window before returning (needed for run_app_on_demand)
            self.want_window = false;
            if self.exit_on_close { el.exit(); }
            return;
        }
        let gpu = &self.gpu;
        let Some(s) = self.screen.as_mut() else { return };
        match event {
            WindowEvent::Resized(size) => {
                s.config.width = size.width.max(1);
                s.config.height = size.height.max(1);
                s.surface.configure(&gpu.device, &s.config);
                s.window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => { /* px_per_unit = s.window.scale_factor(); a Resized follows */ }
            WindowEvent::CursorMoved { position, .. } => {           // PhysicalPosition<f64>, top-left origin
                let sf = s.window.scale_factor();
                let h = s.config.height as f64 / sf;
                self.cursor_units = Some((position.x / sf, h - position.y / sf));
                s.window.request_redraw();                            // hover readout
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let _zoom_steps = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y as f64,                              // mouse wheel
                    MouseScrollDelta::PixelDelta(p) => p.y / s.window.scale_factor() / 16.0,    // trackpad, physical px
                };
                s.window.request_redraw();
            }
            // Also available: MouseInput, ModifiersChanged, PinchGesture (macOS trackpad), KeyboardInput
            WindowEvent::RedrawRequested => render(gpu, s),
            _ => {}
        }
    }
}

fn render(gpu: &Gpu, s: &mut Screen) {
    let frame = match s.surface.get_current_texture() {          // v29+: an enum, not a Result
        wgpu::CurrentSurfaceTexture::Success(f) => f,
        wgpu::CurrentSurfaceTexture::Suboptimal(f) => {
            drop(f); s.surface.configure(&gpu.device, &s.config); s.window.request_redraw(); return;
        }
        wgpu::CurrentSurfaceTexture::Outdated => {
            s.surface.configure(&gpu.device, &s.config); s.window.request_redraw(); return;
        }
        wgpu::CurrentSurfaceTexture::Lost => {
            s.surface = gpu.instance.create_surface(s.window.clone()).expect("recreate surface");
            s.surface.configure(&gpu.device, &s.config); s.window.request_redraw(); return;
        }
        wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => return, // e.g. hidden window on macOS
        wgpu::CurrentSurfaceTexture::Validation => panic!("surface validation error"),
    };
    let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut enc = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
    {
        let _pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("figure"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::WHITE), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,                                   // v28+
        });
        // scene.draw(&mut _pass, px_per_unit = s.window.scale_factor() as f32);
    }
    gpu.queue.submit([enc.finish()]);
    s.window.pre_present_notify();
    gpu.queue.present(frame);                                      // v30: replaces frame.present()
}
```

### 2.4 Hosting the event loop: blocking `show()` and a non-blocking pump
```rust
// winit 0.30 allows ONE EventLoop per process, created on the main thread; a second
// build() returns Err(RecreationAttempt). EventLoop is !Send, so keep it in a main-thread thread_local.
thread_local! {
    static EVENT_LOOP: RefCell<Option<EventLoop<UserEvent>>> = const { RefCell::new(None) };
}
fn with_event_loop<R>(f: impl FnOnce(&mut EventLoop<UserEvent>) -> R) -> R {
    EVENT_LOOP.with_borrow_mut(|slot| {
        let el = slot.get_or_insert_with(|| {
            EventLoop::<UserEvent>::with_user_event().build()
                .expect("EventLoop must be created on the main thread, once per process")
        });
        f(el)
    }) // never call this from inside a handler callback: the RefCell would panic on a double borrow
}

/// Create before running; Send + Clone (UserEvent: Send). Lets a simulation thread wake the UI.
pub fn proxy() -> EventLoopProxy<UserEvent> { with_event_loop(|el| el.create_proxy()) }

/// Blocking show, like GLMakie `wait(display(fig))`. Can be called repeatedly.
pub fn show_blocking(app: &mut App) {
    app.want_window = true;
    app.exit_on_close = true;
    with_event_loop(|el| {
        el.set_control_flow(ControlFlow::Wait);   // idle = 0% CPU; redraw only when requested
        el.run_app_on_demand(app).expect("run_app_on_demand");
    });
}

/// Non-blocking: call from the simulation loop. Returns false once the user closed the window.
pub fn pump(app: &mut App, dirty: bool) -> bool {
    app.exit_on_close = false;                    // don't call el.exit() in pump mode
    if dirty { if let Some(s) = &app.screen { s.window.request_redraw(); } } // render happens inside the callback
    let alive = with_event_loop(|el| {
        el.set_control_flow(ControlFlow::Wait);
        matches!(el.pump_app_events(Some(Duration::ZERO), app), PumpStatus::Continue)
    });
    alive && app.want_window
}

// Usage A (sim on the main thread):
//   let mut app = App { gpu, want_window: true, exit_on_close: false, screen: None, cursor_units: None };
//   let mut last = Instant::now();
//   loop { sim.step(); hm.set(&sim.n);
//          if last.elapsed() >= Duration::from_millis(16) { last = Instant::now(); if !pump(&mut app, true) { break } } }
// Usage B (sim on a worker thread):
//   let p = proxy(); spawn(move || loop { step(); publish(); if p.send_event(UserEvent::Redraw).is_err() { break } });
//   show_blocking(&mut app);
```

### 2.5 Headless render to a texture, readback, PNG
```rust
pub fn render_png(gpu: &Gpu, width: u32, height: u32, px_per_unit: f32, path: &std::path::Path)
    -> Result<(), Box<dyn std::error::Error>>
{
    let format = wgpu::TextureFormat::Rgba8Unorm; // RGBA byte order, gamma-space blending
    let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("offscreen"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());

    let unpadded = width * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT; // 256; applies to texture<->buffer copies only
    let padded = unpadded.div_ceil(align) * align;
    let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: padded as u64 * height as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut enc = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    {
        let _pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("figure"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view, depth_slice: None, resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::WHITE), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
        });
        // scene.draw(&mut _pass, px_per_unit);
    }
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(height) },
        },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );
    let idx = gpu.queue.submit([enc.finish()]);

    let slice = buf.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| { let _ = tx.send(r); }); // FnOnce(Result<(), BufferAsyncError>) + Send + 'static
    gpu.device.poll(wgpu::PollType::Wait { submission_index: Some(idx), timeout: None })?; // -> Result<PollStatus, PollError>
    rx.recv()??;
    let mut rgba = Vec::with_capacity((unpadded * height) as usize);
    {
        let data = slice.get_mapped_range()?;   // v30: Result<BufferView, MapRangeError>
        for row in data.chunks_exact(padded as usize) { rgba.extend_from_slice(&row[..unpadded as usize]); }
    }
    buf.unmap();

    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut pe = png::Encoder::new(file, width, height);
    pe.set_color(png::ColorType::Rgba);
    pe.set_depth(png::BitDepth::Eight);
    pe.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    let ppm = (96.0 * px_per_unit / 0.0254).round() as u32; // 1 unit = 1 CSS px = 1/96 in
    pe.set_pixel_dims(Some(png::PixelDimensions { xppu: ppm, yppu: ppm, unit: png::Unit::Meter }));
    let mut w = pe.write_header()?;
    w.write_image_data(&rgba)?;
    w.finish()?;
    Ok(())
}
```

### 2.6 Pipeline creation with the v28–v30 field changes
```rust
let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
    label: None,
    bind_group_layouts: &[Some(&bgl)],   // v29: Option<&BindGroupLayout>
    immediate_size: 0,                   // v28: replaces push_constant_ranges
});
let inst = wgpu::VertexBufferLayout {
    array_stride: 8, step_mode: wgpu::VertexStepMode::Instance,
    attributes: &wgpu::vertex_attr_array![0 => Float32x2],
};
let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
    label: Some("scatter"),
    layout: Some(&pl),
    vertex: wgpu::VertexState {
        module: &shader, entry_point: Some("vs"), compilation_options: Default::default(),
        buffers: &[Some(inst)],          // v30: &[Option<VertexBufferLayout>] (owned, not &layout)
    },
    fragment: Some(wgpu::FragmentState {
        module: &shader, entry_point: Some("fs"), compilation_options: Default::default(),
        targets: &[Some(wgpu::ColorTargetState {
            format, blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL,
        })],
    }),
    primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
    depth_stencil: None,
    multisample: wgpu::MultisampleState::default(),
    multiview_mask: None,                // v28: was `multiview`
    cache: None,
});
```

---

## 3. Current API shapes in brief

### wgpu 30.0.1
- **Instance:** `Instance::new(desc)` takes the descriptor **by value** (v27 took a reference).
  - `InstanceDescriptor::{new_with_display_handle(Box<dyn WgpuHasDisplayHandle>), new_without_display_handle(), *_from_env()}`.
  - `Instance::default()` still exists and means "no display handle".
- **request_adapter / request_device:**
  - `request_adapter(&RequestAdapterOptions)` returns a future of `Result<Adapter, RequestAdapterError>`. Options now include `apply_limit_buckets` (default false).
  - `request_device(&DeviceDescriptor)` returns a future of `Result<(Device, Queue), RequestDeviceError>`. The descriptor fields are `label, required_features, required_limits, experimental_features, memory_hints, trace`, and it implements `Default`.
- **Surface:**
  - `create_surface(impl Into<SurfaceTarget<'w>>)`: an `Arc<Window>` gives `Surface<'static>`.
  - `get_current_texture()` returns `CurrentSurfaceTexture`; `SurfaceError` has been removed.
  - Present with `queue.present(frame)`.
  - `get_default_config(&adapter, w, h)` and `get_configuration()` are available.
- **SurfaceConfiguration:** `usage, format, color_space, width, height, present_mode, desired_maximum_frame_latency, alpha_mode, view_formats: Vec<_>`.
- **Render pass:**
  - `RenderPassDescriptor { label, color_attachments, depth_stencil_attachment, timestamp_writes, occlusion_query_set, multiview_mask }`.
  - `RenderPassColorAttachment { view, depth_slice, resolve_target, ops }`.
- **Copy types:** `TexelCopyTextureInfo`, `TexelCopyBufferInfo`, `TexelCopyBufferLayout { offset, bytes_per_row: Option<u32>, rows_per_image: Option<u32> }`, and `wgpu::COPY_BYTES_PER_ROW_ALIGNMENT = 256`.
- **Polling:** `Device::poll(PollType) -> Result<PollStatus, PollError>`.
  - `PollType::Wait { submission_index: Option<SubmissionIndex>, timeout: Option<Duration> }`, `PollType::Poll`, and `PollType::wait_indefinitely()`.
- **Mapping:**
  - `BufferSlice::map_async(MapMode, FnOnce(Result<(), BufferAsyncError>))` and also `Buffer::map_async(mode, range, cb)`.
  - `get_mapped_range()` returns `Result`.
  - Write mappings return `WriteOnly<[u8]>`: use `.slice(a..b)` instead of indexing.
  - `CommandEncoder::map_buffer_on_submit` is available (v27+).

### winit 0.30.13
- **ApplicationHandler<T = ()>:**
  - Required: `resumed(&mut self, &ActiveEventLoop)` and `window_event(&mut self, &ActiveEventLoop, WindowId, WindowEvent)`.
  - Provided: `new_events`, `user_event(&mut self, &ActiveEventLoop, T)`, `about_to_wait`, `suspended`, `exiting`, `device_event`, `memory_warning`.
- **Creating and running:**
  - Build with `EventLoop::<T>::with_user_event().build()`, or `EventLoop::new()` for `()`.
  - Run with `run_app(self, &mut A)` (consumes the loop), `EventLoopExtRunOnDemand::run_app_on_demand(&mut self, &mut A)`, or `EventLoopExtPumpEvents::pump_app_events(&mut self, Option<Duration>, &mut A) -> PumpStatus::{Continue, Exit(i32)}`.
  - Both run-on-demand and pump are supported on Windows, Linux, macOS and Android; neither works on iOS or Web.
- **Waking from other threads:** `EventLoopProxy<T>` is Clone + Send + Sync. `send_event(T) -> Result<(), EventLoopClosed<T>>`.
- **Windows:** create them from `ActiveEventLoop::create_window(Window::default_attributes()...)`; `EventLoop::create_window` is deprecated.
- **HiDPI:**
  - `Window::scale_factor()` is 2.0 on Retina.
  - `inner_size()` is physical.
  - `WindowEvent::ScaleFactorChanged { scale_factor, inner_size_writer }` fires, followed by `Resized`.
  - `CursorMoved { position: PhysicalPosition<f64> }` is in physical pixels.
- **Control flow:** use `ControlFlow::Wait` for plots and redraw on demand. `Poll` spins the CPU. `WaitUntil(Instant)` gives throttled animation.
- **winit 0.31 (beta):** `can_create_surfaces` replaces `resumed` for window creation, there is no user-event generic (`proxy_wake_up` + `EventLoopProxy::wake_up()` instead), and handlers receive `&dyn ActiveEventLoop`. Plan to migrate once it is stable.

---

## 4. Text crates for our four needs

| crate (version) | (a) coverage raster into own atlas | (b) outlines for SVG | (c) advances / kerning | (d) CFF `.otf` | weight | verdict |
|---|---|---|---|---|---|---|
| **ab_glyph 0.2.32** | yes: `font.outline_glyph(glyph)?.draw(\|x,y,c\|…)` at any `PxScale`, sub-pixel position | yes: `Font::outline(id) -> Option<Outline>`, curves are `Line/Quad/Cubic` in font units, implicit closes added | `h_advance_unscaled`, `ascent/descent_unscaled`. Kerning reads the legacy `kern` table only (no GPOS). | yes (ttf-parser 0.25 handles CFF/CFF2) | tiny | **Recommended** |
| fontdue 0.9.4 | yes, fast | **no public outline API** | metrics + `horizontal_kern` | via ttf-parser 0.25 | tiny | Fails (b) |
| swash 0.2.10 | yes (zeno), hinting | yes (scaled outlines) | full shaping including GPOS | yes | medium; pins skrifa ≤0.44 | Capable, more than we need |
| skrifa 0.47.0 (+read-fonts 0.44) | **no rasteriser** (add zeno or your own) | yes (`OutlinePen`, hinting) | advances yes; no shaping (harfrust 0.13.3 would be needed) | yes | medium; frequent 0.x breaks | Good base, more assembly |
| cosmic-text 0.19.0 | via swash cache | indirect | full (harfrust, bidi, system fonts via fontdb) | yes | heavy | Overkill |
| parley 0.11.1 | none (pair with a renderer) | via skrifa | full rich text (fontique, ICU) | yes | heavy | Overkill |
| glyphon 0.12.0 (wgpu ^30) | owns its own atlas and pipeline | no | via cosmic-text | yes | heavy | Doesn't fit (b) or give pipeline control |

Why ab_glyph is enough:
- Makie's `text_layouting.jl` does `x += hadvance*scale`, with no kerning.
- TeXGyreHerosMakie-Regular.otf has tables `CFF, OS/2, cmap, head, hhea, hmtx, maxp, name, post` only: no kern, GPOS or GSUB.
- Its metrics: unitsPerEm 1000, ascender 947, descender −218, 1054 glyphs.

If real kerning or shaping is ever needed, move to skrifa + harfrust or to swash.

---

## 5. Existing Rust plotting crates

None of these provides a Makie-style Figure/GridLayout/Axis model, GPU rendering, interaction with hover readout, live Observable-style updates and matching PNG+SVG output all together.

| crate | latest | good at | lacks | worth borrowing |
|---|---|---|---|---|
| plotters | 0.3.7 (2024-09-08; no release in 2 years, repo pushed 2026-04) | many backends, static PNG/SVG, wasm | wordy builder chains, CPU only, no real interactivity or layout alignment | little |
| egui_plot | 0.37.0 (2026-08-05; "looking for maintainer") | immediate-mode pan, zoom, box-zoom, linked axes, hover labels; has `Heatmap`, `PlotImage`, `Legend` | needs an egui app, no publication export, no figure layout; log axes not advertised | interaction semantics, linked-axis groups, hover formatter |
| rerun | 0.38.1 (2026-09-16) | streaming live data; images/tensors with colormaps and hover pixel readout; time scrubbing | separate viewer, heavy, not a figure or publication tool | hover pixel-value UX, streaming/coalesced updates |
| plotly (plotly.rs) | 0.14.1 | rich interactive HTML in a browser | not native, not live; static export needs WebDriver + Chrome/Firefox (`plotly_static`) or legacy Kaleido | nothing core |
| charming | 0.6.0 (2025-06) | ECharts output, many chart types | HTML/JS; images need heavy SSR | – |
| poloto | 19.1.2 (2023-07, stale) | CSS-styled SVG | small feature set | – |
| textplots | 0.8.7 | terminal plots | – | – |
| **ruviz** | 0.14.2 (2026-09-17) | Makie-*claimed* performance with matplotlib ease: 29 plot types, PNG/SVG/PDF, themes, Typst math, `interactive` feature (winit + softbuffer) | 2D is CPU (tiny-skia; wgpu only for 3D/"gpu" feature); **one builder chain with no Figure/Axis objects**; no protrusion-aware GridLayout or Observables | extension-dispatched `.save()`, ndarray/nalgebra input traits, per-series setter ordering |
| **pluot** | 0.1.23 (2026-09-25, experimental) | wgpu layers rendering to **pixels or an SVG string from the same layer**, decoupled from any windowing system; Zarr tiling | young; aimed at multiple language bindings | confirms our dual GPU/SVG backend split |
| kuva | 0.5.0 (2026-08) | SVG-first, 64 plot types, CLI and terminal output | no GPU or interactivity | – |
| starsight | 0.3.3 (2026-05) | "figure compiler", `plot!` macro, tiny-skia/SVG | GPU and interactivity only on its roadmap | macro one-liner ergonomics |
| rsplot | 0.5.6 (2026-07) | silx port on egui + wgpu: GPU colormapped images, min/max curve decimation | egui-bound, API still moving | min/max decimation for 1M-point lines |
| iced_plot 0.5.0 / runmat-plot 0.6.2 / lumen-charts 2.0.2 | GPU widgets for iced, a MATLAB-like runtime, Vello-based finance charts | not general scientific figures | – |
| ChartGPU (JS, not Rust) | 1M points: LTTB downsampling in a compute shader, one instanced draw per series | – | the decimation and instancing ideas |

---

## 6. Font: TeX Gyre Heros

- **Licence:**
  - The GUST Font License is "legally identical to LPPL 1.3c or later". The SPDX id is `LPPL-1.3c`, which is OSI-approved.
  - It adds one *non-binding* request: rename derived fonts and list them in a `MANIFEST-<id>.txt`.
  - Embedding with `include_bytes!` and redistributing unmodified files is allowed. Ship the license text alongside.
  - Put `(MIT OR Apache-2.0) AND LPPL-1.3c` in `license`, or isolate the fonts in an `ezviz-fonts` crate.
  - If you subset or modify the files, rename them (Makie already renamed its version "TeX Gyre Heros Makie").
- **Use Makie's version (local path `~/.julia/artifacts/ad4e594b35357bcfafa2ed97db3137382a3f09bb/fonts/`):**
  - Files and sizes: `TeXGyreHerosMakie-Regular.otf` 170,996 B, `-Bold` 172,208, `-Italic` 177,112, `-BoldItalic` 178,612.
  - It is "TeX Gyre Heros with slightly decreased descenders and ascenders" (Makie PR #1897); the licence text is in `LICENSES.md` in that directory.
  - Makie's defaults: Regular for labels and ticks, Bold for titles. v1 only needs Regular and Bold (~343 KB).
  - The CTAN originals are about 134–139 KB each. They have different vertical metrics and include GPOS, so they would not match Makie's layout.
- **Glyph coverage of Regular:**
  - Present: U+2212 minus, Greek (49 codepoints), × ° ± ≈ ≤ √ ∑ ∞ ∂ ·, and superscripts ² ¹.
  - Missing: ⁻ ⁴ (most of the superscript block), ∇, ℏ, ∫, and thin space U+2009.
  - That is why Makie's `tick_format.jl` draws log-tick exponents as rich-text **superscript spans** (smaller size plus a baseline offset), not Unicode superscripts. ezviz needs the same rich-text span support.
- **Fallbacks:**
  - Liberation Sans or Arimo: OFL-1.1 / Apache-2.0, Helvetica-metric-compatible.
  - DejaVu Sans (757 KB; Makie's old default; broad math and Greek coverage) or Noto Sans Math (OFL) as a symbol fallback.
  - Avoid Nimbus Sans (AGPL/GPL) in a permissively licensed crate.

---

## 7. Local cargo cache (offline availability)

- **Present:**
  - Direct dependencies: `bytemuck-1.25.2`, `png-0.17.16`, `png-0.18.1`, `raw-window-handle-0.6.2`, `base64-0.21.7/0.22.1/0.23.1`, `log-0.4.34`.
  - All of png 0.18.1's dependencies: `fdeflate-0.3.7`, `flate2-1.1.10`, `crc32fast-1.5.2`, `miniz_oxide-0.8.9/0.9.1`, `simd-adler32-0.3.10`, `bitflags-2.13.2`.
  - General crates: `image-0.24.9/0.25.10`, `crossbeam-channel-0.5.17`, `flume-0.11.1`, `parking_lot-0.12.5`, `smallvec-1.16.1`, `thiserror-2.0.20`, `arrayvec-0.7.8`, `hashbrown-0.15.5/0.17.1`, `indexmap-2.14.2`, `rustc-hash-2.1.3`, `dpi-0.1.2`.
  - Apple bindings: `objc2-0.6.4`, `objc2-foundation/app-kit/core-graphics/core-foundation-0.3.2`, `block2-0.6.2`, `dispatch2-0.3.1`.
  - Font and plotting crates: `ttf-parser-0.20.0` (too old), `font-kit-0.14.3`, `plotters-0.3.7`, `plotters-svg-0.3.7`.
- **Absent (need download):**
  - wgpu, wgpu-core/hal/types, naga.
  - winit 0.30 and its objc2 0.5 / objc2-app-kit 0.2.
  - objc2-metal and objc2-quartz-core.
  - pollster, bytemuck_derive, ab_glyph, owned_ttf_parser, ttf-parser 0.25.
  - ndarray, nalgebra, and every other text crate.
- The cache has no sparse-index entries for wgpu or winit, so it has never resolved them.

---

## 8. Version-specific gotchas

**wgpu 30.0.1**
1. **`InstanceDescriptor` has no `Default`** (v29). Use `new_without_display_handle()` or `new_with_display_handle(Box::new(el.owned_display_handle()))`. If you pass a display, every surface's display must be the same one, or `create_surface` panics.
2. **On macOS, `create_surface` panics off the main thread.** Headless device creation and rendering work on any thread.
3. **Getting and presenting frames changed:**
   - `get_current_texture()` returns the `CurrentSurfaceTexture` enum (v29): reconfigure on `Suboptimal`/`Outdated`, skip on `Occluded`/`Timeout`, recreate the surface on `Lost`.
   - Present with `queue.present(frame)` (v30); `SurfaceTexture::present` is gone.
   - Call `window.pre_present_notify()` first.
4. **New or changed descriptor fields since v28:**
   - `SurfaceConfiguration.color_space` (v30).
   - `VertexState.buffers: &[Option<VertexBufferLayout>]` (v30): wrap the owned layout in `Some`, not `&layout`.
   - `bind_group_layouts: &[Option<&BGL>]` (v29).
   - `immediate_size` replaces push constants (v28; `set_immediates`, WGSL `var<immediate>`).
   - `multiview_mask` on both the pipeline and render-pass descriptors (v28).
   - `depth_slice: None` on color attachments.
   - `DepthStencilState.depth_write_enabled/depth_compare` are now `Option` (v29).
   - `SamplerDescriptor.mipmap_filter` takes a `MipmapFilterMode` (v28).
5. **WGSL change (v30):** integer varyings must be marked `@interpolate(flat)` explicitly, e.g. a marker-shape id.
6. **Error and mapping API changes:**
   - `get_mapped_range()` returns a `Result` (v30).
   - Write mappings are `WriteOnly<[u8]>` (v29).
   - `Device::poll` returns a `Result`.
   - Error scopes are guards: `let g = device.push_error_scope(..); g.pop().await` (v28).
   - `enumerate_adapters` is async (v28).
   - `dispatch` was renamed `dispatch_workgroups` (v30).
7. **Row alignment:** `COPY_BYTES_PER_ROW_ALIGNMENT` (256) applies only to buffer↔texture copies. `queue.write_texture` (heatmap uploads) needs no padding.
8. **Limits:** default `Limits` cap 2D textures at 8192. Request `adapter.limits()` (M3: 16384); larger heatmaps need tiling. Default storage binding is 128 MiB, which covers 1M `vec2<f32>` (8 MB) easily.
9. **Float textures:** `R32Float` can't be linearly sampled without `Features::FLOAT32_FILTERABLE` (Apple GPUs support it). For Makie's default `interpolate=false`, use `textureLoad`; or use `R16Float`, which is filterable by default. Keep the colormap in an `Rgba8Unorm` 1D/2D texture.
10. **No wide lines or points, no geometry shaders.** `LineList` and `PointList` are 1 px only, and WGSL has no geometry stage. Port GLMakie's `lines.geom`/`scatter` expansion as instanced quads, using storage-buffer vertex pulling or an instance buffer bound twice at +8 bytes for segment endpoints.
11. **Colour and blending:** use a **non-sRGB** target (`Bgra8Unorm` surface, `Rgba8Unorm` offscreen) to reproduce GLMakie and Cairo's gamma-space blending and antialiasing. The `*Srgb` format that `caps.formats[0]` usually returns on macOS blends in linear space: AA edges look different and translucent overlaps change colour.
12. **Precision:** user data is f64 but GPU math is f32. Do Makie-style float32 rescaling on the CPU (see `src/float32-scaling.jl`), for example time axes around 1e9 with small ranges.
13. **Logging:** wgpu is quiet at info level since v28. Use `RUST_LOG=wgpu=warn`, and register `device.on_uncaptured_error` in debug builds.
14. **MSRV:** `wgpu` needs 1.87, and nalgebra 0.35 needs 1.89. Your 1.98.1 covers everything.

**winit 0.30.13**
15. **One `EventLoop` per process, on the main thread** (a static `EVENT_LOOP_CREATED` flag). A second `build()` returns `Err(RecreationAttempt)`. Keep it in a main-thread `thread_local!` and reuse it for every `show()`/`display()`.
    - This means window tests can't run under the normal `cargo test` harness, which uses worker threads. Use `harness = false` or examples. Headless tests are fine.
16. **`run_app` consumes the loop**, so a library needs `run_app_on_demand` for a repeatable blocking `show()`. No window state survives between runs, so drop windows and surfaces before exiting (0.30 fixed a macOS bug where windows stayed open).
17. **`pump_app_events` on macOS works but is second-class:**
    - It stops the global `NSApplication` whenever the run loop is about to block. It is less efficient than on other platforms, and the stopped state is visible to crates like `rfd`.
    - Rendering *outside* the callback causes resize artifacts. Always render inside `RedrawRequested`.
    - Live window resizing is a modal loop, so the pump stalls while the user resizes.
    - An open PR (winit #4155) reworks this.
    - Pump at about 60 Hz, not every simulation step. If the simulation blocks for seconds between pumps, macOS will mark the app unresponsive.
18. **After `PumpStatus::Exit`, behaviour is murky.** For non-blocking mode, don't call `el.exit()`: drop the window and track a flag instead. Create later windows inside a callback (`new_events` or `about_to_wait`) using `&ActiveEventLoop`.
19. **Don't re-enter the loop from a handler.** The thread_local `RefCell` would panic on a double borrow. Create `EventLoopProxy`s before running (`UserEvent` must be `Send`).
20. **HiDPI:**
    - Size windows with `LogicalSize` so Makie `size` equals CSS px, and set `px_per_unit = window.scale_factor()`.
    - `CursorMoved` and `MouseScrollDelta::PixelDelta` (trackpad) are physical pixels with a top-left origin: divide by the scale factor and flip y.
    - Mouse wheels send `LineDelta`; trackpad pinch arrives as `PinchGesture`.
21. **Two objc2 versions get compiled:** winit 0.30 uses objc2 0.5 and wgpu 30 uses objc2 0.6. This only costs build time.
22. **Stay off winit 0.31 for now.** Beta.3 reshapes the API (see section 3) and makes many enums `#[non_exhaustive]`. wgpu's own examples still target 0.30.

**Other crates**
23. **ab_glyph size semantics:** `PxScale` is the pixel *height* (ascent − descent), not the em size. For a Makie `fontsize` in px, use `PxScale::from(fontsize_px * font.height_unscaled() / font.units_per_em().unwrap())`, which is ×1.165 for TeXGyreHerosMakie.
24. **SVG text from outlines:** outlines are in font units with y up, so flip y. Emit each glyph once as `<symbol>` and reference it with `<use>`, as Cairo does. Outline curves are separate segments, so start a new `M` whenever a segment's start point differs from the previous end point. SVG `width`/`height` in pt = units × 0.75.
25. **Heatmap orientation:** Makie's `heatmap(x, y, M)` treats `M[ix, iy]` in Julia column-major order. nalgebra `DMatrix` matches that layout directly. A row-major ndarray `Array2` indexed `[ix, iy]` has `iy` varying fastest, so transpose on upload or swap the sampling axes.
26. **pollster 1.0 is API-compatible with 0.4** (`block_on`, plus `#[pollster::main]`).
27. **base64 0.23 usage:** `use base64::prelude::*; BASE64_STANDARD.encode(bytes)`.

---

## Sources
- wgpu CHANGELOG (v27–v30): https://github.com/gfx-rs/wgpu/blob/trunk/CHANGELOG.md
- wgpu v30.0.1 hello_window: https://github.com/gfx-rs/wgpu/blob/v30.0.1/examples/standalone/02_hello_window/src/main.rs
- wgpu v30.0.1 hello_compute (readback and poll): https://github.com/gfx-rs/wgpu/blob/v30.0.1/examples/standalone/01_hello_compute/src/main.rs
- wgpu v30.0.1 render_to_texture: https://github.com/gfx-rs/wgpu/blob/v30.0.1/examples/features/src/render_to_texture/mod.rs
- wgpu v30.0.1 InstanceDescriptor: https://github.com/gfx-rs/wgpu/blob/v30.0.1/wgpu-types/src/instance.rs
- wgpu v30.0.1 surface.rs: https://github.com/gfx-rs/wgpu/blob/v30.0.1/wgpu/src/api/surface.rs
- wgpu v30.0.1 buffer.rs: https://github.com/gfx-rs/wgpu/blob/v30.0.1/wgpu/src/api/buffer.rs
- wgpu MSRV (README): https://github.com/gfx-rs/wgpu/blob/v30.0.1/README.md
- crates.io, wgpu: https://crates.io/crates/wgpu
- crates.io, winit: https://crates.io/crates/winit
- winit pump_events: https://docs.rs/winit/0.30.13/winit/platform/pump_events/trait.EventLoopExtPumpEvents.html
- winit pump_events source: https://github.com/rust-windowing/winit/blob/v0.30.13/src/platform/pump_events.rs
- winit run_on_demand: https://docs.rs/winit/0.30.13/winit/platform/run_on_demand/trait.EventLoopExtRunOnDemand.html
- winit once-per-process check: https://github.com/rust-windowing/winit/blob/v0.30.13/src/event_loop.rs
- winit ApplicationHandler: https://docs.rs/winit/0.30.13/winit/application/trait.ApplicationHandler.html
- winit EventLoopProxy: https://docs.rs/winit/0.30.13/winit/event_loop/struct.EventLoopProxy.html
- winit EventLoopBuilder: https://docs.rs/winit/0.30.13/winit/event_loop/struct.EventLoopBuilder.html
- winit PR #4155 (macOS pump rework): https://github.com/rust-windowing/winit/pull/4155
- winit 0.31.0-beta.3 release notes: https://github.com/rust-windowing/winit/releases/tag/v0.31.0-beta.3
- winit 0.31 beta ApplicationHandler: https://docs.rs/winit/0.31.0-beta.3/winit/application/trait.ApplicationHandler.html
- pollster 1.0.1: https://docs.rs/pollster/1.0.1/pollster/
- base64 0.23.1: https://docs.rs/base64/0.23.1/base64/
- png 0.18.1 Encoder: https://docs.rs/png/0.18.1/png/struct.Encoder.html (also the local cached source)
- ndarray 0.17.1 release: https://github.com/rust-ndarray/ndarray/releases/tag/0.17.1
- ab_glyph: https://github.com/alexheretic/ab-glyph and https://docs.rs/ab_glyph/0.2.32/ab_glyph/trait.Font.html
- owned_ttf_parser kerning (kern table only): https://github.com/alexheretic/owned-ttf-parser/blob/main/src/preparse.rs
- fontdue Font: https://docs.rs/fontdue/0.9.4/fontdue/struct.Font.html
- swash: https://github.com/dfrg/swash
- skrifa: https://crates.io/crates/skrifa
- cosmic-text: https://crates.io/crates/cosmic-text
- parley: https://crates.io/crates/parley
- glyphon: https://crates.io/crates/glyphon
- ruviz: https://github.com/Ameyanagi/ruviz
- pluot: https://github.com/keller-mark/pluot
- kuva: https://github.com/Psy-Fer/kuva
- starsight: https://github.com/resonant-jovian/starsight
- rsplot: https://github.com/physwkim/rsplot
- iced_plot: https://crates.io/crates/iced_plot
- lumen-charts: https://github.com/jagtesh/lumen-charts
- egui_plot: https://docs.rs/egui_plot/0.37.0/egui_plot/
- plotly.rs: https://github.com/plotly/plotly.rs
- plotters: https://github.com/plotters-rs/plotters
- charming: https://github.com/yuankunzhang/charming
- rerun: https://crates.io/crates/rerun
- ChartGPU: https://www.webgpu.com/showcase/chartgpu-webgpu-charts/
- Visualization crate list: https://lib.rs/visualization
- GUST Font License text: https://www.gust.org.pl/projects/e-foundry/licenses/GUST-FONT-LICENSE.txt
- CTAN GFL page: https://ctan.org/license/gfl
- TeX Gyre licensing: https://www.gust.org.pl/projects/e-foundry/licenses/licensing-of-tex-gyre
- TUG GFL page: https://www.tug.org/fonts/licenses/gfl.html
- CTAN TeX Gyre OTF files: https://mirrors.ctan.org/fonts/tex-gyre/opentype/
- SPDX license list: https://github.com/spdx/license-list-data
- Makie PR #1897 (TeXGyreHerosMakie metrics): https://github.com/MakieOrg/Makie.jl/pull/1897
- Local Makie sources: `~/.julia/packages/Makie/Iy6pu/src/layouting/text_layouting.jl` (no kerning), `src/tick_format.jl` (superscript spans), `src/theming.jl` (default fonts)