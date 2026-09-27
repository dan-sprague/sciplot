//! winit events -> interaction [`Input`]s: mouse, wheel, keys, trackpad pinch and touch
//! gestures. Positions become figure units (logical px).
//!
//! Browsers differ from native windows in two ways that matter here:
//! - winit reports DOM wheel events as `PixelDelta` scaled by the device pixel ratio (a mouse
//!   notch is 100–120 CSS px in Chrome, a native mouse notch is one `LineDelta` step), so web
//!   wheel steps are capped at one per event; trackpad deltas convert like native ones.
//! - a trackpad pinch arrives as a wheel event with Ctrl held (no gesture events), with
//!   `deltaY = -100 ln(scale)` in Chrome; it becomes the same `Pinch` a native magnify gesture
//!   gives.
//!
//! Touch (both platforms): one finger pans, two fingers pinch-zoom (and pan with their
//! midpoint), a tap clicks (hover readout; a double tap resets the limits).

use super::interact::{self, Button, Input, Modifiers};
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

/// Physical cursor position -> figure units.
pub(crate) fn cursor_units(p: PhysicalPosition<f64>, scale: f64) -> [f64; 2] {
    [p.x / scale, p.y / scale]
}

/// Native scroll delta -> wheel steps (trackpad pixels: `px / scale / 16`).
pub(crate) fn scroll_steps(d: MouseScrollDelta, scale: f64) -> [f64; 2] {
    match d {
        MouseScrollDelta::LineDelta(x, y) => [x as f64, y as f64],
        MouseScrollDelta::PixelDelta(p) => [p.x / scale / 16.0, p.y / scale / 16.0],
    }
}

/// Browser wheel event -> input. Pixel deltas convert like native trackpad pixels, but one event
/// never zooms more than one wheel step (a native mouse notch); with Ctrl held (a trackpad pinch)
/// the delta is a magnification.
pub(crate) fn web_wheel(d: MouseScrollDelta, scale: f64, ctrl: bool) -> Input {
    if ctrl && let MouseScrollDelta::PixelDelta(p) = d {
        // Chrome: deltaY = -100 ln(scale); winit flips the sign and multiplies by the DPR.
        // Clamped so a Ctrl + mouse-wheel notch zooms by 1.25x, not e^1.2.
        let magnify = (p.y / scale / 100.0).exp().clamp(0.8, 1.25);
        return Input::Pinch(magnify - 1.0);
    }
    let [x, y] = scroll_steps(d, scale);
    Input::Scroll([x.clamp(-1.0, 1.0), y.clamp(-1.0, 1.0)])
}

/// Translates a winit window event into an interaction input (`mods`: the modifiers held).
pub(crate) fn translate(ev: &WindowEvent, scale: f64, time: f64, mods: Modifiers) -> Option<Input> {
    Some(match ev {
        WindowEvent::CursorMoved { position, .. } => Input::CursorMoved(cursor_units(*position, scale)),
        WindowEvent::CursorLeft { .. } => Input::CursorLeft,
        WindowEvent::MouseInput { state, button, .. } => Input::Button {
            button: match button {
                MouseButton::Left => Button::Left,
                MouseButton::Right => Button::Right,
                MouseButton::Middle => Button::Middle,
                _ => return None,
            },
            pressed: *state == ElementState::Pressed,
            time,
        },
        WindowEvent::MouseWheel { delta, .. } => {
            if cfg!(target_arch = "wasm32") {
                web_wheel(*delta, scale, mods.ctrl)
            } else {
                Input::Scroll(scroll_steps(*delta, scale))
            }
        }
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
            Input::Modifiers(Modifiers { ctrl: s.control_key(), shift: s.shift_key(), alt: s.alt_key() })
        }
        WindowEvent::Focused(false) => Input::FocusLost,
        _ => return None,
    })
}

/// Touch gestures as mouse-like inputs: the fingers drive a right-button drag (the pan binding)
/// at their midpoint, two fingers also pinch, and a tap is a left click.
#[derive(Debug, Default)]
pub(crate) struct Touches {
    /// Active touches (winit touch id, position in units), in the order they went down.
    pts: Vec<(u64, [f64; 2])>,
    /// Where the current single-finger gesture started.
    start: [f64; 2],
    /// The gesture moved (or used two fingers): lifting the finger is no tap.
    moved: bool,
    /// Distance between the first two fingers at the last event.
    span: Option<f64>,
    /// The emulated pan button is down.
    dragging: bool,
}

impl Touches {
    /// Inputs for one touch event at `p` (figure units).
    pub fn handle(&mut self, id: u64, phase: TouchPhase, p: [f64; 2], time: f64) -> Vec<Input> {
        let mut out = Vec::new();
        let before = self.active();
        match phase {
            TouchPhase::Started => {
                if self.pts.iter().all(|(i, _)| *i != id) {
                    self.pts.push((id, p));
                }
                if self.pts.len() == 1 {
                    self.start = p;
                    self.moved = false;
                } else {
                    self.moved = true;
                }
            }
            TouchPhase::Moved => {
                let Some(k) = self.pts.iter().position(|(i, _)| *i == id) else { return out };
                self.pts[k].1 = p;
                if k >= 2 {
                    return out;
                }
                if dist(p, self.start) >= interact::DRAG_THRESHOLD {
                    self.moved = true;
                }
                if let Some(c) = self.center() {
                    out.push(Input::CursorMoved(c));
                }
                if let (Some(old), Some(new)) = (self.span, self.pair_span())
                    && old > 0.0
                {
                    out.push(Input::Pinch(new / old - 1.0));
                    self.span = Some(new);
                }
                return out;
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                self.pts.retain(|(i, _)| *i != id);
                if self.pts.is_empty() {
                    self.release(&mut out, time);
                    let tap = phase == TouchPhase::Ended && !self.moved;
                    if tap {
                        // A click where the finger was: shows the hover readout; two quick
                        // taps reset the limits (the double-click binding).
                        out.push(Input::Button { button: Button::Left, pressed: true, time });
                        out.push(Input::Button { button: Button::Left, pressed: false, time });
                    } else {
                        out.push(Input::CursorLeft);
                    }
                    self.span = None;
                    return out;
                }
            }
        }
        // The first two fingers changed: restart the drag on the new set.
        if self.active() != before {
            self.release(&mut out, time);
            if let Some(c) = self.center() {
                out.push(Input::CursorMoved(c));
                out.push(Input::Button { button: Button::Right, pressed: true, time });
                self.dragging = true;
            }
            self.span = self.pair_span();
        }
        out
    }

    /// Ids of the fingers that drive the gesture.
    fn active(&self) -> Vec<u64> {
        self.pts.iter().take(2).map(|(i, _)| *i).collect()
    }

    fn release(&mut self, out: &mut Vec<Input>, time: f64) {
        if std::mem::take(&mut self.dragging) {
            out.push(Input::Button { button: Button::Right, pressed: false, time });
        }
    }

    /// The gesture's point: the finger, or the midpoint of the first two.
    fn center(&self) -> Option<[f64; 2]> {
        match self.pts.as_slice() {
            [] => None,
            [(_, p)] => Some(*p),
            [(_, a), (_, b), ..] => Some([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]),
        }
    }

    fn pair_span(&self) -> Option<f64> {
        match self.pts.as_slice() {
            [(_, a), (_, b), ..] => Some(dist(*a, *b)),
            _ => None,
        }
    }
}

fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::interact::{AxisView, Effect, InteractState, handle};

    fn press(b: Button, pressed: bool) -> impl Fn(&Input) -> bool {
        move |i| matches!(i, Input::Button { button, pressed: p, .. } if *button == b && *p == pressed)
    }

    #[test]
    fn web_wheel_matches_native_steps() {
        // Chrome mouse notch at DPR 2: DOM deltaY = +120 -> winit PixelDelta y = -240.
        let notch = MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, -240.0));
        assert_eq!(web_wheel(notch, 2.0, false), Input::Scroll([0.0, -1.0]));
        // A small trackpad delta converts exactly like the native one.
        let pad = MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 8.0));
        assert_eq!(web_wheel(pad, 2.0, false), Input::Scroll(scroll_steps(pad, 2.0)));
        assert_eq!(web_wheel(pad, 2.0, false), Input::Scroll([0.0, 0.25]));
        // Firefox line deltas (3 lines per notch) also count as one step.
        let lines = MouseScrollDelta::LineDelta(0.0, 3.0);
        assert_eq!(web_wheel(lines, 1.0, false), Input::Scroll([0.0, 1.0]));
    }

    #[test]
    fn web_ctrl_wheel_is_a_pinch() {
        // Pinch out (zoom in) by 10 %: DOM deltaY = -100 ln 1.1 -> winit y = +100 ln 1.1 * dpr.
        let d = MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 100.0 * 1.1f64.ln() * 2.0));
        let Input::Pinch(m) = web_wheel(d, 2.0, true) else { panic!() };
        assert!((m - 0.1).abs() < 1e-9, "{m}");
        // A Ctrl + mouse notch is clamped.
        let notch = MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, -240.0));
        assert_eq!(web_wheel(notch, 2.0, true), Input::Pinch(0.8 - 1.0));
    }

    #[test]
    fn one_finger_pans_and_tap_clicks() {
        let mut t = Touches::default();
        let down = t.handle(1, TouchPhase::Started, [100.0, 100.0], 0.0);
        assert_eq!(down[0], Input::CursorMoved([100.0, 100.0]));
        assert!(press(Button::Right, true)(&down[1]));
        let mv = t.handle(1, TouchPhase::Moved, [120.0, 90.0], 0.05);
        assert_eq!(mv, vec![Input::CursorMoved([120.0, 90.0])]);
        let up = t.handle(1, TouchPhase::Ended, [120.0, 90.0], 0.1);
        assert!(press(Button::Right, false)(&up[0]));
        assert_eq!(up[1], Input::CursorLeft);

        // A tap: press/release right (no drag), then a left click at the finger.
        let _ = t.handle(2, TouchPhase::Started, [50.0, 50.0], 1.0);
        let up = t.handle(2, TouchPhase::Ended, [50.0, 50.0], 1.05);
        assert!(press(Button::Right, false)(&up[0]));
        assert!(press(Button::Left, true)(&up[1]));
        assert!(press(Button::Left, false)(&up[2]));
    }

    #[test]
    fn two_fingers_pinch_about_their_midpoint() {
        let mut t = Touches::default();
        let _ = t.handle(1, TouchPhase::Started, [100.0, 100.0], 0.0);
        let second = t.handle(2, TouchPhase::Started, [200.0, 100.0], 0.0);
        assert!(press(Button::Right, false)(&second[0]));
        assert_eq!(second[1], Input::CursorMoved([150.0, 100.0]));
        assert!(press(Button::Right, true)(&second[2]));
        // Spread to twice the distance, symmetric: midpoint unchanged, pinch +100 %.
        let a = t.handle(1, TouchPhase::Moved, [50.0, 100.0], 0.1);
        let b = t.handle(2, TouchPhase::Moved, [250.0, 100.0], 0.1);
        let pinch: f64 =
            a.iter().chain(&b).filter_map(|i| if let Input::Pinch(d) = i { Some(1.0 + d) } else { None }).product();
        assert!((pinch - 2.0).abs() < 1e-9, "{a:?} {b:?}");
        // Lifting one finger continues as a one-finger pan; no tap at the end.
        let lift = t.handle(1, TouchPhase::Ended, [50.0, 100.0], 0.2);
        assert!(press(Button::Right, false)(&lift[0]));
        assert_eq!(lift[1], Input::CursorMoved([250.0, 100.0]));
        let end = t.handle(2, TouchPhase::Ended, [250.0, 100.0], 0.3);
        assert!(!end.iter().any(press(Button::Left, true)));
    }

    #[test]
    fn touch_pinch_zooms_the_axis() {
        let views = vec![AxisView::new([0.0, 0.0, 400.0, 400.0], [0.0, 10.0, 0.0, 10.0])];
        let mut st = InteractState::default();
        let mut t = Touches::default();
        let mut last = None;
        let mut feed = |inputs: Vec<Input>, st: &mut InteractState| {
            for i in inputs {
                for e in handle(st, i, &views) {
                    if let Effect::SetLimits { limits, .. } = e {
                        last = Some(limits);
                    }
                }
            }
        };
        feed(t.handle(1, TouchPhase::Started, [150.0, 200.0], 0.0), &mut st);
        feed(t.handle(2, TouchPhase::Started, [250.0, 200.0], 0.0), &mut st);
        feed(t.handle(1, TouchPhase::Moved, [100.0, 200.0], 0.1), &mut st);
        feed(t.handle(2, TouchPhase::Moved, [300.0, 200.0], 0.1), &mut st);
        let l = last.expect("pinch zoomed");
        // Spreading the fingers zooms in about the center: the visible width shrinks.
        assert!(l[1] - l[0] < 10.0 && l[0] > 0.0 && l[1] < 10.0, "{l:?}");
    }
}
