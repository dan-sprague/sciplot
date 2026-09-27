//! Scripted-window hooks (feature `testing`): synthetic input and inspection of window state.

use super::interact::{AxisView, Button, Key, Modifiers};
use parking_lot::Mutex;
use std::collections::HashMap;

/// A synthetic input event. Positions are in figure units (logical px); the window converts them
/// to physical pixels and back through the same path as real events.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Synthetic {
    CursorMoved {
        x: f64,
        y: f64,
    },
    CursorLeft,
    Button {
        button: Button,
        pressed: bool,
    },
    /// Wheel steps (like a mouse wheel's `LineDelta`).
    ScrollLines {
        dx: f64,
        dy: f64,
    },
    /// Trackpad scroll in logical pixels (like `PixelDelta`).
    ScrollPixels {
        dx: f64,
        dy: f64,
    },
    Pinch(f64),
    Key {
        key: Key,
        pressed: bool,
    },
    Modifiers(Modifiers),
}

/// Window operations for scripted tests, delivered in order with synthetic input.
#[derive(Clone, Debug)]
pub(crate) enum Op {
    Input(Synthetic),
    /// Replies once every earlier event has been processed.
    Flush(std::sync::mpsc::Sender<()>),
    Minimize(bool),
    /// Writes the current frame, overlays included, to a PNG and replies with the outcome.
    Dump(std::path::PathBuf, std::sync::mpsc::Sender<Result<(), String>>),
}

/// A synthetic event as the interaction input the window would get from the real one (same
/// unit conversions).
pub(crate) fn translate_synthetic(ev: Synthetic, scale: f64, time: f64) -> super::interact::Input {
    use super::input::{cursor_units, scroll_steps};
    use super::interact::Input;
    use Synthetic as S;
    use winit::dpi::PhysicalPosition;
    use winit::event::MouseScrollDelta;
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

static HOVER: Mutex<Option<HashMap<u64, Option<String>>>> = Mutex::new(None);

pub(crate) fn record_hover(fig_uid: u64, text: Option<&str>) {
    HOVER.lock().get_or_insert_with(HashMap::new).insert(fig_uid, text.map(str::to_owned));
}

/// The tooltip text currently shown for `fig` (`None` if no tooltip).
pub fn hover_text(fig: &crate::Figure) -> Option<String> {
    HOVER.lock().as_ref().and_then(|m| m.get(&fig.sh.uid).cloned().flatten())
}

static VIEWS: Mutex<Option<HashMap<u64, Vec<AxisView>>>> = Mutex::new(None);

pub(crate) fn record_views(fig_uid: u64, views: &[AxisView]) {
    VIEWS.lock().get_or_insert_with(HashMap::new).insert(fig_uid, views.to_vec());
}

/// The axes (rectangles, limits, scales) of the last frame built for `fig`'s window.
pub fn axis_views(fig: &crate::Figure) -> Vec<AxisView> {
    VIEWS.lock().as_ref().and_then(|m| m.get(&fig.sh.uid).cloned()).unwrap_or_default()
}

/// Axis rectangles `[x, y, w, h]` (figure units) of the last frame built for `fig`'s window.
pub fn axis_rects(fig: &crate::Figure) -> Vec<[f64; 4]> {
    axis_views(fig).iter().map(|v| v.rect).collect()
}

/// Feeds a synthetic input event to the window as if the user made it.
pub fn inject(live: &crate::Live, ev: Synthetic) {
    live.sh.send(super::UserEvent::Test(live.sh.fig_uid, Op::Input(ev)));
}

/// Waits (up to 5 s) until the window has processed every event sent before; `false` on timeout.
pub fn flush(live: &crate::Live) -> bool {
    let (tx, rx) = std::sync::mpsc::channel();
    live.sh.send(super::UserEvent::Test(live.sh.fig_uid, Op::Flush(tx)));
    rx.recv_timeout(std::time::Duration::from_secs(5)).is_ok()
}

/// Minimizes or restores the window.
pub fn set_minimized(live: &crate::Live, on: bool) {
    live.sh.send(super::UserEvent::Test(live.sh.fig_uid, Op::Minimize(on)));
}

/// The interactive (pan/zoom) limits of `ax`, `[x0, x1, y0, y1]`, if any.
pub fn interactive_limits(ax: &crate::Axis) -> Option<[f64; 4]> {
    ax.sh.state.lock().block(ax.id).and_then(|b| b.as_axis()).and_then(|a| a.interactive)
}

/// Writes the window's current frame, including the hover tooltip and the rectangle-zoom shade,
/// to a PNG (at the window's scale factor). Waits up to 5 s for the window.
pub fn dump(live: &crate::Live, path: impl AsRef<std::path::Path>) -> Result<(), String> {
    let (tx, rx) = std::sync::mpsc::channel();
    live.sh.send(super::UserEvent::Test(live.sh.fig_uid, Op::Dump(path.as_ref().to_owned(), tx)));
    rx.recv_timeout(std::time::Duration::from_secs(5)).map_err(|e| e.to_string())?
}
