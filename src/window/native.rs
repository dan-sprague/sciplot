//! Native entry points: `show()` and `animate()` block on the main thread until the windows
//! close (winit `run_app_on_demand`, one event loop per thread, reused across calls).

use super::animate::Frame;
use super::app::{App, Gfx, OpenReq};
use super::UserEvent;
use crate::error::{Error, Result};
use crate::figure::Figure;
use crate::render::gpu::{Gpu, gpu};
use std::cell::RefCell;
use std::sync::Arc;
use winit::dpi::LogicalSize;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::platform::run_on_demand::EventLoopExtRunOnDemand;
use winit::window::Window;

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

pub(crate) fn with_event_loop<R>(f: impl FnOnce(&mut EventLoop<UserEvent>) -> Result<R>) -> Result<R> {
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

/// Opens an OS window of `size` units and its surface on the shared device.
pub(super) fn create_window(
    el: &ActiveEventLoop,
    gpu: Arc<Gpu>,
    size: [f64; 2],
    title: &str,
) -> Result<(Arc<Window>, Gfx)> {
    let attrs = Window::default_attributes().with_title(title).with_inner_size(LogicalSize::new(size[0], size[1]));
    let window = Arc::new(el.create_window(attrs).map_err(|e| Error::EventLoop(e.to_string()))?);
    let surface = gpu.instance.create_surface(window.clone()).map_err(|e| Error::Gpu(e.to_string()))?;
    let gfx = Gfx::new(gpu, surface)?;
    Ok((window, gfx))
}

/// Runs `app` until its last window closes and returns the first error it recorded.
fn run(el: &mut EventLoop<UserEvent>, app: &mut App<'_>) -> Result<()> {
    el.set_control_flow(ControlFlow::Wait);
    let r = el.run_app_on_demand(app).map_err(|e| Error::EventLoop(e.to_string()));
    let ids: Vec<_> = app.anims.keys().copied().collect();
    for id in ids {
        app.write_anim_dump(id);
    }
    app.wins.clear();
    if let Some(e) = app.error.take() {
        return Err(e);
    }
    r
}

/// Opens each figure in its own window and blocks until all are closed.
///
/// Must be called on the main thread. Set `EZVIZ_AUTOCLOSE=<seconds>` to close automatically
/// (useful for smoke tests). In the browser (wasm32) each figure is mounted into a canvas
/// appended to the page and this returns at once.
pub fn show_all(figs: &[&Figure]) -> Result<()> {
    let gpu = gpu()?;
    with_event_loop(|el| {
        let mut app = App::new(Some(gpu), el.create_proxy(), true, None);
        app.to_open = figs.iter().map(|f| OpenReq::new(f, 0)).collect();
        run(el, &mut app)
    })
}

impl Figure {
    /// Opens the figure in a window and blocks until it is closed (Makie's `wait(display(fig))`).
    ///
    /// Interactions (Makie's): scroll to zoom about the cursor, right-drag (or Option/Alt +
    /// left-drag) to pan, left-drag to zoom into a rectangle; hold `x` or `y` to restrict any of
    /// them to one dimension; Ctrl+click (or double-click) resets the limits and
    /// Ctrl+Shift+click returns to full autolimits. Trackpads pinch-zoom; touch screens pan with
    /// one finger and pinch-zoom with two. Hovering a point shows its coordinates (turn off with
    /// [`Figure::datainspector`]). Must be called on the main thread; to run a simulation while
    /// the window is open use [`Figure::animate`] or [`Figure::show_live`].
    ///
    /// In the browser (wasm32) the figure is mounted into a new `<canvas>` appended to the page
    /// (CSS size = figure size) and this returns at once; see also `Figure::show_in`.
    pub fn show(&self) -> Result<()> {
        show_all(&[self])
    }

    /// Opens the figure in a window and calls `f` once per displayed frame until the window is
    /// closed (Makie's `record`/`on(events.tick)` loop, for live animations and simulations).
    ///
    /// `f` runs on the event-loop (main) thread right before each frame is drawn, so the changes
    /// it makes through plot and axis handles appear in that frame. [`Frame`] gives the time
    /// since the first frame (`t`), the time since the previous one (`dt`, at most
    /// [`Frame::MAX_DT`]) and the frame number (`count`); [`Frame::stop`] ends the
    /// animation (the window stays open) and [`Frame::close`] closes the window. Frames are paced
    /// by the display (vsync); while the window is hidden or minimized `f` is not called. Pan,
    /// zoom and hover keep working during the animation. Keep `f` short (a few simulation steps
    /// per frame): the window does not respond while it runs. For long-running work on a
    /// separate thread use [`Figure::show_live`].
    ///
    /// Must be called on the main thread. With `EZVIZ_WINDOW_DUMP=<png>` the last frame before
    /// the window closes is written (or frame `EZVIZ_WINDOW_DUMP_FRAME`, if set).
    ///
    /// The same code runs in the browser (wasm32), driven by `requestAnimationFrame`: there
    /// `animate` mounts the figure into a new canvas and returns at once, so `f` must be
    /// `'static` (move owned handles into it).
    ///
    /// ```no_run
    /// use ezviz::prelude::*;
    /// let fig = Figure::new();
    /// let ax = Axis::new(fig.at(1, 1)).limits(-1.2, 1.2, -1.2, 1.2);
    /// let sc = ax.scatter([1.0], [0.0]);
    /// fig.animate(|frame| {
    ///     let a = 2.0 * frame.t;
    ///     sc.set_data([a.cos()], [a.sin()]);
    ///     ax.title(format!("frame {}", frame.count));
    ///     if frame.t > 10.0 {
    ///         frame.stop();
    ///     }
    /// })?;
    /// # Ok::<(), ezviz::Error>(())
    /// ```
    pub fn animate(&self, mut f: impl FnMut(&mut Frame)) -> Result<()> {
        let gpu = gpu()?;
        with_event_loop(|el| {
            let mut app = App::new(Some(gpu), el.create_proxy(), true, None);
            let mut req = OpenReq::new(self, crate::figure::next_uid());
            req.anim = Some(Box::new(&mut f));
            app.to_open.push(req);
            run(el, &mut app)
        })
    }
}
