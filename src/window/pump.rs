//! Pump mode: `fig.display()` opens a window and returns; the caller's loop drives it with
//! `screen.pump()`.

use super::app::{App, OpenReq};
use super::native::with_event_loop;
use crate::error::{Error, Result};
use crate::figure::Figure;
use std::cell::{Cell, RefCell};
use std::marker::PhantomData;
use std::time::Duration;
use web_time::Instant;
use winit::platform::pump_events::EventLoopExtPumpEvents;
use winit::window::WindowId;

/// Pumps closer together than this only check a timestamp (~120 Hz).
const PUMP_INTERVAL: Duration = Duration::from_millis(8);

thread_local! {
    /// The one application handler shared by all pump-mode windows of this (main) thread.
    static PUMP_APP: RefCell<Option<App<'static>>> = const { RefCell::new(None) };
}

/// Runs `f` with the event loop and the shared pump-mode app.
fn with_pump<R>(
    f: impl FnOnce(&mut winit::event_loop::EventLoop<super::UserEvent>, &mut App<'static>) -> Result<R>,
) -> Result<R> {
    with_event_loop(|el| {
        PUMP_APP.with(|cell| {
            let mut slot = cell.try_borrow_mut().map_err(|_| Error::Reentrant)?;
            if slot.is_none() {
                *slot = Some(App::new(Some(crate::render::gpu::gpu()?), el.create_proxy(), false, None));
            }
            let app = slot.as_mut().expect("pump app was just created");
            f(el, app)
        })
    })
}

/// A window opened by [`Figure::display`], driven by [`Screen::pump`] from the caller's loop.
///
/// `Screen` is tied to the main thread:
///
/// ```compile_fail
/// fn assert_send<T: Send>() {}
/// assert_send::<ezviz::Screen>();
/// ```
///
/// **Caveat (macOS):** while the user live-resizes the window, macOS runs a modal loop inside
/// `pump()`, so the caller's loop (and its simulation) stalls until the resize ends. Use
/// [`Figure::show_live`] if that matters.
pub struct Screen {
    id: WindowId,
    last_pump: Cell<Option<Instant>>,
    _main_thread: PhantomData<*const ()>,
}

impl Figure {
    /// Opens the figure in a window and returns immediately (Makie's `display(fig)`). Keep the
    /// window responsive by calling [`Screen::pump`] regularly, e.g. once per simulation step.
    /// Must be called on the main thread.
    ///
    /// ```no_run
    /// use ezviz::prelude::*;
    /// let fig = Figure::new();
    /// let sc = Axis::new(fig.at(1, 1)).scatter([0.0], [0.0]);
    /// let screen = fig.display()?;
    /// let mut t = 0.0f64;
    /// while screen.is_open() {
    ///     t += 0.01;
    ///     sc.set_data([t.cos()], [t.sin()]);
    ///     screen.pump()?;
    /// }
    /// # Ok::<(), ezviz::Error>(())
    /// ```
    pub fn display(&self) -> Result<Screen> {
        let token = crate::figure::next_uid();
        with_pump(|el, app| {
            app.to_open.push(OpenReq::new(self, token));
            let start = Instant::now();
            loop {
                let _ = el.pump_app_events(Some(Duration::ZERO), app);
                if let Some(e) = app.error.take() {
                    return Err(e);
                }
                if let Some(i) = app.opened.iter().position(|(t, _)| *t == token) {
                    let (_, id) = app.opened.swap_remove(i);
                    return Ok(Screen { id, last_pump: Cell::new(None), _main_thread: PhantomData });
                }
                if start.elapsed() > Duration::from_secs(10) {
                    app.to_open.retain(|r| r.token != token);
                    return Err(Error::EventLoop("the window did not open".into()));
                }
                std::thread::yield_now();
            }
        })
    }
}

impl Screen {
    /// Processes pending window events (input, resize, redraws). Cheap to call often: calls
    /// closer than 8 ms apart return at once.
    pub fn pump(&self) -> Result<()> {
        let now = Instant::now();
        if self.last_pump.get().is_some_and(|t| now - t < PUMP_INTERVAL) {
            return Ok(());
        }
        self.last_pump.set(Some(now));
        with_pump(|el, app| {
            let _ = el.pump_app_events(Some(Duration::ZERO), app);
            app.error.take().map_or(Ok(()), Err)
        })
    }

    /// Whether the window is still open (the user has not closed it).
    pub fn is_open(&self) -> bool {
        PUMP_APP.with(|cell| match cell.try_borrow() {
            Ok(app) => app.as_ref().is_some_and(|a| a.wins.contains_key(&self.id)),
            Err(_) => true,
        })
    }

    /// Blocks, handling events, until the user closes the window.
    pub fn wait(self) -> Result<()> {
        while self.is_open() {
            with_pump(|el, app| {
                let _ = el.pump_app_events(None, app);
                app.error.take().map_or(Ok(()), Err)
            })?;
        }
        Ok(())
    }

    /// Closes the window.
    pub fn close(self) {}
}

impl Drop for Screen {
    fn drop(&mut self) {
        PUMP_APP.with(|cell| {
            if let Ok(mut app) = cell.try_borrow_mut()
                && let Some(app) = app.as_mut()
            {
                app.wins.remove(&self.id);
            }
        });
    }
}
