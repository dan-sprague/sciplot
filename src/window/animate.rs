//! `fig.animate(|frame| ...)`: a per-frame callback on the event-loop thread.
//!
//! The callback runs once per displayed frame, right before the frame is built, so every change
//! it makes shows in that frame. Nothing here blocks or spawns threads: the loop is driven by the
//! window system's redraw requests (vsync-paced), and time comes from the event loop's monotonic
//! clock. The same [`Ticker`] can be driven by a browser's `requestAnimationFrame` timestamps.

use super::{App, UserEvent, overlay, with_event_loop};
use crate::error::{Error, Result};
use crate::figure::Figure;
use std::path::PathBuf;
use std::time::Duration;
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::platform::run_on_demand::EventLoopExtRunOnDemand;
use winit::window::WindowId;

/// How often an occluded (hidden, minimized) animated window checks whether it is visible again.
/// Like `requestAnimationFrame` in a background tab, the callback does not run meanwhile.
const OCCLUDED_POLL: Duration = Duration::from_millis(250);

/// One animation frame, passed to the [`Figure::animate`] callback.
///
/// `t`, `dt` and `count` describe the frame; [`stop`](Frame::stop) and [`close`](Frame::close)
/// control the loop. Changes made through plot and axis handles during the callback appear in
/// this frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    /// Seconds since the first frame (0 on the first frame).
    pub t: f64,
    /// Seconds since the previous frame (0 on the first frame).
    pub dt: f64,
    /// Number of this frame: 0, 1, 2, ...
    pub count: u64,
    control: Control,
}

/// What the callback asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Control {
    Continue,
    /// Stop calling the callback; the window stays open and interactive.
    Stop,
    /// Close the window; `animate` returns.
    Close,
}

impl Frame {
    /// Stops the animation after this frame: the callback is not called again, and the window
    /// stays open (still interactive) until the user closes it.
    pub fn stop(&mut self) {
        if self.control == Control::Continue {
            self.control = Control::Stop;
        }
    }

    /// Closes the window after this frame; [`Figure::animate`] then returns.
    pub fn close(&mut self) {
        self.control = Control::Close;
    }

    /// Whether [`stop`](Frame::stop) or [`close`](Frame::close) was called in this frame.
    pub fn is_stopped(&self) -> bool {
        self.control != Control::Continue
    }
}

/// Turns monotonic timestamps (seconds, any origin) into [`Frame`]s. Platform independent: the
/// native loop feeds it the event loop's clock, a browser would feed it `requestAnimationFrame`
/// timestamps.
#[derive(Debug, Default, Clone)]
pub(crate) struct Ticker {
    start: Option<f64>,
    last: f64,
    count: u64,
    done: bool,
}

impl Ticker {
    /// Runs `f` for the frame displayed at time `now` (seconds). Returns what the callback asked
    /// for; once it returned `Stop` or `Close`, later ticks do nothing and return the same.
    pub(crate) fn tick(&mut self, now: f64, f: &mut dyn FnMut(&mut Frame)) -> Control {
        if self.done {
            return Control::Stop;
        }
        let start = *self.start.get_or_insert(now);
        // Clamp: timestamps never run backwards for the callback.
        let now = now.max(self.last).max(start);
        let dt = if self.count == 0 { 0.0 } else { now - self.last };
        let mut frame = Frame { t: now - start, dt, count: self.count, control: Control::Continue };
        f(&mut frame);
        self.last = now;
        self.count += 1;
        self.done = frame.control != Control::Continue;
        frame.control
    }

    /// Whether the callback stopped the animation.
    pub(crate) fn done(&self) -> bool {
        self.done
    }
}

/// Wraps the window [`App`] and ticks the callback before each redraw of the animated window.
struct Animator<'f> {
    app: App,
    f: &'f mut dyn FnMut(&mut Frame),
    ticker: Ticker,
    token: u64,
    win: Option<WindowId>,
    /// `EZVIZ_WINDOW_DUMP` without `EZVIZ_WINDOW_DUMP_FRAME`: written from the last frame when the
    /// window closes (the first frame of an animation is rarely the interesting one).
    dump_last: Option<PathBuf>,
    /// Last redraw request of an occluded window (seconds on the app clock).
    occluded_poll: f64,
}

impl Animator<'_> {
    /// Learns the window id once the app opened it and moves the window dump to the last frame.
    fn adopt_window(&mut self) {
        if self.win.is_some() {
            return;
        }
        let Some(i) = self.app.opened.iter().position(|(t, _)| *t == self.token) else { return };
        let (_, id) = self.app.opened.swap_remove(i);
        self.win = Some(id);
        if std::env::var_os("EZVIZ_WINDOW_DUMP_FRAME").is_none()
            && let Some(w) = self.app.wins.get_mut(&id)
        {
            self.dump_last = w.dump.take().map(|(p, _)| p);
        }
    }

    fn animating(&self) -> bool {
        !self.ticker.done() && self.win.is_some_and(|id| self.app.wins.contains_key(&id))
    }

    /// Writes the pending last-frame window dump (before the window goes away).
    fn write_dump(&mut self) {
        let Some(path) = self.dump_last.take() else { return };
        let Some(w) = self.win.and_then(|id| self.app.wins.get_mut(&id)) else { return };
        w.rebuild_if_stale();
        let Some(b) = &w.built else { return };
        let dl = overlay::compose(&b.dl, &b.st, &b.views, &w.ui, w.hover.as_ref()).into_owned();
        if let Err(e) = w.dump_png(&dl, &path) {
            log::error!("ezviz: window dump failed: {e}");
        }
    }

    /// Whether `EZVIZ_AUTOCLOSE` fires within the next millisecond.
    fn autoclose_due(&self) -> bool {
        self.app.autoclose.is_some_and(|t| !t.checked_sub(Duration::from_millis(1)).unwrap_or(t).elapsed().is_zero())
    }
}

impl ApplicationHandler<UserEvent> for Animator<'_> {
    fn new_events(&mut self, el: &ActiveEventLoop, cause: StartCause) {
        if self.autoclose_due() {
            self.write_dump();
        }
        self.app.new_events(el, cause);
        self.adopt_window();
    }

    fn resumed(&mut self, el: &ActiveEventLoop) {
        self.app.resumed(el);
        self.adopt_window();
    }

    fn user_event(&mut self, el: &ActiveEventLoop, ev: UserEvent) {
        self.app.user_event(el, ev);
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if Some(id) != self.win {
            self.app.window_event(el, id, event);
            return;
        }
        match event {
            WindowEvent::RedrawRequested => {
                let visible = self.app.wins.get(&id).is_some_and(|w| !w.occluded);
                if visible && !self.ticker.done() {
                    let now = self.app.now();
                    if self.ticker.tick(now, &mut *self.f) == Control::Close {
                        self.write_dump();
                        self.app.close(el, id);
                        return;
                    }
                }
                self.app.window_event(el, id, event);
                // Continuous redraw, paced by the surface's vsync (occluded: see about_to_wait).
                if let Some(w) = self.app.wins.get(&id)
                    && !w.occluded
                    && !self.ticker.done()
                {
                    w.window.request_redraw();
                }
            }
            WindowEvent::CloseRequested | WindowEvent::Destroyed => {
                self.write_dump();
                self.app.window_event(el, id, event);
            }
            event => self.app.window_event(el, id, event),
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if !self.animating() {
            return;
        }
        let Some(w) = self.win.and_then(|id| self.app.wins.get(&id)) else { return };
        if !w.occluded {
            return;
        }
        // Occluded: no vsync pacing, so probe for visibility at a low rate instead of spinning.
        let now = self.app.now();
        if now - self.occluded_poll >= OCCLUDED_POLL.as_secs_f64() {
            self.occluded_poll = now;
            w.window.request_redraw();
        }
        let wake = std::time::Instant::now() + OCCLUDED_POLL;
        let wake = self.app.autoclose.map_or(wake, |t| t.min(wake));
        el.set_control_flow(ControlFlow::WaitUntil(wake));
    }
}

impl Figure {
    /// Opens the figure in a window and calls `f` once per displayed frame until the window is
    /// closed (Makie's `record`/`on(events.tick)` loop, for live animations and simulations).
    ///
    /// `f` runs on the event-loop (main) thread right before each frame is drawn, so the changes
    /// it makes through plot and axis handles appear in that frame. [`Frame`] gives the time
    /// since the first frame (`t`), the time since the previous one (`dt`) and the frame number
    /// (`count`); [`Frame::stop`] ends the animation (the window stays open) and
    /// [`Frame::close`] closes the window. Frames are paced by the display (vsync); while the
    /// window is hidden or minimized `f` is not called. Pan, zoom and hover keep working during
    /// the animation. Keep `f` short (a few simulation steps per frame): the window does not
    /// respond while it runs. For long-running work on a separate thread use
    /// [`Figure::show_live`].
    ///
    /// Must be called on the main thread. With `EZVIZ_WINDOW_DUMP=<png>` the last frame before
    /// the window closes is written (or frame `EZVIZ_WINDOW_DUMP_FRAME`, if set).
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
        let gpu = crate::render::gpu::gpu()?;
        with_event_loop(|el| {
            let token = crate::figure::next_uid();
            let mut app = App::new(gpu, el.create_proxy(), true, None);
            app.to_open.push((self.clone(), token));
            let mut anim = Animator {
                app,
                f: &mut f,
                ticker: Ticker::default(),
                token,
                win: None,
                dump_last: None,
                occluded_poll: 0.0,
            };
            el.set_control_flow(ControlFlow::Wait);
            let r = el.run_app_on_demand(&mut anim).map_err(|e| Error::EventLoop(e.to_string()));
            anim.write_dump();
            anim.app.wins.clear();
            if let Some(e) = anim.app.error.take() {
                return Err(e);
            }
            r
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticker_times_and_counts() {
        let mut tk = Ticker::default();
        let mut seen = Vec::new();
        let mut f = |fr: &mut Frame| seen.push((fr.t, fr.dt, fr.count));
        for now in [10.0, 10.016, 10.033, 10.05] {
            assert_eq!(tk.tick(now, &mut f), Control::Continue);
        }
        let want = [(0.0, 0.0, 0), (0.016, 0.016, 1), (0.033, 0.017, 2), (0.05, 0.017, 3)];
        for ((t, dt, c), (wt, wdt, wc)) in seen.iter().zip(want) {
            assert!((t - wt).abs() < 1e-9 && (dt - wdt).abs() < 1e-9 && *c == wc, "{seen:?}");
        }
    }

    #[test]
    fn ticker_clamps_backwards_time() {
        let mut tk = Ticker::default();
        let mut last = None;
        let mut f = |fr: &mut Frame| last = Some((fr.t, fr.dt));
        tk.tick(5.0, &mut f);
        tk.tick(4.0, &mut f);
        assert_eq!(last, Some((0.0, 0.0)));
    }

    #[test]
    fn ticker_stop_and_close() {
        let mut tk = Ticker::default();
        let mut calls = 0;
        let mut f = |fr: &mut Frame| {
            calls += 1;
            if fr.count == 2 {
                fr.stop();
                assert!(fr.is_stopped());
            }
        };
        assert_eq!(tk.tick(0.0, &mut f), Control::Continue);
        assert_eq!(tk.tick(0.1, &mut f), Control::Continue);
        assert_eq!(tk.tick(0.2, &mut f), Control::Stop);
        assert_eq!(tk.tick(0.3, &mut f), Control::Stop);
        assert!(tk.done());
        assert_eq!(calls, 3);

        let mut tk = Ticker::default();
        let mut f = |fr: &mut Frame| {
            fr.close();
            fr.stop(); // close wins
        };
        assert_eq!(tk.tick(0.0, &mut f), Control::Close);
    }
}
