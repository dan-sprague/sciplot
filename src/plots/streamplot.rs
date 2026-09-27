//! `streamplot`: streamlines of a 2D vector field `f(x, y) -> (u, v)` (Makie's `StreamPlot`).
//!
//! The streamlines come from Makie's `streamplot_impl`: seed cells are drawn from the same
//! quasi-random (R2) sequence, each streamline is integrated with normalized Euler steps in both
//! directions from its seed until it leaves the box, enters a visited cell or reaches `maxsteps`,
//! and an arrowhead marks every seed. The result is deterministic and identical to Makie's for the
//! same field. It is computed lazily once per function/box/parameter change and shared by every
//! snapshot of the figure; the arrowheads are rebuilt in figure units only when the axis moves.

use super::arrows::{CpuColormap, ScalarFn, VectorColor, arrow_legend, axis_geometry_key};
use super::lines::prim_color_mapped;
use super::{ColorSpec, PlotImpl, PlotKind, add_to_axis, plot_common, point_bounds};
use crate::attrs::attributes;
use crate::color::{Color, Colormap, MappingAttrs};
use crate::data::Num;
use crate::figure::{Dirty, FigShared, PlotId};
use crate::scene::PlotCtx;
use crate::scene::drawlist::{LinesPrim, MeshPrim, MeshVertex, Prim};
use crate::style::{JoinStyle, LineCap, Linestyle};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock};

/// A streamplot handle (Makie's `StreamPlot`).
///
/// ```no_run
/// use ezviz::prelude::*;
/// // Van der Pol oscillator.
/// let sp = streamplot(|x, y| (y, (1.0 - x * x) * y - x), -3.0..=3.0, -4.0..=4.0);
/// sp.colormap(Colormap::MAGMA).density(0.8);
/// sp.save("vdp.png").unwrap();
/// ```
#[derive(Clone)]
pub struct StreamPlot {
    pub(crate) sh: Arc<FigShared>,
    pub(crate) id: PlotId,
}

/// A 2D vector field `f(x, y) -> (u, v)`.
pub type VectorFieldFn = Arc<dyn Fn(f64, f64) -> (f64, f64) + Send + Sync>;

/// Anything usable as a coordinate interval: `-2.0..=2.0`, `0..10`, `(lo, hi)` or `[lo, hi]`
/// (Makie takes the `extrema`, so reversed bounds are fine).
pub trait Interval {
    /// `[min, max]` of the interval.
    fn bounds(self) -> [f64; 2];
}

fn sorted(a: f64, b: f64) -> [f64; 2] {
    if b < a { [b, a] } else { [a, b] }
}

impl<T: Num> Interval for std::ops::RangeInclusive<T> {
    fn bounds(self) -> [f64; 2] {
        sorted(self.start().to_f64(), self.end().to_f64())
    }
}
impl<T: Num> Interval for std::ops::Range<T> {
    fn bounds(self) -> [f64; 2] {
        sorted(self.start.to_f64(), self.end.to_f64())
    }
}
impl<A: Num, B: Num> Interval for (A, B) {
    fn bounds(self) -> [f64; 2] {
        sorted(self.0.to_f64(), self.1.to_f64())
    }
}
impl<T: Num> Interval for [T; 2] {
    fn bounds(self) -> [f64; 2] {
        sorted(self[0].to_f64(), self[1].to_f64())
    }
}

/// The computed streamlines (Makie's `streamplot_impl` output).
#[derive(Debug, Default)]
pub(crate) struct Streamlines {
    /// Seed points, where the arrowheads go.
    pub arrow_pos: Vec<[f64; 2]>,
    /// Unit field direction at each seed (NaN at fixed points).
    pub arrow_dir: Vec<[f64; 2]>,
    /// Color value of each arrowhead.
    pub arrow_val: Vec<f64>,
    /// Polylines, each half-line starting with a NaN break.
    pub line_pts: Arc<Vec<[f64; 2]>>,
    /// Color value of each line point.
    pub line_val: Arc<Vec<f64>>,
}

/// Parameters of the streamline computation.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StreamParams {
    pub origin: [f64; 2],
    pub widths: [f64; 2],
    pub gridsize: [usize; 2],
    pub stepsize: f64,
    pub maxsteps: f64,
    pub density: f64,
}

/// Makie's R2 sequence coefficients for 2D: `ϕ .^ (-(1:2))` with ϕ = 1.324717957244746
/// (the plastic number), exactly as Julia computes them.
const R2: [f64; 2] = [0.7548776662466927, 0.5698402909980532];

/// Julia's `LinRange` element `j` (0-based) of `d + 1` points from `a` to `b` (`Base.lerpi`).
fn lerpi(j: usize, d: usize, a: f64, b: f64) -> f64 {
    let t = j as f64 / d as f64;
    (1.0 - t) * a + t * b
}

/// Julia's `searchsortedlast(LinRange(a, b, d + 1), x)` (1-based; 0 below the range).
fn searchsortedlast(a: f64, b: f64, d: usize, x: f64) -> usize {
    let h = (b - a) / d as f64;
    if x < a {
        0
    } else if h == 0.0 || x >= b {
        d + 1
    } else {
        let n = ((x - a) / h + 1.0).round_ties_even().clamp(1.0, (d + 1) as f64) as usize;
        if x < lerpi(n - 1, d, a, b) { n - 1 } else { n }
    }
}

/// Makie's `streamplot_impl` for 2D fields. `value` gives the color value of a field vector.
pub(crate) fn streamlines(
    f: &dyn Fn(f64, f64) -> (f64, f64),
    value: &dyn Fn(f64, f64) -> f64,
    p: &StreamParams,
) -> Streamlines {
    let res = [p.gridsize[0].max(1), p.gridsize[1].max(1)];
    let mini = p.origin;
    let maxi = [p.origin[0] + p.widths[0], p.origin[1] + p.widths[1]];
    let step = [(maxi[0] - mini[0]) / res[0] as f64, (maxi[1] - mini[1]) / res[1] as f64];
    let mut mask = vec![true; res[0] * res[1]];
    let idx = |c: [usize; 2]| (c[0] - 1) + (c[1] - 1) * res[0];
    let inside = |x: [f64; 2]| (0..2).all(|i| x[i] <= maxi[i] && x[i] >= mini[i]);
    // Makie converts the step to Float32 (`Point{N, Float32}(stepsize)`); line colors are Float32.
    let dt = p.stepsize as f32 as f64;
    let val = |u: f64, v: f64| value(u, v) as f32 as f64;
    let mut out = Streamlines::default();
    let (mut pts, mut vals) = (Vec::new(), Vec::new());
    let target = (res[0] * res[1]) as f64 * p.density.min(1.0);
    // Every cell is eventually drawn by the sequence; the cap only guards pathological inputs.
    let cap = 1000 * res[0] * res[1] + 100_000;
    let (mut n_points, mut ind) = (0usize, 0usize);
    while (n_points as f64) < target && ind < cap {
        let c: [usize; 2] = std::array::from_fn(|i| {
            let j = (((0.5 + R2[i] * ind as f64) % 1.0) * res[i] as f64).ceil();
            j.clamp(1.0, res[i] as f64) as usize
        });
        ind += 1;
        if !mask[idx(c)] {
            continue;
        }
        let x0: [f64; 2] = std::array::from_fn(|i| mini[i] + (c[i] as f64 - 0.5) * step[i]);
        let (u, v) = f(x0[0], x0[1]);
        let pnorm = (u * u + v * v).sqrt();
        let color = val(u, v);
        out.arrow_pos.push(x0);
        out.arrow_dir.push([u / pnorm, v / pnorm]);
        out.arrow_val.push(color);
        mask[idx(c)] = false;
        n_points += 1;
        for d in [-1.0, 1.0] {
            let mut n_linepoints = 1.0;
            let mut x = x0;
            let mut ccur = c;
            pts.extend([[f64::NAN; 2], x]);
            vals.extend([color, color]);
            while inside(x) && n_linepoints < p.maxsteps {
                let (u, v) = f(x[0], x[1]);
                let pnorm = (u * u + v * v).sqrt();
                x = [x[0] + d * dt * u / pnorm, x[1] + d * dt * v / pnorm];
                if !inside(x) {
                    break;
                }
                let cell: [usize; 2] = std::array::from_fn(|i| {
                    // Makie would index out of bounds exactly on the upper edge; clamp instead.
                    searchsortedlast(mini[i], maxi[i], res[i], x[i]).clamp(1, res[i])
                });
                if cell != ccur {
                    if !mask[idx(cell)] {
                        break;
                    }
                    mask[idx(cell)] = false;
                    n_points += 1;
                    ccur = cell;
                }
                pts.push(x);
                vals.push(val(u, v));
                n_linepoints += 1.0;
            }
        }
    }
    out.line_pts = Arc::new(pts);
    out.line_val = Arc::new(vals);
    out
}

/// The lazily computed streamlines, shared by every snapshot until an input changes.
type StreamCache = Arc<OnceLock<Arc<Streamlines>>>;

#[derive(Clone)]
pub(crate) struct StreamPlotState {
    pub f: VectorFieldFn,
    /// Makie's `Rect(xmin, ymin, xmax - xmin, ymax - ymin)`: origin and widths.
    pub origin: [f64; 2],
    pub widths: [f64; 2],
    pub attrs: StreamPlotAttrs,
    /// The figure theme's streamplot defaults (fixed per figure), so the computation can resolve
    /// its parameters where no theme is at hand (autolimits).
    pub theme: StreamPlotAttrs,
    pub style_rev: u64,
    pub cache: StreamCache,
}

impl std::fmt::Debug for StreamPlotState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamPlotState")
            .field("origin", &self.origin)
            .field("widths", &self.widths)
            .field("attrs", &self.attrs)
            .finish_non_exhaustive()
    }
}

attributes! {
    StreamPlot(StreamPlotAttrs, StreamPlotResolved, StreamPlotTheme) via with_attrs {
        /// Line and arrowhead color: [`Magnitude`](crate::Magnitude) (default, Makie's
        /// `color = norm`: the field's speed through the colormap), a color, or `Cycled(i)`.
        /// See also [`StreamPlot::color_fn`].
        color: VectorColor = |_| VectorColor::Magnitude, STYLE;
        /// Colormap for mapped colors (default viridis).
        colormap: Colormap = |_| Colormap::VIRIDIS, STYLE;
        /// `(lo, hi)` mapped to the colormap ends; default: the finite extrema of the values (the
        /// lines and the arrowheads each use their own, like Makie).
        colorrange: Option<[f64; 2]> = |_| None, STYLE;
        /// Color for values below the colorrange (default: the first colormap color).
        lowclip: Option<Color> = |_| None, STYLE;
        /// Color for values above the colorrange (default: the last colormap color).
        highclip: Option<Color> = |_| None, STYLE;
        /// Color for NaN values (default transparent).
        nan_color: Color = |_| Color::TRANSPARENT, STYLE;
        /// Opacity multiplier.
        alpha: f64 = |_| 1.0, STYLE;
        /// Step length of the normalized Euler integration, in data units (default 0.01).
        stepsize: f64 = |_| 0.01, DATA;
        /// Number of cells `(nx, ny)` of the seeding grid (default (32, 32)).
        gridsize: [f64; 2] = |_| [32.0, 32.0], DATA;
        /// Maximum number of points per half streamline (default 500).
        maxsteps: f64 = |_| 500.0, DATA;
        /// Fraction of cells that must be visited by streamlines, 0..=1 (default 1).
        density: f64 = |_| 1.0, DATA;
        /// Arrowhead size in units (Makie's triangle marker size; default 15).
        arrow_size: f64 = |_| 15.0, STYLE;
        /// Streamline width in units (default: the theme's linewidth, 1.5).
        linewidth: f64 = |g| g.linewidth, STYLE;
        /// Dash pattern.
        linestyle: Linestyle = |_| Linestyle::Solid, STYLE;
        /// Shape of the line ends.
        linecap: LineCap = |_| LineCap::Butt, STYLE;
        /// Shape of the corners.
        joinstyle: JoinStyle = |_| JoinStyle::Miter, STYLE;
        /// Miter joints sharper than this angle (radians) are beveled (Makie default π/3).
        miter_limit: f64 = |_| std::f64::consts::FRAC_PI_3, STYLE;
    }
}

plot_common!(StreamPlot);
super::color_mapped!(StreamPlot);

impl StreamPlotResolved {
    fn mapping(&self) -> MappingAttrs<'_> {
        MappingAttrs {
            colormap: &self.colormap,
            colorrange: self.colorrange,
            lowclip: self.lowclip,
            highclip: self.highclip,
            nan_color: self.nan_color,
            alpha: self.alpha,
        }
    }

    /// Whether colors are values through the colormap (vs. one plain color).
    fn mapped(&self) -> bool {
        match &self.color {
            VectorColor::Spec(ColorSpec::Values(_) | ColorSpec::PerPoint(_)) => {
                crate::warn_once("streamplot: per-point colors are not supported; coloring by magnitude");
                true
            }
            VectorColor::Spec(_) => false,
            _ => true,
        }
    }
}

impl StreamPlotState {
    fn params(&self) -> (StreamParams, Option<ScalarFn>) {
        let r = self.attrs.resolve(&self.theme, &crate::theme::Globals::default());
        let count = |v: f64| if v.is_finite() && v >= 1.0 { v as usize } else { 1 };
        let p = StreamParams {
            origin: self.origin,
            widths: self.widths,
            gridsize: [count(r.gridsize[0]), count(r.gridsize[1])],
            stepsize: r.stepsize,
            maxsteps: r.maxsteps,
            density: r.density,
        };
        let func = match r.color {
            VectorColor::Func(g) => Some(g),
            _ => None,
        };
        (p, func)
    }

    /// Hash of everything the streamlines depend on (except the function, replaced explicitly).
    fn compute_key(&self) -> u64 {
        let (p, func) = self.params();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for v in [p.origin[0], p.origin[1], p.widths[0], p.widths[1], p.stepsize, p.maxsteps, p.density] {
            v.to_bits().hash(&mut h);
        }
        (p.gridsize, func.map(|g| Arc::as_ptr(&g) as *const () as usize)).hash(&mut h);
        h.finish()
    }

    /// The streamlines, computed on first use.
    pub(crate) fn lines(&self) -> Arc<Streamlines> {
        self.cache
            .get_or_init(|| {
                let (p, func) = self.params();
                let value = |u: f64, v: f64| match &func {
                    Some(g) => g(u, v),
                    None => (u * u + v * v).sqrt(),
                };
                Arc::new(streamlines(&*self.f, &value, &p))
            })
            .clone()
    }
}

/// Makie's `:utriangle` marker (units of markersize, y up): the apex and the two base corners.
const UTRIANGLE: [[f64; 2]; 3] = [[0.0, 0.485], [-0.36375, -0.2425], [0.36375, -0.2425]];

impl PlotImpl for StreamPlotState {
    fn cycle_group(&self) -> &'static str {
        "streamplot"
    }

    fn color_is_auto(&self, theme: &crate::theme::Theme) -> bool {
        matches!(
            self.attrs.color.as_ref().or(theme.streamplot.color.as_ref()),
            Some(VectorColor::Spec(ColorSpec::Auto))
        )
    }

    fn data_bounds(&self, xs: crate::transform::Scale, ys: crate::transform::Scale) -> Option<[f64; 4]> {
        // Makie: the union of the children's limits (the streamlines and the arrowheads' seeds).
        let s = self.lines();
        let mut b = point_bounds(&s.line_pts, xs, ys);
        if let Some(a) = point_bounds(&s.arrow_pos, xs, ys) {
            b = Some(match b {
                Some(b) => [b[0].min(a[0]), b[1].max(a[1]), b[2].min(a[2]), b[3].max(a[3])],
                None => a,
            });
        }
        b
    }

    fn legend_elements(&self, ctx: &super::legend_elements::LegendCtx<'_>) -> Vec<super::LegendElement> {
        let r = self.attrs.resolve(&ctx.theme.streamplot, ctx.g);
        let spec = match &r.color {
            VectorColor::Spec(s) => s.clone(),
            _ => ColorSpec::Values(Arc::new(Vec::new())),
        };
        let color = ctx.color(&spec, false, r.alpha, super::legend_elements::DEFAULT_LINECOLOR);
        arrow_legend(color, r.linewidth, r.arrow_size)
    }

    fn emit(&self, ctx: &mut PlotCtx<'_>) {
        let r = self.attrs.resolve(&ctx.theme.streamplot, ctx.g);
        let s = self.lines();
        let mapped = r.mapped();
        let map = r.mapping();

        // Streamlines: one NaN-separated polyline with per-point values.
        if s.line_pts.len() >= 2 {
            let pts = ctx.local_points(0, &s.line_pts);
            let spec = if mapped { ColorSpec::Values(s.line_val.clone()) } else { self::plain(&r.color) };
            let color = prim_color_mapped(ctx, &spec, r.alpha as f32, s.line_pts.len(), 1, self.style_rev, Some(&map));
            ctx.push_data(Prim::Lines(LinesPrim {
                pts,
                color,
                width: r.linewidth as f32,
                pattern: r.linestyle.pattern(),
                cap: r.linecap,
                join: r.joinstyle,
                miter_limit: r.miter_limit as f32,
                segments: false,
                closed: false,
                append: false,
            }));
        }

        // Arrowheads: rotated triangles in figure units, rebuilt when the axis moves.
        if s.arrow_pos.is_empty() || r.arrow_size.is_nan() || r.arrow_size <= 0.0 {
            return;
        }
        let a = ctx.axis;
        let key = {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (ctx.data_rev, self.style_rev, ctx.cycle, axis_geometry_key(a)).hash(&mut h);
            h.finish()
        };
        let plain = (!mapped).then(|| {
            let c = crate::scene::resolve_color(&self::plain(&r.color), ctx.cycle, &ctx.g.palette)
                .unwrap_or(Color::rgb(0.0, 0.0, 0.0));
            c.with_alpha(c.a * r.alpha as f32)
        });
        let verts = ctx.cache.memo(ctx.uid, 2, key, || {
            let cmap = CpuColormap::new(&map, s.arrow_val.iter().copied());
            // Makie's `register_projected_rotations_2d!`: the screen direction of a small step.
            let bb = point_bounds(&s.arrow_pos, crate::transform::Scale::Identity, crate::transform::Scale::Identity);
            let delta = bb.map_or(1.0, |b| 1e-3 * (b[1] - b[0]).hypot(b[3] - b[2]));
            let delta = if delta > 0.0 { delta } else { 1e-3 };
            let size = r.arrow_size;
            let mut out = Vec::with_capacity(3 * s.arrow_pos.len());
            for i in 0..s.arrow_pos.len() {
                let ([x, y], [u, v]) = (s.arrow_pos[i], s.arrow_dir[i]);
                let (Some(p0), Some(p1)) = (a.to_units(x, y), a.to_units(x + delta * u, y + delta * v)) else {
                    continue;
                };
                let (dx, dy) = (p1[0] - p0[0], p1[1] - p0[1]);
                let n = dx.hypot(dy);
                if !(n > 0.0 && n.is_finite()) {
                    continue;
                }
                // Marker "up" along the direction; figure units are y down.
                let (ux, uy) = (dx / n, dy / n);
                let c = plain.unwrap_or_else(|| cmap.color(s.arrow_val[i])).to_premul_u32();
                if c >> 24 == 0 {
                    continue;
                }
                out.extend(UTRIANGLE.map(|[mx, my]| MeshVertex {
                    pos: [(p0[0] + size * (my * ux - mx * uy)) as f32, (p0[1] + size * (my * uy + mx * ux)) as f32],
                    color: c,
                }));
            }
            out
        });
        if !verts.is_empty() {
            let buf = ctx.keyed_buf(2, key, verts);
            ctx.push_figure(Prim::Mesh(MeshPrim { verts: buf }));
        }
    }

    fn pick(&self, ctx: &mut super::pick::PickCtx<'_>) -> Option<super::pick::Hover> {
        let s = self.lines();
        let (dist, anchor, [x, y]) = ctx.nearest_on_polyline(s.line_pts.iter())?;
        let (u, v) = (self.f)(x, y);
        let text =
            format!("{}\nu: {}\nv: {}", super::pick::point_text(x, y), super::pick::sig6(u), super::pick::sig6(v));
        Some(super::pick::Hover { dist, anchor, text, ring: None, outline: None })
    }

    fn colormapping(&self, theme: &crate::theme::Theme, g: &crate::theme::Globals) -> Option<super::ResolvedColormap> {
        let r = self.attrs.resolve(&theme.streamplot, g);
        if !r.mapped() {
            return Some(super::ResolvedColormap::unmapped(r.colormap, r.alpha));
        }
        // The lines' mapping (the arrowheads may span a narrower automatic range).
        let s = self.lines();
        let [lo, hi] = r.colorrange.unwrap_or_else(|| {
            crate::color::ValueEncoding::new(crate::data::finite_extrema(s.line_val.iter().copied())).auto_range()
        });
        Some(super::ResolvedColormap {
            colormap: r.colormap,
            colorrange: (lo, hi),
            lowclip: r.lowclip,
            highclip: r.highclip,
            alpha: r.alpha,
            mapped: true,
        })
    }
}

/// The plain color spec of an unmapped color.
fn plain(c: &VectorColor) -> ColorSpec {
    match c {
        VectorColor::Spec(s) => s.clone(),
        _ => ColorSpec::Solid(Color::rgb(0.0, 0.0, 0.0)),
    }
}

impl StreamPlot {
    fn with_attrs(&self, f: impl FnOnce(&mut StreamPlotAttrs), dirty: u8) {
        self.with_slot(dirty | Dirty::LIMITS, |p| {
            if let PlotKind::StreamPlot(s) = &mut p.kind {
                let before = s.compute_key();
                f(&mut s.attrs);
                s.style_rev += 1;
                if s.compute_key() != before {
                    s.cache = StreamCache::default();
                    p.data_rev += 1;
                }
            }
        });
    }

    fn update(&self, f: impl FnOnce(&mut StreamPlotState)) {
        self.with_slot(Dirty::DATA | Dirty::LIMITS, |p| {
            if let PlotKind::StreamPlot(s) = &mut p.kind {
                f(s);
                s.cache = StreamCache::default();
                p.data_rev += 1;
            }
        });
    }

    pub(crate) fn create(ax: &crate::Axis, f: VectorFieldFn, x: [f64; 2], y: [f64; 2]) -> StreamPlot {
        let theme = ax.sh.state.lock().theme.streamplot.clone();
        let st = StreamPlotState {
            f,
            origin: [x[0], y[0]],
            widths: [x[1] - x[0], y[1] - y[0]],
            attrs: StreamPlotAttrs::default(),
            theme,
            style_rev: 0,
            cache: StreamCache::default(),
        };
        let id = add_to_axis(ax, PlotKind::StreamPlot(st));
        StreamPlot { sh: ax.sh.clone(), id }
    }

    /// Colors lines and arrowheads by `f(u, v)` of the field vector, mapped through the colormap
    /// (Makie's `color = f`; the default is the speed, `hypot(u, v)`).
    pub fn color_fn(&self, f: impl Fn(f64, f64) -> f64 + Send + Sync + 'static) -> StreamPlot {
        self.color(VectorColor::Func(Arc::new(f)))
    }

    /// Replaces the vector field (e.g. after a parameter change); the streamlines are recomputed.
    pub fn set_function(&self, f: impl Fn(f64, f64) -> (f64, f64) + Send + Sync + 'static) -> StreamPlot {
        let f: VectorFieldFn = Arc::new(f);
        self.update(move |s| s.f = f);
        self.clone()
    }

    /// Replaces the box the streamlines fill.
    pub fn set_limits(&self, x: impl Interval, y: impl Interval) -> StreamPlot {
        let (x, y) = (x.bounds(), y.bounds());
        self.update(move |s| {
            s.origin = [x[0], y[0]];
            s.widths = [x[1] - x[0], y[1] - y[0]];
        });
        self.clone()
    }

    /// Number of streamline seeds (= arrowheads) for the current inputs.
    pub fn seeds(&self) -> usize {
        let st = match self.sh.state.lock().plot(self.id).map(|p| &p.kind) {
            Some(PlotKind::StreamPlot(s)) => s.clone(),
            _ => return 0,
        };
        st.lines().arrow_pos.len()
    }
}

impl crate::Axis {
    /// Makie's `streamplot!(ax, f, xrange, yrange)`: streamlines of `f(x, y) -> (u, v)` filling
    /// the box `xrange × yrange` (e.g. `-2.0..=2.0` or `(lo, hi)`).
    pub fn streamplot(
        &self,
        f: impl Fn(f64, f64) -> (f64, f64) + Send + Sync + 'static,
        x: impl Interval,
        y: impl Interval,
    ) -> StreamPlot {
        StreamPlot::create(self, Arc::new(f), x.bounds(), y.bounds())
    }
}

impl crate::GridPosition {
    /// Makie's `streamplot(fig[r, c], f, xrange, yrange)`: a new Axis at this position.
    pub fn streamplot(
        &self,
        f: impl Fn(f64, f64) -> (f64, f64) + Send + Sync + 'static,
        x: impl Interval,
        y: impl Interval,
    ) -> StreamPlot {
        crate::Axis::new(self.clone()).streamplot(f, x, y)
    }
}

/// Makie's `streamplot(f, xrange, yrange)`: a new Figure and Axis with a streamplot.
#[must_use = "this creates a new Figure; call .save(..) or .show() on it"]
pub fn streamplot(
    f: impl Fn(f64, f64) -> (f64, f64) + Send + Sync + 'static,
    x: impl Interval,
    y: impl Interval,
) -> StreamPlot {
    crate::Figure::new().at(1, 1).streamplot(f, x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linrange_search_matches_julia() {
        // LinRange(-π, π, 33): cell k (1-based) is [r[k], r[k+1]).
        let (a, b) = (-std::f64::consts::PI, std::f64::consts::PI);
        assert_eq!(searchsortedlast(a, b, 32, a), 1);
        assert_eq!(searchsortedlast(a, b, 32, b), 33);
        assert_eq!(searchsortedlast(a, b, 32, a - 1e-9), 0);
        let r5 = lerpi(4, 32, a, b);
        assert_eq!(searchsortedlast(a, b, 32, r5), 5);
        assert_eq!(searchsortedlast(a, b, 32, r5 - 1e-12), 4);
    }

    fn run(f: fn(f64, f64) -> (f64, f64), x: [f64; 2], y: [f64; 2], grid: usize, density: f64) -> Streamlines {
        let p = StreamParams {
            origin: [x[0], y[0]],
            widths: [x[1] - x[0], y[1] - y[0]],
            gridsize: [grid, grid],
            stepsize: 0.01,
            maxsteps: 500.0,
            density,
        };
        streamlines(&f, &|u: f64, v: f64| (u * u + v * v).sqrt(), &p)
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    /// Reference numbers printed by `tools/vectorfield_check.jl` (CairoMakie 0.15 / Makie 0.24).
    #[test]
    fn matches_makie_streamplot_impl() {
        let pi = std::f64::consts::PI;
        let s = run(|t, w| (w, -t.sin() - 0.2 * w), [-pi, pi], [-3.0, 3.0], 32, 1.0);
        assert_eq!((s.arrow_pos.len(), s.line_pts.len()), (177, 17630));
        let seeds = [[-0.098175, -0.09375], [-1.472622, -2.53125], [-3.043418, 0.84375]];
        for (p, q) in s.arrow_pos.iter().zip(seeds) {
            assert!(close(p[0], q[0]) && close(p[1], q[1]), "{p:?} vs {q:?}");
        }
        let (lo, hi) = crate::data::finite_extrema(s.line_val.iter().copied()).unwrap();
        assert!(close(lo, 0.036824) && close(hi, 3.370254), "{lo} {hi}");
        // Every half-line starts with a NaN break, then its seed.
        assert!(s.line_pts[0][0].is_nan() && s.line_pts[1] == s.arrow_pos[0]);

        let s = run(|x, y| (y, (1.0 - x * x) * y - x), [-3.0, 3.0], [-4.0, 4.0], 24, 0.8);
        assert_eq!((s.arrow_pos.len(), s.line_pts.len()), (48, 11347));
        let (lo, hi) = crate::data::finite_extrema(s.line_val.iter().copied()).unwrap();
        assert!(close(lo, 0.026134) && close(hi, 29.638844), "{lo} {hi}");
    }

    #[test]
    fn degenerate_fields_terminate() {
        // A zero field: every seed is a fixed point (NaN direction), lines stop at once.
        let s = run(|_, _| (0.0, 0.0), [0.0, 1.0], [0.0, 1.0], 8, 1.0);
        assert_eq!(s.arrow_pos.len(), 64);
        assert!(s.arrow_dir.iter().all(|d| d[0].is_nan()));
        // NaN everywhere and an empty box.
        let s = run(|_, _| (f64::NAN, f64::NAN), [0.0, 0.0], [0.0, 0.0], 4, 1.0);
        assert_eq!(s.arrow_pos.len(), 16);
        assert_eq!(run(|x, y| (x, y), [0.0, 1.0], [0.0, 1.0], 4, 0.0).arrow_pos.len(), 0);
    }

    #[test]
    fn recomputes_only_when_inputs_change() {
        let sp = streamplot(|x, y| (-y, x), -1.0..=1.0, -1.0..=1.0);
        let lines = |sp: &StreamPlot| match &sp.sh.state.lock().plot(sp.id).unwrap().kind {
            PlotKind::StreamPlot(s) => s.lines(),
            _ => unreachable!(),
        };
        let a = lines(&sp);
        sp.colormap(Colormap::MAGMA).linewidth(3);
        assert!(Arc::ptr_eq(&a, &lines(&sp)), "style changes keep the streamlines");
        sp.density(0.5);
        let b = lines(&sp);
        assert!(!Arc::ptr_eq(&a, &b));
        sp.set_function(|x, y| (y, -x));
        assert!(!Arc::ptr_eq(&b, &lines(&sp)));
        assert_eq!(sp.seeds(), lines(&sp).arrow_pos.len());
    }
}
