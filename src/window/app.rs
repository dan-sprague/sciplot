//! The app core shared by native windows and browser canvases: one winit `ApplicationHandler`
//! that owns every window with its GPU surface, the last built frame, the interaction and hover
//! state, and the per-window animation callbacks.
//!
//! Platform code only creates windows: `native.rs` opens OS windows and creates their surface
//! at once; `web.rs` binds canvases and creates the GPU context asynchronously (the window
//! draws nothing until [`App::gpu_ready`] delivers it).

use super::animate::{Anim, AnimFn};
use super::input::{Touches, translate};
use super::interact::{self, AxisView, Effect, Input, InteractState};
use super::live::LiveShared;
use super::{UserEvent, overlay};
use crate::error::{Error, Result};
use crate::figure::{BlockId, Dirty, FigState, Figure};
use crate::plots::pick::{Hover, PickCache};
use crate::render::gpu::{Gpu, Renderer, surface_format};
use crate::scene::AxisFrame;
use crate::scene::drawlist::DrawList;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use web_time::Instant;
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::window::{Window, WindowId};

/// A figure waiting for its window.
pub(crate) struct OpenReq<'f> {
    pub fig: Figure,
    /// Reported in [`App::opened`] once the window exists.
    pub token: u64,
    /// Web: id of the `<canvas>` to draw into (`None`: append a new canvas to `<body>`).
    pub canvas: Option<String>,
    /// Per-frame callback ([`Figure::animate`]).
    pub anim: Option<AnimFn<'f>>,
}

impl<'f> OpenReq<'f> {
    pub fn new(fig: &Figure, token: u64) -> OpenReq<'f> {
        OpenReq { fig: fig.clone(), token, canvas: None, anim: None }
    }
}

/// The GPU side of one window: its surface and the renderer drawing into it.
pub(crate) struct Gfx {
    pub gpu: Arc<Gpu>,
    pub surface: wgpu::Surface<'static>,
    /// Width and height are 0 until the window has a size.
    pub config: wgpu::SurfaceConfiguration,
    pub renderer: Renderer,
    /// Device pixels per figure unit: the scale factor, lowered when the window would exceed the
    /// GPU's texture size limit (the browser then upscales the canvas).
    pub ppu: f64,
}

impl Gfx {
    /// Picks the surface format (`Bgra8Unorm` or `Rgba8Unorm`, never sRGB) and the renderer; the
    /// surface is configured by the first [`Gfx::resize`] with a non-zero size.
    pub fn new(gpu: Arc<Gpu>, surface: wgpu::Surface<'static>) -> Result<Gfx> {
        let caps = surface.get_capabilities(&gpu.adapter);
        let format = surface_format(&caps.formats)
            .ok_or_else(|| Error::Gpu(format!("no 8-bit non-sRGB surface format among {:?}", caps.formats)))?;
        let alpha_mode = if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
            wgpu::CompositeAlphaMode::Opaque
        } else {
            caps.alpha_modes.first().copied().unwrap_or(wgpu::CompositeAlphaMode::Auto)
        };
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::Srgb,
            width: 0,
            height: 0,
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: vec![],
        };
        let renderer = Renderer::new(gpu.clone());
        Ok(Gfx { gpu, surface, config, renderer, ppu: 1.0 })
    }

    /// Whether the surface is configured (the window has a non-zero size).
    pub fn ready(&self) -> bool {
        self.config.width > 0 && self.config.height > 0
    }

    /// Configures the surface for a window of `phys` device pixels at scale factor `scale`.
    /// Empty sizes (a canvas before layout, a minimized window) are skipped.
    pub fn resize(&mut self, phys: [u32; 2], scale: f64) {
        if phys[0] == 0 || phys[1] == 0 {
            return;
        }
        let (size, k) = clamp_size(phys, self.gpu.max_texture_size());
        self.ppu = scale * k;
        self.config.width = size[0];
        self.config.height = size[1];
        self.configure();
    }

    pub fn configure(&self) {
        if !self.ready() {
            return;
        }
        self.surface.configure(&self.gpu.device, &self.config);
        #[cfg(target_os = "macos")]
        super::macos::force_srgb_colorspace(&self.surface);
    }
}

/// Scales `phys` down (keeping the aspect) so neither side exceeds `max`; returns the size and
/// the factor applied.
pub(crate) fn clamp_size(phys: [u32; 2], max: u32) -> ([u32; 2], f64) {
    let k = (max as f64 / phys[0].max(phys[1]).max(1) as f64).min(1.0);
    let side = |v: u32| ((v as f64 * k).round() as u32).clamp(1, max.max(1));
    ([side(phys[0]), side(phys[1])], k)
}

/// Makie's layout jitter guard (`timed_ticklabelspace_reset`): while the user zooms or pans,
/// the tick-label space of the axes involved stays at its value when the burst started, so the
/// axis does not move under the cursor as the labels change width; [`FREEZE_SECS`] after the
/// last event the attributes are restored and the layout adapts once.
pub(crate) struct Freeze {
    /// App time (seconds) at which the attributes are restored.
    until: f64,
    /// Frozen axes with their `xticklabelspace` and `yticklabelspace` attributes before the burst.
    saved: Vec<(BlockId, LabelSpace, LabelSpace)>,
}

/// A `x/yticklabelspace` attribute as stored (unset, automatic or fixed).
type LabelSpace = Option<Option<f64>>;

/// How long the tick-label space stays frozen after the last zoom or pan event (Makie: 0.2 s).
pub(crate) const FREEZE_SECS: f64 = 0.2;

/// The last built frame of a window.
pub(crate) struct Built {
    pub dl: DrawList,
    pub frames: Vec<AxisFrame>,
    /// Interaction views; updated in place by interactions until the next rebuild.
    pub views: Vec<AxisView>,
    /// The snapshot the frame was built from (hover picking reads it, never the live state).
    pub st: FigState,
    pub rev: u64,
    pub size: [f64; 2],
}

/// One open window (a native window or a browser canvas).
pub(crate) struct Win {
    pub window: Arc<Window>,
    /// `None` in the browser until the asynchronous GPU initialization finished.
    pub gfx: Option<Gfx>,
    pub fig: Figure,
    wake_id: u64,
    pub built: Option<Built>,
    /// Frames rendered so far (presented, or built while occluded for the dump).
    pub frames: u64,
    /// `EZVIZ_WINDOW_DUMP` path and the frame number to write (`EZVIZ_WINDOW_DUMP_FRAME`, 1).
    pub dump: Option<(std::path::PathBuf, u64)>,
    pub ui: InteractState,
    touches: Touches,
    pub hover: Option<Hover>,
    picks: PickCache,
    pub live: Option<Arc<LiveShared>>,
    pub occluded: bool,
    /// Latest inner size in device pixels (0x0 for a canvas until the browser laid it out).
    pub phys: [u32; 2],
    /// Web: the canvas was created by ezviz and is removed with the window.
    pub owns_canvas: bool,
    /// Tick-label space frozen by an ongoing zoom or pan.
    freeze: Option<Freeze>,
}

impl Drop for Win {
    fn drop(&mut self) {
        self.fig.sh.wake.detach(self.wake_id);
        self.thaw();
        if let Some(l) = &self.live {
            l.set_closed();
        }
        #[cfg(target_arch = "wasm32")]
        if self.owns_canvas {
            super::web::remove_canvas(&self.window);
        }
    }
}

pub(crate) struct App<'f> {
    /// The shared device (native). In the browser every canvas gets its context asynchronously.
    pub gpu: Option<Arc<Gpu>>,
    /// Figures to open once the event loop runs.
    pub to_open: Vec<OpenReq<'f>>,
    /// `(token, window)` of opened requests.
    pub opened: Vec<(u64, WindowId)>,
    pub wins: HashMap<WindowId, Win>,
    /// Animation callbacks by window.
    pub anims: HashMap<WindowId, Anim<'f>>,
    pub proxy: EventLoopProxy<UserEvent>,
    pub error: Option<Error>,
    pub autoclose: Option<Instant>,
    /// `show`/`show_live`/`animate` exit the loop when the last window closes; pump mode and the
    /// browser never exit.
    exit_when_empty: bool,
    live: Option<Arc<LiveShared>>,
    epoch: Instant,
    /// Whether `resumed` ran (windows can be created).
    pub resumed: bool,
    /// Last redraw request of an occluded animated window (seconds on the app clock).
    pub occluded_poll: f64,
}

impl<'f> App<'f> {
    pub fn new(
        gpu: Option<Arc<Gpu>>,
        proxy: EventLoopProxy<UserEvent>,
        exit_when_empty: bool,
        live: Option<Arc<LiveShared>>,
    ) -> App<'f> {
        let autoclose = std::env::var("EZVIZ_AUTOCLOSE")
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .map(|s| Instant::now() + Duration::from_secs_f64(s.max(0.0)));
        App {
            gpu,
            to_open: Vec::new(),
            opened: Vec::new(),
            wins: HashMap::new(),
            anims: HashMap::new(),
            proxy,
            error: None,
            autoclose,
            exit_when_empty,
            live,
            epoch: Instant::now(),
            resumed: false,
            occluded_poll: 0.0,
        }
    }

    pub fn open_pending(&mut self, el: &ActiveEventLoop) {
        if !self.resumed {
            return;
        }
        for req in std::mem::take(&mut self.to_open) {
            let token = req.token;
            match self.open(el, req) {
                Ok(id) => self.opened.push((token, id)),
                Err(e) => {
                    log::error!("ezviz: could not open a window: {e}");
                    self.error = Some(e);
                    if self.exit_when_empty {
                        el.exit();
                    }
                    return;
                }
            }
        }
    }

    fn open(&mut self, el: &ActiveEventLoop, req: OpenReq<'f>) -> Result<WindowId> {
        let OpenReq { fig, token: _, canvas, anim } = req;
        let (size, title) = {
            let st = fig.sh.state.lock();
            (st.theme.globals().size, st.window_title.clone().unwrap_or_else(|| "ezviz".into()))
        };
        #[cfg(not(target_arch = "wasm32"))]
        let (window, gfx, owns_canvas) = {
            let _ = canvas;
            let gpu = self.gpu.clone().ok_or_else(|| Error::NoGpuAdapter("no GPU context".into()))?;
            let (window, gfx) = super::native::create_window(el, gpu, size, &title)?;
            (window, Some(gfx), false)
        };
        #[cfg(target_arch = "wasm32")]
        let (window, gfx, owns_canvas) = {
            let _ = title;
            let (window, owned) = super::web::create_window(el, canvas.as_deref(), size)?;
            super::web::init_gpu(window.clone());
            (window, None, owned)
        };
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
        let live = self.live.clone().filter(|l| l.fig_uid == uid);
        let id = window.id();
        let phys = window.inner_size();
        let mut win = Win {
            window,
            gfx,
            fig,
            wake_id,
            built: None,
            frames: 0,
            dump: dump_request(),
            ui: InteractState::default(),
            touches: Touches::default(),
            hover: None,
            picks: PickCache::default(),
            live,
            occluded: false,
            phys: [phys.width, phys.height],
            owns_canvas,
            freeze: None,
        };
        let scale = win.scale();
        if let Some(g) = win.gfx.as_mut() {
            g.resize(win.phys, scale);
        }
        if let Some(f) = anim {
            // An animation's first frame is rarely the interesting one: without
            // `EZVIZ_WINDOW_DUMP_FRAME` the dump is written from the last frame.
            let dump_last = if std::env::var_os("EZVIZ_WINDOW_DUMP_FRAME").is_none() {
                win.dump.take().map(|(p, _)| p)
            } else {
                None
            };
            self.anims.insert(id, Anim::new(f, dump_last));
        }
        self.wins.insert(id, win);
        Ok(id)
    }

    /// The asynchronously created GPU context of window `id` arrived (browser).
    pub fn gpu_ready(&mut self, id: WindowId, gfx: Result<Gfx>) {
        let Some(w) = self.wins.get_mut(&id) else { return };
        match gfx {
            Ok(mut g) => {
                g.resize(w.phys, w.scale());
                #[cfg(target_arch = "wasm32")]
                super::web::mark_ready(&w.window, &g);
                w.gfx = Some(g);
                w.window.request_redraw();
            }
            Err(e) => {
                log::error!("ezviz: GPU initialization failed: {e}");
                #[cfg(target_arch = "wasm32")]
                super::web::mark_failed(&w.window, &e.to_string());
                self.error = Some(e);
            }
        }
    }

    pub fn close(&mut self, el: &ActiveEventLoop, id: WindowId) {
        self.write_anim_dump(id);
        self.anims.remove(&id);
        self.wins.remove(&id);
        if self.exit_when_empty && self.wins.is_empty() {
            el.exit();
        }
    }

    /// Seconds since the app started (double-click timing, animation clock).
    pub fn now(&self) -> f64 {
        self.epoch.elapsed().as_secs_f64()
    }

    /// Runs the animation callback (if any) and renders window `id`.
    fn redraw(&mut self, el: &ActiveEventLoop, id: WindowId) {
        if self.tick(id) {
            self.close(el, id);
            return;
        }
        if let Some(w) = self.wins.get_mut(&id) {
            w.render();
        }
        self.request_next_frame(id);
    }
}

impl Win {
    pub fn scale(&self) -> f64 {
        self.window.scale_factor()
    }

    /// Whether the window can show a frame now (GPU ready, sized, not occluded).
    pub fn can_render(&self) -> bool {
        !self.occluded && self.gfx.as_ref().is_some_and(Gfx::ready)
    }

    /// The window size in figure units.
    fn units(&self) -> [f64; 2] {
        let s = self.scale();
        [self.phys[0] as f64 / s, self.phys[1] as f64 / s]
    }

    /// New inner size (device pixels) from the window system.
    fn resized(&mut self, phys: [u32; 2]) {
        self.phys = phys;
        let scale = self.scale();
        if let Some(g) = self.gfx.as_mut() {
            g.resize(phys, scale);
        }
        self.window.request_redraw();
    }

    /// Feeds one input to the interaction state machine and applies its effects (`now`: app
    /// time in seconds).
    pub fn input(&mut self, input: Input, now: f64) {
        // While frames are skipped (occluded window) nothing rebuilds the scene, so bring the
        // hit-test geometry up to date here.
        if self.occluded || self.built.is_none() {
            self.rebuild_if_stale();
        }
        let mut redraw = false;
        if let Some(b) = self.built.as_mut() {
            let fx = interact::handle(&mut self.ui, input, &b.views);
            // Scroll, pinch and pan moves come in bursts: freeze the layout (rectangle zooms
            // and resets are single steps and relayout at once, like Makie).
            let burst = matches!(input, Input::Scroll(_) | Input::Pinch(_) | Input::CursorMoved(_));
            if burst && fx.iter().any(|e| matches!(e, Effect::SetLimits { .. })) {
                freeze_layout(&self.fig, &mut self.freeze, b, &fx, now);
            }
            if !fx.is_empty() {
                redraw |= apply_effects(&self.fig, b, &fx);
            }
        } else {
            interact::handle(&mut self.ui, input, &[]);
        }
        if matches!(input, Input::CursorMoved(_) | Input::CursorLeft | Input::Button { .. } | Input::FocusLost) {
            redraw |= self.update_hover();
        }
        if redraw {
            self.window.request_redraw();
        }
    }

    /// Restores the frozen tick-label space once its time is up; returns when it will be
    /// (app seconds) while it is still frozen.
    pub fn thaw_if_due(&mut self, now: f64) -> Option<f64> {
        let until = self.freeze.as_ref()?.until;
        if now < until {
            return Some(until);
        }
        self.thaw();
        None
    }

    /// Restores the tick-label space attributes saved by [`freeze_layout`].
    fn thaw(&mut self) {
        let Some(f) = self.freeze.take() else { return };
        self.fig.sh.update(Dirty::LAYOUT, |st| {
            for (id, x, y) in f.saved {
                if let Some(a) = st.block_mut(id).and_then(|b| b.as_axis_mut()) {
                    a.attrs.xticklabelspace = x;
                    a.attrs.yticklabelspace = y;
                }
            }
        });
    }

    /// Re-picks the hovered element; returns whether the tooltip changed.
    fn update_hover(&mut self) -> bool {
        let new = match (&self.built, self.ui.cursor) {
            (Some(b), Some(c)) if b.st.datainspector && !self.ui.dragging() => {
                overlay::pick(&b.st, &b.frames, c, &mut self.picks)
            }
            _ => None,
        };
        let changed = new != self.hover;
        self.hover = new;
        #[cfg(feature = "testing")]
        super::testing::record_hover(self.fig.sh.uid, self.hover.as_ref().map(|h| h.text.as_str()));
        changed
    }

    /// Rebuilds the frame when the figure changed (outside a batch) or the window was resized.
    pub fn rebuild_if_stale(&mut self) {
        let size = self.units();
        let Some(gfx) = self.gfx.as_mut() else { return };
        if size[0] <= 0.0 || size[1] <= 0.0 {
            return;
        }
        let (rev, batch_open) = {
            let st = self.fig.sh.state.lock();
            (st.rev, st.batch_depth > 0)
        };
        let stale = self.built.as_ref().is_none_or(|b| b.size != size || (b.rev != rev && !batch_open));
        if stale {
            let st = self.fig.sh.snapshot();
            let (dl, frames) = crate::scene::build(&st, Some(size), &mut gfx.renderer.scene);
            let views = overlay::views(&frames, &st);
            self.picks.retain(|uid| st.iter_plots().any(|(_, p)| p.uid == uid));
            #[cfg(feature = "testing")]
            super::testing::record_views(self.fig.sh.uid, &views);
            self.built = Some(Built { dl, frames, views, rev: st.rev, st, size });
            self.update_hover();
        }
    }

    fn render(&mut self) {
        self.fig.sh.wake.clear();
        let Some(gfx) = self.gfx.as_ref() else { return };
        if !gfx.ready() {
            return;
        }
        // Acquire the surface first: an occluded (hidden, minimized) window skips the frame
        // without rebuilding the scene.
        let dump_pending = self.dump.is_some();
        let frame = match gfx.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) => {
                self.occluded = false;
                Some(f)
            }
            wgpu::CurrentSurfaceTexture::Suboptimal(f) => {
                drop(f);
                gfx.configure();
                self.window.request_redraw();
                return;
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                gfx.configure();
                self.window.request_redraw();
                return;
            }
            wgpu::CurrentSurfaceTexture::Occluded => {
                self.occluded = true;
                // Build the first frame anyway (hit testing needs it) and every frame while
                // the window dump is pending.
                if self.built.is_some() && !dump_pending {
                    // Skipped frame: count it as shown so `wait_frame` never blocks on it.
                    if let Some(l) = &self.live {
                        l.presented(self.fig.sh.state.lock().rev);
                    }
                    return;
                }
                None
            }
            wgpu::CurrentSurfaceTexture::Timeout => {
                self.window.request_redraw();
                return;
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                log::error!("ezviz: surface validation error");
                return;
            }
        };
        self.rebuild_if_stale();
        let (Some(b), Some(gfx)) = (&self.built, self.gfx.as_mut()) else { return };
        let built_rev = b.rev;
        let dl = overlay::compose(&b.dl, &b.st, &b.views, &self.ui, self.hover.as_ref());
        if let Some(frame) = frame {
            let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
            let size = [gfx.config.width, gfx.config.height];
            let cmd = gfx.renderer.render(&dl, &view, gfx.config.format, size, gfx.ppu);
            gfx.gpu.queue.submit([cmd]);
            self.window.pre_present_notify();
            gfx.gpu.queue.present(frame);
        }
        if let Some(l) = &self.live {
            l.presented(built_rev);
        }
        // `EZVIZ_WINDOW_DUMP`: write the n-th frame, as rendered for this window, to a PNG.
        self.frames += 1;
        if self.dump.as_ref().is_some_and(|(_, nth)| self.frames == *nth) {
            let dl = dl.into_owned();
            if let Some((path, _)) = self.dump.take()
                && let Err(e) = self.dump_png(&dl, &path)
            {
                log::error!("ezviz: window dump failed: {e}");
            }
        }
    }

    /// Renders `dl` offscreen at the window's scale and writes it to a PNG.
    pub fn dump_png(&mut self, dl: &DrawList, path: &std::path::Path) -> Result<()> {
        let ppu = self.scale();
        let gfx = self.gfx.as_mut().ok_or_else(|| Error::Gpu("the window has no GPU context yet".into()))?;
        let (width, height, data) = gfx.renderer.render_rgba(dl, ppu)?;
        crate::figure::write_png(path, &crate::figure::RgbaImage { width, height, data }, ppu)
    }

    /// Writes the frame as currently shown, overlays included.
    pub fn dump_current(&mut self, path: &std::path::Path) -> Result<()> {
        self.rebuild_if_stale();
        let Some(b) = &self.built else { return Err(Error::EventLoop("no frame built yet".into())) };
        let dl = overlay::compose(&b.dl, &b.st, &b.views, &self.ui, self.hover.as_ref()).into_owned();
        self.dump_png(&dl, path)
    }
}

/// Freezes the tick-label space of the axes the effects move at its value in the built frame
/// (only axes not frozen yet) and extends the freeze to [`FREEZE_SECS`] from `now`.
fn freeze_layout(fig: &Figure, freeze: &mut Option<Freeze>, b: &Built, fx: &[Effect], now: f64) {
    let f = freeze.get_or_insert_with(|| Freeze { until: now, saved: Vec::new() });
    f.until = now + FREEZE_SECS;
    let new: Vec<(BlockId, [f64; 2])> = fx
        .iter()
        .filter_map(|e| match e {
            Effect::SetLimits { axis, .. } => b.frames.get(*axis),
            _ => None,
        })
        .filter(|fr| f.saved.iter().all(|(id, ..)| *id != fr.id))
        .map(|fr| (fr.id, crate::scene::axis::actual_ticklabelspace(fr)))
        .collect();
    if new.is_empty() {
        return;
    }
    fig.sh.update(Dirty::LAYOUT, |st| {
        for (id, [x, y]) in new {
            if f.saved.iter().any(|(s, ..)| *s == id) {
                continue;
            }
            if let Some(a) = st.block_mut(id).and_then(|b| b.as_axis_mut()) {
                let prev = (a.attrs.xticklabelspace, a.attrs.yticklabelspace);
                a.attrs.xticklabelspace = Some(Some(x));
                a.attrs.yticklabelspace = Some(Some(y));
                f.saved.push((id, prev.0, prev.1));
            }
        }
    });
}

/// Writes the effects into the figure state (short locks) and into the frame's views.
/// Returns whether the overlay needs a redraw (limit changes wake the window by themselves).
fn apply_effects(fig: &Figure, b: &mut Built, fx: &[Effect]) -> bool {
    let overlay = fx.iter().any(|e| matches!(e, Effect::Overlay));
    if fx.iter().all(|e| matches!(e, Effect::Overlay)) {
        return overlay;
    }
    let ids: Vec<_> = b.frames.iter().map(|f| f.id).collect();
    // Pan and zoom: set the interactive limits (already propagated to linked axes).
    // Resets: clear them, and snapshot the state to compute the reset target off the lock.
    let applied = fig.sh.update(Dirty::LIMITS, |st| {
        if ids.iter().any(|id| st.block(*id).and_then(|b| b.as_axis()).is_none()) {
            return None; // the layout changed since this frame; drop the stale interaction
        }
        let mut resets = Vec::new();
        for e in fx {
            match *e {
                Effect::SetLimits { axis, limits } => {
                    if let Some(a) = axis_mut(st, &ids, axis) {
                        a.interactive = Some(limits);
                    }
                }
                Effect::ResetLimits { axis } | Effect::AutoLimits { axis } => {
                    if let Some(a) = axis_mut(st, &ids, axis) {
                        a.interactive = None;
                        if matches!(e, Effect::AutoLimits { .. }) {
                            a.xlims = (None, None);
                            a.ylims = (None, None);
                        }
                        resets.push(axis);
                    }
                }
                Effect::Overlay => {}
            }
        }
        Some(if resets.is_empty() { None } else { Some((resets, st.clone())) })
    });
    let Some(resets) = applied else { return overlay };
    interact::apply_to_views(&mut b.views, fx);
    let Some((resets, snap)) = resets else { return overlay };
    // Reset target limits (data bounds are O(n): computed on the snapshot, not under the lock).
    let lims = crate::scene::axis::compute_limits(&snap, &ids, &snap.theme.globals());
    // Linked axes follow the reset axis in the linked dimension(s), like Makie's link
    // propagation. An axis linked in both dimensions goes back to automatic limits; an automatic
    // axis follows by itself (linked autolimits share their data bounds).
    // (axis, its new interactive limits, its new view limits)
    let mut follow: Vec<(usize, Option<[f64; 4]>, [f64; 4])> = Vec::new();
    for &a in &resets {
        for e in interact::propagate(&b.views, a, lims[a]) {
            let Effect::SetLimits { axis: o, limits } = e else { continue };
            let interactive = snap.block(ids[o]).and_then(|b| b.as_axis()).is_some_and(|x| x.interactive.is_some());
            if limits == lims[a] {
                follow.push((o, None, limits));
            } else if interactive {
                follow.push((o, Some(limits), limits));
            }
        }
    }
    if !follow.is_empty() {
        fig.sh.update(Dirty::LIMITS, |st| {
            for (o, l, _) in &follow {
                if let Some(a) = axis_mut(st, &ids, *o) {
                    a.interactive = *l;
                }
            }
        });
    }
    for (i, v) in b.views.iter_mut().enumerate() {
        if let Some((_, _, l)) = follow.iter().find(|f| f.0 == i) {
            v.limits = *l;
        } else if resets.contains(&i) {
            v.limits = lims[i];
        }
    }
    overlay
}

fn axis_mut<'a>(
    st: &'a mut FigState,
    ids: &[crate::figure::BlockId],
    i: usize,
) -> Option<&'a mut crate::blocks::axis::AxisState> {
    st.block_mut(*ids.get(i)?)?.as_axis_mut()
}

/// The window dump requested through the environment, if any.
fn dump_request() -> Option<(std::path::PathBuf, u64)> {
    let path = std::env::var_os("EZVIZ_WINDOW_DUMP")?;
    let nth = std::env::var("EZVIZ_WINDOW_DUMP_FRAME").ok().and_then(|s| s.parse().ok()).unwrap_or(1u64);
    Some((path.into(), nth.max(1)))
}

impl ApplicationHandler<UserEvent> for App<'_> {
    fn new_events(&mut self, el: &ActiveEventLoop, _cause: StartCause) {
        // `EZVIZ_AUTOCLOSE`: `about_to_wait` wakes the loop at the deadline.
        if self.autoclose.is_some_and(|t| Instant::now() >= t) {
            let ids: Vec<WindowId> = self.anims.keys().copied().collect();
            for id in ids {
                self.write_anim_dump(id);
            }
            self.wins.clear();
            if self.exit_when_empty {
                el.exit();
            }
            return;
        }
        if !self.to_open.is_empty() {
            self.open_pending(el);
        }
    }

    fn resumed(&mut self, el: &ActiveEventLoop) {
        self.resumed = true;
        #[cfg(target_arch = "wasm32")]
        super::web::drain_inbox(self);
        self.open_pending(el);
    }

    fn user_event(&mut self, el: &ActiveEventLoop, ev: UserEvent) {
        match ev {
            UserEvent::Wake(uid) => {
                for w in self.wins.values().filter(|w| w.fig.sh.uid == uid) {
                    w.window.request_redraw();
                }
            }
            UserEvent::Close(session) => {
                let ids: Vec<WindowId> = self
                    .wins
                    .iter()
                    .filter(|(_, w)| w.live.as_ref().is_some_and(|l| l.session == session))
                    .map(|(id, _)| *id)
                    .collect();
                for id in ids {
                    self.close(el, id);
                }
            }
            UserEvent::Panicked(session, msg) => {
                for w in self.wins.values().filter(|w| w.live.as_ref().is_some_and(|l| l.session == session)) {
                    let title = w.fig.sh.state.lock().window_title.clone().unwrap_or_else(|| "ezviz".into());
                    w.window.set_title(&format!("{title} — simulation panicked: {msg}"));
                }
            }
            #[cfg(target_arch = "wasm32")]
            UserEvent::Inbox => {
                if self.resumed {
                    super::web::drain_inbox(self);
                    self.open_pending(el);
                }
            }
            #[cfg(feature = "testing")]
            UserEvent::Test(uid, op) => {
                let t = self.now();
                for w in self.wins.values_mut().filter(|w| w.fig.sh.uid == uid) {
                    match &op {
                        super::testing::Op::Input(s) => {
                            let input = super::testing::translate_synthetic(*s, w.scale(), t);
                            w.input(input, t);
                        }
                        super::testing::Op::Minimize(on) => w.window.set_minimized(*on),
                        super::testing::Op::Dump(path, tx) => {
                            let _ = tx.send(w.dump_current(path).map_err(|e| e.to_string()));
                        }
                        super::testing::Op::Flush(_) => {}
                    }
                }
                if let super::testing::Op::Flush(tx) = op {
                    let _ = tx.send(());
                }
            }
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested | WindowEvent::Destroyed => self.close(el, id),
            WindowEvent::Resized(size) => {
                if let Some(w) = self.wins.get_mut(&id) {
                    w.resized([size.width, size.height]);
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(w) = self.wins.get(&id) {
                    w.window.request_redraw();
                }
            }
            WindowEvent::Occluded(occluded) => {
                if let Some(w) = self.wins.get_mut(&id) {
                    w.occluded = occluded;
                    if let Some(l) = &w.live {
                        l.set_occluded(occluded);
                    }
                    if !occluded {
                        w.window.request_redraw();
                    }
                }
            }
            WindowEvent::RedrawRequested => self.redraw(el, id),
            WindowEvent::Touch(t) => {
                let now = self.now();
                if let Some(w) = self.wins.get_mut(&id) {
                    let s = w.scale();
                    let p = [t.location.x / s, t.location.y / s];
                    for input in w.touches.handle(t.id, t.phase, p, now) {
                        w.input(input, now);
                    }
                }
            }
            ref ev => {
                let t = self.now();
                if let Some(w) = self.wins.get_mut(&id)
                    && let Some(input) = translate(ev, w.scale(), t, w.ui.mods)
                {
                    w.input(input, t);
                }
            }
        }
    }

    /// Sleeps until the next event, or until the earliest timer: the autoclose deadline, a
    /// frozen layout to restore, the occluded-window poll.
    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let now = self.now();
        let mut wake = self.poll_occluded(now);
        for w in self.wins.values_mut() {
            if let Some(t) = w.thaw_if_due(now) {
                wake = Some(wake.map_or(t, |w: f64| w.min(t)));
            }
        }
        let mut deadline = wake.map(|t| self.epoch + Duration::from_secs_f64(t.max(0.0)));
        if let Some(a) = self.autoclose {
            deadline = Some(deadline.map_or(a, |d| d.min(a)));
        }
        el.set_control_flow(deadline.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }
}

#[cfg(test)]
mod tests {
    use super::clamp_size;

    #[test]
    fn surface_size_is_clamped_to_the_texture_limit() {
        assert_eq!(clamp_size([1600, 800], 2048), ([1600, 800], 1.0));
        // A 4096 px wide canvas on a 2048 px device: half resolution, aspect kept.
        let (size, k) = clamp_size([4096, 1000], 2048);
        assert_eq!(size, [2048, 500]);
        assert!((k - 0.5).abs() < 1e-12);
        assert_eq!(clamp_size([1, 1], 2048), ([1, 1], 1.0));
    }
}
