//! `fig.animate(|frame| ...)`: a per-frame callback on the event-loop thread.
//!
//! The callback runs once per displayed frame, right before the frame is built, so every change
//! it makes shows in that frame. Nothing here blocks or spawns threads: the loop is driven by the
//! window system's redraw requests (vsync-paced natively, `requestAnimationFrame` in the
//! browser), and time comes from the event loop's monotonic clock through [`Ticker`]. The
//! `Figure::animate` entry points live in `native.rs` and `web.rs`.

use super::app::App;
use std::path::PathBuf;
use std::time::Duration;
use winit::window::WindowId;

/// How often an occluded (hidden, minimized) animated native window checks whether it is visible
/// again. Like `requestAnimationFrame` in a background tab, the callback does not run meanwhile.
const OCCLUDED_POLL: Duration = Duration::from_millis(250);

const MAX_DT: f64 = Frame::MAX_DT;

/// One animation frame, passed to the [`Figure::animate`](crate::Figure::animate) callback.
///
/// `t`, `dt` and `count` describe the frame; [`stop`](Frame::stop) and [`close`](Frame::close)
/// control the loop. Changes made through plot and axis handles during the callback appear in
/// this frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    /// Animation time in seconds: 0 on the first frame, then the sum of the `dt`s.
    pub t: f64,
    /// Seconds since the previous frame (0 on the first frame), at most [`Frame::MAX_DT`].
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
    /// Close the window.
    Close,
}

impl Frame {
    /// The longest `dt` a frame reports. Longer gaps between frames (a hidden browser tab, a
    /// minimized window, a stall) count as this much, so simulations stepping by `dt` stay
    /// stable and `t` does not jump.
    pub const MAX_DT: f64 = 0.25;

    /// Stops the animation after this frame: the callback is not called again, and the window
    /// stays open (still interactive) until the user closes it.
    pub fn stop(&mut self) {
        if self.control == Control::Continue {
            self.control = Control::Stop;
        }
    }

    /// Closes the window after this frame (natively `animate` then returns; in the browser the
    /// figure is unmounted and a canvas ezviz created is removed).
    pub fn close(&mut self) {
        self.control = Control::Close;
    }

    /// Whether [`stop`](Frame::stop) or [`close`](Frame::close) was called in this frame.
    pub fn is_stopped(&self) -> bool {
        self.control != Control::Continue
    }
}

/// Turns monotonic timestamps (seconds, any origin) into [`Frame`]s. Platform independent: the
/// event loop feeds it its clock at every redraw (vsync natively, `requestAnimationFrame` in the
/// browser).
#[derive(Debug, Default, Clone)]
pub(crate) struct Ticker {
    last: Option<f64>,
    t: f64,
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
        // Clamp: time never runs backwards, and long gaps count as MAX_DT.
        let dt = self.last.map_or(0.0, |last| (now - last).clamp(0.0, MAX_DT));
        self.t += dt;
        let mut frame = Frame { t: self.t, dt, count: self.count, control: Control::Continue };
        f(&mut frame);
        self.last = Some(self.last.map_or(now, |l| l.max(now)));
        self.count += 1;
        self.done = frame.control != Control::Continue;
        frame.control
    }

    /// Whether the callback stopped the animation.
    pub(crate) fn done(&self) -> bool {
        self.done
    }
}

/// A per-frame callback ([`Figure::animate`](crate::Figure::animate)).
pub(crate) type AnimFn<'f> = Box<dyn FnMut(&mut Frame) + 'f>;

/// The animation of one window.
pub(crate) struct Anim<'f> {
    f: AnimFn<'f>,
    ticker: Ticker,
    /// `EZVIZ_WINDOW_DUMP` without `EZVIZ_WINDOW_DUMP_FRAME`: written from the last frame when the
    /// window closes (the first frame of an animation is rarely the interesting one).
    dump_last: Option<PathBuf>,
}

impl<'f> Anim<'f> {
    pub fn new(f: AnimFn<'f>, dump_last: Option<PathBuf>) -> Anim<'f> {
        Anim { f, ticker: Ticker::default(), dump_last }
    }
}

impl App<'_> {
    /// Runs the animation callback of window `id` for the frame about to be drawn (only when the
    /// window can show it). Returns whether the callback asked to close the window.
    pub(super) fn tick(&mut self, id: WindowId) -> bool {
        let now = self.now();
        let Some(a) = self.anims.get_mut(&id) else { return false };
        if a.ticker.done() || !self.wins.get(&id).is_some_and(|w| w.can_render()) {
            return false;
        }
        a.ticker.tick(now, &mut *a.f) == Control::Close
    }

    /// Continuous redraw for a running animation, paced by vsync / `requestAnimationFrame`.
    /// Windows that can't render yet (GPU not ready, occluded) are woken by the event that
    /// changes that (GPU ready, `Occluded(false)`, the occluded poll).
    pub(super) fn request_next_frame(&self, id: WindowId) {
        if self.anims.get(&id).is_some_and(|a| !a.ticker.done())
            && let Some(w) = self.wins.get(&id)
            && w.can_render()
        {
            w.window.request_redraw();
        }
    }

    /// Writes the pending last-frame window dump of `id` (before the window goes away).
    pub(super) fn write_anim_dump(&mut self, id: WindowId) {
        let Some(path) = self.anims.get_mut(&id).and_then(|a| a.dump_last.take()) else { return };
        let Some(w) = self.wins.get_mut(&id) else { return };
        if let Err(e) = w.dump_current(&path) {
            log::error!("ezviz: window dump failed: {e}");
        }
    }

    /// Native: an occluded window gets no vsync-paced redraws, so animated windows probe for
    /// visibility at a low rate instead of spinning. (Browsers report visibility changes.)
    /// Returns the app time of the next probe, if any window needs one.
    pub(super) fn poll_occluded(&mut self, now: f64) -> Option<f64> {
        if cfg!(target_arch = "wasm32") {
            return None;
        }
        let occluded: Vec<WindowId> = self
            .anims
            .iter()
            .filter(|(id, a)| !a.ticker.done() && self.wins.get(id).is_some_and(|w| w.occluded))
            .map(|(id, _)| *id)
            .collect();
        if occluded.is_empty() {
            return None;
        }
        let period = OCCLUDED_POLL.as_secs_f64();
        if now - self.occluded_poll >= period {
            self.occluded_poll = now;
            for id in &occluded {
                if let Some(w) = self.wins.get(id) {
                    w.window.request_redraw();
                }
            }
        }
        Some(self.occluded_poll + period)
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
        let mut seen = Vec::new();
        let mut f = |fr: &mut Frame| seen.push((fr.t, fr.dt));
        tk.tick(5.0, &mut f);
        tk.tick(4.0, &mut f);
        tk.tick(5.1, &mut f);
        assert_eq!(seen[1], (0.0, 0.0));
        let (t, dt) = seen[2];
        assert!((t - 0.1).abs() < 1e-9 && (dt - 0.1).abs() < 1e-9, "{seen:?}");
    }

    #[test]
    fn ticker_clamps_long_gaps() {
        // A hidden tab: requestAnimationFrame pauses for 30 s.
        let mut tk = Ticker::default();
        let mut seen = Vec::new();
        let mut f = |fr: &mut Frame| seen.push((fr.t, fr.dt));
        tk.tick(1.0, &mut f);
        tk.tick(1.016, &mut f);
        tk.tick(31.016, &mut f);
        let (t, dt) = seen[2];
        assert!((dt - MAX_DT).abs() < 1e-9 && (t - (0.016 + MAX_DT)).abs() < 1e-9, "{seen:?}");
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
