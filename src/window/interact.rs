//! Makie's axis interactions as a pure state machine (no window, no figure lock).
//!
//! [`handle`] turns one [`Input`] into [`Effect`]s given the axes of the last frame. Coordinates
//! are figure units (1 unit = 1 logical px), origin top-left, y down. Bindings follow Makie
//! (`Makie/src/makielayout/interactions.jl`, `mousestatemachine.jl`):
//!
//! | action | input |
//! |---|---|
//! | scroll zoom about the cursor | wheel: `0.9^Δ`; hold `x` / `y` to zoom one dimension |
//! | pan | right drag (or Option/Alt + left drag); hold `x` / `y` to pan one dimension |
//! | rectangle zoom | left drag (starts after 2 px); hold `x` / `y` to restrict it |
//! | reset limits | Ctrl + click, or double-click |
//! | full autolimits | Ctrl + Shift + click |
//! | pinch zoom (trackpads) | magnify gesture about the cursor |
//!
//! Zooming and panning happen in scaled space (log axes zoom by factors) and respect reversed
//! axes. Limits set on one axis propagate to its linked axes.

use crate::transform::Scale;

/// Movement (units) before a press becomes a drag (Makie's `drag_threshold`).
pub const DRAG_THRESHOLD: f64 = 2.0;
/// Maximum interval between the clicks of a double-click, in seconds.
pub const DOUBLE_CLICK: f64 = 0.2;
/// Makie's `ScrollZoom` speed: each wheel step scales the visible width by `1 - SCROLL_SPEED`.
pub const SCROLL_SPEED: f64 = 0.1;

/// One axis as the interaction sees it: where it is and what it shows.
#[derive(Clone, Debug, PartialEq)]
pub struct AxisView {
    /// Axis rectangle `[x, y, w, h]` in figure units (y down).
    pub rect: [f64; 4],
    /// Visible data limits `[x0, x1, y0, y1]` with `x0 < x1` and `y0 < y1`.
    pub limits: [f64; 4],
    pub xscale: Scale,
    pub yscale: Scale,
    pub xreversed: bool,
    pub yreversed: bool,
    /// Indices (into the view slice) of axes whose x limits are linked to this one.
    pub xlinks: Vec<usize>,
    /// Indices of axes whose y limits are linked to this one.
    pub ylinks: Vec<usize>,
}

impl AxisView {
    /// A linear, unlinked, unreversed axis.
    pub fn new(rect: [f64; 4], limits: [f64; 4]) -> AxisView {
        AxisView {
            rect,
            limits,
            xscale: Scale::Identity,
            yscale: Scale::Identity,
            xreversed: false,
            yreversed: false,
            xlinks: Vec::new(),
            ylinks: Vec::new(),
        }
    }

    /// Whether the point (figure units) is inside the axis rectangle.
    pub fn contains(&self, p: [f64; 2]) -> bool {
        let [x, y, w, h] = self.rect;
        p[0] >= x && p[0] <= x + w && p[1] >= y && p[1] <= y + h
    }

    /// The point as a fraction of the axis, x to the right and y up, flipped on reversed axes
    /// (Makie's `mp_axfraction`). `[0, 0]` is the lowest-limit corner.
    pub fn fraction(&self, p: [f64; 2]) -> [f64; 2] {
        let [x, y, w, h] = self.rect;
        let fx = (p[0] - x) / w;
        let fy = (y + h - p[1]) / h;
        [if self.xreversed { 1.0 - fx } else { fx }, if self.yreversed { 1.0 - fy } else { fy }]
    }

    /// The limits in scaled space (log etc. applied).
    pub fn scaled(&self) -> [f64; 4] {
        let [x0, x1, y0, y1] = self.limits;
        [self.xscale.forward(x0), self.xscale.forward(x1), self.yscale.forward(y0), self.yscale.forward(y1)]
    }

    /// Scaled-space limits back to data limits.
    fn unscale(&self, s: [f64; 4]) -> [f64; 4] {
        [self.xscale.inverse(s[0]), self.xscale.inverse(s[1]), self.yscale.inverse(s[2]), self.yscale.inverse(s[3])]
    }

    /// Figure units -> data coordinates, clamped to the visible limits.
    pub fn to_data_clamped(&self, p: [f64; 2]) -> [f64; 2] {
        let f = self.fraction(p);
        let s = self.scaled();
        let fx = f[0].clamp(0.0, 1.0);
        let fy = f[1].clamp(0.0, 1.0);
        [self.xscale.inverse(s[0] + fx * (s[1] - s[0])), self.yscale.inverse(s[2] + fy * (s[3] - s[2]))]
    }

    /// Data coordinates -> figure units (`None` outside the scale's domain).
    pub fn to_units(&self, x: f64, y: f64) -> Option<[f64; 2]> {
        let s = self.scaled();
        let (sx, sy) = (self.xscale.forward(x), self.yscale.forward(y));
        if !(sx.is_finite() && sy.is_finite()) {
            return None;
        }
        let mut fx = (sx - s[0]) / (s[1] - s[0]);
        let mut fy = (sy - s[2]) / (s[3] - s[2]);
        if self.xreversed {
            fx = 1.0 - fx;
        }
        if self.yreversed {
            fy = 1.0 - fy;
        }
        let [rx, ry, w, h] = self.rect;
        Some([rx + fx * w, ry + h - fy * h])
    }

    /// Makie's `_axis_limits_are_valid`: finite, positive widths, inside the scales' domains.
    pub fn valid(&self, l: [f64; 4]) -> bool {
        l.iter().all(|v| v.is_finite())
            && l[1] > l[0]
            && l[3] > l[2]
            && self.xscale.valid(l[0])
            && self.xscale.valid(l[1])
            && self.yscale.valid(l[2])
            && self.yscale.valid(l[3])
    }
}

/// Modifier keys.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    /// Control (also on macOS, like Makie; not Cmd).
    pub ctrl: bool,
    pub shift: bool,
    /// Alt / Option.
    pub alt: bool,
}

/// Mouse buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
}

/// Keys the interactions care about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// Restricts zoom and pan to x.
    X,
    /// Restricts zoom and pan to y.
    Y,
}

/// One input event, in figure units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Input {
    CursorMoved([f64; 2]),
    CursorLeft,
    /// A button press or release; `time` in seconds (any monotonic origin) for double-clicks.
    Button {
        button: Button,
        pressed: bool,
        time: f64,
    },
    /// Scroll in wheel steps (`[x, y]`); positive y zooms in. Trackpad pixel deltas are
    /// converted by the window as `px / scale_factor / 16`.
    Scroll([f64; 2]),
    /// Trackpad magnification delta (positive zooms in).
    Pinch(f64),
    Key {
        key: Key,
        pressed: bool,
    },
    Modifiers(Modifiers),
    /// The window lost focus: forget held keys, buttons and drags.
    FocusLost,
}

/// What the window must do after an input.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// Set interactive limits (data space, ordered) on axis `axis` (an index into the views).
    SetLimits { axis: usize, limits: [f64; 4] },
    /// Makie's `reset_limits!`: back to the user limits, automatic elsewhere.
    ResetLimits { axis: usize },
    /// Makie's `autolimits!`: forget user limits too.
    AutoLimits { axis: usize },
    /// Only the overlay (rectangle-zoom shade) changed; redraw without relayout.
    Overlay,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Action {
    RectZoom,
    Pan,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Press {
    button: Button,
    at: [f64; 2],
    axis: Option<usize>,
    action: Action,
    dragging: bool,
    /// Rectangle zoom: the clamped data point where the drag started.
    from: [f64; 2],
}

/// Interaction state of one window.
#[derive(Clone, Debug, Default)]
pub struct InteractState {
    /// Last cursor position in figure units (`None` outside the window).
    pub cursor: Option<[f64; 2]>,
    pub mods: Modifiers,
    /// `x` is held.
    pub x_held: bool,
    /// `y` is held.
    pub y_held: bool,
    press: Option<Press>,
    /// Rectangle zoom: the current clamped data point.
    to: [f64; 2],
    last_click: Option<f64>,
}

impl InteractState {
    /// Whether a drag (pan or rectangle zoom) is in progress.
    pub fn dragging(&self) -> bool {
        self.press.is_some_and(|p| p.dragging)
    }

    /// The active rectangle-zoom selection: axis index and chosen data limits.
    pub fn selection(&self, views: &[AxisView]) -> Option<(usize, [f64; 4])> {
        let p = self.press?;
        let a = p.axis?;
        let v = views.get(a)?;
        (p.dragging && p.action == Action::RectZoom).then(|| (a, self.chosen_limits(v, p.from)))
    }

    /// Makie's `_chosen_limits`: holding `x` keeps the full y range, holding `y` the full x range.
    fn chosen_limits(&self, v: &AxisView, from: [f64; 2]) -> [f64; 4] {
        let l = v.limits;
        let (mut x0, mut x1) = (from[0].min(self.to[0]), from[0].max(self.to[0]));
        let (mut y0, mut y1) = (from[1].min(self.to[1]), from[1].max(self.to[1]));
        if self.y_held {
            (x0, x1) = (l[0], l[1]);
        }
        if self.x_held {
            (y0, y1) = (l[2], l[3]);
        }
        [x0.clamp(l[0], l[1]), x1.clamp(l[0], l[1]), y0.clamp(l[2], l[3]), y1.clamp(l[2], l[3])]
    }
}

/// Index of the topmost axis containing `p`.
pub fn axis_at(views: &[AxisView], p: [f64; 2]) -> Option<usize> {
    views.iter().rposition(|v| v.contains(p))
}

/// Processes one input. Effects are in application order; `SetLimits` effects for linked axes
/// follow the one that caused them.
pub fn handle(st: &mut InteractState, input: Input, views: &[AxisView]) -> Vec<Effect> {
    let mut fx = Vec::new();
    match input {
        Input::CursorMoved(p) => {
            let prev = st.cursor.replace(p);
            let Some(mut press) = st.press else { return fx };
            if !press.dragging && dist(p, press.at) >= DRAG_THRESHOLD {
                press.dragging = true;
            }
            if press.dragging
                && let Some(v) = press.axis.and_then(|a| views.get(a).map(|v| (a, v)))
            {
                match press.action {
                    Action::Pan => {
                        if let Some(l) = prev.and_then(|q| pan(v.1, q, p, st.x_held, st.y_held)) {
                            set_limits(views, v.0, l, &mut fx);
                        }
                    }
                    Action::RectZoom => {
                        st.to = v.1.to_data_clamped(p);
                        fx.push(Effect::Overlay);
                    }
                    Action::None => {}
                }
            }
            st.press = Some(press);
        }
        Input::CursorLeft => st.cursor = None,
        Input::Button { button, pressed: true, .. } => {
            let Some(p) = st.cursor else { return fx };
            if st.press.is_some() {
                return fx; // one button at a time (Makie)
            }
            let axis = axis_at(views, p);
            let action = match button {
                Button::Right => Action::Pan,
                Button::Left if st.mods.alt => Action::Pan,
                Button::Left => Action::RectZoom,
                Button::Middle => Action::None,
            };
            let from = axis.map_or([0.0; 2], |a| views[a].to_data_clamped(p));
            st.to = from;
            st.press = Some(Press { button, at: p, axis, action, dragging: false, from });
        }
        Input::Button { button, pressed: false, time } => {
            let Some(press) = st.press.filter(|p| p.button == button) else { return fx };
            if press.dragging {
                if press.action == Action::RectZoom {
                    if let Some((a, l)) = st.selection(views)
                        && views[a].valid(l)
                    {
                        set_limits(views, a, l, &mut fx);
                    }
                    fx.push(Effect::Overlay);
                }
            } else if let (Button::Left, Some(axis)) = (button, press.axis) {
                // A click (Makie's LimitReset; double-click reset is a macOS-friendly extra).
                if st.mods.ctrl {
                    fx.push(if st.mods.shift { Effect::AutoLimits { axis } } else { Effect::ResetLimits { axis } });
                    st.last_click = None;
                } else if st.last_click.is_some_and(|t| time - t <= DOUBLE_CLICK) {
                    fx.push(Effect::ResetLimits { axis });
                    st.last_click = None;
                } else {
                    st.last_click = Some(time);
                }
            }
            st.press = None;
        }
        Input::Scroll([_, dy]) => {
            if dy != 0.0 {
                zoom(st, views, (1.0 - SCROLL_SPEED).powf(dy), &mut fx);
            }
        }
        Input::Pinch(d) => {
            if d.is_finite() && d != 0.0 {
                zoom(st, views, 1.0 / (1.0 + d.max(-0.9)), &mut fx);
            }
        }
        Input::Key { key, pressed } => {
            match key {
                Key::X => st.x_held = pressed,
                Key::Y => st.y_held = pressed,
            }
            if st.selection(views).is_some() {
                fx.push(Effect::Overlay);
            }
        }
        Input::Modifiers(m) => st.mods = m,
        Input::FocusLost => {
            if st.selection(views).is_some() {
                fx.push(Effect::Overlay);
            }
            st.press = None;
            st.x_held = false;
            st.y_held = false;
            st.mods = Modifiers::default();
        }
    }
    fx
}

fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Makie's `ScrollZoom`: scale the widths by `z` about the cursor, in scaled space.
fn zoom(st: &InteractState, views: &[AxisView], z: f64, fx: &mut Vec<Effect>) {
    let Some(p) = st.cursor else { return };
    let Some(a) = axis_at(views, p) else { return };
    let v = &views[a];
    let f = v.fraction(p);
    let [x0, x1, y0, y1] = v.scaled();
    let (w, h) = (x1 - x0, y1 - y0);
    let (nw, nh) = (w * z, h * z);
    let (nx0, ny0) = (x0 + f[0] * (w - nw), y0 + f[1] * (h - nh));
    let s = if st.x_held {
        [nx0, nx0 + nw, y0, y1]
    } else if st.y_held {
        [x0, x1, ny0, ny0 + nh]
    } else {
        [nx0, nx0 + nw, ny0, ny0 + nh]
    };
    let l = v.unscale(s);
    if v.valid(l) {
        set_limits(views, a, l, fx);
    }
}

/// Makie's `DragPan`: move the limits by the cursor movement, in scaled space.
fn pan(v: &AxisView, from: [f64; 2], to: [f64; 2], x_held: bool, y_held: bool) -> Option<[f64; 4]> {
    let (f0, f1) = (v.fraction(from), v.fraction(to));
    let [x0, x1, y0, y1] = v.scaled();
    let (w, h) = (x1 - x0, y1 - y0);
    let nx0 = if y_held { x0 } else { x0 - (f1[0] - f0[0]) * w };
    let ny0 = if x_held { y0 } else { y0 - (f1[1] - f0[1]) * h };
    let l = v.unscale([nx0, nx0 + w, ny0, ny0 + h]);
    (v.valid(l) && l != v.limits).then_some(l)
}

/// Emits `SetLimits` for `a` followed by its linked axes (see [`propagate`]).
fn set_limits(views: &[AxisView], a: usize, l: [f64; 4], fx: &mut Vec<Effect>) {
    fx.push(Effect::SetLimits { axis: a, limits: l });
    fx.extend(propagate(views, a, l));
}

/// `SetLimits` effects that carry new limits `l` of axis `a` over to every axis linked to it
/// (transitively), replacing only the linked dimension(s) of each.
pub fn propagate(views: &[AxisView], a: usize, l: [f64; 4]) -> Vec<Effect> {
    let xs = closure(views, a, |v| &v.xlinks);
    let ys = closure(views, a, |v| &v.ylinks);
    let mut fx = Vec::new();
    for (b, v) in views.iter().enumerate() {
        if b == a || !(xs.contains(&b) || ys.contains(&b)) {
            continue;
        }
        let mut m = v.limits;
        if xs.contains(&b) {
            m[0] = l[0];
            m[1] = l[1];
        }
        if ys.contains(&b) {
            m[2] = l[2];
            m[3] = l[3];
        }
        fx.push(Effect::SetLimits { axis: b, limits: m });
    }
    fx
}

/// All axes reachable from `a` through `links`.
fn closure(views: &[AxisView], a: usize, links: impl Fn(&AxisView) -> &Vec<usize>) -> Vec<usize> {
    let mut seen = vec![a];
    let mut i = 0;
    while i < seen.len() {
        if let Some(v) = views.get(seen[i]) {
            for &b in links(v) {
                if b < views.len() && !seen.contains(&b) {
                    seen.push(b);
                }
            }
        }
        i += 1;
    }
    seen
}

/// Applies `SetLimits` effects to the views (so the next input sees the new limits before the
/// window re-renders).
pub fn apply_to_views(views: &mut [AxisView], fx: &[Effect]) {
    for e in fx {
        if let Effect::SetLimits { axis, limits } = e
            && let Some(v) = views.get_mut(*axis)
        {
            v.limits = *limits;
        }
    }
}
