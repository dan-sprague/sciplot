//! Native windows (winit + wgpu): blocking `show()`, live updates (`show_live`), pump mode
//! (`display` + `pump`), Makie's pan/zoom interactions and the hover inspector.
//!
//! One `App` (winit `ApplicationHandler`) owns the windows. Each window keeps the last built
//! frame (draw list, axis frames, the snapshot it came from); hover and the rectangle-zoom shade
//! are overlays appended to that draw list, so they never trigger a relayout.

pub mod interact;
mod live;
#[cfg(target_os = "macos")]
mod macos;
mod overlay;
mod pump;
#[cfg(feature = "testing")]
pub mod testing;

pub use live::Live;
pub use pump::Screen;

use crate::error::{Error, Result};
use crate::figure::{Dirty, FigState, Figure};
use crate::plots::pick::{Hover, PickCache};
use crate::render::gpu::{Gpu, Renderer, TARGET_FORMAT, gpu};
use crate::scene::AxisFrame;
use crate::scene::drawlist::DrawList;
use interact::{AxisView, Effect, Input, InteractState};
use live::LiveShared;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use web_time::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::platform::run_on_demand::EventLoopExtRunOnDemand;
use winit::window::{Window, WindowId};

#[derive(Debug, Clone)]
pub(crate) enum UserEvent {
    /// A figure with this uid changed.
    Wake(u64),
    /// Close the windows of this `show_live` session.
    Close(u64),
    /// The simulation of this `show_live` session panicked.
    Panicked(u64, String),
    /// Scripted-test operation for the windows of the figure with this uid.
    #[cfg(feature = "testing")]
    Test(u64, testing::Op),
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
            let el = EventLoop::<UserEvent>::with_user_event().build().map_err(|e| Error::EventLoop(e.to_string()))?;
            *slot = Some(el);
        }
        match slot.as_mut() {
            Some(el) => f(el),
            None => Err(Error::EventLoop("no event loop".into())),
        }
    })
}

/// The last built frame of a window.
struct Built {
    dl: DrawList,
    frames: Vec<AxisFrame>,
    /// Interaction views; updated in place by interactions until the next rebuild.
    views: Vec<AxisView>,
    /// The snapshot the frame was built from (hover picking reads it, never the live state).
    st: FigState,
    rev: u64,
    size: [f64; 2],
}

/// One open window.
struct Win {
    surface: wgpu::Surface<'static>,
    window: Arc<Window>,
    config: wgpu::SurfaceConfiguration,
    fig: Figure,
    renderer: Renderer,
    wake_id: u64,
    built: Option<Built>,
    /// Frames rendered so far (presented, or built while occluded for the dump).
    frames: u64,
    /// `EZVIZ_WINDOW_DUMP` path and the frame number to write (`EZVIZ_WINDOW_DUMP_FRAME`, 1).
    dump: Option<(std::path::PathBuf, u64)>,
    ui: InteractState,
    hover: Option<Hover>,
    picks: PickCache,
    live: Option<Arc<LiveShared>>,
    occluded: bool,
}

impl Drop for Win {
    fn drop(&mut self) {
        self.fig.sh.wake.detach(self.wake_id);
        if let Some(l) = &self.live {
            l.set_closed();
        }
    }
}

pub(crate) struct App {
    gpu: Arc<Gpu>,
    /// Figures to open, with a token reported in `opened` once their window exists.
    to_open: Vec<(Figure, u64)>,
    opened: Vec<(u64, WindowId)>,
    wins: HashMap<WindowId, Win>,
    proxy: EventLoopProxy<UserEvent>,
    error: Option<Error>,
    autoclose: Option<Instant>,
    /// `show`/`show_live` exit the loop when the last window closes; pump mode never exits.
    exit_when_empty: bool,
    live: Option<Arc<LiveShared>>,
    epoch: Instant,
}

impl App {
    fn new(
        gpu: Arc<Gpu>,
        proxy: EventLoopProxy<UserEvent>,
        exit_when_empty: bool,
        live: Option<Arc<LiveShared>>,
    ) -> App {
        let autoclose = std::env::var("EZVIZ_AUTOCLOSE")
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .map(|s| Instant::now() + Duration::from_secs_f64(s.max(0.0)));
        App {
            gpu,
            to_open: Vec::new(),
            opened: Vec::new(),
            wins: HashMap::new(),
            proxy,
            error: None,
            autoclose,
            exit_when_empty,
            live,
            epoch: Instant::now(),
        }
    }

    fn open_pending(&mut self, el: &ActiveEventLoop) {
        for (fig, token) in std::mem::take(&mut self.to_open) {
            match self.open(el, fig) {
                Ok(id) => self.opened.push((token, id)),
                Err(e) => {
                    self.error = Some(e);
                    if self.exit_when_empty {
                        el.exit();
                    }
                    return;
                }
            }
        }
    }

    fn open(&mut self, el: &ActiveEventLoop, fig: Figure) -> Result<WindowId> {
        let (size, title) = {
            let st = fig.sh.state.lock();
            (st.theme.globals().size, st.window_title.clone().unwrap_or_else(|| "ezviz".into()))
        };
        let attrs = Window::default_attributes().with_title(title).with_inner_size(LogicalSize::new(size[0], size[1]));
        let window = Arc::new(el.create_window(attrs).map_err(|e| Error::EventLoop(e.to_string()))?);
        let surface = self.gpu.instance.create_surface(window.clone()).map_err(|e| Error::Gpu(e.to_string()))?;
        let caps = surface.get_capabilities(&self.gpu.adapter);
        if !caps.formats.contains(&TARGET_FORMAT) {
            return Err(Error::Gpu(format!("surface does not support {TARGET_FORMAT:?}")));
        }
        let phys = window.inner_size();
        let alpha_mode = if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
            wgpu::CompositeAlphaMode::Opaque
        } else {
            caps.alpha_modes.first().copied().unwrap_or(wgpu::CompositeAlphaMode::Auto)
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
        let live = self.live.clone().filter(|l| l.fig_uid == uid);
        let id = window.id();
        let renderer = Renderer::new(self.gpu.clone());
        self.wins.insert(
            id,
            Win {
                surface,
                window,
                config,
                fig,
                renderer,
                wake_id,
                built: None,
                frames: 0,
                dump: dump_request(),
                ui: InteractState::default(),
                hover: None,
                picks: PickCache::default(),
                live,
                occluded: false,
            },
        );
        Ok(id)
    }

    fn close(&mut self, el: &ActiveEventLoop, id: WindowId) {
        self.wins.remove(&id);
        if self.exit_when_empty && self.wins.is_empty() {
            el.exit();
        }
    }

    /// Seconds since the app started (for double-click timing).
    fn now(&self) -> f64 {
        self.epoch.elapsed().as_secs_f64()
    }
}

impl Win {
    fn scale(&self) -> f64 {
        self.window.scale_factor()
    }

    /// Feeds one input to the interaction state machine and applies its effects.
    fn input(&mut self, input: Input) {
        // While frames are skipped (occluded window) nothing rebuilds the scene, so bring the
        // hit-test geometry up to date here.
        if self.occluded || self.built.is_none() {
            self.rebuild_if_stale();
        }
        let mut redraw = false;
        if let Some(b) = self.built.as_mut() {
            let fx = interact::handle(&mut self.ui, input, &b.views);
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
        testing::record_hover(self.fig.sh.uid, self.hover.as_ref().map(|h| h.text.as_str()));
        changed
    }

    /// Rebuilds the frame when the figure changed (outside a batch) or the window was resized.
    fn rebuild_if_stale(&mut self) {
        let ppu = self.scale();
        let size = [self.config.width as f64 / ppu, self.config.height as f64 / ppu];
        let (rev, batch_open) = {
            let st = self.fig.sh.state.lock();
            (st.rev, st.batch_depth > 0)
        };
        let stale = self.built.as_ref().is_none_or(|b| b.size != size || (b.rev != rev && !batch_open));
        if stale {
            let st = self.fig.sh.snapshot();
            let (dl, frames) = crate::scene::build(&st, Some(size), &mut self.renderer.scene);
            let views = overlay::views(&frames, &st);
            self.picks.retain(|uid| st.iter_plots().any(|(_, p)| p.uid == uid));
            #[cfg(feature = "testing")]
            testing::record_views(self.fig.sh.uid, &views);
            self.built = Some(Built { dl, frames, views, rev: st.rev, st, size });
            self.update_hover();
        }
    }

    fn render(&mut self, gpu: &Gpu) {
        let ppu = self.scale();
        self.fig.sh.wake.clear();
        // Acquire the surface first: an occluded (hidden, minimized) window skips the frame
        // without rebuilding the scene.
        let dump_pending = self.dump.is_some();
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) => {
                self.occluded = false;
                Some(f)
            }
            wgpu::CurrentSurfaceTexture::Suboptimal(f) => {
                drop(f);
                configure(gpu, &self.surface, &self.config);
                self.window.request_redraw();
                return;
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                configure(gpu, &self.surface, &self.config);
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
        let Some(b) = &self.built else { return };
        let built_rev = b.rev;
        let dl = overlay::compose(&b.dl, &b.st, &b.views, &self.ui, self.hover.as_ref());
        if let Some(frame) = frame {
            let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
            let cmd = self.renderer.render(&dl, &view, [self.config.width, self.config.height], ppu);
            gpu.queue.submit([cmd]);
            self.window.pre_present_notify();
            gpu.queue.present(frame);
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
    fn dump_png(&mut self, dl: &DrawList, path: &std::path::Path) -> Result<()> {
        let ppu = self.scale();
        let (width, height, data) = self.renderer.render_rgba(dl, ppu)?;
        crate::figure::write_png(path, &crate::figure::RgbaImage { width, height, data }, ppu)
    }

    /// Writes the frame as currently shown, overlays included (testing hook).
    #[cfg(feature = "testing")]
    fn dump_current(&mut self, path: &std::path::Path) -> Result<()> {
        let Some(b) = &self.built else { return Err(Error::EventLoop("no frame built yet".into())) };
        let dl = overlay::compose(&b.dl, &b.st, &b.views, &self.ui, self.hover.as_ref()).into_owned();
        self.dump_png(&dl, path)
    }
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

fn configure(gpu: &Gpu, surface: &wgpu::Surface<'static>, config: &wgpu::SurfaceConfiguration) {
    surface.configure(&gpu.device, config);
    #[cfg(target_os = "macos")]
    macos::force_srgb_colorspace(surface);
}

/// Physical cursor position -> figure units.
fn cursor_units(p: PhysicalPosition<f64>, scale: f64) -> [f64; 2] {
    [p.x / scale, p.y / scale]
}

/// Scroll delta -> wheel steps (trackpad pixels: `px / scale / 16`).
fn scroll_steps(d: MouseScrollDelta, scale: f64) -> [f64; 2] {
    match d {
        MouseScrollDelta::LineDelta(x, y) => [x as f64, y as f64],
        MouseScrollDelta::PixelDelta(p) => [p.x / scale / 16.0, p.y / scale / 16.0],
    }
}

/// Translates a winit window event into an interaction input.
fn translate(ev: &WindowEvent, scale: f64, time: f64) -> Option<Input> {
    Some(match ev {
        WindowEvent::CursorMoved { position, .. } => Input::CursorMoved(cursor_units(*position, scale)),
        WindowEvent::CursorLeft { .. } => Input::CursorLeft,
        WindowEvent::MouseInput { state, button, .. } => Input::Button {
            button: match button {
                MouseButton::Left => interact::Button::Left,
                MouseButton::Right => interact::Button::Right,
                MouseButton::Middle => interact::Button::Middle,
                _ => return None,
            },
            pressed: *state == ElementState::Pressed,
            time,
        },
        WindowEvent::MouseWheel { delta, .. } => Input::Scroll(scroll_steps(*delta, scale)),
        WindowEvent::PinchGesture { delta, .. } => Input::Pinch(*delta),
        WindowEvent::KeyboardInput { event, .. } if !event.repeat => {
            let key = match event.physical_key {
                PhysicalKey::Code(KeyCode::KeyX) => interact::Key::X,
                PhysicalKey::Code(KeyCode::KeyY) => interact::Key::Y,
                _ => return None,
            };
            Input::Key { key, pressed: event.state == ElementState::Pressed }
        }
        WindowEvent::ModifiersChanged(m) => {
            let s = m.state();
            Input::Modifiers(interact::Modifiers { ctrl: s.control_key(), shift: s.shift_key(), alt: s.alt_key() })
        }
        WindowEvent::Focused(false) => Input::FocusLost,
        _ => return None,
    })
}

#[cfg(feature = "testing")]
fn translate_synthetic(ev: testing::Synthetic, scale: f64, time: f64) -> Input {
    use testing::Synthetic as S;
    match ev {
        S::CursorMoved { x, y } => Input::CursorMoved(cursor_units(PhysicalPosition::new(x * scale, y * scale), scale)),
        S::CursorLeft => Input::CursorLeft,
        S::Button { button, pressed } => Input::Button { button, pressed, time },
        S::ScrollLines { dx, dy } => {
            Input::Scroll(scroll_steps(MouseScrollDelta::LineDelta(dx as f32, dy as f32), scale))
        }
        S::ScrollPixels { dx, dy } => Input::Scroll(scroll_steps(
            MouseScrollDelta::PixelDelta(PhysicalPosition::new(dx * scale, dy * scale)),
            scale,
        )),
        S::Pinch(d) => Input::Pinch(d),
        S::Key { key, pressed } => Input::Key { key, pressed },
        S::Modifiers(m) => Input::Modifiers(m),
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn new_events(&mut self, el: &ActiveEventLoop, _cause: StartCause) {
        if let Some(t) = self.autoclose {
            if Instant::now() >= t {
                self.wins.clear();
                if self.exit_when_empty {
                    el.exit();
                }
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
            #[cfg(feature = "testing")]
            UserEvent::Test(uid, op) => {
                let t = self.now();
                for w in self.wins.values_mut().filter(|w| w.fig.sh.uid == uid) {
                    match &op {
                        testing::Op::Input(s) => {
                            let input = translate_synthetic(*s, w.scale(), t);
                            w.input(input);
                        }
                        testing::Op::Minimize(on) => w.window.set_minimized(*on),
                        testing::Op::Dump(path, tx) => {
                            let _ = tx.send(w.dump_current(path).map_err(|e| e.to_string()));
                        }
                        testing::Op::Flush(_) => {}
                    }
                }
                if let testing::Op::Flush(tx) = op {
                    let _ = tx.send(());
                }
            }
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested | WindowEvent::Destroyed => self.close(el, id),
            WindowEvent::Resized(size) => {
                let gpu = self.gpu.clone();
                if let Some(w) = self.wins.get_mut(&id) {
                    w.config.width = size.width.max(1);
                    w.config.height = size.height.max(1);
                    configure(&gpu, &w.surface, &w.config);
                    w.window.request_redraw();
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
            WindowEvent::RedrawRequested => {
                let gpu = self.gpu.clone();
                if let Some(w) = self.wins.get_mut(&id) {
                    w.render(&gpu);
                }
            }
            ref ev => {
                let t = self.now();
                if let Some(w) = self.wins.get_mut(&id)
                    && let Some(input) = translate(ev, w.scale(), t)
                {
                    w.input(input);
                }
            }
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
        let mut app = App::new(gpu, el.create_proxy(), true, None);
        app.to_open = figs.iter().map(|f| ((*f).clone(), 0)).collect();
        el.set_control_flow(ControlFlow::Wait);
        let r = el.run_app_on_demand(&mut app).map_err(|e| Error::EventLoop(e.to_string()));
        app.wins.clear();
        if let Some(e) = app.error.take() {
            return Err(e);
        }
        r
    })
}

impl Figure {
    /// Opens the figure in a window and blocks until it is closed (Makie's `wait(display(fig))`).
    ///
    /// Interactions (Makie's): scroll to zoom about the cursor, right-drag (or Option/Alt +
    /// left-drag) to pan, left-drag to zoom into a rectangle; hold `x` or `y` to restrict any of
    /// them to one dimension; Ctrl+click (or double-click) resets the limits and
    /// Ctrl+Shift+click returns to full autolimits. Hovering a point shows its coordinates
    /// (turn off with [`Figure::datainspector`]). Must be called on the main thread; to run a
    /// simulation while the window is open use [`Figure::show_live`].
    pub fn show(&self) -> Result<()> {
        show_all(&[self])
    }
}
