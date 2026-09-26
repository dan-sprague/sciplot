//! `fig.show_live(|live| ...)`: a simulation on a scoped worker thread, the window on the main
//! thread.

use super::{App, UserEvent, with_event_loop};
use crate::error::{Error, Result};
use crate::figure::Figure;
use parking_lot::{Condvar, Mutex};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use winit::event_loop::EventLoopProxy;
use winit::platform::run_on_demand::EventLoopExtRunOnDemand;

/// State shared by the worker (`Live`) and the event loop.
pub(crate) struct LiveShared {
    /// Identifies this `show_live` call in user events.
    pub session: u64,
    pub fig_uid: u64,
    open: AtomicBool,
    occluded: AtomicBool,
    /// Revision of the last presented (or skipped) snapshot.
    rendered: Mutex<u64>,
    cv: Condvar,
    proxy: Mutex<EventLoopProxy<UserEvent>>,
    panic: Mutex<Option<String>>,
}

impl LiveShared {
    pub(crate) fn send(&self, e: UserEvent) {
        let _ = self.proxy.lock().send_event(e);
    }

    /// The window presented the snapshot with revision `rev`.
    pub fn presented(&self, rev: u64) {
        let mut r = self.rendered.lock();
        if rev > *r {
            *r = rev;
        }
        self.cv.notify_all();
    }

    /// The window is occluded or minimized (frames are skipped, waiters must not block).
    pub fn set_occluded(&self, on: bool) {
        self.occluded.store(on, Ordering::Release);
        let _g = self.rendered.lock();
        self.cv.notify_all();
    }

    /// The window closed (or the event loop ended): `is_open()` turns false, waiters wake.
    pub fn set_closed(&self) {
        self.open.store(false, Ordering::Release);
        let _g = self.rendered.lock();
        self.cv.notify_all();
    }

    pub fn panic_message(&self) -> Option<String> {
        self.panic.lock().clone()
    }
}

/// The handle a `show_live` simulation receives. `Send + Sync`; every method is cheap and never
/// blocks indefinitely.
pub struct Live {
    pub(crate) sh: Arc<LiveShared>,
    fig: Figure,
}

impl Live {
    /// Whether the window is still open. Loop on this in the simulation.
    pub fn is_open(&self) -> bool {
        self.sh.open.load(Ordering::Acquire)
    }

    /// Closes the window (`show_live` then returns once the simulation closure returns).
    pub fn close(&self) {
        self.sh.set_closed();
        self.sh.send(UserEvent::Close(self.sh.session));
    }

    /// Makes several changes appear atomically (no frame shows half of them); same as
    /// [`Figure::batch`].
    pub fn batch<R>(&self, f: impl FnOnce() -> R) -> R {
        self.fig.batch(f)
    }

    /// The figure being shown.
    pub fn figure(&self) -> &Figure {
        &self.fig
    }

    /// Current revision of the figure, and whether a batch is open.
    fn published(&self) -> (u64, bool) {
        let st = self.fig.sh.state.lock();
        (st.rev, st.batch_depth > 0)
    }

    /// Whether the window has presented every change made so far (backpressure: publish the
    /// next state only when this is `true` to run the simulation at display rate without
    /// queueing work). Also `true` while the window is closed, occluded or minimized.
    pub fn frame_due(&self) -> bool {
        if !self.is_open() || self.sh.occluded.load(Ordering::Acquire) {
            return true;
        }
        *self.sh.rendered.lock() >= self.published().0
    }

    /// Blocks until the changes made so far are on screen. Returns `true` when they are (or the
    /// frame was skipped because the window is occluded or minimized), `false` right away when
    /// the window is closed or a [`batch`](Live::batch) is open on this figure.
    pub fn wait_frame(&self) -> bool {
        self.wait(None)
    }

    /// Like [`wait_frame`](Live::wait_frame) with a timeout; `false` on timeout.
    pub fn wait_frame_timeout(&self, timeout: Duration) -> bool {
        self.wait(Some(Instant::now() + timeout))
    }

    fn wait(&self, deadline: Option<Instant>) -> bool {
        let (target, batch) = self.published();
        if batch {
            return false;
        }
        let mut r = self.sh.rendered.lock();
        loop {
            if *r >= target {
                return true;
            }
            if !self.is_open() {
                return false;
            }
            if self.sh.occluded.load(Ordering::Acquire) {
                *r = target;
                return true;
            }
            match deadline {
                None => self.sh.cv.wait(&mut r),
                Some(d) => {
                    if self.sh.cv.wait_until(&mut r, d).timed_out() {
                        return *r >= target;
                    }
                }
            }
        }
    }
}

/// Clears `open` (and wakes waiters) however the event loop ends, so the join cannot hang on a
/// simulation that polls `is_open()`.
struct OpenGuard<'a>(&'a LiveShared);

impl Drop for OpenGuard<'_> {
    fn drop(&mut self) {
        self.0.set_closed();
    }
}

fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(non-string panic payload)".into())
}

impl Figure {
    /// Shows the figure in a window while `sim` runs on a worker thread; returns `sim`'s result
    /// once the window is closed.
    ///
    /// The window (and its event loop) stay on the main thread, as macOS requires; `sim` runs on
    /// a scoped thread named `ezviz-sim`, so it may borrow locals. Update plots through their
    /// handles; each change wakes the window, and [`Live::batch`] groups changes into one frame.
    /// The window stays open after `sim` returns; [`Live::close`] closes it. Loop on
    /// [`Live::is_open`] so closing the window stops the simulation.
    ///
    /// If `sim` panics, the panic is logged at once and shown in the window title, and this
    /// returns [`Error::WorkerPanicked`] after the window is closed.
    ///
    /// ```no_run
    /// use ezviz::prelude::*;
    /// let fig = Figure::new();
    /// let ax = Axis::new(fig.at(1, 1));
    /// let sc = ax.scatter([0.0], [0.0]);
    /// let steps = fig.show_live(|live| {
    ///     let mut n = 0;
    ///     while live.is_open() {
    ///         n += 1;
    ///         let t = n as f64 * 0.01;
    ///         live.batch(|| {
    ///             sc.set_data([t.cos()], [t.sin()]);
    ///             ax.title(format!("step {n}"));
    ///         });
    ///         live.wait_frame();
    ///     }
    ///     n
    /// })?;
    /// # Ok::<(), ezviz::Error>(())
    /// ```
    pub fn show_live<R, F>(&self, sim: F) -> Result<R>
    where
        F: FnOnce(&Live) -> R + Send,
        R: Send,
    {
        let gpu = crate::render::gpu::gpu()?;
        with_event_loop(|el| {
            let sh = Arc::new(LiveShared {
                session: crate::figure::next_uid(),
                fig_uid: self.sh.uid,
                open: AtomicBool::new(true),
                occluded: AtomicBool::new(false),
                rendered: Mutex::new(0),
                cv: Condvar::new(),
                proxy: Mutex::new(el.create_proxy()),
                panic: Mutex::new(None),
            });
            let live = Live { sh: sh.clone(), fig: self.clone() };
            let mut app = App::new(gpu, el.create_proxy(), true, Some(sh.clone()));
            app.to_open.push((self.clone(), 0));
            std::thread::scope(|s| {
                let worker = std::thread::Builder::new()
                    .name("ezviz-sim".into())
                    .spawn_scoped(s, || {
                        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sim(&live)));
                        if let Err(p) = &r {
                            let msg = panic_text(p.as_ref());
                            log::error!("ezviz: the show_live simulation panicked: {msg}");
                            *live.sh.panic.lock() = Some(msg.clone());
                            live.sh.send(UserEvent::Panicked(live.sh.session, msg));
                        }
                        r.ok()
                    })
                    .map_err(Error::Io)?;
                let run = {
                    let _guard = OpenGuard(&sh);
                    let r = el.run_app_on_demand(&mut app).map_err(|e| Error::EventLoop(e.to_string()));
                    app.wins.clear();
                    r
                };
                let out = worker.join();
                if let Some(msg) = sh.panic_message() {
                    return Err(Error::WorkerPanicked(msg));
                }
                if let Some(e) = app.error.take() {
                    return Err(e);
                }
                run?;
                match out {
                    Ok(Some(r)) => Ok(r),
                    _ => Err(Error::WorkerPanicked("the simulation thread panicked".into())),
                }
            })
        })
    }
}
