//! The browser backend (wasm32): figures mount into `<canvas>` elements and the browser drives
//! the event loop.
//!
//! A page has one winit event loop, started by the first figure (`EventLoopExtWebSys::spawn_app`,
//! which returns at once). Later figures reach the running app through [`INBOX`] plus an
//! `EventLoopProxy` wake-up; so do the GPU contexts, which are created asynchronously per canvas
//! (`wasm_bindgen_futures::spawn_local`). Until its context arrives a canvas draws nothing, and
//! it stays blank until the browser laid it out (winit reports a 0x0 size before the first
//! `ResizeObserver` callback).
//!
//! The canvas gets `data-ezviz-backend="webgpu"|"webgl2"` once it can draw, or
//! `data-ezviz-error="..."` if the GPU could not be initialized, so pages and tests can wait for
//! it.

use super::UserEvent;
use super::animate::Frame;
use super::app::{App, Gfx, OpenReq};
use crate::error::{Error, Result};
use crate::figure::Figure;
use std::cell::RefCell;
use std::sync::Arc;
use wasm_bindgen::JsCast;
use web_sys::HtmlCanvasElement;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::platform::web::{EventLoopExtWebSys, WindowAttributesExtWebSys, WindowExtWebSys};
use winit::window::{Window, WindowId};

/// A message for the app, which the browser owns once spawned.
enum Msg {
    Open(OpenReq<'static>),
    GpuReady(WindowId, Result<Gfx>),
}

thread_local! {
    static INBOX: RefCell<Vec<Msg>> = const { RefCell::new(Vec::new()) };
    /// Wakes the running app (`None` until the first figure is mounted).
    static PROXY: RefCell<Option<EventLoopProxy<UserEvent>>> = const { RefCell::new(None) };
}

/// Queues `msg` for the app, starting the app on first use.
fn post(msg: Msg) -> Result<()> {
    INBOX.with(|i| i.borrow_mut().push(msg));
    let proxy = PROXY.with(|p| p.borrow().clone());
    if let Some(proxy) = proxy {
        let _ = proxy.send_event(UserEvent::Inbox);
        return Ok(());
    }
    init_console();
    let el = EventLoop::<UserEvent>::with_user_event().build().map_err(|e| Error::EventLoop(e.to_string()))?;
    el.set_control_flow(ControlFlow::Wait);
    PROXY.with(|p| *p.borrow_mut() = Some(el.create_proxy()));
    let app: App<'static> = App::new(None, el.create_proxy(), false, None);
    el.spawn_app(app); // returns at once; `resumed` drains the inbox
    Ok(())
}

/// Hands queued figures and GPU contexts to the app.
pub(super) fn drain_inbox(app: &mut App<'_>) {
    let msgs = INBOX.with(|i| std::mem::take(&mut *i.borrow_mut()));
    for m in msgs {
        match m {
            Msg::Open(req) => app.to_open.push(req),
            Msg::GpuReady(id, gfx) => app.gpu_ready(id, gfx),
        }
    }
}

/// Panics go to the browser console, and so do `log` records (unless the page's code installed
/// a logger first).
fn init_console() {
    console_error_panic_hook::set_once();
    let _ = console_log::init_with_level(log::Level::Info);
}

fn document() -> Result<web_sys::Document> {
    web_sys::window().and_then(|w| w.document()).ok_or_else(|| Error::EventLoop("no document".into()))
}

/// The `<canvas>` with this id.
fn find_canvas(id: &str) -> Result<HtmlCanvasElement> {
    document()?
        .get_element_by_id(id)
        .ok_or_else(|| Error::EventLoop(format!("no element with id \"{id}\"")))?
        .dyn_into::<HtmlCanvasElement>()
        .map_err(|_| Error::EventLoop(format!("element \"{id}\" is not a <canvas>")))
}

/// Creates the winit window for canvas `canvas_id`, or for a new canvas appended to `<body>`
/// with the CSS size of the figure (`size`, units = CSS px). Returns whether the canvas is new.
pub(super) fn create_window(
    el: &ActiveEventLoop,
    canvas_id: Option<&str>,
    size: [f64; 2],
) -> Result<(Arc<Window>, bool)> {
    let canvas = canvas_id.map(find_canvas).transpose()?;
    let owned = canvas.is_none();
    let attrs = Window::default_attributes()
        .with_canvas(canvas)
        .with_append(owned)
        .with_prevent_default(true) // no page scroll/zoom from wheel and touch over the figure
        .with_focusable(true); // keyboard (x/y keys, modifiers) needs focus
    let window = el.create_window(attrs).map_err(|e| Error::EventLoop(e.to_string()))?;
    let canvas = window.canvas().ok_or_else(|| Error::EventLoop("the window has no canvas".into()))?;
    let style = canvas.style();
    // No browser gestures over the figure; no focus ring (the canvas takes focus on click for
    // the x/y keys and modifiers).
    let _ = style.set_property("touch-action", "none");
    let _ = style.set_property("outline", "none");
    if owned {
        let _ = style.set_property("display", "block");
        let _ = style.set_property("width", &format!("{}px", size[0]));
        let _ = style.set_property("height", &format!("{}px", size[1]));
    } else {
        ensure_css_size(&canvas, size);
    }
    Ok((Arc::new(window), owned))
}

/// Gives the canvas a CSS size (the figure's) in each dimension its CSS leaves to the
/// `width`/`height` attributes. Those are the drawing-buffer size in device pixels, which wgpu
/// sets on every configure: a canvas laid out by them would grow by the pixel ratio each time.
/// Detected by changing the attributes and watching the layout size.
fn ensure_css_size(canvas: &HtmlCanvasElement, size: [f64; 2]) {
    let (w, h) = (canvas.width(), canvas.height());
    let before = canvas.get_bounding_client_rect();
    canvas.set_width(w + 17);
    canvas.set_height(h + 17);
    let after = canvas.get_bounding_client_rect();
    canvas.set_width(w);
    canvas.set_height(h);
    let style = canvas.style();
    if after.width() != before.width() {
        let _ = style.set_property("width", &format!("{}px", size[0]));
    }
    if after.height() != before.height() {
        let _ = style.set_property("height", &format!("{}px", size[1]));
    }
}

/// Starts creating the GPU context of `window`; it arrives as `Msg::GpuReady`.
pub(super) fn init_gpu(window: Arc<Window>) {
    let id = window.id();
    wasm_bindgen_futures::spawn_local(async move {
        let gfx = match crate::render::gpu::gpu_for_surface(window).await {
            Ok((gpu, surface)) => Gfx::new(gpu, surface),
            Err(e) => Err(e),
        };
        if let Err(e) = post(Msg::GpuReady(id, gfx)) {
            log::error!("ezviz: {e}");
        }
    });
}

/// The canvas can draw: tag it with the backend.
pub(super) fn mark_ready(window: &Window, gfx: &Gfx) {
    let backend = match gfx.gpu.backend() {
        wgpu::Backend::BrowserWebGpu => "webgpu",
        wgpu::Backend::Gl => "webgl2",
        _ => "other",
    };
    let name = gfx.gpu.adapter.get_info().name;
    let name = if name.is_empty() { String::new() } else { format!(" on {name}") };
    log::info!(
        "ezviz: {backend} canvas{name}, format {:?}, max texture {} px",
        gfx.config.format,
        gfx.gpu.max_texture_size()
    );
    if let Some(c) = window.canvas() {
        let _ = c.set_attribute("data-ezviz-backend", backend);
    }
}

/// The GPU context could not be created: tag the canvas with the error.
pub(super) fn mark_failed(window: &Window, err: &str) {
    if let Some(c) = window.canvas() {
        let _ = c.set_attribute("data-ezviz-error", err);
    }
}

/// Removes a canvas ezviz appended (its window closed).
pub(super) fn remove_canvas(window: &Window) {
    if let Some(c) = window.canvas() {
        c.remove();
    }
}

fn mount(fig: &Figure, canvas: Option<&str>, anim: Option<Box<dyn FnMut(&mut Frame)>>) -> Result<()> {
    if let Some(id) = canvas {
        find_canvas(id)?;
    }
    let mut req = OpenReq::new(fig, crate::figure::next_uid());
    req.canvas = canvas.map(str::to_owned);
    req.anim = anim;
    post(Msg::Open(req))
}

/// Mounts each figure into a new `<canvas>` appended to the page and returns at once (browser).
pub fn show_all(figs: &[&Figure]) -> Result<()> {
    figs.iter().try_for_each(|f| mount(f, None, None))
}

impl Figure {
    /// Mounts the figure into a new `<canvas>` appended to `<body>` (CSS size = figure size) and
    /// returns at once; the browser keeps it interactive (pan, zoom, hover, touch).
    ///
    /// Uses WebGPU when the browser has it, WebGL2 otherwise (`?backend=gl` in the page URL
    /// forces WebGL2). `show_live`, `display` and `pump` are native only: in the browser run
    /// simulations with [`Figure::animate`].
    pub fn show(&self) -> Result<()> {
        mount(self, None, None)
    }

    /// Mounts the figure into the existing `<canvas id="canvas_id">` and returns at once. The
    /// figure is laid out at the canvas' CSS size; if the page's CSS gives the canvas no size,
    /// the figure's size is set.
    pub fn show_in(&self, canvas_id: &str) -> Result<()> {
        mount(self, Some(canvas_id), None)
    }

    /// Mounts the figure into a new `<canvas>` (like [`Figure::show`]) and calls `f` before
    /// each displayed frame (`requestAnimationFrame`), with the same [`Frame`] as natively.
    /// Returns at once. `f` is not called while the canvas is off-screen or the tab is hidden,
    /// and `dt` is capped at [`Frame::MAX_DT`] afterwards.
    pub fn animate(&self, f: impl FnMut(&mut Frame) + 'static) -> Result<()> {
        mount(self, None, Some(Box::new(f)))
    }

    /// [`Figure::animate`] into the existing `<canvas id="canvas_id">`.
    pub fn animate_in(&self, canvas_id: &str, f: impl FnMut(&mut Frame) + 'static) -> Result<()> {
        mount(self, Some(canvas_id), Some(Box::new(f)))
    }
}
