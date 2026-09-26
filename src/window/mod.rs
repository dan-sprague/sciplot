//! Native windows (winit + wgpu): blocking `show()`, redraw on change, HiDPI.

#[cfg(target_os = "macos")]
mod macos;

use crate::error::{Error, Result};
use crate::figure::Figure;
use crate::render::gpu::{Gpu, Renderer, TARGET_FORMAT, gpu};
use crate::scene::drawlist::DrawList;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::platform::run_on_demand::EventLoopExtRunOnDemand;
use winit::window::{Window, WindowId};

#[derive(Debug, Clone, Copy)]
pub(crate) enum UserEvent {
    /// A figure with this uid changed.
    Wake(u64),
}

thread_local! {
    static EVENT_LOOP: RefCell<Option<EventLoop<UserEvent>>> = const { RefCell::new(None) };
}

/// Whether we are on the process' main thread (required for windows on macOS).
pub(crate) fn is_main_thread() -> bool {
    #[cfg(target_os = "macos")]
    {
        objc2::MainThreadMarker::new().is_some()
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::thread::current().name() == Some("main")
    }
}

fn with_event_loop<R>(f: impl FnOnce(&mut EventLoop<UserEvent>) -> Result<R>) -> Result<R> {
    if !is_main_thread() {
        return Err(Error::NotMainThread);
    }
    EVENT_LOOP.with(|cell| {
        let mut slot = cell.try_borrow_mut().map_err(|_| Error::Reentrant)?;
        if slot.is_none() {
            let el = EventLoop::<UserEvent>::with_user_event()
                .build()
                .map_err(|e| Error::EventLoop(e.to_string()))?;
            *slot = Some(el);
        }
        f(slot.as_mut().unwrap())
    })
}

struct Screen {
    surface: wgpu::Surface<'static>,
    window: Arc<Window>,
    config: wgpu::SurfaceConfiguration,
    fig: Figure,
    renderer: Renderer,
    wake_id: u64,
    last: Option<DrawList>,
    dumped: bool,
}

impl Drop for Screen {
    fn drop(&mut self) {
        self.fig.sh.wake.detach(self.wake_id);
    }
}

struct App {
    gpu: Arc<Gpu>,
    to_open: Vec<Figure>,
    screens: HashMap<WindowId, Screen>,
    proxy: EventLoopProxy<UserEvent>,
    error: Option<Error>,
    autoclose: Option<Instant>,
}

impl App {
    fn open_pending(&mut self, el: &ActiveEventLoop) {
        for fig in std::mem::take(&mut self.to_open) {
            if let Err(e) = self.open(el, fig) {
                self.error = Some(e);
                el.exit();
                return;
            }
        }
    }

    fn open(&mut self, el: &ActiveEventLoop, fig: Figure) -> Result<()> {
        let (size, title) = {
            let st = fig.sh.state.lock();
            (
                st.theme.globals().size,
                st.window_title.clone().unwrap_or_else(|| "ezviz".into()),
            )
        };
        let attrs = Window::default_attributes()
            .with_title(title)
            .with_inner_size(LogicalSize::new(size[0], size[1]));
        let window = Arc::new(
            el.create_window(attrs)
                .map_err(|e| Error::EventLoop(e.to_string()))?,
        );
        let surface = self
            .gpu
            .instance
            .create_surface(window.clone())
            .map_err(|e| Error::Gpu(e.to_string()))?;
        let caps = surface.get_capabilities(&self.gpu.adapter);
        if !caps.formats.contains(&TARGET_FORMAT) {
            return Err(Error::Gpu(format!(
                "surface does not support {TARGET_FORMAT:?}"
            )));
        }
        let phys = window.inner_size();
        let alpha_mode = if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
            wgpu::CompositeAlphaMode::Opaque
        } else {
            caps.alpha_modes[0]
        };
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: TARGET_FORMAT,
            color_space: wgpu::SurfaceColorSpace::Srgb,
            width: phys.width.max(1),
            height: phys.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: vec![],
        };
        configure(&self.gpu, &surface, &config);
        let wake_id = crate::figure::next_uid();
        let proxy = parking_lot::Mutex::new(self.proxy.clone());
        let uid = fig.sh.uid;
        fig.sh.wake.attach(
            wake_id,
            Box::new(move || {
                let _ = proxy.lock().send_event(UserEvent::Wake(uid));
            }),
        );
        window.request_redraw();
        let renderer = Renderer::new(self.gpu.clone());
        self.screens.insert(
            window.id(),
            Screen {
                surface,
                window,
                config,
                fig,
                renderer,
                wake_id,
                last: None,
                dumped: false,
            },
        );
        Ok(())
    }

    fn render(&mut self, id: WindowId) {
        let gpu = self.gpu.clone();
        let Some(s) = self.screens.get_mut(&id) else {
            return;
        };
        let ppu = s.window.scale_factor();
        let size_units = [s.config.width as f64 / ppu, s.config.height as f64 / ppu];
        s.fig.sh.wake.clear();
        let st = s.fig.sh.snapshot();
        let dl = match (&s.last, st.batch_depth > 0) {
            (Some(last), true) => last.clone(),
            _ => crate::scene::build(&st, Some(size_units), &mut s.renderer.scene).0,
        };
        let frame = match s.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) => f,
            wgpu::CurrentSurfaceTexture::Suboptimal(f) => {
                drop(f);
                configure(&gpu, &s.surface, &s.config);
                s.window.request_redraw();
                return;
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                configure(&gpu, &s.surface, &s.config);
                s.window.request_redraw();
                return;
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => return,
            wgpu::CurrentSurfaceTexture::Validation => {
                log::error!("ezviz: surface validation error");
                return;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let cmd = s
            .renderer
            .render(&dl, &view, [s.config.width, s.config.height], ppu);
        gpu.queue.submit([cmd]);
        s.window.pre_present_notify();
        gpu.queue.present(frame);
        // Testing hook: write the first presented frame (as rendered for this window) to a PNG.
        if !s.dumped {
            if let Ok(path) = std::env::var("EZVIZ_WINDOW_DUMP") {
                s.dumped = true;
                match s.renderer.render_rgba(&dl, ppu) {
                    Ok((width, height, data)) => {
                        let img = crate::figure::RgbaImage {
                            width,
                            height,
                            data,
                        };
                        if let Err(e) =
                            crate::figure::write_png(std::path::Path::new(&path), &img, ppu)
                        {
                            log::error!("ezviz: window dump failed: {e}");
                        }
                    }
                    Err(e) => log::error!("ezviz: window dump failed: {e}"),
                }
            }
        }
        s.last = Some(dl);
    }
}

fn configure(gpu: &Gpu, surface: &wgpu::Surface<'static>, config: &wgpu::SurfaceConfiguration) {
    surface.configure(&gpu.device, config);
    #[cfg(target_os = "macos")]
    macos::force_srgb_colorspace(surface);
}

impl ApplicationHandler<UserEvent> for App {
    fn new_events(&mut self, el: &ActiveEventLoop, _cause: StartCause) {
        if let Some(t) = self.autoclose {
            if Instant::now() >= t {
                self.screens.clear();
                el.exit();
                return;
            }
            el.set_control_flow(ControlFlow::WaitUntil(t));
        }
        if !self.to_open.is_empty() {
            self.open_pending(el);
        }
    }

    fn resumed(&mut self, el: &ActiveEventLoop) {
        self.open_pending(el);
    }

    fn user_event(&mut self, _el: &ActiveEventLoop, ev: UserEvent) {
        match ev {
            UserEvent::Wake(uid) => {
                for s in self.screens.values() {
                    if s.fig.sh.uid == uid {
                        s.window.request_redraw();
                    }
                }
            }
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested | WindowEvent::Destroyed => {
                self.screens.remove(&id);
                if self.screens.is_empty() {
                    el.exit();
                }
            }
            WindowEvent::Resized(size) => {
                let gpu = self.gpu.clone();
                if let Some(s) = self.screens.get_mut(&id) {
                    s.config.width = size.width.max(1);
                    s.config.height = size.height.max(1);
                    configure(&gpu, &s.surface, &s.config);
                    s.last = None;
                    s.window.request_redraw();
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(s) = self.screens.get(&id) {
                    s.window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => self.render(id),
            _ => {}
        }
    }
}

/// Opens each figure in its own window and blocks until all are closed.
///
/// Must be called on the main thread. Set `EZVIZ_AUTOCLOSE=<seconds>` to close automatically
/// (useful for smoke tests).
pub fn show_all(figs: &[&Figure]) -> Result<()> {
    let gpu = gpu()?;
    with_event_loop(|el| {
        let autoclose = std::env::var("EZVIZ_AUTOCLOSE")
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .map(|s| Instant::now() + Duration::from_secs_f64(s));
        let mut app = App {
            gpu,
            to_open: figs.iter().map(|f| (*f).clone()).collect(),
            screens: HashMap::new(),
            proxy: el.create_proxy(),
            error: None,
            autoclose,
        };
        el.set_control_flow(ControlFlow::Wait);
        let r = el
            .run_app_on_demand(&mut app)
            .map_err(|e| Error::EventLoop(e.to_string()));
        app.screens.clear();
        if let Some(e) = app.error.take() {
            return Err(e);
        }
        r
    })
}

impl Figure {
    /// Opens the figure in a window and blocks until it is closed (Makie's `wait(display(fig))`).
    ///
    /// Pan with right-drag, zoom with the scroll wheel, reset with Ctrl+click. Must be called on the
    /// main thread; to run a simulation while the window is open use `show_live`.
    pub fn show(&self) -> Result<()> {
        show_all(&[self])
    }
}
